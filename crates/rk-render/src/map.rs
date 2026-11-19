// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Drawing maps: many static objects, each a mesh already in world space
//! with a material per submesh. This first path lights them plainly (the
//! sun, the scene's ambient light and fog); maps get their own physically
//! based shading later. Characters keep their own path in `lib.rs`.

use crate::{Camera, Renderer, COLOR_FORMAT};
use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use rk_chara::scene::Image;
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;

/// How a material's surface is drawn, as Unity's Standard shader's
/// rendering modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    Opaque,
    /// Alpha below the threshold is cut away; depth is written.
    Cutout,
    /// Blended by alpha, drawn after everything opaque, back to front.
    Fade,
}

/// A map texture: the game's block-compressed data as it is stored, or
/// decoded colour for the formats that are not blocks.
#[derive(Debug)]
pub enum MapImage {
    /// BC1 (DXT1) or BC3 (DXT5) blocks, each mip level after the other,
    /// rows of blocks bottom-up as Unity stores them. The GPU decodes them
    /// as the game's does, so the picture is the game's exactly.
    Compressed {
        name: String,
        width: u32,
        height: u32,
        bc3: bool,
        levels: Vec<Vec<u8>>,
        srgb: bool,
        repeat: bool,
    },
    Decoded(Arc<Image>),
}

impl MapImage {
    pub fn repeat(&self) -> bool {
        match self {
            MapImage::Compressed { repeat, .. } => *repeat,
            MapImage::Decoded(i) => i.repeat,
        }
    }

    /// The bytes it takes on the GPU, mip levels included.
    pub fn gpu_bytes(&self) -> u64 {
        match self {
            MapImage::Compressed { levels, .. } => levels.iter().map(|l| l.len() as u64).sum(),
            // Mip levels add a third.
            MapImage::Decoded(i) => i.rgba.len() as u64 * 4 / 3,
        }
    }
}

/// A map material, as far as this path reads it.
#[derive(Debug)]
pub struct MapMaterial {
    /// Linear colour the texture is multiplied by.
    pub color: [f32; 4],
    pub albedo: Option<Arc<MapImage>>,
    /// Texture scale in xy, offset in zw.
    pub st: [f32; 4],
    /// Linear emitted colour, added after lighting.
    pub emission: [f32; 3],
    pub blend: Blend,
    pub cutoff: f32,
    pub two_sided: bool,
    /// Lit by nothing: the colour shows as it is.
    pub unlit: bool,
    /// False for a material another pass draws, or none does yet: its
    /// submesh is left out.
    pub drawn: bool,
    /// Unity's render queue: blended surfaces are drawn in its order first.
    pub queue: i32,
}

impl MapMaterial {
    /// A material whose submesh is not drawn.
    pub fn hidden() -> MapMaterial {
        MapMaterial {
            color: [0.0; 4],
            albedo: None,
            st: [1.0, 1.0, 0.0, 0.0],
            emission: [0.0; 3],
            blend: Blend::Opaque,
            cutoff: 0.5,
            two_sided: false,
            unlit: false,
            drawn: false,
            queue: 2000,
        }
    }
}

/// A mesh in world space.
#[derive(Debug, Default)]
pub struct MapMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// First index and count of each submesh.
    pub submeshes: Vec<(u32, u32)>,
    /// Lowest and highest corner, in the world.
    pub bounds: ([f32; 3], [f32; 3]),
}

impl MapMesh {
    /// The box around the positions.
    pub fn compute_bounds(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for p in positions {
            let p = Vec3::from(*p);
            if p.is_finite() {
                min = min.min(p);
                max = max.max(p);
            }
        }
        (min.to_array(), max.to_array())
    }
}

/// Whether a box lies at least partly inside the view of `view_proj`: it
/// is out when all its corners are behind one of the view's planes.
fn in_view(view_proj: &Mat4, (min, max): ([f32; 3], [f32; 3])) -> bool {
    let rows = [view_proj.row(0), view_proj.row(1), view_proj.row(2), view_proj.row(3)];
    // Left, right, bottom, top, near (depth 0 to 1), far.
    let planes = [
        rows[3] + rows[0],
        rows[3] - rows[0],
        rows[3] + rows[1],
        rows[3] - rows[1],
        rows[2],
        rows[3] - rows[2],
    ];
    planes.iter().all(|p| {
        // The corner furthest along the plane's normal.
        let corner = glam::Vec4::new(
            if p.x >= 0.0 { max[0] } else { min[0] },
            if p.y >= 0.0 { max[1] } else { min[1] },
            if p.z >= 0.0 { max[2] } else { min[2] },
            1.0,
        );
        p.dot(corner) >= 0.0
    })
}

