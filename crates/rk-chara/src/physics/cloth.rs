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
//! - a floor at the character's root, where there is no map;
//! - what the character is among (`Obstacles`): a map's floors, seats,
//!   beds and desks, as the triangles that are drawn. A segment of a
//!   chain, from the waist down, does not go into a surface from its
//!   front, the side it is drawn from, so a dress rests on a chair seat
//!   or a bed and slides along it instead of passing through. It may
//!   come out through the back of one, as the skirt of a character sunk
//!   into a mattress does.
//!
//! Every constraint only pushes out, so a skirt that touches nothing stays
//! where the game's step put it.

use super::{keep_length, position, world, Bone, Particle};
use crate::scene::{Part, Scene, Skinning};
use glam::{Mat4, Vec3};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Rounds of the constraints per frame.
const ITERATIONS: u32 = 8;
/// Places along each limb bone where the body mesh's thickness is taken;
/// a limb bone gets one capsule between each two.
const STATIONS: usize = 4;

/// How far off a surface a segment that met it is put.
const SURFACE_GAP: f32 = 0.01;
/// Rounds of the surface test per frame: a segment put back off one
/// surface can meet another.
const SURFACE_ROUNDS: usize = 3;
/// How much further than a skirt reaches its nearby triangles are
/// gathered, so that they serve while it moves a little.
const GATHER_MARGIN: f32 = 0.3;
/// A triangle of `Obstacles` in the world, with the box around it and
/// the way its front faces: the side it is drawn from (the game's
/// meshes wind clockwise seen from there), which is the outside of the
/// thing it is part of.
#[derive(Debug, Clone, Copy)]
pub(super) struct Triangle {
    pub(super) corners: [Vec3; 3],
    front: Vec3,
    lo: Vec3,
    hi: Vec3,
}

impl Triangle {
    pub(super) fn new(corners: [Vec3; 3]) -> Self {
        let [a, b, c] = corners;
        Self {
            corners,
            front: (b - a).cross(c - a).normalize_or_zero(),
            lo: a.min(b).min(c),
            hi: a.max(b).max(c),
        }
    }
}

/// The side of a box of `Surfaces`.
const SURFACE_BOX: f32 = 0.05;

/// Some boxes of `Surfaces`: the first and the last along each axis.
type Span = ([usize; 3], [usize; 3]);

/// Calls `f` with the number of each box of `span`.
#[inline]
fn boxes(span: &Span, dims: [usize; 3], mut f: impl FnMut(usize)) {
    let ([x0, y0, z0], [x1, y1, z1]) = *span;
    for z in z0..=z1 {
        for y in y0..=y1 {
            for x in x0..=x1 {
                f((z * dims[1] + y) * dims[0] + x);
            }
        }
    }
}

/// Triangles sorted into boxes `SURFACE_BOX` wide, so that a segment is
/// tested against the ones near it only.
#[derive(Debug, Clone, Default)]
pub(super) struct Surfaces {
    triangles: Vec<Triangle>,
    lo: Vec3,
    dims: [usize; 3],
    /// Per box: where its triangles start in `held`, and one more.
    first: Vec<u32>,
    held: Vec<u32>,
}

impl Surfaces {
    /// The triangles as far as they are in the box from `lo` to `hi`.
    pub(super) fn new(triangles: Vec<Triangle>, lo: Vec3, hi: Vec3) -> Self {
        let d = ((hi - lo).max(Vec3::ZERO) / SURFACE_BOX).ceil().max(Vec3::ONE);
        let dims = [d.x as usize, d.y as usize, d.z as usize];
        let mut out = Surfaces {
            triangles,
            lo,
            dims,
            first: Vec::new(),
            held: Vec::new(),
        };
        let spans: Vec<Option<Span>> = out.triangles.iter().map(|t| out.span(t.lo, t.hi)).collect();
        let mut first = vec![0u32; dims[0] * dims[1] * dims[2] + 1];
        for span in spans.iter().flatten() {
            boxes(span, dims, |b| first[b + 1] += 1);
        }
        for b in 1..first.len() {
            first[b] += first[b - 1];
        }
        let mut filled = first.clone();
        let mut held = vec![0u32; first[first.len() - 1] as usize];
        for (t, span) in spans.iter().enumerate() {
            let Some(span) = span else { continue };
            boxes(span, dims, |b| {
                held[filled[b] as usize] = t as u32;
                filled[b] += 1;
            });
        }
        out.first = first;
        out.held = held;
        out
    }

