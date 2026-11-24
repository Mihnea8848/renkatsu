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

use glam::{Mat4, Vec3};
use rk_card::{ExtData, Value};
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
    pub fn find(world: &[Mat4], find: impl Fn(&str) -> Option<usize>) -> Option<Shaft> {
        let points: Vec<usize> = SHAFT_BONES.iter().filter_map(|n| find(n)).collect();
        let virtual_points = VIRTUAL_BONES.iter().filter_map(|n| find(n)).collect();
        let balls_top = find("cm_J_dan_f_top")?;
        if points.len() < 2 {
            return None;
        }
        let at = |i: usize| world[points[i]].w_axis.truncate();
        let base_length = at(0).distance(at(1)) * (points.len() - 1) as f32;
        Some(Shaft {
            points,
            virtual_points,
            balls_top,
            end: find("cm_J_dan119_00"),
            base_length,
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