/// An object of the map: its mesh, placed, and a material per submesh.
#[derive(Debug)]
pub struct MapObject {
    pub name: String,
    pub mesh: MapMesh,
    pub materials: Vec<Arc<MapMaterial>>,
}

/// Linear fog.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fog {
    pub color: [f32; 3],
    pub start: f32,
    pub end: f32,
}

/// What lights a map.
#[derive(Debug, Clone, PartialEq)]
pub struct MapLight {
    /// Unit vector towards the sun.
    pub sun_dir: [f32; 3],
    /// Linear colour times intensity.
    pub sun_color: [f32; 3],
    pub ambient: [f32; 3],
    pub fog: Option<Fog>,
    /// What shows where nothing is drawn.
    pub background: [f32; 3],
    /// A cutaway: nothing above this height is drawn. For looking into
    /// rooms from above.
    pub cut_above: Option<f32>,
}

/// A map ready to draw.
#[derive(Debug)]
pub struct MapScene {
    pub objects: Vec<MapObject>,
    pub light: MapLight,
}

impl MapScene {
    /// The lowest and highest corner of everything drawn.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for (lo, hi) in self
            .objects
            .iter()
            .filter(|o| !o.mesh.positions.is_empty())
            .map(|o| o.mesh.bounds)
        {
            min = min.min(Vec3::from(lo));
            max = max.max(Vec3::from(hi));
        }
        if min.x > max.x {
            (Vec3::ZERO, Vec3::ONE)
        } else {
            (min, max)
        }
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
    eye: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    ambient: [f32; 4],
    fog_color: [f32; 4],
    fog_range: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialUniform {
    color: [f32; 4],
    st: [f32; 4],
    emission: [f32; 4],
    params: [f32; 4],
}

/// The map pass's pipelines, made the first time a map is drawn.
pub(crate) struct Pipelines {
    globals: wgpu::BindGroupLayout,
    material: wgpu::BindGroupLayout,
    /// By blending and by whether back faces are culled.
    pipelines: HashMap<(bool, bool), wgpu::RenderPipeline>,
    repeat: wgpu::Sampler,
    clamp: wgpu::Sampler,
    /// Uploaded block-compressed textures, by the image's address.
    compressed: HashMap<usize, (Arc<MapImage>, wgpu::TextureView)>,
    /// How many objects the last frame left out as outside the view.
    culled: std::cell::Cell<usize>,
}

/// Uploads BC blocks with their mip levels.
fn upload_blocks(device: &wgpu::Device, queue: &wgpu::Queue, image: &MapImage) -> Option<wgpu::TextureView> {
    let MapImage::Compressed {
        name,
        width,
        height,
        bc3,
        levels,
        srgb,
        ..
    } = image
    else {
        return None;
    };
    let format = match (bc3, srgb) {
        (false, true) => wgpu::TextureFormat::Bc1RgbaUnormSrgb,
        (false, false) => wgpu::TextureFormat::Bc1RgbaUnorm,
        (true, true) => wgpu::TextureFormat::Bc3RgbaUnormSrgb,
        (true, false) => wgpu::TextureFormat::Bc3RgbaUnorm,
    };
    let block = if *bc3 { 16 } else { 8 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(name),
        size: wgpu::Extent3d {
            width: *width,
            height: *height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len().max(1) as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (i, data) in levels.iter().enumerate() {
        let (w, h) = ((width >> i).max(1), (height >> i).max(1));
        let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: i as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bw * block),
                rows_per_image: Some(bh),
            },
            // A level smaller than a block still takes a whole block.
            wgpu::Extent3d {
                width: bw * 4,
                height: bh * 4,
                depth_or_array_layers: 1,
            },
        );
    }
    Some(texture.create_view(&Default::default()))
}