    /// The corners of every triangle.
    pub(super) fn corners(&self) -> Vec<[Vec3; 3]> {
        self.triangles.iter().map(|t| t.corners).collect()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    /// The boxes that the box from `lo` to `hi` touches, if any.
    fn span(&self, lo: Vec3, hi: Vec3) -> Option<Span> {
        let top = Vec3::new(self.dims[0] as f32, self.dims[1] as f32, self.dims[2] as f32);
        let a = ((lo - self.lo) / SURFACE_BOX).floor();
        let b = ((hi - self.lo) / SURFACE_BOX).floor();
        if b.cmplt(Vec3::ZERO).any() || a.cmpge(top).any() {
            return None;
        }
        let (a, b) = (a.max(Vec3::ZERO), b.min(top - 1.0));
        Some((
            [a.x as usize, a.y as usize, a.z as usize],
            [b.x as usize, b.y as usize, b.z as usize],
        ))
    }

    /// Calls `f` with every triangle in the boxes that the box from
    /// `lo` to `hi` touches; one that lies in several of them comes
    /// more than once.
    fn near(&self, lo: Vec3, hi: Vec3, mut f: impl FnMut(&Triangle)) {
        let Some(span) = self.span(lo, hi) else { return };
        boxes(&span, self.dims, |b| {
            for &t in &self.held[self.first[b] as usize..self.first[b + 1] as usize] {
                f(&self.triangles[t as usize]);
            }
        });
    }
}

/// The triangles near one skirt, kept while the skirt stays in the box
/// they were gathered for and the obstacles are the same.
#[derive(Debug, Clone, Default)]
pub(super) struct Nearby {
    from: usize,
    lo: Vec3,
    hi: Vec3,
    pub(super) surfaces: Arc<Surfaces>,
}

impl Nearby {
    /// Gathers the triangles of `obstacles` again when the box from `lo`
    /// to `hi` left the one they were gathered for, or they changed.
    pub(super) fn refresh(&mut self, obstacles: Option<&Arc<Obstacles>>, lo: Vec3, hi: Vec3) {
        let Some(obstacles) = obstacles else {
            *self = Nearby::default();
            return;
        };
        let from = Arc::as_ptr(obstacles) as usize;
        let inside = self.lo.cmple(lo).all() && self.hi.cmpge(hi).all();
        if self.from != from || !inside {
            let grow = Vec3::splat(GATHER_MARGIN);
            let (lo, hi) = (lo - grow, hi + grow);
            let mut triangles = Vec::new();
            obstacles.gather(lo, hi, &mut triangles);
            *self = Nearby {
                from,
                lo,
                hi,
                surfaces: Arc::new(Surfaces::new(triangles, lo, hi)),
            };
        }
    }
}

/// The triangles around a character that its skirts rest on, other than
/// its own body: a map's floors and furniture.
#[derive(Debug, Clone, Default)]
pub struct Obstacles {
    pub pieces: Vec<Obstacle>,
}

/// One mesh of `Obstacles`, in its own space.
#[derive(Debug, Clone)]
pub struct Obstacle {
    pub positions: Arc<Vec<[f32; 3]>>,
    pub indices: Arc<Vec<u32>>,
    /// The box around `positions`.
    pub bounds: ([f32; 3], [f32; 3]),
    /// Where the mesh is in the world.
    pub world: Mat4,
}

impl Obstacles {
    /// The triangles, in the world, of every mesh that may reach into the
    /// box from `lo` to `hi`.
    pub(super) fn gather(&self, lo: Vec3, hi: Vec3, out: &mut Vec<Triangle>) {
        for piece in &self.pieces {
            if piece.world.determinant().abs() < 1e-12 {
                continue;
            }
            // The world box as a box in the piece's space.
            let back = piece.world.inverse();
            // A mirrored piece winds the other way round.
            let mirrored = piece.world.determinant() < 0.0;
            let (mut llo, mut lhi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for k in 0..8 {
                let corner = Vec3::new(
                    if k & 1 == 0 { lo.x } else { hi.x },
                    if k & 2 == 0 { lo.y } else { hi.y },
                    if k & 4 == 0 { lo.z } else { hi.z },
                );
                let p = back.transform_point3(corner);
                llo = llo.min(p);
                lhi = lhi.max(p);
            }
            let (blo, bhi) = (Vec3::from(piece.bounds.0), Vec3::from(piece.bounds.1));
            if !(blo.cmple(lhi).all() && bhi.cmpge(llo).all()) {
                continue;
            }
            for t in piece.indices.chunks_exact(3) {
                let corner = |i: usize| piece.positions.get(t[i] as usize).copied().map(Vec3::from);
                let (Some(a), Some(b), Some(c)) = (corner(0), corner(1), corner(2)) else {
                    continue;
                };
                let (tlo, thi) = (a.min(b).min(c), a.max(b).max(c));
                if tlo.cmple(lhi).all() && thi.cmpge(llo).all() {
                    let w = |p: Vec3| piece.world.transform_point3(p);
                    out.push(Triangle::new(if mirrored {
                        [w(a), w(c), w(b)]
                    } else {
                        [w(a), w(b), w(c)]
                    }));
                }
            }
        }
    }
}

/// Where the segment from `p` to `q` passes through the triangle, as a
/// share of the way from `p` (Moeller and Trumbore), both faces counting.
fn segment_meets(p: Vec3, q: Vec3, [a, b, c]: &[Vec3; 3]) -> Option<f32> {
    let dir = q - p;
    let (e1, e2) = (*b - *a, *c - *a);
    let h = dir.cross(e2);
    let det = e1.dot(h);
    if det.abs() < 1e-12 {
        return None;
    }
    let f = 1.0 / det;
    let s = p - *a;
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qv = s.cross(e1);
    let v = f * dir.dot(qv);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = f * e2.dot(qv);
    (0.0..=1.0).contains(&t).then_some(t)
}

/// Keeps each segment of a chain, from the root down, out of the things
/// around it: a lower end that went into a surface from its front is put
/// back onto it, a little off, where it went in. A segment that comes
/// out through the back of a surface is left alone: a character sunk
/// into a mattress has the top of its skirt inside it, and the cloth
/// that comes out of it with the body then rests on it like the rest.
pub(super) fn rest_on(chain: &mut [Vec3], surfaces: &Surfaces) {
    for i in 1..chain.len() {
        for _ in 0..SURFACE_ROUNDS {
            let (p, q) = (chain[i - 1], chain[i]);
            let (lo, hi) = (p.min(q), p.max(q));
            let mut first: Option<(f32, Vec3)> = None;
            surfaces.near(lo, hi, |tri| {
                if tri.lo.cmple(hi).all() && tri.hi.cmpge(lo).all() && tri.front.dot(q - p) < 0.0 {
                    if let Some(t) = segment_meets(p, q, &tri.corners) {
                        if first.is_none_or(|(best, _)| t < best) {
                            first = Some((t, tri.front));
                        }
                    }
                }
            });
            let Some((t, n)) = first else {
                break;
            };
            let hit = p + (q - p) * t;
            let depth = (q - hit).dot(n);
            chain[i] = q + n * (SURFACE_GAP - depth);
        }
    }
}

/// A capsule with a radius at each end, in the space of `node`.
#[derive(Debug, Clone)]
pub struct Capsule {
    pub node: usize,
    pub a: Vec3,
    pub b: Vec3,
    /// The limb's mean thickness at each end, which the chains collide
    /// with.
    pub ra: f32,
    pub rb: f32,
    /// The thickness that nine in ten of the limb's vertices stay within,
    /// which skirt vertices are pushed out of.
    pub fa: f32,
    pub fb: f32,
    /// Whether the skirt's chains collide with it, or only its vertices.
    pub chains: bool,
    /// Whether the cloth net collides with it: everything but the hands.
    pub net: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct WorldCapsule {
    pub(super) a: Vec3,
    pub(super) b: Vec3,
    pub(super) ra: f32,
    pub(super) rb: f32,
}

impl WorldCapsule {
    /// The box around the capsule.
    pub(super) fn bounds(&self) -> (Vec3, Vec3) {
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
    /// The obstacles' triangles near each ring.
    nearby: Vec<Nearby>,
    /// The skirts' moving bones, by node, for the push-out of their
    /// vertices.
    skirt: Arc<Vec<bool>>,
    /// The cloth nets, and the ring each was built from.
    nets: Vec<super::net::Net>,
    net_rings: Vec<usize>,
    /// The garments' vertices bound to the nets.
    bindings: Arc<Vec<super::net::Binding>>,
    /// The body the nets and their vertices collide with, as fitted and
    /// as it is in the world at the last step.
    pub shells: Vec<super::shell::Shell>,
    world_shells: Arc<Vec<super::shell::WorldShell>>,
    /// The nets as the GPU solver runs them (`drape`), and whether it
    /// does: the nets above then stand by.
    drape: Option<super::drape::Drape>,
    on_gpu: bool,
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
        // The bones below the top one of each chain: the waistband sits
        // on the hips, where the limbs' fit is too thick to push from.
        let mut skirt = vec![false; scene.nodes.len()];
        for &b in rings.iter().flatten() {
            for p in particles(&bones[b]).unwrap_or(&[]).iter().skip(2) {
                if let Some(n) = p.node {
                    skirt[n] = true;
                }
            }
        }
        // The cloth nets.
        let mut nets = Vec::new();
        let mut net_rings = Vec::new();
        for (i, ring) in rings.iter().enumerate() {
            if let Some(net) = super::net::Net::build(scene, bones, ring) {
                nets.push(net);
                net_rings.push(i);
            }
        }
        let bindings = Arc::new(super::net::bind(scene, &mut nets));
        let shells = if nets.is_empty() {
            Vec::new()
        } else {
            super::shell::fit(scene, names)
        };
        for net in &mut nets {
            net.fitted(scene, &shells);
        }
        let drape = super::drape::Drape::build(scene, &nets, &shells, &bindings, Default::default());
        Self {
            drape,
            on_gpu: false,
            capsules,
            shells,
            world_shells: Arc::default(),
            floor: names.get("cf_j_root").copied(),
            rest: Vec::new(),
            nearby: Vec::new(),
            skirt: Arc::new(skirt),
            rings,
            nets,
            net_rings,
            bindings,
        }
    }

