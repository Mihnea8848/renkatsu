// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The sky behind a map: Unity 5.6's built-in skybox shaders
//! (`Skybox/Procedural`, `Skybox/6 Sided`, `Skybox/Cubemap`), drawn at the
//! far plane where nothing else was drawn. The formulas are in `sky.wgsl`,
//! ported from the programs the game's map bundles carry.

use crate::map::MapImage;
use crate::Camera;
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// `unity_ColorSpaceDouble` in a program compiled for linear colour, which
/// the textured skies multiply by.
pub const DOUBLE_LINEAR: f32 = 4.594_794;
/// The same in a program compiled for gamma colour.
pub const DOUBLE_GAMMA: f32 = 2.0;
/// `_Tex_HDR` of a plain texture: its colour as it is.
pub const HDR_PLAIN: [f32; 4] = [1.0, 1.0, 0.0, 0.0];

/// `Skybox/Procedural`'s sun, by the material's `_SUNDISK_*` keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SunDisk {
    None,
    Simple,
    HighQuality,
}

/// The material of `Skybox/Procedural`, colours linear as the shader gets
/// them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProceduralSky {
    pub sun_disk: SunDisk,
    pub sun_size: f32,
    pub atmosphere_thickness: f32,
    pub sky_tint: [f32; 3],
    pub ground: [f32; 3],
    pub exposure: f32,
}

/// What the two textured skies share.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyLook {
    /// `_Tint`, linear.
    pub tint: [f32; 3],
    pub exposure: f32,
    /// `_Rotation` about the vertical axis, in degrees.
    pub rotation: f32,
    /// `unity_ColorSpaceDouble` as the bundle's program was compiled:
    /// [`DOUBLE_LINEAR`] or [`DOUBLE_GAMMA`].
    pub double: f32,
    /// Whether the program raises alpha to the HDR exponent, which the
    /// linear one does.
    pub exponent: bool,
}

/// One texture of a sky with its `_Tex_HDR` decoding.
#[derive(Debug, Clone, Default)]
pub struct SkyTexture {
    pub image: Option<Arc<MapImage>>,
    pub hdr: [f32; 4],
}

/// A cubemap's top level: six square RGBA8 faces in Direct3D's order (+X,
/// -X, +Y, -Y, +Z, -Z), rows as Unity stores them.
#[derive(Debug, PartialEq, Eq)]
pub struct MapCube {
    pub name: String,
    pub size: u32,
    pub faces: [Vec<u8>; 6],
    pub srgb: bool,
}

/// The faces of `Skybox/6 Sided` in the order they are kept here.
pub const FRONT: usize = 0;
pub const BACK: usize = 1;
pub const LEFT: usize = 2;
pub const RIGHT: usize = 3;
pub const UP: usize = 4;
pub const DOWN: usize = 5;

impl PartialEq for SkyTexture {
    /// The same picture, not an equal one: textures are shared, not copied.
    fn eq(&self, other: &Self) -> bool {
        let same = match (&self.image, &other.image) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        same && self.hdr == other.hdr
    }
}

/// A scene's sky: its `RenderSettings.m_SkyboxMaterial`.
#[derive(Debug, Clone, PartialEq)]
pub enum Sky {
    Procedural(ProceduralSky),
    /// Front (+Z), back (-Z), left (+X), right (-X), up, down.
    SixSided {
        faces: [SkyTexture; 6],
        look: SkyLook,
    },
    /// Without a cubemap the shader samples Unity's grey default.
    Cubemap {
        cube: Option<Arc<MapCube>>,
        hdr: [f32; 4],
        look: SkyLook,
    },
}

