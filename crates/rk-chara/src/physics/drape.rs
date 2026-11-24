// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The cloth solver for skirts and dresses that runs on the GPU
//! (`rk_render::drape`). Renkatsu's own, not the game's.
//!
//! This file holds what the GPU solver is given (`Setup` once, a `Frame`
//! per physics frame), the driver that makes the frames from the posed
//! body (`Drape`), and a slow twin of the solver on the CPU (`run`). The
//! twin is the reference the shader is tested against and nothing else:
//! the app never runs it.
//!
//! The solver, per substep and all points at once:
//! - `predict`: gravity and damping move each point on; the rows fixed to
//!   the body go where the waist carries them.
//! - `solve`, from the predicted places of the neighbours:
//!   1. the lengths along the net and its diagonals (XPBD, Macklin et al.
//!      2016), each point taking the mean of what its stretched links ask
//!      (Jacobi with averaging, Macklin et al. 2014 "Unified Particle
//!      Physics"), and a link pushed together pushing back with its
//!      buckling load;
//!   2. bending, as hinges of three points in a row or a column;
//!   3. the tether to the fixed top of its column (Kim et al. 2012, "Long
//!      Range Attachments");
//!   4. contact, last (NvCloth, Houdini Vellum): one plane per body shell
//!      it is near, the floor and the map's triangles found at the start
//!      of the frame. A shell's plane is taken from where the point was
//!      on the limb when the substep began and carried with the limb to
//!      the substep's end (Lewin 2018, "Cloth Self Collision with
//!      Predictive Contacts"), so the point ends on the side it began on
//!      however fast the limb goes. The middles of its links are kept
//!      out of the shells too (NvCloth's virtual particles). Planes that
//!      pull different ways are averaged (NvCloth), each by how deep the
//!      point is behind it: cloth between two thighs sits between them.
//!      What cannot give way, the floor and the map, has the last word.
//!   5. friction on the positions, with a static and a kinetic
//!      coefficient, against each thing's own motion and no more than its
//!      push (Macklin et al. 2014).
//!
//! The speed is what the point moved by over the substep (PBD, Mueller
//! et al. 2007). A point that began the substep inside something is put
//! out without speed (Macklin et al. 2014, pre-stabilisation).
//!
//! One round of this per substep and many substeps (Macklin et al. 2019,
//! "Small Steps in Physics Simulation"). Jacobi rather than colours: two
//! passes per substep whatever the net, the result does not depend on an
//! order, contacts and lengths meet in one place per point, and the twin
//! does the same sums in the same order.
//!
//! Not here yet: the cloth's own collisions, resting fold angles and the
//! kinds of cloth (step 2 fills `Material` per garment).

use super::cloth::{Nearby, Obstacles};
use super::net::{Binding, Net};
use super::shell::{Shell, ANGLES};
use super::world;
use crate::scene::Scene;
use glam::{Mat4, Vec3};
use std::sync::{Arc, Mutex};

/// What a piece of cloth is made of. One for every garment for now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    /// Peirce's bending length in metres: bending stiffness per width
    /// over weight per area is g times its cube (Peirce 1930, "The
    /// handle of cloth as a measurable quantity"). A strip twice this
    /// long held out level by one end hangs 41.5 degrees down. Apparel
    /// cloth is 1.5 to 5 cm; 4 cm is what the net before this used down
    /// the skirt.
    pub bending_length: f32,
    /// Shear stiffness in newtons per metre and radian. Kawabata's
    /// tables put apparel cloth at 0.5 to 1.5 gf/(cm deg); 1 gf/(cm deg)
    /// is 56 N/m per radian. From memory, to be checked in step 2.
    pub shear: f32,
    /// Weight per area, kg/m2: apparel cloth is 0.15 to 0.25.
    pub weight: f32,
    /// How far the innermost layer keeps off the skin, the floor and
    /// the map: half a centimetre covers a 4.3 cm link's sag over a
    /// thigh of 7 cm radius (0.043^2 / (8 x 0.07) = 3.3 mm).
    pub thickness: f32,
    /// Static and kinetic friction of cloth on skin. Measured values for
    /// dry skin are 0.3 to 0.6 (Derler and Gerhardt 2012, "Tribology of
    /// skin", from memory). The same on the floor and the map for now.
    pub friction: [f32; 2],
    /// How fast speed dies away, per second. The user's Unity demos take
    /// 0.05 of it per step of 1/90 s: -ln(0.95) x 90.
    pub damping: f32,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            bending_length: 0.04,
            shear: 56.0,
            weight: 0.2,
            thickness: 0.006,
            friction: [0.5, 0.4],
            damping: 4.6,
        }
    }
}

const GRAVITY: f32 = 9.81;
/// The time one substep stands for. The fastest a leg goes in the walk
/// and run animations is 8 cm in a frame of 1/60 s (measured for the
/// research notes), and a limb is to move less than the cloth's
/// thickness per substep (Mueller, Ten Minute Physics 15): 8 cm over
/// 6 mm is 14, so 16 substeps per 1/60 s.
pub const SUBSTEP_TIME: f32 = 1.0 / 960.0;
/// The most substeps a frame is cut into: a frame of 1/30 s.
pub const MOST_SUBSTEPS: u32 = 32;
/// The fastest a point goes, m/s: the particle speed limit of the user's
/// Unity demos.
pub const FASTEST: f32 = 4.0;
/// Links and hinges per point, map planes kept per point and the most
/// planes a point is held by at once.
pub const LINKS: usize = 8;
pub const HINGES: usize = 6;
pub const MAP_PLANES: usize = 4;
pub const BODY_PLANES: usize = 16;
/// No point.
pub const NONE: u32 = u32::MAX;
/// How near a shell's skin a point is given its plane: twice what the
/// fastest point and the fastest limb close in on each other in a
/// substep, 2 x (4 + 4.8) m/s / 960.
pub const NEAR: f32 = 0.02;
/// The least a hinge's compliance is, in units of a point's lightness
/// over the substep squared. A point's hinges add up while its lengths
/// are averaged, and with stiffer hinges than this the two swing each
/// other up: a strip of points 1 cm apart never comes to rest at 7.5
/// and does at 15 (the strip test below); twice that. A net of 4.3 cm
/// in cloth of 4 cm bending length is at 1250 and far from it; only a
/// very fine net or very stiff cloth is made softer by this.
pub const HINGE_LEAST: f32 = 30.0;
/// How much of being pressed is left 1/60 s after the cloth came off,
/// and how much of what a layer stands off the net by goes where it is
/// pressed: as the net before this (`net.rs`).
pub const PRESSED_FADE: f32 = 0.7;
pub const FLATTEN: f32 = 0.8;
/// How far a vertex on the net is kept off the skin.
pub const VERTEX_GAP: f32 = 0.003;
/// Frames the body is moved into its pose over when the cloth starts,
/// and the time each stands for: as the net before this.
const WARM_UP: u32 = 40;
const WARM_UP_STEP: f32 = 1.0 / 60.0;
/// A part of the body that moves further than this in a frame, as seen
/// from the waist, starts the cloth again: as the net before this.
const JUMP: f32 = 0.25;
/// How much further than the cloth reaches the map is gathered.
const MAP_MARGIN: f32 = 0.1;