    /// Moves the cloth nets on by `dt` seconds after the chains moved;
    /// `live` says which bones ran.
    pub(super) fn step_nets(&mut self, scene: &Scene, live: &[bool], obstacles: Option<&Arc<Obstacles>>, dt: f32) {
        if self.nets.is_empty() {
            return;
        }
        // The body in the world.
        self.world_shells = Arc::new(self.shells.iter().map(|s| s.in_world(&world(scene, s.node))).collect());
        let mapped = obstacles.is_some_and(|o| !o.pieces.is_empty());
        let floor = self.floor.filter(|_| !mapped).map(|n| {
            let m = world(scene, n);
            (m.w_axis.truncate(), m.y_axis.truncate().normalize_or_zero())
        });
        if let Some(drape) = self.drape.as_mut().filter(|_| self.on_gpu) {
            let active = self
                .net_rings
                .iter()
                .all(|&ring| self.rings[ring].iter().all(|&b| live.get(b).copied().unwrap_or(false)));
            drape.step(scene, active, floor, obstacles, dt);
            return;
        }
        for (net, &ring) in self.nets.iter_mut().zip(&self.net_rings) {
            let active = self.rings[ring].iter().all(|&b| live.get(b).copied().unwrap_or(false));
            net.step(scene, active, &self.shells, &self.world_shells, floor, obstacles, dt);
        }
    }

