// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The main-game BetterPenetration plugin's body collision
//! (`KK_BetterPenetration`, `Core_BetterPenetration`): the shaft's end kept
//! inside the partner's body.
//!
//! The partner carries two rows of collision points, one up her front and
//! one up her back (`CollisionAgent.Initialize`). Each pair of neighbours
//! spans a surface (`TwistedPlane`). A shaft that would pass through a
//! surface is bent at the crossing and runs along it, keeping its length
//! (`DanAgent.ConstrainDanToCollisionPoints`).
//!
//! Everything here works on positions and directions; nothing reads a
//! scene. Numbers follow the C#: `double` where it computes in `double`,
//! Unity's `Vector3` functions through `rk_ik::unity`. Float expressions
//! the C# widens only at the end are summed in `double`, as the game's
//! Mono does (see `rk_ik::unity`).

use crate::better_penetration::{squish, Mode, Settings};
use glam::{Mat4, Vec3};
use rk_ik::unity;

/// The front row's bones, from the crotch up (`BoneNames.frontCollisionList`).
pub const FRONT_BONES: [&str; 3] = ["k_f_kokan_00", "a_n_waist_f", "a_n_bust_f"];
/// The back row's bones, from the crotch up (`BoneNames.backCollisionList`).
pub const BACK_BONES: [&str; 3] = ["k_f_ana_00", "a_n_waist_b", "a_n_back"];
/// The simplified inner target of vaginal and anal (`BoneNames.InnerTarget`,
/// `CollisionAgent.m_innerTarget`).
pub const INNER_TARGET: &str = "cf_j_waist01";
/// The simplified inner target of oral (`BoneNames.InnerHeadTarget`,
/// `CollisionAgent.m_innerHeadTarget`).
pub const INNER_HEAD_TARGET: &str = "cf_J_FaceBase";

/// The options the collision reads, with the plugin's defaults
/// (`KK_BetterPenetration.Awake`, the `Config.Bind` calls, and the lists
/// at the top of the class).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyOptions {
    /// "Clipping Offset: Front Collision N" (`frontOffsets`).
    pub front_offsets: [f32; 3],
    /// "Clipping Offset: Back Collision N" (`backOffsets`).
    pub back_offsets: [f32; 3],
    /// `frontPointsInward`, not a setting.
    pub front_inward: [bool; 3],
    /// `backPointsInward`, not a setting.
    pub back_inward: [bool; 3],
    /// "Clipping Depth".
    pub clipping_depth: f32,
    /// "Target Offset: Vagina Target".
    pub kokan_offset: f32,
    /// "Target Offset: Inner Vagina Target".
    pub inner_kokan_offset: f32,
    /// "Target Offset: Mouth Target".
    pub mouth_offset: f32,
    /// "Target Offset: Inner Mouth Target".
    pub inner_mouth_offset: f32,
    /// `CollisionOptions.enableBellyBulge`. The plugin never passes it, so
    /// it is the constructor's default.
    pub belly_bulge: bool,
    /// "Simplify Penetration Calculation".
    pub simplify_vaginal: bool,
    /// "Simplify Anal Calculation".
    pub simplify_anal: bool,
    /// "Simplify Oral Calculation".
    pub simplify_oral: bool,
    /// "Limit Penis Movement".
    pub limit_correction: bool,
    /// "Limit Penis Amount".
    pub max_correction: f32,
    /// "Rotate Balls with Shaft".
    pub rotate_balls: bool,
    /// "Penis: Squish Oral Girth".
    pub squish_oral_girth: bool,
}

impl Default for BodyOptions {
    fn default() -> Self {
        BodyOptions {
            front_offsets: [-0.04, 0.04, 0.06],
            back_offsets: [-0.04, 0.02, 0.0],
            front_inward: [false, false, false],
            back_inward: [false, true, true],
            clipping_depth: 0.02,
            kokan_offset: 0.0,
            inner_kokan_offset: 0.0,
            mouth_offset: 0.0,
            inner_mouth_offset: 0.0,
            belly_bulge: true,
            simplify_vaginal: false,
            simplify_anal: false,
            simplify_oral: false,
            limit_correction: false,
            max_correction: 0.2,
            rotate_balls: true,
            squish_oral_girth: false,
        }
    }
}