/// A point of cloth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// Where it rests, in the space of its net's waist.
    pub rest: Vec3,
    pub net: u32,
    /// Fixed to the body, or no cloth there: it goes where the waist
    /// carries it.
    pub fixed: bool,
    /// How far off the skin it keeps, of the body and of an arm or hand.
    pub gap: f32,
    pub gap_out: f32,
    /// How deep in the body it rests as the garment was made: it may be
    /// that deep in it.
    pub allow: f32,
    /// The fixed point at the top of its column and the most it gets
    /// away from it.
    pub anchor: u32,
    pub tether: f32,
    /// How far out of the net its innermost layer rests.
    pub inner: f32,
    /// The points that give the directions round the skirt and down it
    /// here (`round = (p[1] - p[0]) * turn`, `down = p[3] - p[2]`).
    pub around: [u32; 4],
    pub turn: f32,
}

/// A length kept between a point and one next to it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Link {
    pub to: u32,
    pub rest: f32,
    /// Compliance when pulled apart, as seconds squared for a point of
    /// lightness one; zero down and round the skirt.
    pub soft: f32,
    /// What it pushes its ends apart by when pushed together, m/s2.
    pub buckle: f32,
}

/// A bend kept at a point: the middle of three in a row or a column is
/// `rest` off the middle of the other two (Kelager et al. 2010, "A
/// Triangle Bending Constraint Model for Position-Based Dynamics", with
/// the midpoint for the centroid). In the first two slots of a point
/// the point is the middle and `a`, `b` the ends; in the others it is
/// an end, `a` the middle and `b` the other end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hinge {
    pub a: u32,
    pub b: u32,
    pub rest: f32,
    pub soft: f32,
}

/// One net's part of the points.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub first: usize,
    pub cols: usize,
    pub rows: usize,
    /// Rows from the waist down that are fixed.
    pub fixed: usize,
    /// Per point: where the garment has no cloth, as `Net::open`.
    pub open: Vec<u8>,
}

/// Everything about a character's cloth that does not change from frame
/// to frame.
#[derive(Debug, Clone)]
pub struct Setup {
    pub material: Material,
    pub points: Vec<Point>,
    /// `LINKS` per point.
    pub links: Vec<Link>,
    /// `HINGES` per point.
    pub hinges: Vec<Hinge>,
    /// The body, each shell in the space of its node.
    pub shells: Vec<Shell>,
    pub nets: Vec<Part>,
    /// The garments' vertices on the nets; corners count from their
    /// net's first point.
    pub bindings: Arc<Vec<Binding>>,
}

impl Setup {
    /// Poses per boundary between substeps: the shells' nodes, then the
    /// nets' waists.
    pub fn slots(&self) -> usize {
        self.shells.len() + self.nets.len()
    }
}

/// How a frame begins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Begin {
    /// From where the last one ended.
    Keep,
    /// Every point where it rests on its waist, without speed.
    Reset,
    /// The cloth moved with the character by this.
    Carry(Mat4),
}

/// One physics frame of the solver.
#[derive(Debug, Clone)]
pub struct Frame {
    pub dt: f32,
    pub substeps: u32,
    /// Where each slot is in the world at each boundary between
    /// substeps: `(substeps + 1) * slots`, the frame's start first.
    pub poses: Vec<Mat4>,
    pub begin: Begin,
    /// Whether arms and hands are there.
    pub arms: bool,
    /// The floor where there is no map: a point of it and the way up.
    pub floor: Option<(Vec3, Vec3)>,
    /// The map's triangles near the cloth, fronts by their winding.
    pub map: Option<Arc<Vec<[Vec3; 3]>>>,
}

/// The cloth read back from the GPU: each point's place with how much
/// it is pressed, and its speed.
pub type Readback<'a> = (&'a [[f32; 4]], &'a [[f32; 4]]);

/// What the solver moves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub pos: Vec<Vec3>,
    pub vel: Vec<Vec3>,
    pub pressed: Vec<f32>,
    /// `MAP_PLANES` per point: a plane of the map it may meet this
    /// frame, as its normal and how far along it the plane is; a zero
    /// normal for none.
    pub planes: Vec<[f32; 4]>,
}

impl State {
    pub fn new(points: usize) -> Self {
        State {
            pos: vec![Vec3::ZERO; points],
            vel: vec![Vec3::ZERO; points],
            pressed: vec![0.0; points],
            planes: vec![[0.0; 4]; points * MAP_PLANES],
        }
    }
}

/// A shell's thickness `t` of the way along its line in the direction at
/// `angle` round it, in its own space. As `WorldShell::radius`.
pub fn radius(radii: &[f32], stations: usize, t: f32, angle: f32) -> f32 {
    let s = t.clamp(0.0, 1.0) * (stations - 1) as f32;
    let s0 = (s as usize).min(stations - 1);
    let s1 = (s0 + 1).min(stations - 1);
    let fs = s - s0 as f32;
    let tau = std::f32::consts::TAU;
    let turn = (angle - (angle / tau).floor() * tau) / tau * ANGLES as f32;
    let a0 = (turn as usize) % ANGLES;
    let a1 = (a0 + 1) % ANGLES;
    let fa = turn - turn.floor();
    let at = |s: usize, a: usize| radii[s * ANGLES + a];
    let near = at(s0, a0) + (at(s0, a1) - at(s0, a0)) * fa;
    let far = at(s1, a0) + (at(s1, a1) - at(s1, a0)) * fa;
    near + (far - near) * fs
}

/// Where `l`, a point in a shell's own space, is on the shell: the
/// point of its line it is measured from, the way from there to it, how
/// far that is and how thick the shell is that way. None past an open
/// end or on the line itself.
fn on_shell(s: &Shell, ends: [f32; 2], l: Vec3) -> Option<(Vec3, Vec3, f32, f32)> {
    let axis = s.b - s.a;
    let len2 = axis.length_squared().max(1e-12);
    let t = (l - s.a).dot(axis) / len2;
    if (t < 0.0 && !s.caps[0]) || (t > 1.0 && !s.caps[1]) {
        return None;
    }
    let beyond = !(0.0..=1.0).contains(&t);
    let t = t.clamp(0.0, 1.0);
    let on = s.a + axis * t;
    let off = l - on;
    let dist = off.length();
    if dist < 1e-6 {
        return None;
    }
    let r = if beyond {
        ends[usize::from(t > 0.5)]
    } else {
        radius(&s.radii, s.stations, t, off.dot(s.e2).atan2(off.dot(s.e1)))
    };
    Some((on, off / dist, dist, r))
}

/// A plane a point is kept in front of during a substep.
#[derive(Debug, Clone, Copy, Default)]
struct Plane {
    n: Vec3,
    d: f32,
    /// How the thing moved at the point over the substep.
    moved: Vec3,
    /// Where the plane was along `n` when the substep began.
    began: f32,
    /// What it pushed the point by.
    pushed: f32,
}

