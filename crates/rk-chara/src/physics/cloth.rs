// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Advanced Skirt Physics: Renkatsu's own pass, not the game's. It runs
//! after the game's DynamicBone step and treats the chains of one skirt
//! as one cloth, so that legs, arms and hands do not pass through it and
//! it does not fall through the floor:
//! - capsules along the legs, arms and hands, each as thick as the
//!   card's own body mesh is there;
//! - collision of the particles and of the edges between them, down each
//!   chain and across to the next one, since the game tests particles
//!   only and a leg slips through the 17 to 20 cm between two chains;
//! - a floor at the character's root.
//!
//! Every constraint only pushes out, so a skirt that touches nothing stays
//! where the game's step put it.

use super::{keep_length, position, world, Bone, Particle};
use crate::scene::{Part, Scene, Skinning};
use glam::Vec3;
use std::collections::{HashMap, HashSet};

/// Rounds of the constraints per frame.
const ITERATIONS: u32 = 8;
/// Places along each limb bone where the body mesh's thickness is taken;
/// a limb bone gets one capsule between each two.
const STATIONS: usize = 4;

/// A capsule with a radius at each end, in the space of `node`.
#[derive(Debug, Clone)]
pub struct Capsule {
    pub node: usize,
    pub a: Vec3,
    pub b: Vec3,
    pub ra: f32,
    pub rb: f32,
}

#[derive(Debug, Clone, Copy)]
struct WorldCapsule {
    a: Vec3,
    b: Vec3,
    ra: f32,
    rb: f32,
}

impl WorldCapsule {
    /// The box around the capsule.
    fn bounds(&self) -> (Vec3, Vec3) {
        let r = Vec3::splat(self.ra.max(self.rb));
        (self.a.min(self.b) - r, self.a.max(self.b) + r)
    }
}

/// Whether the segment `p q` can touch something inside the box `lo hi`.
fn near(p: Vec3, q: Vec3, (lo, hi): (Vec3, Vec3)) -> bool {
    p.min(q).cmple(hi).all() && p.max(q).cmpge(lo).all()
}

#[derive(Debug, Clone)]
pub struct Cloth {
    /// Rings of chains around the body: indices into `Physics::bones`.
    pub rings: Vec<Vec<usize>>,
    pub capsules: Vec<Capsule>,
    /// The node whose XZ plane is the floor.
    pub floor: Option<usize>,
    /// Rest places of every ring particle, taken before the bones step.
    rest: Vec<Vec<Vec<Vec3>>>,
}

/// The chain number in `cf_j_sk_03_00` or `cf_d_sk_03_00`.
fn skirt_chain(name: &str) -> Option<u32> {
    let mut parts = name.split('_');
    while let Some(p) = parts.next() {
        if p == "sk" {
            return parts.next()?.parse().ok();
        }
    }
    None
}

fn particles(bone: &Bone) -> Option<&[Particle]> {
    match bone {
        Bone::Dynamic(d) => Some(&d.particles),
        _ => None,
    }
}

/// Whether particle `i` hangs from particle `i - 1` all the way down.
fn linear(ps: &[Particle]) -> bool {
    ps.len() >= 2 && ps.iter().enumerate().all(|(i, p)| p.parent == i.checked_sub(1))
}

impl Cloth {
    /// Finds the skirts among `bones` and fits the limb capsules to the
    /// body as it is now, which should be the pose it was built in.
    pub fn build(scene: &Scene, names: &HashMap<String, usize>, bones: &[Bone]) -> Self {
        // The skirt chains of one object make one ring, in number order.
        let mut groups: HashMap<usize, Vec<(u32, usize)>> = HashMap::new();
        for (i, b) in bones.iter().enumerate() {
            let Bone::Dynamic(d) = b else { continue };
            let Some(root) = d.def.root else { continue };
            if !d.def.enabled || !linear(&d.particles) {
                continue;
            }
            let Some(n) = skirt_chain(&scene.nodes[root].name) else {
                continue;
            };
            groups.entry(d.def.node).or_default().push((n, i));
        }
        let mut rings: Vec<Vec<usize>> = groups
            .into_values()
            .filter(|g| g.len() >= 3)
            .map(|mut g| {
                g.sort();
                g.into_iter().map(|(_, i)| i).collect()
            })
            .collect();
        rings.sort();
        let capsules = if rings.is_empty() {
            Vec::new()
        } else {
            fit_limbs(scene, names)
        };
        Self {
            capsules,
            floor: names.get("cf_j_root").copied(),
            rest: Vec::new(),
            rings,
        }
    }

