// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The cloth solver for skirts and dresses on the GPU: runs the frames
//! `rk_chara::physics::drape` makes with the compute passes of
//! `drape.wgsl`, keeps the cloth there from frame to frame, and puts the
//! garments' vertices on it after skinning. Nothing comes back to the
//! CPU to be drawn; `Gpu::read` is for tests and measurements.

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use rk_chara::physics::drape::{
    Begin, Frame, Setup, FASTEST, HINGES, LINKS, MAP_PLANES, MOST_SUBSTEPS, NONE, OWN, PRESSED_FADE,
};
use rk_chara::physics::shell::ANGLES;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

/// The cloth read back: each point's place with how much it is pressed,
/// and its speed.
pub type Cloth = (Vec<[f32; 4]>, Vec<[f32; 4]>);

/// One substep's numbers, as `Sub` in `drape.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Sub {
    floor: [f32; 4],
    carry: [[f32; 4]; 4],
    h: f32,
    damp: f32,
    fade: f32,
    reach: f32,
    boundary: u32,
    slots: u32,
    shells: u32,
    flags: u32,
    points: u32,
    triangles: u32,
    friction: [f32; 2],
    thickness: f32,
    keep: f32,
    own_reach: f32,
    lift: f32,
    calm: f32,
    pad: [f32; 3],
}

const ARMS: u32 = 1;
const FLOOR: u32 = 2;
const MAP: u32 = 4;
const RESET: u32 = 8;
const CARRY: u32 = 16;

/// A point, as `Point` in `drape.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Point {
    rest: [f32; 3],
    gap: f32,
    gap_out: f32,
    allow: f32,
    tether: f32,
    inner: f32,
    anchor: u32,
    bits: u32,
    turn: f32,
    pad: u32,
    around: [u32; 4],
    grid: [u32; 4],
    prop: [f32; 4],
}

/// A bound vertex, as `Bound` in `drape.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Bound {
    at: [u32; 4],
    bary: [f32; 4],
    place: [f32; 4],
    normal: [f32; 4],
    tangent: [f32; 4],
}

/// The space one `Sub` takes in the uniform buffer: the least a bound
/// range may be offset by.
const SUB_STRIDE: u64 = 256;
/// The slot of the pass that dresses the meshes, after the substeps'.
const DRESS_SLOT: u64 = MOST_SUBSTEPS as u64;
/// Floats per shell before the thicknesses, as `SHELL` in the shader.
const SHELL: usize = 20;

/// The passes of `drape.wgsl`, made once per device.
pub struct Pipelines {
    begin: (wgpu::ComputePipeline, wgpu::BindGroupLayout),
    find: (wgpu::ComputePipeline, wgpu::BindGroupLayout),
    predict: (wgpu::ComputePipeline, wgpu::BindGroupLayout),
    solve: (wgpu::ComputePipeline, wgpu::BindGroupLayout),
    frames: (wgpu::ComputePipeline, wgpu::BindGroupLayout),
    dress: (wgpu::ComputePipeline, wgpu::BindGroupLayout),
}

/// The bindings each pass uses: their numbers in `drape.wgsl` and
/// whether the shader declares them written. Binding 0 is the substep's
/// numbers.
const BEGIN: &[(u32, bool)] = &[(1, true), (2, true), (4, false), (7, false)];
const FIND: &[(u32, bool)] = &[(1, true), (4, false), (8, true), (9, false)];
const PREDICT: &[(u32, bool)] = &[(1, true), (2, true), (3, true), (4, false), (5, false), (7, false)];
const SOLVE: &[(u32, bool)] = &[
    (1, true),
    (2, true),
    (3, true),
    (4, false),
    (5, false),
    (6, false),
    (7, false),
    (8, true),
];
const FRAMES: &[(u32, bool)] = &[(1, true), (4, false), (10, true)];
const DRESS: &[(u32, bool)] = &[(6, false), (7, false), (10, true), (11, false), (12, true)];

