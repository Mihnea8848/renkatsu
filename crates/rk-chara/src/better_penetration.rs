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

use crate::physics::Physics;
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
    /// The shaft's colliders (names with `cm_J_dan`) and the virtual
    /// points' (`cm_J_vdan`), with their radius and height as authored,
    /// by their place in the physics' colliders (`InitializeDan`).
    pub colliders: Vec<(usize, f32, f32)>,
    pub virtual_colliders: Vec<usize>,
    /// The balls' dynamic bones (`cm_J_dan_Pivot_f_L`, `_R`), by their place
    /// in the physics' bones, when both are there (`InitializeTama`).
    pub balls: Vec<usize>,
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
            colliders: Vec::new(),
            virtual_colliders: Vec::new(),
            balls: Vec::new(),
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

/// The body colliders the balls meet on a partner, and of which only the
/// waist's on their own character (`BoneNames.BodyColliders`,
/// `MidSectionColliders`).
pub const BODY_COLLIDERS: [&str; 21] = [
    "cf_hit_thigh01_L",
    "cf_hit_thigh01_R",
    "cf_hit_thigh02_L",
    "cf_hit_thigh02_R",
    "cf_hit_waist_L",
    "cf_hit_berry",
    "cf_hit_waist01",
    "cf_hit_waist02",
    "cf_hit_spine01",
    "cf_hit_spine02",
    "cf_hit_spine03",
    "cf_hit_spine03_2",
    "cf_hit_bust00",
    "cf_hit_bust02_L",
    "cf_hit_bust02_R",
    "cf_hit_neck",
    "cf_hit_head",
    "cf_hit_arm_L",
    "cf_hit_arm_R",
    "cf_hit_shoulder_L",
    "cf_hit_shoulder_R",
];

/// A dynamic bone's name: the object its component sits on.
pub fn bone_name<'a>(physics: &Physics, scene: &'a Scene, bone: usize) -> Option<&'a str> {
    match physics.bones.get(bone)? {
        crate::physics::Bone::Dynamic(d) => Some(scene.nodes[d.def.node].name.as_str()),
        _ => None,
    }
}

impl Shaft {
    /// Finds the shaft's colliders and the balls' dynamic bones in the
    /// character's physics, and gives the balls the colliders the plugin
    /// gives them on their own character: the first two of the shaft and
    /// the waist's (`InitializeTama`, `AddTamaColliders`).
    pub fn wire(&mut self, physics: &mut Physics, scene: &Scene) {
        let name = |c: &crate::physics::Collider| scene.nodes[c.node].name.as_str();
        self.colliders = physics
            .colliders
            .iter()
            .enumerate()
            .filter(|(_, c)| name(c).contains("cm_J_dan"))
            .map(|(i, c)| (i, c.radius, c.height))
            .collect();
        self.virtual_colliders = physics
            .colliders
            .iter()
            .enumerate()
            .filter(|(_, c)| name(c).contains("cm_J_vdan"))
            .map(|(i, _)| i)
            .collect();
        let balls: Vec<Option<usize>> = ["cm_J_dan_Pivot_f_L", "cm_J_dan_Pivot_f_R"]
            .iter()
            .map(|want| (0..physics.bones.len()).find(|&b| bone_name(physics, scene, b) == Some(want)))
            .collect();
        self.balls = balls.iter().flatten().copied().collect();
        if self.balls.len() != 2 {
            self.balls.clear();
            return;
        }
        let mut own: Vec<usize> = if self.points.len() > 2 && self.colliders.len() >= 2 {
            vec![self.colliders[0].0, self.colliders[1].0]
        } else {
            self.colliders.first().map(|c| c.0).into_iter().collect()
        };
        own.extend(physics.colliders.iter().position(|c| name(c) == "cf_hit_waist02"));
        for &b in &self.balls {
            if let crate::physics::Bone::Dynamic(d) = &mut physics.bones[b] {
                d.def.colliders.extend(own.iter().copied());
            }
        }
    }