/// The closest point of the triangle to `p` (Ericson, Real-Time Collision
/// Detection 5.1.5).
fn closest(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// The planes of the map a point at `p` may meet within a frame: of
/// each triangle it is in front of and within `reach` of, the plane
/// through the nearest point of the triangle, facing the point (the
/// triangle's own where that point is inside it). The `MAP_PLANES`
/// nearest are kept.
fn map_planes(p: Vec3, triangles: &[[Vec3; 3]], reach: f32, behind: f32, out: &mut [[f32; 4]]) {
    let mut far = [f32::MAX; MAP_PLANES];
    for plane in out.iter_mut() {
        *plane = [0.0; 4];
    }
    for &[a, b, c] in triangles {
        let front = (b - a).cross(c - a);
        let area = front.length();
        if area < 1e-12 {
            continue;
        }
        let front = front / area;
        let height = (p - a).dot(front);
        if height < -behind || height > reach {
            continue;
        }
        let q = closest(p, a, b, c);
        let off = p - q;
        let dist = off.length();
        if dist > reach {
            continue;
        }
        // Over the triangle itself its own plane; past its edge the
        // plane that faces the point, which rounds the edge.
        let along = (off - front * height).length();
        let n = if along < 1e-5 || dist < 1e-6 { front } else { off / dist };
        if n.dot(front) < 0.0 {
            continue;
        }
        let dist = (p - q).dot(n);
        let mut k = MAP_PLANES;
        while k > 0 && dist < far[k - 1] {
            k -= 1;
        }
        if k < MAP_PLANES {
            for j in (k + 1..MAP_PLANES).rev() {
                far[j] = far[j - 1];
                out[j] = out[j - 1];
            }
            far[k] = dist;
            out[k] = [n.x, n.y, n.z, q.dot(n)];
        }
    }
}

/// Runs one frame of the solver on the CPU, as `drape.wgsl` does on the
/// GPU. A test reference: slow on purpose, one point after another.
pub fn run(setup: &Setup, state: &mut State, frame: &Frame) {
    let n = setup.points.len();
    let slots = setup.slots();
    let material = &setup.material;
    let subs = frame.substeps.max(1) as usize;
    let h = frame.dt / subs as f32;
    let waist = |boundary: usize, net: u32| frame.poses[boundary * slots + setup.shells.len() + net as usize];
    match frame.begin {
        Begin::Keep => {}
        Begin::Reset => {
            for (i, p) in setup.points.iter().enumerate() {
                state.pos[i] = waist(0, p.net).transform_point3(p.rest);
                state.vel[i] = Vec3::ZERO;
                state.pressed[i] = 0.0;
            }
        }
        Begin::Carry(m) => {
            for i in 0..n {
                state.pos[i] = m.transform_point3(state.pos[i]);
                state.vel[i] = m.transform_vector3(state.vel[i]);
            }
        }
    }
    // The map's planes, from where the points are as the frame begins.
    let reach = FASTEST * frame.dt + material.thickness;
    for i in 0..n {
        let out = &mut state.planes[i * MAP_PLANES..(i + 1) * MAP_PLANES];
        match &frame.map {
            Some(map) if !setup.points[i].fixed => map_planes(state.pos[i], map, reach, material.thickness, out),
            _ => out.fill([0.0; 4]),
        }
    }
    let ends: Vec<[f32; 2]> = setup.shells.iter().map(Shell::ends).collect();
    let damp = (-material.damping * h).exp();
    let fade = PRESSED_FADE.powf(h * 60.0);
    let mut next = vec![Vec3::ZERO; n];
    for sub in 0..subs {
        let pose = |boundary: usize, k: usize| frame.poses[(sub + boundary) * slots + k];
        // Predict.
        for (i, p) in setup.points.iter().enumerate() {
            next[i] = if p.fixed {
                waist(sub + 1, p.net).transform_point3(p.rest)
            } else {
                let v = (state.vel[i] + Vec3::new(0.0, -GRAVITY, 0.0) * h) * damp;
                state.pos[i] + v * h
            };
        }
        // Solve.
        for (i, p) in setup.points.iter().enumerate() {
            if p.fixed {
                state.vel[i] = (next[i] - state.pos[i]) / h;
                state.pos[i] = next[i];
                continue;
            }
            let x0 = state.pos[i];
            let y = next[i];
            let light = |j: u32| if setup.points[j as usize].fixed { 0.0 } else { 1.0 };
            // 1. Lengths.
            let (mut sum, mut count) = (Vec3::ZERO, 0.0f32);
            for link in &setup.links[i * LINKS..(i + 1) * LINKS] {
                if link.to == NONE {
                    continue;
                }
                let d = y - next[link.to as usize];
                let len = d.length();
                if len < 1e-9 {
                    continue;
                }
                let c = len - link.rest;
                let w = light(link.to);
                if c > 0.0 {
                    sum -= d * (c / (len * (1.0 + w + link.soft / (h * h))));
                } else {
                    // Pushed together it pushes back with its buckling
                    // load, and no further than to its length.
                    sum += d * ((link.buckle * h * h).min(-c / (1.0 + w)) / len);
                }
                count += 1.0;
            }
            let mut x = y;
            if count > 0.0 {
                x += sum / count;
            }
            // 2. Bends.
            let mut bend = Vec3::ZERO;
            for (slot, hinge) in setup.hinges[i * HINGES..(i + 1) * HINGES].iter().enumerate() {
                if hinge.a == NONE {
                    continue;
                }
                let (ya, yb) = (next[hinge.a as usize], next[hinge.b as usize]);
                let middle = slot < 2;
                let d = if middle {
                    y - (ya + yb) * 0.5
                } else {
                    ya - (y + yb) * 0.5
                };
                let len = d.length();
                if len < 1e-9 {
                    continue;
                }
                // Lightness times the gradient squared, summed: the
                // middle counts whole and each end a quarter.
                let weights = if middle {
                    1.0 + (light(hinge.a) + light(hinge.b)) * 0.25
                } else {
                    light(hinge.a) + (1.0 + light(hinge.b)) * 0.25
                };
                let soft = (hinge.soft / (h * h)).max(HINGE_LEAST);
                let lambda = -(len - hinge.rest) / (weights + soft);
                bend += d * (lambda / len * if middle { 1.0 } else { -0.5 });
            }
            x += bend;
            // 3. The tether.
            if p.anchor != NONE {
                let top = next[p.anchor as usize];
                let d = x - top;
                let len = d.length();
                if len > p.tether {
                    x = top + d * (p.tether / len);
                }
            }
            // 4. Contact: the planes of the body near the point.
            let mut body = [Plane::default(); BODY_PLANES];
            let mut bodies = 0;
            for (k, s) in setup.shells.iter().enumerate() {
                if bodies == BODY_PLANES || (s.outside && !frame.arms) {
                    continue;
                }
                let (from, to) = (pose(0, k), pose(1, k));
                let scale = to.x_axis.truncate().length();
                let l = from.inverse().transform_point3(x0);
                let Some((on, dir, dist, r)) = on_shell(s, ends[k], l) else {
                    continue;
                };
                let gap = if s.outside { p.gap_out } else { p.gap - p.allow };
                let skin = r * scale + gap;
                let began = skin - dist * scale;
                if began < -NEAR {
                    continue;
                }
                let moved = to.transform_point3(l) - x0;
                let mut plane = Plane {
                    n: to.transform_vector3(dir).normalize_or_zero(),
                    d: 0.0,
                    moved,
                    began: 0.0,
                    pushed: 0.0,
                };
                plane.d = to.transform_point3(on).dot(plane.n) + skin;
                plane.began = x0.dot(plane.n) + began;
                if s.outside && began > 0.0 {
                    // Inside an arm or a hand the cloth leaves it
                    // towards the body: it is under a hand on a lap.
                    let mut best = (f32::MAX, Vec3::ZERO);
                    for (j, t) in setup.shells.iter().enumerate() {
                        if t.outside {
                            continue;
                        }
                        let at = pose(0, j);
                        let lt = at.inverse().transform_point3(x0);
                        let axis = t.b - t.a;
                        let along = ((lt - t.a).dot(axis) / axis.length_squared().max(1e-12)).clamp(0.0, 1.0);
                        let off = lt - (t.a + axis * along);
                        let far = off.length();
                        if far < 1e-6 {
                            continue;
                        }
                        let thick = radius(&t.radii, t.stations, along, off.dot(t.e2).atan2(off.dot(t.e1)));
                        let clear = (far - thick) * at.x_axis.truncate().length();
                        if clear < best.0 {
                            best = (clear, at.transform_vector3(-off).normalize_or_zero());
                        }
                    }
                    if best.0 < f32::MAX {
                        let way = best.1;
                        let off = from.transform_vector3(l - on);
                        let along = off.dot(way);
                        let out = -along + (along * along + skin * skin - off.length_squared()).max(0.0).sqrt();
                        plane.n = way;
                        plane.began = (x0 + way * out).dot(way);
                        plane.d = (x0 + way * out + moved).dot(way);
                    }
                }
                body[bodies] = plane;
                bodies += 1;
            }
            // The middles of its links, between where its lengths put the
            // point and where the other end is predicted: a
            // limb thinner than the net is wide would come through a
            // cell between points that go round it (NvCloth's virtual
            // particles). A middle in a shell is put out of it by both
            // ends alike. Not the links to the fixed top: it sits in
            // the body where a thigh comes up, and its links with it.
            for link in &setup.links[i * LINKS..(i + 1) * LINKS] {
                if link.to == NONE || light(link.to) == 0.0 {
                    continue;
                }
                let other = next[link.to as usize];
                let middle = (x + other) * 0.5;
                for (k, s) in setup.shells.iter().enumerate() {
                    if bodies == BODY_PLANES || (s.outside && !frame.arms) {
                        continue;
                    }
                    let (from, to) = (pose(0, k), pose(1, k));
                    let scale = to.x_axis.truncate().length();
                    let Some((_, dir, dist, r)) = on_shell(s, ends[k], to.inverse().transform_point3(middle)) else {
                        continue;
                    };
                    let gap = if s.outside { p.gap_out } else { p.gap - p.allow };
                    let skin = r * scale + gap;
                    let deep = skin - dist * scale;
                    if deep <= 0.0 {
                        continue;
                    }
                    let nrm = to.transform_vector3(dir).normalize_or_zero();
                    // Each end goes out by as much as the middle is in:
                    // nothing where it only just touches.
                    body[bodies] = Plane {
                        n: nrm,
                        d: x.dot(nrm) + deep,
                        moved: to.transform_point3(from.inverse().transform_point3(x0)) - x0,
                        // With its speed: a middle put out without it
                        // (tried) keeps falling in and never rests.
                        began: -f32::MAX,
                        pushed: 0.0,
                    };
                    bodies += 1;
                }
            }
            // What cannot give way: the floor and the map.
            let mut ground = [Plane::default(); MAP_PLANES + 1];
            let mut grounds = 0;
            if let Some((at, up)) = frame.floor {
                let d = at.dot(up) + p.gap;
                ground[grounds] = Plane {
                    n: up,
                    d,
                    began: d,
                    ..Default::default()
                };
                grounds += 1;
            }
            for plane in &state.planes[i * MAP_PLANES..(i + 1) * MAP_PLANES] {
                let nrm = Vec3::new(plane[0], plane[1], plane[2]);
                if nrm == Vec3::ZERO {
                    continue;
                }
                let d = plane[3] + p.gap;
                ground[grounds] = Plane {
                    n: nrm,
                    d,
                    began: d,
                    ..Default::default()
                };
                grounds += 1;
            }
            // A point that began inside something is put out of it
            // without speed: where it began is taken through the planes
            // as they were then, and the point moved by as much.
            let mut started = x0;
            for _ in 0..2 {
                for set in [&body[..bodies], &ground[..grounds]] {
                    let (mut sum, mut count) = (Vec3::ZERO, 0.0f32);
                    for plane in set {
                        let c = started.dot(plane.n) - plane.began;
                        if c < 0.0 {
                            sum += plane.n * (c * c);
                            count -= c;
                        }
                    }
                    if count > 0.0 {
                        started += sum / count;
                    }
                }
            }
            x += started - x0;
            // Two rounds: the body's planes averaged, then the ground's.
            // Each plane counts by how deep the point is behind it, so
            // that one it barely touches does not halve the others.
            for _ in 0..2 {
                for set in [&mut body[..bodies], &mut ground[..grounds]] {
                    let (mut sum, mut count) = (Vec3::ZERO, 0.0f32);
                    for plane in set.iter() {
                        let c = x.dot(plane.n) - plane.d;
                        if c < 0.0 {
                            sum += plane.n * (c * c);
                            count -= c;
                        }
                    }
                    if count > 0.0 {
                        for plane in set.iter_mut() {
                            let c = x.dot(plane.n) - plane.d;
                            if c < 0.0 {
                                plane.pushed += c * c / count;
                            }
                        }
                        x += sum / count;
                    }
                }
            }
            // 5. Friction, against each thing that pushed.
            let (mut rub, mut rubs) = (Vec3::ZERO, 0.0f32);
            for plane in body[..bodies].iter().chain(&ground[..grounds]) {
                if plane.pushed <= 0.0 {
                    continue;
                }
                rubs += plane.pushed;
                let slid = x - started - plane.moved;
                let across = slid - plane.n * slid.dot(plane.n);
                let far = across.length();
                if far < 1e-9 {
                    continue;
                }
                // Each by its share of the pushing.
                if far < material.friction[0] * plane.pushed {
                    rub -= across * plane.pushed;
                } else {
                    rub -= across * ((material.friction[1] * plane.pushed / far).min(1.0) * plane.pushed);
                }
            }
            if rubs > 0.0 {
                x += rub / rubs;
            }
            let mut v = (x - started) / h;
            let speed = v.length();
            if speed > FASTEST {
                v *= FASTEST / speed;
            }
            state.vel[i] = v;
            state.pos[i] = x;
            state.pressed[i] = if rubs > 0.0 { 1.0 } else { state.pressed[i] * fade };
        }
    }
}

/// A character's cloth for the renderer: what it is and the frames the
/// physics made that the GPU has not run yet.
#[derive(Debug)]
pub struct Live {
    pub setup: Setup,
    pub pending: Mutex<Vec<Frame>>,
}

impl Live {
    /// Takes the frames that wait.
    pub fn take(&self) -> Vec<Frame> {
        std::mem::take(&mut *self.pending.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }

    fn push(&self, frame: Frame) {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(frame);
    }
}

/// A mesh's place on a character's cloth, for the renderer to put its
/// bound vertices on the nets after skinning.
#[derive(Debug, Clone)]
pub struct Dress {
    pub live: Arc<Live>,
    /// Which of `Setup::bindings`.
    pub binding: usize,
}

/// The transform `f` of the way from one to another: the place and the
/// size straight, the turn the short way round.
fn between(from: &Mat4, to: &Mat4, f: f32) -> Mat4 {
    if from == to {
        return *to;
    }
    let (s0, r0, t0) = from.to_scale_rotation_translation();
    let (s1, r1, t1) = to.to_scale_rotation_translation();
    Mat4::from_scale_rotation_translation(s0.lerp(s1, f), r0.slerp(r1, f), t0.lerp(t1, f))
}

/// The poses of a frame of `substeps` in which every slot goes from
/// `from` to `to`.
fn poses(from: &[Mat4], to: &[Mat4], substeps: u32) -> Vec<Mat4> {
    let mut out = Vec::with_capacity((substeps as usize + 1) * to.len());
    for k in 0..=substeps {
        let f = k as f32 / substeps as f32;
        for (a, b) in from.iter().zip(to) {
            out.push(if k == substeps { *b } else { between(a, b, f) });
        }
    }
    out
}

/// How many substeps a frame of `dt` seconds is cut into.
pub fn substeps(dt: f32) -> u32 {
    ((dt / SUBSTEP_TIME).ceil() as u32).clamp(1, MOST_SUBSTEPS)
}

/// Makes the solver's frames from the character as the game poses it.
#[derive(Debug)]
pub struct Drape {
    pub live: Arc<Live>,
    /// The node each slot follows.
    nodes: Vec<usize>,
    /// Where each slot was fitted, in the space of the first waist.
    rest: Vec<Mat4>,
    /// Where each slot was at the frame before, in the world.
    was: Vec<Mat4>,
    /// The shells' middles at the frame before, in the first waist's
    /// space.
    middles: Vec<Vec3>,
    /// How far the cloth reaches from its waists.
    reach: f32,
    ready: bool,
    carried: Option<Mat4>,
    nearby: Nearby,
    map: Option<(usize, Arc<Vec<[Vec3; 3]>>)>,
    /// The fastest a shell's middle went in a frame so far, in metres.
    pub fastest: f32,
}

impl Clone for Drape {
    /// A copy is another character's cloth: it has its own frames and
    /// starts again.
    fn clone(&self) -> Self {
        Drape {
            live: Arc::new(Live {
                setup: self.live.setup.clone(),
                pending: Mutex::default(),
            }),
            nodes: self.nodes.clone(),
            rest: self.rest.clone(),
            was: Vec::new(),
            middles: Vec::new(),
            reach: self.reach,
            ready: false,
            carried: None,
            nearby: Nearby::default(),
            map: None,
            fastest: 0.0,
        }
    }
}

impl Drape {
    /// The cloth of `nets` among `shells`, with the body as it was
    /// fitted: call in the pose both were made in. None without nets.
    pub(super) fn build(
        scene: &Scene,
        nets: &[Net],
        shells: &[Shell],
        bindings: &Arc<Vec<Binding>>,
        material: Material,
    ) -> Option<Drape> {
        if nets.is_empty() {
            return None;
        }
        let fitted: Vec<_> = shells.iter().map(|s| s.in_world(&world(scene, s.node))).collect();
        let (mut points, mut links, mut hinges, mut parts) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (k, net) in nets.iter().enumerate() {
            let part = net.part();
            let first = points.len();
            let (cols, rows) = (part.cols, part.rows);
            let count = cols * rows;
            let at = |c: usize, r: usize| r * cols + (c % cols);
            let none = |i: usize| part.open.get(i).is_some_and(|o| o & super::net::OPEN_POINT != 0);
            let joined = |i: usize| part.open.get(i).is_none_or(|o| o & super::net::OPEN_ROUND == 0);
            let fixed = |i: usize| i < part.fixed * cols || none(i);
            // How far down its column each point is from the fixed top.
            let mut tether = vec![0.0f32; count];
            for i in part.fixed * cols..count {
                tether[i] = tether[i - cols] + (part.rest[i] - part.rest[i - cols]).length();
            }
            for (i, &tether) in tether.iter().enumerate() {
                let (c, r) = (i % cols, i / cols);
                let (next, last) = (at(c + 1, r), at(c + cols - 1, r));
                let (round, turn) = match (joined(i), joined(last)) {
                    (true, false) => ([i, next], 2.0),
                    (false, true) => ([last, i], 2.0),
                    _ => ([last, next], 1.0),
                };
                let down = if r + 1 == rows && rows > 2 {
                    [at(c, r - 2), i]
                } else {
                    [at(c, r.saturating_sub(1)), at(c, (r + 1).min(rows - 1))]
                };
                let rest_world = part.rest[i];
                let allow = fitted
                    .iter()
                    .filter(|s| !s.outside)
                    .map(|s| s.depth(rest_world, part.gap[i]))
                    .fold(0.0, f32::max);
                points.push(Point {
                    rest: part.rest_on_waist[i],
                    net: k as u32,
                    fixed: fixed(i),
                    gap: part.gap[i],
                    gap_out: part.gap_out[i],
                    allow,
                    anchor: if fixed(i) {
                        NONE
                    } else {
                        (first + at(c, part.fixed - 1)) as u32
                    },
                    tether,
                    inner: (material.thickness - part.gap[i]) * part.outward,
                    around: [round[0], round[1], down[0], down[1]].map(|j| (first + j) as u32),
                    turn,
                });
            }
            // The links of each point.
            let mut mine = vec![
                Link {
                    to: NONE,
                    rest: 0.0,
                    soft: 0.0,
                    buckle: 0.0
                };
                count * LINKS
            ];
            let buckle = |rest: f32| {
                std::f32::consts::PI.powi(2) * GRAVITY * material.bending_length.powi(3) / rest.max(1e-4).powi(3)
            };
            for &(a, b, kind) in part.links {
                let (a, b) = (a as usize, b as usize);
                if fixed(a) && fixed(b) {
                    continue;
                }
                let rest = (part.rest[a] - part.rest[b]).length();
                // A diagonal stands for the shear of half a cell.
                let soft = if kind == super::net::DIAGONAL {
                    material.weight * rest * rest * 0.5 / material.shear
                } else {
                    0.0
                };
                for (from, to) in [(a, b), (b, a)] {
                    let slots = &mut mine[from * LINKS..(from + 1) * LINKS];
                    if let Some(slot) = slots.iter_mut().find(|l| l.to == NONE) {
                        *slot = Link {
                            to: (first + to) as u32,
                            rest,
                            soft,
                            buckle: buckle(rest),
                        };
                    }
                }
            }
            links.extend(mine);
            // The hinges: each bend of the net at its middle point and at
            // its two ends.
            let mut mine = vec![
                Hinge {
                    a: NONE,
                    b: NONE,
                    rest: 0.0,
                    soft: 0.0
                };
                count * HINGES
            ];
            for bend in part.bends {
                let [a, b] = bend.ends.map(|e| e as usize);
                let m = bend.at as usize;
                if fixed(a) && fixed(b) && fixed(m) {
                    continue;
                }
                let (pa, pm, pb) = (part.rest[a], part.rest[m], part.rest[b]);
                let rest = (pm - (pa + pb) * 0.5).length();
                // A hinge of links of length L in cloth of bending length
                // c: stiffness 4 B / L^3 per width on the middle's offset
                // h (h = L x angle / 2, energy B angle^2 / 2L), B = weight
                // x g x c^3, over a point's mass weight x L per width.
                let len = ((pm - pa).length() + (pm - pb).length()) * 0.5;
                let soft = len.powi(4) / (4.0 * GRAVITY * material.bending_length.powi(3));
                let hinge = |a: usize, b: usize| Hinge {
                    a: (first + a) as u32,
                    b: (first + b) as u32,
                    rest,
                    soft,
                };
                let mut put = |at: usize, range: std::ops::Range<usize>, hinge: Hinge| {
                    if let Some(slot) = mine[at * HINGES..(at + 1) * HINGES][range]
                        .iter_mut()
                        .find(|h| h.a == NONE)
                    {
                        *slot = hinge;
                    }
                };
                put(m, 0..2, hinge(a, b));
                put(a, 2..HINGES, hinge(m, b));
                put(b, 2..HINGES, hinge(m, a));
            }
            hinges.extend(mine);
            parts.push(Part {
                first,
                cols,
                rows,
                fixed: part.fixed,
                open: part.open.to_vec(),
            });
        }
        let nodes: Vec<usize> = shells
            .iter()
            .map(|s| s.node)
            .chain(nets.iter().map(|n| n.part().waist))
            .collect();
        let back = world(scene, nodes[shells.len()]).inverse();
        let rest: Vec<Mat4> = nodes.iter().map(|&n| back * world(scene, n)).collect();
        let reach = points.iter().map(|p: &Point| p.rest.length()).fold(0.0, f32::max);
        Some(Drape {
            live: Arc::new(Live {
                setup: Setup {
                    material,
                    points,
                    links,
                    hinges,
                    shells: shells.to_vec(),
                    nets: parts,
                    bindings: bindings.clone(),
                },
                pending: Mutex::default(),
            }),
            was: Vec::new(),
            middles: Vec::new(),
            nodes,
            rest,
            reach,
            ready: false,
            carried: None,
            nearby: Nearby::default(),
            map: None,
            fastest: 0.0,
        })
    }

    /// Whether it has made a frame since it last started.
    pub(super) fn ready(&self) -> bool {
        self.ready
    }

    /// Starts the cloth again in the pose the body is in at the next
    /// frame.
    pub(super) fn reset(&mut self) {
        self.ready = false;
    }

    /// Moves the cloth with the character.
    pub(super) fn carry(&mut self, moved: Mat4) {
        if !self.ready {
            return;
        }
        for m in &mut self.was {
            *m = moved * *m;
        }
        self.carried = Some(match self.carried {
            Some(before) => moved * before,
            None => moved,
        });
    }

    /// One frame of `dt` seconds, after the game posed the body.
    /// `active` false leaves the skirts as the game's chains put them.
    pub(super) fn step(
        &mut self,
        scene: &Scene,
        active: bool,
        floor: Option<(Vec3, Vec3)>,
        obstacles: Option<&Arc<Obstacles>>,
        dt: f32,
    ) {
        if !active {
            self.ready = false;
            return;
        }
        let live = self.live.clone();
        let shells = live.setup.shells.len();
        let now: Vec<Mat4> = self.nodes.iter().map(|&n| world(scene, n)).collect();
        let waist = now[shells];
        // A limb that jumped, as when a pose is put on at once, would go
        // through the cloth: it starts again.
        let back = waist.inverse();
        let middles: Vec<Vec3> = live
            .setup
            .shells
            .iter()
            .zip(&now)
            .map(|(s, m)| back.transform_point3(m.transform_point3((s.a + s.b) * 0.5)))
            .collect();
        let far = middles
            .iter()
            .zip(&self.middles)
            .map(|(p, b)| (*p - *b).length())
            .fold(0.0, f32::max);
        let jumped = middles.len() == self.middles.len() && far > JUMP;
        if self.ready && !jumped {
            self.fastest = self.fastest.max(far);
        }
        self.middles = middles;
        if !self.ready || jumped {
            // The cloth begins as it rests on the waist with the body as
            // it was fitted around it, and the body is moved into its
            // pose over the warm-up: the legs take the cloth with them,
            // so that it ends on the right side of them whatever the
            // pose. In the air and without arms, each of which goes a
            // straight way here and would sweep the cloth along.
            // What waited to run is of no use any more.
            drop(live.take());
            let start: Vec<Mat4> = self.rest.iter().map(|m| waist * *m).collect();
            let subs = substeps(WARM_UP_STEP);
            let mut from = start.clone();
            for k in 1..=WARM_UP {
                let f = k as f32 / WARM_UP as f32;
                let to: Vec<Mat4> = start.iter().zip(&now).map(|(a, b)| between(a, b, f)).collect();
                live.push(Frame {
                    dt: WARM_UP_STEP,
                    substeps: subs,
                    poses: poses(&from, &to, subs),
                    begin: if k == 1 { Begin::Reset } else { Begin::Keep },
                    arms: false,
                    floor: None,
                    map: None,
                });
                from = to;
            }
            self.was.clone_from(&now);
            self.carried = None;
            self.ready = true;
        }
        if dt > 0.0 {
            let reach = Vec3::splat(self.reach + MAP_MARGIN);
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for m in now[shells..].iter().chain(&self.was[shells..]) {
                lo = lo.min(m.w_axis.truncate() - reach);
                hi = hi.max(m.w_axis.truncate() + reach);
            }
            self.nearby.refresh(obstacles, lo, hi);
            let from = Arc::as_ptr(&self.nearby.surfaces) as usize;
            if self.map.as_ref().is_none_or(|(of, _)| *of != from) {
                self.map = Some((from, Arc::new(self.nearby.surfaces.corners())));
            }
            let map = self.map.as_ref().map(|(_, m)| m.clone()).filter(|m| !m.is_empty());
            let subs = substeps(dt);
            live.push(Frame {
                dt,
                substeps: subs,
                poses: poses(&self.was, &now, subs),
                begin: self.carried.take().map_or(Begin::Keep, Begin::Carry),
                arms: true,
                floor,
                map,
            });
        }
        self.was = now;
    }
}

/// Small nets made in memory, for the tests of the solver here and of the
/// same on the GPU.
pub mod sample {
    use super::*;

    /// A frame of 1/60 s in which nothing but the cloth moves.
    pub fn still(setup: &Setup, begin: Begin, floor: Option<(Vec3, Vec3)>) -> Frame {
        let subs = substeps(1.0 / 60.0);
        Frame {
            dt: 1.0 / 60.0,
            substeps: subs,
            poses: vec![Mat4::IDENTITY; (subs as usize + 1) * setup.slots()],
            begin,
            arms: true,
            floor,
            map: None,
        }
    }

    /// A tube of cloth round the y axis that hangs from a fixed top row,
    /// with a body shell of `thick` round the line from `a` to `b`: a
    /// small net with everything the solver has.
    pub fn tube(cols: usize, rows: usize, radius: f32, thick: f32, a: Vec3, b: Vec3) -> Setup {
        let material = Material::default();
        let spacing = std::f32::consts::TAU * radius / cols as f32;
        let at = |c: usize, r: usize| r * cols + (c % cols);
        let place = |i: usize| {
            let angle = (i % cols) as f32 / cols as f32 * std::f32::consts::TAU;
            Vec3::new(
                angle.cos() * radius,
                -((i / cols) as f32) * spacing,
                angle.sin() * radius,
            )
        };
        let count = cols * rows;
        let none_link = Link {
            to: NONE,
            rest: 0.0,
            soft: 0.0,
            buckle: 0.0,
        };
        let mut links = vec![none_link; count * LINKS];
        let mut hinges = vec![
            Hinge {
                a: NONE,
                b: NONE,
                rest: 0.0,
                soft: 0.0
            };
            count * HINGES
        ];
        let mut points = Vec::new();
        for i in 0..count {
            let (c, r) = (i % cols, i / cols);
            points.push(Point {
                rest: place(i),
                net: 0,
                fixed: r == 0,
                gap: material.thickness,
                gap_out: material.thickness,
                allow: 0.0,
                anchor: if r == 0 { NONE } else { c as u32 },
                tether: r as f32 * spacing,
                inner: 0.0,
                around: [
                    at(c + cols - 1, r),
                    at(c + 1, r),
                    at(c, r.saturating_sub(1)),
                    at(c, (r + 1).min(rows - 1)),
                ]
                .map(|j| j as u32),
                turn: 1.0,
            });
            let mut near = vec![(at(c + 1, r), false), (at(c + cols - 1, r), false)];
            if r > 0 {
                near.extend([
                    (at(c, r - 1), false),
                    (at(c + 1, r - 1), true),
                    (at(c + cols - 1, r - 1), true),
                ]);
            }
            if r + 1 < rows {
                near.extend([
                    (at(c, r + 1), false),
                    (at(c + 1, r + 1), true),
                    (at(c + cols - 1, r + 1), true),
                ]);
            }
            for (slot, (to, diagonal)) in near.into_iter().enumerate() {
                let rest = (place(i) - place(to)).length();
                links[i * LINKS + slot] = Link {
                    to: to as u32,
                    rest,
                    soft: if diagonal {
                        material.weight * rest * rest * 0.5 / material.shear
                    } else {
                        0.0
                    },
                    buckle: std::f32::consts::PI.powi(2) * GRAVITY * material.bending_length.powi(3) / rest.powi(3),
                };
            }
            let soft = spacing.powi(4) / (4.0 * GRAVITY * material.bending_length.powi(3));
            // A hinge with `m` in the middle of `e` and `f`, as this
            // point holds it.
            let hinge = |m: usize, e: usize, f: usize| {
                let rest = (place(m) - (place(e) + place(f)) * 0.5).length();
                let (a, b) = if m == i {
                    (e, f)
                } else {
                    (m, if e == i { f } else { e })
                };
                Hinge {
                    a: a as u32,
                    b: b as u32,
                    rest,
                    soft,
                }
            };
            let (next, last) = (at(c + 1, r), at(c + cols - 1, r));
            hinges[i * HINGES] = hinge(i, last, next);
            hinges[i * HINGES + 2] = hinge(next, i, at(c + 2, r));
            hinges[i * HINGES + 3] = hinge(last, i, at(c + cols - 2, r));
            if r > 0 && r + 1 < rows {
                hinges[i * HINGES + 1] = hinge(i, at(c, r - 1), at(c, r + 1));
            }
            if r + 2 < rows {
                hinges[i * HINGES + 4] = hinge(at(c, r + 1), i, at(c, r + 2));
            }
            if r >= 2 {
                hinges[i * HINGES + 5] = hinge(at(c, r - 1), i, at(c, r - 2));
            }
        }
        let stations = 3;
        let axis = (b - a).normalize();
        let e1 = axis.any_orthonormal_vector();
        Setup {
            material,
            points,
            links,
            hinges,
            shells: vec![Shell {
                node: 0,
                a,
                b,
                e1,
                e2: axis.cross(e1),
                stations,
                radii: vec![thick; stations * ANGLES].into(),
                caps: [true, true],
                outside: false,
            }],
            nets: vec![Part {
                first: 0,
                cols,
                rows,
                fixed: 1,
                open: Vec::new(),
            }],
            bindings: Arc::default(),
        }
    }

    /// Frames in which the tube's shell goes from `from` to `to` at an
    /// even pace and the waist stays.
    pub fn sweep(setup: &Setup, from: Mat4, to: Mat4, frames: u32, floor: Option<(Vec3, Vec3)>) -> Vec<Frame> {
        let subs = substeps(1.0 / 60.0);
        (0..frames)
            .map(|k| {
                let at = |f: f32| vec![between(&from, &to, f), Mat4::IDENTITY];
                Frame {
                    dt: 1.0 / 60.0,
                    substeps: subs,
                    poses: poses(&at(k as f32 / frames as f32), &at((k + 1) as f32 / frames as f32), subs),
                    begin: if k == 0 { Begin::Reset } else { Begin::Keep },
                    arms: true,
                    floor,
                    map: None,
                }
            })
            .collect::<Vec<_>>()
            .into_iter()
            .inspect(|f| assert_eq!(f.poses.len(), (subs as usize + 1) * setup.slots()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::sample::{still, sweep, tube};
    use super::*;

    /// A flat strip of `count` points `spacing` apart along x from the
    /// origin, the first two fixed, with the hinges and links of one
    /// column.
    fn strip(count: usize, spacing: f32, material: Material) -> Setup {
        let mut points = Vec::new();
        let none_link = Link {
            to: NONE,
            rest: 0.0,
            soft: 0.0,
            buckle: 0.0,
        };
        let none_hinge = Hinge {
            a: NONE,
            b: NONE,
            rest: 0.0,
            soft: 0.0,
        };
        let mut links = vec![none_link; count * LINKS];
        let mut hinges = vec![none_hinge; count * HINGES];
        let link = |to: usize| Link {
            to: to as u32,
            rest: spacing,
            soft: 0.0,
            buckle: std::f32::consts::PI.powi(2) * GRAVITY * material.bending_length.powi(3) / spacing.powi(3),
        };
        let soft = spacing.powi(4) / (4.0 * GRAVITY * material.bending_length.powi(3));
        for i in 0..count {
            points.push(Point {
                rest: Vec3::new(i as f32 * spacing, 0.0, 0.0),
                net: 0,
                fixed: i < 2,
                gap: material.thickness,
                gap_out: material.thickness,
                allow: 0.0,
                anchor: if i < 2 { NONE } else { 1 },
                tether: (i as f32 - 1.0) * spacing,
                inner: 0.0,
                around: [0; 4],
                turn: 1.0,
            });
            if i > 0 {
                links[i * LINKS] = link(i - 1);
            }
            if i + 1 < count {
                links[i * LINKS + 1] = link(i + 1);
            }
            let hinge = |a: usize, b: usize| Hinge {
                a: a as u32,
                b: b as u32,
                rest: 0.0,
                soft,
            };
            if i > 0 && i + 1 < count {
                hinges[i * HINGES] = hinge(i - 1, i + 1);
            }
            if i + 2 < count {
                hinges[i * HINGES + 2] = hinge(i + 1, i + 2);
            }
            if i >= 2 {
                hinges[i * HINGES + 3] = hinge(i - 1, i - 2);
            }
        }
        Setup {
            material,
            points,
            links,
            hinges,
            shells: Vec::new(),
            nets: vec![Part {
                first: 0,
                cols: 1,
                rows: count,
                fixed: 2,
                open: Vec::new(),
            }],
            bindings: Arc::default(),
        }
    }

    #[test]
    fn a_strip_held_out_bends_as_its_bending_length_says() {
        // A strip held out level by one end comes to rest where, at each
        // of its points, the hinge there holds up the cloth beyond it:
        // a hinge turned by an angle gives the moment B sin(angle) / L,
        // B = weight x g x c^3 (Peirce), against weight x L x g per
        // point times its distance out. So c^3 sin(angle) / L^2 is the
        // sum of the distances.
        let material = Material::default();
        for spacing in [0.02, 0.03] {
            let free = (2.0 * material.bending_length / spacing).round() as usize;
            let setup = strip(free + 2, spacing, material);
            let mut state = State::new(setup.points.len());
            run(&setup, &mut state, &still(&setup, Begin::Reset, None));
            for _ in 0..600 {
                run(&setup, &mut state, &still(&setup, Begin::Keep, None));
            }
            let speed = state.vel.iter().map(|v| v.length()).fold(0.0, f32::max);
            assert!(speed < 0.001, "the strip came to rest: {speed}");
            let p = &state.pos;
            for j in 1..=free {
                let (before, after) = ((p[j] - p[j - 1]).normalize(), (p[j + 1] - p[j]).normalize());
                let held = material.bending_length.powi(3) * before.cross(after).length() / (spacing * spacing);
                let load: f32 = (j + 1..free + 2).map(|i| p[i].x - p[j].x).sum();
                // Within an eighth: the links that carry most are left
                // up to 3 % long by the averaging, which counts twice.
                assert!(
                    (held - load).abs() < 0.12 * load.abs().max(0.01),
                    "hinge {j}: {held} against {load}"
                );
            }
            // It hangs down, and keeps its length.
            assert!(p[free + 1].y < -spacing);
            for w in p.windows(2) {
                assert!(((w[1] - w[0]).length() / spacing - 1.0).abs() < 0.03);
            }
        }
    }

    #[test]
    fn a_strip_of_many_points_comes_to_rest() {
        // Finer than the substep carries the cloth's own stiffness: the
        // hinges are softened (`HINGE_LEAST`) and nothing swings up.
        let material = Material::default();
        for spacing in [0.01, 0.005] {
            let setup = strip(18, spacing, material);
            let mut state = State::new(setup.points.len());
            run(&setup, &mut state, &still(&setup, Begin::Reset, None));
            for _ in 0..600 {
                run(&setup, &mut state, &still(&setup, Begin::Keep, None));
            }
            let speed = state.vel.iter().map(|v| v.length()).fold(0.0, f32::max);
            assert!(speed < 0.001, "{spacing}: {speed}");
        }
    }

    #[test]
    fn hanging_cloth_keeps_its_length_and_comes_to_rest() {
        let setup = tube(12, 8, 0.1, 0.03, Vec3::new(0.0, -0.1, 0.0), Vec3::new(0.0, -0.3, 0.0));
        let mut state = State::new(setup.points.len());
        run(&setup, &mut state, &still(&setup, Begin::Reset, None));
        for _ in 0..120 {
            run(&setup, &mut state, &still(&setup, Begin::Keep, None));
        }
        let cols = 12;
        for c in 0..cols {
            let down = (state.pos[7 * cols + c] - state.pos[c]).length();
            let rest = setup.points[7 * cols + c].tether;
            assert!(down <= rest * 1.001 && down > rest * 0.97, "{down} of {rest}");
        }
        let speed = state.vel.iter().map(|v| v.length()).fold(0.0, f32::max);
        assert!(speed < 0.01, "{speed}");
    }

    #[test]
    fn cloth_lies_on_the_floor_and_stays() {
        // The floor 10 cm under the top of a tube 30 cm long.
        let setup = tube(12, 8, 0.1, 0.03, Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.2, 0.0));
        let floor = Some((Vec3::new(0.0, -0.1, 0.0), Vec3::Y));
        let mut state = State::new(setup.points.len());
        run(&setup, &mut state, &still(&setup, Begin::Reset, None));
        for _ in 0..240 {
            run(&setup, &mut state, &still(&setup, Begin::Keep, floor));
        }
        let low = state.pos.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        assert!(low >= -0.1 + setup.material.thickness - 1e-4, "{low}");
        let speed = state.vel.iter().map(|v| v.length()).sum::<f32>() / state.vel.len() as f32;
        assert!(speed < 0.01, "{speed}");
    }

    #[test]
    fn a_fast_limb_takes_the_cloth_with_it_and_does_not_come_through() {
        // A limb down the middle of the tube swings out sideways through
        // its wall at 4.5 m/s, as a running leg does: 7.5 cm a frame
        // against cloth 2.6 cm apart.
        let (a, b) = (Vec3::new(0.0, -0.05, 0.0), Vec3::new(0.0, -0.3, 0.0));
        let setup = tube(24, 10, 0.1, 0.04, a, b);
        let mut state = State::new(setup.points.len());
        let frames = sweep(
            &setup,
            Mat4::IDENTITY,
            Mat4::from_translation(Vec3::new(0.15, 0.0, 0.0)),
            2,
            None,
        );
        for frame in &frames {
            run(&setup, &mut state, frame);
        }
        // Every point within the limb's length is still on the side of
        // the limb it began on: none is inside it, and the ones it met
        // are ahead of it.
        let shell = setup.shells[0].in_world(&Mat4::from_translation(Vec3::new(0.15, 0.0, 0.0)));
        let mut ahead = 0;
        for (p, point) in state.pos.iter().zip(&setup.points) {
            if point.fixed {
                continue;
            }
            assert!(shell.depth(*p, 0.0) <= 1e-4, "{p} is in the limb");
            if point.rest.x > 0.05 && p.y < -0.08 && p.y > -0.28 && p.z.abs() < 0.02 {
                assert!(p.x > 0.15 + 0.04, "{p} was left behind the limb");
                ahead += 1;
            }
        }
        assert!(ahead > 0);
    }

    #[test]
    fn cloth_between_two_things_sits_between_them() {
        // A point between the floor and a limb pressed 2 cm into it: the
        // floor has the last word.
        let (a, b) = (Vec3::new(-0.3, 0.0, 0.0), Vec3::new(0.3, 0.0, 0.0));
        let mut setup = tube(8, 2, 0.05, 0.05, a, b);
        // One free point under the limb's middle, held by nothing.
        setup.links.iter_mut().for_each(|l| l.to = NONE);
        setup.hinges.iter_mut().for_each(|h| h.a = NONE);
        setup.points.iter_mut().for_each(|p| p.anchor = NONE);
        let floor = Some((Vec3::new(0.0, -0.03, 0.0), Vec3::Y));
        let mut state = State::new(setup.points.len());
        let mut frame = still(&setup, Begin::Reset, None);
        run(&setup, &mut state, &frame);
        frame.begin = Begin::Keep;
        frame.floor = floor;
        for i in 8..16 {
            state.pos[i] = Vec3::new(0.0, -0.04, 0.0);
            state.vel[i] = Vec3::ZERO;
        }
        for _ in 0..30 {
            run(&setup, &mut state, &frame);
        }
        let p = state.pos[8];
        assert!((p.y - (-0.03 + setup.material.thickness)).abs() < 1e-4, "{p}");
        assert!(state.vel[8].length() < 1e-3);
    }

    #[test]
    fn cloth_stays_on_a_slope_below_the_angle_of_friction() {
        // A free point on a floor tilted by less than atan(0.5) stays,
        // on a steeper one it slides.
        let mut setup = tube(8, 2, 0.05, 0.01, Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, 6.0, 0.0));
        setup.links.iter_mut().for_each(|l| l.to = NONE);
        setup.hinges.iter_mut().for_each(|h| h.a = NONE);
        setup.points.iter_mut().for_each(|p| p.anchor = NONE);
        let slid = |degrees: f32| {
            let up = Vec3::new(degrees.to_radians().sin(), degrees.to_radians().cos(), 0.0);
            let mut state = State::new(setup.points.len());
            let mut frame = still(&setup, Begin::Reset, None);
            run(&setup, &mut state, &frame);
            frame.begin = Begin::Keep;
            frame.floor = Some((Vec3::ZERO, up));
            state.pos[8] = up * setup.material.thickness;
            state.vel[8] = Vec3::ZERO;
            for _ in 0..30 {
                run(&setup, &mut state, &frame);
            }
            (state.pos[8] - up * setup.material.thickness).length()
        };
        assert!(slid(20.0) < 1e-4, "{}", slid(20.0));
        assert!(slid(35.0) > 0.01, "{}", slid(35.0));
    }

    #[test]
    fn the_map_holds_cloth_from_its_front_only_and_rounds_its_edges() {
        let tri = [
            [
                Vec3::new(-1.0, 0.0, -1.0),
                Vec3::new(-1.0, 0.0, 1.0),
                Vec3::new(1.0, 0.0, -1.0),
            ],
            [
                Vec3::new(1.0, 0.0, -1.0),
                Vec3::new(-1.0, 0.0, 1.0),
                Vec3::new(1.0, 0.0, 1.0),
            ],
        ];
        let mut out = [[0.0f32; 4]; MAP_PLANES];
        // Above it: its own plane, twice (one triangle's face, the
        // other's edge).
        map_planes(Vec3::new(-0.5, 0.02, -0.5), &tri, 0.07, 0.006, &mut out);
        assert_eq!(out[0], [0.0, 1.0, 0.0, 0.0]);
        // Under it: nothing.
        map_planes(Vec3::new(-0.5, -0.02, -0.5), &tri, 0.07, 0.006, &mut out);
        assert_eq!(out[0], [0.0; 4]);
        // Past its edge and a little above: a plane that leans out.
        map_planes(Vec3::new(1.03, 0.03, 0.0), &tri, 0.07, 0.006, &mut out);
        assert!(out[0][0] > 0.5 && out[0][1] > 0.5, "{:?}", out[0]);
    }

    #[test]
    fn a_frame_is_cut_into_substeps_of_the_same_time() {
        assert_eq!(substeps(1.0 / 60.0), 16);
        assert_eq!(substeps(1.0 / 120.0), 8);
        assert_eq!(substeps(1.0), MOST_SUBSTEPS);
        assert_eq!(substeps(0.0), 1);
    }
}
