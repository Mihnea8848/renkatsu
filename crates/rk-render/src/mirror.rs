// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Mirrors of a map, as the game's `MirrorReflection` component and its
//! shader `FX/MirrorReflection_Transparent` draw them: before the camera
//! draws the mirror's renderer, a second camera draws the scene reflected
//! about the mirror's plane into a square texture, cut at the plane by an
//! oblique near plane, and the mirror's surface reads that texture at the
//! pixel's own place on the screen.
//!
//! The game draws the reflected view with the world mirrored
//! (`worldToCameraMatrix * reflectionMat`) and the culling turned round
//! (`GL.SetRevertBackfacing`). Here the same picture is drawn flipped
//! left to right, which makes the view an ordinary camera standing at the
//! reflected eye (`mirrored_camera`), and the surface reads it flipped
//! back (`mirror.wgsl`). The pipelines keep their culling that way.

use crate::Camera;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use std::collections::HashMap;

/// Half the side of Unity's built-in Plane mesh, which the game's mirrors
/// are: it lies in X and Z and faces +Y.
pub const PLANE_HALF: f32 = 5.0;

/// The Water layer, which `MirrorReflection` never reflects
/// (`cullingMask = -17 & m_ReflectLayers`).
const WATER_LAYERS: u32 = 1 << 4;

/// The Chara layer, which the characters are on.
pub const CHARA_LAYER: u32 = 10;

/// A mirror of a map: a renderer with the game's `MirrorReflection`
/// component, drawn with `FX/MirrorReflection_Transparent`.
#[derive(Debug, Clone, PartialEq)]
pub struct MapMirror {
    pub name: String,
    /// The mirror object's own space to the map's. Its mesh is Unity's
    /// Plane; its Y axis is the mirror's normal (`transform.up`).
    pub world: Mat4,
    /// `m_TextureSize`: the side of the reflection's square texture.
    pub texture_size: u32,
    /// `m_ClipPlaneOffset`: how far in front of the surface the reflected
    /// view starts.
    pub clip_offset: f32,
    /// `m_ReflectLayers`: the layers the reflection shows, a bit each.
    pub layers: u32,
    /// The layer of the mirror's own object.
    pub layer: u32,
    /// The material's `_Color`, linear.
    pub color: [f32; 4],
    /// The material's render queue (the shader's is Transparent, 3000).
    pub queue: i32,
}

impl MapMirror {
    /// `transform.position`.
    pub fn position(&self) -> Vec3 {
        self.world.transform_point3(Vec3::ZERO)
    }

    /// `transform.up`: the unit normal of the mirror's face.
    pub fn up(&self) -> Vec3 {
        self.world.transform_vector3(Vec3::Y).normalize_or_zero()
    }

    /// The layers the reflection camera draws.
    pub fn camera_mask(&self) -> u32 {
        self.layers & !WATER_LAYERS
    }

    /// The corners of the surface in the map's space, in the order of the
    /// strip that draws it: clockwise from the front, as Unity's Plane.
    pub fn corners(&self) -> [Vec3; 4] {
        let h = PLANE_HALF;
        let corner = |x: f32, z: f32| self.world.transform_point3(Vec3::new(x, 0.0, z));
        // A mirrored object turns its triangles inside out; turn them back.
        if self.world.determinant() < 0.0 {
            [corner(h, h), corner(-h, h), corner(h, -h), corner(-h, -h)]
        } else {
            [corner(h, h), corner(h, -h), corner(-h, h), corner(-h, -h)]
        }
    }

    /// The mirror moved by `root`, the map's place in the world.
    pub fn placed(&self, root: Mat4) -> MapMirror {
        MapMirror {
            world: root * self.world,
            ..self.clone()
        }
    }
}

/// The mirror's plane as `MirrorReflection.OnWillRenderObject` makes it:
/// `(up, -dot(up, position) - m_ClipPlaneOffset)`.
pub fn reflection_plane(position: Vec3, up: Vec3, clip_offset: f32) -> Vec4 {
    up.extend(-up.dot(position) - clip_offset)
}

