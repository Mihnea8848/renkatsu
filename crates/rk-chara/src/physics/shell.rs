// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Body shells: what the cloth net and the vertices on it collide with.
//! Renkatsu's own, not the game's.
//!
//! A shell is a bone's line with the body's thickness around it measured
//! from the body mesh itself: at a few places along the line, in every
//! direction round it, how far the skin is. A capsule takes one radius
//! all round, so cloth hovers over the flat of a thigh and a leg pokes
//! through at its widest; a shell follows the skin to within its grid.
//! It is a small table per bone that moves rigidly with it, which a
//! compute shader reads as easily as this does.
//!
//! The pelvis is one shell round the line up through the hips, which
//! takes in the buttocks and the belly. The feet take in the shoes worn
//! when the shells are fitted. The arms and hands are shells too, marked
//! as being outside the skirt.

use super::{position, world};
use crate::scene::{Part, Scene, Skinning};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::sync::Arc;

/// Directions round a shell's line.
pub const ANGLES: usize = 24;
/// The furthest the skin is looked for from a limb's line and from the
/// line through the hips.
const LIMB_REACH: f32 = 0.2;
const PELVIS_REACH: f32 = 0.3;
/// How far down the thigh its shell starts, as a share of its length.
const HIP: f32 = 0.12;
/// The most a direction's thickness is of the middle one of its ring.
const WIDEST: f32 = 2.5;
/// How far below the hip joints the pelvis shell starts, as a share of
/// the distance between them: the crotch.
const CROTCH: f32 = 0.35;

/// A shell in the space of `node`.
#[derive(Debug, Clone)]
pub struct Shell {
    pub node: usize,
    pub a: Vec3,
    pub b: Vec3,
    /// Two directions across the line; angles are counted from the first
    /// towards the second.
    pub e1: Vec3,
    pub e2: Vec3,
    pub stations: usize,
    /// The skin's distance from the line, `ANGLES` per station from `a`
    /// to `b`.
    pub radii: Arc<[f32]>,
    /// Whether each end is closed by a round cap.
    pub caps: [bool; 2],
    /// An arm or a hand, which is outside the skirt; the rest is inside.
    pub outside: bool,
}

/// A shell in the world.
#[derive(Debug, Clone)]
pub struct WorldShell {
    a: Vec3,
    axis: Vec3,
    len2: f32,
    e1: Vec3,
    e2: Vec3,
    scale: f32,
    stations: usize,
    radii: Arc<[f32]>,
    caps: [bool; 2],
    /// The middle thickness at each end.
    ends: [f32; 2],
    pub outside: bool,
    /// A ball that holds it.
    centre: Vec3,
    reach: f32,
}

impl Shell {
    /// The middle thickness at each end, in its own space.
    pub fn ends(&self) -> [f32; 2] {
        let middle = |ring: &[f32]| {
            let mut ring = ring.to_vec();
            ring.sort_by(f32::total_cmp);
            ring[ring.len() / 2]
        };
        [
            middle(&self.radii[..ANGLES]),
            middle(&self.radii[self.radii.len() - ANGLES..]),
        ]
    }

    pub fn in_world(&self, m: &Mat4) -> WorldShell {
        let (a, b) = (m.transform_point3(self.a), m.transform_point3(self.b));
        let scale = m.x_axis.truncate().length();
        let widest = self.radii.iter().copied().fold(0.0, f32::max) * scale;
        WorldShell {
            a,
            axis: b - a,
            len2: (b - a).length_squared().max(1e-12),
            e1: m.transform_vector3(self.e1).normalize_or_zero(),
            e2: m.transform_vector3(self.e2).normalize_or_zero(),
            scale,
            stations: self.stations,
            radii: self.radii.clone(),
            caps: self.caps,
            ends: self.ends(),
            outside: self.outside,
            centre: (a + b) * 0.5,
            reach: (b - a).length() * 0.5 + widest,
        }
    }
}

impl WorldShell {
    /// Its middle.
    pub fn centre(&self) -> Vec3 {
        self.centre
    }