    fn world_capsules(&self, scene: &Scene) -> Vec<WorldCapsule> {
        self.capsules
            .iter()
            .map(|c| {
                let m = world(scene, c.node);
                let s = m.x_axis.truncate().length();
                WorldCapsule {
                    a: m.transform_point3(c.a),
                    b: m.transform_point3(c.b),
                    ra: c.ra * s,
                    rb: c.rb * s,
                }
            })
            .collect()
    }

    /// Records where the ring particles rest; call before the bones step,
    /// while the skirt bones still hold their rest transforms.
    pub(super) fn begin(&mut self, scene: &Scene, bones: &[Bone]) {
        if self.rings.is_empty() {
            return;
        }
        self.rest = self
            .rings
            .iter()
            .map(|ring| {
                ring.iter()
                    .map(|&b| {
                        let ps = particles(&bones[b]).unwrap_or(&[]);
                        ps.iter()
                            .enumerate()
                            .map(|(i, p)| match (p.node, i.checked_sub(1).and_then(|pi| ps[pi].node)) {
                                (Some(n), _) => position(scene, n),
                                (None, Some(pn)) => world(scene, pn).transform_point3(p.end_offset),
                                (None, None) => p.pos,
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect();
    }

    /// Pushes the particles the bones step left out of the limbs and the
    /// floor, then poses the bones again. `live` says which bones ran.
    pub(super) fn solve(&self, scene: &mut Scene, bones: &mut [Bone], live: &[bool]) {
        if self.rings.is_empty() {
            return;
        }
        let all = self.world_capsules(scene);
        let floor = self.floor.map(|n| {
            let m = world(scene, n);
            (m.w_axis.truncate(), m.y_axis.truncate().normalize_or_zero())
        });
        for (ring, rest) in self.rings.iter().zip(&self.rest) {
            if ring.iter().any(|&b| !live[b]) || rest.len() != ring.len() {
                continue;
            }
            let mut pos: Vec<Vec<Vec3>> = Vec::new();
            let mut radius: Vec<Vec<f32>> = Vec::new();
            for &b in ring {
                let Bone::Dynamic(d) = &bones[b] else { return };
                pos.push(d.particles.iter().map(|p| p.pos).collect());
                radius.push(d.particles.iter().map(|p| p.radius * d.object_scale).collect());
            }
            if pos.iter().zip(rest).any(|(p, r)| p.len() != r.len()) {
                continue;
            }
            // Only the limbs near the skirt: a box around its particles,
            // grown by how far a particle can go in one frame.
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for p in pos.iter().flatten() {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            let margin = 0.1;
            let capsules: Vec<WorldCapsule> = all
                .iter()
                .filter(|c| {
                    let r = c.ra.max(c.rb) + margin;
                    let (clo, chi) = (c.a.min(c.b) - Vec3::splat(r), c.a.max(c.b) + Vec3::splat(r));
                    clo.cmple(hi).all() && chi.cmpge(lo).all()
                })
                .copied()
                .collect();
            if capsules.is_empty() && floor.is_none() {
                continue;
            }
            let n = ring.len();
            for _ in 0..ITERATIONS {
                for cap in &capsules {
                    let bounds = cap.bounds();
                    for chain in &mut pos {
                        for p in chain.iter_mut().skip(1) {
                            if near(*p, *p, bounds) {
                                point_capsule(p, cap);
                            }
                        }
                    }
                    for c in 0..n {
                        // Down the chain; the root does not move.
                        for i in 1..pos[c].len() {
                            let (up, down) = pos[c].split_at_mut(i);
                            if near(up[i - 1], down[0], bounds) {
                                let w0 = if i == 1 { 0.0 } else { 1.0 };
                                edge_capsule(&mut up[i - 1], &mut down[0], w0, cap);
                            }
                        }
                        // Across to the next chain.
                        let d = (c + 1) % n;
                        for i in 1..pos[c].len().min(pos[d].len()) {
                            let (mut p, mut q) = (pos[c][i], pos[d][i]);
                            if near(p, q, bounds) {
                                edge_capsule(&mut p, &mut q, 1.0, cap);
                                pos[c][i] = p;
                                pos[d][i] = q;
                            }
                        }
                    }
                }
                if let Some((origin, up)) = floor {
                    for (chain, radius) in pos.iter_mut().zip(&radius) {
                        for (p, r) in chain.iter_mut().zip(radius).skip(1) {
                            let h = (*p - origin).dot(up) - r;
                            if h < 0.0 {
                                *p -= up * h;
                            }
                        }
                    }
                }
                for (chain, rest) in pos.iter_mut().zip(rest) {
                    for i in 1..chain.len() {
                        let len = (rest[i] - rest[i - 1]).length();
                        let parent = chain[i - 1];
                        keep_length(parent, &mut chain[i], len);
                    }
                }
            }
            for (c, &b) in ring.iter().enumerate() {
                let Bone::Dynamic(d) = &mut bones[b] else { continue };
                for (p, new) in d.particles.iter_mut().zip(&pos[c]) {
                    p.pos = *new;
                }
                d.apply(scene);
            }
        }
    }
}

/// Closest points of segments `p0 p1` and `q0 q1` as parameters (Ericson,
/// Real-Time Collision Detection 5.1.9).
fn closest(p0: Vec3, p1: Vec3, q0: Vec3, q1: Vec3) -> (f32, f32) {
    let (d1, d2, r) = (p1 - p0, q1 - q0, p0 - q0);
    let (a, e, f) = (d1.length_squared(), d2.length_squared(), d2.dot(r));
    if a <= 1e-12 && e <= 1e-12 {
        return (0.0, 0.0);
    }
    if a <= 1e-12 {
        return (0.0, (f / e).clamp(0.0, 1.0));
    }
    let c = d1.dot(r);
    if e <= 1e-12 {
        return ((-c / a).clamp(0.0, 1.0), 0.0);
    }
    let b = d1.dot(d2);
    let denom = a * e - b * b;
    let mut s = if denom > 1e-12 {
        ((b * f - c * e) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut t = (b * s + f) / e;
    if t < 0.0 {
        t = 0.0;
        s = (-c / a).clamp(0.0, 1.0);
    } else if t > 1.0 {
        t = 1.0;
        s = ((b - c) / a).clamp(0.0, 1.0);
    }
    (s, t)
}

fn point_capsule(p: &mut Vec3, cap: &WorldCapsule) {
    let axis = cap.b - cap.a;
    let len2 = axis.length_squared();
    let t = if len2 > 0.0 {
        ((*p - cap.a).dot(axis) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let d = *p - (cap.a + axis * t);
    let r = cap.ra + (cap.rb - cap.ra) * t;
    let dist = d.length();
    if dist < r && dist > 1e-6 {
        *p += d * ((r - dist) / dist);
    }
}

/// Pushes the edge `p0 p1` out of the capsule; `w0` is 0 when `p0` is
/// held.
fn edge_capsule(p0: &mut Vec3, p1: &mut Vec3, w0: f32, cap: &WorldCapsule) {
    let (s, t) = closest(*p0, *p1, cap.a, cap.b);
    let d = (*p0 + (*p1 - *p0) * s) - (cap.a + (cap.b - cap.a) * t);
    let r = cap.ra + (cap.rb - cap.ra) * t;
    let dist = d.length();
    if dist >= r || dist <= 1e-6 {
        return;
    }
    let (b0, b1) = ((1.0 - s) * w0, s);
    let sum = b0 * (1.0 - s) + b1 * s;
    if sum <= 1e-6 {
        return;
    }
    let push = d * ((r - dist) / dist / sum);
    *p0 += push * b0;
    *p1 += push * b1;
}

/// The limb bones that get capsules: from one joint to the next, with the
/// joint whose subtree holds the limb's vertices.
const LIMBS: [(&str, &str, &str); 5] = [
    ("cf_j_thigh00", "cf_j_thigh00", "cf_j_leg01"),
    ("cf_j_thigh00", "cf_j_leg01", "cf_j_leg03"),
    ("cf_j_arm00", "cf_j_arm00", "cf_j_forearm01"),
    ("cf_j_arm00", "cf_j_forearm01", "cf_j_hand"),
    ("cf_j_arm00", "cf_j_hand", "cf_j_middle03"),
];

/// Capsules along the legs, arms and hands, with the thickness the body
/// mesh has there: the mean distance from the bone line of the limb's
/// vertices near each of `STATIONS` places along it.
pub fn fit_limbs(scene: &Scene, names: &HashMap<String, usize>) -> Vec<Capsule> {
    let posed = scene.pose();
    let Some(body) = posed
        .iter()
        .filter(|m| m.renderer.part == Part::Body && matches!(m.skinning, Skinning::Bones { .. }))
        .max_by_key(|m| m.base_positions.len())
    else {
        return Vec::new();
    };
    let positions = body.positions();
    let mut out = Vec::new();
    for side in ["L", "R"] {
        let joint = |n: &str| names.get(&format!("{n}_{side}")).copied();
        let mut limb_verts: HashMap<usize, Vec<Vec3>> = HashMap::new();
        for (root, from, to) in LIMBS {
            let (Some(root), Some(from), Some(to)) = (joint(root), joint(from), joint(to)) else {
                continue;
            };
            let verts = limb_verts.entry(root).or_insert_with(|| {
                let limb: HashSet<usize> = scene.subtree(root).into_iter().collect();
                let of_limb: Vec<bool> = body
                    .renderer
                    .bones
                    .iter()
                    .map(|b| b.is_some_and(|n| limb.contains(&n)))
                    .collect();
                body.renderer
                    .mesh
                    .skin
                    .iter()
                    .zip(&positions)
                    .filter(|(bw, _)| {
                        let w: f32 = (0..4)
                            .filter(|&k| of_limb.get(bw.bones[k] as usize).copied().unwrap_or(false))
                            .map(|k| bw.weights[k])
                            .sum();
                        w > 0.5
                    })
                    .map(|(_, p)| Vec3::from(*p))
                    .collect()
            });
            out.extend(fit_bone(scene, verts, from, to));
        }
    }
    out
}

/// The capsules of one limb bone from `from` to `to`.
fn fit_bone(scene: &Scene, verts: &[Vec3], from: usize, to: usize) -> Vec<Capsule> {
    let (a, b) = (position(scene, from), position(scene, to));
    let axis = b - a;
    let len2 = axis.length_squared();
    if len2 <= 0.0 {
        return Vec::new();
    }
    let half = 0.5 / (STATIONS - 1) as f32;
    let radii: Vec<f32> = (0..STATIONS)
        .map(|k| {
            let at = k as f32 / (STATIONS - 1) as f32;
            let (mut sum, mut count) = (0.0, 0u32);
            for v in verts {
                let t = (*v - a).dot(axis) / len2;
                if (0.0..=1.0).contains(&t) && (t - at).abs() <= half {
                    sum += (*v - (a + axis * t)).length();
                    count += 1;
                }
            }
            if count > 0 {
                sum / count as f32
            } else {
                0.0
            }
        })
        .collect();
    let m = world(scene, from);
    let inv = m.inverse();
    let scale = m.x_axis.truncate().length().max(f32::EPSILON);
    (0..STATIONS - 1)
        .filter(|&k| radii[k] > 0.0 && radii[k + 1] > 0.0)
        .map(|k| {
            let t0 = k as f32 / (STATIONS - 1) as f32;
            let t1 = (k + 1) as f32 / (STATIONS - 1) as f32;
            Capsule {
                node: from,
                a: inv.transform_point3(a + axis * t0),
                b: inv.transform_point3(a + axis * t1),
                ra: radii[k] / scale,
                rb: radii[k + 1] / scale,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap() -> WorldCapsule {
        WorldCapsule {
            a: Vec3::new(-1.0, 0.0, 0.0),
            b: Vec3::new(1.0, 0.0, 0.0),
            ra: 0.1,
            rb: 0.1,
        }
    }

    #[test]
    fn an_edge_across_a_capsule_is_lifted_off_it() {
        // Both ends are outside, the middle passes through the axis side.
        let (mut p0, mut p1) = (Vec3::new(0.0, 0.05, -0.5), Vec3::new(0.0, 0.05, 0.5));
        edge_capsule(&mut p0, &mut p1, 1.0, &cap());
        assert!((p0.y - 0.1).abs() < 1e-5 && (p1.y - 0.1).abs() < 1e-5);
        // A held end stays; the free end takes the whole correction.
        let (mut p0, mut p1) = (Vec3::new(0.0, 0.05, -0.5), Vec3::new(0.0, 0.05, 0.5));
        edge_capsule(&mut p0, &mut p1, 0.0, &cap());
        assert_eq!(p0, Vec3::new(0.0, 0.05, -0.5));
        assert!(p1.y > 0.1);
    }

    #[test]
    fn an_edge_clear_of_the_capsule_is_left_alone() {
        let (mut p0, mut p1) = (Vec3::new(0.0, 0.2, -0.5), Vec3::new(0.0, 0.2, 0.5));
        edge_capsule(&mut p0, &mut p1, 1.0, &cap());
        assert_eq!((p0, p1), (Vec3::new(0.0, 0.2, -0.5), Vec3::new(0.0, 0.2, 0.5)));
    }

    #[test]
    fn a_point_leaves_a_tapered_capsule_by_its_local_radius() {
        let c = WorldCapsule { rb: 0.3, ..cap() };
        let mut p = Vec3::new(1.0, 0.1, 0.0);
        point_capsule(&mut p, &c);
        assert!((p.y - 0.3).abs() < 1e-5);
    }

    #[test]
    fn skirt_chain_numbers_come_from_the_bone_name() {
        assert_eq!(skirt_chain("cf_j_sk_03_00"), Some(3));
        assert_eq!(skirt_chain("cf_d_sk_07_00"), Some(7));
        assert_eq!(skirt_chain("cf_j_hairB_00"), None);
    }
}
