// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Advanced Skirt Physics' cloth net: an invisible grid of points per
//! skirt that every layer of the garment follows, so that a long dress can
//! fold and drape where the game's few stiff chains stand out like boards.
//! Renkatsu's own, not the game's.
//!
//! - The net is built at load from the skirt's chains as they rest: the
//!   chains go round the waist, with `BETWEEN` columns between each two
//!   that follow the skirt's round shape, and `ROWS` rows down them.
//! - Each frame its target is where the chains put the skirt after the
//!   game's step and the Advanced pass: every point rides the chain bones
//!   on either side of it, as the mesh is skinned to them.
//! - An XPBD solver moves the net from there: it does not stretch, gives
//!   way when pushed together, resists bending, falls under gravity and
//!   collides with the limbs, the floor and the map with friction, while
//!   a pull and a reach hold it near the target. The pull is strong where
//!   the target hangs down and weak where the chains stick out sideways,
//!   which is where the game's stiffness is wrong.
//! - Every vertex weighted to the chains is bound to the rest net: its
//!   place, normal and tangent in the frame of the net's surface there.
//!   Each frame it is put back in that frame on the moved net. What the
//!   game's skinning adds to that on the target is added too, and fades
//!   as the net leaves the target: a net on its target leaves the mesh
//!   exactly as the game skins it, a net away from it gives cloth.

use super::cloth::{rest_on, Nearby, Obstacles, WorldCapsule};
use super::{world, Bone, Particle};
use crate::scene::{DrawMesh, Scene, Skinning};
use glam::{Mat3, Mat4, Quat, Vec3};
use std::borrow::Cow;
use std::sync::Arc;

/// Columns between each two chains.
pub const BETWEEN: usize = 3;
/// Rows down the skirt, the waist and the hem included.
pub const ROWS: usize = 12;
/// Solver steps per frame.
const SUBSTEPS: u32 = 8;
const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);
/// How far the net keeps off the limbs and the floor.
const THICKNESS: f32 = 0.01;
/// How far a point may leave its target, as a share of its way down from
/// the waist.
const REACH: f32 = 1.5;
/// The pull back to the target (XPBD compliance, metres per newton for
/// a point of unit mass) where the target hangs straight down and where it
/// sticks out level; between the two it goes by how far the target leans.
const PULL_HANGING: f32 = 2e-3;
const PULL_LEVEL: f32 = 0.02;
/// The pull where the target is a standing skirt's, which holds the net
/// on it: there the game's shape is kept. A target counts as that when
/// it is where the skirt rests on the waist (fully within the first of
/// `STRAY`, not at all beyond the second) and hangs down (by the cosine
/// of its lean from straight down, from the first of `UPRIGHT` to the
/// second).
const PULL_RESTING: f32 = 1e-6;
const STRAY: (f32, f32) = (0.015, 0.045);
const UPRIGHT: (f32, f32) = (0.5, 0.8);
/// Compliances of the lengths along and across and of the diagonals when
/// pulled apart.
const STRETCH: f32 = 0.0;
const SHEAR: f32 = 1e-6;
/// Compliances of the lengths over one point, down and across, either
/// way: how easily the net bends. Stiffer down the skirt than round it,
/// so that it folds along its length.
const BEND: f32 = 3e-3;
const BEND_ACROSS: f32 = 3e-2;
/// Compliance of a length or a diagonal pushed together: the net gives
/// way in its plane, where real cloth makes folds smaller than a cell,
/// and its bending keeps it smooth.
const SQUEEZE: f32 = 0.1;
/// The least share of its length a link is pushed together to: the net
/// does not fold onto itself.
const LEAST: f32 = 0.4;
/// How much longer than at rest a length across may get when the chains
/// spread: a little, so that a standing skirt follows them, and no more,
/// so that splayed chains do not make cloth.
const SLACK: f32 = 1.15;
/// Friction on the limbs and on the floor (Coulomb): cloth stays on a
/// hip or a thigh and slides out flat on the floor.
const FRICTION: f32 = 0.5;
const FLOOR_FRICTION: f32 = 0.05;
/// How fast the net's own motion dies away, per second.
const DAMPING: f32 = 3.0;
/// How deep into the buttocks and thighs the net may go where its target
/// is that deep and is a standing skirt's: their fit is rough and the
/// chains put a standing skirt inside it.
pub(super) const DEEPEST: f32 = 0.03;
/// How far a bound vertex's net point leaves its target before the
/// vertex takes nothing from the game's skinning any more.
const FADE: f32 = 0.04;
/// Rows whose reach is below this do not collide: near the waist the
/// target wins over a rough fit of the hips.
const TIGHT: f32 = 0.02;
/// A target further than this from the net starts it again on the target.
const JUMP: f32 = 0.5;
/// Vertices closer to the rest net than `NEAR` follow it fully, ones
/// further than `FAR` keep their skinned place (flaps, ribbons).
const NEAR: f32 = 0.05;
const FAR: f32 = 0.12;

/// A constant, or what `RK_NET_P=name=value,...` gives for it while the
/// net is a prototype.
pub(super) fn tune(name: &str, default: f32) -> f32 {
    static GIVEN: std::sync::OnceLock<Vec<(String, f32)>> = std::sync::OnceLock::new();
    GIVEN
        .get_or_init(|| {
            let text = std::env::var("RK_NET_P").unwrap_or_default();
            text.split(',')
                .filter_map(|kv| {
                    let (k, v) = kv.split_once('=')?;
                    Some((k.to_string(), v.parse().ok()?))
                })
                .collect()
        })
        .iter()
        .find(|(k, _)| k == name)
        .map_or(default, |&(_, v)| v)
}

/// The kinds of link, which give its compliance and its length.
const DOWN: u8 = 0;
const ACROSS: u8 = 1;
const DIAGONAL: u8 = 2;
const BEND_DOWN: u8 = 3;
const BEND_ROUND: u8 = 4;

/// The links of a ring grid: each pair of points whose distance is kept,
/// with its kind.
fn links(cols: usize, rows: usize) -> Vec<(u32, u32, u8)> {
    let at = |c: usize, r: usize| (r * cols + (c % cols)) as u32;
    let mut links = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            if r + 1 < rows {
                links.push((at(c, r), at(c, r + 1), DOWN));
            }
            links.push((at(c, r), at(c + 1, r), ACROSS));
            if r + 1 < rows {
                links.push((at(c, r), at(c + 1, r + 1), DIAGONAL));
                links.push((at(c + 1, r), at(c, r + 1), DIAGONAL));
            }
            if r + 2 < rows {
                links.push((at(c, r), at(c, r + 2), BEND_DOWN));
            }
            links.push((at(c, r), at(c + 2, r), BEND_ROUND));
        }
    }
    links
}