    /// Points of its surface, for drawing it.
    pub fn surface(&self) -> Vec<Vec3> {
        let mut out = Vec::new();
        for s in 0..self.stations {
            let t = s as f32 / (self.stations - 1) as f32;
            for k in 0..ANGLES {
                let angle = k as f32 / ANGLES as f32 * std::f32::consts::TAU;
                let dir = self.e1 * angle.cos() + self.e2 * angle.sin();
                out.push(self.a + self.axis * t + dir * self.radius(t, angle));
            }
        }
        out
    }

    /// The lowest and highest corner of a box that holds it.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        (
            self.centre - Vec3::splat(self.reach),
            self.centre + Vec3::splat(self.reach),
        )
    }

    /// The skin's distance from the line `t` of the way along it, in the
    /// direction at `angle` round it.
    fn radius(&self, t: f32, angle: f32) -> f32 {
        let s = t.clamp(0.0, 1.0) * (self.stations - 1) as f32;
        let s0 = (s as usize).min(self.stations - 1);
        let s1 = (s0 + 1).min(self.stations - 1);
        let fs = s - s0 as f32;
        let turn = angle.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * ANGLES as f32;
        let a0 = (turn as usize) % ANGLES;
        let a1 = (a0 + 1) % ANGLES;
        let fa = turn - turn.floor();
        let at = |s: usize, a: usize| self.radii[s * ANGLES + a];
        let near = at(s0, a0) + (at(s0, a1) - at(s0, a0)) * fa;
        let far = at(s1, a0) + (at(s1, a1) - at(s1, a0)) * fa;
        (near + (far - near) * fs) * self.scale
    }

    /// Whether `p` is inside the shell grown by `gap` (shrunk by a
    /// negative one): the point of the line it is measured from, the
    /// direction from there to `p`, its distance and the skin's with the
    /// gap.
    pub fn inside(&self, p: Vec3, gap: f32) -> Option<(Vec3, Vec3, f32, f32)> {
        if (p - self.centre).length_squared() > (self.reach + gap.max(0.0)).powi(2) {
            return None;
        }
        let t = (p - self.a).dot(self.axis) / self.len2;
        if (t < 0.0 && !self.caps[0]) || (t > 1.0 && !self.caps[1]) {
            return None;
        }
        let beyond = !(0.0..=1.0).contains(&t);
        let t = t.clamp(0.0, 1.0);
        let on = self.a + self.axis * t;
        let off = p - on;
        let dist = off.length();
        if dist < 1e-6 {
            return None;
        }
        // Past an end the shell is a ball of the end's middle thickness:
        // a joint's own shape does not turn with either of its limbs.
        let r = if beyond {
            self.ends[usize::from(t > 0.5)] * self.scale
        } else {
            self.radius(t, off.dot(self.e2).atan2(off.dot(self.e1)))
        } + gap;
        (dist < r).then_some((on, off / dist, dist, r))
    }

    /// Pushes `p` out of the shell grown by `gap`, straight away from
    /// its line. Returns the direction and how far it was pushed.
    pub fn push_out(&self, p: &mut Vec3, gap: f32) -> Option<(Vec3, f32)> {
        let (_, dir, dist, r) = self.inside(*p, gap)?;
        *p += dir * (r - dist);
        Some((dir, r - dist))
    }

    /// Pushes `p` out of the shell grown by `gap` along `way`, a unit
    /// direction: for an arm on the outside of the cloth, which the cloth
    /// leaves on the body's side wherever in the arm it is.
    pub fn push_along(&self, p: &mut Vec3, gap: f32, way: Vec3) -> Option<(Vec3, f32)> {
        let (on, _, dist, r) = self.inside(*p, gap)?;
        let off = *p - on;
        let along = off.dot(way);
        // |off + way s| = r, the root on the side `way` points to.
        let s = -along + (along * along + r * r - dist * dist).max(0.0).sqrt();
        *p += way * s;
        Some((way, s))
    }

    /// The way from `p` to the shell's line, and how far `p` is from
    /// the skin on that way (less than zero inside it).
    pub fn towards(&self, p: Vec3) -> Option<(Vec3, f32)> {
        let t = ((p - self.a).dot(self.axis) / self.len2).clamp(0.0, 1.0);
        let off = self.a + self.axis * t - p;
        let dist = off.length();
        if dist < 1e-6 {
            return None;
        }
        let r = self.radius(t, (-off).dot(self.e2).atan2((-off).dot(self.e1)));
        Some((off / dist, dist - r))
    }

    /// How deep `p` is inside the shell grown by `gap`, or zero.
    pub fn depth(&self, p: Vec3, gap: f32) -> f32 {
        self.inside(p, gap).map_or(0.0, |(_, _, dist, r)| r - dist)
    }
}

