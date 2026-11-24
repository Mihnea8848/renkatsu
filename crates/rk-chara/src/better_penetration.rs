// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The BetterPenetration plugin (`KK_Studio_BetterPenetration` 4.5.5.2):
//! the male shaft aimed and bent at a target on a partner, its colliders
//! given to her dynamic bones, and the mouth pulled.
//!
//! This part is the settings, as its controller keeps them on the card and
//! in scenes (`BetterPenetrationController.OnCardBeingSaved` and
//! `OnReload`): extended data `BetterPenetrationController`, version 1.
//! A value the data does not hold keeps the controller's default.

use crate::scene::{Scene, Trs};
use glam::{Mat4, Quat, Vec3};
use rk_card::{ExtData, Value};
use rk_ik::hierarchy::{Hierarchy, Transform};
use rk_ik::unity;
use serde::{Deserialize, Serialize};

/// The extended data's key, the controller's name with KKAPI.
pub const KEY: &str = "BetterPenetrationController";

/// What the shaft looks for on its own (`ControllerOptions.AutoTarget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoTarget {
    #[default]
    Off,
    Vaginal,
    Anal,
    Oral,
}

impl AutoTarget {
    /// The plugin's number for it.
    pub fn of_number(n: i64) -> AutoTarget {
        match n {
            1 => AutoTarget::Vaginal,
            2 => AutoTarget::Anal,
            3 => AutoTarget::Oral,
            _ => AutoTarget::Off,
        }
    }

    pub const ALL: [AutoTarget; 4] = [AutoTarget::Off, AutoTarget::Vaginal, AutoTarget::Anal, AutoTarget::Oral];

    pub fn label(self) -> &'static str {
        match self {
            AutoTarget::Off => "Off",
            AutoTarget::Vaginal => "Vaginal",
            AutoTarget::Anal => "Anal",
            AutoTarget::Oral => "Oral",
        }
    }
}

/// Where the shaft goes on a partner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Vaginal,
    Anal,
    Oral,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Vaginal, Mode::Anal, Mode::Oral];

    /// The mode auto-target looks for, if any.
    pub fn of_auto(target: AutoTarget) -> Option<Mode> {
        match target {
            AutoTarget::Off => None,
            AutoTarget::Vaginal => Some(Mode::Vaginal),
            AutoTarget::Anal => Some(Mode::Anal),
            AutoTarget::Oral => Some(Mode::Oral),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Vaginal => "Vaginal",
            Mode::Anal => "Anal",
            Mode::Oral => "Oral",
        }
    }

    /// The partner's bone the first target (`k_f_dan_entry`) is held to,
    /// which auto-target also looks for (`CheckAutoTarget`).
    pub fn entry_bone(self) -> &'static str {
        match self {
            Mode::Vaginal => "cf_J_Vagina_root",
            Mode::Anal => "cf_j_ana",
            Mode::Oral => "cf_J_MouthLow",
        }
    }

    /// Where on that bone, in its own space: a little below the lower lip
    /// for the mouth.
    pub fn entry_offset(self) -> Vec3 {
        match self {
            Mode::Oral => Vec3::new(0.0, -0.01, 0.0),
            _ => Vec3::ZERO,
        }
    }

    /// The partner's bone the second target (`k_f_dan_end`) is held to.
    pub fn end_bone(self) -> &'static str {
        match self {
            Mode::Oral => "cf_J_FaceBase",
            _ => "cf_j_waist01",
        }
    }
}

/// The targets on a partner, in the world: where the shaft goes in, and
/// where it heads past that. NodesConstraints puts each at
/// `parent.TransformPoint(offset)` with its default locks and factors.
pub fn targets(mode: Mode, partner_world: &[Mat4], find: impl Fn(&str) -> Option<usize>) -> Option<(Vec3, Vec3)> {
    let entry = partner_world[find(mode.entry_bone())?].transform_point3(mode.entry_offset());
    let end = partner_world[find(mode.end_bone())?].w_axis.truncate();
    Some((entry, end))
}

/// The male's shaft bones (`DanAgent.InitializeDan`).
#[derive(Debug, Clone, PartialEq)]
pub struct Shaft {
    /// The `cm_J_dan101_00` to `cm_J_dan109_00` the body has, in order.
    pub points: Vec<usize>,
    /// The virtual points `cm_J_vdan102_00` to `cm_J_vdan108_00`.
    pub virtual_points: Vec<usize>,
    /// `cm_J_dan_f_top`, the balls' top, and `cm_J_dan119_00`, the end.
    pub balls_top: usize,
    pub end: Option<usize>,
    /// The length from the first point, measured in the pose it was found
    /// in: the first two points' distance times the segments.
    pub base_length: f32,
    /// Each point's local scale as found (`DanPoint.defaultLocalScale`).
    pub scales: Vec<Vec3>,
}

