// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Advanced Skirt Physics' cloth net: an invisible grid of points per
//! skirt that every layer of the garment follows, so that a long dress can
//! fold and drape where the game's few stiff chains stand out like boards.
//! Renkatsu's own, not the game's.
//!
//! - The net is built at load from the skirt's chains as they rest: the
//!   chains go round the waist, with columns between each two that
//!   follow the skirt's round shape and rows down them, `SPACING` apart,
//!   and on past the chains' ends as far as the cloth hangs.
//! - It remembers the shape it settles to under gravity at load
//!   (`settle`), bends by angle (`Bend`) and moves among a body that is
//!   taken from one frame's pose to the next within the step (`Body`).
//!   What follows on targets is older.
//! - Each frame its target is where the chains put the skirt after the
//!   game's step and the Advanced pass: every point rides the chain bones
//!   on either side of it, as the mesh is skinned to them.
//! - An XPBD solver moves the net from there: it does not stretch or get
//!   shorter, gives way round the skirt when pushed together, resists
//!   bending, falls under gravity and collides with the body's shells
//!   (`shell`), the floor and the map with friction; the middles of its
//!   links collide with the body too, so that a limb does not come
//!   through a cell.
//! - The target's shape is held where it hangs down and nothing is in
//!   its way. Where the chains stick out, or the body, the floor or the
//!   map is in the way of the target, the cloth from there down is free:
//!   gravity has it, and only a pull across gravity keeps it over the
//!   ground its target is over.
//! - Every vertex weighted to the chains is bound to the rest net: its
//!   place, normal and tangent in the frame of the net's surface there.
//!   Each frame it is put back in that frame on the moved net. What the
//!   game's skinning adds to that on the target is added too, and fades
//!   as the net leaves the target: a net on its target leaves the mesh
//!   exactly as the game skins it, a net away from it gives cloth.

use super::cloth::{rest_on, Nearby, Obstacles};
use super::shell::{Shell, WorldShell};
use super::{world, Bone, Particle};
use crate::scene::{DrawMesh, Scene, Skinning};
use glam::{Mat3, Mat4, Quat, Vec3};
use std::borrow::Cow;
use std::sync::Arc;

/// How far apart the net's points are, down the skirt and round its
/// hem: the rows and the columns between each two chains are as many as
/// it takes. Every constant of the solver is a length, a time or a
/// property of the cloth, so that the spacing changes how fine the folds
/// are and not how the cloth behaves.
const SPACING: f32 = 0.043;
/// The fewest and the most rows, and the most columns between each two
/// chains.
const ROWS: (usize, usize) = (6, 64);
const BETWEEN: usize = 12;
/// Solver steps per frame.
const SUBSTEPS: u32 = 8;
const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);
/// How far the innermost layer of the garment keeps off the skin and
/// the floor.
const THICKNESS: f32 = 0.006;
/// How far a vertex on the net is kept off the skin after it is put
/// there.
const VERTEX_GAP: f32 = 0.003;
/// How much of what a layer stands off the net by goes where the net is
/// pressed on something, and how much of being pressed is left a step
/// after the net came off.
const FLATTEN: f32 = 0.8;
const PRESSED_FADE: f32 = 0.7;
/// The most a layer's own distance from the net changes how deep the net
/// may go.
const LAYER: f32 = 0.03;
/// How far a point may leave its target, as a share of its way down from
/// the waist.
const REACH: f32 = 1.5;
/// How far down from the waist the net is fixed to the body: the top of
/// a skirt sits on the hips and only what hangs below them is cloth.
const FIXED: f32 = 0.06;
/// The skirt's memory of its shape: each link down the skirt is turned
/// back towards its rest direction, as the waist carries it, as hard as
/// it takes to hold this length of cloth out level, at the top of the
/// free cloth and at the hem. Gravity and the body do the rest.
const RESTORE: (f32, f32) = (0.2, 0.04);
/// How much heavier a point pressed on something is than a free one,
/// less one: the cloth next to it does not pull it off the body.
const HEAVY: f32 = 3.0;
/// A point is caught between two things when the pushes it got add up to
/// less than this share of their lengths.
const PINCH: f32 = 0.5;
/// Compliances of the lengths along and across and of the diagonals when
/// pulled apart.
const STRETCH: f32 = 0.0;
const SHEAR: f32 = 1e-6;
/// The cloth's bending length down the skirt and round it: a strip this
/// long held out level by one end droops to a curve of about twice this
/// radius (bending stiffness over weight per area = g times its cube).
const BEND: f32 = 0.04;
const BEND_ACROSS: f32 = 0.06;
/// The tightest the cloth folds, as the radius of the fold.
const FOLD: f32 = 0.012;
/// The link length the compliances below were found with: a link of
/// another length takes them by its square, as the weight of the cloth
/// it carries and its own give go.
const LINK: f32 = 0.043;
/// Compliance of a length across or a diagonal pushed together: the net
/// gives way round the skirt, where real cloth closes into folds smaller
/// than a cell, and its bending keeps it smooth.
const SQUEEZE: f32 = 0.1;
/// The same down the skirt: cloth does not get shorter, it buckles.
const SQUEEZE_DOWN: f32 = 1e-5;
/// The least share of its length a link is pushed together to, across
/// and down: the net does not fold onto itself.
const LEAST: f32 = 0.4;
const LEAST_DOWN: f32 = 0.8;
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
/// The fastest a point of the net goes, in metres per second: what a
/// collision throws it out of does not send it flying.
const FASTEST: f32 = 4.0;
/// Rows whose reach is below this do not collide: near the waist the
/// target wins over a rough fit of the hips.
const TIGHT: f32 = 0.02;
/// A part of the body that moves further than this in one step, as seen
/// from the waist, starts the net again.
const JUMP: f32 = 0.25;
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

/// Steps the body is moved into its pose over when a net starts, and
/// the time each stands for.
const WARM_UP: u32 = 40;
const WARM_UP_STEP: f32 = 1.0 / 60.0;
/// Steps a net is left to hang for when it is settled, and its bending
/// length round the skirt meanwhile: far shorter than when it runs, so
/// that the ring closes in as cloth falling into folds does.
const SETTLE_STEPS: u32 = 180;
const SETTLE_BEND: f32 = 0.02;
/// How much a skirt stands out, from limp (0, it hangs as soft cloth) to
/// full (1, it keeps the shape it was drawn in). To become a setting per
/// garment.
const FLUFF: f32 = 0.0;

/// The transform `f` of the way from one to another: the place and the
/// size straight, the turn the short way round.
fn between(from: &Mat4, to: &Mat4, f: f32) -> Mat4 {
    let (s0, r0, t0) = from.to_scale_rotation_translation();
    let (s1, r1, t1) = to.to_scale_rotation_translation();
    Mat4::from_scale_rotation_translation(s0.lerp(s1, f), r0.slerp(r1, f), t0.lerp(t1, f))
}

/// The kinds of link, which give its compliance and its length.
/// Bits per point of `Net::open`: no cloth to the next point round the
/// skirt, and none at the point at all.
const OPEN_ROUND: u8 = 1;
const OPEN_POINT: u8 = 2;
const DOWN: u8 = 0;
const ACROSS: u8 = 1;
const DIAGONAL: u8 = 2;

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
        }
    }
    links
}

/// A bend of the net at a point: the angle between the links to its two
/// neighbours along a column (`round` false) or a row, measured round
/// the line through two more points (`axis`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Bend {
    ends: [u32; 2],
    at: u32,
    axis: [u32; 2],
    round: bool,
}

/// The bends of a ring grid: one down the skirt and one round it per
/// point that has both neighbours that way. `open` cuts the ring where
/// the garment has no cloth (see `Net::open`).
fn bends(cols: usize, rows: usize, open: &[u8]) -> Vec<Bend> {
    let at = |c: usize, r: usize| (r * cols + (c % cols)) as u32;
    let joined = |i: u32, way: u8| open.get(i as usize).is_none_or(|o| o & way == 0);
    let mut bends = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let (here, next, last) = (at(c, r), at(c + 1, r), at(c + cols - 1, r));
            if !joined(here, OPEN_POINT) {
                continue;
            }
            // Cloth to the next point round the skirt, and from the last.
            let (on, back) = (joined(here, OPEN_ROUND), joined(last, OPEN_ROUND));
            if r >= 1 && r + 1 < rows && joined(at(c, r - 1), OPEN_POINT) && joined(at(c, r + 1), OPEN_POINT) {
                let axis = match (on, back) {
                    (true, false) => [here, next],
                    (false, true) => [last, here],
                    _ => [last, next],
                };
                bends.push(Bend {
                    ends: [at(c, r - 1), at(c, r + 1)],
                    at: here,
                    axis,
                    round: false,
                });
            }
            if on && back {
                let (up, down) = (r.saturating_sub(1), (r + 1).min(rows - 1));
                let down = if joined(at(c, down), OPEN_POINT) { down } else { r };
                let up = if joined(at(c, up), OPEN_POINT) { up } else { r };
                if up != down {
                    bends.push(Bend {
                        ends: [last, next],
                        at: here,
                        axis: [at(c, up), at(c, down)],
                        round: true,
                    });
                }
            }
        }
    }
    bends
}

/// The angle a bend is at, with the line it is measured round and the
/// two links as seen along that line. None where the net is too crumpled
/// to tell.
fn bend_angle(pos: &[Vec3], b: &Bend) -> Option<(f32, Vec3, [Vec3; 2], [Vec3; 2])> {
    let here = pos[b.at as usize];
    let (d1, d2) = (here - pos[b.ends[0] as usize], pos[b.ends[1] as usize] - here);
    let along = d1 + d2;
    let axis = pos[b.axis[1] as usize] - pos[b.axis[0] as usize];
    let len = along.length_squared();
    if len < 1e-12 {
        return None;
    }
    let a = (axis - along * (axis.dot(along) / len)).normalize_or_zero();
    if a == Vec3::ZERO {
        return None;
    }
    let (e1, e2) = (d1 - a * d1.dot(a), d2 - a * d2.dot(a));
    if e1.length_squared() < 1e-12 || e2.length_squared() < 1e-12 {
        return None;
    }
    Some((e1.cross(e2).dot(a).atan2(e1.dot(e2)), a, [e1, e2], [d1, d2]))
}