impl Pipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("drape"),
            source: wgpu::ShaderSource::Wgsl(include_str!("drape.wgsl").into()),
        });
        let make = |entry: &str, bindings: &[(u32, bool)]| {
            let mut entries = vec![wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Sub>() as u64),
                },
                count: None,
            }];
            entries.extend(bindings.iter().map(|&(binding, written)| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: !written },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }));
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(entry),
                entries: &entries,
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(entry),
                    bind_group_layouts: &[Some(&layout)],
                    immediate_size: 0,
                })),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            });
            (pipeline, layout)
        };
        Pipelines {
            begin: make("cs_begin", BEGIN),
            find: make("cs_find", FIND),
            predict: make("cs_predict", PREDICT),
            solve: make("cs_solve", SOLVE),
            frames: make("cs_frames", FRAMES),
            dress: make("cs_dress", DRESS),
        }
    }
}

/// One character's cloth on the GPU.
pub struct Gpu {
    points: u32,
    slots: u32,
    shells: u32,
    subs: wgpu::Buffer,
    /// By binding number in `drape.wgsl`, from 1.
    buffers: HashMap<u32, wgpu::Buffer>,
    begin: wgpu::BindGroup,
    find: wgpu::BindGroup,
    predict: wgpu::BindGroup,
    solve: wgpu::BindGroup,
    frames: wgpu::BindGroup,
    /// The map triangles uploaded, by the address of their list.
    map: usize,
    /// The bound vertices of each binding, once a mesh was dressed.
    bound: HashMap<usize, wgpu::Buffer>,
    /// Whether a frame ran: before that there is nothing to dress with.
    pub ran: bool,
    /// The last frame that ran.
    last: Option<Frame>,
}