    /// Sizes the shaft's colliders by the settings, and the virtual ones as
    /// the shaft's first (`UpdateDanColliders`).
    pub fn size_colliders(&self, physics: &mut Physics, s: &Settings) {
        if self.colliders.is_empty() || self.colliders.len() >= self.points.len() {
            return;
        }
        for &(i, radius, height) in &self.colliders {
            physics.colliders[i].radius = radius * s.collider_radius_scale;
            physics.colliders[i].height = height * s.collider_length_scale;
        }
        if self.virtual_colliders.len() <= self.colliders.len() {
            for (j, &v) in self.virtual_colliders.iter().enumerate() {
                let (radius, height) = (
                    physics.colliders[self.colliders[j].0].radius,
                    physics.colliders[self.colliders[j].0].height,
                );
                physics.colliders[v].radius = radius;
                physics.colliders[v].height = height;
            }
        }
    }

    /// The colliders a partner's dynamic bones meet, placed where the
    /// shaft is (`AddDanCollidersToTargetKokan`, `...Belly`, `...Ana`):
    /// the shaft's for those named with `cf_J_Vagina` when vaginal or
    /// `cf_j_ana` when anal, the virtual points' for those with
    /// `cf_J_Belly` when vaginal with the belly bulge on. Oral gives none.
    pub fn partner_guests(
        &self,
        physics: &Physics,
        scene: &Scene,
        partner: &Physics,
        partner_scene: &Scene,
        mode: Mode,
        s: &Settings,
    ) -> crate::physics::Guests {
        let shapes = |list: &mut dyn Iterator<Item = usize>| -> Vec<crate::physics::Shape> {
            list.map(|i| physics.colliders[i].scaled_shape(scene, 1.0, 1.0))
                .collect()
        };
        let shaft = shapes(&mut self.colliders.iter().map(|c| c.0));
        let virtual_ = shapes(&mut self.virtual_colliders.iter().copied());
        let mut out = Vec::new();
        for b in 0..partner.bones.len() {
            let Some(name) = bone_name(partner, partner_scene, b) else {
                continue;
            };
            let given = match mode {
                Mode::Vaginal if name.contains("cf_J_Vagina") => &shaft,
                Mode::Vaginal if s.belly_bulge && name.contains("cf_J_Belly") => &virtual_,
                Mode::Anal if name.contains("cf_j_ana") => &shaft,
                _ => continue,
            };
            if !given.is_empty() {
                out.push((b, given.clone()));
            }
        }
        out
    }

    /// The partner's body colliders the balls meet while linked to her,
    /// placed where she is (`AddTamaColliders` with `midSectionOnly` off).
    pub fn ball_guests(&self, partner: &Physics, partner_scene: &Scene) -> crate::physics::Guests {
        let shapes: Vec<crate::physics::Shape> = partner
            .colliders
            .iter()
            .filter(|c| c.enabled && BODY_COLLIDERS.contains(&partner_scene.nodes[c.node].name.as_str()))
            .map(|c| c.scaled_shape(partner_scene, 1.0, 1.0))
            .collect();
        if shapes.is_empty() {
            return Vec::new();
        }
        self.balls.iter().map(|&b| (b, shapes.clone())).collect()
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

/// The bone of the partner's mouth the shaft pulls (`BoneNames.MouthPullBone`).
pub const MOUTH_BONE: &str = "cf_J_MouthMove";

/// The time step the pull is measured with. The plugin multiplies each
/// frame's change by Unity's frame time, so the pull grows with the frame
/// rate; Renkatsu measures it as Studio does at 60 frames a second,
/// whatever the screen's rate.
pub const PULL_STEP: f32 = 1.0 / 60.0;

/// The partner's mouth following the shaft in and out, the only bone the
/// plugin moves straight in Studio (`DanAgent.AdjustPullBones` and
/// `CollisionAgent.PullOralBone`). Studio passes `twoDans` on every update,
/// so there the mouth never returns and holds where the last stroke left
/// it. Renkatsu lets it return as the plugin does in the main game
/// (`ReturnOralBones`, "how quickly the mouth returns to its original shape
/// when there is no penetration"); a return rate of 0 holds it as Studio
/// does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouthPull {
    /// How far the mouth is pushed in (positive) or pulled out (negative).
    pub amount: f32,
    /// The shaft's root's distance to the entry at the last pulling frame.
    pub last_distance: f32,
    /// The direction of the last pull, which a return follows back
    /// (`currentOralDanDirection`).
    pub direction: Vec3,
}

impl MouthPull {
    /// A pull at rest (`CollisionAgent.Initialize`, `DanAgent.InitializeDan`).
    pub fn new(base_length: f32) -> Self {
        Self {
            amount: 0.0,
            last_distance: base_length,
            direction: unity::normalize(Vec3::Y + Vec3::NEG_Z),
        }
    }