/// The shaft bones' names in order (`BoneNames.DanBones`).
pub const SHAFT_BONES: [&str; 9] = [
    "cm_J_dan101_00",
    "cm_J_dan102_00",
    "cm_J_dan103_00",
    "cm_J_dan104_00",
    "cm_J_dan105_00",
    "cm_J_dan106_00",
    "cm_J_dan107_00",
    "cm_J_dan108_00",
    "cm_J_dan109_00",
];

/// The virtual points' names (`BoneNames.VirtualDanBones`).
pub const VIRTUAL_BONES: [&str; 4] = [
    "cm_J_vdan102_00",
    "cm_J_vdan104_00",
    "cm_J_vdan106_00",
    "cm_J_vdan108_00",
];

impl Shaft {
    /// The shaft of a character, when it has at least two points and the
    /// balls' top; the base length from `world`.
    pub fn find(scene: &Scene, world: &[Mat4], find: impl Fn(&str) -> Option<usize>) -> Option<Shaft> {
        let points: Vec<usize> = SHAFT_BONES.iter().filter_map(|n| find(n)).collect();
        let virtual_points = VIRTUAL_BONES.iter().filter_map(|n| find(n)).collect();
        let balls_top = find("cm_J_dan_f_top")?;
        if points.len() < 2 {
            return None;
        }
        let at = |i: usize| world[points[i]].w_axis.truncate();
        let base_length = at(0).distance(at(1)) * (points.len() - 1) as f32;
        let scales = points.iter().map(|&p| scene.nodes[p].local.scale).collect();
        Some(Shaft {
            points,
            virtual_points,
            balls_top,
            end: find("cm_J_dan119_00"),
            base_length,
            scales,
        })
    }

    /// A body made for the plugin: all nine points, each with its collider
    /// (`m_bpColliderBonesFound`); others bend no more than straight.
    pub fn made_for_it(&self) -> bool {
        self.points.len() == SHAFT_BONES.len()
    }

    /// Where its tip is (`DanPoints.GetDanEndPosition`).
    pub fn tip(&self, world: &[Mat4]) -> Vec3 {
        world[*self.points.last().unwrap_or(&self.balls_top)].w_axis.truncate()
    }
}

/// The shaft's new places for a pair of targets (`DanAgent.SetDanTarget`),
/// before they are set on its bones.
#[derive(Debug, Clone, PartialEq)]
pub struct Solve {
    /// Where each point goes, the first staying where it is.
    pub points: Vec<Vec3>,
    /// Where each virtual point goes, vaginal only, else none.
    pub virtual_points: Vec<Vec3>,
    /// How much thicker it grows (`GetSquishedDanGirth`).
    pub girth: f32,
}

/// The length the shaft takes against a target `distance` away, and its
/// girth: a target closer than the length less the threshold's share of it
/// takes the difference off the length by `length_squish` and adds it to
/// the girth by `girth_squish` (`GetSquishedDanLength`,
/// `GetSquishedDanGirth`).
pub fn squish(base_length: f32, distance: f32, s: &Settings) -> (f32, f32) {
    let over = base_length - base_length * s.squish_threshold - distance;
    if over > 0.0 {
        (
            base_length - over * s.length_squish,
            1.0 + over / base_length * s.girth_squish,
        )
    } else {
        (base_length, 1.0)
    }
}

