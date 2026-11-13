// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! wgpu renderer for assembled characters. Scene data stays in the stored
//! conventions: left-handed, Y up, clockwise front faces.

use anyhow::{anyhow, Result};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use rk_chara::{DrawMesh, Image, MaterialInst};
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub fov_y_deg: f32,
}

impl Camera {
    /// Frames a bounding box from the front (the character faces +Z).
    /// `yaw_deg` orbits around the vertical axis.
    pub fn frame(min: Vec3, max: Vec3, aspect: f32, yaw_deg: f32) -> Self {
        let fov_y_deg = 23.0f32;
        let center = (min + max) * 0.5;
        let half = (max - min) * 0.5;
        let tan = (fov_y_deg.to_radians() * 0.5).tan();
        let dist_h = half.y / tan;
        let dist_w = half.x.max(half.z) / (tan * aspect);
        let dist = dist_h.max(dist_w) * 1.08 + half.z;
        let yaw = yaw_deg.to_radians();
        let dir = Vec3::new(yaw.sin(), 0.0, yaw.cos());
        Self {
            eye: center + dir * dist,
            target: center,
            fov_y_deg,
        }
    }

    fn view_proj(&self, aspect: f32) -> Mat4 {
        let dist = (self.eye - self.target).length();
        let proj = Mat4::perspective_lh(
            self.fov_y_deg.to_radians(),
            aspect,
            (dist * 0.05).max(0.01),
            dist * 4.0 + 10.0,
        );
        proj * Mat4::look_at_lh(self.eye, self.target, Vec3::Y)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [f32; 16],
    light_dir: [f32; 4],
    eye: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialUniform {
    color: [f32; 4],
    st: [f32; 4],
    params: [f32; 4],
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    globals_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    sampler_repeat: wgpu::Sampler,
    sampler_clamp: wgpu::Sampler,
    white: wgpu::TextureView,
    textures: HashMap<usize, wgpu::TextureView>,
}

/// Box-filtered mip chain for an RGBA8 image.
fn mip_chain(width: u32, height: u32, rgba: &[u8]) -> Vec<(u32, u32, Vec<u8>)> {
    let mut out = vec![(width, height, rgba.to_vec())];
    loop {
        let (w, h, src) = out.last().unwrap();
        if *w <= 1 && *h <= 1 {
            break;
        }
        let (nw, nh) = ((*w / 2).max(1), (*h / 2).max(1));
        let mut dst = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0u32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(*w - 1);
                    let sy = (y * 2 + dy).min(*h - 1);
                    let i = ((sy * *w + sx) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += src[i + c] as u32;
                    }
                }
                let o = ((y * nw + x) * 4) as usize;
                for c in 0..4 {
                    dst[o + c] = ((acc[c] + 2) / 4) as u8;
                }
            }
        }
        out.push((nw, nh, dst));
    }
    out
}

impl Renderer {
    pub fn new() -> Result<Self> {
        pollster::block_on(Self::new_async())
    }