    /// One frame of the shaft, its root at `start`, aimed at `entry`.
    /// Returns where the mouth bone goes, in the world's directions, when
    /// this frame moves it.
    pub fn step(&mut self, start: Vec3, entry: Vec3, base_length: f32, oral: bool, s: &Settings) -> Option<Vec3> {
        self.step_by(start, entry, base_length, oral, s, PULL_STEP)
    }

    /// [`Self::step`] over a frame of `dt` seconds, as the plugin measures
    /// it with Unity's frame time.
    pub fn step_by(
        &mut self,
        start: Vec3,
        entry: Vec3,
        base_length: f32,
        oral: bool,
        s: &Settings,
        dt: f32,
    ) -> Option<Vec3> {
        let direction = unity::normalize(entry - start);
        let distance = unity::distance(start, entry);
        // Past the shaft's length, the mouth is not in reach.
        if !oral || distance >= base_length {
            self.last_distance = base_length;
            return self.release(s, dt);
        }
        let pulled = self.last_distance - distance;
        self.amount += pulled * s.pull_rate * dt;
        if self.amount > s.max_push {
            self.amount = s.max_push;
        }
        if self.amount < -s.max_pull {
            self.amount = -s.max_pull;
        }
        self.last_distance = distance;
        self.direction = direction;
        Some(self.amount * direction)
    }

