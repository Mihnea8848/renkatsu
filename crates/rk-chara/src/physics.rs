// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The game's DynamicBone physics: hair, ribbons, tails and accessories
//! (`DynamicBone`), node chains (`DynamicBone_Ver01`) and the bust and hips
//! (`DynamicBone_Ver02`), with their colliders.
//!
//! Every formula is the decompiled game code. `DynamicBone.AppendParticles`
//! is the replacement the Dynamic Bones Fix plugin (IllusionFixes) installs,
//! and that plugin also switches off `SkipUpdateParticles` for `DynamicBone`
//! and `DynamicBone_Ver02`.
//!
//! A frame is `Physics::step`: every `Update` (bones go back to their rest
//! transforms), then every `LateUpdate` (particles move, bones follow).
//! `Physics::settle` runs that for a number of frames.

use crate::scene::Scene;
use glam::{Mat3, Mat4, Quat, Vec3};
use rk_unityfs::{PPtr, UObject};

/// Frames the reference capture waits after loading a character.
pub const SETTLE_FRAMES: u32 = 90;
/// Frame time assumed for settling: display refresh at 60 Hz.
pub const FRAME_TIME: f32 = 1.0 / 60.0;
/// Frames of a settle over which Advanced Skirt Physics moves into a new
/// pose (`Physics::ease`).
pub const EASE_FRAMES: u32 = 30;

// ---------------------------------------------------------------------------
// Unity transform semantics on scene nodes.

fn world(scene: &Scene, node: usize) -> Mat4 {
    let mut m = scene.nodes[node].local.matrix();
    let mut p = scene.nodes[node].parent;
    while let Some(i) = p {
        m = scene.nodes[i].local.matrix() * m;
        p = scene.nodes[i].parent;
    }
    m
}

fn position(scene: &Scene, node: usize) -> Vec3 {
    world(scene, node).w_axis.truncate()
}

/// Unity turns a rotation around when it passes through a mirrored scale.
fn mirror_quat(scale: Vec3, q: Quat) -> Quat {
    let s = Vec3::new(scale.x.signum(), scale.y.signum(), scale.z.signum());
    Quat::from_xyzw(q.x * s.y * s.z, q.y * s.x * s.z, q.z * s.x * s.y, q.w)
}

/// `Transform.rotation`: the local rotations multiplied down the hierarchy.
fn rotation(scene: &Scene, node: usize) -> Quat {
    let mut q = scene.nodes[node].local.rot;
    let mut p = scene.nodes[node].parent;
    while let Some(i) = p {
        let l = &scene.nodes[i].local;
        q = l.rot * mirror_quat(l.scale, q);
        p = scene.nodes[i].parent;
    }
    q.normalize()
}

fn set_rotation(scene: &mut Scene, node: usize, q: Quat) {
    let mut chain = Vec::new();
    let mut p = scene.nodes[node].parent;
    while let Some(i) = p {
        chain.push(i);
        p = scene.nodes[i].parent;
    }
    let mut q = q;
    for &i in chain.iter().rev() {
        let l = &scene.nodes[i].local;
        q = mirror_quat(l.scale, l.rot.inverse() * q);
    }
    scene.nodes[node].local.rot = q.normalize();
}

fn set_position(scene: &mut Scene, node: usize, p: Vec3) {
    let local = match scene.nodes[node].parent {
        Some(parent) => world(scene, parent).inverse().transform_point3(p),
        None => p,
    };
    scene.nodes[node].local.pos = local;
}

/// `Transform.lossyScale.x`.
fn lossy_scale_x(scene: &Scene, node: usize) -> f32 {
    let m = Mat3::from_mat4(world(scene, node));
    let r = Mat3::from_quat(rotation(scene, node));
    (r.transpose() * m).x_axis.x
}

fn lossy_scale_z(scene: &Scene, node: usize) -> f32 {
    let m = Mat3::from_mat4(world(scene, node));
    let r = Mat3::from_quat(rotation(scene, node));
    (r.transpose() * m).z_axis.z
}

/// `Quaternion.FromToRotation`.
fn from_to(from: Vec3, to: Vec3) -> Quat {
    let (a, b) = (from.normalize_or_zero(), to.normalize_or_zero());
    if a == Vec3::ZERO || b == Vec3::ZERO {
        return Quat::IDENTITY;
    }
    Quat::from_rotation_arc(a, b)
}

/// `Vector3 * ((magnitude - rest) / magnitude)` guarded as the game does.
fn keep_length(parent: Vec3, pos: &mut Vec3, rest: f32) {
    let v = parent - *pos;
    let len = v.length();
    if len > 0.0 {
        *pos += v * ((len - rest) / len);
    }
}

// ---------------------------------------------------------------------------
// AnimationCurve.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub time: f32,
    pub value: f32,
    pub in_slope: f32,
    pub out_slope: f32,
}

/// A Unity `AnimationCurve`, clamped outside its keys.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Curve {
    pub keys: Vec<Key>,
}

impl Curve {
    /// Cubic Hermite between the two keys around `t`, as Unity evaluates it.
    pub fn evaluate(&self, t: f32) -> f32 {
        let (Some(first), Some(last)) = (self.keys.first(), self.keys.last()) else {
            return 0.0;
        };
        if t <= first.time {
            return first.value;
        }
        if t >= last.time {
            return last.value;
        }
        let i = self.keys.partition_point(|k| k.time <= t).clamp(1, self.keys.len() - 1);
        let (a, b) = (self.keys[i - 1], self.keys[i]);
        let dx = b.time - a.time;
        if dx <= 0.0 || !a.out_slope.is_finite() || !b.in_slope.is_finite() {
            return a.value;
        }
        let s = (t - a.time) / dx;
        let (s2, s3) = (s * s, s * s * s);
        let (m0, m1) = (a.out_slope * dx, b.in_slope * dx);
        (2.0 * s3 - 3.0 * s2 + 1.0) * a.value
            + (s3 - 2.0 * s2 + s) * m0
            + (s3 - s2) * m1
            + (-2.0 * s3 + 3.0 * s2) * b.value
    }

    fn scale(curve: &Option<Curve>, t: f32) -> f32 {
        match curve {
            Some(c) if !c.keys.is_empty() => c.evaluate(t),
            _ => 1.0,
        }
    }
}

pub mod cloth;
pub mod drape;
pub mod net;
pub mod shell;

// ---------------------------------------------------------------------------
// DynamicBoneCollider.

#[derive(Debug, Clone)]
pub struct Collider {
    pub node: usize,
    pub center: Vec3,
    pub radius: f32,
    pub height: f32,
    /// Capsule axis: 0 X, 1 Y, 2 Z.
    pub direction: u8,
    /// Keeps particles inside instead of outside.
    pub inside: bool,
    pub enabled: bool,
}

impl Default for Collider {
    fn default() -> Self {
        Self {
            node: 0,
            center: Vec3::ZERO,
            radius: 0.5,
            height: 0.0,
            direction: 0,
            inside: false,
            enabled: true,
        }
    }
}

/// A collider placed in the world for one update.
#[derive(Debug, Clone, Copy)]
struct Shape {
    p0: Vec3,
    p1: Vec3,
    radius: f32,
    capsule: bool,
    inside: bool,
}

impl Collider {
    fn shape(&self, scene: &Scene) -> Shape {
        let m = world(scene, self.node);
        let radius = self.radius * lossy_scale_z(scene, self.node).abs();
        let half = (self.height - self.radius) * 0.5;
        if half <= 0.0 {
            let c = m.transform_point3(self.center);
            return Shape {
                p0: c,
                p1: c,
                radius,
                capsule: false,
                inside: self.inside,
            };
        }
        let (mut c0, mut c1) = (self.center, self.center);
        let axis = self.direction.min(2) as usize;
        c0[axis] -= half;
        c1[axis] += half;
        Shape {
            p0: m.transform_point3(c0),
            p1: m.transform_point3(c1),
            radius,
            capsule: true,
            inside: self.inside,
        }
    }
}

impl Shape {
    /// `DynamicBoneCollider.Collide`.
    fn collide(&self, pos: &mut Vec3, particle_radius: f32) {
        let r = self.radius + particle_radius;
        let r2 = r * r;
        // Pushes the particle to the sphere surface around `c` when needed.
        let sphere = |pos: &mut Vec3, c: Vec3| {
            let d = *pos - c;
            let len2 = d.length_squared();
            let hit = if self.inside {
                len2 > r2
            } else {
                len2 > 0.0 && len2 < r2
            };
            if hit {
                *pos = c + d * (r / len2.sqrt());
            }
        };
        if !self.capsule {
            sphere(pos, self.p0);
            return;
        }
        let axis = self.p1 - self.p0;
        let mut d = *pos - self.p0;
        let mut t = d.dot(axis);
        if t <= 0.0 {
            sphere(pos, self.p0);
            return;
        }
        let axis2 = axis.length_squared();
        if t >= axis2 {
            sphere(pos, self.p1);
        } else if axis2 > 0.0 {
            t /= axis2;
            d -= axis * t;
            let len2 = d.length_squared();
            let hit = if self.inside {
                len2 > r2
            } else {
                len2 > 0.0 && len2 < r2
            };
            if hit {
                let len = len2.sqrt();
                *pos += d * ((r - len) / len);
            }
        }
    }
}

fn shapes(scene: &Scene, colliders: &[Collider], list: &[usize], need_active: bool) -> Vec<Shape> {
    list.iter()
        .filter_map(|&i| colliders.get(i))
        .filter(|c| c.enabled && (!need_active || active_in_hierarchy(scene, c.node)))
        .map(|c| c.shape(scene))
        .collect()
}

fn active_in_hierarchy(scene: &Scene, node: usize) -> bool {
    let mut n = Some(node);
    while let Some(i) = n {
        if !scene.nodes[i].active {
            return false;
        }
        n = scene.nodes[i].parent;
    }
    true
}

// ---------------------------------------------------------------------------
// Particles, shared by the three components.

#[derive(Debug, Clone)]
struct Particle {
    node: Option<usize>,
    parent: Option<usize>,
    damping: f32,
    elasticity: f32,
    stiffness: f32,
    inert: f32,
    radius: f32,
    bone_length: f32,
    pos: Vec3,
    prev: Vec3,
    end_offset: Vec3,
    init_pos: Vec3,
    init_rot: Quat,
    init_scale: Vec3,
    /// Place in the parent particle's space (Ver01, Ver02).
    local_pos: Vec3,
    rotation_calc: bool,
    /// Ver01.
    add_angle_scale: f32,
    /// Ver02.
    scale_next_bone_length: f32,
    move_limit: bool,
    move_min: Vec3,
    move_max: Vec3,
    keep_min: f32,
    keep_max: f32,
    crush: bool,
    crush_area_min: f32,
    crush_area_max: f32,
    crush_add_min: f32,
    crush_add_max: f32,
    ref_node: Option<usize>,
}

impl Default for Particle {
    fn default() -> Self {
        Self {
            node: None,
            parent: None,
            damping: 0.0,
            elasticity: 0.0,
            stiffness: 0.0,
            inert: 0.0,
            radius: 0.0,
            bone_length: 0.0,
            pos: Vec3::ZERO,
            prev: Vec3::ZERO,
            end_offset: Vec3::ZERO,
            init_pos: Vec3::ZERO,
            init_rot: Quat::IDENTITY,
            init_scale: Vec3::ONE,
            local_pos: Vec3::ZERO,
            rotation_calc: true,
            add_angle_scale: 1.0,
            scale_next_bone_length: 1.0,
            move_limit: false,
            move_min: Vec3::ZERO,
            move_max: Vec3::ZERO,
            keep_min: 0.0,
            keep_max: 0.0,
            crush: false,
            crush_area_min: 0.0,
            crush_area_max: 0.0,
            crush_add_min: 0.0,
            crush_add_max: 0.0,
            ref_node: None,
        }
    }
}

/// How many simulation steps a frame of `dt` seconds runs. `None` skips.
fn loops(time: &mut f32, update_rate: f32, dt: f32, max: u32) -> u32 {
    if update_rate <= 0.0 {
        return 1;
    }
    let step = 1.0 / update_rate;
    *time += dt;
    let mut n = 0;
    while *time >= step {
        *time -= step;
        n += 1;
        if n >= max {
            *time = 0.0;
            break;
        }
    }
    n
}

/// World matrices of the particles' bones before anything moves this frame.
fn rest_mats(scene: &Scene, particles: &[Particle]) -> Vec<Mat4> {
    particles
        .iter()
        .map(|p| p.node.map(|n| world(scene, n)).unwrap_or(Mat4::IDENTITY))
        .collect()
}

/// The pull towards the rest shape: elasticity, then the stiffness limit.
fn shape_pull(p: &mut Particle, rest: Vec3, rest_len: f32, weight: f32, elastic: bool) {
    let stiff = 1.0 + (p.stiffness - 1.0) * weight;
    if stiff > 0.0 || (elastic && p.elasticity > 0.0) {
        if elastic {
            p.pos += (rest - p.pos) * p.elasticity;
        }
        if stiff > 0.0 {
            let d = rest - p.pos;
            let len = d.length();
            let max = rest_len * (1.0 - stiff) * 2.0;
            if len > max {
                p.pos += d * ((len - max) / len);
            }
        }
    }
}