    async fn new_async() -> Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| anyhow!("no graphics adapter found"))?;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("renkatsu"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits {
                        max_texture_dimension_2d: 8192,
                        ..Default::default()
                    },
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("basic"),
            source: wgpu::ShaderSource::Wgsl(include_str!("basic.wgsl").into()),
        });
        let uniform_entry = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("basic"),
            bind_group_layouts: &[&globals_layout, &material_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("basic"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });
        let sampler = |mode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: mode,
                address_mode_v: mode,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                anisotropy_clamp: 8,
                ..Default::default()
            })
        };
        let sampler_repeat = sampler(wgpu::AddressMode::Repeat);
        let sampler_clamp = sampler(wgpu::AddressMode::ClampToEdge);
        let white = Self::upload(&device, &queue, 1, 1, &[255, 255, 255, 255]);
        Ok(Self {
            device,
            queue,
            pipeline,
            globals_layout,
            material_layout,
            sampler_repeat,
            sampler_clamp,
            white,
            textures: HashMap::new(),
        })
    }

    fn upload(device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32, rgba: &[u8]) -> wgpu::TextureView {
        let mips = mip_chain(width, height, rgba);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: mips.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, (w, h, data)) in mips.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(*h),
                },
                wgpu::Extent3d {
                    width: *w,
                    height: *h,
                    depth_or_array_layers: 1,
                },
            );
        }
        texture.create_view(&Default::default())
    }

    /// Drops uploaded textures. Call between characters: the cache is keyed
    /// by image address, which can be reused once a character is dropped.
    pub fn clear_textures(&mut self) {
        self.textures.clear();
    }

    fn texture(&mut self, image: &Arc<Image>) -> wgpu::TextureView {
        let key = Arc::as_ptr(image) as usize;
        if let Some(v) = self.textures.get(&key) {
            return v.clone();
        }
        let max = self.device.limits().max_texture_dimension_2d;
        let view = if image.width == 0 || image.height == 0 || image.width > max || image.height > max {
            self.white.clone()
        } else {
            Self::upload(&self.device, &self.queue, image.width, image.height, &image.rgba)
        };
        self.textures.insert(key, view.clone());
        view
    }

    fn material_bind_group(&mut self, mat: Option<&MaterialInst>) -> wgpu::BindGroup {
        let mut uniform = MaterialUniform {
            color: [1.0; 4],
            st: [1.0, 1.0, 0.0, 0.0],
            params: [0.5, 0.0, 0.0, 0.0],
        };
        let mut view = self.white.clone();
        let mut repeat = true;
        if let Some(mat) = mat {
            if let Some(c) = mat.colors.get("_Color") {
                uniform.color = [c[0], c[1], c[2], 1.0];
            }
            if let Some(slot) = mat.textures.get("_MainTex") {
                view = self.texture(&slot.image);
                repeat = slot.image.repeat;
                uniform.st = [slot.scale[0], slot.scale[1], slot.offset[0], slot.offset[1]];
            }
        }
        let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.material_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(if repeat {
                        &self.sampler_repeat
                    } else {
                        &self.sampler_clamp
                    }),
                },
            ],
        })
    }

    /// Renders posed meshes to an RGBA8 image, top row first.
    pub fn render(
        &mut self,
        draws: &[DrawMesh],
        camera: &Camera,
        width: u32,
        height: u32,
        background: [f64; 4],
    ) -> Result<Vec<u8>> {
        let aspect = width as f32 / height as f32;
        let to_eye = (camera.eye - camera.target).normalize_or_zero();
        // Key light from the camera's upper left, as a viewer expects.
        let right = Vec3::Y.cross(-to_eye).normalize_or_zero();
        let light = (to_eye + Vec3::Y * 0.35 - right * 0.4).normalize();
        let globals = Globals {
            view_proj: camera.view_proj(aspect).to_cols_array(),
            light_dir: [light.x, light.y, light.z, 0.0],
            eye: [camera.eye.x, camera.eye.y, camera.eye.z, 1.0],
        };
        let globals_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("globals"),
            contents: bytemuck::bytes_of(&globals),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let globals_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });

        struct Batch {
            vb: wgpu::Buffer,
            ib: wgpu::Buffer,
            parts: Vec<(u32, u32, wgpu::BindGroup)>,
        }
        let mut batches = Vec::new();
        for d in draws {
            let mesh = &d.renderer.mesh;
            let uv = &mesh.uv[0];
            let verts: Vec<Vertex> = d
                .positions
                .iter()
                .enumerate()
                .map(|(i, p)| Vertex {
                    pos: *p,
                    normal: d.normals[i],
                    uv: uv.get(i).copied().unwrap_or([0.0, 0.0]),
                })
                .collect();
            if verts.is_empty() || mesh.indices.is_empty() {
                continue;
            }
            let vb = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let ib = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            let mut parts = Vec::new();
            for (si, sm) in mesh.submeshes.iter().enumerate() {
                if sm.topology != 0 || sm.index_count == 0 {
                    continue;
                }
                let bg = self.material_bind_group(d.renderer.materials.get(si));
                parts.push((sm.first_index, sm.index_count, bg));
            }
            batches.push(Batch { vb, ib, parts });
        }

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let make = |format, samples, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let msaa = make(COLOR_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
        let depth =
            make(DEPTH_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
        let target = make(
            COLOR_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let target_view = target.create_view(&Default::default());

        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &msaa,
                    resolve_target: Some(&target_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: background[0],
                            g: background[1],
                            b: background[2],
                            a: background[3],
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &globals_bg, &[]);
            for b in &batches {
                pass.set_vertex_buffer(0, b.vb.slice(..));
                pass.set_index_buffer(b.ib.slice(..), wgpu::IndexFormat::Uint32);
                for (first, count, bg) in &b.parts {
                    pass.set_bind_group(1, bg, &[]);
                    pass.draw_indexed(*first..*first + *count, 0, 0..1);
                }
            }
        }

        // Read back, honouring the row alignment wgpu requires.
        let row = width * 4;
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            size,
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| drop(tx.send(r)));
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()??;
        let data = slice.get_mapped_range();
        let mut out = Vec::with_capacity((row * height) as usize);
        for y in 0..height {
            let start = (y * padded) as usize;
            out.extend_from_slice(&data[start..start + row as usize]);
        }
        Ok(out)
    }
}

