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
const SHADOW: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The side of the shadow atlas, four cascades of half that side each.
const SHADOW_ATLAS: u32 = 4096;
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
    /// Linear emitted colour, added after lighting, times `emission_map`.
    pub emission: [f32; 3],
    pub emission_map: Option<Arc<MapImage>>,
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
            emission_map: None,
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
    /// Where each vertex reads its object's lightmap, already scaled and
    /// moved into the object's part of it. Empty without a lightmap.
    pub uv2: Vec<[f32; 2]>,
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
    /// The baked lightmap it reads, by its place in `MapScene::lightmaps`.
    pub lightmap: Option<usize>,
    /// Whether it casts the sun's shadow, and whether shadows fall on it.
    pub casts_shadows: bool,
    pub receives_shadows: bool,
    /// Casts its shadow without being seen (Unity's "shadows only").
    pub shadows_only: bool,
    /// Its Unity layer: the sun lights, and takes shadows from, only the
    /// layers of its culling mask.
    pub layer: u32,
    /// The volumes of `MapScene::vanish` that hide it while the camera is
    /// inside one.
    pub vanish: Vec<usize>,
}

/// A volume of a map's table of walls that hide (CharaStudio's vanish):
/// while the camera is inside it, the objects naming it are not drawn and
/// cast no shadow, so that a room shows from outside its walls.
#[derive(Debug, Clone, PartialEq)]
pub enum VanishVolume {
    /// A box: world to the box's own space, where it spans `centre` plus
    /// and minus `half`.
    Box {
        to_local: Mat4,
        centre: Vec3,
        half: Vec3,
    },
    Sphere {
        centre: Vec3,
        radius: f32,
    },
}

impl VanishVolume {
    pub fn contains(&self, p: Vec3) -> bool {
        match self {
            VanishVolume::Box { to_local, centre, half } => {
                let d = (to_local.transform_point3(p) - *centre).abs();
                d.x <= half.x && d.y <= half.y && d.z <= half.z
            }
            VanishVolume::Sphere { centre, radius } => p.distance(*centre) <= *radius,
        }
    }
}

/// A baked lightmap: the light, stored as RGBM, and, for directional
/// lightmaps, the main direction light came from.
#[derive(Debug)]
pub struct Lightmap {
    pub light: Arc<MapImage>,
    pub direction: Option<Arc<MapImage>>,
}

/// Unity 5.6's decoding of RGBM lightmaps in linear colour
/// (`DecodeLightmapRGBM` with the `unity_Lightmap_HDR` it sets there):
/// light = scale * a^exponent * rgb. The scale is 5^2.2: RGBM keeps up to
/// five times white, in gamma.
pub const RGBM_SCALE: f32 = 34.493_24;
pub const RGBM_EXPONENT: f32 = 2.2;

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
    /// The layers the sun lights, a bit each.
    pub sun_mask: u32,
    /// Ambient light as Unity's shaders read it (`unity_SHAr` to
    /// `unity_SHC`, see `modshader::sh_constants`), for what has no
    /// lightmap.
    pub ambient_sh: [[f32; 4]; 7],
    pub fog: Option<Fog>,
    /// The scene's sky, drawn behind everything as a camera that clears
    /// with the skybox does. Without one the picture is cleared with
    /// `background`.
    pub sky: Option<crate::sky::Sky>,
    /// What shows where nothing is drawn.
    pub background: [f32; 3],
    /// A cutaway: nothing above this height is drawn. For looking into
    /// rooms from above.
    pub cut_above: Option<f32>,
    /// The sun's real-time shadows, when it casts them.
    pub shadows: Option<SunShadows>,
}

/// How the sun's shadows are drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunShadows {
    /// How dark a shadow is: 1 takes all the sun away.
    pub strength: f32,
    /// How far from the camera shadows reach, in metres.
    pub distance: f32,
}

/// A map ready to draw.
#[derive(Debug)]
pub struct MapScene {
    pub objects: Vec<MapObject>,
    pub lightmaps: Vec<Lightmap>,
    pub light: MapLight,
    /// The volumes of the walls that hide.
    pub vanish: Vec<VanishVolume>,
    /// The layers the camera draws, a bit each. Objects on other layers
    /// are not seen, though they still cast the sun's shadow when they are
    /// on its layers, as in Unity.
    pub camera_mask: u32,
}