/// One bend at rest: its angle, how easily it gives (XPBD compliance for
/// points of weight one) and the most it may fold either way.
#[derive(Debug, Clone, Copy)]
struct BendRest {
    angle: f32,
    compliance: f32,
    most: f32,
}

/// One pass over the bends: each is turned towards its rest angle by its
/// stiffness, and back to the tightest fold when it is past it.
fn bends_pass(bends: &[Bend], rest: &[Option<BendRest>], light: &[f32], pos: &mut [Vec3], h: f32) {
    for (b, rest) in bends.iter().zip(rest) {
        let Some(rest) = rest else { continue };
        let (i0, i1, i2) = (b.ends[0] as usize, b.at as usize, b.ends[1] as usize);
        let (w0, w1, w2) = (light[i0], light[i1], light[i2]);
        if w0 + w1 + w2 == 0.0 {
            continue;
        }
        for limit in [false, true] {
            let Some((angle, a, [e1, e2], [d1, d2])) = bend_angle(pos, b) else {
                break;
            };
            let (off, alpha) = if limit {
                if angle.abs() <= rest.most {
                    break;
                }
                (angle - rest.most.copysign(angle), 0.0)
            } else {
                (angle - rest.angle, rest.compliance / (h * h))
            };
            // The turn of each link per move of its end. By the whole
            // link's length, not its length as seen along the line: a
            // link that comes to lie along the line is then not flung.
            let (g0, g2) = (a.cross(e1) / d1.length_squared(), a.cross(e2) / d2.length_squared());
            let g1 = -(g0 + g2);
            let sum = w0 * g0.length_squared() + w1 * g1.length_squared() + w2 * g2.length_squared();
            // One pass per step gives as a spring of the points' own
            // weight over the step squared would on top of the bend's
            // own give: that much is taken off it, so that the cloth is
            // as stiff as it is meant to be however fine the net.
            let give = alpha.max(sum);
            if give < 1e-12 {
                break;
            }
            let lambda = -off / give;
            pos[i0] += g0 * (lambda * w0);
            pos[i1] += g1 * (lambda * w1);
            pos[i2] += g2 * (lambda * w2);
        }
    }
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
    /// The angles that are kept.
    bends: Vec<Bend>,
    /// Every node of the skirt's chains, which vertices are bound by.
    pub(super) chain_nodes: Vec<usize>,
    /// The net as it was built, in the world.
    rest: Vec<Vec3>,
    /// The same in the space of the waist node.
    rest_on_waist: Vec<Vec3>,
    /// The shape the net remembers, in the space of the waist node: the
    /// rest shape, or what it settled to under gravity (`settle`).
    shape_on_waist: Vec<Vec3>,
    /// The rest shape carried by the waist now, which gives the cloth
    /// its lengths.
    full: Vec<Vec3>,
    /// Whether the net is being settled: no memory, soft round the
    /// skirt.
    settling: bool,
    pos: Vec<Vec3>,
    vel: Vec<Vec3>,
    /// Where the game's chains put the net, which it aims for.
    target: Vec<Vec3>,
    /// Per point: how far off the skin the net is kept, which puts the
    /// innermost layer bound there `THICKNESS` off it.
    gap: Vec<f32>,
    /// 1 when the net's surface (round cross down) faces out, else -1.
    outward: f32,
    /// Rows from the waist down that are fixed to the body.
    fixed: usize,
    /// Per point: where the garment has no cloth (`OPEN_ROUND`,
    /// `OPEN_POINT`), as with the two flaps of a slit dress, whose net
    /// is a ring all the same. Empty when it has cloth all round.
    open: Vec<u8>,
    /// Per point: whether it is pressed on something, 0 to 1, which
    /// fades over a few steps.
    pressed: Vec<f32>,
    /// Per point: how far its link from above is turned back to its
    /// rest direction per step; 1 for a fixed point.
    hold: Vec<f32>,
    /// Per body shell: its node's place as fitted, in the waist's space.
    pub(super) shell_rest: Vec<Mat4>,
    /// Per body shell: its middle at the step before, in the waist's
    /// space.
    shell_before: Vec<Vec3>,
    /// Per body shell: its node's place in the world at the step before,
    /// which the body is moved on from within a step.
    shell_was: Vec<Mat4>,
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

/// The body a net moves among during one step.
enum Body<'a> {
    /// As it is.
    Still(&'a [WorldShell]),
    /// The shells with their nodes' places in the world at the start of
    /// the step and at its end: the body is moved from one to the other
    /// over the solver's steps, so that a fast limb pushes the cloth as
    /// fast as it goes and not all at once.
    Moving {
        shells: &'a [Shell],
        from: &'a [Mat4],
        to: &'a [Mat4],
    },
}

impl Body<'_> {
    /// Where each shell's node is `k` of the way through the step; none
    /// for a body that stays as it is.
    fn places(&self, k: f32) -> Option<Vec<Mat4>> {
        match self {
            Body::Still(_) => None,
            Body::Moving { from, to, .. } => Some(
                from.iter()
                    .zip(*to)
                    .map(|(a, b)| if a == b { *b } else { between(a, b, k) })
                    .collect(),
            ),
        }
    }

    /// The body with its shells' nodes at `places`.
    fn at(&self, places: Option<&[Mat4]>) -> Cow<'_, [WorldShell]> {
        match (self, places) {
            (Body::Moving { shells, .. }, Some(places)) => {
                Cow::Owned(shells.iter().zip(places).map(|(s, m)| s.in_world(m)).collect())
            }
            (Body::Still(bodies), _) => Cow::Borrowed(bodies),
            (Body::Moving { .. }, None) => Cow::Owned(Vec::new()),
        }
    }

    /// The body at the end of the step.
    fn last(&self) -> Cow<'_, [WorldShell]> {
        self.at(self.places(1.0).as_deref())
    }
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

        // As many rows and columns as put the points `SPACING` apart
        // down the skirt and round its hem.
        let spacing = tune("spacing", SPACING).max(0.005);
        let length = chains
            .iter()
            .map(|c| c.points.windows(2).map(|w| (w[1] - w[0]).length()).sum::<f32>())
            .sum::<f32>()
            / n;
        let round: f32 = (0..chains.len())
            .map(|j| {
                let hem = |c: &Chain| *c.points.last().expect("two or more");
                (hem(&chains[j]) - hem(&chains[(j + 1) % chains.len()])).length()
            })
            .sum();
        // The garment may hang past the ends of its chains: the net
        // goes on straight below them as far as the cloth does.
        chain_nodes.sort_unstable();
        chain_nodes.dedup();
        let ends: Vec<(Vec3, Vec3)> = chains
            .iter()
            .map(|c| {
                let end = *c.points.last().expect("two or more");
                (end, (end - c.points[c.points.len() - 2]).normalize_or_zero())
            })
            .collect();
        let past = if tune("past", 1.0) != 0.0 {
            past_the_ends(scene, &chain_nodes, &ends)
        } else {
            0.0
        };
        let (between, rows) = grid(length + past, round / n, spacing);
        let between = tune("between", between as f32) as usize;
        let cols = chains.len() * (between + 1);
        let rows = tune("rows", rows as f32) as usize;
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
                .zip(&ends)
                .map(|(c, &(end, on))| {
                    let own: f32 = c.points.windows(2).map(|w| (w[1] - w[0]).length()).sum();
                    let down = share * (own + past);
                    if down > own {
                        return (end + on * (down - own), *c.nodes.last().expect("two or more"));
                    }
                    let (seg, t) = along(&c.points, down / own.max(1e-6));
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
        let bends = bends(cols, rows, &[]);
        if std::env::var("RK_NET_DEBUG").is_ok() {
            let row = |r: usize| (rest[r * cols] - rest[r * cols + 1]).length() * 100.0;
            let down = (rest[0] - rest[cols]).length() * 100.0;
            eprintln!(
                "net build {cols}x{rows} chains {} past {past:.3} cell cm: across top {:.1} hem {:.1} down {:.1} length {:.0}",
                chains.len(),
                row(0),
                row(rows - 1),
                down,
                reach[(rows - 1) * cols] / REACH * 100.0
            );
        }
        let fixed = fixed_rows(&reach, cols, rows);
        let rest_on_waist: Vec<Vec3> = {
            let back = world(scene, waist).inverse();
            rest.iter().map(|&p| back.transform_point3(p)).collect()
        };
        Some(Net {
            cols,
            rows,
            nodes,
            waist,
            waist_before: world(scene, waist),
            shape_on_waist: rest_on_waist.clone(),
            rest_on_waist,
            full: rest.clone(),
            settling: false,
            src,
            reach,
            links,
            bends,
            chain_nodes,
            pos: rest.clone(),
            vel: vec![Vec3::ZERO; rest.len()],
            target: rest.clone(),
            gap: vec![THICKNESS; rest.len()],
            outward: outward(&rest, cols, rows),
            fixed,
            open: Vec::new(),
            hold: vec![0.0; rest.len()],
            pressed: vec![0.0; rest.len()],
            shell_rest: Vec::new(),
            shell_before: Vec::new(),
            shell_was: Vec::new(),
            before: rest.clone(),
            rest,
            ready: false,
            active: false,
            nearby: Nearby::default(),
        })
    }

    /// Notes where the body's shells are around the net as it rests:
    /// call in the pose both were made in.
    pub(super) fn fitted(&mut self, scene: &Scene, shells: &[Shell]) {
        let back = world(scene, self.waist).inverse();
        self.shell_rest = shells.iter().map(|s| back * world(scene, s.node)).collect();
        if tune("settle", 1.0) != 0.0 {
            self.settle(scene, shells);
        }
    }

    /// Lets the net hang from its fixed top under gravity in the pose it
    /// was built in, round the body, and takes what it comes to as the
    /// shape it remembers. The cloth keeps its lengths: an A-line cut
    /// then hangs in closer than it was drawn, as soft cloth does, and
    /// still opens to its full width when something spreads it.
    fn settle(&mut self, scene: &Scene, shells: &[Shell]) {
        let waist = world(scene, self.waist);
        self.shape_on_waist.clone_from(&self.rest_on_waist);
        self.aim(scene, &waist);
        self.pos.clone_from(&self.target);
        self.before.clone_from(&self.target);
        self.vel.fill(Vec3::ZERO);
        self.pressed.fill(0.0);
        let bodies: Vec<WorldShell> = shells.iter().map(|s| s.in_world(&world(scene, s.node))).collect();
        self.settling = true;
        self.waist_before = waist;
        for _ in 0..tune("settle_steps", SETTLE_STEPS as f32) as u32 {
            self.advance(&Body::Still(&bodies), &waist, None, None, WARM_UP_STEP);
        }
        self.settling = false;
        let back = waist.inverse();
        self.shape_on_waist = self.pos.iter().map(|&p| back.transform_point3(p)).collect();
        self.vel.fill(Vec3::ZERO);
        self.pressed.fill(0.0);
        self.ready = false;
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
        for m in &mut self.shell_was {
            *m = moved * *m;
        }
    }

    /// Where the net rests now: its rest shape, carried by the waist.
    /// Nothing of the game's chains is read: they only gave the net its
    /// layout when it was built. (`RK_NET_P=chains=1` aims at the chains
    /// instead, to compare.)
    fn aim(&mut self, scene: &Scene, waist: &Mat4) {
        if tune("chains", 0.0) != 0.0 {
            let mats: Vec<Mat4> = self.nodes.iter().map(|&n| world(scene, n)).collect();
            for (t, src) in self.target.iter_mut().zip(&self.src) {
                *t = src
                    .iter()
                    .map(|&(n, p, w)| mats[n as usize].transform_point3(p) * w)
                    .sum();
            }
            self.full.clone_from(&self.target);
            return;
        }
        for (t, r) in self.target.iter_mut().zip(&self.shape_on_waist) {
            *t = waist.transform_point3(*r);
        }
        for (t, r) in self.full.iter_mut().zip(&self.rest_on_waist) {
            *t = waist.transform_point3(*r);
        }
    }

    /// One frame of `dt` seconds, after the chains moved. `active` false
    /// leaves the skirt as the chains put it. `shells` is the body as it
    /// was fitted and `bodies` the same in the world now.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn step(
        &mut self,
        scene: &Scene,
        active: bool,
        shells: &[Shell],
        bodies: &[WorldShell],
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
        let waist = world(scene, self.waist);
        self.aim(scene, &waist);
        // A limb that jumped, as when a pose is put on at once, would
        // go through the cloth: the net starts again.
        let back = waist.inverse();
        let places: Vec<Vec3> = bodies.iter().map(|s| back.transform_point3(s.centre())).collect();
        let jumped = places.len() == self.shell_before.len()
            && places
                .iter()
                .zip(&self.shell_before)
                .any(|(p, b)| (*p - *b).length_squared() > JUMP * JUMP);
        self.shell_before = places;
        let now: Vec<Mat4> = shells.iter().map(|s| world(scene, s.node)).collect();
        if !self.ready || jumped {
            self.start(scene, &waist, shells);
            self.shell_was.clone_from(&now);
        }
        if dt > 0.0 {
            // The net goes along with the waist (in `advance`): only the
            // motion relative to it is simulated, so that a turn of the
            // whole body does not leave the skirt behind.
            let was = self.shell_was.clone();
            let body = if now.len() == bodies.len() && was.len() == now.len() && tune("moving", 1.0) != 0.0 {
                Body::Moving {
                    shells,
                    from: &was,
                    to: &now,
                }
            } else {
                Body::Still(bodies)
            };
            self.advance(&body, &waist, floor, obstacles, dt);
        }
        self.shell_was = now;
        self.waist_before = waist;
        self.before.clone_from(&self.target);
        if std::env::var("RK_NET_FREEZE").is_ok() {
            self.pos.clone_from(&self.target);
        }
        let (cols, rows) = (self.cols, self.rows);
        self.dump(bodies, &waist, "");
        if std::env::var("RK_NET_DEBUG").is_ok() {
            let lines: Vec<String> = (0..rows)
                .map(|r| {
                    let (mut sum, mut max, mut held) = (0.0f32, 0.0f32, 0.0f32);
                    for c in 0..cols {
                        let d = self.pos[r * cols + c] - self.target[r * cols + c];
                        sum += d.length();
                        max = max.max(d.length());
                        held += self.hold[r * cols + c];
                    }
                    format!(
                        "{:.0}/{:.0}/{:.0}%",
                        sum / cols as f32 * 100.0,
                        max * 100.0,
                        held / cols as f32 * 100.0
                    )
                })
                .collect();
            eprintln!(
                "net {cols}x{rows} {} us shells {} tris {} mean/max cm/held: {}",
                began.elapsed().as_micros(),
                bodies.len(),
                self.nearby.triangles.len(),
                lines.join(" ")
            );
        }
    }

    /// Starts the net again in the pose the body is in now. The net
    /// begins as it rests on the waist, with the body as it was fitted
    /// around it, and the body is then moved into its pose over
    /// `WARM_UP` steps: the legs carry the cloth with them as they would
    /// have, so that it ends up on the right side of them whatever the
    /// pose, which a net put straight on its target does not.
    fn start(&mut self, scene: &Scene, waist: &Mat4, shells: &[Shell]) {
        for (p, r) in self.pos.iter_mut().zip(&self.shape_on_waist) {
            *p = waist.transform_point3(*r);
        }
        let cols = self.cols;
        self.pos[..cols].copy_from_slice(&self.target[..cols]);
        self.before.clone_from(&self.target);
        self.vel.fill(Vec3::ZERO);
        self.pressed.fill(0.0);
        self.waist_before = *waist;
        self.ready = true;
        let steps = tune("warm_up", WARM_UP as f32) as u32;
        if shells.len() != self.shell_rest.len() || steps == 0 {
            return;
        }
        let back = waist.inverse();
        let ends: Vec<(Mat4, Mat4)> = shells
            .iter()
            .zip(&self.shell_rest)
            .map(|(s, rest)| (*rest, back * world(scene, s.node)))
            .collect();
        let place = |f: f32| -> Vec<Mat4> { ends.iter().map(|(from, to)| *waist * between(from, to, f)).collect() };
        let mut from = place(0.0);
        for k in 1..=steps {
            let to = place(k as f32 / steps as f32);
            let body = Body::Moving {
                shells,
                from: &from,
                to: &to,
            };
            // The pose is taken in the air: the floor and the map are
            // there from the first step after it. With them there all
            // along, a body that lies down slides its legs off the cloth
            // they lay on.
            self.advance(&body, waist, None, None, WARM_UP_STEP);
            self.dump(&body.last(), waist, "w");
            from = to;
        }
    }

    /// Writes the net, its target and the body to `RK_NET_DUMP`, and
    /// with `RK_NET_P=frames=1` to a numbered copy per call.
    fn dump(&self, bodies: &[WorldShell], waist: &Mat4, tag: &str) {
        let Ok(path) = std::env::var("RK_NET_DUMP") else {
            return;
        };
        let frames = tune("frames", 0.0) > 0.0;
        if !tag.is_empty() && !frames {
            return;
        }
        let text: String = self
            .pos
            .iter()
            .zip(&self.target)
            .map(|(p, t)| format!("{} {} {} {} {} {}\n", p.x, p.y, p.z, t.x, t.y, t.z))
            .collect();
        let holds: String = self.pressed.iter().map(|k| format!("h {k}\n")).collect();
        let skin: String = bodies
            .iter()
            .enumerate()
            .flat_map(|(i, s)| s.surface().into_iter().map(move |p| (i, p)))
            .map(|(i, p)| format!("s {} {} {} {i}\n", p.x, p.y, p.z))
            .collect();
        let w: Vec<String> = waist.to_cols_array().iter().map(f32::to_string).collect();
        let (cols, rows) = (self.cols, self.rows);
        let _ = std::fs::write(&path, format!("{cols} {rows}\n{text}{holds}{skin}w {}\n", w.join(" ")));
        if frames {
            use std::sync::atomic::{AtomicU32, Ordering};
            static FRAME: AtomicU32 = AtomicU32::new(0);
            let f = FRAME.fetch_add(1, Ordering::Relaxed);
            let _ = std::fs::copy(&path, format!("{path}.{tag}{f:03}"));
        }
    }

    /// Moves the net on by `dt` seconds towards its target among the
    /// `body`, while the waist goes from where it was (`waist_before`) to
    /// `waist` and carries the net along.
    fn advance(
        &mut self,
        body: &Body,
        waist: &Mat4,
        floor: Option<(Vec3, Vec3)>,
        obstacles: Option<&Arc<Obstacles>>,
        dt: f32,
    ) {
        let (cols, rows) = (self.cols, self.rows);
        // The lengths the net keeps. The cloth's own are the rest
        // shape's (across the skirt no more than a little over the ones
        // it was built with); the lengths over one point, which keep its
        // bends, are those of the shape it remembers.
        let slack = tune("slack", SLACK);
        let lengths: Vec<f32> = self
            .links
            .iter()
            .map(|&(a, b, kind)| {
                let (a, b) = (a as usize, b as usize);
                let full = (self.full[a] - self.full[b]).length();
                match kind {
                    DOWN => full,
                    _ => full.min((self.rest[a] - self.rest[b]).length() * slack),
                }
            })
            .collect();
        // How far down the skirt each point is from the waist: it gets no
        // further from the top of its column than that.
        let mut tether = vec![0.0f32; cols * rows];
        for i in cols..cols * rows {
            tether[i] = tether[i - cols] + (self.full[i] - self.full[i - cols]).length();
        }
        // Per kind of link: the compliance pulled apart and pushed
        // together, and the least share of its length it is pushed to.
        let squeeze = tune("squeeze", SQUEEZE);
        let squeeze_down = tune("squeeze_down", SQUEEZE_DOWN);
        let bend = tune("bend", BEND);
        let bend_across = if self.settling {
            // Fluffier cloth is stiffer round the skirt while it
            // settles, up to as stiff as when it runs: it then stands
            // out as it was drawn.
            let fluff = tune("fluff", FLUFF).clamp(0.0, 1.0);
            let limp = tune("settle_bend", SETTLE_BEND);
            limp * (tune("bend_across", BEND_ACROSS) / limp).powf(fluff * fluff)
        } else {
            tune("bend_across", BEND_ACROSS)
        };
        let (least, least_down) = (tune("least", LEAST), tune("least_down", LEAST_DOWN));
        let kinds = [
            (STRETCH, squeeze_down, least_down),
            (STRETCH, squeeze, least),
            (SHEAR, squeeze, least),
        ];
        // The angles the net keeps are those of the shape it remembers.
        // A bend gives by the square of its links' length over the
        // cube of the cloth's bending length, which makes a strip of it
        // droop the same however many points it is made of.
        let fold = tune("fold", FOLD);
        let bend_rest: Vec<Option<BendRest>> = self
            .bends
            .iter()
            .map(|b| {
                let (angle, ..) = bend_angle(&self.target, b)?;
                let here = self.target[b.at as usize];
                let len = ((here - self.target[b.ends[0] as usize]).length()
                    + (here - self.target[b.ends[1] as usize]).length())
                    * 0.5;
                let length = if b.round { bend_across } else { bend };
                Some(BendRest {
                    angle,
                    compliance: len * len / (GRAVITY.length() * length.powi(3)),
                    most: (len / fold).min(2.6).max(angle.abs() + 0.3),
                })
            })
            .collect();
        let friction = tune("friction", FRICTION);
        let floor_friction = tune("floor_friction", FLOOR_FRICTION);
        let gravity = GRAVITY * tune("gravity", 1.0);
        let substeps = tune("substeps", SUBSTEPS as f32) as u32;
        let thickness = tune("thickness", 0.0);

        // What the net may meet: the body and the map near it.
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for (p, t) in self.pos.iter().zip(&self.target) {
            lo = lo.min(*p).min(*t);
            hi = hi.max(*p).max(*t);
        }
        let reach = self.reach.iter().copied().fold(0.0, f32::max);
        let (lo, hi) = (lo - Vec3::splat(reach + 0.1), hi + Vec3::splat(reach + 0.1));
        let nocaps = std::env::var("RK_NET_NOCAPS").is_ok();
        let with_arms = tune("arms", 1.0) != 0.0;
        let near = |s: &&WorldShell| {
            let (slo, shi) = s.bounds();
            !nocaps && slo.cmple(hi).all() && shi.cmpge(lo).all()
        };
        self.nearby.refresh(obstacles, lo, hi);
        if std::env::var("RK_NET_GROUND").is_ok() && !self.nearby.triangles.is_empty() {
            let depth = |p: Vec3| super::cloth::ground(p, &self.nearby.triangles, 0.4).map(|(y, _)| p.y - y);
            let rows_d: Vec<String> = [0, self.fixed, rows / 2, rows - 1]
                .iter()
                .map(|&r| {
                    let d: Vec<f32> = (0..cols).filter_map(|c| depth(self.target[r * cols + c])).collect();
                    let min = d.iter().copied().fold(f32::MAX, f32::min);
                    let under = d.iter().filter(|&&x| x < 0.0).count();
                    format!("row {r}: {under}/{} under, lowest {:.3}", d.len(), min)
                })
                .collect();
            let b = body.last();
            let shells_d: Vec<String> = b
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    let low = s.surface().into_iter().filter_map(depth).fold(f32::MAX, f32::min);
                    (low < 0.0).then(|| format!("{i}:{low:.3}"))
                })
                .collect();
            eprintln!(
                "ground target {} | shells under: {}",
                rows_d.join("; "),
                shells_d.join(" ")
            );
        }
        // The waist's way over this step, which the net is carried along
        // bit by bit: all at once it would be thrown into the floor or
        // a seat and bounce off it.
        let waist_from = self.waist_before;
        let carried = waist_from != *waist;
        let mut waist_at = waist_from;
        let mut places_before = body.places(0.0);

        // The skirt's memory of its shape: how far each link down it is
        // turned back to its rest direction per step, from the top of
        // the free cloth to the hem.
        let restore = if self.settling {
            (0.0, 0.0)
        } else {
            (tune("restore_top", RESTORE.0), tune("restore_hem", RESTORE.1))
        };
        let fixed = self.fixed * cols;
        let h = dt / substeps as f32;
        // Points the garment has no cloth at stay on the target, out of
        // everything's way.
        let none: Vec<bool> = (0..cols * rows)
            .map(|i| self.open.get(i).is_some_and(|o| o & OPEN_POINT != 0))
            .collect();
        let mut hold = std::mem::take(&mut self.hold);
        hold.resize(cols * rows, 0.0);
        for (i, k) in hold.iter_mut().enumerate() {
            let hem = tether[(rows - 1) * cols + i % cols].max(1e-6);
            *k = if i < fixed {
                1.0
            } else {
                // Per step a link is turned by the share that holds
                // that length of cloth: g times the length times the
                // step squared, over its own length squared.
                let held = restore.0 + (restore.1 - restore.0) * (tether[i] / hem);
                let link = (tether[i] - tether[i - cols]).max(1e-4);
                (GRAVITY.length() * held * h * h / (link * link)).min(0.5)
            };
        }
        self.hold = hold;
        // A point pressed on something is heavier.
        let heavy = tune("heavy", HEAVY);
        let light: Vec<f32> = (0..cols * rows)
            .map(|i| {
                if i < fixed || none[i] {
                    0.0
                } else {
                    1.0 / (1.0 + heavy * self.pressed[i])
                }
            })
            .collect();
        let again = tune("again", 1.0) != 0.0;
        let fastest = tune("fastest", FASTEST);
        let mut loose = light.clone();
        let damping = (-tune("damping", DAMPING) * h).exp();
        let mut column = vec![Vec3::ZERO; rows];
        let mut caught = vec![false; cols * rows];
        for step in 0..substeps {
            let k = (step + 1) as f32 / substeps as f32;
            if carried {
                let next = if step + 1 == substeps {
                    *waist
                } else {
                    between(&waist_from, waist, k)
                };
                let moved = next * waist_at.inverse();
                for p in &mut self.pos {
                    *p = moved.transform_point3(*p);
                }
                for v in &mut self.vel {
                    *v = moved.transform_vector3(*v);
                }
                waist_at = next;
            }
            // The body now, and how each part of it moved over this
            // step: what lies on a limb goes along with it.
            let places = body.places(k);
            let bodies = body.at(places.as_deref());
            let moves: Vec<Mat4> = match (&places, &places_before) {
                (Some(now), Some(before)) => now
                    .iter()
                    .zip(before)
                    .map(|(n, b)| if n == b { Mat4::IDENTITY } else { *n * b.inverse() })
                    .collect(),
                _ => vec![Mat4::IDENTITY; bodies.len()],
            };
            places_before = places;
            let body: Vec<(&WorldShell, &Mat4)> = bodies
                .iter()
                .zip(&moves)
                .filter(|(s, _)| (!s.outside || with_arms) && near(s))
                .collect();
            let aim: Vec<Vec3> = self
                .before
                .iter()
                .zip(&self.target)
                .map(|(b, t)| b.lerp(*t, k))
                .collect();
            let old = self.pos.clone();
            self.pos[..fixed].copy_from_slice(&aim[..fixed]);
            for (i, (p, v)) in self.pos.iter_mut().zip(&mut self.vel).enumerate().skip(fixed) {
                if none[i] {
                    *p = aim[i];
                    continue;
                }
                *v = (*v + gravity * h) * damping;
                *p += *v * h;
            }
            lengths_pass(&self.links, &lengths, &kinds, &light, &mut self.pos, h);
            bends_pass(&self.bends, &bend_rest, &light, &mut self.pos, h);
            loose.copy_from_slice(&light);
            caught.fill(false);
            for i in fixed..cols * rows {
                if none[i] {
                    continue;
                }
                // No further from the top of its column than the cloth
                // is long.
                let top = self.pos[i % cols];
                let off = self.pos[i] - top;
                let far = off.length();
                if far > tether[i] && far > 1e-9 {
                    self.pos[i] = top + off * (tether[i] / far);
                }
                // The link from the point above is turned back towards
                // its rest direction.
                let above = i - cols;
                let want = self.pos[above] + (aim[i] - aim[above]);
                let turned = (want - self.pos[i]) * self.hold[i];
                self.pos[i] += turned;
                if self.reach[i] < TIGHT {
                    continue;
                }
                let gap = self.gap[i] + thickness;
                // The deepest thing the point was pushed out of: its
                // direction, how far it pushed, its friction and where
                // the point would be now had it gone along with it.
                let mut touch: Option<(Vec3, f32, f32, Vec3)> = None;
                // Every push the point got, added up as moves and as
                // lengths: where they cancel it is caught between two
                // things. And where those things would have taken it,
                // added up.
                let (mut pushes, mut effort) = (Vec3::ZERO, 0.0f32);
                let (mut taken, mut takers) = (Vec3::ZERO, 0.0f32);
                for (s, moved) in &body {
                    let from = self.pos[i];
                    if let Some((n, depth)) = s.push_out(&mut self.pos[i], gap) {
                        let with = moved.transform_point3(old[i]);
                        pushes += self.pos[i] - from;
                        effort += (self.pos[i] - from).length();
                        taken += with;
                        takers += 1.0;
                        if touch.is_none_or(|t| depth > t.1) {
                            touch = Some((n, depth, friction, with));
                        }
                    }
                }
                if let Some((origin, up)) = floor {
                    let height = (self.pos[i] - origin).dot(up) - gap.max(0.0);
                    if height < 0.0 {
                        self.pos[i] -= up * height;
                        pushes -= up * height;
                        effort -= height;
                        taken += old[i];
                        takers += 1.0;
                        if touch.is_none_or(|t| -height > t.1) {
                            touch = Some((up, -height, floor_friction, old[i]));
                        }
                    }
                }
                if step + 1 == substeps {
                    let now: f32 = if touch.is_some() { 1.0 } else { 0.0 };
                    self.pressed[i] = now.max(self.pressed[i] * PRESSED_FADE);
                }
                loose[i] = if touch.is_some() { 0.0 } else { light[i] };
                // Cloth caught between two things that push against each
                // other (the body and the floor under it, two thighs, a
                // calf on a thigh, a hand on a lap) stays between them:
                // pushed out of one and then the other it would be walked
                // along between them step after step, out from under a
                // lying body and up to the waist. It goes where they take
                // it, half each. Only the floor still has its way.
                caught[i] = effort > 1e-6 && pushes.length() < effort * tune("pinch", PINCH);
                if caught[i] {
                    self.pos[i] = taken / takers.max(1.0);
                    if let Some((origin, up)) = floor {
                        let height = (self.pos[i] - origin).dot(up) - gap.max(0.0);
                        if height < 0.0 {
                            self.pos[i] -= up * height;
                        }
                    }
                    continue;
                }
                // Friction: a touching point loses as much of its slide
                // over what it touches as the push holds (Coulomb), all
                // of it when it barely slides.
                if let Some((n, depth, friction, with)) = touch {
                    let slide = self.pos[i] - with;
                    let along = slide - n * slide.dot(n);
                    let len = along.length();
                    if len > 1e-9 {
                        self.pos[i] -= along * (friction * depth / len).min(1.0);
                    }
                }
            }
            // The cloth between two points is kept out of the body too,
            // by its middle: a limb does not come through a cell.
            for &(a, b, kind) in &self.links {
                if (kind != DOWN && kind != ACROSS) || tune("edges", 1.0) == 0.0 {
                    continue;
                }
                let (a, b) = (a as usize, b as usize);
                let (wa, wb) = (
                    if self.reach[a] < TIGHT { 0.0 } else { 1.0 },
                    if self.reach[b] < TIGHT { 0.0 } else { 1.0 },
                );
                if wa + wb == 0.0 || caught[a] || caught[b] {
                    continue;
                }
                let gap = (self.gap[a] + self.gap[b]) * 0.5 + thickness;
                for (s, _) in &body {
                    let middle = (self.pos[a] + self.pos[b]) * 0.5;
                    if let Some((_, dir, dist, r)) = s.inside(middle, gap) {
                        let push = dir * ((r - dist) * 2.0 / (wa + wb));
                        self.pos[a] += push * wa;
                        self.pos[b] += push * wb;
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
                        let i = r * cols + c;
                        if none[i] {
                            continue;
                        }
                        let pushed = *p - self.pos[i];
                        let far = pushed.length();
                        if far > 1e-6 {
                            self.pressed[i] = 1.0;
                        }
                        self.pos[i] = *p;
                    }
                }
            }
            // The lengths again: what the body pushed apart is brought
            // back together, which keeps the cloth from turning inside
            // out.
            // What the body pushed stays where it was pushed to: the
            // cloth next to it comes to it.
            if again {
                lengths_pass(&self.links, &lengths, &kinds, &loose, &mut self.pos, h);
            }
            for (i, ((v, p), o)) in self.vel.iter_mut().zip(&self.pos).zip(&old).enumerate().skip(fixed) {
                *v = if none[i] {
                    Vec3::ZERO
                } else {
                    ((*p - *o) / h).clamp_length_max(fastest)
                };
            }
        }
    }

    /// The net and its target for drawing, with the directions along
    /// their surfaces at each point.
    fn shape(&self) -> NetShape {
        NetShape {
            active: self.active && self.ready,
            pos: frames(&self.pos, self.cols, self.rows, &self.open),
            target: frames(&self.target, self.cols, self.rows, &self.open),
            inner: self.gap.iter().map(|g| (THICKNESS - g) * self.outward).collect(),
            pressed: self.pressed.clone(),
        }
    }
}