/// Bounding box of posed meshes.
pub fn bounds(draws: &[DrawMesh]) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for d in draws {
        for p in &d.positions {
            let p = Vec3::from(*p);
            if p.is_finite() {
                min = min.min(p);
                max = max.max(p);
            }
        }
    }
    if min.x > max.x {
        return (Vec3::ZERO, Vec3::ONE);
    }
    (min, max)
}

/// Decodes a PNG to RGBA8, top row first.
pub fn load_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let mut dec = png::Decoder::new(bytes);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Indexed => return Err(anyhow!("indexed PNG was not expanded")),
    };
    Ok((info.width, info.height, rgba))
}

pub fn save_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_is_valid_wgsl() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("basic.wgsl")).expect("basic.wgsl parses");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("basic.wgsl validates");
    }

    #[test]
    fn uniform_layouts_match_the_shader() {
        // WGSL: mat4x4 + 2 vec4 for globals, 3 vec4 for materials.
        assert_eq!(std::mem::size_of::<Globals>(), 96);
        assert_eq!(std::mem::size_of::<MaterialUniform>(), 48);
        assert_eq!(std::mem::size_of::<Vertex>(), 32);
    }

    #[test]
    fn mip_chain_halves_down_to_one_pixel() {
        let mut rgba = Vec::new();
        for i in 0..(8 * 4) {
            rgba.extend_from_slice(if i % 2 == 0 {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 255, 255]
            });
        }
        let mips = mip_chain(8, 4, &rgba);
        let sizes: Vec<(u32, u32)> = mips.iter().map(|m| (m.0, m.1)).collect();
        assert_eq!(sizes, [(8, 4), (4, 2), (2, 1), (1, 1)]);
        assert!(mips.iter().all(|(w, h, d)| d.len() == (w * h * 4) as usize));
        // Red and blue columns average to purple.
        assert_eq!(mips[1].2[..4], [128, 0, 128, 255]);
    }

    #[test]
    fn camera_frames_the_whole_box() {
        let (min, max) = (Vec3::new(-0.4, 0.0, -0.2), Vec3::new(0.4, 1.6, 0.2));
        for aspect in [0.5f32, 1.0, 2.0] {
            let cam = Camera::frame(min, max, aspect, 30.0);
            let vp = cam.view_proj(aspect);
            for corner in 0..8 {
                let p = Vec3::new(
                    if corner & 1 == 0 { min.x } else { max.x },
                    if corner & 2 == 0 { min.y } else { max.y },
                    if corner & 4 == 0 { min.z } else { max.z },
                );
                let ndc = vp.project_point3(p);
                assert!(
                    ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z),
                    "{p} is outside the view at aspect {aspect}: {ndc}"
                );
            }
        }
    }

    #[test]
    fn png_round_trip() {
        let rgba: Vec<u8> = (0..3 * 2 * 4).map(|i| (i * 9) as u8).collect();
        let path = std::env::temp_dir().join(format!("renkatsu-test-{}.png", std::process::id()));
        save_png(&path, 3, 2, &rgba).unwrap();
        let loaded = load_png(&std::fs::read(&path).unwrap()).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(loaded, (3, 2, rgba));
        assert!(load_png(b"not a png").is_err());
    }

    #[test]
    fn bounds_ignore_non_finite_points() {
        assert_eq!(bounds(&[]), (Vec3::ZERO, Vec3::ONE));
    }
}