fn reset_particles(scene: &Scene, particles: &mut [Particle]) {
    for i in 0..particles.len() {
        let p = match particles[i].node {
            Some(n) => position(scene, n),
            None => {
                let parent = particles[i].parent.and_then(|pi| particles[pi].node);
                match parent {
                    Some(n) => world(scene, n).transform_point3(particles[i].end_offset),
                    None => continue,
                }
            }
        };
        particles[i].pos = p;
        particles[i].prev = p;
    }
}

// ---------------------------------------------------------------------------
// DynamicBone.

#[derive(Debug, Clone)]
pub struct DynamicBoneDef {
    /// Node the component sits on.
    pub node: usize,
    pub root: Option<usize>,
    pub update_rate: f32,
    pub damping: f32,
    pub damping_distrib: Option<Curve>,
    pub elasticity: f32,
    pub elasticity_distrib: Option<Curve>,
    pub stiffness: f32,
    pub stiffness_distrib: Option<Curve>,
    pub inert: f32,
    pub inert_distrib: Option<Curve>,
    pub radius: f32,
    pub radius_distrib: Option<Curve>,
    pub end_length: f32,
    pub end_offset: Vec3,
    pub gravity: Vec3,
    pub force: Vec3,
    /// Indices into `Physics::colliders`.
    pub colliders: Vec<usize>,
    pub exclusions: Vec<usize>,
    pub not_rolls: Vec<usize>,
    /// 0 none, 1 X, 2 Y, 3 Z.
    pub freeze_axis: u8,
    /// Blend between the rest pose (0) and the simulation (1).
    pub weight: f32,
    pub enabled: bool,
}