    /// One frame with no stroke in the mouth: the pull returns towards
    /// nothing at the return rate. Returns where the mouth bone goes when
    /// this frame moves it.
    pub fn release(&mut self, s: &Settings, dt: f32) -> Option<Vec3> {
        if self.amount.abs() < 1e-6 || s.return_rate <= 0.0 {
            return None;
        }
        let step = s.return_rate * dt;
        self.amount = if self.amount > step {
            self.amount - step
        } else if self.amount < -step {
            self.amount + step
        } else {
            0.0
        };
        Some(self.amount * self.direction)
    }
}

/// The mouth bone's local position for a pull in the world's directions:
/// the direction turned into the bone's own (`InverseTransformDirection`,
/// rotation only).
pub fn mouth_local(bone_world: &Mat4, pull: Vec3) -> Vec3 {
    let (_, rotation, _) = bone_world.to_scale_rotation_translation();
    unity::rotate(unity::inverse(rotation), pull)
}

/// Whether a character's body has the belly dynamic bones a bulge pushes.
/// None of the BP bodies installed here as of 2026-11 has them.
pub fn has_belly(physics: &Physics, scene: &Scene) -> bool {
    (0..physics.bones.len()).any(|b| bone_name(physics, scene, b).is_some_and(|n| n.contains("cf_J_Belly")))
}

/// Sizes a character's belly dynamic bones (`cf_J_Belly`) for a bulge:
/// their particles' radius times `scale`, as `UpdateBellyBones` sets it.
/// Returns whether anything changed.
pub fn bulge(physics: &mut Physics, scene: &Scene, scale: f32) -> bool {
    let bellies: Vec<usize> = (0..physics.bones.len())
        .filter(|&b| bone_name(physics, scene, b).is_some_and(|n| n.contains("cf_J_Belly")))
        .collect();
    let mut changed = false;
    for b in bellies {
        if let crate::physics::Bone::Dynamic(d) = &mut physics.bones[b] {
            changed |= d.set_radius_scale(scale);
        }
    }
    changed
}

/// Renkatsu's link for a shaft no H position aims: the place on another
/// character nearest its tip within the plugin's reach of twice its length
/// (`CheckAutoTarget`), over all three places at once. The link it has
/// holds while that place stays in reach, so a stroke does not flip it.
pub fn nearest_place<T: Copy + PartialEq>(
    tip: Vec3,
    base_length: f32,
    held: Option<(T, Mode)>,
    places: &[(T, Mode, Vec3)],
) -> Option<(T, Mode)> {
    let reach = base_length * 2.0;
    if let Some(held) = held {
        if places
            .iter()
            .any(|&(w, m, at)| (w, m) == held && tip.distance(at) < reach)
        {
            return Some(held);
        }
    }
    places
        .iter()
        .filter(|(_, _, at)| tip.distance(*at) < reach)
        .min_by(|a, b| tip.distance(a.2).total_cmp(&tip.distance(b.2)))
        .map(|&(w, m, _)| (w, m))
}

/// The finger bones whose colliders push a vagina's dynamic bones. The
/// plugin lists the right hand's (`BoneNames.FingerColliders`) for the
/// fingering positions of AI Shoujo and HoneySelect 2, which its Koikatsu
/// build never sets up; Renkatsu gives both hands to every vagina.
pub const FINGER_BONES: [&str; 12] = [
    "cf_j_index02_R",
    "cf_j_index03_R",
    "cf_j_middle02_R",
    "cf_j_middle03_R",
    "cf_j_ring02_R",
    "cf_j_ring03_R",
    "cf_j_index02_L",
    "cf_j_index03_L",
    "cf_j_middle02_L",
    "cf_j_middle03_L",
    "cf_j_ring02_L",
    "cf_j_ring03_L",
];

/// A finger collider's radius and length before the bone's scale
/// (`InitializeFingerColliders(0.0055f, 0.018f)`).
pub const FINGER_RADIUS: f32 = 0.0055;
pub const FINGER_LENGTH: f32 = 0.018;

/// The finger bones a character has.
pub fn finger_nodes(scene: &Scene, top: usize) -> Vec<usize> {
    let names = bones_by_name(scene, top);
    FINGER_BONES.iter().filter_map(|n| names.get(*n).copied()).collect()
}

/// The fingers' colliders where the character is posed: a capsule along
/// each bone's X axis, sized by its scale as `Tools.InitializeCollider`
/// is given it.
pub fn finger_shapes(scene: &Scene, fingers: &[usize]) -> Vec<crate::physics::Shape> {
    let (world, _) = scene.world();
    fingers
        .iter()
        .map(|&node| {
            let (scale, _, _) = world[node].to_scale_rotation_translation();
            let scale = scale.abs();
            crate::physics::Collider {
                node,
                radius: FINGER_RADIUS * (scale.y + scale.z) / 2.0,
                height: FINGER_LENGTH * scale.x,
                direction: 0,
                ..Default::default()
            }
            .scaled_shape(scene, 1.0, 1.0)
        })
        .collect()
}

/// A character's vagina dynamic bones (`m_kokanDynamicBones`), by their
/// place in its physics.
pub fn vagina_bones(physics: &Physics, scene: &Scene) -> Vec<usize> {
    (0..physics.bones.len())
        .filter(|&b| bone_name(physics, scene, b).is_some_and(|n| n.contains("cf_J_Vagina")))
        .collect()
}

/// The bones a vaginal stroke pulls, each with its share
/// (`BoneNames.KokanPullBones` and the first of `KokanPullWeights`, which
/// lists more shares than bones).
pub const VAGINA_PULL: [(&str, f32); 11] = [
    ("cf_J_Vagina_L.011", 0.1),
    ("cf_J_Vagina_R.011", 0.25),
    ("cf_J_Vagina_L.012", 0.25),
    ("cf_J_Vagina_R.012", 0.5),
    ("cf_J_Vagina_L.013", 0.5),
    ("cf_J_Vagina_R.013", 0.5),
    ("cf_J_Vagina_L.014", 0.5),
    ("cf_J_Vagina_L.015", 0.5),
    ("cf_J_Vagina_R.014", 0.5),
    ("cf_J_Vagina_R.015", 0.5),
    ("cf_J_Vagina_B.010", 0.5),
];

/// The bones an anal stroke pulls (`AnaPullBones`, `AnaPullWeights`).
pub const ANUS_PULL: [(&str, f32); 8] = [
    ("cf_J_Ana_s_F", 1.0),
    ("cf_J_Ana_s_B", 1.0),
    ("cf_J_Ana_s_L", 1.0),
    ("cf_J_Ana_s_R", 1.0),
    ("cf_J_Ana_s_F_L", 1.0),
    ("cf_J_Ana_s_F_R", 1.0),
    ("cf_J_Ana_s_B_L", 1.0),
    ("cf_J_Ana_s_B_R", 1.0),
];

/// The bone a stroke turns to the shaft inside her: `m_innerKokan` for
/// the vagina, `m_bpAnaTarget` for the anus.
pub fn inner_bone(mode: Mode) -> Option<&'static str> {
    match mode {
        Mode::Vaginal => Some("cf_J_Vagina_Inner"),
        Mode::Anal => Some("cf_J_Ana_Root"),
        Mode::Oral => None,
    }
}