/// The layers CharaStudio's camera draws: Water (4), Chara (10), Map (11)
/// and 29 (Studio/Camera), as its captures record it.
pub const STUDIO_CAMERA_MASK: u32 = 536_874_000;

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
    uv2: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShadowUniform {
    to_atlas: [[f32; 16]; 4],
    ends: [f32; 4],
    params: [f32; 4],
    texel_world: [f32; 4],
    forward: [f32; 4],
}

/// One cascade of the sun's shadow map: world to the cascade's clip
/// space, and the world size of one texel. The cascade's sides fit its
/// slice of the view, snapped to whole texels so that shadows hold still
/// as the camera moves; its depth covers the whole map, so that a caster
/// outside the slice still shades it.
fn fit_cascade(slice: &[Vec3; 8], light: Mat4, map: (Vec3, Vec3), tile: u32) -> Option<(Mat4, f32)> {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for p in slice {
        let q = light.transform_point3(*p);
        lo = lo.min(q);
        hi = hi.max(q);
    }
    // A square around the slice's bounding sphere keeps the size steady
    // as the view turns.
    let centre_world = slice.iter().fold(Vec3::ZERO, |a, p| a + *p) / 8.0;
    let radius = slice.iter().map(|p| p.distance(centre_world)).fold(0.0, f32::max);
    let size = (radius * 2.0) * (1.0 + 20.0 / tile as f32);
    let texel = size / tile as f32;
    let centre = light.transform_point3(centre_world);
    let snap = |v: f32| (v / texel).floor() * texel;
    let (cx, cy) = (snap(centre.x), snap(centre.y));
    let half = size * 0.5;
    let (mut near, mut far) = (lo.z, hi.z);
    for i in 0..8 {
        let corner = Vec3::new(
            if i & 1 == 0 { map.0.x } else { map.1.x },
            if i & 2 == 0 { map.0.y } else { map.1.y },
            if i & 4 == 0 { map.0.z } else { map.1.z },
        );
        let z = light.transform_point3(corner).z;
        near = near.min(z);
        far = far.max(z);
    }
    if half <= 0.0 || far <= near {
        return None;
    }
    let proj = Mat4::orthographic_lh(cx - half, cx + half, cy - half, cy + half, near - 1.0, far + 1.0);
    Some((proj * light, texel))
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LightmapUniform {
    /// 1 when there is a lightmap, 1 when it is directional, then the RGBM
    /// scale and exponent.
    info: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [f32; 16],
    eye: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    sh: [[f32; 4]; 7],
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
    lightmap: wgpu::BindGroupLayout,
    cascade: wgpu::BindGroupLayout,
    /// Shadow casters, by cut-out and by culling.
    casters: HashMap<(bool, bool), wgpu::RenderPipeline>,
    compare: wgpu::Sampler,
    /// By blending and by whether back faces are culled.
    pipelines: HashMap<(bool, bool), wgpu::RenderPipeline>,
    repeat: wgpu::Sampler,
    clamp: wgpu::Sampler,
    /// Uploaded block-compressed textures, by the image's address.
    compressed: HashMap<usize, (Arc<MapImage>, wgpu::TextureView)>,
    /// How many objects the last frame left out as outside the view.
    culled: std::cell::Cell<usize>,
    sky: crate::sky::SkyPass,
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
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Pipelines {
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
            entries: &[
                uniform(0, both),
                uniform(1, wgpu::ShaderStages::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let cascade = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map shadow cascade"),
            entries: &[uniform(0, wgpu::ShaderStages::VERTEX)],
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let lightmap = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map lightmap"),
            entries: &[
                uniform(0, wgpu::ShaderStages::FRAGMENT),
                texture(1),
                texture(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map"),
            bind_group_layouts: &[Some(&globals), Some(&material), Some(&lightmap)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x2];
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
        // The sun's shadow casters: depth only, cut-out or solid, culled or
        // two-sided.
        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("map shadow"),
            source: wgpu::ShaderSource::Wgsl(include_str!("map_shadow.wgsl").into()),
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map shadow"),
            bind_group_layouts: &[Some(&cascade), Some(&material)],
            immediate_size: 0,
        });
        let mut casters = HashMap::new();
        for cutout in [false, true] {
            for cull in [false, true] {
                let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("map shadow"),
                    layout: Some(&shadow_layout),
                    vertex: wgpu::VertexState {
                        module: &shadow_shader,
                        entry_point: Some("vs_main"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Vertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &attributes,
                        })],
                    },
                    fragment: cutout.then(|| wgpu::FragmentState {
                        module: &shadow_shader,
                        entry_point: Some("fs_cutout"),
                        compilation_options: Default::default(),
                        targets: &[],
                    }),
                    primitive: wgpu::PrimitiveState {
                        front_face: wgpu::FrontFace::Cw,
                        cull_mode: cull.then_some(wgpu::Face::Back),
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: SHADOW,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(wgpu::CompareFunction::LessEqual),
                        stencil: Default::default(),
                        bias: wgpu::DepthBiasState {
                            constant: 2,
                            slope_scale: 2.0,
                            clamp: 0.0,
                        },
                    }),
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                });
                casters.insert((cutout, cull), pipeline);
            }
        }
        let compare = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("map shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        Pipelines {
            globals,
            material,
            lightmap,
            cascade,
            casters,
            compare,
            pipelines,
            repeat: sampler(wgpu::AddressMode::Repeat),
            clamp: sampler(wgpu::AddressMode::ClampToEdge),
            compressed: HashMap::new(),
            culled: std::cell::Cell::new(0),
            sky: crate::sky::SkyPass::new(device, queue, COLOR_FORMAT, DEPTH, SAMPLES),
        }
    }
}