impl BodyOptions {
    /// The main game plugin's settings as its BepInEx config file holds
    /// them: "Female Options" and "Male 1 Options". What the file leaves
    /// out keeps the plugin's defaults.
    pub fn from_config(text: &str) -> BodyOptions {
        let mut o = BodyOptions::default();
        let mut section = "";
        for line in text.lines() {
            let line = line.trim();
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = match name {
                    "Female Options" => "female",
                    "Male 1 Options" => "male",
                    _ => "",
                };
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            let number = value.parse::<f32>().ok();
            let flag = match value {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            };
            let set = |to: &mut f32| {
                if let Some(v) = number {
                    *to = v;
                }
            };
            let switch = |to: &mut bool| {
                if let Some(v) = flag {
                    *to = v;
                }
            };
            match (section, key) {
                ("female", "Clipping Depth") => set(&mut o.clipping_depth),
                ("female", "Target Offset: Vagina Target") => set(&mut o.kokan_offset),
                ("female", "Target Offset: Inner Vagina Target") => set(&mut o.inner_kokan_offset),
                ("female", "Target Offset: Mouth Target") => set(&mut o.mouth_offset),
                ("female", "Target Offset: Inner Mouth Target") => set(&mut o.inner_mouth_offset),
                ("male", "Simplify Penetration Calculation") => switch(&mut o.simplify_vaginal),
                ("male", "Simplify Oral Calculation") => switch(&mut o.simplify_oral),
                ("male", "Simplify Anal Calculation") => switch(&mut o.simplify_anal),
                ("male", "Limit Penis Movement") => switch(&mut o.limit_correction),
                ("male", "Limit Penis Amount") => set(&mut o.max_correction),
                ("male", "Rotate Balls with Shaft") => switch(&mut o.rotate_balls),
                ("male", "Penis: Squish Oral Girth") => switch(&mut o.squish_oral_girth),
                ("female", key) => {
                    for (prefix, into) in [
                        ("Clipping Offset: Front Collision ", &mut o.front_offsets),
                        ("Clipping Offset: Back Collision ", &mut o.back_offsets),
                    ] {
                        if let Some(i) = key.strip_prefix(prefix).and_then(|i| i.parse::<usize>().ok()) {
                            if let Some(slot) = into.get_mut(i) {
                                set(slot);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        o
    }

    /// The settings of the plugin installed in the game at `game_dir`, or
    /// its defaults without one.
    pub fn of_game(game_dir: &std::path::Path) -> BodyOptions {
        std::fs::read_to_string(game_dir.join("BepInEx/config/animal42069.KKbetterpenetration.cfg"))
            .map(|text| BodyOptions::from_config(&text))
            .unwrap_or_default()
    }
}

/// One collision point: what the C# reads of its transform, and its
/// `CollisionPointInfo`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionPoint {
    /// `transform.position`.
    pub position: Vec3,
    /// `transform.forward`.
    pub forward: Vec3,
    /// `transform.right`.
    pub right: Vec3,
    /// `CollisionPointInfo.offset`.
    pub offset: f32,
    /// `CollisionPointInfo.inward`.
    pub inward: bool,
}

impl CollisionPoint {
    /// A point from a bone's world matrix: its place, and its Z and X axes
    /// at length one for `forward` and `right`. Under a skewed parent
    /// Unity's world rotation differs slightly from the matrix axes.
    pub fn of_world(world: &Mat4, offset: f32, inward: bool) -> CollisionPoint {
        CollisionPoint {
            position: world.w_axis.truncate(),
            forward: unity::normalize(world.z_axis.truncate()),
            right: unity::normalize(world.x_axis.truncate()),
            offset,
            inward,
        }
    }
}

/// The partner's two rows (`CollisionPoints`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CollisionPoints {
    /// `frontCollisionPoints`.
    pub front: Vec<CollisionPoint>,
    /// `backCollisionPoints`.
    pub back: Vec<CollisionPoint>,
}

impl CollisionPoints {
    /// The rows from the partner's world matrices, as
    /// `CollisionAgent.Initialize` pairs each bone with its info. None if
    /// a bone is missing: the C# keeps the empty slot and then misreads
    /// its list of places, which no stock skeleton reaches.
    pub fn find(world: &[Mat4], find: impl Fn(&str) -> Option<usize>, o: &BodyOptions) -> Option<CollisionPoints> {
        let row = |names: &[&str; 3], offsets: &[f32; 3], inward: &[bool; 3]| {
            (0..3)
                .map(|i| {
                    Some(CollisionPoint::of_world(
                        world.get(find(names[i])?)?,
                        offsets[i],
                        inward[i],
                    ))
                })
                .collect::<Option<Vec<_>>>()
        };
        Some(CollisionPoints {
            front: row(&FRONT_BONES, &o.front_offsets, &o.front_inward)?,
            back: row(&BACK_BONES, &o.back_offsets, &o.back_inward)?,
        })
    }
}

/// `MathHelpers.ApproximatelyZero`.
fn approximately_zero(value: f64) -> bool {
    value > -0.0001 && value < 0.0001
}

/// `MathHelpers.SolveQuadratic`: the two roots, equal for a line.
fn solve_quadratic(a: f64, b: f64, c: f64) -> Option<(f64, f64)> {
    if approximately_zero(a) {
        if approximately_zero(b) {
            return None;
        }
        let s = (0.0 - c) / b;
        return Some((s, s));
    }
    let num = b * b - 4.0 * a * c;
    if num >= 0.0 {
        Some(((0.0 - b + num.sqrt()) / (2.0 * a), (0.0 - b - num.sqrt()) / (2.0 * a)))
    } else {
        None
    }
}

/// `MathHelpers.SolveSSATriangle`, its `sideC`: the third side of a
/// triangle with sides `side_a` and `side_b` and the angle opposite
/// `side_a`. Zero if any of the three is about zero.
fn solve_ssa_triangle(side_a: f64, side_b: f64, angle_a: f64) -> f64 {
    if approximately_zero(side_a) || approximately_zero(side_b) || approximately_zero(angle_a) {
        return 0.0;
    }
    let angle_b = (angle_a.sin() * side_b / side_a).asin();
    let angle_c = std::f64::consts::PI - angle_a - angle_b;
    side_a * angle_c.sin() / angle_a.sin()
}

/// `MathHelpers.DegToRad`.
fn deg_to_rad(degrees: f64) -> f64 {
    degrees * std::f64::consts::PI / 180.0
}

/// `Vector3.LerpUnclamped`.
fn lerp_unclamped(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    let f = |a: f32, b: f32| (f64::from(a) + (f64::from(b) - f64::from(a)) * f64::from(t)) as f32;
    Vec3::new(f(a.x, b.x), f(a.y, b.y), f(a.z, b.z))
}

/// `MathHelpers.CastToSegment`: `position` dropped onto the line from
/// `line_start` along `line_vector`. The C# divides by the vector's length
/// once, not twice, so it is a true projection only for a vector of
/// length one; kept.
pub fn cast_to_segment(position: Vec3, line_start: Vec3, line_vector: Vec3) -> Vec3 {
    let end = line_start + line_vector;
    let num = unity::dot(position - line_start, line_vector) / unity::magnitude(line_vector);
    lerp_unclamped(line_start, end, num)
}

/// `MathHelpers.CastSegmentToSegment`: the point of the second line
/// nearest the first.
fn cast_segment_to_segment(from_start: Vec3, from_vector: Vec3, to_start: Vec3, to_vector: Vec3) -> Vec3 {
    let from_end = from_start + from_vector;
    let val2 = from_start - unity::dot(from_start - to_start, to_vector) * to_vector;
    let val3 = from_end - unity::dot(from_end - to_start, to_vector) * to_vector - val2;
    let num = unity::dot(to_start - val2, val3);
    let num2 = unity::magnitude(val3);
    let num3 = num / num2;
    cast_to_segment(from_start + from_vector * num3, to_start, to_vector)
}

/// Three products of floats summed in double.
fn wide_dot(a: Vec3, b: Vec3) -> f64 {
    f64::from(a.x) * f64::from(b.x) + f64::from(a.y) * f64::from(b.y) + f64::from(a.z) * f64::from(b.z)
}

/// What `TwistedPlane.IntersectLineOnTwistedPlane` gives for a crossing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Intersection {
    /// `intersectionPoint`.
    pub point: Vec3,
    /// `intersectForwardVector`: along the surface, towards its second edge.
    pub forward: Vec3,
    /// `distanceToSecondVector`.
    pub distance_to_second: f32,
}

/// The surface between two collision points (`TwistedPlane`): the points
/// `first_origin + v * first_vector + u * (forward_vector + v *
/// twist_vector)`, `u` from 0 at the first edge to 1 at the second, `v`
/// unbounded along the edges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TwistedPlane {
    pub first_origin: Vec3,
    pub first_vector: Vec3,
    pub second_origin: Vec3,
    pub second_vector: Vec3,
    /// `secondOrig - firstOrig`.
    pub forward_vector: Vec3,
    /// `secondVec - firstVec`.
    pub twist_vector: Vec3,
}

impl TwistedPlane {
    /// The constructor.
    pub fn new(first_origin: Vec3, first_vector: Vec3, second_origin: Vec3, second_vector: Vec3) -> TwistedPlane {
        TwistedPlane {
            first_origin,
            first_vector,
            second_origin,
            second_vector,
            forward_vector: second_origin - first_origin,
            twist_vector: second_vector - first_vector,
        }
    }

    /// `TwistedPlane.FindIntersectValues`: `v` on the surface and `t` on
    /// the line for a root `u`. None where the C# returns false; its `t`
    /// is zero then.
    #[allow(clippy::too_many_arguments)]
    fn find_intersect_values(
        &self,
        line_vector: Vec3,
        offset_vector: Vec3,
        fv_x_lv: Vec3,
        tw_x_lv: Vec3,
        of_x_lv: Vec3,
        lv_x_fw: Vec3,
        u: f64,
    ) -> Option<(f64, f64)> {
        let mut num = 0.0;
        let mut v = 0.0;
        for axis in 0..3 {
            num = f64::from(fv_x_lv[axis]) + f64::from(tw_x_lv[axis]) * u;
            if approximately_zero(num) {
                continue;
            }
            v = (f64::from(of_x_lv[axis]) + f64::from(lv_x_fw[axis]) * u) / num;
            break;
        }
        if approximately_zero(num) {
            return None;
        }
        let axis = (0..3).find(|&axis| !approximately_zero(f64::from(line_vector[axis])))?;
        let t = (f64::from(0.0 - offset_vector[axis])
            + f64::from(self.first_vector[axis]) * v
            + (f64::from(self.forward_vector[axis]) + f64::from(self.twist_vector[axis]) * v) * u)
            / f64::from(line_vector[axis]);
        Some((v, t))
    }

    /// `TwistedPlane.IntersectLineOnTwistedPlane`: where the segment from
    /// `line_start` to `line_end` crosses the surface, the nearer crossing
    /// of two. `extend_first` and `extend_second` let the surface reach
    /// past its first and second edge (never further than four of its
    /// lengths).
    pub fn intersect_line(
        &self,
        line_start: Vec3,
        line_end: Vec3,
        extend_first: bool,
        extend_second: bool,
    ) -> Option<Intersection> {
        let val = line_end - line_start;
        let val2 = line_start - self.first_origin;
        let val3 = unity::cross(self.twist_vector, self.forward_vector);
        let val4 = unity::cross(self.first_vector, self.forward_vector);
        let val5 = unity::cross(val2, self.twist_vector);
        let val6 = unity::cross(val2, self.first_vector);
        let quad_a = wide_dot(val, val3);
        let quad_b = f64::from(val.x) * (f64::from(val4.x) + f64::from(val5.x))
            + f64::from(val.y) * (f64::from(val4.y) + f64::from(val5.y))
            + f64::from(val.z) * (f64::from(val4.z) + f64::from(val5.z));
        let quad_c = wide_dot(val, val6);
        let (solution, solution2) = solve_quadratic(quad_a, quad_b, quad_c)?;
        let fv_x_lv = unity::cross(self.first_vector, val);
        let tw_x_lv = unity::cross(self.twist_vector, val);
        let of_x_lv = unity::cross(val2, val);
        let lv_x_fw = unity::cross(val, self.forward_vector);
        // One root: whether it counts, and its place on the line.
        let root = |u: f64| -> (bool, f64) {
            let mut t = 0.0;
            let mut flag = false;
            if u < 4.0 && u > -4.0 && (u >= 0.0 || extend_first) && (u <= 1.0 || extend_second) {
                let found = self.find_intersect_values(val, val2, fv_x_lv, tw_x_lv, of_x_lv, lv_x_fw, u);
                flag = found.is_some();
                t = found.map_or(0.0, |(_, t)| t);
                if (flag && t < 0.0) || t > 1.0 {
                    flag = false;
                }
            }
            (flag, t)
        };
        let (flag, t) = root(solution);
        let (flag2, t2) = root(solution2);
        if !flag && !flag2 {
            return None;
        }
        let mut num = 0.0f32;
        if flag {
            num = t as f32;
        }
        if flag2 && (!flag || t2 < t) {
            num = t2 as f32;
        }
        let point = line_start + val * num;
        let val7 = cast_segment_to_segment(line_start, val, self.first_origin, self.first_vector);
        let val8 = cast_segment_to_segment(line_start, val, self.second_origin, self.second_vector);
        Some(Intersection {
            point,
            forward: unity::normalize(val8 - val7),
            distance_to_second: unity::distance(val8, point),
        })
    }

    /// `TwistedPlane.ConstrainLineToTwistedPlane`: the end of a line of
    /// `line_length` from `line_start` that would end at `line_end`, kept
    /// on the near side of the surface. Returns the end and
    /// `bHitPointFound`.
    ///
    /// An end on the near side, a line that does not cross, or a crossing
    /// further than the length leave the end alone. Otherwise the line
    /// bends at the crossing and runs along the surface for what is left
    /// of its length (a triangle, `SolveSSATriangle`). If that runs past
    /// the second edge and the surface may not reach past it, the end is
    /// aimed at the edge instead, no hit is reported and
    /// `extend_beyond_start` is set so that the next surface also catches
    /// what crosses before its first edge.
    pub fn constrain_line(
        &self,
        line_start: Vec3,
        line_end: Vec3,
        line_length: f32,
        extend_beyond_start: &mut bool,
        extend_beyond_end: bool,
    ) -> (Vec3, bool) {
        let val = unity::normalize(line_end - line_start);
        let val2 = unity::normalize(unity::cross(
            unity::normalize(self.first_vector + self.second_vector),
            self.forward_vector,
        ));
        // Unity 5.6's `new Plane(inNormal, inPoint)`: the normal at length
        // one, the distance from the normal as given. `GetSide`.
        let plane_normal = unity::normalize(val2);
        let plane_distance = 0.0 - unity::dot(val2, self.first_origin + self.forward_vector / 2.0);
        if f64::from(unity::dot(plane_normal, line_end)) + f64::from(plane_distance) <= 0.0 {
            *extend_beyond_start = false;
            return (line_end, false);
        }
        let Some(hit) = self.intersect_line(line_start, line_end, *extend_beyond_start, extend_beyond_end) else {
            *extend_beyond_start = false;
            return (line_end, false);
        };
        let num = f64::from(unity::distance(hit.point, line_start));
        if num > f64::from(line_length) {
            *extend_beyond_start = false;
            return (line_end, false);
        }
        let angle_a = deg_to_rad(f64::from(unity::angle(val, -hit.forward)));
        let side_c = solve_ssa_triangle(f64::from(line_length), num, angle_a);
        if !extend_beyond_end && side_c > f64::from(hit.distance_to_second) {
            let edge = hit.point + hit.distance_to_second * hit.forward;
            let end = line_start + unity::normalize(edge - line_start) * line_length;
            *extend_beyond_start = true;
            return (end, false);
        }
        *extend_beyond_start = false;
        (hit.point + side_c as f32 * hit.forward, true)
    }
}

/// `DanAgent.ConstrainDanToCollisionPoints`: the shaft's end held against
/// one row of points, the surfaces taken from the crotch up. The last
/// surface reaches past its far edge. Does nothing once `found` is set,
/// and sets it when a surface holds the end.
///
/// Each point sits `offset` behind its bone along the bone's forward (the
/// other way for the side that is not `inward` on the front, or is on the
/// back), `clipping_depth` deeper except on the front with belly bulge on.
///
/// As in the C#, every surface is given the unconstrained `end`, and the
/// result is that of the last surface tried: an end aimed at a surface's
/// far edge is dropped if the next surface is tried and finds nothing.
pub fn constrain_dan_to_collision_points(
    start: Vec3,
    end: Vec3,
    target_length: f32,
    found: &mut bool,
    points: &[CollisionPoint],
    o: &BodyOptions,
    front_side: bool,
) -> Vec3 {
    let mut result = end;
    if *found {
        return result;
    }
    let mut extend_beyond_start = true;
    let mut extend_beyond_end = false;
    let places: Vec<Vec3> = points
        .iter()
        .map(|p| {
            let mut num = p.offset;
            if !front_side || !o.belly_bulge {
                num += o.clipping_depth;
            }
            if front_side == p.inward {
                p.position + num * p.forward
            } else {
                p.position - num * p.forward
            }
        })
        .collect();
    for i in 1..points.len() {
        if *found {
            break;
        }
        let (a, b) = (&points[i - 1], &points[i]);
        let first = if front_side == a.inward { -a.right } else { a.right };
        let second = if front_side == b.inward { -b.right } else { b.right };
        let plane = TwistedPlane::new(places[i - 1], first, places[i], second);
        if i == points.len() - 1 {
            extend_beyond_end = true;
        }
        (result, *found) = plane.constrain_line(start, end, target_length, &mut extend_beyond_start, extend_beyond_end);
    }
    result
}

/// `DanAgent.ConstrainDanToBody`: the end of a shaft of `length` from
/// `start` along `direction`, held against the front row and, if that
/// did not hold it, the back row.
pub fn constrain_dan_to_body(
    points: &CollisionPoints,
    start: Vec3,
    direction: Vec3,
    length: f32,
    o: &BodyOptions,
) -> Vec3 {
    let end = start + direction * length;
    let mut found = false;
    let end = constrain_dan_to_collision_points(start, end, length, &mut found, &points.front, o, true);
    constrain_dan_to_collision_points(start, end, length, &mut found, &points.back, o, false)
}

/// `DanAgent.ConstrainDanToHead`: straight on, no collision.
pub fn constrain_dan_to_head(start: Vec3, direction: Vec3, length: f32) -> Vec3 {
    start + direction * length
}

/// `DanAgent.ConstrainDanToPull`: straight on, no collision.
pub fn constrain_dan_to_pull(start: Vec3, direction: Vec3, length: f32) -> Vec3 {
    start + direction * length
}

/// The shaft as `DanAgent.ConstrainDan` sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dan {
    /// The first point's place (`GetDanStartPosition`).
    pub start: Vec3,
    /// `m_baseDanLength`.
    pub base_length: f32,
    /// `m_danPenetration`: set by `SetupNewDanTarget` from the H motion
    /// (its name holds "IN", or the target is given and the pose is a
    /// "top stick" one).
    pub penetration: bool,
    /// `m_bpDanPointsFound`: the shaft is a BetterPenetration one.
    pub bp_points: bool,
    /// `lastDanEnd`: where the end bone was after the last vaginal aim.
    pub last_end: Vec3,
    /// `lastDanEndVector`: that bone's forward then, zero before the first.
    pub last_end_vector: Vec3,
}

/// `DanAgent.ConstrainDan`: the end target for the look target `mode`
/// names. `direction` is from the start to the look target, `distance`
/// that far, `length` the squished length, `end_target` the straight end
/// at the base length, `inner_limit` the simplified inner target.
///
/// While penetrating: the inner limit if the mode is simplified and the
/// shaft is a BetterPenetration one, else the body collision (vaginal and
/// anal) or straight on (oral). Otherwise only a vaginal target nearer
/// than the base length changes the end, to straight on at `length`.
#[allow(clippy::too_many_arguments)]
pub fn constrain_dan(
    mode: Mode,
    dan: &Dan,
    direction: Vec3,
    end_target: Vec3,
    inner_limit: Vec3,
    length: f32,
    distance: f32,
    points: &CollisionPoints,
    o: &BodyOptions,
) -> Vec3 {
    if dan.penetration {
        let simplify = match mode {
            Mode::Vaginal => o.simplify_vaginal,
            Mode::Anal => o.simplify_anal,
            Mode::Oral => o.simplify_oral,
        };
        if simplify && dan.bp_points {
            inner_limit
        } else if mode == Mode::Oral {
            constrain_dan_to_head(dan.start, direction, length)
        } else {
            constrain_dan_to_body(points, dan.start, direction, length, o)
        }
    } else if mode == Mode::Vaginal && dan.base_length > distance {
        constrain_dan_to_pull(dan.start, direction, length)
    } else {
        end_target
    }
}

/// The `limitCorrection` block of `DanAgent.AdjustDanToTarget*`: the end
/// target moved no further from the last end than `max_correction * dt`,
/// measured sideways of the last direction. No change before the first
/// aim (`last_end_vector` zero).
pub fn limit_correction(end_target: Vec3, last_end: Vec3, last_end_vector: Vec3, max_correction: f32, dt: f32) -> Vec3 {
    if unity::vec_eq(last_end_vector, Vec3::ZERO) {
        return end_target;
    }
    let cast = cast_to_segment(last_end, end_target, last_end_vector);
    let num = max_correction * dt;
    if unity::distance(cast, last_end) > num {
        last_end + unity::normalize(cast - last_end) * num
    } else {
        end_target
    }
}

/// `DanAgent.GetMaxDanLength`: a length that does not reach past the
/// limit point by way of the look target.
pub fn max_dan_length(length: f32, look_target: Vec3, limit_point: Vec3, distance: f32) -> f32 {
    let num2 = unity::distance(look_target, limit_point);
    if length > distance + num2 {
        distance + num2
    } else {
        length
    }
}

/// What the C# reads of the partner for one aim.
#[derive(Debug, Clone, Copy)]
pub struct Partner<'a> {
    /// `m_referenceTarget.position`: the look target bone. While
    /// penetrating that is `cf_J_Vagina_root` or `cf_J_Ana_Root` if she
    /// has it (`SetupNewDanTarget`), else `k_f_kokan_00`, `k_f_ana_00` or
    /// `k_f_head_00`.
    pub reference_position: Vec3,
    /// `m_referenceTarget.up`.
    pub reference_up: Vec3,
    /// `m_innerTarget.position` (`INNER_TARGET`), or for oral
    /// `m_innerHeadTarget.position` (`INNER_HEAD_TARGET`).
    pub inner_position: Vec3,
    pub points: &'a CollisionPoints,
}

