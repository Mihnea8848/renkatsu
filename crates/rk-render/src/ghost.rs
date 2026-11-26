// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Ghosts of a character, for an editor's onion skin: its meshes in other
//! poses, each as one see-through tint, drawn into a layer of the editor's
//! own that it lays over the finished picture. The picture itself, its
//! depth and its buffers are only read.
//!
//! A ghost draws the meshes the frame drew, from their index buffers and
//! the buffers their skinning reads, skinned in the vertex shader with a
//! pose buffer of its own. Its depth is drawn first, into a depth texture
//! of the ghosts' own, and the tint only where a fragment is that depth,
//! so a ghost reads as one shell rather than as its layers. It leaves
//! itself out behind the scene's depth and wherever the character itself
//! shows.

use crate::{MeshBuffers, DEPTH_TEXTURE_FORMAT};
use glam::Mat4;
use rk_chara::DrawMesh;
use std::collections::HashMap;
use std::sync::Arc;

/// The format of the layer the ghosts are drawn into.
pub const LAYER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A mesh of a ghost: which of the frame's meshes, and its pose.
pub struct GhostMesh {
    /// The mesh by its address, and which of the renderers that share it.
    key: (usize, u32),
    /// The pose buffer, as `skin.wgsl` reads it.
    pose: Vec<u8>,
    indices: u32,
}

/// The meshes of a ghost, from a character posed as the ghost stands.
pub fn meshes(draws: &[DrawMesh]) -> Vec<GhostMesh> {
    let mut seen: HashMap<usize, u32> = HashMap::new();
    draws
        .iter()
        .filter(|d| !d.base_positions.is_empty() && !d.renderer.mesh.indices.is_empty())
        .map(|d| {
            let ptr = Arc::as_ptr(&d.renderer.mesh) as usize;
            let n = seen.entry(ptr).or_default();
            *n += 1;
            GhostMesh {
                key: (ptr, *n - 1),
                pose: crate::skin_pose(d),
                indices: d.renderer.mesh.indices.len() as u32,
            }
        })
        .collect()
}

/// A ghost: its meshes and its tint, straight colour and opacity as the
/// screen shows them.
pub struct Ghost {
    pub meshes: Vec<GhostMesh>,
    pub tint: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [f32; 16],
    tint: [f32; 4],
}

/// A ghost mesh's pose on the GPU, kept from frame to frame.
struct Slot {
    key: (usize, u32),
    /// The mesh's buffer the group was made with, to see it replaced.
    base: wgpu::Buffer,
    pose: wgpu::Buffer,
    group: wgpu::BindGroup,
}

/// A mesh to draw: its slot, its index buffer and how many indices.
type Drawn = (usize, wgpu::Buffer, u32);

/// The ghosts' pipelines and what they keep between frames.
pub(crate) struct Pass {
    frame_layout: wgpu::BindGroupLayout,
    mesh_layout: wgpu::BindGroupLayout,
    depth_pipeline: wgpu::RenderPipeline,
    tint_pipeline: wgpu::RenderPipeline,
    /// The ghosts' own depth, of the layer's size.
    depth: Option<([u32; 2], wgpu::TextureView)>,
    slots: Vec<Slot>,
    globals: Vec<wgpu::Buffer>,
}