impl Net {
    /// Tells the net which of its cells the garment has cloth in
    /// (`cols` per row, `rows - 1` rows): links through the others go,
    /// and points with no cloth round them stay out of the way. A slit
    /// dress is two flaps then, not a tube.
    fn cover(&mut self, cells: &[bool]) {
        let (cols, rows) = (self.cols, self.rows);
        if cells.iter().all(|&c| c) {
            return;
        }
        let cell = |c: usize, r: isize| r >= 0 && (r as usize) < rows - 1 && cells[r as usize * cols + c % cols];
        let mut open = vec![0u8; cols * rows];
        for r in 0..rows {
            for c in 0..cols {
                let i = r * cols + c;
                let last = (c + cols - 1) % cols;
                let r = r as isize;
                if !cell(c, r - 1) && !cell(c, r) {
                    open[i] |= OPEN_ROUND;
                }
                if !cell(c, r - 1) && !cell(c, r) && !cell(last, r - 1) && !cell(last, r) {
                    open[i] |= OPEN_POINT;
                }
            }
        }
        // The fixed rows stay whole: they sit on the body.
        for o in &mut open[..self.fixed * cols] {
            *o = 0;
        }
        let round = |i: u32| open[i as usize] & OPEN_ROUND == 0;
        let point = |i: u32| open[i as usize] & OPEN_POINT == 0;
        self.links.retain(|&(a, b, kind)| match kind {
            ACROSS => round(a),
            DOWN => point(a) && point(b),
            // A diagonal lies in one cell: `a` is in its upper row, at
            // its first column or the one after.
            _ => {
                let (ca, ra) = (a as usize % cols, a as usize / cols);
                let first = if (b as usize % cols) == (ca + 1) % cols {
                    ca
                } else {
                    (ca + cols - 1) % cols
                };
                cells[ra * cols + first]
            }
        });
        if std::env::var("RK_NET_DEBUG").is_ok() {
            let none = open.iter().filter(|&&o| o & OPEN_POINT != 0).count();
            eprintln!(
                "net cover: {} of {} cells have cloth, {none} points have none",
                cells.iter().filter(|&&c| c).count(),
                cells.len()
            );
        }
        self.bends = bends(cols, rows, &open);
        self.open = open;
    }
}