/// One skirt's net.
#[derive(Debug, Clone)]
pub struct Net {
    cols: usize,
    rows: usize,
    /// The nodes the points ride.
    nodes: Vec<usize>,
    /// The node the skirt hangs from, whose motion the net is carried by,
    /// and its world matrix at the step before.
    waist: usize,
    waist_before: Mat4,
    /// Per point: the two nodes it rides (indices into `nodes`), the point
    /// in each node's space as the net was built, and the weights.
    src: Vec<[(u32, Vec3, f32); 2]>,
    /// Per point: how far it may leave the target.
    reach: Vec<f32>,
    /// Pairs of points whose distance is kept, with the kind of link.
    links: Vec<(u32, u32, u8)>,
    /// Every node of the skirt's chains, which vertices are bound by.
    pub(super) chain_nodes: Vec<usize>,
    /// The net as it was built, in the world.
    rest: Vec<Vec3>,
    /// The same in the space of the waist node.
    rest_on_waist: Vec<Vec3>,
    pos: Vec<Vec3>,
    vel: Vec<Vec3>,
    target: Vec<Vec3>,
    /// The target of the frame before, which the steps move from.
    before: Vec<Vec3>,
    /// Whether `pos` holds a net that ran; otherwise it starts on the
    /// target.
    ready: bool,
    /// Whether the skirt ran this frame.
    active: bool,
    nearby: Nearby,
}

/// Where the particles of a chain rest, as `Cloth::begin` takes them, and
/// the node that carries each segment below a particle.
fn chain_rest(scene: &Scene, ps: &[Particle]) -> Option<(Vec<Vec3>, Vec<usize>)> {
    let mut points = Vec::with_capacity(ps.len());
    for (i, p) in ps.iter().enumerate() {
        let parent = i.checked_sub(1).and_then(|pi| ps[pi].node);
        points.push(match (p.node, parent) {
            (Some(n), _) => world(scene, n).w_axis.truncate(),
            (None, Some(pn)) => world(scene, pn).transform_point3(p.end_offset),
            (None, None) => return None,
        });
    }
    // The segment from particle i down rides particle i's node; the end
    // particle rides its parent's.
    let mut nodes = Vec::with_capacity(ps.len());
    let mut last = None;
    for p in ps {
        last = p.node.or(last);
        nodes.push(last?);
    }
    Some((points, nodes))
}

/// The place `share` of the way down a polyline by length, as the segment
/// it is on and how far along that segment.
fn along(points: &[Vec3], share: f32) -> (usize, f32) {
    let lengths: Vec<f32> = points.windows(2).map(|w| (w[1] - w[0]).length()).collect();
    let total: f32 = lengths.iter().sum();
    let mut want = share * total;
    for (i, &l) in lengths.iter().enumerate() {
        if want <= l || i + 1 == lengths.len() {
            return (i, if l > 0.0 { (want / l).clamp(0.0, 1.0) } else { 0.0 });
        }
        want -= l;
    }
    (0, 0.0)
}

impl Net {
    /// The net of the skirt whose chains are `ring` (indices into
    /// `bones`), as the bones rest now.
    pub(super) fn build(scene: &Scene, bones: &[Bone], ring: &[usize]) -> Option<Net> {
        struct Chain {
            points: Vec<Vec3>,
            nodes: Vec<usize>,
        }
        let mut chains = Vec::new();
        let mut chain_nodes = Vec::new();
        let Bone::Dynamic(first) = &bones[*ring.first()?] else {
            return None;
        };
        let waist = scene.nodes[first.particles.first()?.node?].parent?;
        for &b in ring {
            let Bone::Dynamic(d) = &bones[b] else { return None };
            let (points, nodes) = chain_rest(scene, &d.particles)?;
            chain_nodes.extend(d.particles.iter().filter_map(|p| p.node));
            chains.push(Chain { points, nodes });
        }
        if chains.len() < 3 {
            return None;
        }
        // The waist's axis: through the middle of the chains' tops, along
        // the way they hang.
        let n = chains.len() as f32;
        let centre = chains.iter().map(|c| c.points[0]).sum::<Vec3>() / n;
        let up = chains
            .iter()
            .map(|c| c.points[0] - *c.points.last().expect("two or more"))
            .sum::<Vec3>()
            .normalize_or_zero();
        if up == Vec3::ZERO {
            return None;
        }
        let flat = |p: Vec3| {
            let d = p - centre;
            d - up * d.dot(up)
        };
        // Round the waist in order, whatever the chains are numbered.
        let side = up.any_orthonormal_vector();
        let ahead = up.cross(side);
        let angle = |c: &Chain| {
            let d = flat(*c.points.last().expect("two or more"));
            d.dot(ahead).atan2(d.dot(side))
        };
        chains.sort_by(|a, b| angle(a).total_cmp(&angle(b)));

        let between = tune("between", BETWEEN as f32) as usize;
        let cols = chains.len() * (between + 1);
        let rows = tune("rows", ROWS as f32) as usize;
        let mut nodes: Vec<usize> = Vec::new();
        let mut node_index = |node: usize| -> u32 {
            match nodes.iter().position(|&n| n == node) {
                Some(i) => i as u32,
                None => {
                    nodes.push(node);
                    (nodes.len() - 1) as u32
                }
            }
        };
        let mut rest = vec![Vec3::ZERO; cols * rows];
        let mut src = vec![[(0u32, Vec3::ZERO, 0.0f32); 2]; cols * rows];
        for r in 0..rows {
            let share = r as f32 / (rows - 1) as f32;
            let places: Vec<(Vec3, usize)> = chains
                .iter()
                .map(|c| {
                    let (seg, t) = along(&c.points, share);
                    (c.points[seg].lerp(c.points[seg + 1], t), c.nodes[seg])
                })
                .collect();
            for (j, &(pa, na)) in places.iter().enumerate() {
                let (pb, nb) = places[(j + 1) % places.len()];
                for m in 0..=between {
                    let f = m as f32 / (between + 1) as f32;
                    // Round the waist axis: the angle, the distance from
                    // the axis and the height go from one chain to the next.
                    let (fa, fb) = (flat(pa), flat(pb));
                    let (ha, hb) = ((pa - centre).dot(up), (pb - centre).dot(up));
                    let p = if fa.length() > 1e-4 && fb.length() > 1e-4 {
                        let turn = Quat::IDENTITY.slerp(Quat::from_rotation_arc(fa.normalize(), fb.normalize()), f);
                        let radius = fa.length() + (fb.length() - fa.length()) * f;
                        centre + up * (ha + (hb - ha) * f) + turn * fa.normalize() * radius
                    } else {
                        pa.lerp(pb, f)
                    };
                    let i = r * cols + j * (between + 1) + m;
                    rest[i] = p;
                    let local = |node: usize| world(scene, node).inverse().transform_point3(p);
                    src[i] = [(node_index(na), local(na), 1.0 - f), (node_index(nb), local(nb), f)];
                }
            }
        }
        // How far each point may go: from nothing at the waist, `REACH` of
        // the way down.
        let mut reach = vec![0.0; cols * rows];
        for c in 0..cols {
            let mut down = 0.0;
            for r in 1..rows {
                down += (rest[r * cols + c] - rest[(r - 1) * cols + c]).length();
                reach[r * cols + c] = REACH * down;
            }
        }
        let links = links(cols, rows);
        chain_nodes.sort_unstable();
        chain_nodes.dedup();
        if std::env::var("RK_NET_DEBUG").is_ok() {
            let row = |r: usize| (rest[r * cols] - rest[r * cols + 1]).length() * 100.0;
            let down = (rest[0] - rest[cols]).length() * 100.0;
            eprintln!(
                "net build {cols}x{rows} chains {} cell cm: across top {:.1} hem {:.1} down {:.1} length {:.0}",
                chains.len(),
                row(0),
                row(rows - 1),
                down,
                reach[(rows - 1) * cols] / REACH * 100.0
            );
        }
        Some(Net {
            cols,
            rows,
            nodes,
            waist,
            waist_before: world(scene, waist),
            rest_on_waist: {
                let back = world(scene, waist).inverse();
                rest.iter().map(|&p| back.transform_point3(p)).collect()
            },
            src,
            reach,
            links,
            chain_nodes,
            pos: rest.clone(),
            vel: vec![Vec3::ZERO; rest.len()],
            target: rest.clone(),
            before: rest.clone(),
            rest,
            ready: false,
            active: false,
            nearby: Nearby::default(),
        })
    }