/// The places of the points for targets `entry` and `end`, from where they
/// are (`AdjustDanPointsToTargets`). The shaft goes straight at the entry
/// and turns there towards the end, each segment keeping its length: the
/// segment that reaches past the entry is the side of a triangle with the
/// line past it (law of sines). A shaft of only two points, a length that
/// does not reach the entry or a straight line through both targets goes
/// straight. The length is no more than reaches the end
/// (`GetMaxDanLength`).
pub fn solve(
    starts: &[Vec3],
    virtual_starts: &[Vec3],
    entry: Vec3,
    end: Vec3,
    base_length: f32,
    s: &Settings,
    vaginal: bool,
) -> Solve {
    let mut points = starts.to_vec();
    let Some(&start) = starts.first() else {
        return Solve {
            points,
            virtual_points: Vec::new(),
            girth: 1.0,
        };
    };
    let distance = unity::distance(start, entry);
    let (length, girth) = squish(base_length, distance, s);
    let length = length.min(distance + unity::distance(entry, end));
    let mut v1 = unity::normalize(entry - start);
    let mut v2 = unity::normalize(end - entry);
    let mut straight = false;
    if starts.len() <= 2 {
        v1 = unity::normalize(end - start);
        v2 = v1;
        straight = true;
    } else if vectors_equal(v1, v2, 0.001) || length <= distance {
        v2 = v1;
        straight = true;
    }
    let turn = f64::from(unity::angle(v1, -v2)).to_radians();
    let segment = length / (starts.len() - 1) as f32;
    for i in 1..points.len() {
        let left = distance - segment * (i - 1) as f32;
        points[i] = if straight || f64::from(left) < 0.001 {
            points[i - 1] + v2 * segment
        } else if f64::from(left) < f64::from(segment) - 0.001 {
            entry + v2 * past_entry(segment, left, turn)
        } else {
            points[i - 1] + v1 * segment
        };
    }
    // The virtual points reach on at the unsquished length, one segment
    // and then two at a time.
    let mut virtual_points = Vec::new();
    if vaginal && !virtual_starts.is_empty() {
        virtual_points = virtual_starts.to_vec();
        let full = base_length / (starts.len() - 1) as f32;
        for j in 0..virtual_points.len() {
            let (step, left, from) = if j == 0 {
                (full, distance - full, points[0])
            } else {
                (2.0 * full, distance - full * (2 * j - 1) as f32, virtual_points[j - 1])
            };
            virtual_points[j] = if straight || f64::from(left) < 0.001 {
                from + v2 * step
            } else if f64::from(left) < f64::from(step) - 0.001 {
                entry + v2 * past_entry(step, left, turn)
            } else {
                from + v1 * step
            };
        }
    }
    Solve {
        points,
        virtual_points,
        girth,
    }
}

/// How far past the entry a segment of `side` ends that starts `left`
/// before it, the shaft turning by `turn` there (`SolveSSATriangle`, in
/// double precision).
fn past_entry(side: f32, left: f32, turn: f64) -> f32 {
    let (a, b) = (f64::from(side), f64::from(left));
    let zero = |v: f64| v.abs() < 1e-6;
    if zero(a) || zero(b) || zero(turn) {
        return 0.0;
    }
    let angle_b = (turn.sin() * b / a).asin();
    let angle_c = std::f64::consts::PI - turn - angle_b;
    (a * angle_c.sin() / turn.sin()) as f32
}

/// `MathHelpers.VectorsEqual`: every component within `threshold`.
fn vectors_equal(a: Vec3, b: Vec3, threshold: f32) -> bool {
    (a.x - b.x).abs() <= threshold && (a.y - b.y).abs() <= threshold && (a.z - b.z).abs() <= threshold
}

