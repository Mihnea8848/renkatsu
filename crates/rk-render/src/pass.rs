// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! A render pass that leaves out the settings already in place. wgpu
//! checks, tracks and records every pipeline, bind group and buffer it is
//! given, even the one it already has, and a frame sets the same few
//! hundreds of times: a map's pieces share materials and buffers, and its
//! shadow casters are drawn once per cascade. What is drawn is the same;
//! only the repeats are not sent.
//!
//! The wrapped pass is not handed out, so nothing can change its state
//! behind the record kept here.

use std::ops::Range;

/// A vertex or index buffer as set: the buffer and the bytes of it.
struct Bound {
    buffer: wgpu::Buffer,
    offset: wgpu::BufferAddress,
    size: wgpu::BufferAddress,
}

impl Bound {
    fn of(slice: &wgpu::BufferSlice<'_>) -> Bound {
        Bound {
            buffer: slice.buffer().clone(),
            offset: slice.offset(),
            size: slice.size(),
        }
    }

    /// Whether `slice` is this, without taking a handle to compare.
    fn is(&self, slice: &wgpu::BufferSlice<'_>) -> bool {
        self.offset == slice.offset() && self.size == slice.size() && self.buffer == *slice.buffer()
    }
}

/// Bind group slots a pass keeps; the renderer's layouts use fewer.
const GROUPS: usize = 4;
/// Vertex buffer slots a pass keeps.
const VERTEX: usize = 2;

pub(crate) struct Pass<'a> {
    pass: wgpu::RenderPass<'a>,
    /// What is in place. Each holds a handle, so a setting dropped and made
    /// again is never taken for the one in place.
    pipeline: Option<wgpu::RenderPipeline>,
    groups: [Option<wgpu::BindGroup>; GROUPS],
    vertex: [Option<Bound>; VERTEX],
    index: Option<(Bound, wgpu::IndexFormat)>,
}

impl<'a> Pass<'a> {
    pub(crate) fn new(pass: wgpu::RenderPass<'a>) -> Pass<'a> {
        Pass {
            pass,
            pipeline: None,
            groups: Default::default(),
            vertex: Default::default(),
            index: None,
        }
    }

    pub(crate) fn set_pipeline(&mut self, pipeline: &wgpu::RenderPipeline) {
        if self.pipeline.as_ref() != Some(pipeline) {
            self.pass.set_pipeline(pipeline);
            self.pipeline = Some(pipeline.clone());
        }
    }

    pub(crate) fn set_bind_group(&mut self, index: u32, group: &wgpu::BindGroup, offsets: &[wgpu::DynamicOffset]) {
        let slot = self.groups.get_mut(index as usize);
        // A group with dynamic offsets is set every time: the offsets are
        // not kept.
        match slot {
            Some(slot) if offsets.is_empty() => {
                if slot.as_ref() != Some(group) {
                    self.pass.set_bind_group(index, group, offsets);
                    *slot = Some(group.clone());
                }
            }
            Some(slot) => {
                self.pass.set_bind_group(index, group, offsets);
                *slot = None;
            }
            None => self.pass.set_bind_group(index, group, offsets),
        }
    }

    pub(crate) fn set_vertex_buffer(&mut self, slot: u32, buffer: wgpu::BufferSlice<'_>) {
        match self.vertex.get_mut(slot as usize) {
            Some(kept) => {
                if !kept.as_ref().is_some_and(|k| k.is(&buffer)) {
                    *kept = Some(Bound::of(&buffer));
                    self.pass.set_vertex_buffer(slot, buffer);
                }
            }
            None => self.pass.set_vertex_buffer(slot, buffer),
        }
    }

    pub(crate) fn set_index_buffer(&mut self, buffer: wgpu::BufferSlice<'_>, format: wgpu::IndexFormat) {
        if !self.index.as_ref().is_some_and(|(k, f)| *f == format && k.is(&buffer)) {
            self.index = Some((Bound::of(&buffer), format));
            self.pass.set_index_buffer(buffer, format);
        }
    }

    pub(crate) fn set_viewport(&mut self, x: f32, y: f32, w: f32, h: f32, min_depth: f32, max_depth: f32) {
        self.pass.set_viewport(x, y, w, h, min_depth, max_depth);
    }

    pub(crate) fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        self.pass.set_scissor_rect(x, y, width, height);
    }

    pub(crate) fn set_stencil_reference(&mut self, reference: u32) {
        self.pass.set_stencil_reference(reference);
    }

    pub(crate) fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.pass.draw(vertices, instances);
    }

    pub(crate) fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        self.pass.draw_indexed(indices, base_vertex, instances);
    }
}