    /// Starts the net again on its target at the next step.
    pub(super) fn reset(&mut self) {
        self.ready = false;
    }

    /// Moves the net with the character, as `Physics::carry`.
    pub(super) fn carry(&mut self, moved: Mat4) {
        for p in self.pos.iter_mut().chain(&mut self.target).chain(&mut self.before) {
            *p = moved.transform_point3(*p);
        }
        for v in &mut self.vel {
            *v = moved.transform_vector3(*v);
        }
        self.waist_before = moved * self.waist_before;
    }

    /// Where the chains put the net now.
    fn aim(&mut self, scene: &Scene) {
        let mats: Vec<Mat4> = self.nodes.iter().map(|&n| world(scene, n)).collect();
        for (t, src) in self.target.iter_mut().zip(&self.src) {
            *t = src
                .iter()
                .map(|&(n, p, w)| mats[n as usize].transform_point3(p) * w)
                .sum();
        }
    }

    /// One frame of `dt` seconds, after the chains moved. `active` false
    /// leaves the skirt as the chains put it.
    pub(super) fn step(
        &mut self,
        scene: &Scene,
        active: bool,
        capsules: &[(WorldCapsule, f32)],
        floor: Option<(Vec3, Vec3)>,
        obstacles: Option<&Arc<Obstacles>>,
        dt: f32,
    ) {
        self.active = active;
        if !active {
            self.ready = false;
            return;
        }
        let began = std::time::Instant::now();
        self.aim(scene);
        let jumped = self
            .pos
            .iter()
            .zip(&self.target)
            .any(|(p, t)| (*p - *t).length_squared() > JUMP * JUMP);
        let waist = world(scene, self.waist);
        if !self.ready || jumped || dt <= 0.0 {
            self.pos.clone_from(&self.target);
            self.before.clone_from(&self.target);
            self.vel.fill(Vec3::ZERO);
            self.waist_before = waist;
            self.ready = true;
            if dt <= 0.0 {
                return;
            }
        }
        // The net goes along with the waist: only the motion relative to
        // it is simulated, so that a turn of the whole body does not leave
        // the skirt behind.
        let moved = waist * self.waist_before.inverse();
        self.waist_before = waist;
        for p in self.pos.iter_mut().chain(&mut self.before) {
            *p = moved.transform_point3(*p);
        }
        for v in &mut self.vel {
            *v = moved.transform_vector3(*v);
        }
        let (cols, rows) = (self.cols, self.rows);
        // The target's lengths, which the net keeps.
        // The lengths the net keeps: the target's, but across the skirt
        // no more than a little over the rest ones.
        let slack = tune("slack", SLACK);
        let lengths: Vec<f32> = self
            .links
            .iter()
            .map(|&(a, b, kind)| {
                let (a, b) = (a as usize, b as usize);
                let target = (self.target[a] - self.target[b]).length();
                if kind == DOWN || kind == BEND_DOWN {
                    target
                } else {
                    target.min((self.rest[a] - self.rest[b]).length() * slack)
                }
            })
            .collect();
        // Per kind of link: the compliance pulled apart and pushed
        // together.
        let squeeze = tune("squeeze", SQUEEZE);
        let (bend, bend_across) = (tune("bend", BEND), tune("bend_across", BEND_ACROSS));
        let kinds = [
            (STRETCH, squeeze),
            (STRETCH, squeeze),
            (SHEAR, squeeze),
            (bend, bend),
            (bend_across, bend_across),
        ];
        let least = tune("least", LEAST);
        let pull_hanging = tune("pull_hanging", PULL_HANGING);
        let pull_level = tune("pull_level", PULL_LEVEL);
        let pull_resting = tune("pull_resting", PULL_RESTING);
        let friction = tune("friction", FRICTION);
        let floor_friction = tune("floor_friction", FLOOR_FRICTION);
        let reach_scale = tune("reach", REACH) / REACH;
        let gravity = GRAVITY * tune("gravity", 1.0);
        let substeps = tune("substeps", SUBSTEPS as f32) as u32;
        // The pull back to the target, by how far the target leans from
        // hanging straight down there, and stronger where it also is
        // where the skirt rests on the waist.
        let down = GRAVITY.normalize();
        // Per point, how much the target there is a standing skirt's: it
        // hangs down and is where the skirt rests on the waist.
        let mut standing = vec![0.0f32; cols * rows];
        let (stray_from, stray_to) = (tune("stray_from", STRAY.0), tune("stray_to", STRAY.1));
        let pull: Vec<f32> = (0..cols * rows)
            .map(|i| {
                let r = i / cols;
                if r == 0 {
                    return 0.0;
                }
                let dir = (self.target[i] - self.target[i - cols]).normalize_or_zero();
                let hangs = dir.dot(down).clamp(0.0, 1.0);
                let leaning = pull_hanging * (pull_level / pull_hanging).powf(1.0 - hangs);
                let resting = waist.transform_point3(self.rest_on_waist[i]);
                let stray = (self.target[i] - resting).length();
                let stray = ((stray - stray_from) / (stray_to - stray_from)).clamp(0.0, 1.0);
                let upright = ((hangs - UPRIGHT.0) / (UPRIGHT.1 - UPRIGHT.0)).clamp(0.0, 1.0);
                standing[i] = (1.0 - stray) * upright;
                leaning * (pull_resting / pull_hanging).powf(standing[i])
            })
            .collect();
        // What the net may meet: the limbs and the map near it.
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for (p, t) in self.pos.iter().zip(&self.target) {
            lo = lo.min(*p).min(*t);
            hi = hi.max(*p).max(*t);
        }
        let reach = self.reach.iter().copied().fold(0.0, f32::max);
        let (lo, hi) = (lo - Vec3::splat(reach + 0.1), hi + Vec3::splat(reach + 0.1));
        let nocaps = std::env::var("RK_NET_NOCAPS").is_ok();
        let capsules: Vec<(WorldCapsule, f32)> = capsules
            .iter()
            .filter(|_| !nocaps)
            .filter(|(c, _)| {
                let (clo, chi) = c.bounds();
                clo.cmple(hi).all() && chi.cmpge(lo).all()
            })
            .copied()
            .collect();
        self.nearby.refresh(obstacles, lo, hi);

        let h = dt / substeps as f32;
        let damping = (-tune("damping", DAMPING) * h).exp();
        let mut column = vec![Vec3::ZERO; rows];
        for step in 0..substeps {
            let k = (step + 1) as f32 / substeps as f32;
            let aim: Vec<Vec3> = self
                .before
                .iter()
                .zip(&self.target)
                .map(|(b, t)| b.lerp(*t, k))
                .collect();
            let old = self.pos.clone();
            self.pos[..cols].copy_from_slice(&aim[..cols]);
            for (p, v) in self.pos.iter_mut().zip(&mut self.vel).skip(cols) {
                *v = (*v + gravity * h) * damping;
                *p += *v * h;
            }
            let free = |i: usize| if i < cols { 0.0 } else { 1.0 };
            for (&(a, b, kind), &len) in self.links.iter().zip(&lengths) {
                let (a, b) = (a as usize, b as usize);
                let (pulled, pushed) = kinds[kind as usize];
                let (wa, wb) = (free(a), free(b));
                let d = self.pos[a] - self.pos[b];
                let dist = d.length();
                if dist <= 1e-9 || wa + wb == 0.0 {
                    continue;
                }
                // Pushed together a link gives way, down to a share of
                // its length.
                let (compliance, len) = if dist >= len {
                    (pulled, len)
                } else if dist < len * least {
                    (0.0, len * least)
                } else {
                    (pushed, len)
                };
                let alpha = compliance / (h * h);
                let lambda = -(dist - len) / (wa + wb + alpha);
                let n = d / dist;
                self.pos[a] += n * (lambda * wa);
                self.pos[b] -= n * (lambda * wb);
            }
            for i in cols..cols * rows {
                let to = aim[i] - self.pos[i];
                let alpha = pull[i] / (h * h);
                self.pos[i] += to * (1.0 / (1.0 + alpha));
                let off = self.pos[i] - aim[i];
                let far = off.length();
                if far > self.reach[i] * reach_scale {
                    self.pos[i] = aim[i] + off * (self.reach[i] * reach_scale / far);
                }
                if self.reach[i] < TIGHT {
                    continue;
                }
                // The deepest thing the point was pushed out of: its
                // direction and how far it pushed.
                let mut touch: Option<(Vec3, f32, f32)> = None;
                for (cap, give) in &capsules {
                    if let Some((n, depth)) = push_out(&mut self.pos[i], aim[i], cap, *give * standing[i]) {
                        if touch.is_none_or(|t| depth > t.1) {
                            touch = Some((n, depth, friction));
                        }
                    }
                }
                if let Some((origin, up)) = floor {
                    let height = (self.pos[i] - origin).dot(up) - THICKNESS;
                    if height < 0.0 {
                        self.pos[i] -= up * height;
                        if touch.is_none_or(|t| -height > t.1) {
                            touch = Some((up, -height, floor_friction));
                        }
                    }
                }
                // Friction: a touching point loses as much of its slide
                // as the push holds (Coulomb), all of it when it barely
                // slides.
                if let Some((n, depth, friction)) = touch {
                    let slide = self.pos[i] - old[i];
                    let along = slide - n * slide.dot(n);
                    let len = along.length();
                    if len > 1e-9 {
                        self.pos[i] -= along * (friction * depth / len).min(1.0);
                    }
                }
            }
            if !self.nearby.triangles.is_empty() {
                for c in 0..cols {
                    for (r, p) in column.iter_mut().enumerate() {
                        *p = self.pos[r * cols + c];
                    }
                    rest_on(&mut column, &self.nearby.triangles);
                    for (r, p) in column.iter().enumerate() {
                        self.pos[r * cols + c] = *p;
                    }
                }
            }
            for ((v, p), o) in self.vel.iter_mut().zip(&self.pos).zip(&old).skip(cols) {
                *v = (*p - *o) / h;
            }
        }
        self.before.clone_from(&self.target);
        if std::env::var("RK_NET_FREEZE").is_ok() {
            self.pos.clone_from(&self.target);
        }
        if let Ok(path) = std::env::var("RK_NET_DUMP") {
            let text: String = self
                .pos
                .iter()
                .zip(&self.target)
                .map(|(p, t)| format!("{} {} {} {} {} {}\n", p.x, p.y, p.z, t.x, t.y, t.z))
                .collect();
            let caps: String = capsules
                .iter()
                .map(|(c, _)| {
                    format!(
                        "c {} {} {} {} {} {} {} {}\n",
                        c.a.x, c.a.y, c.a.z, c.b.x, c.b.y, c.b.z, c.ra, c.rb
                    )
                })
                .collect();
            let caps = match floor {
                Some((o, _)) => format!("{caps}c 0 {} 0 0 {} 0 0 0\n", o.y, o.y),
                None => caps,
            };
            let _ = std::fs::write(&path, format!("{cols} {rows}\n{text}{caps}"));
            if tune("frames", 0.0) > 0.0 {
                use std::sync::atomic::{AtomicU32, Ordering};
                static FRAME: AtomicU32 = AtomicU32::new(0);
                let f = FRAME.fetch_add(1, Ordering::Relaxed);
                let _ = std::fs::copy(&path, format!("{path}.{f:03}"));
            }
        }
        if std::env::var("RK_NET_DEBUG").is_ok() {
            let lines: Vec<String> = (0..rows)
                .map(|r| {
                    let (mut sum, mut max, mut dy) = (0.0f32, 0.0f32, 0.0f32);
                    for c in 0..cols {
                        let d = self.pos[r * cols + c] - self.target[r * cols + c];
                        sum += d.length();
                        max = max.max(d.length());
                        dy += d.y;
                    }
                    format!(
                        "{:.0}/{:.0}/{:+.0}",
                        sum / cols as f32 * 100.0,
                        max * 100.0,
                        dy / cols as f32 * 100.0
                    )
                })
                .collect();
            eprintln!(
                "net {cols}x{rows} {} us caps {} tris {} mean/max/dy cm: {}",
                began.elapsed().as_micros(),
                capsules.len(),
                self.nearby.triangles.len(),
                lines.join(" ")
            );
        }
    }