    /// Has the GPU solver run the nets (`drape`), or the CPU's own.
    pub(super) fn solve_on_gpu(&mut self, on: bool) {
        if self.on_gpu != on {
            self.on_gpu = on;
            self.reset_nets();
        }
    }

    /// The cloth for the GPU solver, once it has a frame to run.
    pub(super) fn drape(&self) -> Option<Arc<super::drape::Live>> {
        self.drape
            .as_ref()
            .filter(|d| self.on_gpu && d.ready())
            .map(|d| d.live.clone())
    }

    /// The fastest a part of the body went in a frame as the GPU solver
    /// saw it, in metres.
    pub(super) fn fastest_limb(&self) -> f32 {
        self.drape.as_ref().map_or(0.0, |d| d.fastest)
    }

    /// One text per net for `rk render --skirt-dump`: the net's points,
    /// its rest shape on the waist, speeds, pressed and open flags and
    /// the body's shells, as the walking score reads them. `gpu` is the
    /// GPU solver's state read back (places with pressed, speeds).
    pub(super) fn dump(&self, scene: &Scene, gpu: Option<super::drape::Readback<'_>>) -> Vec<String> {
        use std::fmt::Write as _;
        let bodies: Vec<_> = self.shells.iter().map(|s| s.in_world(&world(scene, s.node))).collect();
        let mut body = String::new();
        for (s, b) in self.shells.iter().zip(&bodies) {
            let points = b.surface();
            let m = world(scene, s.node);
            let (a, e) = (m.transform_point3(s.a), m.transform_point3(s.b));
            let _ = writeln!(
                body,
                "s {} {} {} {} {} {} {} {} {} {}",
                s.node,
                s.outside as u8,
                s.stations,
                points.len(),
                a.x,
                a.y,
                a.z,
                e.x,
                e.y,
                e.z
            );
            for p in points {
                let _ = writeln!(body, "{} {} {}", p.x, p.y, p.z);
            }
        }
        let mut out = Vec::new();
        for (k, net) in self.nets.iter().enumerate() {
            let part = net.part();
            let waist = world(scene, part.waist);
            let count = part.cols * part.rows;
            let rest: Vec<Vec3> = part.rest_on_waist.iter().map(|r| waist.transform_point3(*r)).collect();
            let (pos, vel, pressed): (Vec<Vec3>, Vec<Vec3>, Vec<f32>) = match (gpu, &self.drape) {
                (Some((places, speeds)), Some(drape)) if self.on_gpu => {
                    let first = drape.live.setup.nets[k].first;
                    let cut = |all: &[[f32; 4]]| -> Vec<Vec3> {
                        all[first..first + count]
                            .iter()
                            .map(|p| Vec3::new(p[0], p[1], p[2]))
                            .collect()
                    };
                    (
                        cut(places),
                        cut(speeds),
                        places[first..first + count].iter().map(|p| p[3]).collect(),
                    )
                }
                _ => net.state(),
            };
            let mut text = String::new();
            let _ = writeln!(text, "{} {} {} {}", part.cols, part.rows, part.fixed, bodies.len());
            for set in [&pos, &rest, &vel] {
                for p in set.iter() {
                    let _ = writeln!(text, "{} {} {}", p.x, p.y, p.z);
                }
            }
            for (i, p) in pressed.iter().enumerate() {
                let _ = writeln!(text, "{} {}", p, part.open.get(i).copied().unwrap_or(0));
            }
            text.push_str(&body);
            out.push(text);
        }
        out
    }