impl Gpu {
    pub fn new(device: &wgpu::Device, pipelines: &Pipelines, setup: &Setup) -> Self {
        let n = setup.points.len();
        let points: Vec<Point> = setup
            .points
            .iter()
            .map(|p| Point {
                rest: p.rest.to_array(),
                gap: p.gap,
                gap_out: p.gap_out,
                allow: p.allow,
                tether: p.tether,
                inner: p.inner,
                anchor: p.anchor,
                bits: u32::from(p.fixed) | u32::from(p.cell) << 1 | (p.net << 8),
                turn: p.turn,
                pad: 0,
                around: p.around,
                grid: [p.grid[0], p.grid[1], p.grid[2], 0],
                prop: [p.prop.x, p.prop.y, p.prop.z, 0.0],
            })
            .collect();
        let mut topo: Vec<[u32; 4]> = Vec::with_capacity(n * (LINKS + HINGES * 2));
        for i in 0..n {
            for l in &setup.links[i * LINKS..(i + 1) * LINKS] {
                topo.push([l.to, l.rest.to_bits(), l.soft.to_bits(), l.buckle.to_bits()]);
            }
            for h in &setup.hinges[i * HINGES..(i + 1) * HINGES] {
                topo.push([h.a, h.b, h.rest.to_bits(), h.soft.to_bits()]);
                topo.push([h.axis[0], h.axis[1], h.side.to_bits(), 0]);
            }
        }
        // The shells: 20 floats each, then their thicknesses.
        let mut body = vec![0.0f32; setup.shells.len() * SHELL];
        for (k, s) in setup.shells.iter().enumerate() {
            let first = body.len();
            body.extend_from_slice(&s.radii);
            let ends = s.ends();
            let flags = u32::from(s.caps[0]) | u32::from(s.caps[1]) << 1 | u32::from(s.outside) << 2;
            let rows = [
                s.a.extend(s.stations as f32),
                s.b.extend(first as f32),
                s.e1.extend(flags as f32),
                s.e2.extend(0.0),
                glam::Vec4::new(ends[0], ends[1], 0.0, 0.0),
            ];
            for (r, row) in rows.iter().enumerate() {
                body[k * SHELL + r * 4..k * SHELL + r * 4 + 4].copy_from_slice(&row.to_array());
            }
            debug_assert_eq!(s.radii.len(), s.stations * ANGLES);
        }
        if body.is_empty() {
            body.push(0.0);
        }
        let slots = setup.slots().max(1);
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
        let filled = |label: &str, contents: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage: storage,
            })
        };
        let empty = |label: &str, size: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.max(16) as u64,
                usage: storage,
                mapped_at_creation: false,
            })
        };
        let mut buffers = HashMap::new();
        buffers.insert(1, empty("drape pos", n * 16));
        buffers.insert(2, empty("drape vel", n * 16));
        buffers.insert(3, empty("drape next", n * 32));
        buffers.insert(4, filled("drape points", bytemuck::cast_slice(&points)));
        buffers.insert(5, filled("drape topo", bytemuck::cast_slice(&topo)));
        buffers.insert(6, filled("drape body", bytemuck::cast_slice(&body)));
        buffers.insert(7, empty("drape poses", (MOST_SUBSTEPS as usize + 1) * slots * 128));
        // The map's planes and, as one more, its own cloth's triangles.
        const _: () = assert!(OWN == 4);
        buffers.insert(8, empty("drape planes", n * (MAP_PLANES + 1) * 16));
        buffers.insert(9, empty("drape map", 48));
        buffers.insert(10, empty("drape frames", n * 64));
        let subs = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("drape substeps"),
            size: (DRESS_SLOT + 1) * SUB_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = |pass: &(wgpu::ComputePipeline, wgpu::BindGroupLayout), bindings: &[(u32, bool)]| {
            group(device, &pass.1, &subs, bindings, &buffers, &[])
        };
        Gpu {
            points: n as u32,
            slots: slots as u32,
            shells: setup.shells.len() as u32,
            begin: group(&pipelines.begin, BEGIN),
            find: group(&pipelines.find, FIND),
            predict: group(&pipelines.predict, PREDICT),
            solve: group(&pipelines.solve, SOLVE),
            frames: group(&pipelines.frames, FRAMES),
            subs,
            buffers,
            map: 0,
            bound: HashMap::new(),
            ran: false,
            last: None,
        }
    }

    /// Runs one frame: its passes are submitted and not waited for.
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipelines: &Pipelines,
        setup: &Setup,
        frame: &Frame,
    ) {
        // A frame of no substeps only moves the cloth with its character.
        let only_begin = frame.substeps == 0;
        let subs = frame.substeps.clamp(1, MOST_SUBSTEPS);
        let h = if only_begin { 0.0 } else { frame.dt / subs as f32 };
        // The map's triangles, when they are others than last time.
        let mut flags = 0;
        let mut triangles = 0;
        if let Some(map) = &frame.map {
            let from = std::sync::Arc::as_ptr(map) as usize;
            if from != self.map {
                let corners: Vec<[f32; 4]> = map.iter().flatten().map(|c| c.extend(0.0).to_array()).collect();
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("drape map"),
                    contents: bytemuck::cast_slice(&corners),
                    usage: wgpu::BufferUsages::STORAGE,
                });
                self.buffers.insert(9, buffer);
                self.find = group(device, &pipelines.find.1, &self.subs, FIND, &self.buffers, &[]);
                self.map = from;
            }
            flags |= MAP;
            triangles = map.len() as u32;
        }
        let poses: Vec<[[[f32; 4]; 4]; 2]> = frame
            .poses
            .iter()
            .map(|m| [m.to_cols_array_2d(), m.inverse().to_cols_array_2d()])
            .collect();
        queue.write_buffer(&self.buffers[&7], 0, bytemuck::cast_slice(&poses));
        let (mut floor, mut carry) = ([0.0; 4], Mat4::IDENTITY);
        if let Some((at, up)) = frame.floor {
            floor = up.extend(at.dot(up)).to_array();
            flags |= FLOOR;
        }
        if frame.arms {
            flags |= ARMS;
        }
        match frame.begin {
            Begin::Keep => {}
            Begin::Reset => flags |= RESET,
            Begin::Carry(m) => {
                carry = m;
                flags |= CARRY;
            }
        }
        let material = &setup.material;
        let mut sub = Sub {
            floor,
            carry: carry.to_cols_array_2d(),
            h,
            damp: (-material.damping * h).exp(),
            fade: PRESSED_FADE.powf(h * 60.0),
            reach: FASTEST * frame.dt + material.thickness,
            boundary: 0,
            slots: self.slots,
            shells: self.shells,
            flags,
            points: self.points,
            triangles,
            friction: material.friction,
            thickness: material.thickness,
            keep: (-material.memory * h).exp(),
            own_reach: 2.0 * FASTEST * frame.dt + material.thickness,
            lift: frame.lift,
            calm: 1.0 - (-material.calm * h).exp(),
            pad: [0.0; 3],
        };
        let mut bytes = vec![0u8; (subs as u64 * SUB_STRIDE) as usize];
        for s in 0..subs {
            sub.boundary = s;
            let at = (s as u64 * SUB_STRIDE) as usize;
            bytes[at..at + std::mem::size_of::<Sub>()].copy_from_slice(bytemuck::bytes_of(&sub));
        }
        queue.write_buffer(&self.subs, 0, &bytes);
        // The pass that dresses reads the body as the frame leaves it.
        sub.boundary = if only_begin { 0 } else { subs };
        queue.write_buffer(&self.subs, DRESS_SLOT * SUB_STRIDE, bytemuck::bytes_of(&sub));
        if !only_begin {
            self.last = Some(frame.clone());
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("drape"),
                timestamp_writes: None,
            });
            let groups = self.points.div_ceil(64);
            let mut go = |p: &(wgpu::ComputePipeline, wgpu::BindGroupLayout), group: &wgpu::BindGroup, slot: u32| {
                pass.set_pipeline(&p.0);
                pass.set_bind_group(0, group, &[slot * SUB_STRIDE as u32]);
                pass.dispatch_workgroups(groups, 1, 1);
            };
            if flags & (RESET | CARRY) != 0 {
                go(&pipelines.begin, &self.begin, 0);
            }
            if !only_begin {
                go(&pipelines.find, &self.find, 0);
                for s in 0..subs {
                    go(&pipelines.predict, &self.predict, s);
                    go(&pipelines.solve, &self.solve, s);
                }
            }
            go(&pipelines.frames, &self.frames, 0);
        }
        queue.submit([encoder.finish()]);
        self.ran = true;
    }

    /// The last frame again with the body still where the frame left
    /// it, cut into `substeps` (0 for as many as it had): for
    /// measuring what a frame costs.
    pub fn still(&self, setup: &Setup, substeps: u32) -> Option<Frame> {
        let last = self.last.as_ref()?;
        let slots = setup.slots();
        let subs = if substeps == 0 {
            last.substeps
        } else {
            substeps.min(MOST_SUBSTEPS)
        };
        let end = &last.poses[last.substeps as usize * slots..];
        Some(Frame {
            substeps: subs,
            poses: end.iter().copied().cycle().take((subs as usize + 1) * slots).collect(),
            begin: Begin::Keep,
            ..last.clone()
        })
    }

    /// The bind group that dresses a mesh: its vertex buffer `vb` with
    /// the vertices of `setup.bindings[binding]`, and how many they are.
    pub fn dress(
        &mut self,
        device: &wgpu::Device,
        pipelines: &Pipelines,
        setup: &Setup,
        binding: usize,
        vb: &wgpu::Buffer,
    ) -> (wgpu::BindGroup, u32) {
        let vertices = &setup.bindings[binding].vertices;
        let bound = self.bound.entry(binding).or_insert_with(|| {
            let mut all: Vec<Bound> = vertices
                .iter()
                .map(|b| {
                    let first = setup.nets[b.net as usize].first as u32;
                    Bound {
                        at: [
                            b.vertex,
                            first + b.corners[0],
                            first + b.corners[1],
                            first + b.corners[2],
                        ],
                        bary: [b.bary[0], b.bary[1], b.bary[2], b.weight],
                        place: b.local[0].extend(0.0).to_array(),
                        normal: b.local[1].extend(0.0).to_array(),
                        tangent: b.local[2].extend(0.0).to_array(),
                    }
                })
                .collect();
            if all.is_empty() {
                all.push(Bound::zeroed());
            }
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("drape bound"),
                contents: bytemuck::cast_slice(&all),
                usage: wgpu::BufferUsages::STORAGE,
            })
        });
        let extra = [(11, &*bound), (12, vb)];
        (
            group(device, &pipelines.dress.1, &self.subs, DRESS, &self.buffers, &extra),
            vertices.len() as u32,
        )
    }

    /// Dispatches the pass that dresses one mesh.
    pub fn dress_pass(pass: &mut wgpu::ComputePass, pipelines: &Pipelines, group: &wgpu::BindGroup, count: u32) {
        pass.set_pipeline(&pipelines.dress.0);
        pass.set_bind_group(0, group, &[(DRESS_SLOT * SUB_STRIDE) as u32]);
        pass.dispatch_workgroups(count.div_ceil(64), 1, 1);
    }

    /// The cloth as it is now: each point's place with how much it is
    /// pressed, and its speed. Waits for the GPU; for tests and
    /// measurements.
    pub fn read(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Cloth> {
        let size = self.points as u64 * 16;
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("drape read"),
            size: size.max(16) * 2,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.buffers[&1], 0, &staging, 0, size);
        encoder.copy_buffer_to_buffer(&self.buffers[&2], 0, &staging, size, size);
        queue.submit([encoder.finish()]);
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| drop(tx.send(r)));
        device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;
        let data = slice.get_mapped_range()?;
        let all: &[[f32; 4]] = bytemuck::cast_slice(&data[..(size * 2) as usize]);
        let n = self.points as usize;
        Ok((all[..n].to_vec(), all[n..].to_vec()))
    }

    /// Puts the cloth where `places` and `speeds` say; for tests.
    pub fn write(&self, queue: &wgpu::Queue, places: &[[f32; 4]], speeds: &[[f32; 4]]) {
        queue.write_buffer(&self.buffers[&1], 0, bytemuck::cast_slice(places));
        queue.write_buffer(&self.buffers[&2], 0, bytemuck::cast_slice(speeds));
    }
}