impl Sky {
    /// The 2D textures the sky samples.
    pub fn images(&self) -> impl Iterator<Item = &Arc<MapImage>> {
        let faces: &[SkyTexture] = match self {
            Sky::SixSided { faces, .. } => faces,
            _ => &[],
        };
        faces.iter().filter_map(|f| f.image.as_ref())
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub(crate) struct SkyUniform {
    right: [f32; 4],
    up: [f32; 4],
    forward: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    kind: [f32; 4],
    params: [f32; 4],
    sky_tint: [f32; 4],
    ground: [f32; 4],
    tint: [f32; 4],
    hdr: [[f32; 4]; 6],
    wrap_sides: [f32; 4],
    wrap_ends: [f32; 4],
}

/// The camera's axes as the sky shader wants them: the direction through
/// a point of the picture is `forward + x * right + y * up`, with x and y
/// from -1 to 1 (y up). Matches `Mat4::look_at_lh` with
/// `Mat4::perspective_lh`.
pub fn camera_axes(camera: &Camera, aspect: f32) -> [Vec3; 3] {
    let forward = (camera.target - camera.eye).normalize_or_zero();
    let right = Vec3::Y.cross(forward).normalize_or_zero();
    let up = forward.cross(right);
    let tan = (camera.fov_y_deg.to_radians() * 0.5).tan();
    [right * tan * aspect, up * tan, forward]
}

/// The uniform block of a sky under a sun.
pub(crate) fn uniform(sky: &Sky, sun_dir: [f32; 3], sun_color: [f32; 3], camera: &Camera, aspect: f32) -> SkyUniform {
    let [right, up, forward] = camera_axes(camera, aspect);
    let v4 = |v: [f32; 3], w: f32| [v[0], v[1], v[2], w];
    let mut u = SkyUniform {
        right: v4(right.to_array(), 0.0),
        up: v4(up.to_array(), 0.0),
        forward: v4(forward.to_array(), 0.0),
        sun_dir: v4(sun_dir, 0.0),
        sun_color: v4(sun_color, 0.0),
        ..SkyUniform::zeroed()
    };
    let textured = |u: &mut SkyUniform, kind: f32, look: &SkyLook| {
        u.kind = [kind, 0.0, look.exposure, look.rotation.to_radians()];
        u.params = [0.0, 0.0, look.double, f32::from(u8::from(look.exponent))];
        u.tint = v4(look.tint, 0.0);
    };
    match sky {
        Sky::Procedural(p) => {
            let disk = match p.sun_disk {
                SunDisk::None => 0.0,
                SunDisk::Simple => 1.0,
                SunDisk::HighQuality => 2.0,
            };
            u.kind = [0.0, disk, p.exposure, 0.0];
            u.params = [p.sun_size, p.atmosphere_thickness, 0.0, 0.0];
            u.sky_tint = v4(p.sky_tint, 0.0);
            u.ground = v4(p.ground, 0.0);
        }
        Sky::SixSided { faces, look } => {
            textured(&mut u, 1.0, look);
            let wraps = |i: usize| f32::from(u8::from(faces[i].image.as_ref().is_some_and(|m| m.repeat())));
            for (to, face) in u.hdr.iter_mut().zip(faces) {
                *to = face.hdr;
            }
            u.wrap_sides = [wraps(FRONT), wraps(BACK), wraps(LEFT), wraps(RIGHT)];
            u.wrap_ends = [wraps(UP), wraps(DOWN), 0.0, 0.0];
        }
        Sky::Cubemap { hdr, look, .. } => {
            textured(&mut u, 2.0, look);
            u.hdr[0] = *hdr;
        }
    }
    u
}

/// The sky's pipeline and its uploaded cubemaps.
pub(crate) struct SkyPass {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    /// Unity's grey default for a cubemap slot left empty.
    grey: wgpu::TextureView,
    /// Uploaded cubemaps, by the cube's address; the cube is kept so that
    /// its address is not handed out again.
    cubes: HashMap<usize, (Arc<MapCube>, wgpu::TextureView)>,
}

fn upload_cube(device: &wgpu::Device, queue: &wgpu::Queue, cube: &MapCube) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&cube.name),
        size: wgpu::Extent3d {
            width: cube.size,
            height: cube.size,
            depth_or_array_layers: 6,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: if cube.srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        },
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (layer, face) in cube.faces.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer as u32,
                },
                aspect: wgpu::TextureAspect::All,
            },
            face,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(cube.size * 4),
                rows_per_image: Some(cube.size),
            },
            wgpu::Extent3d {
                width: cube.size,
                height: cube.size,
                depth_or_array_layers: 1,
            },
        );
    }
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::Cube),
        ..Default::default()
    })
}