impl Pass {
    pub(crate) fn new(device: &wgpu::Device) -> Pass {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ghost"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ghost.wgsl").into()),
        });
        let depth_texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ghost frame"),
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
                depth_texture(1),
                depth_texture(2),
            ],
        });
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let mesh_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ghost mesh"),
            entries: &[storage(0), storage(1), storage(2)],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ghost"),
            bind_group_layouts: &[Some(&frame_layout), Some(&mesh_layout)],
            immediate_size: 0,
        });
        let pipeline = |tint: bool| {
            let targets = [Some(wgpu::ColorTargetState {
                format: LAYER_FORMAT,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if tint { "ghost tint" } else { "ghost depth" }),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: tint.then(|| wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &targets,
                }),
                // Both faces: a ghost is a shell, whichever way a mesh winds.
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_TEXTURE_FORMAT,
                    depth_write_enabled: Some(!tint),
                    depth_compare: Some(if tint {
                        wgpu::CompareFunction::Equal
                    } else {
                        wgpu::CompareFunction::Less
                    }),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        Pass {
            depth_pipeline: pipeline(false),
            tint_pipeline: pipeline(true),
            frame_layout,
            mesh_layout,
            depth: None,
            slots: Vec::new(),
            globals: Vec::new(),
        }
    }

    /// Draws the ghosts into `layer`, cleared first, the first of them
    /// lowest: as a camera of `view_proj` sees them, behind `scene_depth`
    /// and left out where `own_depth` holds the character itself.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        meshes: &HashMap<(usize, u32), MeshBuffers>,
        layer: &wgpu::TextureView,
        size: [u32; 2],
        view_proj: Mat4,
        scene_depth: &wgpu::TextureView,
        own_depth: &wgpu::TextureView,
        ghosts: &[Ghost],
    ) {
        if self.depth.as_ref().is_none_or(|(s, _)| *s != size) {
            let view = device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("ghost depth"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: DEPTH_TEXTURE_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default());
            self.depth = Some((size, view));
        }
        // Each ghost's tint and each of its meshes' poses, in buffers kept
        // from the last frame where they still fit.
        let mut used = 0;
        let mut drawn: Vec<(usize, Vec<Drawn>)> = Vec::new();
        for (i, ghost) in ghosts.iter().enumerate() {
            let globals = Globals {
                view_proj: view_proj.to_cols_array(),
                tint: ghost.tint,
            };
            if self.globals.len() <= i {
                self.globals.push(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("ghost globals"),
                    size: std::mem::size_of::<Globals>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            queue.write_buffer(&self.globals[i], 0, bytemuck::bytes_of(&globals));
            let mut list = Vec::new();
            for mesh in &ghost.meshes {
                let Some(m) = meshes.get(&mesh.key).or_else(|| meshes.get(&(mesh.key.0, 0))) else {
                    continue;
                };
                let kept = self
                    .slots
                    .get(used)
                    .is_some_and(|s| s.key == mesh.key && s.base == m.base && s.pose.size() == mesh.pose.len() as u64);
                if kept {
                    queue.write_buffer(&self.slots[used].pose, 0, &mesh.pose);
                } else {
                    use wgpu::util::DeviceExt;
                    let pose = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("ghost pose"),
                        contents: &mesh.pose,
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    });
                    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("ghost mesh"),
                        layout: &self.mesh_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: pose.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: m.base.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: m.skin.as_entire_binding(),
                            },
                        ],
                    });
                    let slot = Slot {
                        key: mesh.key,
                        base: m.base.clone(),
                        pose,
                        group,
                    };
                    if used < self.slots.len() {
                        self.slots[used] = slot;
                    } else {
                        self.slots.push(slot);
                    }
                }
                list.push((used, m.ib.clone(), mesh.indices));
                used += 1;
            }
            drawn.push((i, list));
        }
        self.slots.truncate(used);
        self.globals.truncate(ghosts.len());
        let depth = &self.depth.as_ref().expect("made above").1;
        let mut encoder = device.create_command_encoder(&Default::default());
        for (n, (i, list)) in drawn.iter().enumerate() {
            let frame = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ghost frame"),
                layout: &self.frame_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.globals[*i].as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(scene_depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(own_depth),
                    },
                ],
            });
            // The ghost's depth alone, then its tint where it is that depth.
            for tint in [false, true] {
                let color = [Some(wgpu::RenderPassColorAttachment {
                    view: layer,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if n == 0 {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })];
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ghost"),
                    color_attachments: if tint { &color } else { &[] },
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth,
                        depth_ops: Some(wgpu::Operations {
                            load: if tint {
                                wgpu::LoadOp::Load
                            } else {
                                wgpu::LoadOp::Clear(1.0)
                            },
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(if tint {
                    &self.tint_pipeline
                } else {
                    &self.depth_pipeline
                });
                pass.set_bind_group(0, &frame, &[]);
                for (slot, ib, indices) in list {
                    pass.set_bind_group(1, &self.slots[*slot].group, &[]);
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..*indices, 0, 0..1);
                }
            }
        }
        if drawn.is_empty() {
            // Nothing to draw leaves an empty layer.
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ghost"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: layer,
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
        }
        queue.submit([encoder.finish()]);
    }
}