/// The targets one aim ends with, for `AdjustDanPointsToTargets`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// `danLookTarget`: where the shaft goes in.
    pub entry: Vec3,
    /// `danEndTarget`: where its end is to be.
    pub end: Vec3,
    /// The squished length the points are laid out with.
    pub length: f32,
    /// `danDistanceToTarget`.
    pub distance: f32,
}

/// `DanAgent.AdjustDanToTargetKokan`, `AdjustDanToTargetAna` and
/// `AdjustDanToTargetHead` up to the call of `AdjustDanPointsToTargets`:
/// the look target with its offset, the squished length, the constrained
/// end and the movement limit.
///
/// As in the C#: the anal look target takes no offset and its inner limit
/// uses the vaginal inner offset; only oral caps the length
/// (`GetMaxDanLength`); the movement limit runs for every mode but
/// `last_end` is only renewed after a vaginal aim.
pub fn adjust_dan_to_target(
    mode: Mode,
    dan: &Dan,
    partner: &Partner,
    s: &Settings,
    o: &BodyOptions,
    dt: f32,
) -> Target {
    let mut entry = partner.reference_position;
    match mode {
        Mode::Vaginal => entry += o.kokan_offset * partner.reference_up,
        Mode::Oral => entry += o.mouth_offset * partner.reference_up,
        Mode::Anal => {}
    }
    let direction = unity::normalize(entry - dan.start);
    let end_target = dan.start + direction * dan.base_length;
    let distance = unity::distance(dan.start, entry);
    let (mut length, _) = squish(dan.base_length, distance, s);
    let inner_offset = if mode == Mode::Oral {
        o.inner_mouth_offset
    } else {
        o.inner_kokan_offset
    };
    let inner_limit = partner.inner_position + inner_offset * partner.reference_up;
    if mode == Mode::Oral {
        length = max_dan_length(length, entry, inner_limit, distance);
    }
    let mut end = constrain_dan(
        mode,
        dan,
        direction,
        end_target,
        inner_limit,
        length,
        distance,
        partner.points,
        o,
    );
    if o.limit_correction {
        end = limit_correction(end, dan.last_end, dan.last_end_vector, o.max_correction, dt);
    }
    Target {
        entry,
        end,
        length,
        distance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3, eps: f32) -> bool {
        (a - b).abs().max_element() <= eps
    }

    /// A flat surface in the XY plane, from y 0 to y 1, its near side
    /// towards -Z.
    fn flat() -> TwistedPlane {
        TwistedPlane::new(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::X)
    }

    #[test]
    fn defaults_are_the_plugins() {
        let o = BodyOptions::default();
        assert_eq!(o.front_offsets, [-0.04, 0.04, 0.06]);
        assert_eq!(o.back_offsets, [-0.04, 0.02, 0.0]);
        assert_eq!(o.front_inward, [false, false, false]);
        assert_eq!(o.back_inward, [false, true, true]);
        assert_eq!(o.clipping_depth, 0.02);
        assert_eq!(o.kokan_offset, 0.0);
        assert_eq!(o.inner_kokan_offset, 0.0);
        assert_eq!(o.mouth_offset, 0.0);
        assert_eq!(o.inner_mouth_offset, 0.0);
        assert!(o.belly_bulge);
        assert!(!o.simplify_vaginal && !o.simplify_anal && !o.simplify_oral);
        assert!(!o.limit_correction);
        assert_eq!(o.max_correction, 0.2);
        assert_eq!(FRONT_BONES, ["k_f_kokan_00", "a_n_waist_f", "a_n_bust_f"]);
        assert_eq!(BACK_BONES, ["k_f_ana_00", "a_n_waist_b", "a_n_back"]);
    }

    #[test]
    fn quadratic_and_triangle() {
        assert_eq!(solve_quadratic(1.0, -3.0, 2.0), Some((2.0, 1.0)));
        assert_eq!(solve_quadratic(0.0, 2.0, -1.0), Some((0.5, 0.5)));
        assert_eq!(solve_quadratic(0.0, 0.0, 1.0), None);
        assert_eq!(solve_quadratic(1.0, 0.0, 1.0), None);
        // A 3-4-5 triangle: the side opposite the right angle is 5.
        let c = solve_ssa_triangle(5.0, 3.0, std::f64::consts::FRAC_PI_2);
        assert!((c - 4.0).abs() < 1e-9);
        assert_eq!(solve_ssa_triangle(5.0, 0.0, 1.0), 0.0);
    }

    #[test]
    fn cast_to_segment_projects_on_a_unit_vector() {
        let p = cast_to_segment(Vec3::new(0.3, 2.0, -1.0), Vec3::new(0.0, 1.0, 0.0), Vec3::X);
        assert!(close(p, Vec3::new(0.3, 1.0, 0.0), 1e-6));
        // The C# divides by the length once: a vector of length two
        // doubles the step.
        let p = cast_to_segment(Vec3::new(0.3, 2.0, -1.0), Vec3::ZERO, Vec3::X * 2.0);
        assert!(close(p, Vec3::new(0.6, 0.0, 0.0), 1e-6));
    }

    #[test]
    fn flat_plane_crossing() {
        let plane = flat();
        let start = Vec3::new(0.0, 0.2, -0.3);
        let end = Vec3::new(0.0, 0.6, 0.1);
        let hit = plane.intersect_line(start, end, false, false).unwrap();
        assert!(close(hit.point, Vec3::new(0.0, 0.5, 0.0), 1e-6));
        assert!(close(hit.forward, Vec3::Y, 1e-6));
        assert!((hit.distance_to_second - 0.5).abs() < 1e-6);

        let length = (end - start).length();
        let mut extend = true;
        let (held, found) = plane.constrain_line(start, end, length, &mut extend, false);
        assert!(found);
        assert!(!extend);
        // On the surface, further along it, the length kept.
        assert!(held.z.abs() < 1e-6 && held.x.abs() < 1e-6);
        assert!((held.y - 0.6796).abs() < 1e-3, "{held}");
        assert!(((held - start).length() - length).abs() < 1e-5);
    }

    #[test]
    fn flat_plane_misses() {
        let plane = flat();
        // The end on the near side: nothing to do.
        let start = Vec3::new(0.0, 0.2, -0.3);
        let end = Vec3::new(0.0, 0.6, -0.1);
        let mut extend = true;
        assert_eq!(plane.constrain_line(start, end, 0.5, &mut extend, false), (end, false));
        assert!(!extend);
        // A segment that stops short of the surface does not cross it.
        assert!(plane.intersect_line(start, end, true, true).is_none());
        // A crossing further than the line is long.
        let end = Vec3::new(0.0, 0.6, 0.1);
        let mut extend = true;
        assert_eq!(plane.constrain_line(start, end, 0.3, &mut extend, false), (end, false));
    }

    #[test]
    fn flat_plane_edges_follow_the_flags() {
        let plane = flat();
        // Crossing before the first edge, at y -0.5.
        let start = Vec3::new(0.0, -0.8, -0.3);
        let end = Vec3::new(0.0, -0.4, 0.1);
        assert!(plane.intersect_line(start, end, false, false).is_none());
        let hit = plane.intersect_line(start, end, true, false).unwrap();
        assert!(close(hit.point, Vec3::new(0.0, -0.5, 0.0), 1e-6));
        // Crossing past the second edge, at y 1.5.
        let start = Vec3::new(0.0, 1.2, -0.3);
        let end = Vec3::new(0.0, 1.6, 0.1);
        assert!(plane.intersect_line(start, end, false, false).is_none());
        let hit = plane.intersect_line(start, end, false, true).unwrap();
        assert!(close(hit.point, Vec3::new(0.0, 1.5, 0.0), 1e-6));
        let length = (end - start).length();
        let mut extend = false;
        assert_eq!(
            plane.constrain_line(start, end, length, &mut extend, false),
            (end, false)
        );
        let mut extend = false;
        let (held, found) = plane.constrain_line(start, end, length, &mut extend, true);
        assert!(found && held.z.abs() < 1e-6 && held.y > 1.5);
        // Never further than four lengths.
        let start = Vec3::new(0.0, 4.7, -0.3);
        let end = Vec3::new(0.0, 5.1, 0.1);
        assert!(plane.intersect_line(start, end, false, true).is_none());
    }

    #[test]
    fn flat_plane_running_past_the_second_edge() {
        let plane = flat();
        // Crossing at y 0.9: what is left of the length runs past y 1.
        let start = Vec3::new(0.0, 0.6, -0.3);
        let end = Vec3::new(0.0, 1.0, 0.1);
        let length = (end - start).length();
        let mut extend = false;
        let (held, found) = plane.constrain_line(start, end, length, &mut extend, false);
        assert!(!found);
        assert!(extend);
        // Aimed at the edge (0, 1, 0), at the full length.
        let aim = (Vec3::Y - start).normalize();
        assert!(close(held, start + aim * length, 1e-5));
        // With the surface reaching on, it holds the end.
        let mut extend = false;
        let (held, found) = plane.constrain_line(start, end, length, &mut extend, true);
        assert!(found && !extend);
        assert!(held.z.abs() < 1e-6 && held.y > 1.0);
    }

    #[test]
    fn twisted_plane_crossing_lies_on_the_surface() {
        // The second edge turned 45 degrees about Y.
        let (s, c) = std::f32::consts::FRAC_PI_4.sin_cos();
        let plane = TwistedPlane::new(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::new(c, 0.0, s));
        let start = Vec3::new(0.3, 0.2, -0.3);
        let end = Vec3::new(0.3, 0.6, 0.5);
        let hit = plane.intersect_line(start, end, false, false).unwrap();
        // On the line.
        let t = (hit.point.y - start.y) / (end.y - start.y);
        assert!((0.0..=1.0).contains(&t));
        assert!(close(hit.point, start + (end - start) * t, 1e-5));
        // On the surface: (v + u v (c - 1), u, u v s).
        let u = hit.point.y;
        let v = hit.point.x / (1.0 + u * (c - 1.0));
        assert!((0.0..=1.0).contains(&u));
        assert!((hit.point.z - u * v * s).abs() < 1e-5, "{}", hit.point);
        // Off the flat plane: the twist moved it.
        assert!(hit.point.z > 0.01);

        let length = (end - start).length();
        let mut extend = true;
        let (held, found) = plane.constrain_line(start, end, length, &mut extend, false);
        assert!(found);
        assert!(((held - start).length() - length).abs() < 1e-4);
        // A line that stays on the near side of the twisted surface.
        let end = Vec3::new(0.3, 0.6, -0.1);
        let mut extend = true;
        assert_eq!(plane.constrain_line(start, end, 0.5, &mut extend, false), (end, false));
    }

    /// A partner facing +Z: the front row at z 0.04 after its offsets, the
    /// back row's bones turned half round.
    fn partner() -> CollisionPoints {
        let o = BodyOptions::default();
        let names: Vec<&str> = FRONT_BONES.iter().chain(&BACK_BONES).copied().collect();
        let at = |x: f32, y: f32, z: f32| Mat4::from_translation(Vec3::new(x, y, z));
        let turned = |y: f32, z: f32| at(0.0, y, z) * Mat4::from_rotation_y(std::f32::consts::PI);
        let world = [
            at(0.0, 1.0, 0.0),
            at(0.0, 1.15, 0.08),
            at(0.0, 1.35, 0.10),
            at(0.0, 1.0, -0.06),
            turned(1.15, -0.10),
            turned(1.35, -0.12),
        ];
        CollisionPoints::find(&world, |n| names.iter().position(|&m| m == n), &o).unwrap()
    }

    #[test]
    fn points_come_from_the_named_bones() {
        let p = partner();
        assert_eq!(p.front.len(), 3);
        assert_eq!(p.back.len(), 3);
        assert_eq!(p.front[1].position, Vec3::new(0.0, 1.15, 0.08));
        assert_eq!(p.front[1].offset, 0.04);
        assert!(close(p.back[1].forward, -Vec3::Z, 1e-6));
        assert!(close(p.back[1].right, -Vec3::X, 1e-6));
        assert!(p.back[1].inward && !p.back[0].inward);
        // A missing bone: no points.
        let o = BodyOptions::default();
        assert!(CollisionPoints::find(&[Mat4::IDENTITY], |n| (n == "k_f_kokan_00").then_some(0), &o).is_none());
    }

    #[test]
    fn body_keeps_the_end_behind_the_front() {
        let p = partner();
        let o = BodyOptions::default();
        let start = Vec3::new(0.0, 0.95, -0.05);
        let direction = Vec3::new(0.0, 0.6, 0.8);
        let length = 0.19;
        let straight = start + direction * length;
        assert!(straight.z > 0.04);
        let end = constrain_dan_to_body(&p, start, direction, length, &o);
        assert!(end.z <= 0.04 + 1e-5, "{end}");
        assert!((end.z - 0.04).abs() < 1e-5);
        assert!(end.y > 1.0175);
        assert!(((end - start).length() - length).abs() < 1e-5);
    }

    #[test]
    fn body_keeps_the_end_before_the_back() {
        let p = partner();
        let o = BodyOptions::default();
        // The back row sits at z -0.08, -0.06 and -0.10 after its offsets.
        let start = Vec3::new(0.0, 0.95, 0.0);
        let direction = Vec3::new(0.0, 0.6, -0.8);
        let length = 0.19;
        let straight = start + direction * length;
        assert!(straight.z < -0.1);
        let end = constrain_dan_to_body(&p, start, direction, length, &o);
        assert!(end.z > straight.z + 0.03, "{end}");
        assert!(((end - start).length() - length).abs() < 1e-4);
    }

    #[test]
    fn body_leaves_a_shaft_inside_alone() {
        let p = partner();
        let o = BodyOptions::default();
        let start = Vec3::new(0.0, 0.95, -0.02);
        let end = constrain_dan_to_body(&p, start, Vec3::Y, 0.19, &o);
        assert_eq!(end, start + Vec3::Y * 0.19);
    }

    #[test]
    fn front_takes_the_clipping_depth_only_without_belly_bulge() {
        let p = partner();
        let start = Vec3::new(0.0, 0.95, -0.05);
        let direction = Vec3::new(0.0, 0.6, 0.8);
        let o = BodyOptions {
            belly_bulge: false,
            ..BodyOptions::default()
        };
        let end = constrain_dan_to_body(&p, start, direction, 0.19, &o);
        // The front surface 0.02 deeper: z 0.02.
        assert!((end.z - 0.02).abs() < 1e-5, "{end}");
    }

    fn dan(penetration: bool, bp_points: bool) -> Dan {
        Dan {
            start: Vec3::new(0.0, 0.95, -0.05),
            base_length: 0.19,
            penetration,
            bp_points,
            last_end: Vec3::ZERO,
            last_end_vector: Vec3::ZERO,
        }
    }

    #[test]
    fn constrain_dan_picks_by_mode() {
        let p = partner();
        let o = BodyOptions::default();
        let direction = Vec3::new(0.0, 0.6, 0.8);
        let inner = Vec3::new(0.0, 1.1, 0.0);
        let given = Vec3::splat(9.0);
        let d = dan(true, true);
        let straight = d.start + direction * 0.15;
        let run = |mode, d: &Dan, o: &BodyOptions, distance| {
            constrain_dan(mode, d, direction, given, inner, 0.15, distance, &p, o)
        };
        // Penetrating: body for vaginal and anal, straight for oral.
        let body = constrain_dan_to_body(&p, d.start, direction, 0.15, &o);
        assert_ne!(body, straight);
        assert_eq!(run(Mode::Vaginal, &d, &o, 0.1), body);
        assert_eq!(run(Mode::Anal, &d, &o, 0.1), body);
        assert_eq!(run(Mode::Oral, &d, &o, 0.1), straight);
        // Simplified: the inner limit, only for a BetterPenetration shaft.
        let simple = BodyOptions {
            simplify_vaginal: true,
            ..BodyOptions::default()
        };
        assert_eq!(run(Mode::Vaginal, &d, &simple, 0.1), inner);
        assert_eq!(run(Mode::Anal, &d, &simple, 0.1), body);
        assert_eq!(run(Mode::Vaginal, &dan(true, false), &simple, 0.1), body);
        // Not penetrating: straight at the squished length for a near
        // vaginal target, else what was given.
        let d = dan(false, true);
        assert_eq!(run(Mode::Vaginal, &d, &o, 0.1), straight);
        assert_eq!(run(Mode::Vaginal, &d, &o, 0.3), given);
        assert_eq!(run(Mode::Anal, &d, &o, 0.1), given);
        assert_eq!(run(Mode::Oral, &d, &o, 0.1), given);
    }

    #[test]
    fn limit_correction_holds_sideways_movement() {
        let last = Vec3::new(0.0, 1.0, 0.0);
        let target = Vec3::new(0.1, 1.0, 0.0);
        // Before the first aim: no change.
        assert_eq!(limit_correction(target, last, Vec3::ZERO, 0.2, 0.01), target);
        // The last end cast on the line through the target along the last
        // direction is 0.1 away: more than 0.2 * 0.01.
        let held = limit_correction(target, last, Vec3::Z, 0.2, 0.01);
        assert!(close(held, Vec3::new(0.002, 1.0, 0.0), 1e-6), "{held}");
        // Movement along the last direction is not sideways.
        let along = Vec3::new(0.0, 1.0, 0.3);
        assert_eq!(limit_correction(along, last, Vec3::Z, 0.2, 0.01), along);
        // Within the limit: the target as it is.
        assert_eq!(limit_correction(target, last, Vec3::Z, 0.2, 1.0), target);
    }

    #[test]
    fn max_dan_length_stops_at_the_limit_point() {
        let look = Vec3::new(0.0, 0.0, 0.1);
        let limit = Vec3::new(0.0, 0.0, 0.15);
        assert!((max_dan_length(0.19, look, limit, 0.1) - 0.15).abs() < 1e-6);
        assert_eq!(max_dan_length(0.12, look, limit, 0.1), 0.12);
    }

    #[test]
    fn adjust_dan_to_target_follows_the_csharp_order() {
        let p = partner();
        let s = Settings::default();
        let mut o = BodyOptions::default();
        let d = dan(true, true);
        let reference = Vec3::new(0.0, 1.0, 0.0);
        let side = Partner {
            reference_position: reference,
            reference_up: Vec3::Y,
            inner_position: Vec3::new(0.0, 1.1, 0.0),
            points: &p,
        };
        let t = adjust_dan_to_target(Mode::Vaginal, &d, &side, &s, &o, 0.016);
        let distance = unity::distance(d.start, reference);
        let (length, _) = squish(d.base_length, distance, &s);
        assert_eq!(t.entry, reference);
        assert_eq!(t.distance, distance);
        assert_eq!(t.length, length);
        let direction = unity::normalize(reference - d.start);
        assert_eq!(t.end, constrain_dan_to_body(&p, d.start, direction, length, &o));
        // The vaginal offset moves the entry along the target's up; anal
        // takes none.
        o.kokan_offset = 0.01;
        let t = adjust_dan_to_target(Mode::Vaginal, &d, &side, &s, &o, 0.016);
        assert!(close(t.entry, reference + Vec3::Y * 0.01, 1e-7));
        let t = adjust_dan_to_target(Mode::Anal, &d, &side, &s, &o, 0.016);
        assert_eq!(t.entry, reference);
        // Oral: the length stops at the inner limit.
        o.kokan_offset = 0.0;
        let near = Partner {
            inner_position: reference + Vec3::Y * 0.01,
            ..side
        };
        let t = adjust_dan_to_target(Mode::Oral, &d, &near, &s, &o, 0.016);
        assert!((t.length - (distance + 0.01)).abs() < 1e-6);
        let t = adjust_dan_to_target(Mode::Vaginal, &d, &near, &s, &o, 0.016);
        assert_eq!(t.length, length);
        // The movement limit, once there is a last end.
        o.limit_correction = true;
        let free = adjust_dan_to_target(Mode::Anal, &d, &side, &s, &o, 0.016);
        let moved = Dan {
            last_end: free.end + Vec3::X * 0.1,
            last_end_vector: Vec3::Y,
            ..d
        };
        let held = adjust_dan_to_target(Mode::Anal, &moved, &side, &s, &o, 0.016);
        assert!((unity::distance(held.end, moved.last_end) - 0.2 * 0.016).abs() < 1e-6);
    }

    #[test]
    fn the_plugins_config_file_sets_the_options() {
        let text = "[Female Options]\nClipping Depth = 0.03\nClipping Offset: Front Collision 1 = 0.08\n\
                    Target Offset: Mouth Target = 0.01\n[Male 1 Options]\nLimit Penis Movement = true\nRotate Balls with Shaft = false\nPenis: Squish Oral Girth = true\n\
                    Limit Penis Amount = 0.3\n[Male 2 Options]\nLimit Penis Amount = 0.9\n";
        let o = BodyOptions::from_config(text);
        assert_eq!(o.clipping_depth, 0.03);
        assert_eq!(o.front_offsets, [-0.04, 0.08, 0.06]);
        assert_eq!(o.mouth_offset, 0.01);
        assert!(o.limit_correction);
        assert_eq!(o.max_correction, 0.3, "the second male's is not the first's");
        assert_eq!(o.back_offsets, BodyOptions::default().back_offsets);
        assert!(!o.rotate_balls && o.squish_oral_girth);
    }
}