impl Shaft {
    /// Aims the shaft at `entry` and `end` in the character's scene, as
    /// `SetDanTarget` does: its girth, then each point placed and turned
    /// to the next (`LookRotation` with the balls' top's right side for
    /// its roll), the last as the one before, the end bone on the tip, and
    /// the balls' top turned as the first point.
    pub fn aim(&self, scene: &mut Scene, entry: Vec3, end: Vec3, s: &Settings, vaginal: bool) {
        let mut h = Hierarchy::new(
            scene
                .nodes
                .iter()
                .map(|n| Transform {
                    parent: n.parent,
                    local_position: n.local.pos,
                    local_rotation: n.local.rot,
                    local_scale: n.local.scale,
                })
                .collect(),
        );
        let starts: Vec<Vec3> = self.points.iter().map(|&p| h.position(p)).collect();
        let distance = starts.first().map_or(0.0, |&p| unity::distance(p, entry));
        // `SquishDanGirth`: the second point thicker, the ones after it
        // each a little less, so that the scales carried down taper back.
        let (_, girth) = squish(self.base_length, distance, s);
        let n = self.points.len() as f32 - 2.0;
        if n > 0.0 {
            let rest = (girth - (girth - 1.0) / n) / girth;
            for (i, (&p, default)) in self.points.iter().zip(&self.scales).enumerate().skip(1) {
                let g = if i == 1 { girth } else { rest };
                let t = &mut h.transforms[p];
                t.local_scale = Vec3::new(default.x * g, default.y * g, t.local_scale.z);
            }
        }
        let starts: Vec<Vec3> = self.points.iter().map(|&p| h.position(p)).collect();
        let virtual_starts: Vec<Vec3> = self.virtual_points.iter().map(|&p| h.position(p)).collect();
        let solved = solve(&starts, &virtual_starts, entry, end, self.base_length, s, vaginal);
        let right = unity::rotate(h.rotation(self.balls_top), Vec3::X);
        let aim = |h: &mut Hierarchy, nodes: &[usize], places: &[Vec3]| {
            let mut turn = Quat::IDENTITY;
            for i in 0..nodes.len() {
                if i + 1 < nodes.len() {
                    let d = unity::normalize(places[i + 1] - places[i]);
                    turn = unity::look_rotation(d, unity::cross(d, right));
                }
                h.set_position(nodes[i], places[i]);
                h.set_rotation(nodes[i], turn);
            }
            turn
        };
        let last = aim(&mut h, &self.points, &solved.points);
        if let (Some(end_bone), Some(&tip)) = (self.end, solved.points.last()) {
            h.set_position(end_bone, tip);
            h.set_rotation(end_bone, last);
        }
        if let Some(&first) = self.points.first() {
            let turn = h.rotation(first);
            h.set_rotation(self.balls_top, turn);
        }
        if solved.virtual_points.len() == self.virtual_points.len() {
            aim(&mut h, &self.virtual_points, &solved.virtual_points);
        }
        let moved = self
            .points
            .iter()
            .chain(&self.virtual_points)
            .chain(self.end.iter())
            .chain(std::iter::once(&self.balls_top));
        for &i in moved {
            let t = &h.transforms[i];
            scene.nodes[i].local = Trs {
                pos: t.local_position,
                rot: t.local_rotation,
                scale: t.local_scale,
            };
        }
    }
}

/// The character's bones by name as the plugin finds them
/// (`Tools.GetTransformOfChaControl`): among the active objects below
/// `top` in Unity's order, depth first, the last of a name. Bones an
/// uncensor implanted come after the body's own of the same name.
pub fn bones_by_name(scene: &crate::scene::Scene, top: usize) -> std::collections::HashMap<String, usize> {
    let mut out = std::collections::HashMap::new();
    let mut stack = vec![top];
    while let Some(node) = stack.pop() {
        let n = &scene.nodes[node];
        if !n.active {
            continue;
        }
        out.insert(n.name.clone(), node);
        stack.extend(n.children.iter().rev().copied());
    }
    out
}

/// Auto-target's pick among the candidates' target bones: the nearest to
/// the tip closer than twice the base length (`CheckAutoTarget`).
pub fn nearest<T: Copy>(tip: Vec3, base_length: f32, candidates: impl IntoIterator<Item = (T, Vec3)>) -> Option<T> {
    let mut best = None;
    let mut reach = base_length * 2.0;
    for (who, at) in candidates {
        let d = tip.distance(at);
        if d < reach {
            reach = d;
            best = Some(who);
        }
    }
    best
}

/// One character's settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// The controller runs (`Behaviour.enabled`).
    pub enabled: bool,
    /// How much of the length gives way against a target closer than it
    /// (`danLengthSquish`).
    pub length_squish: f32,
    /// How much thicker it grows as it does (`danGirthSquish`).
    pub girth_squish: f32,
    /// The share of the length that may close in before it gives way
    /// (`squishThreshold`).
    pub squish_threshold: f32,
    /// The shaft colliders' radius and length, as shares of their own
    /// (`danRadiusScale`, `danLengthScale`).
    pub collider_radius_scale: f32,
    pub collider_length_scale: f32,
    pub auto_target: AutoTarget,
    /// The mouth's furthest push and pull, and how fast it follows and
    /// returns (`maxOralPush`, `maxOralPull`, `oralPullRate`,
    /// `oralReturnRate`).
    pub max_push: f32,
    pub max_pull: f32,
    pub pull_rate: f32,
    pub return_rate: f32,
    /// The deeper virtual points push the belly's dynamic bones, their
    /// radius times `belly_bulge_scale`.
    pub belly_bulge: bool,
    pub belly_bulge_scale: f32,
}