/// Where the ray from `o` along the unit `d` meets the triangle, as the
/// distance along it (Moeller and Trumbore), either face.
fn ray_triangle(o: Vec3, d: Vec3, [a, b, c]: [Vec3; 3]) -> Option<f32> {
    let (e1, e2) = (b - a, c - a);
    let h = d.cross(e2);
    let det = e1.dot(h);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(h) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 1e-5).then_some(t)
}

/// Which hit of a ray is the skin.
#[derive(Clone, Copy, PartialEq)]
enum Take {
    /// The first: the line is inside the limb.
    First,
    /// The last within reach: the line may be outside the body.
    Last,
}

/// Measures a shell round the world line `a b` from `skin`, and from
/// `over` (shoes) where that is further out. `across` is a direction not
/// along the line, which angles are counted from.
#[allow(clippy::too_many_arguments)]
fn measure(
    a: Vec3,
    b: Vec3,
    across: Vec3,
    stations: usize,
    skin: &[[Vec3; 3]],
    over: &[[Vec3; 3]],
    take: Take,
    reach: f32,
) -> Option<(Vec3, Vec3, Vec<f32>)> {
    let axis = (b - a).normalize_or_zero();
    let e1 = (across - axis * across.dot(axis)).normalize_or_zero();
    if axis == Vec3::ZERO || e1 == Vec3::ZERO {
        return None;
    }
    let e2 = axis.cross(e1);
    // Only the triangles near the line.
    let near = |tris: &[[Vec3; 3]]| -> Vec<[Vec3; 3]> {
        let (lo, hi) = (a.min(b) - Vec3::splat(reach), a.max(b) + Vec3::splat(reach));
        tris.iter()
            .filter(|t| {
                let (tlo, thi) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
                tlo.cmple(hi).all() && thi.cmpge(lo).all()
            })
            .copied()
            .collect()
    };
    let (skin, over) = (near(skin), near(over));
    let mut radii = vec![0.0f32; stations * ANGLES];
    for s in 0..stations {
        let o = a.lerp(b, s as f32 / (stations - 1) as f32);
        for k in 0..ANGLES {
            let angle = k as f32 / ANGLES as f32 * std::f32::consts::TAU;
            let d = e1 * angle.cos() + e2 * angle.sin();
            let hits = |tris: &[[Vec3; 3]], take: Take| -> f32 {
                let mut best = 0.0f32;
                for t in tris {
                    if let Some(at) = ray_triangle(o, d, *t).filter(|&at| at <= reach) {
                        best = match take {
                            Take::First if best > 0.0 => best.min(at),
                            _ => best.max(at),
                        };
                    }
                }
                best
            };
            radii[s * ANGLES + k] = hits(&skin, take).max(hits(&over, Take::Last));
        }
    }
    // A direction that met nothing takes the middle of its ring; a ring
    // that met nothing takes the ring before it.
    for s in 0..stations {
        let ring = &mut radii[s * ANGLES..(s + 1) * ANGLES];
        let mut met: Vec<f32> = ring.iter().copied().filter(|&r| r > 0.0).collect();
        if met.is_empty() {
            continue;
        }
        met.sort_by(f32::total_cmp);
        let middle = met[met.len() / 2];
        for r in ring.iter_mut() {
            // A ray that left through the far side of the trunk, or met
            // the other shoe, is no thickness of this limb.
            if *r <= 0.0 || *r > middle * WIDEST {
                *r = middle;
            }
        }
    }
    for s in 1..stations {
        if radii[s * ANGLES] <= 0.0 {
            radii.copy_within((s - 1) * ANGLES..s * ANGLES, s * ANGLES);
        }
    }
    for s in (0..stations - 1).rev() {
        if radii[s * ANGLES] <= 0.0 {
            radii.copy_within((s + 1) * ANGLES..(s + 2) * ANGLES, s * ANGLES);
        }
    }
    (radii[0] > 0.0).then_some((e1, e2, radii))
}