/// One submesh to draw.
struct Draw {
    vb: usize,
    /// The lightmap's bind group: 0 for none, then one per lightmap.
    lightmap: usize,
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
            self.map = Some(Pipelines::new(&self.device, &self.queue));
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
            sh: l.ambient_sh,
            fog_color: [fog.color[0], fog.color[1], fog.color[2], 1.0],
            fog_range: [fog.start, fog.end, 0.0, if l.fog.is_some() { 1.0 } else { 0.0 }],
        };

        // Textures first: the caches need the renderer mutably.
        let mut views: HashMap<usize, wgpu::TextureView> = HashMap::new();
        let images = map
            .objects
            .iter()
            .flat_map(|o| {
                o.materials
                    .iter()
                    .flat_map(|m| m.albedo.iter().chain(m.emission_map.iter()))
            })
            .chain(
                map.lightmaps
                    .iter()
                    .flat_map(|l| std::iter::once(&l.light).chain(l.direction.as_ref())),
            )
            .chain(l.sky.iter().flat_map(|s| s.images()));
        {
            for image in images {
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
        let view_of = |image: &Arc<MapImage>| views.get(&(Arc::as_ptr(image) as usize)).cloned();
        let sky_cube = match &l.sky {
            Some(crate::sky::Sky::Cubemap { cube: Some(cube), .. }) => {
                let pipes = self.map.as_mut().expect("made above");
                Some(pipes.sky.cube(&self.device, &self.queue, cube))
            }
            _ => None,
        };
        let white = self.white.clone();
        let device = &self.device;
        let pipes = self.map.as_ref().expect("made above");

        let globals_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("map globals"),
            contents: bytemuck::bytes_of(&globals),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        // The sun's shadow: four cascades over slices of the view, in a
        // square atlas, two by two.
        let forward = (camera.target - camera.eye).normalize_or_zero();
        let mut shadow = ShadowUniform::zeroed();
        let mut cascades: Vec<Mat4> = Vec::new();
        if let Some(set) = l.shadows {
            let view = crate::shadow::View {
                eye: camera.eye,
                forward,
                fov_y_deg: camera.fov_y_deg,
                aspect,
            };
            let splits = crate::shadow::ShadowSettings::default().cascade_splits;
            let ranges = crate::shadow::cascade_ranges(0.03, set.distance, splits);
            let light = crate::shadow::light_view(Vec3::from(l.sun_dir));
            for (i, r) in ranges.iter().enumerate() {
                let slice = view.slice_corners(r.0, r.1);
                if let Some((vp, texel)) = fit_cascade(&slice, light, (min, max), SHADOW_ATLAS / 2) {
                    shadow.to_atlas[i] = crate::shadow::world_to_shadow(vp, i).to_cols_array();
                    shadow.texel_world[i] = texel;
                    cascades.push(vp);
                }
            }
            if cascades.len() == crate::shadow::CASCADES {
                shadow.ends = [ranges[0].1, ranges[1].1, ranges[2].1, ranges[3].1];
                shadow.params = [set.strength, 1.0, 1.0 / SHADOW_ATLAS as f32, 0.0];
                shadow.forward = [forward.x, forward.y, forward.z, 0.0];
            } else {
                cascades.clear();
            }
        }
        let atlas_side = if cascades.is_empty() { 1 } else { SHADOW_ATLAS };
        let atlas = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("map shadow atlas"),
                size: wgpu::Extent3d {
                    width: atlas_side,
                    height: atlas_side,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SHADOW,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let shadow_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("map shadow"),
            contents: bytemuck::bytes_of(&shadow),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let cascade_groups: Vec<wgpu::BindGroup> = cascades
            .iter()
            .map(|vp| {
                let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("map shadow cascade"),
                    contents: bytemuck::cast_slice(&vp.to_cols_array()),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("map shadow cascade"),
                    layout: &pipes.cascade,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buf.as_entire_binding(),
                    }],
                })
            })
            .collect();
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("map globals"),
            layout: &pipes.globals,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: shadow_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&atlas),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&pipes.compare),
                },
            ],
        });

        // The sky's textures and its numbers for this view.
        let sky_group = l.sky.as_ref().map(|sky| {
            let face = |i: usize| match sky {
                crate::sky::Sky::SixSided { faces, .. } => faces[i].image.as_ref().and_then(view_of),
                _ => None,
            };
            let faces: [Option<wgpu::TextureView>; 6] = std::array::from_fn(face);
            let uniform = crate::sky::uniform(sky, l.sun_dir, l.sun_color, camera, aspect);
            pipes.sky.bind(
                device,
                &uniform,
                sky_cube.as_ref(),
                std::array::from_fn(|i| faces[i].as_ref().unwrap_or(&white)),
                &pipes.clamp,
                &pipes.repeat,
            )
        });

        // The lightmaps' bind groups: the first for objects without one.
        let lightmap_group = |info: [f32; 4], light: &wgpu::TextureView, direction: &wgpu::TextureView| {
            let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("map lightmap"),
                contents: bytemuck::bytes_of(&LightmapUniform { info }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("map lightmap"),
                layout: &pipes.lightmap,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(light),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(direction),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&pipes.clamp),
                    },
                ],
            })
        };
        let mut lightmaps = vec![lightmap_group([0.0; 4], &white, &white)];
        for l in &map.lightmaps {
            let light = view_of(&l.light).unwrap_or_else(|| white.clone());
            let direction = l.direction.as_ref().and_then(view_of);
            let info = [1.0, f32::from(u8::from(direction.is_some())), RGBM_SCALE, RGBM_EXPONENT];
            lightmaps.push(lightmap_group(info, &light, direction.as_ref().unwrap_or(&white)));
        }

        // One bind group per material and whether shadows fall on it.
        let mut materials: Vec<wgpu::BindGroup> = Vec::new();
        let mut material_index: HashMap<(usize, bool, bool), usize> = HashMap::new();
        let mut bind_material = |m: &Arc<MapMaterial>, receives: bool, sunlit: bool| -> usize {
            *material_index
                .entry((Arc::as_ptr(m) as usize, receives, sunlit))
                .or_insert_with(|| {
                    let uniform = MaterialUniform {
                        color: m.color,
                        st: m.st,
                        emission: [m.emission[0], m.emission[1], m.emission[2], 0.0],
                        params: [
                            if m.blend == Blend::Cutout { m.cutoff } else { -1.0 },
                            if m.blend == Blend::Fade { 1.0 } else { 0.0 },
                            if m.unlit { 1.0 } else { 0.0 },
                            // 1 when shadows fall on it, 2 more when the sun
                            // lights it at all.
                            f32::from(u8::from(receives)) + 2.0 * f32::from(u8::from(sunlit)),
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
                    materials.push(
                        device.create_bind_group(&wgpu::BindGroupDescriptor {
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
                                wgpu::BindGroupEntry {
                                    binding: 3,
                                    resource: wgpu::BindingResource::TextureView(
                                        m.emission_map
                                            .as_ref()
                                            .and_then(|i| views.get(&(Arc::as_ptr(i) as usize)))
                                            .unwrap_or(&white),
                                    ),
                                },
                            ],
                        }),
                    );
                    materials.len() - 1
                })
        };
        let mut buffers: Vec<(wgpu::Buffer, wgpu::Buffer)> = Vec::new();
        let mut draws: Vec<Draw> = Vec::new();
        // What casts the sun's shadow, seen or not: buffer, first index,
        // count, material, cut-out, culled.
        let mut casters: Vec<(usize, u32, u32, usize, bool, bool)> = Vec::new();
        let shadowed = l.shadows.is_some();
        let mut culled = 0;
        // The volumes the camera stands in hide what names them.
        let inside: Vec<bool> = map.vanish.iter().map(|v| v.contains(camera.eye)).collect();
        for o in &map.objects {
            let mesh = &o.mesh;
            if mesh.positions.is_empty() || mesh.indices.is_empty() {
                continue;
            }
            if o.vanish.iter().any(|v| inside.get(*v).copied().unwrap_or(false)) {
                continue;
            }
            let on_camera = !o.shadows_only && o.layer < 32 && map.camera_mask & (1 << o.layer) != 0;
            let seen = on_camera && in_view(&view_proj, mesh.bounds);
            let sunlit = o.layer < 32 && l.sun_mask & (1 << o.layer) != 0;
            if !seen {
                culled += 1;
                if !(shadowed && sunlit && o.casts_shadows) {
                    continue;
                }
            }
            let verts: Vec<Vertex> = (0..mesh.positions.len())
                .map(|i| Vertex {
                    pos: mesh.positions[i],
                    normal: mesh.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]),
                    uv: mesh.uv.get(i).copied().unwrap_or([0.0, 0.0]),
                    uv2: mesh.uv2.get(i).copied().unwrap_or([0.0, 0.0]),
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
            let vb = buffers.len() - 1;
            let middle = (Vec3::from(mesh.bounds.0) + Vec3::from(mesh.bounds.1)) * 0.5;
            for (s, &(first, count)) in mesh.submeshes.iter().enumerate() {
                // Unity draws extra submeshes with the last material.
                let Some(m) = o.materials.get(s).or(o.materials.last()) else {
                    continue;
                };
                if !m.drawn || count == 0 {
                    continue;
                }
                let index = bind_material(m, o.receives_shadows && shadowed && sunlit, sunlit);
                // Blended surfaces dither their shadow nearly away in
                // Unity's Standard shader; they cast none here.
                if shadowed && sunlit && o.casts_shadows && m.blend != Blend::Fade {
                    casters.push((vb, first, count, index, m.blend == Blend::Cutout, !m.two_sided));
                }
                if seen {
                    draws.push(Draw {
                        vb,
                        lightmap: o.lightmap.filter(|i| *i < map.lightmaps.len()).map_or(0, |i| i + 1),
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
        if !cascades.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("map shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &atlas,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let tile = (SHADOW_ATLAS / 2) as f32;
            for (i, group) in cascade_groups.iter().enumerate() {
                let [ox, oy] = crate::shadow::atlas_origin(i);
                let (x, y) = (ox * SHADOW_ATLAS as f32, oy * SHADOW_ATLAS as f32);
                pass.set_viewport(x, y, tile, tile, 0.0, 1.0);
                pass.set_scissor_rect(x as u32, y as u32, tile as u32, tile as u32);
                pass.set_bind_group(0, group, &[]);
                for &(vb, first, count, material, cutout, cull) in &casters {
                    pass.set_pipeline(&pipes.casters[&(cutout, cull)]);
                    pass.set_bind_group(1, &materials[material], &[]);
                    let (v, i) = &buffers[vb];
                    pass.set_vertex_buffer(0, v.slice(..));
                    pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(first..first + count, 0, 0..1);
                }
            }
        }
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
            // As Unity orders a frame: the sky after everything opaque,
            // where the depth buffer is still open, and before what blends.
            let mut sky_group = sky_group.as_ref();
            let mut sky_once = |pass: &mut wgpu::RenderPass<'_>| {
                if let Some(group) = sky_group.take() {
                    pipes.sky.draw(pass, group);
                    pass.set_bind_group(0, &globals_group, &[]);
                }
            };
            for d in &draws {
                if d.blended {
                    sky_once(&mut pass);
                }
                pass.set_pipeline(&pipes.pipelines[&(d.blended, d.cull)]);
                pass.set_bind_group(1, &materials[d.material], &[]);
                pass.set_bind_group(2, &lightmaps[d.lightmap], &[]);
                let (vb, ib) = &buffers[d.vb];
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(d.first..d.first + d.count, 0, 0..1);
            }
            sky_once(&mut pass);
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