/// `MirrorReflection.CalculateReflectionMatrix`: the matrix that mirrors
/// points about `plane`.
pub fn reflection_matrix(plane: Vec4) -> Mat4 {
    let [x, y, z, w] = plane.to_array();
    // Written by rows as the game fills m00 to m33.
    Mat4::from_cols_array_2d(&[
        [1.0 - 2.0 * x * x, -2.0 * x * y, -2.0 * x * z, -2.0 * w * x],
        [-2.0 * y * x, 1.0 - 2.0 * y * y, -2.0 * y * z, -2.0 * w * y],
        [-2.0 * z * x, -2.0 * z * y, 1.0 - 2.0 * z * z, -2.0 * w * z],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .transpose()
}

/// `MirrorReflection.CameraSpacePlane` with a side sign of 1: the plane
/// through `position + up * clip_offset` with normal `up`, in the space
/// of `view`. What is in front of the mirror is on its positive side.
pub fn camera_space_plane(view: Mat4, position: Vec3, up: Vec3, clip_offset: f32) -> Vec4 {
    let at = view.transform_point3(position + up * clip_offset);
    let normal = view.transform_vector3(up).normalize_or_zero();
    normal.extend(-at.dot(normal))
}

fn sgn(a: f32) -> f32 {
    if a > 0.0 {
        1.0
    } else if a < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// Unity's `Camera.CalculateObliqueMatrix` for a projection whose depth
/// runs from 0 to 1: the near plane of `proj` is replaced by `plane`
/// (in view space, the kept side positive), and the far plane is turned
/// to pass through the far corner of the view on that side. Unity writes
/// it for depths of -1 to 1 (`unity_oblique` in the tests); the volume
/// kept and the depth of each point are the same.
pub fn oblique(proj: Mat4, plane: Vec4) -> Mat4 {
    let q = proj.inverse() * Vec4::new(sgn(plane.x), sgn(plane.y), 1.0, 1.0);
    let c = plane * (proj.row(3).dot(q) / plane.dot(q));
    let mut rows = proj.transpose();
    rows.z_axis = c;
    rows.transpose()
}

/// A view with an up of its own: a mirror's reflected camera may roll.
#[derive(Debug, Clone, Copy)]
pub struct Eye {
    pub camera: Camera,
    pub up: Vec3,
}

impl Eye {
    /// The camera as it stands, the world's Y up.
    pub fn of(camera: &Camera) -> Eye {
        Eye {
            camera: *camera,
            up: Vec3::Y,
        }
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_lh(self.camera.eye, self.camera.target, self.up)
    }
}

/// The camera's own up, at a right angle to where it looks.
fn camera_up(camera: &Camera) -> Vec3 {
    let forward = (camera.target - camera.eye).try_normalize().unwrap_or(Vec3::Z);
    let right = Vec3::Y.cross(forward).try_normalize().unwrap_or(Vec3::X);
    forward.cross(right)
}

/// The camera that sees what `camera` sees in a mirror on `plane`: its
/// eye, target and up reflected. Its view is the game's
/// `worldToCameraMatrix * reflectionMat` flipped left to right.
pub fn mirrored_camera(camera: &Camera, plane: Vec4) -> Eye {
    let reflect = reflection_matrix(plane);
    Eye {
        camera: Camera {
            eye: reflect.transform_point3(camera.eye),
            target: reflect.transform_point3(camera.target),
            fov_y_deg: camera.fov_y_deg,
        },
        up: reflect.transform_vector3(camera_up(camera)),
    }
}

/// One reflected view of a frame.
#[derive(Debug, Clone, Copy)]
pub struct MirrorView {
    /// Which of the map's mirrors.
    pub index: usize,
    pub eye: Eye,
    /// World to clip space: the main camera's projection, cut at the
    /// mirror.
    pub view_proj: Mat4,
    /// The side of the square texture.
    pub size: u32,
    /// The layers drawn.
    pub mask: u32,
}

/// The reflected view of `camera` in `mirror`, which stands in the
/// world: `None` when the camera is behind the mirror's face, which then
/// does not show. `aspect`, `near` and `far` are the main camera's, which
/// `UpdateCameraModes` copies.
pub fn mirror_view(
    index: usize,
    mirror: &MapMirror,
    camera: &Camera,
    aspect: f32,
    near: f32,
    far: f32,
) -> Option<MirrorView> {
    let (position, up) = (mirror.position(), mirror.up());
    if up == Vec3::ZERO || up.dot(camera.eye - position) <= mirror.clip_offset {
        return None;
    }
    let eye = mirrored_camera(camera, reflection_plane(position, up, mirror.clip_offset));
    let view = eye.view();
    let proj = Mat4::perspective_lh(camera.fov_y_deg.to_radians(), aspect, near, far);
    let clip = camera_space_plane(view, position, up, mirror.clip_offset);
    Some(MirrorView {
        index,
        eye,
        view_proj: oblique(proj, clip) * view,
        size: mirror.texture_size.max(1),
        mask: mirror.camera_mask(),
    })
}

/// Where the surface reads the reflection for a pixel at `uv` of the
/// screen (0 to 1, from the top left): the shader's
/// `ComputeScreenPos(pos).xy / w`, flipped left to right as the
/// reflection is drawn here.
pub fn reflection_uv(uv: [f32; 2]) -> [f32; 2] {
    [1.0 - uv[0], uv[1]]
}

/// The surface's colour: `_ReflectionTex * _MainTex * _Color`, the alpha
/// from the last two. The reflection is an 8 bit texture in the game, so
/// it holds nothing above 1.
pub fn surface_color(reflection: [f32; 3], main: [f32; 4], color: [f32; 4]) -> [f32; 4] {
    [
        reflection[0].clamp(0.0, 1.0) * main[0] * color[0],
        reflection[1].clamp(0.0, 1.0) * main[1] * color[1],
        reflection[2].clamp(0.0, 1.0) * main[2] * color[2],
        main[3] * color[3],
    ]
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SurfaceUniform {
    view_proj: [f32; 16],
    color: [f32; 4],
    /// xy: the picture's width and height in pixels.
    screen: [f32; 4],
}

/// What a reflection is drawn into, and read from by the surface.
struct MirrorTarget {
    size: u32,
    format: wgpu::TextureFormat,
    msaa: wgpu::TextureView,
    resolved: wgpu::TextureView,
    group: wgpu::BindGroup,
}

/// A mirror uploaded: its surface, and its reflection once a frame drew
/// one.
pub(crate) struct MirrorGpu {
    pub(crate) mirror: MapMirror,
    pub(crate) bounds: ([f32; 3], [f32; 3]),
    pub(crate) middle: Vec3,
    quad: wgpu::Buffer,
    uniform: wgpu::Buffer,
    target: Option<MirrorTarget>,
}

/// The pass that draws mirrors' surfaces.
pub(crate) struct MirrorPass {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    /// By the formats drawn into.
    pipelines: HashMap<(wgpu::TextureFormat, wgpu::TextureFormat), wgpu::RenderPipeline>,
    samples: u32,
}

impl MirrorPass {
    pub(crate) fn new(device: &wgpu::Device, samples: u32) -> MirrorPass {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mirror"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mirror.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mirror"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mirror"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        // A render texture's own: between texels, clamped, one level.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mirror"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        MirrorPass {
            layout,
            pipeline_layout,
            shader,
            sampler,
            pipelines: HashMap::new(),
            samples,
        }
    }

    /// Makes the pipeline for drawing into these formats, once.
    pub(crate) fn prepare(&mut self, device: &wgpu::Device, color: wgpu::TextureFormat, depth: wgpu::TextureFormat) {
        if self.pipelines.contains_key(&(color, depth)) {
            return;
        }
        // The shader's pass: Blend SrcAlpha OneMinusSrcAlpha, ZWrite On,
        // Cull Back.
        let blend = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mirror"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<[f32; 3]>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color,
                    blend: Some(wgpu::BlendState {
                        color: blend,
                        alpha: blend,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: self.samples,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.insert((color, depth), pipeline);
    }

    /// Uploads a mirror's surface.
    pub(crate) fn upload(&self, device: &wgpu::Device, mirror: &MapMirror) -> MirrorGpu {
        use wgpu::util::DeviceExt;
        let corners = mirror.corners();
        let positions: Vec<[f32; 3]> = corners.iter().map(|c| c.to_array()).collect();
        let quad = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&mirror.name),
            contents: bytemuck::cast_slice(&positions),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mirror"),
            size: std::mem::size_of::<SurfaceUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let min = corners.iter().fold(Vec3::splat(f32::MAX), |a, c| a.min(*c));
        let max = corners.iter().fold(Vec3::splat(f32::MIN), |a, c| a.max(*c));
        MirrorGpu {
            mirror: mirror.clone(),
            bounds: (min.to_array(), max.to_array()),
            middle: (min + max) * 0.5,
            quad,
            uniform,
            target: None,
        }
    }

    /// The textures a mirror's reflection is drawn into this frame, many
    /// samples a pixel and resolved: made again only when the size or the
    /// format changes.
    pub(crate) fn target(
        &self,
        device: &wgpu::Device,
        gpu: &mut MirrorGpu,
        size: u32,
        format: wgpu::TextureFormat,
    ) -> (wgpu::TextureView, wgpu::TextureView) {
        if gpu.target.as_ref().is_none_or(|t| t.size != size || t.format != format) {
            let make = |samples, usage| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("mirror reflection"),
                        size: wgpu::Extent3d {
                            width: size,
                            height: size,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            let msaa = make(self.samples, wgpu::TextureUsages::RENDER_ATTACHMENT);
            let resolved = make(
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            );
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mirror"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: gpu.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&resolved),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            gpu.target = Some(MirrorTarget {
                size,
                format,
                msaa,
                resolved,
                group,
            });
        }
        let t = gpu.target.as_ref().expect("made above");
        (t.msaa.clone(), t.resolved.clone())
    }

    /// Writes what the surface needs for a frame seen through
    /// `view_proj`, which takes the map's space to the screen.
    pub(crate) fn write(&self, queue: &wgpu::Queue, gpu: &MirrorGpu, view_proj: Mat4, screen: [f32; 2]) {
        let uniform = SurfaceUniform {
            view_proj: view_proj.to_cols_array(),
            color: gpu.mirror.color,
            screen: [screen[0], screen[1], 0.0, 0.0],
        };
        queue.write_buffer(&gpu.uniform, 0, bytemuck::bytes_of(&uniform));
    }

    /// Draws a mirror's surface, when it has a reflection to show. Leaves
    /// its own pipeline and bind group set.
    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        gpu: &MirrorGpu,
        color: wgpu::TextureFormat,
        depth: wgpu::TextureFormat,
    ) -> bool {
        let (Some(target), Some(pipeline)) = (&gpu.target, self.pipelines.get(&(color, depth))) else {
            return false;
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &target.group, &[]);
        pass.set_vertex_buffer(0, gpu.quad.slice(..));
        pass.draw(0..4, 0..1);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    fn mirror(world: Mat4) -> MapMirror {
        MapMirror {
            name: "mirror".into(),
            world,
            texture_size: 1024,
            clip_offset: 0.0,
            layers: 528_383_031,
            layer: 11,
            color: [1.0; 4],
            queue: 3000,
        }
    }

    /// The first toilet's mirror, as its bundle places it.
    fn toilet() -> MapMirror {
        mirror(Mat4::from_scale_rotation_translation(
            Vec3::new(0.233_397_14, 0.358_417_33, 0.092_311_49),
            glam::Quat::from_xyzw(0.5, 0.5, -0.5, 0.5),
            Vec3::new(-2.895, 1.601, 2.748),
        ))
    }

    #[test]
    fn the_shader_is_valid_wgsl() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("mirror.wgsl")).expect("parses");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validates");
        assert_eq!(std::mem::size_of::<SurfaceUniform>(), 96);
    }

    #[test]
    fn a_mirror_faces_along_its_own_up() {
        let m = toilet();
        assert!(close(m.up(), Vec3::X));
        assert!(close(m.position(), Vec3::new(-2.895, 1.601, 2.748)));
        // The plane is ten units a side before the object's scale.
        let c = m.corners();
        assert!(((c[0] - c[3]).length() - (2.333_971_4f32.powi(2) + 0.923_114_9f32.powi(2)).sqrt()).abs() < 1e-3);
        // Clockwise from the front, as the pipeline's front faces.
        let normal = (c[1] - c[0]).cross(c[2] - c[0]);
        assert!(normal.dot(m.up()) > 0.0);
        let flipped = mirror(m.world * Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)));
        let c = flipped.corners();
        assert!((c[1] - c[0]).cross(c[2] - c[0]).dot(flipped.up()) > 0.0);
    }

    #[test]
    fn the_water_layer_is_never_reflected() {
        let mut m = toilet();
        m.layers = u32::MAX;
        assert_eq!(m.camera_mask(), !16);
        // The love hotel's mirror shows characters and the map alone.
        m.layers = 3072;
        assert_eq!(m.camera_mask(), (1 << 10) | (1 << 11));
    }

    #[test]
    fn the_reflection_matrix_is_the_games() {
        let plane = reflection_plane(Vec3::new(2.0, 0.0, 0.0), Vec3::X, 0.0);
        assert_eq!(plane, Vec4::new(1.0, 0.0, 0.0, -2.0));
        let m = reflection_matrix(plane);
        // m00 = 1 - 2 x x, m03 = -2 w x.
        assert_eq!(m.row(0), Vec4::new(-1.0, 0.0, 0.0, 4.0));
        assert_eq!(m.row(1), Vec4::new(0.0, 1.0, 0.0, 0.0));
        assert_eq!(m.row(3), Vec4::new(0.0, 0.0, 0.0, 1.0));
        assert!(close(
            m.transform_point3(Vec3::new(1.0, 2.0, 3.0)),
            Vec3::new(3.0, 2.0, 3.0)
        ));
        // A point of the plane stays, and twice is no change.
        assert!(close(
            m.transform_point3(Vec3::new(2.0, 5.0, -1.0)),
            Vec3::new(2.0, 5.0, -1.0)
        ));
        let tilted = reflection_plane(Vec3::new(0.3, 1.0, -2.0), Vec3::new(0.6, 0.0, 0.8), 0.07);
        let r = reflection_matrix(tilted);
        let p = Vec3::new(4.0, -1.0, 2.5);
        assert!(close(r.transform_point3(r.transform_point3(p)), p));
        // The offset moves the plane along its normal.
        let on = Vec3::new(0.3, 1.0, -2.0) + Vec3::new(0.6, 0.0, 0.8) * 0.07;
        assert!(close(r.transform_point3(on), on));
    }

    #[test]
    fn the_clip_plane_keeps_what_is_in_front_of_the_mirror() {
        let (position, up) = (Vec3::new(1.0, 1.5, 0.0), Vec3::new(0.0, 0.0, -1.0));
        let camera = Camera {
            eye: Vec3::new(0.5, 1.4, 3.0),
            target: Vec3::new(1.0, 1.2, 0.0),
            fov_y_deg: 30.0,
        };
        let view = Eye::of(&camera).view();
        let plane = camera_space_plane(view, position, up, 0.25);
        let side = |p: Vec3| plane.dot(view.transform_point3(p).extend(1.0));
        assert!(side(position + up * 0.25).abs() < 1e-5);
        assert!(side(position + up) > 0.0);
        assert!(side(position - up) < 0.0);
        assert!((plane.truncate().length() - 1.0).abs() < 1e-5);
    }

    /// Unity's own formula, for a projection whose depth runs from -1 to
    /// 1: `q = inverse(P) * (sgn(x), sgn(y), 1, 1)`,
    /// `c = plane * (2 / dot(plane, q))`, third row `c - fourth row`.
    fn unity_oblique(proj: Mat4, plane: Vec4) -> Mat4 {
        let q = proj.inverse() * Vec4::new(sgn(plane.x), sgn(plane.y), 1.0, 1.0);
        let c = plane * (2.0 / plane.dot(q));
        let mut rows = proj.transpose();
        rows.z_axis = c - rows.w_axis;
        rows.transpose()
    }

    #[test]
    fn the_oblique_projection_cuts_at_the_plane_as_unitys() {
        let (fov, aspect, near, far) = (40f32.to_radians(), 1.6, 0.05, 80.0);
        let proj = Mat4::perspective_lh(fov, aspect, near, far);
        // A plane four units ahead, leaning.
        let normal = Vec3::new(0.3, -0.2, 1.0).normalize();
        let plane = normal.extend(-normal.dot(Vec3::new(0.0, 0.0, 4.0)));
        let cut = oblique(proj, plane);
        // The picture's place is left alone.
        assert_eq!(cut.row(0), proj.row(0));
        assert_eq!(cut.row(1), proj.row(1));
        assert_eq!(cut.row(3), proj.row(3));
        let depth = |m: Mat4, p: Vec3| {
            let c = m * p.extend(1.0);
            c.z / c.w
        };
        // On the plane the depth is 0, behind it below 0.
        let on = Vec3::new(0.5, 0.2, 4.0) - normal * plane.dot(Vec3::new(0.5, 0.2, 4.0).extend(1.0));
        assert!(depth(cut, on).abs() < 1e-5);
        assert!(depth(cut, on - normal * 0.5) < 0.0);
        assert!(depth(cut, on + normal * 0.5) > 0.0);
        // The far corner on the plane's side stays at the far end.
        let q = proj.inverse() * Vec4::new(1.0, -1.0, 1.0, 1.0);
        assert!((depth(cut, q.truncate() / q.w) - 1.0).abs() < 1e-4);
        // Unity's, on the same view with depths of -1 to 1, gives each
        // point the same depth once brought to 0 to 1.
        let gl = Mat4::perspective_lh(fov, aspect, near, far);
        let to_gl = Mat4::from_cols_array_2d(&[
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 2.0, 0.0],
            [0.0, 0.0, -1.0, 1.0],
        ]);
        let unity = unity_oblique(to_gl * gl, plane);
        for p in [
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::new(1.0, -2.0, 9.0),
            Vec3::new(-3.0, 0.5, 30.0),
            on - normal * 0.2,
        ] {
            let ours = depth(cut, p);
            let theirs = (depth(unity, p) + 1.0) * 0.5;
            assert!((ours - theirs).abs() < 1e-4, "{ours} {theirs}");
        }
    }

    #[test]
    fn the_mirrored_camera_is_the_games_view_flipped() {
        // A leaning mirror, as the manga club's.
        let up = Vec3::new(0.991, 0.134, 0.0).normalize();
        let position = Vec3::new(-2.198, 0.911, 0.891);
        let plane = reflection_plane(position, up, 0.0);
        let camera = Camera {
            eye: Vec3::new(0.4, 1.3, -1.0),
            target: Vec3::new(-2.0, 1.0, 0.9),
            fov_y_deg: 23.0,
        };
        // The game's: worldToCameraMatrix * reflectionMat.
        let game = Eye::of(&camera).view() * reflection_matrix(plane);
        let ours = mirrored_camera(&camera, plane).view();
        let flip = Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0));
        let (a, b) = ((flip * game).to_cols_array(), ours.to_cols_array());
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-4, "{a:?} {b:?}");
        }
        // The eye stands behind the mirror, as far as the camera in front.
        let eye = mirrored_camera(&camera, plane).camera.eye;
        assert!((up.dot(eye - position) + up.dot(camera.eye - position)).abs() < 1e-5);
    }

    #[test]
    fn a_point_shows_in_the_mirror_where_its_image_stands() {
        let m = toilet();
        let camera = Camera {
            eye: Vec3::new(-1.0, 1.5, 1.2),
            target: Vec3::new(-2.9, 1.5, 2.7),
            fov_y_deg: 35.0,
        };
        let (aspect, near, far) = (16.0 / 9.0, 0.05, 60.0);
        let view = mirror_view(0, &m, &camera, aspect, near, far).expect("in front");
        assert_eq!((view.size, view.mask), (1024, 528_383_031 & !16));
        let main = Mat4::perspective_lh(camera.fov_y_deg.to_radians(), aspect, near, far) * Eye::of(&camera).view();
        let screen = |m: Mat4, p: Vec3| {
            let c = m * p.extend(1.0);
            [0.5 + 0.5 * c.x / c.w, 0.5 - 0.5 * c.y / c.w]
        };
        // A point in the room, and its image behind the glass.
        let p = Vec3::new(-1.6, 1.2, 2.4);
        let image = reflection_matrix(reflection_plane(m.position(), m.up(), 0.0)).transform_point3(p);
        let seen = screen(main, image);
        let drawn = screen(view.view_proj, p);
        let read = reflection_uv(seen);
        assert!((read[0] - drawn[0]).abs() < 1e-4 && (read[1] - drawn[1]).abs() < 1e-4);
        // Behind the mirror's face there is no reflection to draw.
        let behind = Camera {
            eye: Vec3::new(-4.0, 1.5, 2.7),
            ..camera
        };
        assert!(mirror_view(0, &m, &behind, aspect, near, far).is_none());
    }

    #[test]
    fn the_surface_is_the_reflection_times_its_colour() {
        let c = surface_color([0.5, 2.0, 0.25], [1.0; 4], [1.0, 0.5, 0.5, 0.75]);
        assert_eq!(c, [0.5, 0.5, 0.125, 0.75]);
        assert_eq!(reflection_uv([0.25, 0.75]), [0.75, 0.75]);
    }
}