impl Default for Settings {
    /// The controller's defaults in `OnReload`.
    fn default() -> Self {
        Settings {
            enabled: true,
            length_squish: 0.6,
            girth_squish: 0.2,
            squish_threshold: 0.2,
            collider_radius_scale: 1.0,
            collider_length_scale: 1.0,
            auto_target: AutoTarget::Off,
            max_push: 0.002,
            max_pull: 0.01,
            pull_rate: 1.8,
            return_rate: 0.03,
            belly_bulge: false,
            belly_bulge_scale: 1.0,
        }
    }
}

/// A card's settings, the defaults where it holds none.
pub fn read(ext: &ExtData) -> Settings {
    let mut s = Settings::default();
    let Some(p) = ext.get(KEY) else { return s };
    let float = |key: &str, into: &mut f32| {
        if let Some(v) = p.data.get(key).and_then(Value::as_f64) {
            *into = v as f32;
        }
    };
    let flag = |key: &str, into: &mut bool| {
        if let Some(v) = p.data.get(key).and_then(Value::as_bool) {
            *into = v;
        }
    };
    flag("Enabled", &mut s.enabled);
    float("LengthSquish", &mut s.length_squish);
    float("GirthSquish", &mut s.girth_squish);
    float("SquishThreshold", &mut s.squish_threshold);
    float("ColliderRadiusScale", &mut s.collider_radius_scale);
    float("ColliderLengthScale", &mut s.collider_length_scale);
    if let Some(n) = p.data.get("DanAutoTarget").and_then(Value::as_i64) {
        s.auto_target = AutoTarget::of_number(n);
    }
    float("MaxPush", &mut s.max_push);
    float("MaxPull", &mut s.max_pull);
    float("PullRate", &mut s.pull_rate);
    float("ReturnRate", &mut s.return_rate);
    flag("EnableBellyBulge", &mut s.belly_bulge);
    float("BellyBulgeScale", &mut s.belly_bulge_scale);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_without_the_data_has_the_defaults() {
        assert_eq!(read(&ExtData::default()), Settings::default());
    }

    #[test]
    fn the_plugins_numbers_name_its_targets() {
        assert_eq!(AutoTarget::of_number(0), AutoTarget::Off);
        assert_eq!(AutoTarget::of_number(1), AutoTarget::Vaginal);
        assert_eq!(AutoTarget::of_number(2), AutoTarget::Anal);
        assert_eq!(AutoTarget::of_number(3), AutoTarget::Oral);
        assert_eq!(AutoTarget::of_number(9), AutoTarget::Off);
    }

    #[test]
    fn targets_sit_on_their_bones_and_the_mouths_below_the_lip() {
        let names = ["cf_J_MouthLow", "cf_J_FaceBase", "cf_J_Vagina_root", "cf_j_waist01"];
        let world = vec![
            Mat4::from_scale_rotation_translation(Vec3::splat(2.0), glam::Quat::IDENTITY, Vec3::new(0.0, 1.4, 0.1)),
            Mat4::from_translation(Vec3::new(0.0, 1.45, 0.0)),
            Mat4::from_translation(Vec3::new(0.0, 0.8, 0.05)),
            Mat4::from_translation(Vec3::new(0.0, 0.9, 0.0)),
        ];
        let find = |n: &str| names.iter().position(|x| *x == n);
        let (entry, end) = targets(Mode::Oral, &world, find).unwrap();
        assert!(
            (entry - Vec3::new(0.0, 1.38, 0.1)).length() < 1e-5,
            "0.01 below, in the bone's scaled space"
        );
        assert_eq!(end, Vec3::new(0.0, 1.45, 0.0));
        let (entry, end) = targets(Mode::Vaginal, &world, find).unwrap();
        assert_eq!((entry, end), (Vec3::new(0.0, 0.8, 0.05), Vec3::new(0.0, 0.9, 0.0)));
        assert!(targets(Mode::Anal, &world, find).is_none(), "no cf_j_ana here");
    }

    #[test]
    fn auto_target_takes_the_nearest_within_twice_the_length() {
        let tip = Vec3::ZERO;
        let picks = [
            (1, Vec3::new(0.3, 0.0, 0.0)),
            (2, Vec3::new(0.25, 0.0, 0.0)),
            (3, Vec3::new(0.5, 0.0, 0.0)),
        ];
        assert_eq!(nearest(tip, 0.19, picks), Some(2));
        assert_eq!(nearest(tip, 0.1, picks), None, "all are further than twice 0.1");
    }

    fn shaft(n: usize, length: f32) -> Vec<Vec3> {
        (0..n)
            .map(|i| Vec3::new(0.0, 0.0, length * i as f32 / (n - 1) as f32))
            .collect()
    }

    #[test]
    fn a_close_target_squishes_the_shaft_shorter_and_thicker() {
        let s = Settings::default();
        assert_eq!(squish(0.19, 0.5, &s), (0.19, 1.0), "far: as it is");
        // 0.19 - 0.038 - 0.1 = 0.052 over: 0.6 of it off the length, 0.2 of
        // its share of the length onto the girth.
        let (length, girth) = squish(0.19, 0.1, &s);
        assert!((length - (0.19 - 0.052 * 0.6)).abs() < 1e-6);
        assert!((girth - (1.0 + 0.052 / 0.19 * 0.2)).abs() < 1e-6);
    }

    #[test]
    fn the_shaft_goes_straight_at_the_entry_and_turns_there() {
        let s = Settings {
            length_squish: 0.0,
            girth_squish: 0.0,
            ..Settings::default()
        };
        let starts = shaft(9, 0.19);
        // The entry 0.1 ahead and a little up, the end straight up from it.
        let entry = Vec3::new(0.0, 0.02, 0.1);
        let end = entry + Vec3::new(0.0, 0.2, 0.0);
        let solved = solve(&starts, &[], entry, end, 0.19, &s, false);
        let p = &solved.points;
        assert_eq!(p[0], starts[0], "the first point stays");
        let segment = 0.19 / 8.0;
        for w in p.windows(2) {
            assert!(
                (w[0].distance(w[1]) - segment).abs() < 1e-4,
                "each segment keeps its length"
            );
        }
        let v1 = entry.normalize();
        let (before, after): (Vec<Vec3>, Vec<Vec3>) = p.iter().partition(|q| q.length() <= entry.length() + 1e-5);
        for q in before {
            assert!(
                q.normalize_or_zero().dot(v1) > 0.9999 || q.length() < 1e-6,
                "{q:?} on the way in"
            );
        }
        for q in after {
            let off = q - entry;
            assert!(
                off.x.abs() < 1e-4 && off.z.abs() < 1e-4 && off.y > 0.0,
                "{q:?} past the entry, towards the end"
            );
        }
    }

    #[test]
    fn a_far_target_or_two_points_go_straight() {
        let s = Settings::default();
        let entry = Vec3::new(0.0, 0.1, 0.5);
        let solved = solve(&shaft(9, 0.19), &[], entry, entry + Vec3::Y, 0.19, &s, false);
        let tip = *solved.points.last().unwrap();
        assert!(
            (tip - entry.normalize() * 0.19).length() < 1e-4,
            "straight at the entry, at its length"
        );
        let two = solve(&shaft(2, 0.19), &[], entry, entry + Vec3::Y, 0.19, &s, false);
        let towards_end = (entry + Vec3::Y).normalize();
        assert!(
            (two.points[1] - towards_end * 0.19).length() < 1e-4,
            "two points aim at the end"
        );
    }

    #[test]
    fn the_length_stops_at_the_end() {
        let s = Settings {
            length_squish: 0.0,
            ..Settings::default()
        };
        let entry = Vec3::new(0.0, 0.0, 0.05);
        let end = Vec3::new(0.0, 0.0, 0.08);
        let solved = solve(&shaft(9, 0.19), &[], entry, end, 0.19, &s, false);
        assert!(
            (solved.points.last().unwrap().z - 0.08).abs() < 1e-4,
            "no further than the end"
        );
    }

    #[test]
    fn settings_keep_through_a_scene_file() {
        let s = Settings {
            auto_target: AutoTarget::Oral,
            girth_squish: 0.5,
            belly_bulge: true,
            ..Settings::default()
        };
        let text = toml::to_string(&s).unwrap();
        assert!(text.contains("auto_target = \"oral\""));
        assert_eq!(toml::from_str::<Settings>(&text).unwrap(), s);
    }

    #[test]
    fn a_saved_card_reads_its_settings() {
        let Some(dir) = rk_core::Config::load().ok().map(|c| c.game_dir) else {
            return;
        };
        let path = dir.join("UserData/chara/export/_CharacterExport_2026-05-13-22-13-10_01_Male.png");
        let Ok(card) = rk_card::Card::load(&path) else {
            eprintln!("skipped: no card with BetterPenetration data");
            return;
        };
        assert!(card.ext.contains_key(KEY), "the card carries the plugin's data");
        let s = read(&card.ext);
        assert!((0.0..=2.0).contains(&s.girth_squish) && (0.5..=1.5).contains(&s.collider_radius_scale));
    }
}