impl Default for DynamicBoneDef {
    fn default() -> Self {
        Self {
            node: 0,
            root: None,
            update_rate: 60.0,
            damping: 0.1,
            damping_distrib: None,
            elasticity: 0.1,
            elasticity_distrib: None,
            stiffness: 0.1,
            stiffness_distrib: None,
            inert: 0.0,
            inert_distrib: None,
            radius: 0.0,
            radius_distrib: None,
            end_length: 0.0,
            end_offset: Vec3::ZERO,
            gravity: Vec3::ZERO,
            force: Vec3::ZERO,
            colliders: Vec::new(),
            exclusions: Vec::new(),
            not_rolls: Vec::new(),
            freeze_axis: 0,
            weight: 1.0,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DynamicBone {
    pub def: DynamicBoneDef,
    particles: Vec<Particle>,
    local_gravity: Vec3,
    object_move: Vec3,
    object_prev: Vec3,
    bone_total_length: f32,
    object_scale: f32,
    time: f32,
    weight: f32,
}

impl DynamicBone {
    pub fn new(def: DynamicBoneDef) -> Self {
        let weight = def.weight;
        Self {
            def,
            particles: Vec::new(),
            local_gravity: Vec3::ZERO,
            object_move: Vec3::ZERO,
            object_prev: Vec3::ZERO,
            bone_total_length: 0.0,
            object_scale: 1.0,
            time: 0.0,
            weight,
        }
    }

    /// `SetupParticles`.
    fn start(&mut self, scene: &Scene) {
        self.particles.clear();
        let Some(root) = self.def.root else { return };
        self.local_gravity = rotation(scene, root).inverse() * self.def.gravity;
        self.object_scale = lossy_scale_x(scene, self.def.node);
        self.object_prev = position(scene, self.def.node);
        self.object_move = Vec3::ZERO;
        self.bone_total_length = 0.0;
        self.append(scene, Some(root), None, 0.0);
        let d = &self.def;
        for p in &mut self.particles {
            p.damping = d.damping;
            p.elasticity = d.elasticity;
            p.stiffness = d.stiffness;
            p.inert = d.inert;
            p.radius = d.radius;
            if self.bone_total_length > 0.0 {
                let t = p.bone_length / self.bone_total_length;
                p.damping *= Curve::scale(&d.damping_distrib, t);
                p.elasticity *= Curve::scale(&d.elasticity_distrib, t);
                p.stiffness *= Curve::scale(&d.stiffness_distrib, t);
                p.inert *= Curve::scale(&d.inert_distrib, t);
                p.radius *= Curve::scale(&d.radius_distrib, t);
            }
            p.damping = p.damping.clamp(0.0, 1.0);
            p.elasticity = p.elasticity.clamp(0.0, 1.0);
            p.stiffness = p.stiffness.clamp(0.0, 1.0);
            p.inert = p.inert.clamp(0.0, 1.0);
            p.radius = p.radius.max(0.0);
        }
    }

    /// `AppendParticles` as replaced by the Dynamic Bones Fix plugin.
    fn append(&mut self, scene: &Scene, bone: Option<usize>, parent: Option<usize>, mut bone_length: f32) {
        let mut p = Particle {
            node: bone,
            parent,
            ..Default::default()
        };
        match (bone, parent) {
            (Some(b), _) => {
                p.pos = position(scene, b);
                p.prev = p.pos;
                p.init_pos = scene.nodes[b].local.pos;
                p.init_rot = scene.nodes[b].local.rot;
            }
            (None, Some(pi)) => {
                let Some(t) = self.particles[pi].node else { return };
                let m = world(scene, t);
                let t_pos = m.w_axis.truncate();
                if self.def.end_length > 0.0 {
                    p.end_offset = match scene.nodes[t].parent {
                        Some(tp) => {
                            m.inverse().transform_point3(t_pos * 2.0 - position(scene, tp)) * self.def.end_length
                        }
                        None => Vec3::new(self.def.end_length, 0.0, 0.0),
                    };
                } else {
                    let dir = rotation(scene, self.def.node) * self.def.end_offset;
                    p.end_offset = m.inverse().transform_point3(dir + t_pos);
                }
                p.pos = m.transform_point3(p.end_offset);
                p.prev = p.pos;
            }
            (None, None) => return,
        }
        if let Some(pi) = parent {
            if let Some(pn) = self.particles[pi].node {
                bone_length += (position(scene, pn) - p.pos).length();
            }
            p.bone_length = bone_length;
            self.bone_total_length = self.bone_total_length.max(bone_length);
        }
        let index = self.particles.len();
        self.particles.push(p);
        let Some(mut bone) = bone else { return };
        let mut appended = false;
        while !scene.nodes[bone].children.is_empty() {
            let mut not_roll = false;
            let mut last = 0;
            for (i, &child) in scene.nodes[bone].children.iter().enumerate() {
                let mut skip = self.def.exclusions.contains(&child);
                if !skip {
                    skip = self.def.not_rolls.contains(&child);
                    not_roll = skip;
                    last = i;
                }
                if !skip {
                    self.append(scene, Some(child), Some(index), bone_length);
                    appended = true;
                }
            }
            if !not_roll {
                break;
            }
            bone = scene.nodes[bone].children[last];
        }
        if !appended && (self.def.end_length > 0.0 || self.def.end_offset != Vec3::ZERO) {
            self.append(scene, None, Some(index), bone_length);
        }
    }

    fn running(&self) -> bool {
        self.def.enabled && self.weight > 0.0
    }

    /// `InitTransforms`.
    fn init_transforms(&self, scene: &mut Scene) {
        for p in &self.particles {
            if let Some(n) = p.node {
                scene.nodes[n].local.pos = p.init_pos;
                scene.nodes[n].local.rot = p.init_rot;
            }
        }
    }

    /// `UpdateDynamicBones`.
    fn late_update(&mut self, scene: &mut Scene, colliders: &[Collider], dt: f32) {
        let Some(root) = self.def.root else { return };
        if self.particles.is_empty() {
            return;
        }
        self.object_scale = lossy_scale_x(scene, self.def.node).abs();
        let here = position(scene, self.def.node);
        self.object_move = here - self.object_prev;
        self.object_prev = here;
        let n = loops(&mut self.time, self.def.update_rate, dt, 3);
        if n > 0 {
            let mats = rest_mats(scene, &self.particles);
            let shapes = shapes(scene, colliders, &self.def.colliders, false);
            let root_rot = rotation(scene, root);
            for _ in 0..n {
                self.update_particles1(&mats, root_rot);
                self.update_particles2(scene, &mats, &shapes);
                self.object_move = Vec3::ZERO;
            }
        }
        self.apply(scene);
    }

    fn update_particles1(&mut self, mats: &[Mat4], root_rot: Quat) {
        // Only the part of gravity the rest pose does not already show.
        let mut gravity = self.def.gravity;
        let dir = gravity.normalize_or_zero();
        let now = root_rot * self.local_gravity;
        gravity -= dir * now.dot(dir).max(0.0);
        gravity = (gravity + self.def.force) * self.object_scale;
        for (i, p) in self.particles.iter_mut().enumerate() {
            if p.parent.is_some() {
                let v = p.pos - p.prev;
                let moved = self.object_move * p.inert;
                p.prev = p.pos + moved;
                p.pos += v * (1.0 - p.damping) + gravity + moved;
            } else {
                p.prev = p.pos;
                p.pos = mats[i].w_axis.truncate();
            }
        }
    }

    fn update_particles2(&mut self, scene: &Scene, mats: &[Mat4], shapes: &[Shape]) {
        for i in 1..self.particles.len() {
            let Some(pi) = self.particles[i].parent else { continue };
            let parent_pos = self.particles[pi].pos;
            let parent_node = self.particles[pi].node;
            let p = &mut self.particles[i];
            let local = match p.node {
                Some(n) => scene.nodes[n].local.pos,
                None => p.end_offset,
            };
            let rest_len = match p.node {
                Some(_) => (mats[pi].w_axis - mats[i].w_axis).truncate().length(),
                None => mats[pi].transform_vector3(p.end_offset).length(),
            };
            let mut m = mats[pi];
            m.w_axis = parent_pos.extend(1.0);
            shape_pull(p, m.transform_point3(local), rest_len, self.weight, true);
            let radius = p.radius * self.object_scale;
            for s in shapes {
                s.collide(&mut p.pos, radius);
            }
            if let (1..=3, Some(pn)) = (self.def.freeze_axis, parent_node) {
                let axis = [Vec3::X, Vec3::Y, Vec3::Z][self.def.freeze_axis as usize - 1];
                let normal = (rotation(scene, pn) * axis).normalize_or_zero();
                p.pos -= normal * normal.dot(p.pos - parent_pos);
            }
            keep_length(parent_pos, &mut p.pos, rest_len);
        }
    }

    /// `ApplyParticlesToTransforms`.
    fn apply(&self, scene: &mut Scene) {
        // Mirrored scales on the component or its parent flip the bone turn.
        let node = self.def.node;
        let rot = rotation(scene, node);
        let scale = scene.nodes[node].local.scale;
        let mut mirror = [
            (scale.x < 0.0).then(|| rot * Vec3::X),
            (scale.y < 0.0).then(|| rot * Vec3::Y),
            (scale.z < 0.0).then(|| rot * Vec3::Z),
        ];
        if let Some(parent) = scene.nodes[node].parent {
            let (ps, pr) = (scene.nodes[parent].local.scale, rotation(scene, parent));
            for (k, axis) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().enumerate() {
                if mirror[k].is_none() && ps[k] < 0.0 {
                    mirror[k] = Some(pr * axis);
                }
            }
        }
        for i in 1..self.particles.len() {
            let p = &self.particles[i];
            let Some(pi) = p.parent else { continue };
            let Some(parent_node) = self.particles[pi].node else {
                continue;
            };
            if scene.nodes[parent_node].children.len() <= 1 {
                let dir = match p.node {
                    Some(n) => scene.nodes[n].local.pos,
                    None => p.end_offset,
                };
                let mut v = p.pos - self.particles[pi].pos;
                for axis in mirror.into_iter().flatten() {
                    v -= axis * (v.dot(axis) * 2.0);
                }
                let parent_rot = rotation(scene, parent_node);
                let q = from_to(parent_rot * dir, v);
                set_rotation(scene, parent_node, q * parent_rot);
            }
            if let Some(n) = p.node {
                set_position(scene, n, p.pos);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DynamicBone_Ver01.

#[derive(Debug, Clone)]
pub struct Ver01Def {
    pub node: usize,
    pub root: Option<usize>,
    pub update_rate: f32,
    pub damping: f32,
    pub damping_distrib: Option<Curve>,
    pub elasticity: f32,
    pub elasticity_distrib: Option<Curve>,
    pub stiffness: f32,
    pub stiffness_distrib: Option<Curve>,
    pub inert: f32,
    pub inert_distrib: Option<Curve>,
    pub radius: f32,
    pub radius_distrib: Option<Curve>,
    pub add_angle_scale: f32,
    pub add_angle_scale_distrib: Option<Curve>,
    pub end_length: f32,
    pub end_offset: Vec3,
    pub gravity: Vec3,
    pub force: Vec3,
    pub colliders: Vec<usize>,
    /// Bone and whether it turns towards the next particle.
    pub nodes: Vec<(usize, bool)>,
    pub enabled: bool,
}

impl Default for Ver01Def {
    fn default() -> Self {
        Self {
            node: 0,
            root: None,
            update_rate: 60.0,
            damping: 0.1,
            damping_distrib: None,
            elasticity: 0.1,
            elasticity_distrib: None,
            stiffness: 0.1,
            stiffness_distrib: None,
            inert: 0.0,
            inert_distrib: None,
            radius: 0.0,
            radius_distrib: None,
            add_angle_scale: 1.0,
            add_angle_scale_distrib: None,
            end_length: 0.0,
            end_offset: Vec3::ZERO,
            gravity: Vec3::ZERO,
            force: Vec3::ZERO,
            colliders: Vec::new(),
            nodes: Vec::new(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Ver01 {
    pub def: Ver01Def,
    particles: Vec<Particle>,
    object_move: Vec3,
    object_prev: Vec3,
    object_scale: f32,
    time: f32,
    weight: f32,
}

impl Ver01 {
    pub fn new(def: Ver01Def) -> Self {
        Self {
            def,
            particles: Vec::new(),
            object_move: Vec3::ZERO,
            object_prev: Vec3::ZERO,
            object_scale: 1.0,
            time: 0.0,
            weight: 1.0,
        }
    }

    /// `SetupParticles`.
    fn start(&mut self, scene: &Scene) {
        self.particles.clear();
        if self.def.root.is_none() {
            self.def.root = self.def.nodes.first().map(|n| n.0);
        }
        if self.def.root.is_none() {
            return;
        }
        self.object_scale = lossy_scale_x(scene, self.def.node);
        self.object_prev = position(scene, self.def.node);
        self.object_move = Vec3::ZERO;
        let nodes = self.def.nodes.clone();
        for (i, (node, calc)) in nodes.into_iter().enumerate() {
            self.append(scene, Some(node), calc, i.checked_sub(1));
        }
        if (self.def.end_length > 0.0 || self.def.end_offset.length() != 0.0) && !self.particles.is_empty() {
            self.append(scene, None, true, Some(self.particles.len() - 1));
        }
        let count = self.particles.len();
        let step = if count > 1 { 1.0 / (count - 1) as f32 } else { 0.0 };
        let d = &self.def;
        let mut t = 0.0;
        for p in &mut self.particles {
            p.damping = (d.damping * Curve::scale(&d.damping_distrib, t)).clamp(0.0, 1.0);
            p.elasticity = (d.elasticity * Curve::scale(&d.elasticity_distrib, t)).clamp(0.0, 1.0);
            p.stiffness = (d.stiffness * Curve::scale(&d.stiffness_distrib, t)).clamp(0.0, 1.0);
            p.inert = (d.inert * Curve::scale(&d.inert_distrib, t)).clamp(0.0, 1.0);
            p.radius = (d.radius * Curve::scale(&d.radius_distrib, t)).max(0.0);
            p.add_angle_scale = (d.add_angle_scale * Curve::scale(&d.add_angle_scale_distrib, t)).max(0.0);
            t += step;
        }
    }

    fn append(&mut self, scene: &Scene, node: Option<usize>, calc: bool, parent: Option<usize>) {
        let mut p = Particle {
            node,
            parent,
            rotation_calc: calc,
            ..Default::default()
        };
        let parent_node = parent.and_then(|pi| self.particles[pi].node);
        match node {
            Some(n) => {
                p.pos = position(scene, n);
                p.prev = p.pos;
                p.init_pos = scene.nodes[n].local.pos;
                p.init_rot = scene.nodes[n].local.rot;
                if let Some(pn) = parent_node {
                    p.local_pos = world(scene, pn).inverse().transform_point3(p.pos);
                }
            }
            None => {
                let Some(t) = parent_node else { return };
                let m = world(scene, t);
                p.end_offset = if self.def.end_length > 0.0 {
                    match scene.nodes[t].parent {
                        Some(tp) => {
                            m.inverse()
                                .transform_point3(m.w_axis.truncate() * 2.0 - position(scene, tp))
                                * self.def.end_length
                        }
                        None => Vec3::new(self.def.end_length, 0.0, 0.0),
                    }
                } else {
                    self.def.end_offset
                };
                p.pos = m.transform_point3(p.end_offset);
                p.prev = p.pos;
            }
        }
        self.particles.push(p);
    }

    fn running(&self) -> bool {
        self.def.enabled && self.weight > 0.0
    }

    fn init_transforms(&self, scene: &mut Scene) {
        for p in &self.particles {
            if let Some(n) = p.node {
                scene.nodes[n].local.pos = p.init_pos;
                scene.nodes[n].local.rot = p.init_rot;
            }
        }
    }

    fn rest_len(&self, mats: &[Mat4], i: usize, pi: usize) -> f32 {
        match self.particles[i].node {
            Some(_) => (mats[pi].w_axis - mats[i].w_axis).truncate().length(),
            None => self.particles[i].end_offset.length() * self.object_scale,
        }
    }

    fn late_update(&mut self, scene: &mut Scene, colliders: &[Collider], dt: f32) {
        if self.def.root.is_none() || self.particles.is_empty() {
            return;
        }
        self.object_scale = lossy_scale_x(scene, self.def.node).abs();
        let here = position(scene, self.def.node);
        self.object_move = here - self.object_prev;
        self.object_prev = here;
        let n = loops(&mut self.time, self.def.update_rate, dt, 3);
        let mats = rest_mats(scene, &self.particles);
        if n > 0 {
            let shapes = shapes(scene, colliders, &self.def.colliders, false);
            for _ in 0..n {
                let gravity = (self.def.gravity + self.def.force) * self.object_scale;
                for (i, p) in self.particles.iter_mut().enumerate() {
                    if p.parent.is_some() {
                        let v = p.pos - p.prev;
                        let moved = self.object_move * p.inert;
                        p.prev = p.pos + moved;
                        p.pos += v * (1.0 - p.damping) + gravity + moved;
                    } else {
                        p.prev = p.pos;
                        p.pos = mats[i].w_axis.truncate();
                    }
                }
                for i in 1..self.particles.len() {
                    let Some(pi) = self.particles[i].parent else { continue };
                    let rest_len = self.rest_len(&mats, i, pi);
                    let parent_pos = self.particles[pi].pos;
                    let p = &mut self.particles[i];
                    let local = if p.node.is_some() { p.local_pos } else { p.end_offset };
                    let mut m = mats[pi];
                    m.w_axis = parent_pos.extend(1.0);
                    shape_pull(p, m.transform_point3(local), rest_len, self.weight, true);
                    let radius = p.radius * self.object_scale;
                    for s in &shapes {
                        s.collide(&mut p.pos, radius);
                    }
                    keep_length(parent_pos, &mut p.pos, rest_len);
                }
                self.object_move = Vec3::ZERO;
            }
        } else {
            // SkipUpdateParticles: follow the object without simulating.
            for i in 0..self.particles.len() {
                let Some(pi) = self.particles[i].parent else {
                    self.particles[i].prev = self.particles[i].pos;
                    self.particles[i].pos = mats[i].w_axis.truncate();
                    continue;
                };
                let rest_len = self.rest_len(&mats, i, pi);
                let parent_pos = self.particles[pi].pos;
                let moved = self.object_move * self.particles[i].inert;
                let local = match self.particles[i].node {
                    Some(n) => scene.nodes[n].local.pos,
                    None => self.particles[i].end_offset,
                };
                let p = &mut self.particles[i];
                p.prev += moved;
                p.pos += moved;
                let mut m = mats[pi];
                m.w_axis = parent_pos.extend(1.0);
                shape_pull(p, m.transform_point3(local), rest_len, self.weight, false);
                keep_length(parent_pos, &mut p.pos, rest_len);
            }
        }
        // ApplyParticlesToTransforms.
        for i in 1..self.particles.len() {
            let p = &self.particles[i];
            let Some(pi) = p.parent else { continue };
            let Some(parent_node) = self.particles[pi].node else {
                continue;
            };
            if self.particles[pi].rotation_calc {
                let dir = if p.node.is_some() { p.local_pos } else { p.end_offset };
                let parent_rot = rotation(scene, parent_node);
                let q = from_to(parent_rot * dir, p.pos - self.particles[pi].pos);
                let (axis, angle) = q.to_axis_angle();
                let q = Quat::from_axis_angle(axis, angle * p.add_angle_scale);
                set_rotation(scene, parent_node, q * parent_rot);
            }
            if let Some(n) = p.node {
                set_position(scene, n, p.pos);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DynamicBone_Ver02.

#[derive(Debug, Clone)]
pub struct BoneParam {
    pub ref_node: Option<usize>,
    pub rotation_calc: bool,
    pub damping: f32,
    pub elasticity: f32,
    pub stiffness: f32,
    pub inert: f32,
    pub next_bone_length: f32,
    pub collision_radius: f32,
    pub move_limit: bool,
    pub move_min: Vec3,
    pub move_max: Vec3,
    pub keep_min: f32,
    pub keep_max: f32,
    pub crush: bool,
    pub crush_area_min: f32,
    pub crush_area_max: f32,
    pub crush_add_min: f32,
    pub crush_add_max: f32,
}

impl Default for BoneParam {
    fn default() -> Self {
        Self {
            ref_node: None,
            rotation_calc: false,
            damping: 0.0,
            elasticity: 0.0,
            stiffness: 0.0,
            inert: 0.0,
            next_bone_length: 1.0,
            collision_radius: 0.0,
            move_limit: false,
            move_min: Vec3::ZERO,
            move_max: Vec3::ZERO,
            keep_min: 0.0,
            keep_max: 0.0,
            crush: false,
            crush_area_min: 0.0,
            crush_area_max: 0.0,
            crush_add_min: 0.0,
            crush_add_max: 0.0,
        }
    }
}

impl BoneParam {
    /// The fields `AppendParticlePtn` and `AppendParticles` copy and clamp.
    fn particle(&self) -> Particle {
        Particle {
            rotation_calc: self.rotation_calc,
            damping: self.damping.clamp(0.0, 1.0),
            elasticity: self.elasticity.clamp(0.0, 1.0),
            stiffness: self.stiffness.clamp(0.0, 1.0),
            inert: self.inert.clamp(0.0, 1.0),
            scale_next_bone_length: self.next_bone_length.max(0.0),
            radius: self.collision_radius.max(0.0),
            move_limit: self.move_limit,
            move_min: self.move_min,
            move_max: self.move_max,
            keep_min: self.keep_min,
            keep_max: self.keep_max,
            crush: self.crush,
            crush_area_min: self.crush_area_min,
            crush_area_max: self.crush_area_max,
            crush_add_min: self.crush_add_min,
            crush_add_max: self.crush_add_max,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Pattern {
    pub gravity: Vec3,
    pub end_offset: Vec3,
    pub end_damping: f32,
    pub end_elasticity: f32,
    pub end_stiffness: f32,
    pub end_inert: f32,
    pub params: Vec<BoneParam>,
}

#[derive(Debug, Clone)]
pub struct Ver02Def {
    pub node: usize,
    /// The game tells the bust and hip components apart by this text.
    pub comment: String,
    pub root: Option<usize>,
    pub update_rate: f32,
    pub reflect_speed: f32,
    pub heavy_loop_max: u32,
    pub gravity: Vec3,
    pub force: Vec3,
    pub colliders: Vec<usize>,
    pub bones: Vec<usize>,
    pub patterns: Vec<Pattern>,
    pub enabled: bool,
}

impl Default for Ver02Def {
    fn default() -> Self {
        Self {
            node: 0,
            comment: String::new(),
            root: None,
            update_rate: 60.0,
            reflect_speed: 1.0,
            heavy_loop_max: 3,
            gravity: Vec3::ZERO,
            force: Vec3::ZERO,
            colliders: Vec::new(),
            bones: Vec::new(),
            patterns: Vec::new(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Ver02 {
    pub def: Ver02Def,
    particles: Vec<Particle>,
    /// `BonePtn.ParticlePtns` of each pattern.
    ptns: Vec<Vec<Particle>>,
    pattern: usize,
    gravity: Vec3,
    object_move: Vec3,
    object_prev: Vec3,
    object_scale: f32,
    time: f32,
    weight: f32,
}

impl Ver02 {
    pub fn new(def: Ver02Def) -> Self {
        let gravity = def.gravity;
        Self {
            def,
            particles: Vec::new(),
            ptns: Vec::new(),
            pattern: 0,
            gravity,
            object_move: Vec3::ZERO,
            object_prev: Vec3::ZERO,
            object_scale: 1.0,
            time: 0.0,
            weight: 1.0,
        }
    }

    /// `Awake`.
    fn start(&mut self, scene: &mut Scene) {
        self.init_node_particle(scene);
        self.setup_particles(scene);
        self.init_local_position(scene);
        if self.is_ref_transform() {
            self.set_ptn(0);
        }
        self.init_transforms(scene);
    }

    /// `InitNodeParticle`.
    fn init_node_particle(&mut self, scene: &Scene) {
        self.ptns.clear();
        for pattern in &self.def.patterns {
            let mut list = Vec::new();
            if pattern.params.len() == self.def.bones.len() {
                for param in &pattern.params {
                    let mut p = param.particle();
                    if let Some(r) = param.ref_node {
                        let l = &scene.nodes[r].local;
                        (p.init_pos, p.init_rot, p.init_scale) = (l.pos, l.rot, l.scale);
                        p.ref_node = Some(r);
                    }
                    list.push(p);
                }
                let end = BoneParam {
                    damping: pattern.end_damping,
                    elasticity: pattern.end_elasticity,
                    stiffness: pattern.end_stiffness,
                    inert: pattern.end_inert,
                    ..Default::default()
                };
                let mut p = end.particle();
                p.end_offset = pattern.end_offset;
                list.push(p);
            }
            self.ptns.push(list);
        }
    }

    /// `SetupParticles`.
    fn setup_particles(&mut self, scene: &Scene) {
        self.particles.clear();
        if self.def.root.is_none() {
            self.def.root = self.def.bones.first().copied();
        }
        let Some(root) = self.def.root else { return };
        let Some(first) = self.def.patterns.first() else { return };
        if self.def.bones.is_empty() || self.def.bones.len() != first.params.len() {
            return;
        }
        self.object_scale = lossy_scale_x(scene, root);
        self.object_prev = position(scene, root);
        self.object_move = Vec3::ZERO;
        for (i, (&bone, param)) in self.def.bones.iter().zip(&first.params).enumerate() {
            let mut p = param.particle();
            p.node = Some(bone);
            p.parent = i.checked_sub(1);
            p.pos = position(scene, bone);
            p.prev = p.pos;
            p.init_pos = scene.nodes[bone].local.pos;
            p.init_rot = scene.nodes[bone].local.rot;
            p.ref_node = Some(bone);
            if let Some(pn) = p.parent.and_then(|pi| self.particles[pi].node) {
                p.local_pos = world(scene, pn).inverse().transform_point3(p.pos);
            }
            self.particles.push(p);
        }
        let last = self.particles.len() - 1;
        let mut end = BoneParam::default().particle();
        end.parent = Some(last);
        end.end_offset = first.end_offset;
        if let Some(n) = self.particles[last].node {
            end.pos = world(scene, n).transform_point3(end.end_offset);
            end.prev = end.pos;
        }
        self.particles.push(end);
    }

    /// `InitLocalPosition`: rest transforms and bone offsets of every
    /// pattern, measured on its reference bones.
    fn init_local_position(&mut self, scene: &mut Scene) {
        let saved: Vec<_> = self
            .particles
            .iter()
            .map(|p| p.node.map(|n| scene.nodes[n].local))
            .collect();
        for (pattern, ptns) in self.def.patterns.iter().zip(&mut self.ptns) {
            for (param, ptn) in pattern.params.iter().zip(ptns.iter_mut()) {
                let Some(r) = param.ref_node else { continue };
                let l = &scene.nodes[r].local;
                (ptn.init_pos, ptn.init_rot, ptn.init_scale) = (l.pos, l.rot, l.scale);
                ptn.ref_node = Some(r);
            }
            if ptns.len() == self.particles.len() {
                for (p, ptn) in self.particles.iter().zip(ptns.iter()) {
                    if let Some(n) = p.node {
                        let l = &mut scene.nodes[n].local;
                        (l.pos, l.rot, l.scale) = (ptn.init_pos, ptn.init_rot, ptn.init_scale);
                    }
                }
            }
            for m in 1..pattern.params.len().min(ptns.len()) {
                if let (Some(a), Some(b)) = (pattern.params[m].ref_node, pattern.params[m - 1].ref_node) {
                    ptns[m].local_pos = world(scene, b).inverse().transform_point3(position(scene, a));
                }
            }
        }
        for (p, saved) in self.particles.iter().zip(saved) {
            if let (Some(n), Some(l)) = (p.node, saved) {
                scene.nodes[n].local = l;
            }
        }
    }

    /// `IsRefTransform`.
    fn is_ref_transform(&self) -> bool {
        self.def
            .patterns
            .iter()
            .all(|p| p.params.iter().all(|b| b.ref_node.is_some()))
    }

    /// `setPtn` with `_isSameForcePtn`.
    fn set_ptn(&mut self, ptn: usize) -> bool {
        if self.particles.is_empty() || ptn >= self.def.patterns.len() {
            return false;
        }
        if self.particles.len() != self.ptns[ptn].len() {
            return false;
        }
        self.pattern = ptn;
        self.gravity = self.def.patterns[ptn].gravity;
        for (p, t) in self.particles.iter_mut().zip(&self.ptns[ptn]) {
            *p = Particle {
                node: p.node,
                parent: p.parent,
                pos: p.pos,
                prev: p.prev,
                ..t.clone()
            };
        }
        true
    }

    /// `ResetPosition`, which the game runs after the body shape changes.
    pub fn reset_position(&mut self, scene: &mut Scene) {
        self.init_local_position(scene);
        self.set_ptn(self.pattern);
        if self.def.enabled {
            self.init_transforms(scene);
        }
    }

    /// `setSoftParams(ptn, -1, ...)`: every bone of one pattern, and the
    /// live particles.
    pub fn set_soft_params(&mut self, ptn: usize, damping: f32, elasticity: f32, stiffness: f32) {
        if self.particles.is_empty() || ptn >= self.def.patterns.len() {
            return;
        }
        for p in &mut self.particles {
            (p.damping, p.elasticity, p.stiffness) = (damping, elasticity, stiffness);
        }
        for p in &mut self.ptns[ptn] {
            (p.damping, p.elasticity, p.stiffness) = (damping, elasticity, stiffness);
        }
        for p in &mut self.def.patterns[ptn].params {
            (p.damping, p.elasticity, p.stiffness) = (damping, elasticity, stiffness);
        }
    }

    /// `setGravity(ptn, gravity)`.
    pub fn set_gravity(&mut self, ptn: usize, gravity: Vec3) {
        if self.particles.is_empty() || ptn >= self.def.patterns.len() {
            return;
        }
        self.gravity = gravity;
        self.def.patterns[ptn].gravity = gravity;
    }

    fn running(&self) -> bool {
        self.def.enabled && self.weight > 0.0
    }

    /// `InitTransforms`: bones take the transforms of their reference bones.
    fn init_transforms(&self, scene: &mut Scene) {
        for p in &self.particles {
            let Some(n) = p.node else { continue };
            let (pos, rot, scale) = match p.ref_node {
                Some(r) => {
                    let l = &scene.nodes[r].local;
                    (l.pos, l.rot, l.scale)
                }
                None => (p.init_pos, p.init_rot, p.init_scale),
            };
            let l = &mut scene.nodes[n].local;
            (l.pos, l.rot, l.scale) = (pos, rot, scale);
        }
    }

    /// `LateUpdate`.
    fn late_update(&mut self, scene: &mut Scene, colliders: &[Collider], dt: f32) {
        self.init_transforms(scene);
        let Some(root) = self.def.root else { return };
        if self.particles.is_empty() {
            return;
        }
        self.object_scale = lossy_scale_x(scene, root).abs();
        let here = position(scene, root);
        self.object_move = here - self.object_prev;
        self.object_prev = here;
        let n = loops(&mut self.time, self.def.update_rate, dt, self.def.heavy_loop_max);
        if n > 0 {
            let mats = rest_mats(scene, &self.particles);
            let shapes = shapes(scene, colliders, &self.def.colliders, true);
            for _ in 0..n {
                self.update_particles1(&mats);
                self.update_particles2(&mats, &shapes);
                self.object_move = Vec3::ZERO;
            }
        }
        self.apply(scene);
    }

    fn update_particles1(&mut self, mats: &[Mat4]) {
        if self.def.patterns.is_empty() {
            return;
        }
        let gravity = (self.gravity + self.def.force) * self.object_scale;
        for (i, p) in self.particles.iter_mut().enumerate() {
            if p.parent.is_some() {
                let v = (p.pos - p.prev) * self.def.reflect_speed;
                let moved = self.object_move * p.inert;
                p.prev = p.pos + moved;
                p.pos += v * (1.0 - p.damping) + gravity + moved;
            } else {
                p.prev = p.pos;
                p.pos = mats[i].w_axis.truncate();
            }
        }
    }

    fn update_particles2(&mut self, mats: &[Mat4], shapes: &[Shape]) {
        for i in 1..self.particles.len() {
            let Some(pi) = self.particles[i].parent else { continue };
            let parent_pos = self.particles[pi].pos;
            let p = &mut self.particles[i];
            let rest_len = match p.node {
                Some(_) => (mats[pi].w_axis - mats[i].w_axis).truncate().length(),
                None => p.end_offset.length() * self.object_scale,
            };
            let mut m = mats[pi];
            m.w_axis = parent_pos.extend(1.0);
            let rest = m.transform_point3(if p.node.is_some() { p.local_pos } else { p.end_offset });
            shape_pull(p, rest, rest_len, self.weight, true);
            let radius = p.radius * self.object_scale;
            for s in shapes {
                s.collide(&mut p.pos, radius);
            }
            let v = parent_pos - p.pos;
            let len = v.length();
            if len > 0.0 {
                let over = (len - rest_len) / len;
                if p.keep_min >= over {
                    p.pos += v * (over - p.keep_min);
                } else if over >= p.keep_max {
                    p.pos += v * (over - p.keep_max);
                }
            }
            if p.node.is_some() && p.move_limit {
                let mut m = mats[i];
                m.w_axis = rest.extend(1.0);
                let local = m.inverse().transform_point3(p.pos).clamp(p.move_min, p.move_max);
                p.pos = m.transform_point3(local);
            }
        }
    }

    /// `ApplyParticlesToTransforms`.
    fn apply(&self, scene: &mut Scene) {
        for i in 1..self.particles.len() {
            let p = &self.particles[i];
            let Some(pi) = p.parent else { continue };
            let parent = &self.particles[pi];
            let Some(parent_node) = parent.node else { continue };
            if parent.rotation_calc {
                let dir = if p.node.is_some() { p.local_pos } else { p.end_offset };
                let parent_rot = rotation(scene, parent_node);
                let from = parent_rot * dir;
                let mut to = p.pos - parent.pos;
                if dir.length() != 0.0 {
                    to = p.pos - from * (1.0 - parent.scale_next_bone_length) - parent.pos;
                }
                set_rotation(scene, parent_node, from_to(from, to) * parent_rot);
            }
            let Some(n) = p.node else { continue };
            if p.crush {
                let z = world(scene, n).inverse().transform_point3(p.pos).z;
                let inverse_lerp = |a: f32, b: f32, v: f32| {
                    if a != b {
                        ((v - a) / (b - a)).clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
                };
                let add = if z <= 0.0 {
                    p.crush_add_min * (1.0 - inverse_lerp(p.crush_area_min, 0.0, z))
                } else {
                    p.crush_add_max * inverse_lerp(0.0, p.crush_area_max, z)
                };
                scene.nodes[n].local.scale = p.init_scale + Vec3::new(add, add, 0.0);
            }
            set_position(scene, n, p.pos);
        }
    }
}

// ---------------------------------------------------------------------------
// Bust softness and weight (`BustSoft`, `BustGravity`).

/// Comments of the two bust components.
pub const BUST_COMMENTS: [&str; 2] = ["左胸", "右胸"];

/// `BustSoft.TreeLerp`.
fn tree_lerp(vals: [f32; 3], rate: f32) -> f32 {
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t.clamp(0.0, 1.0);
    if rate < 0.5 {
        lerp(vals[0], vals[1], rate * 2.0)
    } else {
        lerp(vals[1], vals[2], (rate - 0.5) * 2.0)
    }
}

/// `BustSoft.ReCalc`: damping, elasticity and stiffness of the bust bones
/// from the card's softness and bust size.
pub fn bust_soft(softness: f32, size: f32) -> (f32, f32, f32) {
    let rate = (softness * size + 0.01).clamp(0.0, 1.0);
    (
        tree_lerp([0.2, 0.1, 0.1], rate),
        tree_lerp([0.2, 0.15, 0.05], rate),
        tree_lerp([1.0, 0.1, 0.01], rate),
    )
}

/// `BustGravity.ReCalc`: gravity of the bust bones from the card's weight,
/// softness and bust size.
pub fn bust_gravity(weight: f32, softness: f32, size: f32) -> Vec3 {
    let y = (-0.005 * weight.clamp(0.0, 1.0)) * (size * softness * 0.5);
    Vec3::new(0.0, y, 0.0)
}

// ---------------------------------------------------------------------------
// Reading the components from prefab scripts.

/// Follows the references a script holds into the scene.
pub struct Resolve<'a> {
    /// Scene node of a Transform reference.
    pub node: &'a dyn Fn(PPtr) -> Option<usize>,
    /// Index in `Physics::colliders` of a DynamicBoneCollider reference.
    pub collider: &'a dyn Fn(PPtr) -> Option<usize>,
}

fn vec3(obj: &UObject, name: &str) -> Vec3 {
    obj.get(name)
        .and_then(|v| v.floats::<3>())
        .map(Vec3::from)
        .unwrap_or(Vec3::ZERO)
}

fn float(obj: &UObject, name: &str, default: f32) -> f32 {
    obj.f32(name).unwrap_or(default)
}

fn flag(obj: &UObject, name: &str, default: bool) -> bool {
    obj.get(name).and_then(|v| v.as_bool()).unwrap_or(default)
}

fn curve(obj: &UObject, name: &str) -> Option<Curve> {
    let keys = obj.get(name)?.get("m_Curve")?.as_array()?;
    let keys: Vec<Key> = keys
        .iter()
        .filter_map(|k| {
            let k = k.as_object()?;
            Some(Key {
                time: k.f32("time")?,
                value: k.f32("value")?,
                in_slope: k.f32("inSlope").unwrap_or(0.0),
                out_slope: k.f32("outSlope").unwrap_or(0.0),
            })
        })
        .collect();
    (!keys.is_empty()).then_some(Curve { keys })
}

fn refs(obj: &UObject, name: &str, resolve: &dyn Fn(PPtr) -> Option<usize>) -> Vec<usize> {
    obj.array(name)
        .unwrap_or_default()
        .iter()
        .filter_map(|v| resolve(v.as_pptr()?))
        .collect()
}

pub fn read_collider(obj: &UObject, node: usize) -> Collider {
    Collider {
        node,
        center: vec3(obj, "m_Center"),
        radius: float(obj, "m_Radius", 0.5),
        height: float(obj, "m_Height", 0.0),
        direction: obj.i64("m_Direction").unwrap_or(0).clamp(0, 2) as u8,
        inside: obj.i64("m_Bound").unwrap_or(0) == 1,
        enabled: flag(obj, "m_Enabled", true),
    }
}

pub fn read_dynamic_bone(obj: &UObject, node: usize, r: &Resolve) -> DynamicBoneDef {
    DynamicBoneDef {
        node,
        root: obj.pptr("m_Root").and_then(r.node),
        update_rate: float(obj, "m_UpdateRate", 60.0),
        damping: float(obj, "m_Damping", 0.1),
        damping_distrib: curve(obj, "m_DampingDistrib"),
        elasticity: float(obj, "m_Elasticity", 0.1),
        elasticity_distrib: curve(obj, "m_ElasticityDistrib"),
        stiffness: float(obj, "m_Stiffness", 0.1),
        stiffness_distrib: curve(obj, "m_StiffnessDistrib"),
        inert: float(obj, "m_Inert", 0.0),
        inert_distrib: curve(obj, "m_InertDistrib"),
        radius: float(obj, "m_Radius", 0.0),
        radius_distrib: curve(obj, "m_RadiusDistrib"),
        end_length: float(obj, "m_EndLength", 0.0),
        end_offset: vec3(obj, "m_EndOffset"),
        gravity: vec3(obj, "m_Gravity"),
        force: vec3(obj, "m_Force"),
        colliders: refs(obj, "m_Colliders", r.collider),
        exclusions: refs(obj, "m_Exclusions", r.node),
        not_rolls: refs(obj, "m_notRolls", r.node),
        freeze_axis: obj.i64("m_FreezeAxis").unwrap_or(0).clamp(0, 3) as u8,
        weight: 1.0,
        enabled: flag(obj, "m_Enabled", true),
    }
}

pub fn read_ver01(obj: &UObject, node: usize, r: &Resolve) -> Ver01Def {
    let nodes = obj
        .array("m_Nodes")
        .unwrap_or_default()
        .iter()
        .filter_map(|n| {
            let n = n.as_object()?;
            Some(((r.node)(n.pptr("Transform")?)?, flag(n, "RotationCalc", true)))
        })
        .collect();
    Ver01Def {
        node,
        root: obj.pptr("m_Root").and_then(r.node),
        update_rate: float(obj, "m_UpdateRate", 60.0),
        damping: float(obj, "m_Damping", 0.1),
        damping_distrib: curve(obj, "m_DampingDistrib"),
        elasticity: float(obj, "m_Elasticity", 0.1),
        elasticity_distrib: curve(obj, "m_ElasticityDistrib"),
        stiffness: float(obj, "m_Stiffness", 0.1),
        stiffness_distrib: curve(obj, "m_StiffnessDistrib"),
        inert: float(obj, "m_Inert", 0.0),
        inert_distrib: curve(obj, "m_InertDistrib"),
        radius: float(obj, "m_Radius", 0.0),
        radius_distrib: curve(obj, "m_RadiusDistrib"),
        add_angle_scale: float(obj, "m_AddAngleScale", 1.0),
        add_angle_scale_distrib: curve(obj, "m_AddAngleScaleDistrib"),
        end_length: float(obj, "m_EndLength", 0.0),
        end_offset: vec3(obj, "m_EndOffset"),
        gravity: vec3(obj, "m_Gravity"),
        force: vec3(obj, "m_Force"),
        colliders: refs(obj, "m_Colliders", r.collider),
        nodes,
        enabled: flag(obj, "m_Enabled", true),
    }
}

pub fn read_ver02(obj: &UObject, node: usize, r: &Resolve) -> Ver02Def {
    let param = |p: &UObject| BoneParam {
        ref_node: p.pptr("RefTransform").and_then(r.node),
        rotation_calc: flag(p, "IsRotationCalc", false),
        damping: float(p, "Damping", 0.0),
        elasticity: float(p, "Elasticity", 0.0),
        stiffness: float(p, "Stiffness", 0.0),
        inert: float(p, "Inert", 0.0),
        next_bone_length: float(p, "NextBoneLength", 1.0),
        collision_radius: float(p, "CollisionRadius", 0.0),
        move_limit: flag(p, "IsMoveLimit", false),
        move_min: vec3(p, "MoveLimitMin"),
        move_max: vec3(p, "MoveLimitMax"),
        keep_min: float(p, "KeepLengthLimitMin", 0.0),
        keep_max: float(p, "KeepLengthLimitMax", 0.0),
        crush: flag(p, "IsCrush", false),
        crush_area_min: float(p, "CrushMoveAreaMin", 0.0),
        crush_area_max: float(p, "CrushMoveAreaMax", 0.0),
        crush_add_min: float(p, "CrushAddXYMin", 0.0),
        crush_add_max: float(p, "CrushAddXYMax", 0.0),
    };
    let patterns = obj
        .array("Patterns")
        .unwrap_or_default()
        .iter()
        .filter_map(|p| {
            let p = p.as_object()?;
            Some(Pattern {
                gravity: vec3(p, "Gravity"),
                end_offset: vec3(p, "EndOffset"),
                end_damping: float(p, "EndOffsetDamping", 0.0),
                end_elasticity: float(p, "EndOffsetElasticity", 0.0),
                end_stiffness: float(p, "EndOffsetStiffness", 0.0),
                end_inert: float(p, "EndOffsetInert", 0.0),
                params: p
                    .array("Params")
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|b| b.as_object().map(param))
                    .collect(),
            })
        })
        .collect();
    Ver02Def {
        node,
        comment: obj.str("Comment").unwrap_or("").to_string(),
        root: obj.pptr("Root").and_then(r.node),
        update_rate: float(obj, "UpdateRate", 60.0),
        reflect_speed: float(obj, "ReflectSpeed", 1.0),
        heavy_loop_max: obj.i64("HeavyLoopMaxCount").unwrap_or(3).max(0) as u32,
        gravity: vec3(obj, "Gravity"),
        force: vec3(obj, "Force"),
        colliders: refs(obj, "Colliders", r.collider),
        bones: refs(obj, "Bones", r.node),
        patterns,
        enabled: flag(obj, "m_Enabled", true),
    }
}

// ---------------------------------------------------------------------------
// The whole character.

#[derive(Debug, Clone)]
pub enum Bone {
    Dynamic(DynamicBone),
    Ver01(Ver01),
    Ver02(Ver02),
}

impl Bone {
    /// `ResetParticlesPosition`: the particles go back on their bones and
    /// the component's place becomes the one it moves from, so that the
    /// next update does not take the jump for movement.
    fn reset(&mut self, scene: &Scene) {
        match self {
            Bone::Dynamic(d) => {
                reset_particles(scene, &mut d.particles);
                d.object_prev = position(scene, d.def.node);
            }
            Bone::Ver01(d) => {
                reset_particles(scene, &mut d.particles);
                d.object_prev = position(scene, d.def.node);
            }
            Bone::Ver02(d) => {
                reset_particles(scene, &mut d.particles);
                if let Some(root) = d.def.root {
                    d.object_prev = position(scene, root);
                }
            }
        }
    }

    /// The object the component sits on and whether the component itself
    /// is switched on.
    fn component(&self) -> (usize, bool) {
        match self {
            Bone::Dynamic(d) => (d.def.node, d.def.enabled),
            Bone::Ver01(d) => (d.def.node, d.def.enabled),
            Bone::Ver02(d) => (d.def.node, d.def.enabled),
        }
    }

    /// `OnDisable`: the bones go back to their rest transforms.
    fn init_transforms(&self, scene: &mut Scene) {
        match self {
            Bone::Dynamic(d) => d.init_transforms(scene),
            Bone::Ver01(d) => d.init_transforms(scene),
            Bone::Ver02(d) => d.init_transforms(scene),
        }
    }
}

/// What a physics component came with, so that a kind can be switched
/// off: the body (its bust and buttocks too), hair, clothes or an
/// accessory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Belongs {
    #[default]
    Body,
    Hair,
    Clothes,
    Accessory,
}

impl Belongs {
    pub const ALL: [Belongs; 4] = [Belongs::Hair, Belongs::Clothes, Belongs::Body, Belongs::Accessory];
}

/// Every DynamicBone component of a character with the colliders they use.
#[derive(Debug, Clone, Default)]
pub struct Physics {
    pub colliders: Vec<Collider>,
    pub bones: Vec<Bone>,
    /// What each of `bones` came with; a bone without one is the body's.
    pub belongs: Vec<Belongs>,
    /// The kinds that hold still at their rest pose instead of moving.
    still: Vec<Belongs>,
    /// Whether each of `bones` sat on an object that was active when last
    /// looked at; a component of a hidden object does not run.
    active: Vec<bool>,
    /// Advanced Skirt Physics (`cloth`), once fitted to the character.
    cloth: Option<cloth::Cloth>,
    /// Whether it runs; off leaves the skirts exactly as the game moves
    /// them.
    advanced: bool,
    /// What the character is among, for Advanced Skirt Physics.
    obstacles: Option<std::sync::Arc<cloth::Obstacles>>,
    /// While set, `end_frame` leaves the cloth nets as they are
    /// (`hold_cloth`).
    cloth_held: bool,
    /// Which of `bones` ran in the last `end_frame`.
    ran: Vec<bool>,
}

impl Physics {
    /// `Awake` and `Start` of every component: particles are created where
    /// the bones are now.
    pub fn start(&mut self, scene: &mut Scene) {
        for b in &mut self.bones {
            match b {
                Bone::Dynamic(d) => d.start(scene),
                Bone::Ver01(d) => d.start(scene),
                Bone::Ver02(d) => d.start(scene),
            }
        }
        self.active = self
            .bones
            .iter()
            .map(|b| active_in_hierarchy(scene, b.component().0))
            .collect();
    }

    /// What Unity does when an object with a component is switched off or
    /// on, as clothes are by their states and accessories by theirs:
    /// `OnDisable` puts the component's bones back to their rest
    /// transforms, and `OnEnable` starts its particles again from where
    /// the bones are (`ResetParticlesPosition`). A component on an object
    /// that is off runs neither `Update` nor `LateUpdate`.
    pub fn follow_objects(&mut self, scene: &mut Scene) {
        self.active.resize(self.bones.len(), true);
        for (b, was) in self.bones.iter_mut().zip(self.active.iter_mut()) {
            let (node, enabled) = b.component();
            let now = active_in_hierarchy(scene, node);
            if now == *was {
                continue;
            }
            *was = now;
            if !enabled {
                continue;
            }
            if now {
                b.reset(scene);
            } else {
                b.init_transforms(scene);
            }
        }
    }

    /// Whether the bone at `i` sits on an object that is on.
    fn shown(&self, i: usize) -> bool {
        self.active.get(i).copied().unwrap_or(true)
    }

    /// What `ChaControl.LateUpdateForce` does once the body shape is set:
    /// the bust takes its rest pose from the shaped reference bones, then
    /// its softness and gravity from the card.
    pub fn setup_bust(&mut self, scene: &mut Scene, softness: f32, weight: f32, size: f32) {
        let (damping, elasticity, stiffness) = bust_soft(softness, size);
        let gravity = bust_gravity(weight, softness, size);
        for b in &mut self.bones {
            let Bone::Ver02(d) = b else { continue };
            if !BUST_COMMENTS.contains(&d.def.comment.as_str()) {
                continue;
            }
            d.reset_position(scene);
            d.set_soft_params(0, damping, elasticity, stiffness);
            d.set_gravity(0, gravity);
        }
    }

    /// One rendered frame of `dt` seconds.
    pub fn step(&mut self, scene: &mut Scene, dt: f32) {
        self.begin_frame(scene);
        self.end_frame(scene, dt);
    }

    /// The first half of a frame, the components' `Update`: the bones go
    /// back to where they were loaded. An animation writes its bones
    /// after this and before `end_frame`, as Unity's Animator does.
    pub fn begin_frame(&mut self, scene: &mut Scene) {
        self.follow_objects(scene);
        for (i, b) in self.bones.iter().enumerate() {
            if !self.shown(i) {
                continue;
            }
            let moves = self.moves(i);
            match b {
                Bone::Dynamic(d) if d.running() => d.init_transforms(scene),
                Bone::Ver01(d) if d.running() => d.init_transforms(scene),
                // A kind switched off holds its rest pose.
                Bone::Ver02(d) if d.running() && !moves => d.init_transforms(scene),
                _ => {}
            }
        }
    }

    /// Whether the bone at `i` moves: its kind is not held still.
    fn moves(&self, i: usize) -> bool {
        let belongs = self.belongs.get(i).copied().unwrap_or_default();
        !self.still.contains(&belongs)
    }

    /// Holds the kinds in `still` at their rest pose. A kind that moves
    /// again starts from where its bones are, without a jump.
    pub fn hold_still(&mut self, scene: &Scene, still: &[Belongs]) {
        if self.still == still {
            return;
        }
        for (i, b) in self.bones.iter_mut().enumerate() {
            let belongs = self.belongs.get(i).copied().unwrap_or_default();
            if self.still.contains(&belongs) && !still.contains(&belongs) {
                b.reset(scene);
            }
        }
        self.still = still.to_vec();
    }

    /// The second half of a frame, the components' `LateUpdate`: the
    /// simulation moves on by `dt` seconds and poses the bones.
    pub fn end_frame(&mut self, scene: &mut Scene, dt: f32) {
        self.follow_objects(scene);
        let moving: Vec<bool> = (0..self.bones.len()).map(|i| self.moves(i) && self.shown(i)).collect();
        let cloth = self.cloth.as_mut().filter(|_| self.advanced);
        if let Some(cloth) = cloth {
            cloth.begin(scene, &self.bones);
        }
        let mut live = vec![false; self.bones.len()];
        for ((b, moves), live) in self.bones.iter_mut().zip(moving).zip(&mut live) {
            if !moves {
                continue;
            }
            match b {
                Bone::Dynamic(d) if d.running() => {
                    d.late_update(scene, &self.colliders, dt);
                    *live = true;
                }
                Bone::Ver01(d) if d.running() => d.late_update(scene, &self.colliders, dt),
                Bone::Ver02(d) if d.running() => d.late_update(scene, &self.colliders, dt),
                _ => {}
            }
        }
        if let Some(cloth) = self.cloth.as_mut().filter(|_| self.advanced) {
            cloth.solve(scene, &mut self.bones, &live, self.obstacles.as_ref());
            if !self.cloth_held {
                cloth.step_nets(scene, &live, self.obstacles.as_ref(), dt);
            }
        }
        self.ran = live;
        // The vertices between the skirt's bones, after skinning.
        scene.push_out = match &self.cloth {
            Some(cloth) if self.advanced => cloth.push_out(self.has_obstacles()),
            _ => None,
        };
        scene.cloth = match &self.cloth {
            Some(cloth) if self.advanced => cloth.draw(),
            _ => None,
        };
        scene.drape = match &self.cloth {
            Some(cloth) if self.advanced => cloth.drape(),
            _ => None,
        };
    }

    /// Has the GPU solver (`drape`, run by the renderer) move the skirts'
    /// nets in place of the CPU's net solver. Advanced Skirt Physics
    /// must be on for either.
    pub fn solve_skirts_on_gpu(&mut self, on: bool) {
        if let Some(cloth) = &mut self.cloth {
            cloth.solve_on_gpu(on);
        }
    }

    /// The fastest a part of the body went in a frame since the GPU
    /// solver started, as seen from the waist, in metres.
    pub fn fastest_limb(&self) -> f32 {
        self.cloth.as_ref().map_or(0.0, cloth::Cloth::fastest_limb)
    }

    /// One text per cloth net for the walking score (`rk render
    /// --skirt-dump`); `gpu` is the GPU solver's state read back.
    pub fn skirt_dump(&self, scene: &Scene, gpu: Option<drape::Readback<'_>>) -> Vec<String> {
        match &self.cloth {
            Some(cloth) if self.advanced => cloth.dump(scene, gpu),
            _ => Vec::new(),
        }
    }

    /// While `held`, `end_frame` moves everything but the cloth nets,
    /// which stay as they are until `settle_cloth` moves them. A scene
    /// that stands still settles its hair and chains at once and its
    /// nets, which cost far more, over the frames it is drawn in.
    pub fn hold_cloth(&mut self, held: bool) {
        self.cloth_held = held;
    }

    /// Has `solver` run the steps of the cloth nets in place of the CPU's
    /// own solver; none goes back to that. Call after `fit_cloth`.
    pub fn solve_cloth_with(&mut self, solver: Option<net::SharedSolver>) {
        if let Some(cloth) = &mut self.cloth {
            cloth.solve_with(solver);
        }
    }

    /// One line per skirt that Advanced Skirt Physics found, whether it
    /// runs or not: its chains, its net and the meshes on the net.
    pub fn skirts(&self, scene: &Scene) -> Vec<String> {
        self.cloth
            .as_ref()
            .map_or_else(Vec::new, |c| c.summary(scene, &self.bones))
    }

    /// Whether Advanced Skirt Physics runs and the character has cloth
    /// nets for it.
    pub fn has_nets(&self) -> bool {
        self.advanced && self.cloth.as_ref().is_some_and(cloth::Cloth::has_nets)
    }

    /// Moves only the cloth nets on by `frames` frames of `dt` seconds,
    /// among the body as it is posed now. The first of them takes the
    /// body from where the nets last saw it to where it is. With
    /// Advanced Skirt Physics off the skirts are drawn as the game moves
    /// them.
    pub fn settle_cloth(&mut self, scene: &mut Scene, frames: u32, dt: f32) {
        let Some(cloth) = self.cloth.as_mut().filter(|_| self.advanced) else {
            scene.cloth = None;
            scene.drape = None;
            scene.push_out = None;
            return;
        };
        for _ in 0..frames {
            cloth.step_nets(scene, &self.ran, self.obstacles.as_ref(), dt);
        }
        scene.cloth = cloth.draw();
        scene.drape = cloth.drape();
    }

    /// Fits Advanced Skirt Physics to the character as it is now, which
    /// should be the pose it was built in, and switches it `on` or off.
    pub fn fit_cloth(&mut self, scene: &Scene, names: &std::collections::HashMap<String, usize>, on: bool) {
        self.cloth = Some(cloth::Cloth::build(scene, names, &self.bones));
        self.advanced = on;
    }

    /// What the character is among (a map's floors and furniture), for
    /// Advanced Skirt Physics to rest skirts on; `None` for nothing.
    /// Returns whether that is something else than before.
    pub fn set_obstacles(&mut self, obstacles: Option<std::sync::Arc<cloth::Obstacles>>) -> bool {
        let same = match (&self.obstacles, &obstacles) {
            (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        self.obstacles = obstacles;
        !same
    }

    /// Whether there is anything around the character for skirts to rest
    /// on.
    pub fn has_obstacles(&self) -> bool {
        self.obstacles.as_ref().is_some_and(|o| !o.pieces.is_empty())
    }

    /// Whether Advanced Skirt Physics runs.
    pub fn advanced(&self) -> bool {
        self.advanced && self.cloth.is_some()
    }

    /// Switches Advanced Skirt Physics on or off; true when that changed
    /// anything, so that the skirts are to settle again.
    pub fn set_advanced(&mut self, on: bool) -> bool {
        let was = self.advanced();
        self.advanced = on;
        self.advanced() != was
    }

    /// With Advanced Skirt Physics on, a new pose is reached over
    /// `EASE_FRAMES` frames of the settle instead of in one jump, so that
    /// a leg moving into a skirt pushes it instead of passing through it.
    /// Call after the pose was written: every bone the physics does not
    /// drive goes from `from` towards it by `amount`, from 0 to 1.
    pub fn ease(&self, scene: &mut Scene, from: &[crate::scene::Trs], amount: f32) {
        if !self.advanced() || amount >= 1.0 {
            return;
        }
        let mut driven = vec![false; scene.nodes.len()];
        for b in &self.bones {
            let ps = match b {
                Bone::Dynamic(d) => &d.particles,
                Bone::Ver01(d) => &d.particles,
                Bone::Ver02(d) => &d.particles,
            };
            for n in ps.iter().filter_map(|p| p.node) {
                driven[n] = true;
            }
        }
        for ((node, f), driven) in scene.nodes.iter_mut().zip(from).zip(driven) {
            if driven {
                continue;
            }
            let l = &mut node.local;
            l.pos = f.pos.lerp(l.pos, amount);
            l.rot = f.rot.slerp(l.rot, amount);
            l.scale = f.scale.lerp(l.scale, amount);
        }
    }

    /// Runs `frames` frames of `dt` seconds each and leaves the bones posed.
    pub fn settle(&mut self, scene: &mut Scene, frames: u32, dt: f32) {
        for _ in 0..frames {
            self.step(scene, dt);
        }
    }

    /// Largest particle movement of the last simulated step, in units.
    pub fn speed(&self) -> f32 {
        let particles = |b: &Bone| -> Vec<(Vec3, Vec3)> {
            let ps = match b {
                Bone::Dynamic(d) => &d.particles,
                Bone::Ver01(d) => &d.particles,
                Bone::Ver02(d) => &d.particles,
            };
            ps.iter()
                .filter(|p| p.parent.is_some())
                .map(|p| (p.pos, p.prev))
                .collect()
        };
        self.bones
            .iter()
            .flat_map(particles)
            .map(|(a, b)| (a - b).length())
            .fold(0.0, f32::max)
    }

    /// Puts every particle back on its bone, as `ResetParticlesPosition`.
    pub fn reset(&mut self, scene: &Scene) {
        for b in &mut self.bones {
            b.reset(scene);
        }
        if let Some(cloth) = &mut self.cloth {
            cloth.reset_nets();
        }
    }

    /// Starts the cloth nets again in the pose the body is in at the next
    /// step: they then take the body from their rest shape into that pose
    /// at their own pace (`Net::start`), whatever way the bones came by.
    pub fn restart_cloth(&mut self) {
        if let Some(cloth) = &mut self.cloth {
            cloth.reset_nets();
        }
    }

    /// Moves every particle with the character when it is placed somewhere
    /// else while nothing runs, so that hair and skirts keep the shape they
    /// settled in. `ResetParticlesPosition` would put them back on bones
    /// that hold their rest shape, which in a sitting or lying pose is
    /// inside the legs. `moved` is the change of the character's world
    /// transform.
    pub fn carry(&mut self, moved: Mat4) {
        let carry = |particles: &mut [Particle], prev: &mut Vec3| {
            for p in particles.iter_mut() {
                p.pos = moved.transform_point3(p.pos);
                p.prev = moved.transform_point3(p.prev);
            }
            *prev = moved.transform_point3(*prev);
        };
        for b in &mut self.bones {
            match b {
                Bone::Dynamic(d) => carry(&mut d.particles, &mut d.object_prev),
                Bone::Ver01(d) => carry(&mut d.particles, &mut d.object_prev),
                Bone::Ver02(d) => carry(&mut d.particles, &mut d.object_prev),
            }
        }
        if let Some(cloth) = &mut self.cloth {
            cloth.carry(moved);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Trs;

    fn at(pos: Vec3) -> Trs {
        Trs {
            pos,
            ..Default::default()
        }
    }

    /// A root at `origin` with `count` bones below it, each `step` further.
    fn chain(scene: &mut Scene, origin: Vec3, step: Vec3, count: usize) -> Vec<usize> {
        let mut nodes = vec![scene.add_node("root", None, at(origin))];
        for i in 0..count {
            let parent = *nodes.last().unwrap();
            nodes.push(scene.add_node(&format!("bone{i}"), Some(parent), at(step)));
        }
        nodes
    }

    fn dynamic(def: DynamicBoneDef) -> Physics {
        Physics {
            colliders: Vec::new(),
            bones: vec![Bone::Dynamic(DynamicBone::new(def))],
            ..Physics::default()
        }
    }

    fn close(a: Vec3, b: Vec3, eps: f32) -> bool {
        (a - b).length() < eps
    }

    #[test]
    fn curve_is_hermite_between_keys_and_clamped_outside() {
        let key = |time, value, slope| Key {
            time,
            value,
            in_slope: slope,
            out_slope: slope,
        };
        let line = Curve {
            keys: vec![key(0.0, 1.0, -0.5), key(1.0, 0.5, -0.5)],
        };
        assert!((line.evaluate(0.25) - 0.875).abs() < 1e-6);
        assert_eq!(line.evaluate(-1.0), 1.0);
        assert_eq!(line.evaluate(2.0), 0.5);
        // Flat tangents ease in and out: the midpoint is the mean.
        let ease = Curve {
            keys: vec![key(0.0, 0.0, 0.0), key(2.0, 1.0, 0.0)],
        };
        assert!((ease.evaluate(1.0) - 0.5).abs() < 1e-6);
        assert!((ease.evaluate(0.5) - 0.15625).abs() < 1e-6);
        // No keys leave a value unscaled.
        assert_eq!(Curve::scale(&Some(Curve::default()), 0.3), 1.0);
        assert_eq!(Curve::scale(&None, 0.3), 1.0);
    }

    #[test]
    fn frames_run_steps_at_the_update_rate() {
        let mut time = 0.0;
        // One step per frame at the update rate.
        assert_eq!(loops(&mut time, 60.0, 1.0 / 60.0, 3), 1);
        // Half as long a frame steps every other time.
        let mut time = 0.0;
        let steps: Vec<u32> = (0..4).map(|_| loops(&mut time, 60.0, 1.0 / 120.0, 3)).collect();
        assert_eq!(steps.iter().sum::<u32>(), 2);
        // A long frame is cut off at the limit and the rest is dropped.
        let mut time = 0.0;
        assert_eq!(loops(&mut time, 60.0, 0.5, 3), 3);
        assert_eq!(time, 0.0);
        // No rate: once every frame.
        assert_eq!(loops(&mut time, 0.0, 0.5, 3), 1);
    }

    #[test]
    fn chain_under_a_force_settles_and_keeps_its_lengths() {
        let mut scene = Scene::default();
        let nodes = chain(&mut scene, Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -0.1, 0.0), 4);
        let mut physics = dynamic(DynamicBoneDef {
            node: nodes[0],
            root: Some(nodes[0]),
            damping: 0.2,
            elasticity: 0.05,
            stiffness: 0.0,
            force: Vec3::new(0.002, 0.0, 0.0),
            ..Default::default()
        });
        physics.start(&mut scene);
        physics.settle(&mut scene, 600, FRAME_TIME);
        assert!(physics.speed() < 1e-6, "still moving: {}", physics.speed());
        let tip = position(&scene, nodes[4]);
        // Pushed sideways, so it also rises, and the root stays put.
        assert!(tip.x > 0.02 && tip.y > 0.6);
        assert!(close(position(&scene, nodes[0]), Vec3::new(0.0, 1.0, 0.0), 1e-6));
        for w in nodes.windows(2) {
            let len = (position(&scene, w[0]) - position(&scene, w[1])).length();
            assert!((len - 0.1).abs() < 1e-4, "bone length {len}");
        }
        // Bones turn towards their child: the first bone's local -Y axis
        // points at the second bone.
        let dir = rotation(&scene, nodes[1]) * Vec3::NEG_Y;
        let to = (position(&scene, nodes[2]) - position(&scene, nodes[1])).normalize();
        assert!(close(dir, to, 1e-3));
        // More frames change nothing.
        physics.settle(&mut scene, 300, FRAME_TIME);
        assert!(close(position(&scene, nodes[4]), tip, 1e-5));
    }

    #[test]
    fn velocity_is_damped_and_force_scaled_as_the_step_says() {
        let mut scene = Scene::default();
        let nodes = chain(&mut scene, Vec3::ZERO, Vec3::new(0.0, -0.1, 0.0), 1);
        scene.nodes[nodes[0]].local.scale = Vec3::splat(2.0);
        let mut bone = DynamicBone::new(DynamicBoneDef {
            node: nodes[0],
            root: Some(nodes[0]),
            damping: 0.25,
            force: Vec3::new(0.0, 0.0, 0.01),
            ..Default::default()
        });
        bone.start(&scene);
        bone.object_scale = lossy_scale_x(&scene, nodes[0]).abs();
        let start = bone.particles[1].pos;
        bone.particles[1].prev = start - Vec3::new(0.04, 0.0, 0.0);
        let mats = rest_mats(&scene, &bone.particles);
        bone.update_particles1(&mats, Quat::IDENTITY);
        // Velocity 0.04 keeps 75 percent; the force is scaled by the object.
        let moved = bone.particles[1].pos - start;
        assert!(close(moved, Vec3::new(0.03, 0.0, 0.02), 1e-6));
        assert!(close(bone.particles[1].prev, start, 1e-6));
    }

    #[test]
    fn elasticity_and_stiffness_pull_towards_the_rest_place() {
        let rest = Vec3::new(0.0, -1.0, 0.0);
        // Elasticity covers that share of the way.
        let mut p = Particle {
            pos: Vec3::new(1.0, -1.0, 0.0),
            elasticity: 0.25,
            ..Default::default()
        };
        shape_pull(&mut p, rest, 1.0, 1.0, true);
        assert!(close(p.pos, Vec3::new(0.75, -1.0, 0.0), 1e-6));
        // Stiffness allows at most (1 - stiffness) * 2 bone lengths away.
        let mut p = Particle {
            pos: Vec3::new(1.0, -1.0, 0.0),
            stiffness: 0.75,
            ..Default::default()
        };
        shape_pull(&mut p, rest, 1.0, 1.0, true);
        assert!(close(p.pos, Vec3::new(0.5, -1.0, 0.0), 1e-6));
        // Full stiffness holds the rest shape.
        p.stiffness = 1.0;
        shape_pull(&mut p, rest, 1.0, 1.0, true);
        assert!(close(p.pos, rest, 1e-6));
        // At weight 0 the bone counts as fully stiff.
        let mut p = Particle {
            pos: Vec3::new(1.0, -1.0, 0.0),
            ..Default::default()
        };
        shape_pull(&mut p, rest, 1.0, 0.0, true);
        assert!(close(p.pos, rest, 1e-6));
    }

    #[test]
    fn gravity_only_acts_once_the_root_turns() {
        let mut scene = Scene::default();
        let holder = scene.add_node("holder", None, Trs::default());
        let root = scene.add_node("root", Some(holder), at(Vec3::new(0.0, 1.0, 0.0)));
        let a = scene.add_node("a", Some(root), at(Vec3::new(0.0, -0.1, 0.0)));
        let b = scene.add_node("b", Some(a), at(Vec3::new(0.0, -0.1, 0.0)));
        let mut physics = dynamic(DynamicBoneDef {
            node: root,
            root: Some(root),
            gravity: Vec3::new(0.0, -0.01, 0.0),
            stiffness: 0.0,
            ..Default::default()
        });
        physics.start(&mut scene);
        physics.settle(&mut scene, 30, FRAME_TIME);
        // The rest pose already shows gravity: nothing moves.
        assert!(close(position(&scene, b), Vec3::new(0.0, 0.8, 0.0), 1e-6));
        // Turned on its side, the chain sags from where the pose puts it.
        scene.nodes[holder].local.rot = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        physics.reset(&scene);
        let posed = position(&scene, b);
        physics.settle(&mut scene, 120, FRAME_TIME);
        assert!(position(&scene, b).y < posed.y - 0.01);
    }

    #[test]
    fn a_reset_is_not_taken_for_movement() {
        // A skirt-like chain with inertia, moved a metre and reset: the
        // next frame must not carry it along by the move, as
        // `ResetParticlesPosition` sets where the component moves from.
        let mut scene = Scene::default();
        let holder = scene.add_node("holder", None, Trs::default());
        let root = scene.add_node("root", Some(holder), at(Vec3::new(0.0, 1.0, 0.0)));
        let a = scene.add_node("a", Some(root), at(Vec3::new(0.0, -0.1, 0.0)));
        let b = scene.add_node("b", Some(a), at(Vec3::new(0.0, -0.1, 0.0)));
        let mut physics = dynamic(DynamicBoneDef {
            node: root,
            root: Some(root),
            inert: 0.8,
            stiffness: 0.0,
            elasticity: 0.0,
            damping: 0.0,
            ..Default::default()
        });
        physics.start(&mut scene);
        physics.settle(&mut scene, 10, FRAME_TIME);
        scene.nodes[holder].local.pos = Vec3::new(1.0, 0.0, 0.0);
        physics.reset(&scene);
        let placed = position(&scene, b);
        physics.step(&mut scene, FRAME_TIME);
        assert!(
            close(position(&scene, b), placed, 1e-5),
            "{:?} {placed:?}",
            position(&scene, b)
        );
    }

    /// A waist with four skirt chains round it, as the game names them,
    /// and their physics with Advanced Skirt Physics fitted and on.
    fn skirt() -> (Scene, Physics, usize) {
        let mut scene = Scene::default();
        let waist = scene.add_node("cf_j_waist02", None, at(Vec3::new(0.0, 1.0, 0.0)));
        let mut bones = Vec::new();
        for k in 0..4 {
            let angle = k as f32 * std::f32::consts::FRAC_PI_2;
            let mut parent = scene.add_node(
                &format!("cf_j_sk_{k:02}_00"),
                Some(waist),
                at(Vec3::new(angle.cos() * 0.1, 0.0, angle.sin() * 0.1)),
            );
            let root = parent;
            for i in 1..4 {
                parent = scene.add_node(
                    &format!("cf_j_sk_{k:02}_{i:02}"),
                    Some(parent),
                    at(Vec3::new(0.0, -0.1, 0.0)),
                );
            }
            bones.push(Bone::Dynamic(DynamicBone::new(DynamicBoneDef {
                node: waist,
                root: Some(root),
                enabled: true,
                ..Default::default()
            })));
        }
        let mut physics = Physics {
            bones,
            ..Physics::default()
        };
        physics.start(&mut scene);
        physics.fit_cloth(&scene, &std::collections::HashMap::new(), true);
        (scene, physics, waist)
    }

    #[test]
    fn held_cloth_nets_wait_for_their_own_settle() {
        let (mut scene, mut physics, waist) = skirt();
        assert!(physics.has_nets());
        let told = physics.skirts(&scene);
        assert_eq!(told.len(), 1);
        assert!(told[0].starts_with("4 chains") && told[0].contains("net "), "{told:?}");
        physics.step(&mut scene, FRAME_TIME);
        let top = |scene: &Scene| scene.cloth.as_ref().expect("a net").nets[0].pos[0][0];
        let was = top(&scene);
        // The body goes up half a metre while the nets are held: the
        // chains go along, the net stays.
        physics.hold_cloth(true);
        scene.nodes[waist].local.pos.y += 0.5;
        physics.step(&mut scene, FRAME_TIME);
        assert_eq!(top(&scene), was);
        // Its own settle takes it there.
        physics.hold_cloth(false);
        physics.settle_cloth(&mut scene, 1, FRAME_TIME);
        assert!(
            close(top(&scene), was + Vec3::new(0.0, 0.5, 0.0), 1e-4),
            "{}",
            top(&scene)
        );
        // With Advanced Skirt Physics off the skirts are drawn as the
        // game moves them.
        assert!(physics.set_advanced(false));
        assert!(!physics.has_nets());
        physics.settle_cloth(&mut scene, 1, FRAME_TIME);
        assert!(scene.cloth.is_none() && scene.push_out.is_none());
    }

    #[test]
    fn new_obstacles_are_told_from_the_same_ones() {
        let (_, mut physics, _) = skirt();
        let map = std::sync::Arc::new(cloth::Obstacles::default());
        assert!(physics.set_obstacles(Some(map.clone())));
        assert!(!physics.set_obstacles(Some(map)));
        assert!(physics.set_obstacles(None));
        assert!(!physics.set_obstacles(None));
    }

    #[test]
    fn a_hidden_object_stops_its_physics() {
        // A skirt on a clothing object that is switched off, as a bottom
        // in state 3 is: `OnDisable` puts its bones back to rest and it
        // stays there; switched on, it starts again from the bones.
        let mut scene = Scene::default();
        let clothes = scene.add_node("ct_clothesBot", None, Trs::default());
        let bones = chain(&mut scene, Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -0.1, 0.0), 3);
        let mut physics = dynamic(DynamicBoneDef {
            node: clothes,
            root: Some(bones[0]),
            force: Vec3::new(0.01, 0.0, 0.0),
            ..Default::default()
        });
        let rest = position(&scene, bones[2]);
        physics.start(&mut scene);
        physics.settle(&mut scene, 30, FRAME_TIME);
        assert!(position(&scene, bones[2]).x > 0.01);

        scene.nodes[clothes].active = false;
        physics.settle(&mut scene, 30, FRAME_TIME);
        assert!(close(position(&scene, bones[2]), rest, 1e-5));

        // On again: the first frame starts from the bones at rest, so the
        // chain has only begun to swing.
        scene.nodes[clothes].active = true;
        physics.step(&mut scene, FRAME_TIME);
        let x = position(&scene, bones[2]).x;
        assert!(x > 0.0 && x < 0.01, "{x}");
    }

    #[test]
    fn a_carried_chain_keeps_its_shape() {
        let mut scene = Scene::default();
        let holder = scene.add_node("holder", None, Trs::default());
        let root = scene.add_node("root", Some(holder), at(Vec3::new(0.0, 1.0, 0.0)));
        let a = scene.add_node("a", Some(root), at(Vec3::new(0.0, -0.1, 0.0)));
        let b = scene.add_node("b", Some(a), at(Vec3::new(0.0, -0.1, 0.0)));
        let mut physics = dynamic(DynamicBoneDef {
            node: root,
            root: Some(root),
            inert: 0.8,
            force: Vec3::new(0.01, 0.0, 0.0),
            ..Default::default()
        });
        physics.start(&mut scene);
        physics.settle(&mut scene, 120, FRAME_TIME);
        let swung = position(&scene, b) - position(&scene, root);
        assert!(swung.x > 0.01);
        // Turned and moved while nothing runs: the swing turns with it.
        let before = scene.world().0[holder];
        scene.nodes[holder].local = Trs {
            pos: Vec3::new(2.0, 0.0, 0.0),
            rot: Quat::from_rotation_y(std::f32::consts::PI),
            ..Default::default()
        };
        let after = scene.world().0[holder];
        physics.carry(after * before.inverse());
        physics.begin_frame(&mut scene);
        physics.end_frame(&mut scene, 0.0);
        let held = position(&scene, b) - position(&scene, root);
        assert!(
            close(held, Quat::from_rotation_y(std::f32::consts::PI) * swung, 1e-4),
            "{held:?} {swung:?}"
        );
    }

    #[test]
    fn colliders_push_particles_out_or_keep_them_in() {
        let sphere = Shape {
            p0: Vec3::ZERO,
            p1: Vec3::ZERO,
            radius: 1.0,
            capsule: false,
            inside: false,
        };
        let mut p = Vec3::new(0.5, 0.0, 0.0);
        sphere.collide(&mut p, 0.1);
        assert!(close(p, Vec3::new(1.1, 0.0, 0.0), 1e-6));
        // Outside already: untouched.
        let mut p = Vec3::new(0.0, 2.0, 0.0);
        sphere.collide(&mut p, 0.1);
        assert_eq!(p, Vec3::new(0.0, 2.0, 0.0));
        // An inside bound pulls back to the surface.
        let cage = Shape { inside: true, ..sphere };
        sphere.collide(&mut p, 0.0);
        cage.collide(&mut p, 0.0);
        assert!(close(p, Vec3::new(0.0, 1.0, 0.0), 1e-6));

        let capsule = Shape {
            p0: Vec3::new(0.0, -1.0, 0.0),
            p1: Vec3::new(0.0, 1.0, 0.0),
            radius: 0.5,
            capsule: true,
            inside: false,
        };
        // Beside the middle: pushed straight out from the axis.
        let mut p = Vec3::new(0.2, 0.3, 0.0);
        capsule.collide(&mut p, 0.0);
        assert!(close(p, Vec3::new(0.5, 0.3, 0.0), 1e-6));
        // Past an end: pushed out from the end's centre.
        let mut p = Vec3::new(0.0, 1.2, 0.0);
        capsule.collide(&mut p, 0.0);
        assert!(close(p, Vec3::new(0.0, 1.5, 0.0), 1e-6));
    }

    #[test]
    fn collider_shape_follows_its_bone_and_scale() {
        let mut scene = Scene::default();
        let node = scene.add_node(
            "hit",
            None,
            Trs {
                pos: Vec3::new(1.0, 0.0, 0.0),
                scale: Vec3::splat(2.0),
                ..Default::default()
            },
        );
        let capsule = Collider {
            node,
            radius: 0.1,
            height: 0.5,
            direction: 1,
            ..Default::default()
        };
        let s = capsule.shape(&scene);
        assert!(s.capsule);
        assert!((s.radius - 0.2).abs() < 1e-6);
        // Half length (height - radius) / 2 along Y, scaled by the bone.
        assert!(close(s.p0, Vec3::new(1.0, -0.4, 0.0), 1e-6));
        assert!(close(s.p1, Vec3::new(1.0, 0.4, 0.0), 1e-6));
        // Too short for a capsule: a sphere.
        let ball = Collider {
            height: 0.05,
            ..capsule
        };
        assert!(!ball.shape(&scene).capsule);
    }

    #[test]
    fn hanging_chain_rests_outside_a_collider() {
        let mut scene = Scene::default();
        let nodes = chain(&mut scene, Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -0.1, 0.0), 3);
        let hit = scene.add_node("hit", None, at(Vec3::new(-0.05, 0.75, 0.0)));
        let mut physics = dynamic(DynamicBoneDef {
            node: nodes[0],
            root: Some(nodes[0]),
            colliders: vec![0],
            radius: 0.01,
            ..Default::default()
        });
        physics.colliders.push(Collider {
            node: hit,
            radius: 0.1,
            ..Default::default()
        });
        physics.start(&mut scene);
        physics.settle(&mut scene, 200, FRAME_TIME);
        let centre = position(&scene, hit);
        for &n in &nodes[1..] {
            let d = (position(&scene, n) - centre).length();
            assert!(d > 0.11 - 1e-3, "bone inside the collider: {d}");
        }
        // The tip was inside and has been pushed to the far side.
        assert!(position(&scene, nodes[3]).x > 0.02);
        // A disabled collider is ignored.
        physics.colliders[0].enabled = false;
        physics.settle(&mut scene, 300, FRAME_TIME);
        assert!(close(position(&scene, nodes[3]), Vec3::new(0.0, 0.7, 0.0), 1e-3));
    }

    #[test]
    fn particles_skip_excluded_and_not_rolling_bones() {
        let mut scene = Scene::default();
        let root = scene.add_node("root", None, Trs::default());
        let skipped = scene.add_node("skipped", Some(root), at(Vec3::X));
        let _under = scene.add_node("under", Some(skipped), at(Vec3::X));
        let joint = scene.add_node("joint", Some(root), at(Vec3::Y));
        let leaf = scene.add_node("leaf", Some(joint), at(Vec3::Y));
        let mut bone = DynamicBone::new(DynamicBoneDef {
            node: root,
            root: Some(root),
            exclusions: vec![skipped],
            not_rolls: vec![joint],
            end_length: 0.5,
            ..Default::default()
        });
        bone.start(&scene);
        // Root, the leaf under the not-rolling joint, and the leaf's end.
        let nodes: Vec<_> = bone.particles.iter().map(|p| p.node).collect();
        assert_eq!(nodes, vec![Some(root), Some(leaf), None]);
        assert_eq!(bone.particles[1].parent, Some(0));
        // The end sits half a bone beyond the leaf.
        assert!(close(bone.particles[2].pos, Vec3::new(0.0, 2.5, 0.0), 1e-6));
        // Distribution runs over the length from the root.
        assert!((bone.bone_total_length - 2.5).abs() < 1e-6);
    }

    #[test]
    fn disabled_or_weightless_bones_stay_in_the_rest_pose() {
        for (enabled, weight) in [(false, 1.0), (true, 0.0)] {
            let mut scene = Scene::default();
            let nodes = chain(&mut scene, Vec3::ZERO, Vec3::new(0.0, -0.1, 0.0), 2);
            let mut physics = dynamic(DynamicBoneDef {
                node: nodes[0],
                root: Some(nodes[0]),
                force: Vec3::new(0.01, 0.0, 0.0),
                enabled,
                weight,
                ..Default::default()
            });
            physics.start(&mut scene);
            physics.settle(&mut scene, 60, FRAME_TIME);
            assert!(close(position(&scene, nodes[2]), Vec3::new(0.0, -0.2, 0.0), 1e-6));
        }
    }

    #[test]
    fn a_kind_held_still_stays_at_rest_and_moves_again_when_let_go() {
        let swung = |still: &[Belongs]| {
            let mut scene = Scene::default();
            let nodes = chain(&mut scene, Vec3::ZERO, Vec3::new(0.0, -0.1, 0.0), 2);
            let mut physics = dynamic(DynamicBoneDef {
                node: nodes[0],
                root: Some(nodes[0]),
                force: Vec3::new(0.002, 0.0, 0.0),
                ..Default::default()
            });
            physics.belongs = vec![Belongs::Hair];
            physics.start(&mut scene);
            physics.hold_still(&scene, still);
            physics.settle(&mut scene, 200, FRAME_TIME);
            let held = position(&scene, nodes[2]);
            physics.hold_still(&scene, &[]);
            physics.settle(&mut scene, 200, FRAME_TIME);
            (held, position(&scene, nodes[2]))
        };
        let rest = Vec3::new(0.0, -0.2, 0.0);
        let (held, freed) = swung(&[Belongs::Hair]);
        assert!(close(held, rest, 1e-6), "held hair moved to {held}");
        assert!(freed.x > 0.01, "let go, the force pushes it: {freed}");
        // Another kind held still leaves the hair moving.
        let (moving, _) = swung(&[Belongs::Clothes]);
        assert!(moving.x > 0.01);
    }

    #[test]
    fn frozen_axis_keeps_the_chain_in_a_plane() {
        let mut scene = Scene::default();
        let nodes = chain(&mut scene, Vec3::ZERO, Vec3::new(0.0, -0.1, 0.0), 2);
        let mut physics = dynamic(DynamicBoneDef {
            node: nodes[0],
            root: Some(nodes[0]),
            force: Vec3::new(0.002, 0.0, 0.002),
            freeze_axis: 1,
            ..Default::default()
        });
        physics.start(&mut scene);
        physics.settle(&mut scene, 200, FRAME_TIME);
        let tip = position(&scene, nodes[2]);
        assert!(tip.x.abs() < 1e-4 && tip.z > 0.01);
    }

    #[test]
    fn node_chain_settles_and_scales_its_turn() {
        let build = |angle_scale: f32| {
            let mut scene = Scene::default();
            let nodes = chain(&mut scene, Vec3::ZERO, Vec3::new(0.0, -0.1, 0.0), 2);
            let mut physics = Physics {
                colliders: Vec::new(),
                bones: vec![Bone::Ver01(Ver01::new(Ver01Def {
                    node: nodes[0],
                    nodes: nodes.iter().map(|&n| (n, true)).collect(),
                    force: Vec3::new(0.002, 0.0, 0.0),
                    add_angle_scale: angle_scale,
                    ..Default::default()
                }))],
                ..Physics::default()
            };
            physics.start(&mut scene);
            physics.settle(&mut scene, 400, FRAME_TIME);
            assert!(physics.speed() < 1e-6);
            (scene, nodes)
        };
        let (scene, nodes) = build(1.0);
        let tip = position(&scene, nodes[2]);
        assert!(tip.x > 0.005);
        let len = (position(&scene, nodes[1]) - tip).length();
        assert!((len - 0.1).abs() < 1e-4);
        let turned = rotation(&scene, nodes[0]).angle_between(Quat::IDENTITY);
        assert!(turned > 0.01);
        // Half the angle scale turns the first bone half as far; places are equal.
        let (half, half_nodes) = build(0.5);
        assert!(close(position(&half, half_nodes[2]), tip, 1e-5));
        let half_turned = rotation(&half, half_nodes[0]).angle_between(Quat::IDENTITY);
        assert!((half_turned - turned * 0.5).abs() < 1e-3);
    }

    /// A bust-like setup: two bones that copy reference bones, and an end.
    fn ver02(scene: &mut Scene) -> (Physics, [usize; 4]) {
        let root = scene.add_node("root", None, at(Vec3::new(0.0, 1.0, 0.0)));
        let b1 = scene.add_node("b1", Some(root), Trs::default());
        let b2 = scene.add_node("b2", Some(b1), at(Vec3::new(0.0, 0.0, 0.03)));
        let r2 = scene.add_node("r2", Some(b1), at(Vec3::new(0.0, 0.0, 0.05)));
        let param = |ref_node, rotation_calc| BoneParam {
            ref_node: Some(ref_node),
            rotation_calc,
            damping: 0.1,
            elasticity: 0.1,
            stiffness: 0.1,
            ..Default::default()
        };
        let def = Ver02Def {
            node: root,
            comment: BUST_COMMENTS[0].to_string(),
            root: Some(root),
            heavy_loop_max: 5,
            bones: vec![b1, b2],
            patterns: vec![Pattern {
                end_offset: Vec3::new(0.0, 0.0, 0.2),
                params: vec![param(b1, true), param(r2, false)],
                ..Default::default()
            }],
            ..Default::default()
        };
        let physics = Physics {
            colliders: Vec::new(),
            bones: vec![Bone::Ver02(Ver02::new(def))],
            ..Physics::default()
        };
        (physics, [root, b1, b2, r2])
    }

    #[test]
    fn bust_bones_follow_their_reference_bones() {
        let mut scene = Scene::default();
        let (mut physics, [_, _, b2, r2]) = ver02(&mut scene);
        physics.start(&mut scene);
        // The bone takes the reference bone's place.
        assert!(close(scene.nodes[b2].local.pos, Vec3::new(0.0, 0.0, 0.05), 1e-6));
        // A slider moves the reference bone; the game then resets the bust.
        scene.nodes[r2].local.pos = Vec3::new(0.0, 0.0, 0.08);
        scene.nodes[r2].local.scale = Vec3::splat(1.5);
        physics.setup_bust(&mut scene, 0.0, 0.0, 1.0);
        physics.settle(&mut scene, 120, FRAME_TIME);
        assert!(close(scene.nodes[b2].local.pos, Vec3::new(0.0, 0.0, 0.08), 1e-4));
        assert_eq!(scene.nodes[b2].local.scale, Vec3::splat(1.5));
    }

    #[test]
    fn bust_softness_and_weight_follow_the_game_tables() {
        let (damping, elasticity, stiffness) = bust_soft(0.5, 1.0);
        assert!((damping - 0.1).abs() < 1e-6);
        assert!((elasticity - 0.148).abs() < 1e-6);
        assert!((stiffness - 0.0982).abs() < 1e-6);
        // A flat or firm bust is stiff.
        let (damping, elasticity, stiffness) = bust_soft(0.0, 1.0);
        assert!((damping - 0.198).abs() < 1e-6);
        assert!((elasticity - 0.199).abs() < 1e-6);
        assert!((stiffness - 0.982).abs() < 1e-6);
        assert_eq!(bust_gravity(1.0, 1.0, 1.0), Vec3::new(0.0, -0.0025, 0.0));
        assert_eq!(bust_gravity(0.0, 1.0, 1.0).y, 0.0);

        // The values reach every particle, and a heavy soft bust sags.
        let mut scene = Scene::default();
        let (mut physics, [_, _, b2, _]) = ver02(&mut scene);
        physics.start(&mut scene);
        physics.setup_bust(&mut scene, 1.0, 1.0, 1.0);
        let Bone::Ver02(bust) = &physics.bones[0] else {
            unreachable!()
        };
        assert!(bust.particles.iter().all(|p| (p.stiffness - 0.01).abs() < 1e-6));
        assert_eq!(bust.gravity, Vec3::new(0.0, -0.0025, 0.0));
        let before = position(&scene, b2);
        physics.settle(&mut scene, 300, FRAME_TIME);
        assert!(physics.speed() < 1e-6);
        assert!(position(&scene, b2).y < before.y - 0.005);
    }

    #[test]
    fn rotation_passes_through_mirrored_parents_both_ways() {
        let mut scene = Scene::default();
        let parent = scene.add_node(
            "mirror",
            None,
            Trs {
                rot: Quat::from_rotation_y(0.4),
                scale: Vec3::new(-1.0, 1.0, 1.0),
                ..Default::default()
            },
        );
        let child = scene.add_node(
            "child",
            Some(parent),
            Trs {
                rot: Quat::from_rotation_z(0.3),
                ..Default::default()
            },
        );
        let q = rotation(&scene, child);
        // The mirror turns the child's Z rotation the other way round.
        let expected = Quat::from_rotation_y(0.4) * Quat::from_rotation_z(-0.3);
        assert!(q.angle_between(expected) < 1e-5);
        // Setting a world rotation and reading it back agree.
        let target = Quat::from_euler(glam::EulerRot::YXZ, 0.2, -0.5, 0.9);
        set_rotation(&mut scene, child, target);
        assert!(rotation(&scene, child).angle_between(target) < 1e-5);
        // A world position lands in the mirrored local space.
        set_position(&mut scene, child, Vec3::new(0.3, 0.2, 0.1));
        assert!(close(position(&scene, child), Vec3::new(0.3, 0.2, 0.1), 1e-6));
    }
}