    /// The net and its target for drawing, with the directions along
    /// their surfaces at each point.
    fn shape(&self) -> NetShape {
        NetShape {
            active: self.active && self.ready,
            pos: frames(&self.pos, self.cols, self.rows),
            target: frames(&self.target, self.cols, self.rows),
        }
    }
}

/// The point of a capsule's axis nearest to `p`, and the capsule's radius
/// there with the net's thickness.
fn on_axis(p: Vec3, cap: &WorldCapsule) -> (Vec3, f32) {
    let axis = cap.b - cap.a;
    let len2 = axis.length_squared();
    let t = if len2 > 0.0 {
        ((p - cap.a).dot(axis) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (cap.a + axis * t, cap.ra + (cap.rb - cap.ra) * t + THICKNESS)
}

/// Pushes a net point out of a capsule, but only as far as its target
/// `aim` is out of it: where the chains put the skirt inside a rough fit
/// of the body (the hips), the net may be as deep, up to `DEEPEST`.
/// Returns the direction it was pushed in, when it was.
fn push_out(p: &mut Vec3, aim: Vec3, cap: &WorldCapsule, give: f32) -> Option<(Vec3, f32)> {
    let (on, r) = on_axis(*p, cap);
    let d = *p - on;
    let dist = d.length();
    if dist >= r || dist <= 1e-6 {
        return None;
    }
    let (aim_on, aim_r) = on_axis(aim, cap);
    let allowed = (aim_r - (aim - aim_on).length()).clamp(0.0, give);
    let r = r - allowed;
    if dist >= r {
        return None;
    }
    *p += d * ((r - dist) / dist);
    Some((d / dist, r - dist))
}

/// Each point of a grid with the directions round the waist and down
/// the skirt there.
fn frames(points: &[Vec3], cols: usize, rows: usize) -> Vec<[Vec3; 3]> {
    (0..cols * rows)
        .map(|i| {
            let (c, r) = (i % cols, i / cols);
            let at = |c: usize, r: usize| points[r * cols + c];
            let round = at((c + 1) % cols, r) - at((c + cols - 1) % cols, r);
            // The hem's row takes two rows up: one cell tucked under
            // at the hem does not turn the surface over.
            let down = if r + 1 == rows && rows > 2 {
                at(c, r) - at(c, r - 2)
            } else {
                at(c, (r + 1).min(rows - 1)) - at(c, r.saturating_sub(1))
            };
            [points[i], round, down]
        })
        .collect()
}

/// A garment vertex bound to a net: the triangle of the rest net it is
/// nearest to, where on it, and how much it follows the net.
#[derive(Debug, Clone, Copy)]
pub struct Bound {
    pub vertex: u32,
    pub net: u32,
    pub corners: [u32; 3],
    pub bary: [f32; 3],
    pub weight: f32,
    /// The vertex as it rests, in the frame of the rest net's surface
    /// there: its place, normal and tangent.
    pub local: [Vec3; 3],
}

/// The bound vertices of one renderer.
#[derive(Debug, Clone)]
pub struct Binding {
    pub renderer: usize,
    /// The mesh it was bound for, by address.
    pub mesh: usize,
    pub vertices: Vec<Bound>,
}

/// Closest point of the triangle `a b c` to `p`, as weights of its corners
/// (Ericson, Real-Time Collision Detection 5.1.5).
fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> [f32; 3] {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return [1.0, 0.0, 0.0];
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return [0.0, 1.0, 0.0];
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return [1.0 - v, v, 0.0];
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return [0.0, 0.0, 1.0];
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return [1.0 - w, 0.0, w];
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return [0.0, 1.0 - w, w];
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    [1.0 - v - w, v, w]
}

/// Binds every vertex weighted to a net's chains, in every renderer of
/// the scene whether shown or not, to the rest net.
pub(super) fn bind(scene: &Scene, nets: &[Net]) -> Vec<Binding> {
    if nets.is_empty() {
        return Vec::new();
    }
    let (world, _) = scene.world();
    let mut of_net = vec![u32::MAX; scene.nodes.len()];
    for (k, net) in nets.iter().enumerate() {
        for &n in &net.chain_nodes {
            of_net[n] = k as u32;
        }
    }
    // The rest nets' triangles, two per cell.
    let triangles: Vec<Vec<[u32; 3]>> = nets
        .iter()
        .map(|net| {
            let at = |c: usize, r: usize| (r * net.cols + c % net.cols) as u32;
            let mut out = Vec::new();
            for r in 0..net.rows - 1 {
                for c in 0..net.cols {
                    out.push([at(c, r), at(c + 1, r), at(c + 1, r + 1)]);
                    out.push([at(c, r), at(c + 1, r + 1), at(c, r + 1)]);
                }
            }
            out
        })
        .collect();
    let mut out = Vec::new();
    for (ri, r) in scene.renderers.iter().enumerate() {
        if !r.skinned || r.mesh.skin.is_empty() || r.mesh.positions.is_empty() {
            continue;
        }
        let slots: Vec<u32> = r.bones.iter().map(|b| b.map_or(u32::MAX, |n| of_net[n])).collect();
        if slots.iter().all(|&s| s == u32::MAX) {
            continue;
        }
        let draw = scene.draw_mesh(r, &world);
        let positions = draw.positions();
        let posed = draw.posed();
        let rest_frames: Vec<Vec<[Vec3; 3]>> = nets.iter().map(|n| frames(&n.rest, n.cols, n.rows)).collect();
        let mut vertices = Vec::new();
        for (vi, (sv, p)) in r.mesh.skin.iter().zip(&positions).enumerate() {
            // The net the vertex is most weighted to, and by how much.
            let mut share = [0.0f32; 4];
            let mut nets_of = [u32::MAX; 4];
            for k in 0..4 {
                let net = slots.get(sv.bones[k] as usize).copied().unwrap_or(u32::MAX);
                if net == u32::MAX || sv.weights[k] <= 0.0 {
                    continue;
                }
                let slot = nets_of.iter().position(|&n| n == net || n == u32::MAX).unwrap_or(0);
                nets_of[slot] = net;
                share[slot] += sv.weights[k];
            }
            let Some(best) = (0..4)
                .filter(|&k| nets_of[k] != u32::MAX)
                .max_by(|&a, &b| share[a].total_cmp(&share[b]))
            else {
                continue;
            };
            let k = nets_of[best] as usize;
            let net = &nets[k];
            let p = Vec3::from(*p);
            let mut nearest = (f32::MAX, [0u32; 3], [0.0f32; 3]);
            for tri in &triangles[k] {
                let [a, b, c] = tri.map(|i| net.rest[i as usize]);
                let bary = closest_on_triangle(p, a, b, c);
                let q = a * bary[0] + b * bary[1] + c * bary[2];
                let d = (p - q).length_squared();
                if d < nearest.0 {
                    nearest = (d, *tri, bary);
                }
            }
            let dist = nearest.0.sqrt();
            let near = 1.0 - ((dist - NEAR) / (FAR - NEAR)).clamp(0.0, 1.0);
            let weight = share[best].min(1.0) * near;
            if weight <= 1e-3 {
                continue;
            }
            let mut bound = Bound {
                vertex: vi as u32,
                net: k as u32,
                corners: nearest.1,
                bary: nearest.2,
                weight,
                local: [Vec3::ZERO; 3],
            };
            let (at, turn) = surface(&rest_frames[k], &bound);
            let back = turn.transpose();
            let t = posed.tangents[vi];
            bound.local = [
                back * (p - at),
                back * Vec3::from(posed.normals[vi]),
                back * Vec3::new(t[0], t[1], t[2]),
            ];
            vertices.push(bound);
        }
        if std::env::var("RK_NET_DEBUG").is_ok() && !vertices.is_empty() {
            let mut bound = vec![false; positions.len()];
            for b in &vertices {
                bound[b.vertex as usize] = true;
            }
            let mut edges: Vec<f32> = Vec::new();
            let mut tris = 0;
            for t in r.mesh.indices.chunks_exact(3) {
                if t.iter().all(|&i| bound[i as usize]) {
                    tris += 1;
                    for k in 0..3 {
                        let (a, b) = (positions[t[k] as usize], positions[t[(k + 1) % 3] as usize]);
                        edges.push((Vec3::from(a) - Vec3::from(b)).length());
                    }
                }
            }
            edges.sort_by(f32::total_cmp);
            let q = |f: f32| {
                edges
                    .get(((edges.len() as f32 - 1.0) * f) as usize)
                    .copied()
                    .unwrap_or(0.0)
                    * 100.0
            };
            eprintln!(
                "bind {} verts {} of {} tris {} edge cm p10 {:.1} p50 {:.1} p90 {:.1} max {:.1}",
                r.name,
                vertices.len(),
                positions.len(),
                tris,
                q(0.1),
                q(0.5),
                q(0.9),
                q(1.0)
            );
        }
        if !vertices.is_empty() {
            out.push(Binding {
                renderer: ri,
                mesh: Arc::as_ptr(&r.mesh) as *const u8 as usize,
                vertices,
            });
        }
    }
    out
}

/// One net as drawn: each point with the directions round and down.
#[derive(Debug, Clone)]
pub struct NetShape {
    pub active: bool,
    pub pos: Vec<[Vec3; 3]>,
    pub target: Vec<[Vec3; 3]>,
}

/// The cloth nets of a character, for `Scene::pose` to put the bound
/// vertices on.
#[derive(Debug, Clone)]
pub struct ClothDraw {
    pub nets: Vec<NetShape>,
    pub bindings: Arc<Vec<Binding>>,
}

/// The place and the turn of a net's surface at a bound vertex.
fn surface(points: &[[Vec3; 3]], b: &Bound) -> (Vec3, Mat3) {
    let mut p = Vec3::ZERO;
    let (mut round, mut down) = (Vec3::ZERO, Vec3::ZERO);
    for (&c, &w) in b.corners.iter().zip(&b.bary) {
        let [q, r, d] = points[c as usize];
        p += q * w;
        round += r * w;
        down += d * w;
    }
    let e1 = down.normalize_or_zero();
    let e3 = round.cross(down).normalize_or_zero();
    let e2 = e3.cross(e1);
    (p, Mat3::from_cols(e1, e2, e3))
}

/// Puts a bound vertex on its net. `skinned` is its place, normal and
/// tangent as the game skins it. The vertex goes where it rests in the
/// frame of the net's surface; what the skinning adds to that on the
/// target is added by how near the net still is to the target. Returns
/// the three, and how far the skinning is from the net's own carry.
fn carry(b: &Bound, net: &NetShape, skinned: [Vec3; 3]) -> Option<([Vec3; 3], f32)> {
    let (to, to_turn) = surface(&net.pos, b);
    let (from, from_turn) = surface(&net.target, b);
    if to_turn.determinant().abs() < 0.5 || from_turn.determinant().abs() < 0.5 {
        return None;
    }
    let turn = to_turn * from_turn.transpose();
    let [p, n, t] = skinned;
    let [local, local_n, local_t] = b.local;
    let extra = p - (from + from_turn * local);
    let fade = tune("fade", FADE);
    let off = (to - from).length() + (to_turn.z_axis - from_turn.z_axis).length() * fade;
    let game = (1.0 - off / fade).clamp(0.0, 1.0);
    let moved = to + to_turn * local + turn * extra * game;
    let cloth_n = (to_turn * local_n).lerp(turn * n, game);
    let cloth_t = (to_turn * local_t).lerp(turn * t, game);
    Some((
        [
            p.lerp(moved, b.weight),
            n.lerp(cloth_n, b.weight).normalize_or_zero(),
            t.lerp(cloth_t, b.weight).normalize_or_zero(),
        ],
        extra.length(),
    ))
}

impl ClothDraw {
    pub fn from_nets(nets: &[Net], bindings: &Arc<Vec<Binding>>) -> Self {
        ClothDraw {
            nets: nets.iter().map(Net::shape).collect(),
            bindings: bindings.clone(),
        }
    }

    /// Puts the vertices of renderer `ri` that are bound to a net on it:
    /// the mesh is skinned and pushed out on the CPU, and handed on as
    /// already in the world.
    pub fn dress(&self, ri: usize, d: &mut DrawMesh<'_>) {
        let Some(binding) = self.bindings.iter().find(|b| b.renderer == ri) else {
            return;
        };
        if binding.mesh != Arc::as_ptr(&d.renderer.mesh) as *const u8 as usize
            || !matches!(d.skinning, Skinning::Bones { .. })
            || !binding
                .vertices
                .iter()
                .any(|b| self.nets.get(b.net as usize).is_some_and(|n| n.active))
        {
            return;
        }
        let began = std::time::Instant::now();
        let push = d.push.take();
        let mut posed = d.posed();
        let mut extra: Vec<f32> = Vec::new();
        for b in &binding.vertices {
            let Some(net) = self.nets.get(b.net as usize).filter(|n| n.active) else {
                continue;
            };
            let vi = b.vertex as usize;
            let t = posed.tangents[vi];
            let skinned = [
                Vec3::from(posed.positions[vi]),
                Vec3::from(posed.normals[vi]),
                Vec3::new(t[0], t[1], t[2]),
            ];
            let Some(([p, n, tv], game)) = carry(b, net, skinned) else {
                continue;
            };
            extra.push(game);
            posed.positions[vi] = p.to_array();
            posed.normals[vi] = n.to_array();
            posed.tangents[vi] = [tv.x, tv.y, tv.z, t[3]];
        }
        if let Some(push) = &push {
            for (p, sv) in posed.positions.iter_mut().zip(&d.renderer.mesh.skin) {
                *p = push.apply(Vec3::from(*p), push.share(sv.bones, sv.weights)).to_array();
            }
        }
        if std::env::var("RK_NET_DEBUG").is_ok() && !extra.is_empty() {
            extra.sort_by(f32::total_cmp);
            let q = |f: f32| extra[((extra.len() as f32 - 1.0) * f) as usize] * 100.0;
            eprintln!(
                "dress {} {} us, skinning over the net cm p50 {:.1} p90 {:.1} max {:.1}",
                d.renderer.name,
                began.elapsed().as_micros(),
                q(0.5),
                q(0.9),
                q(1.0)
            );
        }
        if let Ok(dir) = std::env::var("RK_NET_MESH") {
            let mut text = String::new();
            for p in &posed.positions {
                text += &format!("v {} {} {}\n", p[0], p[1], p[2]);
            }
            for t in d.renderer.mesh.indices.chunks_exact(3) {
                text += &format!("f {} {} {}\n", t[0] + 1, t[1] + 1, t[2] + 1);
            }
            let _ = std::fs::write(format!("{dir}/mesh{ri}.obj"), text);
        }
        dress_posed(d, posed);
    }
}

/// Hands vertices already in the world to the renderer: one identity
/// matrix for all of them, and a culling box around them.
fn dress_posed(d: &mut DrawMesh<'_>, posed: crate::scene::Posed) {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &posed.positions {
        lo = lo.min(Vec3::from(*p));
        hi = hi.max(Vec3::from(*p));
    }
    if let Some(b) = &mut d.bounds {
        *b = [Vec3::from(b[0]).min(lo).to_array(), Vec3::from(b[1]).max(hi).to_array()];
    }
    d.base_positions = Cow::Owned(posed.positions);
    d.base_normals = Cow::Owned(posed.normals);
    d.base_tangents = Some(posed.tangents);
    d.skinning = Skinning::Rigid {
        matrix: Mat4::IDENTITY,
        normal: Mat4::IDENTITY,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Node, Trs};

    /// A scene of one node at the origin, and a net round the y axis that
    /// rides it: a cone from `top` wide at the waist to `hem` wide,
    /// `length` long.
    fn cone(cols: usize, rows: usize, top: f32, hem: f32, length: f32) -> (Scene, Net) {
        let mut scene = Scene::default();
        scene.nodes.push(Node {
            name: "waist".into(),
            parent: None,
            children: Vec::new(),
            local: Trs::default(),
            active: true,
        });
        let mut rest = Vec::new();
        for r in 0..rows {
            let f = r as f32 / (rows - 1) as f32;
            let radius = top + (hem - top) * f;
            for c in 0..cols {
                let a = c as f32 / cols as f32 * std::f32::consts::TAU;
                rest.push(Vec3::new(a.cos() * radius, 1.0 - f * length, a.sin() * radius));
            }
        }
        let mut reach = vec![0.0; cols * rows];
        for r in 1..rows {
            for c in 0..cols {
                reach[r * cols + c] = REACH * length * r as f32 / (rows - 1) as f32;
            }
        }
        let net = Net {
            cols,
            rows,
            nodes: vec![0],
            waist: 0,
            waist_before: Mat4::IDENTITY,
            src: rest.iter().map(|&p| [(0, p, 1.0), (0, p, 0.0)]).collect(),
            reach,
            links: links(cols, rows),
            chain_nodes: vec![0],
            pos: rest.clone(),
            vel: vec![Vec3::ZERO; rest.len()],
            target: rest.clone(),
            before: rest.clone(),
            rest_on_waist: rest.clone(),
            rest,
            ready: false,
            active: false,
            nearby: Nearby::default(),
        };
        (scene, net)
    }

    fn run(scene: &Scene, net: &mut Net, capsules: &[(WorldCapsule, f32)], floor: Option<(Vec3, Vec3)>, frames: u32) {
        for _ in 0..frames {
            net.step(scene, true, capsules, floor, None, 1.0 / 60.0);
        }
    }

    #[test]
    fn a_hanging_net_stays_on_its_target() {
        let (scene, mut net) = cone(16, 8, 0.12, 0.2, 0.4);
        run(&scene, &mut net, &[], None, 120);
        for (p, t) in net.pos.iter().zip(&net.target) {
            assert!((*p - *t).length() < 0.01, "{p} is off {t}");
        }
    }

    #[test]
    fn a_net_is_kept_off_the_floor_and_keeps_its_lengths() {
        // The floor is 15 cm above the hem.
        let (scene, mut net) = cone(16, 8, 0.12, 0.2, 0.4);
        let floor = Some((Vec3::new(0.0, 0.75, 0.0), Vec3::Y));
        run(&scene, &mut net, &[], floor, 120);
        for p in &net.pos[net.cols..] {
            assert!(p.y >= 0.75 + THICKNESS - 1e-4, "{p} is under the floor");
        }
        for &(a, b, kind) in &net.links {
            if kind == DOWN || kind == ACROSS {
                let now = (net.pos[a as usize] - net.pos[b as usize]).length();
                let rest = (net.rest[a as usize] - net.rest[b as usize]).length();
                assert!(now < rest * 1.2, "a link of {rest} is {now} long");
            }
        }
    }

    #[test]
    fn a_limb_under_a_level_skirt_holds_it_up() {
        // A thigh sticks out level under the front of the skirt: the net
        // stays out of it and hangs down beside it.
        let (scene, mut net) = cone(16, 8, 0.12, 0.2, 0.4);
        let thigh = WorldCapsule {
            a: Vec3::new(0.0, 0.9, 0.0),
            b: Vec3::new(0.5, 0.9, 0.0),
            ra: 0.07,
            rb: 0.07,
        };
        run(&scene, &mut net, &[(thigh, 0.0)], None, 120);
        for p in net.pos.iter().skip(2 * net.cols) {
            let (on, r) = on_axis(*p, &thigh);
            assert!((*p - on).length() > r - 2e-3, "{p} is inside the thigh");
        }
    }

    #[test]
    fn a_point_may_be_as_deep_in_a_capsule_as_its_target_up_to_the_give() {
        let cap = WorldCapsule {
            a: Vec3::ZERO,
            b: Vec3::X,
            ra: 0.1,
            rb: 0.1,
        };
        let r = 0.1 + THICKNESS;
        // No give: out to the surface.
        let mut p = Vec3::new(0.5, 0.05, 0.0);
        let aim = p;
        let (n, depth) = push_out(&mut p, aim, &cap, 0.0).expect("pushed");
        assert!((p.y - r).abs() < 1e-6 && n == Vec3::Y && (depth - (r - 0.05)).abs() < 1e-6);
        // The target is 2 cm deep and that much is given: left there.
        let aim = Vec3::new(0.5, r - 0.02, 0.0);
        let mut p = aim;
        assert!(push_out(&mut p, aim, &cap, 0.03).is_none());
        // Deeper than the give allows: out to the give.
        let mut p = Vec3::new(0.5, 0.01, 0.0);
        push_out(&mut p, Vec3::new(0.5, 0.0, 0.0), &cap, 0.03);
        assert!((p.y - (r - 0.03)).abs() < 1e-6);
    }

    /// A flat net of one cell row, and a vertex bound a little off it.
    fn patch() -> (NetShape, Bound) {
        let points = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(2.0, -1.0, 0.0),
        ];
        let shape = NetShape {
            active: true,
            pos: frames(&points, 3, 2),
            target: frames(&points, 3, 2),
        };
        let mut bound = Bound {
            vertex: 0,
            net: 0,
            corners: [0, 1, 4],
            bary: [0.25, 0.25, 0.5],
            weight: 1.0,
            local: [Vec3::ZERO; 3],
        };
        let (at, turn) = surface(&shape.target, &bound);
        let back = turn.transpose();
        let rest = at + turn.z_axis * 0.02;
        bound.local = [back * (rest - at), back * turn.z_axis, back * turn.x_axis];
        (shape, bound)
    }

    #[test]
    fn a_net_on_its_target_leaves_the_vertex_as_the_game_skins_it() {
        let (shape, bound) = patch();
        // The game's skinning puts the vertex somewhere else than the
        // net's own carry would.
        let skinned = [Vec3::new(0.8, -0.4, 0.07), Vec3::Z, Vec3::X];
        let ([p, n, t], extra) = carry(&bound, &shape, skinned).expect("a frame");
        assert!((p - skinned[0]).length() < 1e-6);
        assert!((n - Vec3::Z).length() < 1e-6 && (t - Vec3::X).length() < 1e-6);
        assert!(extra > 0.01);
    }

    #[test]
    fn a_net_far_from_its_target_carries_the_vertex_as_it_rests_on_the_net() {
        let (mut shape, bound) = patch();
        // The net is moved and turned a quarter round x.
        let turn = Mat3::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let moved: Vec<Vec3> = shape
            .target
            .iter()
            .map(|f| turn * f[0] + Vec3::new(0.0, 0.0, 1.0))
            .collect();
        shape.pos = frames(&moved, 3, 2);
        let (rest, rest_turn) = surface(&shape.target, &bound);
        let rest = rest + rest_turn.z_axis * 0.02;
        let skinned = [Vec3::new(0.8, -0.4, 0.3), Vec3::Z, Vec3::X];
        let ([p, n, _], _) = carry(&bound, &shape, skinned).expect("a frame");
        let want = turn * rest + Vec3::new(0.0, 0.0, 1.0);
        assert!((p - want).length() < 1e-5, "{p} is not {want}");
        assert!((n - turn * Vec3::Z).length().min((n + turn * Vec3::Z).length()) < 1e-5);
    }

    #[test]
    fn the_hem_tucked_under_by_one_cell_keeps_its_side_up() {
        // Three rows; the last row is folded back under the second.
        let mut points = Vec::new();
        for y in [0.0, -1.0, -0.7] {
            for x in [0.0, 1.0, 2.0] {
                points.push(Vec3::new(x, y, 0.0));
            }
        }
        let f = frames(&points, 3, 3);
        assert!(f[7][2].y < 0.0, "the hem's way down is {}", f[7][2]);
    }

    #[test]
    fn links_join_each_point_to_its_neighbours_round_the_ring() {
        let l = links(4, 3);
        assert!(l.contains(&(3, 0, ACROSS)));
        assert!(l.contains(&(0, 4, DOWN)));
        assert!(l.contains(&(0, 8, BEND_DOWN)));
        assert!(!l.iter().any(|&(a, b, _)| a == b));
        // Per column: 2 down, 3 across, 4 diagonals, 1 and 3 bends.
        assert_eq!(l.len(), 4 * (2 + 3 + 4 + 1 + 3));
    }
}