/// A bind group for a pass: the substeps' numbers at 0 and each of
/// `bindings` from `buffers`, or from `extra` where it is there.
fn group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    subs: &wgpu::Buffer,
    bindings: &[(u32, bool)],
    buffers: &HashMap<u32, wgpu::Buffer>,
    extra: &[(u32, &wgpu::Buffer)],
) -> wgpu::BindGroup {
    let mut entries = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: subs,
            offset: 0,
            size: wgpu::BufferSize::new(std::mem::size_of::<Sub>() as u64),
        }),
    }];
    for &(binding, _) in bindings {
        let buffer = extra
            .iter()
            .find(|(b, _)| *b == binding)
            .map_or_else(|| &buffers[&binding], |(_, buffer)| *buffer);
        entries.push(wgpu::BindGroupEntry {
            binding,
            resource: buffer.as_entire_binding(),
        });
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("drape"),
        layout,
        entries: &entries,
    })
}

/// No point, as the shader has it.
const _: () = assert!(NONE == u32::MAX);

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    use rk_chara::physics::drape::{run, sample, State};

    /// A device to test on, where the machine has one (CI has none).
    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        pollster::block_on(async {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                    apply_limit_buckets: false,
                })
                .await
                .ok()?;
            adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("drape test"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints: wgpu::MemoryHints::default(),
                    trace: wgpu::Trace::Off,
                })
                .await
                .ok()
        })
    }

    /// A tube of cloth with a limb that swings out through its wall at
    /// 2.7 m/s over a floor and a table's corner, then stays: every rule
    /// of the solver has its turn.
    fn scene() -> (Setup, Vec<Frame>) {
        let mut setup = sample::tube(24, 12, 0.1, 0.04, Vec3::new(0.0, -0.05, 0.0), Vec3::new(0.0, -0.3, 0.0));
        // Propped outwards a little, and half of that kept.
        for p in setup.points.iter_mut().filter(|p| !p.fixed) {
            p.prop = Vec3::new(p.rest.x, -0.9, p.rest.z).normalize() * 9.0 + Vec3::Y * 9.81;
        }
        let floor = Some((Vec3::new(0.0, -0.25, 0.0), Vec3::Y));
        let table = std::sync::Arc::new(vec![
            [
                Vec3::new(-0.3, -0.15, -0.3),
                Vec3::new(-0.3, -0.15, 0.3),
                Vec3::new(-0.07, -0.15, -0.3),
            ],
            [
                Vec3::new(-0.07, -0.15, -0.3),
                Vec3::new(-0.3, -0.15, 0.3),
                Vec3::new(-0.07, -0.15, 0.3),
            ],
        ]);
        let out = Mat4::from_translation(Vec3::new(0.18, 0.0, 0.0));
        let mut frames = sample::sweep(&setup, Mat4::IDENTITY, out, 4, floor);
        frames.extend(sample::sweep(&setup, out, out, 26, floor).into_iter().map(|mut f| {
            f.begin = Begin::Keep;
            f
        }));
        for f in &mut frames {
            f.map = Some(table.clone());
            f.lift = 0.5;
        }
        (setup, frames)
    }

    #[test]
    fn the_gpu_moves_cloth_as_the_cpu_twin_does() {
        let Some((device, queue)) = device() else {
            eprintln!("no graphics adapter: skipped");
            return;
        };
        let (setup, frames) = scene();
        let pipelines = Pipelines::new(&device);
        let mut gpu = Gpu::new(&device, &pipelines, &setup);
        let mut state = State::new(setup.points.len());
        let mut worst = 0.0f32;
        let mut pressed = 0;
        for (k, frame) in frames.iter().enumerate() {
            run(&setup, &mut state, frame);
            gpu.run(&device, &queue, &pipelines, &setup, frame);
            let (places, speeds) = gpu.read(&device, &queue).expect("read back");
            let mut far = 0.0f32;
            for (i, p) in state.pos.iter().enumerate() {
                far = far.max((*p - Vec3::new(places[i][0], places[i][1], places[i][2])).length());
            }
            worst = worst.max(far);
            pressed += places.iter().filter(|p| p[3] > 0.5).count();
            // The two do the same sums in another order of rounding, and
            // a point on the edge of sticking can fall either way: the
            // GPU is put back on the twin each frame, so that what is
            // compared is one frame's work and not which way such a
            // point went thirty frames ago. The way a point was pushed
            // is the sum of pushes that may nearly cancel, and the speed
            // taken off along it differs by more than rounding where
            // they do: 46 um at the worst point of one frame here, 20
            // um before that rule.
            assert!(far < 1e-4, "frame {k}: {far} m apart");
            assert_eq!(speeds.len(), state.vel.len());
            let places: Vec<[f32; 4]> = state
                .pos
                .iter()
                .zip(&state.pressed)
                .map(|(p, w)| p.extend(*w).to_array())
                .collect();
            let speeds: Vec<[f32; 4]> = state.vel.iter().map(|v| v.extend(0.0).to_array()).collect();
            gpu.write(&queue, &places, &speeds);
        }
        // The scene did press the cloth on things.
        assert!(pressed > 100, "{pressed}");
        eprintln!("GPU against the CPU twin: at most {worst} m apart after a frame");
    }

    #[test]
    fn the_gpu_left_to_itself_stays_near_the_cpu_twin() {
        let Some((device, queue)) = device() else {
            eprintln!("no graphics adapter: skipped");
            return;
        };
        let (setup, frames) = scene();
        let pipelines = Pipelines::new(&device);
        let mut gpu = Gpu::new(&device, &pipelines, &setup);
        let mut state = State::new(setup.points.len());
        for frame in &frames {
            run(&setup, &mut state, frame);
            gpu.run(&device, &queue, &pipelines, &setup, frame);
        }
        let (places, _) = gpu.read(&device, &queue).expect("read back");
        let mut far: Vec<f32> = state
            .pos
            .iter()
            .zip(&places)
            .map(|(p, g)| (*p - Vec3::new(g[0], g[1], g[2])).length())
            .collect();
        far.sort_by(f32::total_cmp);
        let (middle, most, worst) = (far[far.len() / 2], far[far.len() * 9 / 10], far[far.len() - 1]);
        eprintln!("GPU against the CPU twin after 30 frames: middle {middle} m, nine in ten {most} m, worst {worst} m");
        // Where the hem is heaped on the floor a point is on one side of
        // its own cloth or the other by the last bit of a sum, and the
        // few that fall differently end centimetres apart: nine in ten
        // are held to, not the worst. The cloth is thrown by the limb
        // and still swings at the end; since being pushed out of
        // something no longer moves a point without speed first, the
        // first hem point that falls differently (frame 8) takes more
        // of the cloth with it by frame 30: the middle was 0.8 mm and
        // is 8 mm, nine in ten were 5 mm and are 22 mm. The same with
        // plain PBD speeds, so it is the scene and not the rule.
        assert!(
            middle < 2e-2 && most < 5e-2,
            "middle {middle}, nine in ten {most}, worst {worst}"
        );
    }

    /// What a frame costs by net size and substeps: `cargo test -p
    /// rk-render --release drape -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement, needs a graphics adapter"]
    fn what_a_frame_costs() {
        let Some((device, queue)) = device() else {
            return;
        };
        let pipelines = Pipelines::new(&device);
        for (cols, rows) in [(24, 8), (36, 16), (48, 24), (96, 48), (192, 96)] {
            let setup = sample::tube(
                cols,
                rows,
                0.3,
                0.08,
                Vec3::new(0.0, -0.05, 0.0),
                Vec3::new(0.0, -0.6, 0.0),
            );
            let mut gpu = Gpu::new(&device, &pipelines, &setup);
            let mut line = format!("{cols}x{rows} ({} points):", cols * rows);
            for subs in [8, 16, 32] {
                let mut frame = sample::still(&setup, Begin::Reset, Some((Vec3::new(0.0, -0.4, 0.0), Vec3::Y)));
                frame.substeps = subs;
                frame.dt = subs as f32 / 960.0;
                frame.poses = vec![Mat4::IDENTITY; (subs as usize + 1) * setup.slots()];
                gpu.run(&device, &queue, &pipelines, &setup, &frame);
                frame.begin = Begin::Keep;
                device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
                let start = std::time::Instant::now();
                let times = 200;
                for _ in 0..times {
                    gpu.run(&device, &queue, &pipelines, &setup, &frame);
                }
                device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
                let ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(times);
                line += &format!("  {subs} substeps {ms:.3} ms");
            }
            eprintln!("{line}");
        }
    }

    #[test]
    fn the_shader_is_valid_wgsl_and_its_structures_are_the_sizes_sent() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("drape.wgsl")).expect("drape.wgsl parses");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("drape.wgsl validates");
        let mut layouter = wgpu::naga::proc::Layouter::default();
        layouter.update(module.to_ctx()).expect("layout");
        let size = |name: &str| {
            let (handle, _) = module
                .types
                .iter()
                .find(|(_, t)| t.name.as_deref() == Some(name))
                .unwrap_or_else(|| panic!("no {name}"));
            layouter[handle].size as usize
        };
        assert_eq!(size("Sub"), std::mem::size_of::<Sub>());
        assert_eq!(size("Point"), std::mem::size_of::<Point>());
        assert_eq!(size("Bound"), std::mem::size_of::<Bound>());
        assert_eq!(size("Pose"), 128);
        assert!(std::mem::size_of::<Sub>() as u64 <= SUB_STRIDE);
    }
}