/// A measured shell put in the space of `node`.
#[allow(clippy::too_many_arguments)]
fn shell_of(
    scene: &Scene,
    node: usize,
    a: Vec3,
    b: Vec3,
    stations: usize,
    measured: (Vec3, Vec3, Vec<f32>),
    caps: [bool; 2],
    outside: bool,
) -> Shell {
    let m = world(scene, node);
    let inv = m.inverse();
    let scale = m.x_axis.truncate().length().max(f32::EPSILON);
    let (e1, e2, radii) = measured;
    Shell {
        node,
        a: inv.transform_point3(a),
        b: inv.transform_point3(b),
        e1: inv.transform_vector3(e1).normalize_or_zero(),
        e2: inv.transform_vector3(e2).normalize_or_zero(),
        stations,
        radii: radii.into_iter().map(|r| r / scale).collect(),
        caps,
        outside,
    }
}

/// The limbs' shells: the group of bones each belongs to, the joints it
/// runs between and how many places along it are measured.
const LIMBS: [(&str, &str, &str, usize); 6] = [
    ("cf_j_thigh00", "cf_j_thigh00", "cf_j_leg01", 6),
    ("cf_j_thigh00", "cf_j_leg01", "cf_j_leg03", 5),
    ("cf_j_thigh00", "cf_j_leg03", "cf_j_toes", 4),
    ("cf_j_arm00", "cf_j_arm00", "cf_j_forearm01", 4),
    ("cf_j_arm00", "cf_j_forearm01", "cf_j_hand", 4),
    ("cf_j_arm00", "cf_j_hand", "cf_j_middle03", 3),
];
/// Places up the pelvis shell.
const PELVIS_STATIONS: usize = 7;