impl Pipelines {
    fn new(device: &wgpu::Device) -> Pipelines {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("map"),
            source: wgpu::ShaderSource::Wgsl(include_str!("map.wgsl").into()),
        });
        let uniform = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let both = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let globals = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map globals"),
            entries: &[uniform(0, both)],
        });
        let material = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map material"),
            entries: &[
                uniform(0, wgpu::ShaderStages::FRAGMENT),
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
            label: Some("map"),
            bind_group_layouts: &[Some(&globals), Some(&material)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
        let mut pipelines = HashMap::new();
        for blended in [false, true] {
            for cull in [false, true] {
                let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("map"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Vertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &attributes,
                        })],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: COLOR_FORMAT,
                            blend: blended.then_some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        // Unity's front faces wind clockwise; the data is
                        // left-handed and so is the projection.
                        front_face: wgpu::FrontFace::Cw,
                        cull_mode: cull.then_some(wgpu::Face::Back),
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH,
                        depth_write_enabled: Some(!blended),
                        depth_compare: Some(wgpu::CompareFunction::LessEqual),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: wgpu::MultisampleState {
                        count: SAMPLES,
                        ..Default::default()
                    },
                    multiview_mask: None,
                    cache: None,
                });
                pipelines.insert((blended, cull), pipeline);
            }
        }
        let sampler = |mode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: mode,
                address_mode_v: mode,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                anisotropy_clamp: 16,
                ..Default::default()
            })
        };
        Pipelines {
            globals,
            material,
            pipelines,
            repeat: sampler(wgpu::AddressMode::Repeat),
            clamp: sampler(wgpu::AddressMode::ClampToEdge),
            compressed: HashMap::new(),
            culled: std::cell::Cell::new(0),
        }
    }
}

/// One submesh to draw.
struct Draw {
    vb: usize,
    first: u32,
    count: u32,
    material: usize,
    blended: bool,
    cull: bool,
    /// Distance from the eye to the object's middle, for sorting.
    distance: f32,
    queue: i32,
}

impl Renderer {
    /// How many objects the last map drawn left out as outside the view.
    pub fn map_culled(&self) -> usize {
        self.map.as_ref().map_or(0, |p| p.culled.get())
    }