/// The most a garment is taken to hang past the ends of its chains.
const PAST: f32 = 0.4;

/// How far the cloth weighted to `chain_nodes` hangs past the ends of
/// the chains: `ends` gives each chain's end and the way it points
/// there. The furthest vertex counts, each measured from the end
/// nearest to it.
fn past_the_ends(scene: &Scene, chain_nodes: &[usize], ends: &[(Vec3, Vec3)]) -> f32 {
    let (world, _) = scene.world();
    let mut past = 0.0f32;
    for r in &scene.renderers {
        if !r.skinned || r.mesh.skin.is_empty() || r.mesh.positions.is_empty() {
            continue;
        }
        let ours: Vec<bool> = r
            .bones
            .iter()
            .map(|b| b.is_some_and(|n| chain_nodes.binary_search(&n).is_ok()))
            .collect();
        if !ours.iter().any(|&o| o) {
            continue;
        }
        let positions = scene.draw_mesh(r, &world).positions();
        for (sv, p) in r.mesh.skin.iter().zip(&positions) {
            let share: f32 = (0..4)
                .filter(|&k| ours.get(sv.bones[k] as usize).copied().unwrap_or(false))
                .map(|k| sv.weights[k])
                .sum();
            if share <= 0.5 {
                continue;
            }
            let p = Vec3::from(*p);
            let Some(&(end, on)) = ends
                .iter()
                .min_by(|a, b| (a.0 - p).length_squared().total_cmp(&(b.0 - p).length_squared()))
            else {
                continue;
            };
            past = past.max(past_one(p, end, on));
        }
    }
    past.min(PAST)
}

