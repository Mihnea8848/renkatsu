// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! wgpu renderer for assembled characters. Scene data stays in the stored
//! conventions: left-handed, Y up, clockwise front faces.

pub mod material;
pub mod shadow;

use anyhow::{anyhow, Result};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use material::{MaterialDesc, MaterialUniform};
use rk_chara::{DrawMesh, Image, ShadowCasting, Stencil};
use shadow::{ShadowSettings, CASCADES};
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// The game renders in linear colour space: shaders write linear values,
/// blending happens on them, and the target stores them as sRGB.
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Submesh and material index pairs a renderer draws: one per submesh, and
/// each extra material on the last submesh.
fn submesh_materials(submeshes: usize, materials: usize) -> Vec<(usize, usize)> {
    if submeshes == 0 {
        return Vec::new();
    }
    (0..submeshes.max(materials))
        .map(|i| (i.min(submeshes - 1), i))
        .collect()
}

/// sRGB to linear for one channel, 0 to 1.
fn srgb_to_linear(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
const SAMPLES: u32 = 4;
/// Unity 5.6 keeps shadow maps in 16 bits.
const SHADOW_MAP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth16Unorm;
/// The camera's depth texture, which the shadow mask is built from.
const DEPTH_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

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

    /// Unit vector the camera looks along.
    fn forward(&self) -> Vec3 {
        (self.target - self.eye).try_normalize().unwrap_or(Vec3::Z)
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
    uv0: [f32; 2],
    uv1: [f32; 2],
    uv2: [f32; 2],
    uv3: [f32; 2],
    color: [f32; 4],
    tangent: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [f32; 16],
    light_dir: [f32; 4],
    eye: [f32; 4],
    ambient_shadow: [f32; 4],
    line_color: [f32; 4],
    params: [f32; 4],
    shadow: [f32; 4],
}

/// What the shadow mask pass reads; `Shadow` in `shadow.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShadowUniform {
    inv_view_proj: [f32; 16],
    world_to_shadow: [[f32; 16]; CASCADES],
    splits_near: [f32; 4],
    splits_far: [f32; 4],
    eye: [f32; 4],
    forward: [f32; 4],
    params: [f32; 4],
}

/// Scene-wide shading settings. The defaults are the game's own.
#[derive(Debug, Clone)]
pub struct Environment {
    /// Light amount to brightness. `None` gives a hard light/shadow edge.
    pub ramp: Option<Arc<Image>>,
    /// Colour of shadows across the scene.
    pub shadow_color: [f32; 3],
    /// How dark shadows get, 0 to 1.
    pub shadow_depth: f32,
    /// How dark outlines and drawn lines get, 0 to 1.
    pub line_depth: f32,
    pub line_width: f32,
    pub light_intensity: f32,
    pub ambient: f32,
    /// Unit vector pointing from the scene to the light. `None` puts the
    /// light at the camera's upper left.
    pub light_dir: Option<[f32; 3]>,
    /// Real-time shadows of the light. Switch `enabled` off for none.
    pub shadows: ShadowSettings,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            ramp: None,
            shadow_color: [0.5, 0.5, 0.5],
            shadow_depth: 0.26,
            line_depth: 1.0,
            line_width: 0.307,
            light_intensity: 1.0,
            // The game's shaders compute light * 0.6 + 0.4.
            ambient: 0.4,
            light_dir: None,
            shadows: ShadowSettings::default(),
        }
    }
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    /// One pipeline per combination of draw settings in use.
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    /// Shadow caster pipelines, by target and culling.
    caster_pipelines: HashMap<CasterKey, wgpu::RenderPipeline>,
    mask_pipeline: wgpu::RenderPipeline,
    mask_layout: wgpu::BindGroupLayout,
    sampler_shadow: wgpu::Sampler,
    globals_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    sampler_repeat: wgpu::Sampler,
    sampler_clamp: wgpu::Sampler,
    white: wgpu::TextureView,
    hard_ramp: wgpu::TextureView,
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

fn texture_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

/// The draw settings a pipeline is built for.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct PipelineKey {
    /// The outline pass: back faces pushed outward.
    outline: bool,
    blend: bool,
    zwrite: bool,
    stencil: Stencil,
}