/// The bones a stroke in `mode` pulls.
pub fn pull_bones(mode: Mode) -> &'static [(&'static str, f32)] {
    match mode {
        Mode::Vaginal => &VAGINA_PULL,
        Mode::Anal => &ANUS_PULL,
        Mode::Oral => &[],
    }
}

/// Whether a body has the bones a vaginal or anal pull moves.
pub fn has_pull_bones(scene: &Scene, top: usize) -> bool {
    let names = bones_by_name(scene, top);
    VAGINA_PULL
        .iter()
        .chain(&ANUS_PULL)
        .any(|(n, _)| names.contains_key(*n))
}

/// A vaginal or anal pull on her for one frame (`PullKokanBones`,
/// `PullAnaBone` and their returns), in the world's directions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HolePull {
    pub mode: Mode,
    pub amount: f32,
    /// The bones' direction: the average of the shaft's way in and its way
    /// inside her (`TargetType.Average`, the default for `outer`).
    pub direction: Vec3,
    /// The inner bone's up: the way inside her (`TargetType.Inside`).
    pub inside: Vec3,
    /// No stroke is in her: the bones return along their own up, and the
    /// inner bone turns back (`ReturnKokanBones`).
    pub returning: bool,
}

/// The directions of a stroke: from the shaft's root to the entry and from
/// the entry to the end (`AdjustPullBones`' `danDirection` and
/// `insideDirection`), as the pull bones and the inner bone take them.
pub fn hole_directions(start: Vec3, entry: Vec3, end: Vec3) -> (Vec3, Vec3) {
    let outer = unity::normalize(entry - start);
    let inside = unity::normalize(end - entry);
    (unity::normalize(outer + inside), inside)
}

/// Places her pull bones and turns her inner bone for a pull. Bones she
/// lacks are passed over.
pub fn apply_hole_pull(scene: &mut Scene, top: usize, pull: &HolePull) {
    let names = bones_by_name(scene, top);
    let (world, _) = scene.world();
    // The return writes only while there is a pull left, as the plugin does.
    if !(pull.returning && pull.amount.abs() < 1e-4) {
        for &(name, share) in pull_bones(pull.mode) {
            let Some(&node) = names.get(name) else { continue };
            let local = if pull.returning {
                share * pull.amount * Vec3::Y
            } else {
                share * pull.amount * mouth_local(&world[node], pull.direction)
            };
            scene.nodes[node].local.pos = local;
        }
    }
    let Some(&inner) = inner_bone(pull.mode).and_then(|n| names.get(n)) else {
        return;
    };
    if pull.returning {
        scene.nodes[inner].local.rot = Quat::IDENTITY;
        return;
    }
    // `transform.rotation = LookRotation(transform.forward, up)`.
    let (_, rotation, _) = world[inner].to_scale_rotation_translation();
    let forward = unity::rotate(rotation, Vec3::Z);
    let turned = unity::look_rotation(forward, pull.inside);
    let parent = scene.nodes[inner]
        .parent
        .map(|p| world[p].to_scale_rotation_translation().1)
        .unwrap_or(Quat::IDENTITY);
    scene.nodes[inner].local.rot = unity::mul(unity::inverse(parent), turned);
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
    /// Renkatsu's own, not the plugin's: with no partner chosen and no
    /// auto-target, the shaft links where the game's H position aims it
    /// (`crate::dan_look`), or else to the nearest place in reach.
    #[serde(default = "yes")]
    pub auto_link: bool,
    /// Renkatsu's own: for her vagina or anus, the shaft goes straight in
    /// and bends only where it would leave her body, as the plugin does in
    /// the main game (`crate::body_collision`), rather than ending at her
    /// waist as in Studio.
    #[serde(default = "yes")]
    pub body_collision: bool,
    /// The vaginal and anal pulls (`enableKokanPush`, `enableAnaPush`),
    /// which both builds of the plugin leave off; for bodies with the pull
    /// bones. They take the same push, pull and rates as the mouth.
    #[serde(default)]
    pub pull_holes: bool,
}

