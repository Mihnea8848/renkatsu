// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Draws the transform gizmo's solids on the GPU, into a see-through
//! layer of the viewport's size that egui lays over the scene's picture.
//! A layer of its own lets the gizmo show through whatever stands in
//! front of it, hide its own far parts behind its near ones, and have
//! smooth edges by multisampling without touching the scene's picture.

use eframe::egui_wgpu::{self, wgpu};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;

/// A corner of a triangle: where it is in the world, which way its
/// surface faces (zero for a flat colour without shading) and its colour
/// as the screen shows it, with its opacity.
pub type Vertex = [f32; 10];

/// The triangles of a gizmo, three corners each: the solid parts, and
/// the see-through parts that are drawn over them.
#[derive(Default)]
pub struct Mesh {
    pub solid: Vec<Vertex>,
    pub glass: Vec<Vertex>,
}

const SHADER: &str = r"
struct Globals { view_proj: mat4x4<f32>, eye: vec4<f32> };
@group(0) @binding(0) var<uniform> g: Globals;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) world: vec3<f32>,
};

@vertex
fn vs(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) color: vec4<f32>) -> Out {
    var out: Out;
    out.clip = g.view_proj * vec4<f32>(pos, 1.0);
    out.color = color;
    out.normal = normal;
    out.world = pos;
    return out;
}

// A surface is a little darker where it turns away from the viewer,
// which is enough to read a cone or a cube as a solid.
@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    var rgb = in.color.rgb;
    if (dot(in.normal, in.normal) > 0.0) {
        let facing = abs(dot(normalize(in.normal), normalize(g.eye.xyz - in.world)));
        rgb = rgb * (0.72 + 0.28 * facing);
    }
    return vec4<f32>(rgb * in.color.a, in.color.a);
}
";

struct Layer {
    size: [u32; 2],
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolved: wgpu::TextureView,
    id: egui::TextureId,
}

pub struct GizmoPass {
    solid: wgpu::RenderPipeline,
    glass: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    group: wgpu::BindGroup,
    layer: Option<Layer>,
}

impl GizmoPass {
    pub fn new(device: &wgpu::Device) -> GizmoPass {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gizmo"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gizmo"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gizmo"),
            bind_group_layouts: &[Some(&group_layout)],
            immediate_size: 0,
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gizmo globals"),
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gizmo"),
            layout: &group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        // The see-through parts are tested against the solid ones but do
        // not hide each other.
        let pipeline = |label: &str, depth_write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4],
                    })],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview_mask: None,
                cache: None,
            })
        };
        GizmoPass {
            solid: pipeline("gizmo solid", true),
            glass: pipeline("gizmo glass", false),
            globals,
            group,
            layer: None,
        }
    }

    /// Gives back the picture egui was shown.
    pub fn release(&mut self, render_state: &egui_wgpu::RenderState) {
        if let Some(layer) = self.layer.take() {
            render_state.renderer.write().free_texture(&layer.id);
        }
    }

    fn layer(&mut self, render_state: &egui_wgpu::RenderState, size: [u32; 2]) -> &Layer {
        if self.layer.as_ref().is_none_or(|l| l.size != size) {
            let device = &render_state.device;
            let texture = |label, format, sample_count, usage| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d {
                            width: size[0],
                            height: size[1],
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
            let resolved = texture(
                "gizmo layer",
                FORMAT,
                1,
                attachment | wgpu::TextureUsages::TEXTURE_BINDING,
            );
            let mut egui_renderer = render_state.renderer.write();
            let id = match &self.layer {
                Some(old) => {
                    egui_renderer.update_egui_texture_from_wgpu_texture(
                        device,
                        &resolved,
                        wgpu::FilterMode::Linear,
                        old.id,
                    );
                    old.id
                }
                None => egui_renderer.register_native_texture(device, &resolved, wgpu::FilterMode::Linear),
            };
            self.layer = Some(Layer {
                size,
                color: texture("gizmo colour", FORMAT, SAMPLES, attachment),
                depth: texture("gizmo depth", DEPTH, SAMPLES, attachment),
                resolved,
                id,
            });
        }
        self.layer.as_ref().expect("made above")
    }

    /// Draws a gizmo into the layer, of `size` pixels, and returns the id
    /// egui shows the layer under.
    pub fn paint(
        &mut self,
        render_state: &egui_wgpu::RenderState,
        size: [u32; 2],
        view_proj: Mat4,
        eye: Vec3,
        mesh: &Mesh,
    ) -> egui::TextureId {
        let (device, queue) = (&render_state.device, &render_state.queue);
        let mut globals = view_proj.to_cols_array().to_vec();
        globals.extend(eye.extend(1.0).to_array());
        queue.write_buffer(&self.globals, 0, &bytes(&globals));
        let buffer = |vertices: &[Vertex]| {
            (!vertices.is_empty()).then(|| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("gizmo vertices"),
                    contents: &bytes(vertices.as_flattened()),
                    usage: wgpu::BufferUsages::VERTEX,
                })
            })
        };
        let (solid, glass) = (buffer(&mesh.solid), buffer(&mesh.glass));
        let mut encoder = device.create_command_encoder(&Default::default());
        let (solid_pipeline, glass_pipeline, group) = (self.solid.clone(), self.glass.clone(), self.group.clone());
        let layer = self.layer(render_state, size);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gizmo"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &layer.color,
                    depth_slice: None,
                    resolve_target: Some(&layer.resolved),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &layer.depth,
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
            pass.set_bind_group(0, &group, &[]);
            for (pipeline, buffer, vertices) in [
                (&solid_pipeline, &solid, &mesh.solid),
                (&glass_pipeline, &glass, &mesh.glass),
            ] {
                if let Some(buffer) = buffer {
                    pass.set_pipeline(pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..vertices.len() as u32, 0..1);
                }
            }
        }
        queue.submit([encoder.finish()]);
        layer.id
    }
}

fn bytes(numbers: &[f32]) -> Vec<u8> {
    numbers.iter().flat_map(|n| n.to_ne_bytes()).collect()
}