/// How far past a chain's `end` the point `p` is, along the way `on` the
/// chain points there.
fn past_one(p: Vec3, end: Vec3, on: Vec3) -> f32 {
    (p - end).dot(on).max(0.0)
}

/// How many columns between each two chains and how many rows put the
/// points of a net `spacing` apart: `length` is the skirt's length and
/// `round` the distance between two chains at the hem.
fn grid(length: f32, round: f32, spacing: f32) -> (usize, usize) {
    let between = ((round / spacing).round() as usize).clamp(1, BETWEEN + 1) - 1;
    let rows = ((length / spacing).round() as usize + 1).clamp(ROWS.0, ROWS.1);
    (between, rows)
}

/// The cells of a net (`cols` per row, `rows - 1` rows) that a triangle
/// of a mesh bound to it lies in, added to `cells`. `on_net` gives each
/// bound vertex's place on the net as a column and a row with fractions.
fn covered(cells: &mut [bool], cols: usize, indices: &[u32], on_net: &std::collections::HashMap<u32, [f32; 2]>) {
    let rows = cells.len() / cols;
    for t in indices.chunks_exact(3) {
        let (Some(a), Some(b), Some(c)) = (on_net.get(&t[0]), on_net.get(&t[1]), on_net.get(&t[2])) else {
            continue;
        };
        // Columns go round: the other two are taken the short way from
        // the first.
        let near = |u: f32| a[0] + (u - a[0] + cols as f32 * 0.5).rem_euclid(cols as f32) - cols as f32 * 0.5;
        let us = [a[0], near(b[0]), near(c[0])];
        let vs = [a[1], b[1], c[1]];
        // The triangle's box on the net, a little smaller so that one
        // that only touches a cell's side does not count for it.
        let lo = |x: [f32; 3]| x.iter().copied().fold(f32::MAX, f32::min) + 0.05;
        let hi = |x: [f32; 3]| x.iter().copied().fold(f32::MIN, f32::max) - 0.05;
        let (u0, u1) = (lo(us).floor() as i64, hi(us).max(lo(us)).floor() as i64);
        let (v0, v1) = (lo(vs).floor() as i64, hi(vs).max(lo(vs)).floor() as i64);
        for v in v0.max(0)..=v1.min(rows as i64 - 1) {
            for u in u0..=u1 {
                cells[v as usize * cols + u.rem_euclid(cols as i64) as usize] = true;
            }
        }
    }
}

/// How many rows from the waist down are fixed: those within `FIXED` of
/// it down the skirt, the waist's own always.
fn fixed_rows(reach: &[f32], cols: usize, rows: usize) -> usize {
    let within = tune("fixed", FIXED) * REACH;
    (1..rows)
        .take_while(|&r| reach[r * cols..(r + 1) * cols].iter().all(|&d| d <= within))
        .count()
        + 1
}

/// One pass over the links: each is brought to its length, by the
/// lightness of its two ends.
fn lengths_pass(
    links: &[(u32, u32, u8)],
    lengths: &[f32],
    kinds: &[(f32, f32, f32); 3],
    light: &[f32],
    pos: &mut [Vec3],
    h: f32,
) {
    for (&(a, b, kind), &len) in links.iter().zip(lengths) {
        let (a, b) = (a as usize, b as usize);
        let (pulled, pushed, least) = kinds[kind as usize];
        let (wa, wb) = (light[a], light[b]);
        let d = pos[a] - pos[b];
        let dist = d.length();
        if dist <= 1e-9 || wa + wb == 0.0 {
            continue;
        }
        // Pushed together a link gives way, down to a share of its
        // length.
        let (compliance, len) = if dist >= len {
            (pulled, len)
        } else if dist < len * least {
            (0.0, len * least)
        } else {
            (pushed, len)
        };
        let alpha = compliance * (len / LINK).powi(2) / (h * h);
        let lambda = -(dist - len) / (wa + wb + alpha);
        let n = d / dist;
        pos[a] += n * (lambda * wa);
        pos[b] -= n * (lambda * wb);
    }
}

