// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Draws the grid on the ground, at height 0, into a see-through layer of
//! the viewport's size that egui lays over the scene's picture. Each pixel
//! finds its place on the ground, draws the lines that pass there and
//! leaves itself out where the scene's depth says something stands in
//! front, so the grid lies under the characters without being part of
//! their picture.

use eframe::egui_wgpu::{self, wgpu};
use glam::{Mat4, Vec3};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// How the grid looks: the size of a cell in metres, how far from the
/// camera it reaches, and the colours of its lines and of the two axes
/// that lie on the ground, as the screen shows them, with their opacity.
pub struct Grid {
    pub cell: f32,
    pub reach: f32,
    pub line: [f32; 4],
    pub axis_x: [f32; 4],
    pub axis_z: [f32; 4],
}

const SHADER: &str = r"
struct Globals {
    view_proj: mat4x4<f32>,
    inverse: mat4x4<f32>,
    eye: vec4<f32>,
    // The cell, the reach, and the size of the layer in pixels.
    params: vec4<f32>,
    line: vec4<f32>,
    axis_x: vec4<f32>,
    axis_z: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var scene_depth: texture_depth_2d;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

// How much of a pixel the lines of a grid of `cell` metres cover. The
// lines along each axis fade out by themselves where they come too close
// together to be told apart.
fn lines(at: vec2<f32>, cell: f32) -> f32 {
    let p = at / cell;
    let w = max(fwidth(p), vec2<f32>(1e-6));
    let cover = 1.0 - min(abs(fract(p - 0.5) - 0.5) / w, vec2<f32>(1.0));
    let apart = 1.0 - smoothstep(vec2<f32>(0.1), vec2<f32>(0.4), w);
    return max(cover.x * apart.x, cover.y * apart.y);
}

@fragment
fn fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let ndc = vec2<f32>(frag.x / g.params.z * 2.0 - 1.0, 1.0 - frag.y / g.params.w * 2.0);
    let far = g.inverse * vec4<f32>(ndc, 1.0, 1.0);
    let dir = normalize(far.xyz / far.w - g.eye.xyz);
    if (abs(dir.y) < 1e-5) { discard; }
    let t = -g.eye.y / dir.y;
    if (t <= 0.0) { discard; }
    let world = g.eye.xyz + dir * t;

    // Behind what the scene drew there.
    let clip = g.view_proj * vec4<f32>(world, 1.0);
    let depth = clip.z / clip.w;
    let size = vec2<i32>(textureDimensions(scene_depth));
    let pixel = clamp(vec2<i32>(frag.xy), vec2<i32>(0), size - vec2<i32>(1));
    let drawn = textureLoad(scene_depth, pixel, 0);
    if (drawn < 1.0 && drawn < depth - 1e-5) { discard; }

    let fine = lines(world.xz, g.params.x);
    let coarse = lines(world.xz, g.params.x * 10.0);
    var color = g.line.rgb;
    var alpha = max(fine * 0.5, coarse) * g.line.a;

    // The X axis runs where z is 0, the Z axis where x is 0.
    let w = max(fwidth(world.xz), vec2<f32>(1e-6));
    let on = 1.0 - min(abs(world.xz) / w, vec2<f32>(1.0));
    if (on.y > 0.0) {
        color = g.axis_x.rgb;
        alpha = max(alpha, on.y * g.axis_x.a);
    }
    if (on.x > 0.0) {
        color = g.axis_z.rgb;
        alpha = max(alpha, on.x * g.axis_z.a);
    }

    // It fades out with the distance from the camera.
    alpha = alpha * (1.0 - smoothstep(g.params.y * 0.35, g.params.y, t));
    return vec4<f32>(color * alpha, alpha);
}
";

struct Layer {
    size: [u32; 2],
    view: wgpu::TextureView,
    id: egui::TextureId,
}

pub struct GridPass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    globals: wgpu::Buffer,
    layer: Option<Layer>,
}

impl GridPass {
    pub fn new(device: &wgpu::Device) -> GridPass {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grid"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grid"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
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
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("grid"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("grid"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(FORMAT.into())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grid globals"),
            size: 52 * 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        GridPass {
            pipeline,
            layout,
            globals,
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
            let view = device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("grid layer"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default());
            let mut egui_renderer = render_state.renderer.write();
            let id = match &self.layer {
                Some(old) => {
                    egui_renderer.update_egui_texture_from_wgpu_texture(
                        device,
                        &view,
                        wgpu::FilterMode::Linear,
                        old.id,
                    );
                    old.id
                }
                None => egui_renderer.register_native_texture(device, &view, wgpu::FilterMode::Linear),
            };
            self.layer = Some(Layer { size, view, id });
        }
        self.layer.as_ref().expect("made above")
    }

    /// Draws the grid into the layer, of `size` pixels, as a camera at
    /// `eye` sees it, behind what `depth` holds of the scene. Returns the
    /// id egui shows the layer under.
    pub fn paint(
        &mut self,
        render_state: &egui_wgpu::RenderState,
        size: [u32; 2],
        view_proj: Mat4,
        eye: Vec3,
        depth: &wgpu::TextureView,
        grid: &Grid,
    ) -> egui::TextureId {
        let (device, queue) = (&render_state.device, &render_state.queue);
        let mut globals = view_proj.to_cols_array().to_vec();
        globals.extend(view_proj.inverse().to_cols_array());
        globals.extend(eye.extend(1.0).to_array());
        globals.extend([grid.cell.max(1e-3), grid.reach, size[0] as f32, size[1] as f32]);
        globals.extend(grid.line);
        globals.extend(grid.axis_x);
        globals.extend(grid.axis_z);
        let bytes: Vec<u8> = globals.iter().flat_map(|n| n.to_ne_bytes()).collect();
        queue.write_buffer(&self.globals, 0, &bytes);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grid"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
            ],
        });
        let pipeline = self.pipeline.clone();
        let layer = self.layer(render_state, size);
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("grid"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &layer.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
        layer.id
    }
}