/// Fits the shells to the body as it is posed now, which should be the
/// pose it was built in.
pub fn fit(scene: &Scene, names: &HashMap<String, usize>) -> Vec<Shell> {
    let posed = scene.pose();
    let Some(body) = posed
        .iter()
        .filter(|m| m.renderer.part == Part::Body && matches!(m.skinning, Skinning::Bones { .. }))
        .max_by_key(|m| m.base_positions.len())
    else {
        return Vec::new();
    };
    let positions: Vec<Vec3> = body.positions().into_iter().map(Vec3::from).collect();
    // The group of each bone: 0 the trunk, then each leg and each arm.
    let roots = ["cf_j_thigh00_L", "cf_j_thigh00_R", "cf_j_arm00_L", "cf_j_arm00_R"];
    let mut group_of_node = vec![0u8; scene.nodes.len()];
    for (g, root) in roots.iter().enumerate() {
        if let Some(&n) = names.get(*root) {
            for k in scene.subtree(n) {
                group_of_node[k] = g as u8 + 1;
            }
        }
    }
    let slot_group: Vec<u8> = body
        .renderer
        .bones
        .iter()
        .map(|b| b.map_or(0, |n| group_of_node[n]))
        .collect();
    let group: Vec<u8> = body
        .renderer
        .mesh
        .skin
        .iter()
        .map(|sv| {
            let mut share = [0.0f32; 5];
            for k in 0..4 {
                share[slot_group.get(sv.bones[k] as usize).copied().unwrap_or(0) as usize] += sv.weights[k];
            }
            (0..5).max_by(|&a, &b| share[a].total_cmp(&share[b])).unwrap_or(0) as u8
        })
        .collect();
    // The body's triangles of the trunk and one group, or of the trunk
    // and both legs.
    let triangles = |keep: &dyn Fn(u8) -> bool| -> Vec<[Vec3; 3]> {
        body.renderer
            .mesh
            .indices
            .chunks_exact(3)
            .filter(|t| t.iter().all(|&i| group.get(i as usize).is_some_and(|&g| keep(g))))
            .filter_map(|t| {
                let p = |i: u32| positions.get(i as usize).copied();
                Some([p(t[0])?, p(t[1])?, p(t[2])?])
            })
            .collect()
    };
    // The shoes worn now.
    let mut shoes: Vec<[Vec3; 3]> = Vec::new();
    for m in &posed {
        if !matches!(m.renderer.part, Part::Clothes(7 | 8)) {
            continue;
        }
        let ps: Vec<Vec3> = m.positions().into_iter().map(Vec3::from).collect();
        for t in m.renderer.mesh.indices.chunks_exact(3) {
            if let (Some(&a), Some(&b), Some(&c)) =
                (ps.get(t[0] as usize), ps.get(t[1] as usize), ps.get(t[2] as usize))
            {
                shoes.push([a, b, c]);
            }
        }
    }

    let mut out = Vec::new();
    // The pelvis: up from the crotch to the waist, through the middle of
    // the hips.
    if let (Some(&hips), Some(&left), Some(&right), Some(&top)) = (
        names.get("cf_j_waist02"),
        names.get("cf_j_thigh00_L"),
        names.get("cf_j_thigh00_R"),
        names.get("cf_j_spine01"),
    ) {
        let (l, r, top) = (position(scene, left), position(scene, right), position(scene, top));
        let middle = (l + r) * 0.5;
        let up = (top - middle).normalize_or_zero();
        let a = middle - up * (l - r).length() * CROTCH;
        let skin = triangles(&|g| g <= 2);
        if let Some(measured) = measure(a, top, l - r, PELVIS_STATIONS, &skin, &[], Take::Last, PELVIS_REACH) {
            out.push(shell_of(
                scene,
                hips,
                a,
                top,
                PELVIS_STATIONS,
                measured,
                [false, false],
                false,
            ));
        }
    }
    for (root, from, to, stations) in LIMBS {
        for (side, g) in [("L", 0u8), ("R", 1u8)] {
            let joint = |n: &str| names.get(&format!("{n}_{side}")).copied();
            let (Some(from), Some(to)) = (joint(from), joint(to)) else {
                continue;
            };
            let arm = root == "cf_j_arm00";
            let g = g + if arm { 3 } else { 1 };
            let skin = triangles(&|v| v == 0 || v == g);
            let (a, b) = (position(scene, from), position(scene, to));
            // The thigh starts below the hip joint: the skin round the
            // joint is the pelvis's and does not turn with the thigh.
            let a = if from == joint("cf_j_thigh00").unwrap_or(usize::MAX) {
                a.lerp(b, HIP)
            } else {
                a
            };
            let lower_leg = !arm && from != joint("cf_j_thigh00").unwrap_or(usize::MAX);
            let over: &[[Vec3; 3]] = if lower_leg { &shoes } else { &[] };
            // Angles are counted from the bone's own sideways axis.
            let across = world(scene, from).x_axis.truncate();
            let across = if across.cross(b - a).length_squared() < 1e-8 {
                world(scene, from).z_axis.truncate()
            } else {
                across
            };
            if let Some(measured) = measure(a, b, across, stations, &skin, over, Take::First, LIMB_REACH) {
                out.push(shell_of(scene, from, a, b, stations, measured, [true, true], arm));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box round the z axis, 0.2 wide in x and 0.1 in y, from z 0 to 1.
    fn slab() -> Vec<[Vec3; 3]> {
        let c = |x: f32, y: f32, z: f32| Vec3::new(x * 0.1, y * 0.05, z);
        let quad = |p: [Vec3; 4]| [[p[0], p[1], p[2]], [p[0], p[2], p[3]]];
        let mut out = Vec::new();
        for (sx, sy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let side = if sx != 0.0 {
                [c(sx, -1.0, 0.0), c(sx, 1.0, 0.0), c(sx, 1.0, 1.0), c(sx, -1.0, 1.0)]
            } else {
                [c(-1.0, sy, 0.0), c(1.0, sy, 0.0), c(1.0, sy, 1.0), c(-1.0, sy, 1.0)]
            };
            out.extend(quad(side));
        }
        out
    }

    fn slab_shell() -> WorldShell {
        let (a, b) = (Vec3::new(0.0, 0.0, 0.1), Vec3::new(0.0, 0.0, 0.9));
        let (e1, e2, radii) = measure(a, b, Vec3::X, 3, &slab(), &[], Take::First, 0.5).expect("measured");
        Shell {
            node: 0,
            a,
            b,
            e1,
            e2,
            stations: 3,
            radii: radii.into(),
            caps: [true, true],
            outside: false,
        }
        .in_world(&Mat4::IDENTITY)
    }

    #[test]
    fn a_shell_takes_the_thickness_of_each_side() {
        let shell = slab_shell();
        // Wide in x, thin in y: a capsule would take one radius for both.
        assert!((shell.radius(0.5, 0.0) - 0.1).abs() < 1e-4);
        assert!((shell.radius(0.5, std::f32::consts::FRAC_PI_2) - 0.05).abs() < 1e-4);
        assert!((shell.radius(0.5, std::f32::consts::PI) - 0.1).abs() < 1e-4);
    }

    #[test]
    fn a_point_is_pushed_out_to_the_skin_on_its_side() {
        let shell = slab_shell();
        let mut p = Vec3::new(0.0, 0.02, 0.5);
        let (dir, depth) = shell.push_out(&mut p, 0.01).expect("inside");
        assert!((p.y - 0.06).abs() < 1e-4, "{p}");
        assert!((dir - Vec3::Y).length() < 1e-4 && (depth - 0.04).abs() < 1e-4);
        // Outside the thin side but within the wide side's radius: left.
        let mut q = Vec3::new(0.0, 0.08, 0.5);
        assert!(shell.push_out(&mut q, 0.01).is_none());
    }

    #[test]
    fn an_arm_is_left_on_the_side_it_is_told() {
        let shell = slab_shell();
        // A point just above the middle leaves downwards, through it.
        let mut p = Vec3::new(0.0, 0.01, 0.5);
        shell.push_along(&mut p, 0.0, -Vec3::Y).expect("inside");
        assert!((p.y + 0.05).abs() < 1e-4, "{p}");
    }

    #[test]
    fn the_way_to_a_shell_is_to_its_line_and_tells_how_far_the_skin_is() {
        // The slab is 5 cm thick above and below its line.
        let shell = slab_shell();
        let (way, off) = shell.towards(Vec3::new(0.0, 0.2, 0.5)).expect("off the line");
        assert!((way + Vec3::Y).length() < 1e-4, "{way}");
        assert!((off - 0.15).abs() < 1e-3, "{off}");
        // Inside it the distance is below zero.
        let (_, off) = shell.towards(Vec3::new(0.0, 0.02, 0.5)).expect("off the line");
        assert!((off + 0.03).abs() < 1e-3, "{off}");
    }

    #[test]
    fn an_open_end_has_no_cap() {
        let mut shell = slab_shell();
        assert!(shell.depth(Vec3::new(0.0, 0.0, 0.07), 0.0) > 0.0);
        shell.caps = [false, false];
        assert_eq!(shell.depth(Vec3::new(0.0, 0.0, 0.07), 0.0), 0.0);
        assert!(shell.depth(Vec3::new(0.0, 0.01, 0.5), 0.0) > 0.0);
    }

    #[test]
    fn a_direction_that_meets_nothing_takes_the_middle_of_its_ring() {
        // Only the two wide sides: the rays along x meet nothing.
        let sides: Vec<[Vec3; 3]> = slab().into_iter().skip(4).collect();
        let (_, _, radii) = measure(
            Vec3::new(0.0, 0.0, 0.1),
            Vec3::new(0.0, 0.0, 0.9),
            Vec3::X,
            2,
            &sides,
            &[],
            Take::First,
            0.5,
        )
        .expect("measured");
        assert!(radii.iter().all(|&r| r > 0.04));
    }
}