fn yes() -> bool {
    true
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
            auto_link: true,
            body_collision: true,
            pull_holes: false,
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
    fn an_aimed_shaft_pushes_a_partners_vagina_bones() {
        let Ok(game) = rk_vfs::Game::open_user(&[]) else {
            eprintln!("skipped: no game");
            return;
        };
        let Some(dir) = rk_core::Config::load().ok().map(|c| c.game_dir) else {
            return;
        };
        let load = |path: &str, body: Option<&str>| {
            let card = rk_card::Card::load(dir.join(path)).ok()?;
            let opts = crate::BuildOptions {
                clothes: false,
                accessories: false,
                uncensor_body: body.map(str::to_owned),
                ..Default::default()
            };
            crate::build(&game, &card, &opts).ok()
        };
        let (Some(mut him), Some(her)) = (
            load("UserData/chara/male/Gallagher.png", None),
            load("UserData/chara/female/Simple.png", Some("bp.profundis_nt")),
        ) else {
            eprintln!("skipped: no Gallagher.png, Simple.png or BP Profundis");
            return;
        };
        let shaft = him.shaft.clone().expect("a BP shaft");
        assert_eq!(shaft.colliders.len(), 4, "dan102, 104, 106, 108");
        assert_eq!(shaft.balls.len(), 2);
        let s = Settings::default();
        shaft.size_colliders(&mut him.physics, &s);
        // Her in front of him, her vagina a little ahead of his shaft's root.
        let (world, _) = her.scene.world();
        let names = bones_by_name(&her.scene, her.top);
        let (entry, end) = targets(Mode::Vaginal, &world, |n| names.get(n).copied()).expect("her targets");
        shaft.aim(&mut him.scene, entry, end, &s, true);
        let guests = shaft.partner_guests(&him.physics, &him.scene, &her.physics, &her.scene, Mode::Vaginal, &s);
        assert!(!guests.is_empty(), "her vagina dynamic bones meet the shaft");
        assert!(guests.iter().all(|(_, g)| g.len() == 4));
        assert!(
            shaft
                .ball_guests(&her.physics, &her.scene)
                .iter()
                .all(|(_, g)| g.len() > 5),
            "the balls meet her body"
        );
        assert!(shaft
            .partner_guests(&him.physics, &him.scene, &her.physics, &her.scene, Mode::Oral, &s)
            .is_empty());
        // Her vagina bones after a few frames with and without the shaft.
        let settle = |mut chara: crate::build::Character, guests: crate::physics::Guests| {
            chara.physics.set_guests(guests);
            for _ in 0..30 {
                chara.physics.begin_frame(&mut chara.scene);
                chara.physics.end_frame(&mut chara.scene, crate::physics::FRAME_TIME);
            }
            let (world, _) = chara.scene.world();
            let named = bones_by_name(&chara.scene, chara.top);
            // The pivots are the chains' roots and stay; the bones below them move.
            ["cf_J_Vagina_L.003", "cf_J_Vagina_R.003", "cf_J_Vagina_B"]
                .iter()
                .filter_map(|n| named.get(*n).map(|&i| world[i].w_axis.truncate()))
                .collect::<Vec<Vec3>>()
        };
        let again = load("UserData/chara/female/Simple.png", Some("bp.profundis_nt")).expect("built before");
        let pushed = settle(her, guests);
        let alone = settle(again, Vec::new());
        assert_eq!(pushed.len(), alone.len());
        let moved = pushed
            .iter()
            .zip(&alone)
            .map(|(a, b)| a.distance(*b))
            .fold(0.0, f32::max);
        eprintln!("the shaft moves her vagina bones by up to {moved} m");
        assert!(moved > 1e-4, "the shaft pushes her vagina bones");
    }

    #[test]
    fn strokes_pull_the_mouth_as_studio_does() {
        // Studio's hold: no return.
        let s = Settings {
            return_rate: 0.0,
            ..Settings::default()
        };
        let base = 0.19;
        let mut pull = MouthPull::new(base);
        let start = Vec3::ZERO;
        let at = |d: f32| Vec3::new(0.0, 0.0, d);
        // In: from the base length to 0.15 pushes (0.19 - 0.15) * 1.8 / 60.
        let moved = pull.step(start, at(0.15), base, true, &s).unwrap();
        assert!((pull.amount - 0.0012).abs() < 1e-6, "{}", pull.amount);
        assert!((moved - Vec3::new(0.0, 0.0, 0.0012)).length() < 1e-6);
        // Deeper, the push stops at its most.
        pull.step(start, at(0.05), base, true, &s);
        assert_eq!(pull.amount, s.max_push);
        // Out: the mouth follows the shaft, at most its pull.
        pull.step(start, at(0.15), base, true, &s);
        assert!(
            (pull.amount - (0.002 - 0.1 * 1.8 / 60.0)).abs() < 1e-6,
            "{}",
            pull.amount
        );
        for _ in 0..40 {
            pull.step(start, at(0.05), base, true, &s);
            pull.step(start, at(0.18), base, true, &s);
        }
        assert!(pull.amount >= -s.max_pull - 1e-7 && pull.amount <= s.max_push + 1e-7);
        // Out of reach, or not in the mouth, nothing moves and the pull holds.
        let held = pull.amount;
        assert_eq!(pull.step(start, at(0.2), base, true, &s), None);
        assert_eq!(pull.step(start, at(0.1), base, false, &s), None);
        assert_eq!(pull.amount, held);
        // Coming back in starts from the base length again.
        pull.step(start, at(0.18), base, true, &s);
        assert!((pull.amount - (held + 0.01 * 1.8 / 60.0)).abs() < 1e-6);
    }

    #[test]
    fn without_a_stroke_the_mouth_returns_at_its_rate() {
        let s = Settings::default();
        let base = 0.19;
        let mut pull = MouthPull::new(base);
        let at = |d: f32| Vec3::new(0.0, 0.0, d);
        pull.step(Vec3::ZERO, at(0.05), base, true, &s);
        assert_eq!(pull.amount, s.max_push);
        // Out of reach: 0.03 m/s, so 0.0005 m a frame at 60 frames a second.
        let moved = pull.step(Vec3::ZERO, at(0.25), base, true, &s).unwrap();
        assert!((pull.amount - (0.002 - 0.03 / 60.0)).abs() < 1e-7, "{}", pull.amount);
        assert!(
            (moved - Vec3::new(0.0, 0.0, pull.amount)).length() < 1e-7,
            "along the last pull"
        );
        for _ in 0..10 {
            pull.release(&s, PULL_STEP);
        }
        assert_eq!(pull.amount, 0.0);
        assert_eq!(pull.release(&s, PULL_STEP), None, "at rest nothing moves");
    }

    #[test]
    fn the_mouth_takes_the_pull_in_its_own_directions() {
        let bone = Mat4::from_rotation_translation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), Vec3::ONE);
        let local = mouth_local(&bone, Vec3::new(0.0, 0.0, 0.002));
        assert!((local - Vec3::new(-0.002, 0.0, 0.0)).length() < 1e-6, "{local:?}");
    }

    #[test]
    fn a_bigger_bulge_pushes_the_belly_further() {
        let Ok(game) = rk_vfs::Game::open_user(&[]) else {
            return;
        };
        let Some(dir) = rk_core::Config::load().ok().map(|c| c.game_dir) else {
            return;
        };
        let load = |path: &str, body: Option<&str>| {
            let card = rk_card::Card::load(dir.join(path)).ok()?;
            let opts = crate::BuildOptions {
                clothes: false,
                accessories: false,
                uncensor_body: body.map(str::to_owned),
                ..Default::default()
            };
            crate::build(&game, &card, &opts).ok()
        };
        let Some(mut him) = load("UserData/chara/male/Gallagher.png", None) else {
            return;
        };
        let s = Settings {
            belly_bulge: true,
            ..Settings::default()
        };
        let shaft = him.shaft.clone().expect("a BP shaft");
        shaft.size_colliders(&mut him.physics, &s);
        // The belly as it settles with the shaft deep in her, at a size.
        let mut belly = |scale: f32| -> Option<Vec<Vec3>> {
            let mut her = load("UserData/chara/female/Simple.png", Some("bp.profundis_nt"))?;
            let (world, _) = her.scene.world();
            let names = bones_by_name(&her.scene, her.top);
            let (entry, end) = targets(Mode::Vaginal, &world, |n| names.get(n).copied())?;
            shaft.aim(&mut him.scene, entry, end, &s, true);
            let guests = shaft.partner_guests(&him.physics, &him.scene, &her.physics, &her.scene, Mode::Vaginal, &s);
            bulge(&mut her.physics, &her.scene, scale);
            her.physics.settle(&mut her.scene, 60, crate::physics::FRAME_TIME);
            her.physics.set_guests(guests);
            her.physics.settle(&mut her.scene, 120, crate::physics::FRAME_TIME);
            let (world, _) = her.scene.world();
            Some(
                her.scene
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| n.name.starts_with("cf_J_Belly"))
                    .map(|(i, _)| world[i].w_axis.truncate())
                    .collect(),
            )
        };
        let (Some(rest), Some(one), Some(two)) = (belly(0.01), belly(1.0), belly(2.0)) else {
            eprintln!("skipped: no Simple.png or BP Profundis");
            return;
        };
        if rest.is_empty() {
            eprintln!("skipped: BP Profundis here has no belly bones");
            return;
        }
        let moved = |a: &[Vec3]| a.iter().zip(&rest).map(|(a, b)| a.distance(*b)).fold(0.0, f32::max);
        eprintln!(
            "belly moved {:.2} mm at size 1, {:.2} mm at size 2",
            moved(&one) * 1000.0,
            moved(&two) * 1000.0
        );
        assert!(moved(&two) > moved(&one) + 1e-4, "a bigger bulge pushes further");
    }

    #[test]
    fn the_nearest_place_in_reach_links_and_holds() {
        let at = |z: f32| Vec3::new(0.0, 0.0, z);
        let places = [
            (1, Mode::Vaginal, at(0.10)),
            (1, Mode::Anal, at(0.12)),
            (2, Mode::Oral, at(0.5)),
        ];
        assert_eq!(nearest_place(Vec3::ZERO, 0.19, None, &places), Some((1, Mode::Vaginal)));
        // Held while in reach, though another is nearer now.
        assert_eq!(
            nearest_place(Vec3::ZERO, 0.19, Some((1, Mode::Anal)), &places),
            Some((1, Mode::Anal))
        );
        // Out of reach (0.38), a held link lets go.
        assert_eq!(
            nearest_place(Vec3::ZERO, 0.19, Some((2, Mode::Oral)), &places),
            Some((1, Mode::Vaginal))
        );
        assert_eq!(nearest_place(at(2.0), 0.19, None, &places), None);
    }

    #[test]
    fn fingers_find_both_hands_and_a_bp_vagina_its_bones() {
        let Ok(game) = rk_vfs::Game::open_user(&[]) else {
            return;
        };
        let Some(dir) = rk_core::Config::load().ok().map(|c| c.game_dir) else {
            return;
        };
        let Ok(card) = rk_card::Card::load(dir.join("UserData/chara/female/Simple.png")) else {
            return;
        };
        let opts = crate::BuildOptions {
            clothes: false,
            accessories: false,
            uncensor_body: Some("bp.profundis_nt".into()),
            ..Default::default()
        };
        let Ok(her) = crate::build(&game, &card, &opts) else {
            return;
        };
        let fingers = finger_nodes(&her.scene, her.top);
        assert_eq!(fingers.len(), 12);
        let shapes = finger_shapes(&her.scene, &fingers);
        assert_eq!(shapes.len(), 12);
        assert!(
            !vagina_bones(&her.physics, &her.scene).is_empty(),
            "BP Profundis has vagina bones"
        );
    }

    #[test]
    fn a_stroke_pulls_the_bones_by_their_shares_and_returns_along_up() {
        let mut scene = Scene::default();
        let top = scene.add_node("top", None, Trs::default());
        let a = scene.add_node("cf_J_Vagina_L.011", Some(top), Trs::default());
        let b = scene.add_node("cf_J_Vagina_R.012", Some(top), Trs::default());
        let (direction, inside) = hole_directions(Vec3::ZERO, Vec3::Z * 0.1, Vec3::new(0.0, 0.1, 0.1));
        assert!((direction - Vec3::new(0.0, 1.0, 1.0).normalize()).length() < 1e-6);
        assert_eq!(inside, Vec3::Y);
        let mut pull = HolePull {
            mode: Mode::Vaginal,
            amount: 0.002,
            direction,
            inside,
            returning: false,
        };
        apply_hole_pull(&mut scene, top, &pull);
        assert!((scene.nodes[a].local.pos - 0.1 * 0.002 * direction).length() < 1e-9);
        assert!((scene.nodes[b].local.pos - 0.5 * 0.002 * direction).length() < 1e-9);
        pull.returning = true;
        pull.amount = 0.001;
        apply_hole_pull(&mut scene, top, &pull);
        assert!(
            (scene.nodes[b].local.pos - Vec3::new(0.0, 0.0005, 0.0)).length() < 1e-9,
            "the return goes along up"
        );
        assert_eq!(pull_bones(Mode::Oral).len(), 0);
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