    /// Draws a map from `camera` to an RGBA8 image, top row first.
    pub fn render_map(&mut self, map: &MapScene, camera: &Camera, width: u32, height: u32) -> Result<Vec<u8>> {
        if self.map.is_none() {
            self.map = Some(Pipelines::new(&self.device));
        }
        let aspect = width as f32 / height as f32;
        let (min, max) = map.bounds();
        // Near and far from the map's size: rooms and grounds alike.
        let far = ((max - min).length() * 2.0).max(50.0) + camera.eye.distance((min + max) * 0.5);
        let proj = Mat4::perspective_lh(camera.fov_y_deg.to_radians(), aspect, 0.03, far);
        let view_proj = proj * Mat4::look_at_lh(camera.eye, camera.target, Vec3::Y);
        let l = &map.light;
        let fog = l.fog.unwrap_or(Fog {
            color: [0.0; 3],
            start: 0.0,
            end: 1.0,
        });
        let globals = Globals {
            view_proj: view_proj.to_cols_array(),
            eye: [
                camera.eye.x,
                camera.eye.y,
                camera.eye.z,
                l.cut_above.unwrap_or(f32::MAX),
            ],
            sun_dir: [l.sun_dir[0], l.sun_dir[1], l.sun_dir[2], 0.0],
            sun_color: [l.sun_color[0], l.sun_color[1], l.sun_color[2], 0.0],
            ambient: [l.ambient[0], l.ambient[1], l.ambient[2], 0.0],
            fog_color: [fog.color[0], fog.color[1], fog.color[2], 1.0],
            fog_range: [fog.start, fog.end, 0.0, if l.fog.is_some() { 1.0 } else { 0.0 }],
        };

        // Textures first: the caches need the renderer mutably.
        let mut views: HashMap<usize, wgpu::TextureView> = HashMap::new();
        for o in &map.objects {
            for m in &o.materials {
                let Some(image) = &m.albedo else { continue };
                let key = Arc::as_ptr(image) as usize;
                if views.contains_key(&key) {
                    continue;
                }
                let view = match image.as_ref() {
                    MapImage::Decoded(i) => self.texture(i),
                    MapImage::Compressed { .. } => {
                        let pipes = self.map.as_mut().expect("made above");
                        match pipes.compressed.get(&key) {
                            Some((_, v)) => v.clone(),
                            None => {
                                let v = upload_blocks(&self.device, &self.queue, image)
                                    .unwrap_or_else(|| self.white.clone());
                                // The image is kept with its texture, so its
                                // address is not handed out again.
                                pipes.compressed.insert(key, (image.clone(), v.clone()));
                                v
                            }
                        }
                    }
                };
                views.insert(key, view);
            }
        }
        let white = self.white.clone();
        let device = &self.device;
        let pipes = self.map.as_ref().expect("made above");

        let globals_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("map globals"),
            contents: bytemuck::bytes_of(&globals),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("map globals"),
            layout: &pipes.globals,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });

        // One bind group per material, by its address.
        let mut materials: Vec<wgpu::BindGroup> = Vec::new();
        let mut material_index: HashMap<usize, usize> = HashMap::new();
        let mut buffers: Vec<(wgpu::Buffer, wgpu::Buffer)> = Vec::new();
        let mut draws: Vec<Draw> = Vec::new();
        let mut culled = 0;
        for o in &map.objects {
            let mesh = &o.mesh;
            if mesh.positions.is_empty() || mesh.indices.is_empty() {
                continue;
            }
            if !in_view(&view_proj, mesh.bounds) {
                culled += 1;
                continue;
            }
            let verts: Vec<Vertex> = (0..mesh.positions.len())
                .map(|i| Vertex {
                    pos: mesh.positions[i],
                    normal: mesh.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]),
                    uv: mesh.uv.get(i).copied().unwrap_or([0.0, 0.0]),
                })
                .collect();
            let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&o.name),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&o.name),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            buffers.push((vb, ib));
            let middle = (Vec3::from(mesh.bounds.0) + Vec3::from(mesh.bounds.1)) * 0.5;
            for (s, &(first, count)) in mesh.submeshes.iter().enumerate() {
                // Unity draws extra submeshes with the last material.
                let Some(m) = o.materials.get(s).or(o.materials.last()) else {
                    continue;
                };
                if !m.drawn || count == 0 {
                    continue;
                }
                let key = Arc::as_ptr(m) as usize;
                let index = *material_index.entry(key).or_insert_with(|| {
                    let uniform = MaterialUniform {
                        color: m.color,
                        st: m.st,
                        emission: [m.emission[0], m.emission[1], m.emission[2], 0.0],
                        params: [
                            if m.blend == Blend::Cutout { m.cutoff } else { -1.0 },
                            if m.blend == Blend::Fade { 1.0 } else { 0.0 },
                            if m.unlit { 1.0 } else { 0.0 },
                            0.0,
                        ],
                    };
                    let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("map material"),
                        contents: bytemuck::bytes_of(&uniform),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                    let view = m
                        .albedo
                        .as_ref()
                        .and_then(|i| views.get(&(Arc::as_ptr(i) as usize)))
                        .unwrap_or(&white);
                    let repeat = m.albedo.as_ref().is_none_or(|i| i.repeat());
                    materials.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("map material"),
                        layout: &pipes.material,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: buf.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::Sampler(if repeat {
                                    &pipes.repeat
                                } else {
                                    &pipes.clamp
                                }),
                            },
                        ],
                    }));
                    materials.len() - 1
                });
                draws.push(Draw {
                    vb: buffers.len() - 1,
                    first,
                    count,
                    material: index,
                    blended: m.blend == Blend::Fade,
                    cull: !m.two_sided,
                    distance: middle.distance(camera.eye),
                    queue: m.queue,
                });
            }
        }
        pipes.culled.set(culled);
        // Opaque and cut-out first, then blended by queue, far to near.
        draws.sort_by(|a, b| {
            a.blended.cmp(&b.blended).then_with(|| {
                if a.blended {
                    a.queue.cmp(&b.queue).then(b.distance.total_cmp(&a.distance))
                } else {
                    a.material.cmp(&b.material)
                }
            })
        });

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let make = |format, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("map target"),
                    size,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("map picture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&Default::default());
        let msaa = make(COLOR_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let depth = make(DEPTH, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let bg = l.background;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("map"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &msaa,
                    depth_slice: None,
                    resolve_target: Some(&target_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0] as f64,
                            g: bg[1] as f64,
                            b: bg[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Discard,
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
                multiview_mask: None,
            });
            pass.set_bind_group(0, &globals_group, &[]);
            for d in &draws {
                pass.set_pipeline(&pipes.pipelines[&(d.blended, d.cull)]);
                pass.set_bind_group(1, &materials[d.material], &[]);
                let (vb, ib) = &buffers[d.vb];
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(d.first..d.first + d.count, 0, 0..1);
            }
        }
        // Read back, honouring the row alignment wgpu requires.
        let row = width * 4;
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("map readback"),
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
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            size,
        );
        self.queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| drop(tx.send(r)));
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;
        let data = slice.get_mapped_range()?;
        let mut out = Vec::with_capacity((row * height) as usize);
        for y in 0..height {
            let start = (y * padded) as usize;
            out.extend_from_slice(&data[start..start + row as usize]);
        }
        Ok(out)
    }
}