impl PipelineKey {
    fn surface(desc: &MaterialDesc) -> Self {
        Self {
            outline: false,
            blend: desc.blended,
            zwrite: desc.zwrite,
            stencil: desc.stencil,
        }
    }

    /// Outlines are solid and write depth. They respect marked pixels the
    /// way their surface does, and never mark any themselves.
    fn outline(desc: &MaterialDesc) -> Self {
        Self {
            outline: true,
            blend: false,
            zwrite: true,
            stencil: match desc.stencil {
                Stencil::SkipEqual => Stencil::SkipEqual,
                _ => Stencil::None,
            },
        }
    }
}

/// The settings a shadow caster pipeline is built for.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct CasterKey {
    /// The camera's depth texture instead of the shadow map.
    depth_texture: bool,
    cull_back: bool,
}

/// One thing to draw, in order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Step {
    item: usize,
    outline: bool,
}

/// Draw order after the game: surfaces up to the last solid queue, then
/// their outlines, then everything later in queue order, each with its
/// outline first. `items` holds (queue, has outline) and is sorted by queue.
fn draw_order(items: &[(i32, bool)]) -> Vec<Step> {
    let solid = |i: &usize| items[*i].0 <= shadow::LAST_SOLID_QUEUE;
    let mut steps = Vec::new();
    let all = 0..items.len();
    steps.extend(all.clone().filter(solid).map(|item| Step { item, outline: false }));
    steps.extend(
        all.clone()
            .filter(solid)
            .filter(|i| items[*i].1)
            .map(|item| Step { item, outline: true }),
    );
    for item in all.filter(|i| !solid(i)) {
        if items[item].1 {
            steps.push(Step { item, outline: true });
        }
        steps.push(Step { item, outline: false });
    }
    steps
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
            label: Some("toon"),
            source: wgpu::ShaderSource::Wgsl(include_str!("toon.wgsl").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                uniform_entry(0),
                texture_entry(1, wgpu::ShaderStages::FRAGMENT),
                sampler_entry(2),
                texture_entry(3, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let depth_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let mask_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow mask"),
            entries: &[
                uniform_entry(0),
                depth_entry(1),
                depth_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let mask_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadow mask"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shadow.wgsl").into()),
        });
        let mask_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow mask"),
            bind_group_layouts: &[&mask_layout],
            push_constant_ranges: &[],
        });
        let mask_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow mask"),
            layout: Some(&mask_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &mask_shader,
                entry_point: Some("vs_mask"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &mask_shader,
                entry_point: Some("fs_mask"),
                targets: &[Some(MASK_FORMAT.into())],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        // Hardware comparison with bilinear blending, as the game's shadow
        // map sampler does. A sample is lit when it is no deeper than the map.
        let sampler_shadow = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let mut material_entries = vec![uniform_entry(0)];
        for i in 0..material::SLOTS.len() as u32 {
            material_entries.push(texture_entry(1 + i, wgpu::ShaderStages::VERTEX_FRAGMENT));
        }
        material_entries.push(sampler_entry(1 + material::SLOTS.len() as u32));
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &material_entries,
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("toon"),
            bind_group_layouts: &[&globals_layout, &material_layout],
            push_constant_ranges: &[],
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
        let white = Self::upload(&device, &queue, 1, 1, &[255, 255, 255, 255], false);
        // Fallback ramp: dark below half light, bright above.
        let hard: Vec<u8> = (0..16)
            .flat_map(|i| if i < 8 { [0u8, 0, 0, 255] } else { [255, 255, 255, 255] })
            .collect();
        let hard_ramp = Self::upload(&device, &queue, 16, 1, &hard, false);
        Ok(Self {
            device,
            queue,
            shader,
            layout,
            pipelines: HashMap::new(),
            caster_pipelines: HashMap::new(),
            mask_pipeline,
            mask_layout,
            sampler_shadow,
            globals_layout,
            material_layout,
            sampler_repeat,
            sampler_clamp,
            white,
            hard_ramp,
            textures: HashMap::new(),
        })
    }

    fn make_pipeline(&self, key: PipelineKey) -> wgpu::RenderPipeline {
        let (vs, fs) = if key.outline {
            ("vs_outline", "fs_outline")
        } else {
            ("vs_main", "fs_main")
        };
        let face = match key.stencil {
            Stencil::None => wgpu::StencilFaceState::IGNORE,
            Stencil::Write => wgpu::StencilFaceState {
                compare: wgpu::CompareFunction::Always,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op: wgpu::StencilOperation::Replace,
            },
            Stencil::SkipEqual => wgpu::StencilFaceState {
                compare: wgpu::CompareFunction::NotEqual,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op: wgpu::StencilOperation::Keep,
            },
        };
        self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(fs),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some(vs),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x2, 4 => Float32x2, 5 => Float32x2, 6 => Float32x4, 7 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some(fs),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: key.blend.then_some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Cw,
                // The outline is the mesh's back faces, pushed outward.
                cull_mode: key.outline.then_some(wgpu::Face::Front),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: key.zwrite,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState {
                    front: face,
                    back: face,
                    read_mask: 0xff,
                    write_mask: 0xff,
                },
                // Layers that do not write depth lie on the surface they
                // decorate and must win against it.
                bias: if key.blend && !key.zwrite {
                    wgpu::DepthBiasState {
                        constant: -8,
                        slope_scale: -1.0,
                        clamp: 0.0,
                    }
                } else {
                    Default::default()
                },
            }),
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        })
    }

    /// Pipeline of the shaders' shadow caster pass: depth only.
    fn make_caster_pipeline(&self, key: CasterKey) -> wgpu::RenderPipeline {
        self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow caster"),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_caster"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x2, 4 => Float32x2, 5 => Float32x2, 6 => Float32x4, 7 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_caster"),
                targets: &[],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: key.cull_back.then_some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: if key.depth_texture {
                    DEPTH_TEXTURE_FORMAT
                } else {
                    SHADOW_MAP_FORMAT
                },
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: Default::default(),
                // The passes state `Offset 1, 1`.
                bias: wgpu::DepthBiasState {
                    constant: 1,
                    slope_scale: 1.0,
                    clamp: 0.0,
                },
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        })
    }

    /// Uploads an image. sRGB images get an sRGB format, so sampling returns
    /// linear values as Unity does in linear colour space.
    fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        rgba: &[u8],
        srgb: bool,
    ) -> wgpu::TextureView {
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
            format: if srgb {
                wgpu::TextureFormat::Rgba8UnormSrgb
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            },
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
            Self::upload(
                &self.device,
                &self.queue,
                image.width,
                image.height,
                &image.rgba,
                image.srgb,
            )
        };
        self.textures.insert(key, view.clone());
        view
    }

    fn material_bind_group(&mut self, desc: &MaterialDesc) -> wgpu::BindGroup {
        let views: Vec<wgpu::TextureView> = desc
            .textures
            .iter()
            .map(|t| {
                t.as_ref()
                    .map(|i| self.texture(i))
                    .unwrap_or_else(|| self.white.clone())
            })
            .collect();
        let repeat = desc.textures[0].as_ref().is_none_or(|i| i.repeat);
        let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of::<MaterialUniform>(&desc.uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }];
        for (i, v) in views.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: 1 + i as u32,
                resource: wgpu::BindingResource::TextureView(v),
            });
        }
        entries.push(wgpu::BindGroupEntry {
            binding: 1 + views.len() as u32,
            resource: wgpu::BindingResource::Sampler(if repeat {
                &self.sampler_repeat
            } else {
                &self.sampler_clamp
            }),
        });
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.material_layout,
            entries: &entries,
        })
    }

    /// Renders posed meshes to an RGBA8 image, top row first.
    pub fn render(
        &mut self,
        draws: &[DrawMesh],
        camera: &Camera,
        env: &Environment,
        width: u32,
        height: u32,
        background: [f64; 4],
    ) -> Result<Vec<u8>> {
        let aspect = width as f32 / height as f32;
        let to_eye = (camera.eye - camera.target).normalize_or_zero();
        // Key light from the camera's upper left, as a viewer expects.
        let right = Vec3::Y.cross(-to_eye).normalize_or_zero();
        let light = match env.light_dir {
            Some(d) => Vec3::from(d).normalize_or_zero(),
            None => (to_eye + Vec3::Y * 0.35 - right * 0.4).normalize(),
        };
        let cast = env.shadows.enabled && light.length_squared() > 0.0;
        let base = Globals {
            view_proj: camera.view_proj(aspect).to_cols_array(),
            light_dir: [light.x, light.y, light.z, 0.0],
            eye: [camera.eye.x, camera.eye.y, camera.eye.z, 1.0],
            // Both are set with SetGlobalColor, which Unity linearises.
            ambient_shadow: material::linear([
                env.shadow_color[0],
                env.shadow_color[1],
                env.shadow_color[2],
                1.0 - env.shadow_depth,
            ]),
            line_color: material::linear([0.5, 0.5, 0.5, 1.0 - env.line_depth]),
            params: [env.line_width, env.light_intensity, env.ambient, 0.0],
            shadow: [0.0; 4],
        };
        let ramp = match &env.ramp {
            Some(image) => self.texture(image),
            None => self.hard_ramp.clone(),
        };
        // One set of globals per view the meshes are drawn from.
        let globals_group = |r: &Self, globals: Globals, mask: &wgpu::TextureView| {
            let buf = r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("globals"),
                contents: bytemuck::bytes_of(&globals),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &r.globals_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&ramp),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&r.sampler_clamp),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(mask),
                    },
                ],
            })
        };

        struct Batch {
            vb: wgpu::Buffer,
            ib: wgpu::Buffer,
        }
        /// A submesh drawn with its shader's shadow caster pass.
        struct Caster {
            batch: usize,
            first: u32,
            count: u32,
            bind: usize,
            role: shadow::Role,
        }
        struct Item {
            batch: usize,
            first: u32,
            count: u32,
            queue: i32,
            outline: bool,
            surface_key: PipelineKey,
            outline_key: PipelineKey,
            stencil_ref: u32,
            bind: usize,
        }
        let mut batches = Vec::new();
        let mut items = Vec::new();
        let mut casters = Vec::new();
        let mut binds = Vec::new();
        for d in draws {
            let mesh = &d.renderer.mesh;
            let uv = |set: usize, i: usize| mesh.uv[set].get(i).copied().unwrap_or([0.0, 0.0]);
            let verts: Vec<Vertex> = d
                .positions
                .iter()
                .enumerate()
                .map(|(i, p)| Vertex {
                    pos: *p,
                    normal: d.normals[i],
                    uv0: uv(0, i),
                    uv1: uv(1, i),
                    uv2: uv(2, i),
                    uv3: uv(3, i),
                    color: mesh.colors.get(i).copied().unwrap_or([1.0; 4]),
                    tangent: d.tangents[i],
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
            // Unity draws materials beyond the submesh count with the last
            // submesh again; MaterialEditor's copies rely on this.
            let pairs = submesh_materials(mesh.submeshes.len(), d.renderer.materials.len());
            for (si, mi) in pairs {
                let sm = &mesh.submeshes[si];
                if sm.topology != 0 || sm.index_count == 0 {
                    continue;
                }
                let mut desc = MaterialDesc::new(d.renderer.materials.get(mi));
                desc.uniform.info[3] = shadow::receives(d.renderer.receive_shadows, desc.queue) as u32;
                let bind = binds.len();
                binds.push(self.material_bind_group(&desc));
                let role = shadow::role(d.renderer.cast_shadows, desc.shadow, desc.queue);
                if cast && (role.shadow_map.is_some() || role.depth.is_some()) {
                    casters.push(Caster {
                        batch: batches.len(),
                        first: sm.first_index,
                        count: sm.index_count,
                        bind,
                        role,
                    });
                }
                // Only its shadow is seen.
                if d.renderer.cast_shadows == ShadowCasting::ShadowsOnly {
                    continue;
                }
                items.push(Item {
                    batch: batches.len(),
                    first: sm.first_index,
                    count: sm.index_count,
                    queue: desc.queue,
                    outline: desc.outline,
                    surface_key: PipelineKey::surface(&desc),
                    outline_key: PipelineKey::outline(&desc),
                    stencil_ref: desc.stencil_ref as u32,
                    bind,
                });
            }
            batches.push(Batch { vb, ib });
        }
        // Unity order: by render queue, ties in submission order.
        items.sort_by_key(|i| i.queue);
        let order: Vec<(i32, bool)> = items.iter().map(|i| (i.queue, i.outline)).collect();
        let steps = draw_order(&order);
        for step in &steps {
            let item = &items[step.item];
            let key = if step.outline {
                item.outline_key
            } else {
                item.surface_key
            };
            if !self.pipelines.contains_key(&key) {
                let pipeline = self.make_pipeline(key);
                self.pipelines.insert(key, pipeline);
            }
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
        let mask = if cast {
            let draw = |pass: &mut wgpu::RenderPass, r: &Self, depth_texture: bool| {
                for c in &casters {
                    let role = if depth_texture { c.role.depth } else { c.role.shadow_map };
                    let Some(cull_back) = role else { continue };
                    let key = CasterKey {
                        depth_texture,
                        cull_back,
                    };
                    pass.set_pipeline(&r.caster_pipelines[&key]);
                    let b: &Batch = &batches[c.batch];
                    pass.set_vertex_buffer(0, b.vb.slice(..));
                    pass.set_index_buffer(b.ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.set_bind_group(1, &binds[c.bind], &[]);
                    pass.draw_indexed(c.first..c.first + c.count, 0, 0..1);
                }
            };
            for depth_texture in [false, true] {
                for cull_back in [false, true] {
                    let key = CasterKey {
                        depth_texture,
                        cull_back,
                    };
                    if !self.caster_pipelines.contains_key(&key) {
                        let pipeline = self.make_caster_pipeline(key);
                        self.caster_pipelines.insert(key, pipeline);
                    }
                }
            }
            fn depth_pass(view: &wgpu::TextureView) -> wgpu::RenderPassDescriptor<'_> {
                wgpu::RenderPassDescriptor {
                    label: Some("shadow caster"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                }
            }
            let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;

            // The light's shadow map: four cascades side by side.
            let set = &env.shadows;
            let side = shadow::atlas_size(
                width,
                height,
                set.max_resolution.min(self.device.limits().max_texture_dimension_2d),
            );
            let half = side / 2;
            let view = shadow::View {
                eye: camera.eye,
                forward: camera.forward(),
                fov_y_deg: camera.fov_y_deg,
                aspect,
            };
            let ranges = shadow::cascade_ranges(set.camera_near, set.distance, set.cascade_splits);
            let light_view = shadow::light_view(light);
            let cascades = ranges.map(|r| shadow::fit_cascade(&view, r, light_view, set.near_plane_offset, half));
            let map = self
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("shadow map"),
                    size: wgpu::Extent3d {
                        width: side,
                        height: side,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: SHADOW_MAP_FORMAT,
                    usage: attachment,
                    view_formats: &[],
                })
                .create_view(&Default::default());
            let groups: Vec<wgpu::BindGroup> = cascades
                .iter()
                .map(|c| {
                    let globals = Globals {
                        view_proj: c.view_proj.to_cols_array(),
                        shadow: [
                            shadow::caster_bias(set.bias, c.depth_range),
                            1.0,
                            0.0,
                            shadow::caster_normal_bias(set.normal_bias, c.texel),
                        ],
                        ..base
                    };
                    globals_group(self, globals, &self.white)
                })
                .collect();
            {
                let mut pass = encoder.begin_render_pass(&depth_pass(&map));
                for (i, group) in groups.iter().enumerate() {
                    let [ox, oy] = shadow::atlas_origin(i);
                    let h = half as f32;
                    pass.set_viewport(ox * side as f32, oy * side as f32, h, h, 0.0, 1.0);
                    pass.set_bind_group(0, group, &[]);
                    draw(&mut pass, self, false);
                }
            }

            // The camera's depth texture: the same pass, seen from the camera
            // and without the light's bias.
            let depth_texture = make(DEPTH_TEXTURE_FORMAT, 1, attachment).create_view(&Default::default());
            {
                let group = globals_group(self, base, &self.white);
                let mut pass = encoder.begin_render_pass(&depth_pass(&depth_texture));
                pass.set_bind_group(0, &group, &[]);
                draw(&mut pass, self, true);
            }

            // Collect the shadows into the screen-sized mask.
            let view_proj = camera.view_proj(aspect);
            let uniform = ShadowUniform {
                inv_view_proj: view_proj.inverse().to_cols_array(),
                world_to_shadow: std::array::from_fn(|i| {
                    shadow::world_to_shadow(cascades[i].view_proj, i).to_cols_array()
                }),
                splits_near: ranges.map(|r| r.0),
                splits_far: ranges.map(|r| r.1),
                eye: [camera.eye.x, camera.eye.y, camera.eye.z, 1.0],
                forward: view.forward.extend(0.0).to_array(),
                params: [shadow::shadow_floor(set.strength), side as f32, 1.0 / side as f32, 0.0],
            };
            let uniform_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("shadow mask"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.mask_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&depth_texture),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&map),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.sampler_shadow),
                    },
                ],
            });
            let mask = make(MASK_FORMAT, 1, attachment).create_view(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shadow mask"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &mask,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.mask_pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.draw(0..3, 0..1);
            }
            Some(mask)
        } else {
            None
        };
        let globals_bg = globals_group(
            self,
            Globals {
                shadow: [0.0, 0.0, if mask.is_some() { 1.0 } else { 0.0 }, 0.0],
                ..base
            },
            mask.as_ref().unwrap_or(&self.white),
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &msaa,
                    resolve_target: Some(&target_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: srgb_to_linear(background[0]),
                            g: srgb_to_linear(background[1]),
                            b: srgb_to_linear(background[2]),
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
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Discard,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &globals_bg, &[]);
            let mut current = None;
            for step in &steps {
                let item = &items[step.item];
                let key = if step.outline {
                    item.outline_key
                } else {
                    item.surface_key
                };
                if current != Some(key) {
                    pass.set_pipeline(&self.pipelines[&key]);
                    current = Some(key);
                }
                pass.set_stencil_reference(item.stencil_ref);
                let b = &batches[item.batch];
                pass.set_vertex_buffer(0, b.vb.slice(..));
                pass.set_index_buffer(b.ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.set_bind_group(1, &binds[item.bind], &[]);
                pass.draw_indexed(item.first..item.first + item.count, 0, 0..1);
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
        // A mesh that only casts shadows is not part of the picture.
        if d.renderer.cast_shadows == ShadowCasting::ShadowsOnly {
            continue;
        }
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
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("toon.wgsl")).expect("toon.wgsl parses");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("toon.wgsl validates");
    }

    #[test]
    fn shadow_shader_is_valid_wgsl() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("shadow.wgsl")).expect("shadow.wgsl parses");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("shadow.wgsl validates");
    }

    #[test]
    fn extra_materials_draw_the_last_submesh() {
        assert_eq!(submesh_materials(2, 1), vec![(0, 0), (1, 1)]);
        assert_eq!(submesh_materials(1, 2), vec![(0, 0), (0, 1)]);
        assert_eq!(submesh_materials(2, 4), vec![(0, 0), (1, 1), (1, 2), (1, 3)]);
        assert!(submesh_materials(0, 3).is_empty());
    }

    #[test]
    fn background_is_cleared_in_linear() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-12);
        assert!((srgb_to_linear(0.5) - 0.2140411).abs() < 1e-6);
        assert!((srgb_to_linear(0.04) - 0.04 / 12.92).abs() < 1e-12);
    }

    #[test]
    fn draw_order_follows_the_queues() {
        let step = |item, outline| Step { item, outline };
        // Skin, cloth with outline, eye layer, fringe with outline.
        let items = [(2350, true), (2450, true), (3000, false), (3040, true)];
        assert_eq!(
            draw_order(&items),
            vec![
                // Solid surfaces first, then their outlines.
                step(0, false),
                step(1, false),
                step(0, true),
                step(1, true),
                // Later queues one at a time, outline before surface.
                step(2, false),
                step(3, true),
                step(3, false),
            ]
        );
        assert!(draw_order(&[]).is_empty());
    }

    #[test]
    fn uniform_layouts_match_the_shader() {
        // WGSL: mat4x4 and five vec4 for globals.
        assert_eq!(std::mem::size_of::<Globals>(), 160);
        // WGSL: five mat4x4 and five vec4 for the shadow mask.
        assert_eq!(std::mem::size_of::<ShadowUniform>(), 400);
        assert_eq!(std::mem::size_of::<Vertex>(), 88);
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