    /// One line per skirt: its chains, its net and the meshes bound to
    /// the net, for the command line to print.
    pub(super) fn summary(&self, scene: &Scene, bones: &[Bone]) -> Vec<String> {
        self.rings
            .iter()
            .enumerate()
            .map(|(i, ring)| {
                let longest = ring
                    .iter()
                    .filter_map(|&b| particles(&bones[b]))
                    .map(<[Particle]>::len)
                    .max()
                    .unwrap_or(0);
                let net = self.net_rings.iter().position(|&r| r == i);
                let grid = net.map_or("no net".to_string(), |k| {
                    let (cols, rows) = self.nets[k].size();
                    format!("net {cols}x{rows}")
                });
                let bound: Vec<String> = self
                    .bindings
                    .iter()
                    .filter_map(|b| {
                        let count = b.vertices.iter().filter(|v| Some(v.net as usize) == net).count();
                        (count > 0).then(|| format!("{} ({count})", scene.renderers[b.renderer].name))
                    })
                    .collect();
                format!(
                    "{} chains of up to {longest} particles, {grid}, bound: {}",
                    ring.len(),
                    if bound.is_empty() {
                        "nothing".to_string()
                    } else {
                        bound.join(", ")
                    }
                )
            })
            .collect()
    }

    /// Has `solver` run the nets' steps; none for the CPU's own.
    pub(super) fn solve_with(&mut self, solver: Option<super::net::SharedSolver>) {
        for net in &mut self.nets {
            net.solve_with(solver.clone());
        }
    }

    /// Whether any skirt has a cloth net.
    pub(super) fn has_nets(&self) -> bool {
        !self.nets.is_empty()
    }

    /// The nets for drawing, when there are any.
    pub(super) fn draw(&self) -> Option<super::net::ClothDraw> {
        (!self.nets.is_empty() && !self.on_gpu)
            .then(|| super::net::ClothDraw::from_nets(&self.nets, &self.bindings, &self.world_shells))
    }

    /// Starts the nets again on their targets.
    pub(super) fn reset_nets(&mut self) {
        self.nets.iter_mut().for_each(super::net::Net::reset);
        if let Some(drape) = &mut self.drape {
            drape.reset();
        }
    }

    /// Moves the nets with the character.
    pub(super) fn carry(&mut self, moved: Mat4) {
        for net in &mut self.nets {
            net.carry(moved);
        }
        if let Some(drape) = &mut self.drape {
            drape.carry(moved);
        }
    }

    /// What the skirts' vertices are pushed out of after skinning: the
    /// limbs, and the floor at the root where there is no map.
    pub(super) fn push_out(&self, mapped: bool) -> Option<crate::scene::PushOut> {
        if self.rings.is_empty() {
            return None;
        }
        Some(crate::scene::PushOut {
            capsules: self.capsules.iter().map(|c| (c.node, c.a, c.b, c.fa, c.fb)).collect(),
            floor: self.floor.filter(|_| !mapped),
            skirt: self.skirt.clone(),
        })
    }