/// Whether a ring grid's surface (round cross down) faces away from the
/// line through the middles of its rows: 1, or -1 when it faces it.
fn outward(points: &[Vec3], cols: usize, rows: usize) -> f32 {
    let mut sum = 0.0;
    for r in 0..rows {
        let row = &points[r * cols..(r + 1) * cols];
        let middle = row.iter().sum::<Vec3>() / cols as f32;
        let below = (r + 1).min(rows - 1);
        let above = r.saturating_sub(1);
        for c in 0..cols {
            let round = row[(c + 1) % cols] - row[(c + cols - 1) % cols];
            let down = points[below * cols + c] - points[above * cols + c];
            sum += round.cross(down).normalize_or_zero().dot(row[c] - middle);
        }
    }
    if sum < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// Each point of a grid with the directions round the waist and down
/// the skirt there.
fn frames(points: &[Vec3], cols: usize, rows: usize, open: &[u8]) -> Vec<[Vec3; 3]> {
    // Whether there is cloth from a point to the next round the skirt,
    // and to the next down it.
    let joined = |i: usize, way: u8| open.get(i).is_none_or(|o| o & way == 0);
    (0..cols * rows)
        .map(|i| {
            let (c, r) = (i % cols, i / cols);
            let at = |c: usize, r: usize| points[r * cols + c];
            let (next, last) = ((c + 1) % cols, (c + cols - 1) % cols);
            // Round the skirt by the neighbours there is cloth to: at
            // the edge of a slit only the one on its own side.
            let round = match (joined(i, OPEN_ROUND), joined(r * cols + last, OPEN_ROUND)) {
                (true, false) => (at(next, r) - at(c, r)) * 2.0,
                (false, true) => (at(c, r) - at(last, r)) * 2.0,
                _ => at(next, r) - at(last, r),
            };
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
pub(super) fn bind(scene: &Scene, nets: &mut [Net]) -> Vec<Binding> {
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
    // Per net point: how far out of the net the innermost layer bound
    // there is.
    let mut inner: Vec<Vec<f32>> = nets.iter().map(|n| vec![f32::MAX; n.rest.len()]).collect();
    let mut out: Vec<Binding> = Vec::new();
    // Per binding and vertex: its place, normal and tangent at rest,
    // which are put in the net's frame once the nets know where they
    // have cloth.
    let mut resting: Vec<Vec<[Vec3; 3]>> = Vec::new();
    // Per net: the cells some bound triangle lies in.
    let mut cells: Vec<Vec<bool>> = nets.iter().map(|n| vec![false; n.cols * (n.rows - 1)]).collect();
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
        let mut vertices = Vec::new();
        let mut places = Vec::new();
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
            let t = posed.tangents[vi];
            places.push([p, Vec3::from(posed.normals[vi]), Vec3::new(t[0], t[1], t[2])]);
            vertices.push(Bound {
                vertex: vi as u32,
                net: k as u32,
                corners: nearest.1,
                bary: nearest.2,
                weight,
                local: [Vec3::ZERO; 3],
            });
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
        for (k, net) in nets.iter().enumerate() {
            // Where on the net each vertex bound to it is.
            let cols = net.cols as i32;
            let on_net: std::collections::HashMap<u32, [f32; 2]> = vertices
                .iter()
                .filter(|b| b.net == k as u32)
                .map(|b| {
                    let first = (b.corners[0] as usize % net.cols) as i32;
                    let (mut u, mut v) = (0.0, 0.0);
                    for (&c, &w) in b.corners.iter().zip(&b.bary) {
                        let col = (c as usize % net.cols) as i32;
                        let col = first + (col - first + cols / 2).rem_euclid(cols) - cols / 2;
                        u += col as f32 * w;
                        v += (c as usize / net.cols) as f32 * w;
                    }
                    (b.vertex, [u, v])
                })
                .collect();
            covered(&mut cells[k], net.cols, &r.mesh.indices, &on_net);
        }
        if !vertices.is_empty() {
            out.push(Binding {
                renderer: ri,
                mesh: Arc::as_ptr(&r.mesh) as *const u8 as usize,
                vertices,
            });
            resting.push(places);
        }
    }
    // A net with no cloth in some of its cells is cut there, unless
    // nothing is bound to it at all.
    if tune("slits", 1.0) != 0.0 {
        for (net, cells) in nets.iter_mut().zip(&cells) {
            if cells.iter().any(|&c| c) {
                net.cover(cells);
            }
        }
    }
    // Each vertex in the frame of the rest net's surface where it is
    // bound.
    let rest_frames: Vec<Vec<[Vec3; 3]>> = nets.iter().map(|n| frames(&n.rest, n.cols, n.rows, &n.open)).collect();
    for (binding, places) in out.iter_mut().zip(&resting) {
        for (bound, &[p, normal, tangent]) in binding.vertices.iter_mut().zip(places) {
            let k = bound.net as usize;
            let (at, turn) = surface(&rest_frames[k], bound);
            let back = turn.transpose();
            bound.local = [back * (p - at), back * normal, back * tangent];
            if bound.weight > 0.5 {
                let out_of = bound.local[0].z * nets[k].outward;
                for (&c, &w) in bound.corners.iter().zip(&bound.bary) {
                    if w > 0.2 {
                        inner[k][c as usize] = inner[k][c as usize].min(out_of);
                    }
                }
            }
        }
    }
    // The net is kept as far off the skin as puts its innermost layer
    // `THICKNESS` off it; a point with no layer takes its neighbours'.
    for (net, inner) in nets.iter_mut().zip(&inner) {
        let known: Vec<f32> = inner.iter().copied().filter(|&d| d < f32::MAX).collect();
        let usual = if known.is_empty() {
            0.0
        } else {
            known.iter().sum::<f32>() / known.len() as f32
        };
        for (gap, &d) in net.gap.iter_mut().zip(inner) {
            let d = if d < f32::MAX { d } else { usual };
            *gap = THICKNESS - d.clamp(-LAYER, LAYER);
        }
        if std::env::var("RK_NET_DEBUG").is_ok() {
            let mut gaps = net.gap.clone();
            gaps.sort_by(f32::total_cmp);
            let q = |f: f32| gaps[((gaps.len() as f32 - 1.0) * f) as usize] * 100.0;
            eprintln!(
                "net gap cm p5 {:.1} p50 {:.1} p95 {:.1}, outward {}",
                q(0.05),
                q(0.5),
                q(0.95),
                net.outward
            );
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
    /// Per point: how far out of the net its innermost layer rests,
    /// along the surface's own third direction, and how much the net is
    /// pressed on something there, 0 to 1.
    pub inner: Vec<f32>,
    pub pressed: Vec<f32>,
}

/// The cloth nets of a character, for `Scene::pose` to put the bound
/// vertices on.
#[derive(Debug, Clone)]
pub struct ClothDraw {
    pub nets: Vec<NetShape>,
    pub bindings: Arc<Vec<Binding>>,
    /// The body in the world, which the vertices are kept out of.
    pub shells: Arc<Vec<WorldShell>>,
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
    if from_turn.determinant().abs() < 0.5 {
        return None;
    }
    // Where the net is folded flat its surface has no direction: the
    // vertex keeps the target's there, and is only moved along.
    let to_turn = if to_turn.determinant().abs() < 0.5 {
        from_turn
    } else {
        to_turn
    };
    let [p, n, t] = skinned;
    let [local, local_n, local_t] = b.local;
    // How far the game's skinning is from the net's own carry: nothing
    // of it is used, it is only told.
    let extra = p - (from + from_turn * local);
    // Cloth pressed on something lies flat on it: what the layers stand
    // off the net by, more than the innermost one, mostly goes.
    let (mut inner, mut pressed) = (0.0, 0.0);
    for (&c, &w) in b.corners.iter().zip(&b.bary) {
        inner += net.inner[c as usize] * w;
        pressed += net.pressed[c as usize] * w;
    }
    let mut local = local;
    local.z = inner + (local.z - inner) * (1.0 - tune("flatten", FLATTEN) * pressed);
    let moved = to + to_turn * local;
    let (cloth_n, cloth_t) = (to_turn * local_n, to_turn * local_t);
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
    pub(super) fn from_nets(nets: &[Net], bindings: &Arc<Vec<Binding>>, shells: &Arc<Vec<WorldShell>>) -> Self {
        ClothDraw {
            nets: nets.iter().map(Net::shape).collect(),
            bindings: bindings.clone(),
            shells: shells.clone(),
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
        // The vertices are kept out of the body itself, by their share
        // on the skirt's bones, and above the floor.
        if let Some(push) = &push {
            let floor_only = crate::scene::PushDraw {
                capsules: Vec::new(),
                floor: push.floor,
                skirt_slots: Vec::new(),
            };
            let vertex_gap = tune("vertex_gap", VERTEX_GAP);
            for (p, sv) in posed.positions.iter_mut().zip(&d.renderer.mesh.skin) {
                let share = push.share(sv.bones, sv.weights);
                if share <= 0.0 {
                    continue;
                }
                let from = Vec3::from(*p);
                let mut q = from;
                for s in self.shells.iter().filter(|s| !s.outside) {
                    s.push_out(&mut q, vertex_gap);
                }
                *p = floor_only.apply(from.lerp(q, share), share).to_array();
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
            bends: bends(cols, rows, &[]),
            chain_nodes: vec![0],
            pos: rest.clone(),
            vel: vec![Vec3::ZERO; rest.len()],
            target: rest.clone(),
            gap: vec![THICKNESS; rest.len()],
            outward: outward(&rest, cols, rows),
            fixed: 1,
            open: Vec::new(),
            hold: vec![0.0; rest.len()],
            pressed: vec![0.0; rest.len()],
            shell_rest: Vec::new(),
            shell_before: Vec::new(),
            shell_was: Vec::new(),
            before: rest.clone(),
            rest_on_waist: rest.clone(),
            shape_on_waist: rest.clone(),
            full: rest.clone(),
            settling: false,
            rest,
            ready: false,
            active: false,
            nearby: Nearby::default(),
        };
        (scene, net)
    }

    fn run(scene: &Scene, net: &mut Net, bodies: &[WorldShell], floor: Option<(Vec3, Vec3)>, frames: u32) {
        for _ in 0..frames {
            net.step(scene, true, &[], bodies, floor, None, 1.0 / 60.0);
        }
    }

    /// A round limb from `a` to `b`.
    fn limb(a: Vec3, b: Vec3, radius: f32, outside: bool) -> WorldShell {
        let e1 = (b - a).any_orthonormal_vector();
        Shell {
            node: 0,
            a,
            b,
            e1,
            e2: (b - a).normalize().cross(e1),
            stations: 2,
            radii: vec![radius; 2 * super::super::shell::ANGLES].into(),
            caps: [true, true],
            outside,
        }
        .in_world(&Mat4::IDENTITY)
    }

    #[test]
    fn a_hanging_net_stays_on_its_target() {
        let (scene, mut net) = cone(16, 8, 0.12, 0.16, 0.4);
        run(&scene, &mut net, &[], None, 120);
        for (p, t) in net.pos.iter().zip(&net.target) {
            assert!((*p - *t).length() < 0.03, "{p} is off {t}");
        }
    }

    #[test]
    fn a_settled_net_hangs_in_closer_and_keeps_its_cloth() {
        let (scene, mut net) = cone(16, 8, 0.12, 0.3, 0.4);
        net.settle(&scene, &[]);
        let (cols, rows) = (net.cols, net.rows);
        let radius = |points: &[Vec3], r: usize| {
            let row = &points[r * cols..(r + 1) * cols];
            row.iter().map(|p| Vec3::new(p.x, 0.0, p.z).length()).sum::<f32>() / cols as f32
        };
        // The fixed top stays, the hem comes in.
        assert!((radius(&net.shape_on_waist, 0) - 0.12).abs() < 1e-4);
        let hem = radius(&net.shape_on_waist, rows - 1);
        assert!(hem < 0.27, "the hem is still {hem} out");
        // The cloth is as long as it was.
        for c in 0..cols {
            for r in 1..rows {
                let (a, b) = (r * cols + c, (r - 1) * cols + c);
                let now = (net.shape_on_waist[a] - net.shape_on_waist[b]).length();
                let rest = (net.rest[a] - net.rest[b]).length();
                assert!((now - rest).abs() < rest * 0.1, "a link of {rest} is {now} long");
            }
        }
        // It then rests in that shape.
        run(&scene, &mut net, &[], None, 120);
        let after = radius(&net.pos, rows - 1);
        assert!((after - hem).abs() < 0.02, "the hem went from {hem} to {after}");
    }

    #[test]
    fn cloth_caught_between_a_limb_and_the_floor_stays_there() {
        // The skirt lies piled on the floor, and a limb comes to lie on
        // it, sunk into the floor.
        let (scene, mut net) = cone(16, 8, 0.12, 0.2, 0.4);
        let floor = Some((Vec3::new(0.0, 0.75, 0.0), Vec3::Y));
        run(&scene, &mut net, &[], floor, 120);
        let before = net.pos.clone();
        let on_floor = |p: &Vec3| p.y < 0.75 + THICKNESS + 1e-3;
        let (i, at) = before
            .iter()
            .enumerate()
            .filter(|(_, p)| on_floor(p))
            .max_by(|a, b| a.1.z.total_cmp(&b.1.z))
            .expect("the hem lies on the floor");
        let lying = limb(Vec3::new(-0.5, 0.8, at.z), Vec3::new(0.5, 0.8, at.z), 0.09, false);
        run(&scene, &mut net, &[lying], floor, 60);
        assert!((net.pos[i] - *at).length() < 2e-3, "{at} crept to {}", net.pos[i]);
    }

    #[test]
    fn a_triangle_covers_the_cells_it_lies_in_and_goes_round_the_ring() {
        let cols = 8;
        let mut cells = vec![false; cols * 3];
        let on_net: std::collections::HashMap<u32, [f32; 2]> = [
            (0, [1.2, 0.2]),
            (1, [1.8, 0.3]),
            (2, [1.5, 0.9]),
            // Over the seam of the ring, in its last column and row.
            (3, [7.2, 2.2]),
            (4, [0.0, 2.5]),
            (5, [7.5, 2.9]),
        ]
        .into_iter()
        .collect();
        covered(&mut cells, cols, &[0, 1, 2, 3, 4, 5, 0, 1, 9], &on_net);
        let want: Vec<usize> = vec![1, 2 * cols + 7];
        let got: Vec<usize> = (0..cells.len()).filter(|&i| cells[i]).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn a_net_with_no_cloth_on_one_side_is_two_edges_and_nothing_between() {
        let (scene, mut net) = cone(16, 8, 0.12, 0.2, 0.4);
        let (cols, rows) = (net.cols, net.rows);
        // No cloth in columns 4 to 9 below the first row of cells.
        let mut cells = vec![true; cols * (rows - 1)];
        for r in 1..rows - 1 {
            for c in 4..10 {
                cells[r * cols + c] = false;
            }
        }
        let whole = net.links.len();
        net.cover(&cells);
        assert!(net.links.len() < whole);
        // Points 5 to 9 of the lower rows have none round them, and no
        // link is left to them; the edges at 4 and 10 are still cloth.
        let none = |i: usize| net.open[i] & OPEN_POINT != 0;
        for r in 2..rows {
            for c in 0..cols {
                assert_eq!(none(r * cols + c), (5..10).contains(&c), "column {c} row {r}");
            }
        }
        for &(a, b, _) in &net.links {
            assert!(!none(a as usize) && !none(b as usize) || (a as usize) < 2 * cols || (b as usize) < 2 * cols);
        }
        // The surface at an edge is told by its own side only.
        let f = frames(&net.rest, cols, rows, &net.open);
        let i = 4 * cols + 4;
        assert!((f[i][1] - (net.rest[i] - net.rest[i - 1]) * 2.0).length() < 1e-6);
        // A limb where there is no cloth moves nothing.
        run(&scene, &mut net, &[], None, 60);
        let free = net.pos.clone();
        let through = limb(Vec3::new(0.0, 0.7, 0.0), Vec3::new(-0.55, 0.7, 0.23), 0.03, false);
        net.reset();
        run(&scene, &mut net, &[through], None, 60);
        for (p, q) in net.pos.iter().zip(&free) {
            assert!((*p - *q).length() < 1e-3, "{q} moved to {p}");
        }
    }

    #[test]
    fn a_net_is_kept_off_the_floor_and_keeps_its_lengths() {
        // The floor is 15 cm above the hem.
        let (scene, mut net) = cone(16, 8, 0.12, 0.2, 0.4);
        let floor = Some((Vec3::new(0.0, 0.75, 0.0), Vec3::Y));
        run(&scene, &mut net, &[], floor, 120);
        for p in &net.pos[net.cols..] {
            assert!(p.y >= 0.75 + THICKNESS - 1e-3, "{p} is under the floor");
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
    fn a_limb_through_the_skirt_holds_it_out() {
        // A thigh sticks out level through the front of the skirt: the
        // net stays out of it.
        let (scene, mut net) = cone(16, 8, 0.12, 0.2, 0.4);
        let thigh = limb(Vec3::new(0.0, 0.8, 0.0), Vec3::new(0.5, 0.8, 0.0), 0.07, false);
        run(&scene, &mut net, std::slice::from_ref(&thigh), None, 120);
        for p in net.pos.iter().skip(2 * net.cols) {
            assert!(thigh.depth(*p, 0.0) < 4e-3, "{p} is inside the thigh");
        }
        // The cloth over the thigh lies on it: it is pressed there, and
        // the hem at the back hangs free.
        assert!(net.pressed.iter().any(|&k| k > 0.5));
        assert!(net.pressed[net.cols * 7 + 8] < 0.5);
    }

    #[test]
    fn an_arm_in_the_wall_of_the_skirt_pushes_the_cloth_in() {
        // A forearm lies level in the front wall of the skirt, its line
        // a little outside the cloth: the cloth leaves it inwards.
        let (scene, mut net) = cone(16, 8, 0.12, 0.2, 0.4);
        let radius_at = |y: f32| 0.12 + 0.08 * (1.0 - y) / 0.4;
        let arm = limb(
            Vec3::new(radius_at(0.8) - 0.01, 0.8, -0.2),
            Vec3::new(radius_at(0.8) - 0.01, 0.8, 0.2),
            0.03,
            true,
        );
        run(&scene, &mut net, std::slice::from_ref(&arm), None, 60);
        for (i, p) in net.pos.iter().enumerate().skip(2 * net.cols) {
            assert!(arm.depth(*p, 0.0) < 2e-3, "{p} is inside the arm");
            if (p.y - 0.8).abs() < 0.03 && p.z.abs() < 0.1 && p.x > 0.0 {
                assert!(p.x < radius_at(0.8) - 0.01, "point {i} at {p} went over the arm");
            }
        }
    }

    #[test]
    fn a_grid_puts_its_points_the_spacing_apart() {
        // 47 cm long, chains 17 cm apart at the hem, points 4.3 cm apart.
        assert_eq!(grid(0.47, 0.17, 0.043), (3, 12));
        // Half the spacing: twice the cells.
        assert_eq!(grid(0.47, 0.17, 0.0215), (7, 23));
        // Never fewer than the least, never more than the most.
        assert_eq!(grid(0.05, 0.01, 0.043), (0, ROWS.0));
        assert_eq!(grid(5.0, 5.0, 0.01), (BETWEEN, ROWS.1));
    }

    #[test]
    fn cloth_past_a_chains_end_is_measured_along_the_chain() {
        let (end, on) = (Vec3::new(0.0, 0.5, 0.2), Vec3::new(0.0, -0.8, 0.6));
        assert!((past_one(end + on * 0.1 + Vec3::X * 0.3, end, on) - 0.1).abs() < 1e-6);
        assert_eq!(past_one(end - on * 0.1, end, on), 0.0);
    }

    #[test]
    fn a_moving_body_is_part_of_the_way_there_within_a_step() {
        let (from, to) = ([Mat4::IDENTITY], [Mat4::from_translation(Vec3::X)]);
        let body = Body::Moving {
            shells: &[],
            from: &from,
            to: &to,
        };
        let places = body.places(0.25).expect("it moves");
        assert!((places[0].w_axis.x - 0.25).abs() < 1e-6);
        assert!(Body::Still(&[]).places(0.25).is_none());
    }

    #[test]
    fn cloth_on_a_limb_goes_along_with_it() {
        // A thigh sticks out level through the front of the skirt and
        // the cloth has come to rest on it. It then slides along its own
        // length by 2 cm over one frame: the cloth pressed on it goes
        // most of that way with it, where friction in the world's space
        // would have held it back.
        let (scene, mut net) = cone(16, 8, 0.12, 0.2, 0.4);
        let thigh = limb(Vec3::new(0.0, 0.8, 0.0), Vec3::new(0.5, 0.8, 0.0), 0.07, false);
        run(&scene, &mut net, std::slice::from_ref(&thigh), None, 120);
        let (i, at) = net
            .pos
            .iter()
            .copied()
            .enumerate()
            .filter(|&(i, _)| net.pressed[i] > 0.5)
            .max_by(|a, b| a.1.y.total_cmp(&b.1.y))
            .expect("cloth lies on the thigh");
        let shell = Shell {
            node: 0,
            a: Vec3::new(0.0, 0.8, 0.0),
            b: Vec3::new(0.5, 0.8, 0.0),
            e1: Vec3::Y,
            e2: Vec3::Z,
            stations: 2,
            radii: vec![0.07; 2 * crate::physics::shell::ANGLES].into(),
            caps: [true, true],
            outside: false,
        };
        let (from, to) = ([Mat4::IDENTITY], [Mat4::from_translation(Vec3::X * 0.02)]);
        let body = Body::Moving {
            shells: std::slice::from_ref(&shell),
            from: &from,
            to: &to,
        };
        net.advance(&body, &Mat4::IDENTITY, None, None, 1.0 / 60.0);
        let went = net.pos[i].x - at.x;
        assert!(went > 0.01, "the cloth went {went} of 0.02 with the thigh");
    }

    #[test]
    fn a_ring_cut_open_has_no_bend_across_the_cut() {
        // No cloth from column 1 to column 2, in every row.
        let (cols, rows) = (6, 4);
        let mut open = vec![0u8; cols * rows];
        for r in 0..rows {
            open[r * cols + 1] = OPEN_ROUND;
        }
        let b = bends(cols, rows, &open);
        // Columns 1 and 2 have a neighbour on one side only.
        assert!(!b
            .iter()
            .any(|b| b.round && (b.at as usize % cols == 1 || b.at as usize % cols == 2)));
        // Their bends down the skirt are measured round that one side.
        let down = b
            .iter()
            .find(|b| !b.round && b.at as usize == cols + 1)
            .expect("a bend");
        assert_eq!(down.axis, [cols as u32, cols as u32 + 1]);
        let down = b
            .iter()
            .find(|b| !b.round && b.at as usize == cols + 2)
            .expect("a bend");
        assert_eq!(down.axis, [cols as u32 + 2, cols as u32 + 3]);
    }

    #[test]
    fn the_way_between_two_places_turns_the_short_way() {
        let from = Mat4::from_translation(Vec3::X);
        let to = Mat4::from_rotation_translation(Quat::from_rotation_z(1.0), Vec3::new(3.0, 0.0, 0.0));
        let half = between(&from, &to, 0.5);
        let (_, turn, place) = half.to_scale_rotation_translation();
        assert!((place - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-5);
        assert!(turn.angle_between(Quat::from_rotation_z(0.5)) < 1e-4);
    }

    #[test]
    fn a_cone_round_the_y_axis_knows_which_way_is_out() {
        let (_, net) = cone(16, 8, 0.12, 0.2, 0.4);
        let (cols, rows, out) = (net.cols, net.rows, net.outward);
        for i in [cols * 3, cols * 3 + 5] {
            let (c, r) = (i % cols, i / cols);
            let round = net.rest[r * cols + (c + 1) % cols] - net.rest[r * cols + (c + cols - 1) % cols];
            let down = net.rest[(r + 1).min(rows - 1) * cols + c] - net.rest[(r - 1) * cols + c];
            let n = round.cross(down).normalize() * out;
            assert!(n.dot(Vec3::new(net.rest[i].x, 0.0, net.rest[i].z)) > 0.0);
        }
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
            pos: frames(&points, 3, 2, &[]),
            target: frames(&points, 3, 2, &[]),
            inner: vec![0.0; 6],
            pressed: vec![0.0; 6],
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
    fn a_vertex_takes_its_rest_place_on_the_net_whatever_the_game_skins() {
        let (shape, bound) = patch();
        // The game's skinning, by the skirt's chains, puts the vertex
        // somewhere else than it rests on the net: the net wins.
        let (rest, rest_turn) = surface(&shape.target, &bound);
        let skinned = [Vec3::new(0.8, -0.4, 0.07), Vec3::Z, Vec3::X];
        let ([p, _, _], extra) = carry(&bound, &shape, skinned).expect("a frame");
        assert!((p - (rest + rest_turn.z_axis * 0.02)).length() < 1e-5, "{p}");
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
        shape.pos = frames(&moved, 3, 2, &[]);
        let (rest, rest_turn) = surface(&shape.target, &bound);
        let rest = rest + rest_turn.z_axis * 0.02;
        let skinned = [Vec3::new(0.8, -0.4, 0.3), Vec3::Z, Vec3::X];
        let ([p, n, _], _) = carry(&bound, &shape, skinned).expect("a frame");
        let want = turn * rest + Vec3::new(0.0, 0.0, 1.0);
        assert!((p - want).length() < 1e-5, "{p} is not {want}");
        assert!((n - turn * Vec3::Z).length().min((n + turn * Vec3::Z).length()) < 1e-5);
    }

    #[test]
    fn a_layer_on_a_net_pressed_on_something_lies_nearly_flat_on_it() {
        let (mut shape, bound) = patch();
        let moved: Vec<Vec3> = shape.target.iter().map(|f| f[0] + Vec3::new(0.0, 0.0, 1.0)).collect();
        shape.pos = frames(&moved, 3, 2, &[]);
        shape.pressed = vec![1.0; 6];
        let (rest, rest_turn) = surface(&shape.target, &bound);
        // The vertex rests 2 cm off the net; pressed, a fifth of that.
        let skinned = [rest + rest_turn.z_axis * 0.02, Vec3::Z, Vec3::X];
        let ([p, _, _], _) = carry(&bound, &shape, skinned).expect("a frame");
        let want = rest + Vec3::new(0.0, 0.0, 1.0) + rest_turn.z_axis * 0.02 * (1.0 - FLATTEN);
        assert!((p - want).length() < 1e-5, "{p} is not {want}");
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
        let f = frames(&points, 3, 3, &[]);
        assert!(f[7][2].y < 0.0, "the hem's way down is {}", f[7][2]);
    }

    /// A strip of cloth `length` long held out level by one end, made
    /// of points `spacing` apart, left to droop: how far its free end
    /// comes down. Three columns, so that the bends have a line to be
    /// measured round.
    fn droop(length: f32, spacing: f32, bending: f32) -> f32 {
        let rows = (length / spacing).round() as usize + 2;
        let cols = 3;
        let mut pos: Vec<Vec3> = (0..rows * cols)
            .map(|i| Vec3::new((i / cols) as f32 * spacing, 1.0, (i % cols) as f32 * spacing))
            .collect();
        let rest = pos.clone();
        // The ring's bends, less those round it: the strip is flat.
        let bends: Vec<Bend> = bends(cols, rows, &[]).into_iter().filter(|b| !b.round).collect();
        let bend_rest: Vec<Option<BendRest>> = bends
            .iter()
            .map(|_| {
                Some(BendRest {
                    angle: 0.0,
                    compliance: spacing * spacing / (9.81 * bending.powi(3)),
                    most: 3.0,
                })
            })
            .collect();
        let links = links(cols, rows);
        let lengths: Vec<f32> = links
            .iter()
            .map(|&(a, b, _)| (rest[a as usize] - rest[b as usize]).length())
            .collect();
        let kinds = [(0.0, 0.0, 1.0); 3];
        let light: Vec<f32> = (0..rows * cols).map(|i| if i < 2 * cols { 0.0 } else { 1.0 }).collect();
        let mut vel = vec![Vec3::ZERO; pos.len()];
        let h = 1.0 / 480.0;
        for _ in 0..480 * 6 {
            let old = pos.clone();
            for i in 2 * cols..pos.len() {
                vel[i] = (vel[i] + GRAVITY * h) * (-3.0 * h).exp();
                pos[i] += vel[i] * h;
            }
            lengths_pass(&links, &lengths, &kinds, &light, &mut pos, h);
            bends_pass(&bends, &bend_rest, &light, &mut pos, h);
            for i in 0..pos.len() {
                vel[i] = (pos[i] - old[i]) / h;
            }
        }
        rest[pos.len() - 2].y - pos[pos.len() - 2].y
    }

    #[test]
    fn a_strip_held_out_droops_by_its_bending_length_at_any_spacing() {
        // A beam whose weight bends it a little: the end comes down by
        // its length to the fourth over eight times the bending length
        // cubed. Here 6 cm of cloth with a bending length of 6 cm. A
        // strip of few points is heavier at its end and bends in full at
        // the point it is held by, so it droops more than that, by about
        // the same at 2 cm and at 1 cm.
        let theory = 0.06f32.powi(4) / (8.0 * 0.06f32.powi(3));
        let (coarse, fine) = (droop(0.06, 0.02, 0.06), droop(0.06, 0.01, 0.06));
        assert!(coarse > theory && coarse < theory * 2.5, "{coarse} for {theory}");
        assert!((fine - coarse).abs() < coarse * 0.15, "{fine} for {coarse}");
        // Limp cloth hangs straight down from where it is held.
        let limp = droop(0.2, 0.02, 0.02);
        assert!(limp > 0.15, "{limp}");
    }

    #[test]
    fn links_join_each_point_to_its_neighbours_round_the_ring() {
        let l = links(4, 3);
        assert!(l.contains(&(3, 0, ACROSS)));
        assert!(l.contains(&(0, 4, DOWN)));
        assert!(!l.iter().any(|&(a, b, _)| a == b));
        // Per column: 2 down, 3 across, 4 diagonals.
        assert_eq!(l.len(), 4 * (2 + 3 + 4));
        // One bend down per column, at the middle row, and one round
        // per point.
        let b = bends(4, 3, &[]);
        assert_eq!(b.len(), 4 + 12);
        assert!(b.contains(&Bend {
            ends: [0, 8],
            at: 4,
            axis: [7, 5],
            round: false
        }));
    }
}