impl SkyPass {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        color: wgpu::TextureFormat,
        depth: wgpu::TextureFormat,
        samples: u32,
    ) -> SkyPass {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("map sky"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky.wgsl").into()),
        });
        let texture = |binding, view_dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension,
                multisampled: false,
            },
            count: None,
        };
        let sampler = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let d2 = wgpu::TextureViewDimension::D2;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map sky"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(1, wgpu::TextureViewDimension::Cube),
                texture(2, d2),
                texture(3, d2),
                texture(4, d2),
                texture(5, d2),
                texture(6, d2),
                texture(7, d2),
                sampler(8),
                sampler(9),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map sky"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("map sky"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            // The shaders' own state: tested against depth, never written.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        let grey = upload_cube(
            device,
            queue,
            &MapCube {
                name: "grey".into(),
                size: 1,
                faces: std::array::from_fn(|_| vec![128; 4]),
                srgb: false,
            },
        );
        SkyPass {
            layout,
            pipeline,
            grey,
            cubes: HashMap::new(),
        }
    }

    /// The view of a cubemap, uploaded the first time it is asked for.
    pub(crate) fn cube(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cube: &Arc<MapCube>,
    ) -> wgpu::TextureView {
        let key = Arc::as_ptr(cube) as usize;
        if let Some((_, view)) = self.cubes.get(&key) {
            return view.clone();
        }
        let max = device.limits().max_texture_dimension_2d;
        let whole = cube
            .faces
            .iter()
            .all(|f| f.len() == (cube.size * cube.size * 4) as usize);
        let view = if cube.size == 0 || cube.size > max || !whole {
            self.grey.clone()
        } else {
            upload_cube(device, queue, cube)
        };
        self.cubes.insert(key, (cube.clone(), view.clone()));
        view
    }

    /// The bind group of one sky for one frame. `faces` are the views of
    /// the six sided sky's textures, `cube` the cubemap's.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn bind(
        &self,
        device: &wgpu::Device,
        uniform: &SkyUniform,
        cube: Option<&wgpu::TextureView>,
        faces: [&wgpu::TextureView; 6],
        clamp: &wgpu::Sampler,
        repeat: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("map sky"),
            contents: bytemuck::bytes_of(uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(cube.unwrap_or(&self.grey)),
            },
        ];
        for (i, view) in faces.into_iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: 2 + i as u32,
                resource: wgpu::BindingResource::TextureView(view),
            });
        }
        entries.push(wgpu::BindGroupEntry {
            binding: 8,
            resource: wgpu::BindingResource::Sampler(clamp),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 9,
            resource: wgpu::BindingResource::Sampler(repeat),
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("map sky"),
            layout: &self.layout,
            entries: &entries,
        })
    }

    /// Draws the sky over what the depth buffer left open.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, group: &wgpu::BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Mat4, Vec4};

    fn camera() -> Camera {
        Camera {
            eye: Vec3::new(1.0, 2.0, -3.0),
            target: Vec3::new(4.0, 2.5, 5.0),
            fov_y_deg: 50.0,
        }
    }

    #[test]
    fn the_sky_shader_is_valid() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("sky.wgsl")).expect("parses");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validates");
    }

    #[test]
    fn the_axes_point_where_the_projection_does() {
        let c = camera();
        let aspect = 1.6;
        let [right, up, forward] = camera_axes(&c, aspect);
        let view_proj = Mat4::perspective_lh(c.fov_y_deg.to_radians(), aspect, 0.03, 100.0)
            * Mat4::look_at_lh(c.eye, c.target, Vec3::Y);
        for p in [
            Vec3::new(3.0, 4.0, 6.0),
            Vec3::new(6.0, 1.0, 2.0),
            Vec3::new(2.0, 0.0, 9.0),
        ] {
            let clip = view_proj * Vec4::new(p.x, p.y, p.z, 1.0);
            let ndc = clip.truncate() / clip.w;
            let dir = (forward + right * ndc.x + up * ndc.y).normalize();
            let want = (p - c.eye).normalize();
            assert!(dir.distance(want) < 1e-4, "{dir} against {want}");
        }
    }

    #[test]
    fn a_procedural_sky_fills_its_block() {
        let sky = Sky::Procedural(ProceduralSky {
            sun_disk: SunDisk::Simple,
            sun_size: 0.04,
            atmosphere_thickness: 1.0,
            sky_tint: [0.2; 3],
            ground: [0.1, 0.2, 0.3],
            exposure: 1.3,
        });
        let u = uniform(&sky, [0.0, 1.0, 0.0], [1.0, 2.0, 3.0], &camera(), 1.0);
        assert_eq!(u.kind, [0.0, 1.0, 1.3, 0.0]);
        assert_eq!(u.params, [0.04, 1.0, 0.0, 0.0]);
        assert_eq!(u.ground, [0.1, 0.2, 0.3, 0.0]);
        assert_eq!(u.sun_color, [1.0, 2.0, 3.0, 0.0]);
    }

    #[test]
    fn a_textured_sky_carries_its_compile_and_its_wrapping() {
        let look = SkyLook {
            tint: [0.5; 3],
            exposure: 1.09,
            rotation: 180.0,
            double: DOUBLE_GAMMA,
            exponent: false,
        };
        let repeating = Arc::new(MapImage::Compressed {
            name: "front".into(),
            width: 4,
            height: 4,
            bc3: false,
            levels: vec![vec![0; 8]],
            srgb: true,
            repeat: true,
        });
        let mut faces: [SkyTexture; 6] = Default::default();
        faces[FRONT] = SkyTexture {
            image: Some(repeating),
            hdr: HDR_PLAIN,
        };
        faces[DOWN].hdr = [34.0, 2.2, 0.0, 1.0];
        let sky = Sky::SixSided { faces, look };
        assert_eq!(sky.images().count(), 1);
        let u = uniform(&sky, [0.0, 1.0, 0.0], [1.0; 3], &camera(), 1.0);
        assert_eq!(u.kind[0], 1.0);
        assert!((u.kind[3] - std::f32::consts::PI).abs() < 1e-6);
        assert_eq!(u.params, [0.0, 0.0, 2.0, 0.0]);
        assert_eq!(u.wrap_sides, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(u.hdr[DOWN], [34.0, 2.2, 0.0, 1.0]);

        let sky = Sky::Cubemap {
            cube: None,
            hdr: HDR_PLAIN,
            look: SkyLook {
                double: DOUBLE_LINEAR,
                exponent: true,
                ..look
            },
        };
        let u = uniform(&sky, [0.0, 1.0, 0.0], [1.0; 3], &camera(), 1.0);
        assert_eq!(u.kind[0], 2.0);
        assert_eq!(u.params, [0.0, 0.0, DOUBLE_LINEAR, 1.0]);
        assert_eq!(u.hdr[0], HDR_PLAIN);
    }
}
