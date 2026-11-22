// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The outline drawn around what an editor has selected: the selected
//! meshes are drawn into a depth texture of their own with their shaders'
//! caster pass, so cut-out hair and lace keep their shape, and the pixels
//! just outside that silhouette are painted over the finished picture.

use wgpu::util::DeviceExt;

/// What to outline, and how.
#[derive(Debug, Clone, PartialEq)]
pub struct Highlight {
    /// The places of the outlined meshes in the draw list.
    pub draws: Vec<usize>,
    /// The outlined objects of the map, by their place in
    /// `MapScene::objects`, and copies of them, by their place in the list
    /// `Renderer::set_map_copies` was given.
    pub map_objects: Vec<usize>,
    pub map_copies: Vec<usize>,
    /// The outline's colour, straight (not premultiplied) and linear.
    pub color: [f32; 4],
    /// The outline's width in pixels.
    pub width: f32,
}

/// The pipeline that paints the outline.
pub(crate) struct Pass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    color: [f32; 4],
    width: [f32; 4],
}

impl Pass {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Pass {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("highlight"),
            source: wgpu::ShaderSource::Wgsl(include_str!("highlight.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("highlight"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("highlight"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("highlight"),
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
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Pass { pipeline, layout }
    }

    /// Paints the outline of the silhouette in `mask` onto `target`.
    pub(crate) fn paint(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        mask: &wgpu::TextureView,
        target: &wgpu::TextureView,
        highlight: &Highlight,
    ) {
        let params = Params {
            color: highlight.color,
            width: [highlight.width.max(0.5), 0.0, 0.0, 0.0],
        };
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("highlight"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("highlight"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(mask),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });
        let mut pass = crate::pass::Pass::new(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("highlight"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}