    fn world_capsules(&self, scene: &Scene) -> Vec<WorldCapsule> {
        self.capsules
            .iter()
            .filter(|c| c.chains)
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

    /// Pushes the particles the bones step left out of the limbs, the
    /// floor and the `obstacles`, then poses the bones again. `live` says
    /// which bones ran.
    pub(super) fn solve(
        &mut self,
        scene: &mut Scene,
        bones: &mut [Bone],
        live: &[bool],
        obstacles: Option<&std::sync::Arc<Obstacles>>,
    ) {
        if self.rings.is_empty() {
            return;
        }
        let all = self.world_capsules(scene);
        // A map brings its own floors; the plane at the root is for
        // where there is none.
        let mapped = obstacles.is_some_and(|o| !o.pieces.is_empty());
        let floor = self.floor.filter(|_| !mapped).map(|n| {
            let m = world(scene, n);
            (m.w_axis.truncate(), m.y_axis.truncate().normalize_or_zero())
        });
        self.nearby.resize(self.rings.len(), Nearby::default());
        for ((ring, rest), nearby) in self.rings.iter().zip(&self.rest).zip(&mut self.nearby) {
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
            nearby.refresh(obstacles, lo - Vec3::splat(margin), hi + Vec3::splat(margin));
            let surfaces = &nearby.surfaces;
            if capsules.is_empty() && floor.is_none() && surfaces.is_empty() {
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
            if !surfaces.is_empty() {
                for chain in &mut pos {
                    rest_on(chain, surfaces);
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

pub(super) fn point_capsule(p: &mut Vec3, cap: &WorldCapsule) {
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

/// The body parts that get capsules: the joint whose subtree holds the
/// part's vertices, the bone from one joint to the next (the same joint
/// twice for a ball around it), how many places along it the thickness is
/// taken at, and whether skirt vertices are pushed out of the thickness
/// nine in ten of its vertices stay within or of its mean thickness. Only
/// the buttocks take the thicker one: on the legs it reaches past the
/// thighs and flattens the pleats of a standing skirt. Both sides of each
/// fit the shader's `scene::PUSH_CAPSULES`. The hands come last, so that a
/// hand resting on the lap keeps the skirt between it and the thigh, and
/// they push only the vertices (`VERTICES_ONLY`): a hand on the lap would
/// otherwise hold a whole panel of the skirt up.
const LIMBS: [(&str, &str, &str, usize, bool); 7] = [
    ("cf_d_siri", "cf_j_siri", "cf_j_siri", 1, true),
    ("cf_j_thigh00", "cf_j_thigh00", "cf_j_leg01", STATIONS, false),
    ("cf_j_thigh00", "cf_j_leg01", "cf_j_leg03", STATIONS, false),
    ("cf_j_thigh00", "cf_j_leg03", "cf_j_toes", 2, false),
    ("cf_j_arm00", "cf_j_arm00", "cf_j_forearm01", STATIONS, false),
    ("cf_j_arm00", "cf_j_forearm01", "cf_j_hand", STATIONS, false),
    ("cf_j_arm00", "cf_j_hand", "cf_j_middle03", 2, false),
];
/// The joints of `LIMBS` whose capsules push only the skirt's vertices:
/// the hands, and the buttocks, which the game's own colliders keep the
/// chains off already.
const VERTICES_ONLY: [&str; 2] = ["cf_j_hand", "cf_j_siri"];

/// Capsules along the buttocks, legs, feet, arms and hands, with the
/// thickness the body mesh has there: the distances from the bone line
/// of the part's vertices near each place along it.
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
    // The vertices of a part, by the joint whose subtree holds it.
    let mut part_verts: HashMap<usize, Vec<Vec3>> = HashMap::new();
    let mut out = Vec::new();
    for (root, from, to, stations, thick) in LIMBS {
        for side in ["L", "R"] {
            let joint = |n: &str| names.get(&format!("{n}_{side}")).copied();
            let (Some(root), Some(from), Some(to)) = (joint(root), joint(from), joint(to)) else {
                continue;
            };
            let verts = part_verts.entry(root).or_insert_with(|| {
                let part: HashSet<usize> = scene.subtree(root).into_iter().collect();
                let of_part: Vec<bool> = body
                    .renderer
                    .bones
                    .iter()
                    .map(|b| b.is_some_and(|n| part.contains(&n)))
                    .collect();
                body.renderer
                    .mesh
                    .skin
                    .iter()
                    .zip(&positions)
                    .filter(|(bw, _)| {
                        let w: f32 = (0..4)
                            .filter(|&k| of_part.get(bw.bones[k] as usize).copied().unwrap_or(false))
                            .map(|k| bw.weights[k])
                            .sum();
                        w > 0.5
                    })
                    .map(|(_, p)| Vec3::from(*p))
                    .collect()
            });
            let mut fitted = if from == to {
                fit_ball(scene, verts, from)
            } else {
                fit_bone(scene, verts, from, to, stations)
            };
            for c in &mut fitted {
                if !thick {
                    (c.fa, c.fb) = (c.ra, c.rb);
                }
                c.chains = !VERTICES_ONLY.iter().any(|j| scene.nodes[from].name.starts_with(j));
                c.net = !scene.nodes[from].name.starts_with("cf_j_hand");
            }
            out.extend(fitted);
        }
    }
    out
}

/// A ball around the joint `at`, as thick as its vertices are from it.
fn fit_ball(scene: &Scene, verts: &[Vec3], at: usize) -> Vec<Capsule> {
    let c = position(scene, at);
    let mut far: Vec<f32> = verts.iter().map(|v| (*v - c).length()).collect();
    if far.is_empty() {
        return Vec::new();
    }
    far.sort_by(f32::total_cmp);
    let mean = far.iter().sum::<f32>() / far.len() as f32;
    let tenth = far[(far.len() - 1) * 9 / 10];
    let m = world(scene, at);
    let scale = m.x_axis.truncate().length().max(f32::EPSILON);
    let local = m.inverse().transform_point3(c);
    vec![Capsule {
        node: at,
        a: local,
        b: local,
        ra: mean / scale,
        rb: mean / scale,
        fa: tenth / scale,
        fb: tenth / scale,
        chains: true,
        net: true,
    }]
}

/// The capsules of one limb bone from `from` to `to`.
fn fit_bone(scene: &Scene, verts: &[Vec3], from: usize, to: usize, stations: usize) -> Vec<Capsule> {
    let (a, b) = (position(scene, from), position(scene, to));
    let axis = b - a;
    let len2 = axis.length_squared();
    if len2 <= 0.0 {
        return Vec::new();
    }
    let half = 0.5 / (stations - 1) as f32;
    // The mean and the ninth tenth of the distances near each station.
    let radii: Vec<(f32, f32)> = (0..stations)
        .map(|k| {
            let at = k as f32 / (stations - 1) as f32;
            let mut near: Vec<f32> = verts
                .iter()
                .filter_map(|v| {
                    let t = (*v - a).dot(axis) / len2;
                    ((0.0..=1.0).contains(&t) && (t - at).abs() <= half).then(|| (*v - (a + axis * t)).length())
                })
                .collect();
            if near.is_empty() {
                return (0.0, 0.0);
            }
            near.sort_by(f32::total_cmp);
            let mean = near.iter().sum::<f32>() / near.len() as f32;
            (mean, near[(near.len() - 1) * 9 / 10])
        })
        .collect();
    let m = world(scene, from);
    let inv = m.inverse();
    let scale = m.x_axis.truncate().length().max(f32::EPSILON);
    (0..stations - 1)
        .filter(|&k| radii[k].0 > 0.0 && radii[k + 1].0 > 0.0)
        .map(|k| {
            let t0 = k as f32 / (stations - 1) as f32;
            let t1 = (k + 1) as f32 / (stations - 1) as f32;
            Capsule {
                node: from,
                a: inv.transform_point3(a + axis * t0),
                b: inv.transform_point3(a + axis * t1),
                ra: radii[k].0 / scale,
                rb: radii[k + 1].0 / scale,
                fa: radii[k].1 / scale,
                fb: radii[k + 1].1 / scale,
                chains: true,
                net: true,
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

    /// A square seat at height 0.5, one metre across.
    fn seat() -> Surfaces {
        let (a, b, c, d) = (
            Vec3::new(-0.5, 0.5, -0.5),
            Vec3::new(0.5, 0.5, -0.5),
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::new(-0.5, 0.5, 0.5),
        );
        // Wound to face up.
        Surfaces::new(
            vec![Triangle::new([a, c, b]), Triangle::new([a, d, c])],
            Vec3::splat(-1.0),
            Vec3::splat(1.0),
        )
    }

    #[test]
    fn surfaces_give_a_segment_the_triangles_near_it() {
        // A big seat and a small triangle far off to one side.
        let far = Triangle::new([
            Vec3::new(0.80, 0.9, 0.80),
            Vec3::new(0.85, 0.9, 0.80),
            Vec3::new(0.80, 0.9, 0.85),
        ]);
        let mut triangles = seat().triangles;
        triangles.push(far);
        let all = Surfaces::new(triangles, Vec3::splat(-1.0), Vec3::splat(1.0));
        let count = |lo: Vec3, hi: Vec3| {
            let mut seen = Vec::new();
            all.near(lo, hi, |t| seen.push(t.corners[0]));
            seen.dedup();
            seen.len()
        };
        // Anywhere over the seat: its two triangles, not the far one.
        assert_eq!(count(Vec3::new(0.3, 0.45, -0.4), Vec3::new(0.32, 0.55, -0.38)), 1);
        assert_eq!(count(Vec3::new(0.82, 0.88, 0.82), Vec3::new(0.83, 0.92, 0.83)), 1);
        // High over everything, and outside the boxes: nothing.
        assert_eq!(count(Vec3::new(0.0, 0.7, 0.0), Vec3::new(0.05, 0.75, 0.05)), 0);
        assert_eq!(count(Vec3::splat(3.0), Vec3::splat(4.0)), 0);
    }

    #[test]
    fn a_chain_that_starts_inside_a_thing_comes_out_of_it_and_rests_on_it() {
        // The top of the chain is under the seat's surface, as the waist
        // of a character sunk into a mattress: it goes up through the
        // surface from behind, and down onto it again further on.
        let mut chain = vec![
            Vec3::new(0.0, 0.4, 0.0),
            Vec3::new(0.1, 0.6, 0.0),
            Vec3::new(0.2, 0.6, 0.0),
            Vec3::new(0.3, 0.4, 0.0),
        ];
        rest_on(&mut chain, &seat());
        assert_eq!(chain[1], Vec3::new(0.1, 0.6, 0.0));
        assert_eq!(chain[2], Vec3::new(0.2, 0.6, 0.0));
        assert!((chain[3].y - 0.5 - SURFACE_GAP).abs() < 1e-5, "{:?}", chain[3]);
    }

    #[test]
    fn a_chain_through_a_seat_rests_on_it() {
        let mut chain: Vec<Vec3> = (0..4)
            .map(|i| Vec3::new(0.1 * i as f32, 0.7 - 0.1 * i as f32, 0.0))
            .collect();
        rest_on(&mut chain, &seat());
        for p in &chain {
            assert!(p.y >= 0.5, "{p:?}");
        }
        // It slid along the seat instead of stopping where it met it.
        assert!((chain[3].x - 0.3).abs() < 1e-5, "{:?}", chain[3]);
        assert!((chain[3].y - 0.5 - SURFACE_GAP).abs() < 1e-5);
    }

    #[test]
    fn a_chain_beside_a_seat_hangs_free() {
        let mut chain: Vec<Vec3> = (0..4).map(|i| Vec3::new(0.7, 0.7 - 0.1 * i as f32, 0.0)).collect();
        let was = chain.clone();
        rest_on(&mut chain, &seat());
        assert_eq!(chain, was);
    }

    #[test]
    fn obstacles_give_the_triangles_near_a_box_in_the_world() {
        let o = Obstacles {
            pieces: vec![Obstacle {
                positions: Arc::new(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]),
                indices: Arc::new(vec![0, 1, 2]),
                bounds: ([0.0, 0.0, 0.0], [1.0, 0.0, 1.0]),
                world: Mat4::from_translation(Vec3::new(0.0, 0.5, 0.0)),
            }],
        };
        let mut out = Vec::new();
        o.gather(Vec3::new(0.2, 0.4, 0.2), Vec3::new(0.3, 0.6, 0.3), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].corners[1], Vec3::new(1.0, 0.5, 0.0));
        out.clear();
        o.gather(Vec3::new(0.2, 0.0, 0.2), Vec3::new(0.3, 0.2, 0.3), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn a_mirrored_piece_still_faces_the_way_it_is_drawn() {
        // A triangle that faces up, in a piece mirrored from left to
        // right: its corners then go round the other way, and its front
        // is still up.
        let piece = |world: Mat4| Obstacles {
            pieces: vec![Obstacle {
                positions: Arc::new(vec![[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]]),
                indices: Arc::new(vec![0, 1, 2]),
                bounds: ([0.0, 0.0, 0.0], [1.0, 0.0, 1.0]),
                world,
            }],
        };
        for world in [Mat4::IDENTITY, Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0))] {
            let mut out = Vec::new();
            piece(world).gather(Vec3::splat(-2.0), Vec3::splat(2.0), &mut out);
            assert_eq!(out.len(), 1);
            assert!(out[0].front.y > 0.99, "{:?}", out[0].front);
        }
    }

    #[test]
    fn a_skirt_vertex_is_pushed_out_by_its_share() {
        use crate::scene::{PushDraw, PUSH_GAP};
        use glam::Vec4;
        let push = PushDraw {
            capsules: vec![[Vec4::new(0.0, 0.0, 0.0, 0.1), Vec4::new(1.0, 0.0, 0.0, 0.1)]],
            floor: Some(Vec4::new(0.0, 1.0, 0.0, -1.0)),
            skirt_slots: vec![false, true],
        };
        // Weighted half on a skirt bone: half way out of the capsule.
        let share = push.share([0, 1, 0, 0], [0.5, 0.5, 0.0, 0.0]);
        assert_eq!(share, 0.5);
        let p = push.apply(Vec3::new(0.5, 0.05, 0.0), share);
        assert!((p.y - (0.05 + 0.5 * (0.1 + PUSH_GAP - 0.05))).abs() < 1e-5, "{p:?}");
        // All on the skirt: onto the surface; none: left alone.
        let p = push.apply(Vec3::new(0.5, 0.05, 0.0), 1.0);
        assert!((p.y - (0.1 + PUSH_GAP)).abs() < 1e-5, "{p:?}");
        assert_eq!(push.apply(Vec3::new(0.5, 0.05, 0.0), 0.0), Vec3::new(0.5, 0.05, 0.0));
        // Under the floor at height -1: back above it.
        let p = push.apply(Vec3::new(3.0, -1.2, 0.0), 1.0);
        assert!((p.y - (-1.0 + PUSH_GAP)).abs() < 1e-5, "{p:?}");
    }

    #[test]
    fn skirt_chain_numbers_come_from_the_bone_name() {
        assert_eq!(skirt_chain("cf_j_sk_03_00"), Some(3));
        assert_eq!(skirt_chain("cf_d_sk_07_00"), Some(7));
        assert_eq!(skirt_chain("cf_j_hairB_00"), None);
    }
}
