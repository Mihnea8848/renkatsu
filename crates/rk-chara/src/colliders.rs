// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Colliders plugin (`KK_Colliders`): a floor, arm and leg colliders
//! for skirts, hair and the bust.
//!
//! `ColliderController.Start` puts a floor collider on `cf_j_root`, six
//! capsules on the arms and six on the thighs. One frame after a load
//! (`LateUpdate` with every `apply` flag set) it goes over the DynamicBone
//! components of the character:
//! - every `DynamicBone` gets the floor collider when the floor is on;
//! - the ones of a bottom (`ct_clothesBot`) get the leg colliders when the
//!   skirt colliders are on, and the chain whose root is `cf_j_sk_00_*` has
//!   its frozen axis set to X when they are on and to none when off;
//! - the two bust components (`DynamicBone_Ver02` with the comments
//!   `右胸` and `左胸`) get the floor collider when floor and breast are
//!   both on, the arm colliders when breast is on, and a collision radius
//!   on their first two bones of 0.1 and 0.08 times the card's bust size,
//!   or 0 when breast is off.
//!
//! The three switches: in Studio they start from the plugin setting
//! "Default Studio Settings", which is Off as installed, and a card saved
//! from Studio carries its own values.

use crate::physics::{Collider, DynamicBoneDef, Ver02Def};
use crate::scene::Scene;
use glam::Vec3;
use rk_card::{ExtData, Value};
use std::collections::HashMap;

pub const KEY: &str = "com.deathweasel.bepinex.colliders";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Settings {
    pub breast: bool,
    pub floor: bool,
    pub skirt: bool,
}

/// The switches of a character added in Studio (`OnReload`), with the
/// plugin setting at Off.
pub fn read(ext: &ExtData) -> Settings {
    let mut s = Settings::default();
    let Some(p) = ext.get(KEY) else { return s };
    let flag = |key: &str, default: bool| p.data.get(key).and_then(Value::as_bool).unwrap_or(default);
    s.breast = flag("BreastCollidersEnabled", s.breast);
    s.floor = flag("FloorColliderEnabled", s.floor);
    s.skirt = flag("SkirtCollidersEnabled", s.skirt);
    s
}

/// `ColliderData`: bone, radius, height, centre, capsule axis.
type Data = (&'static str, f32, f32, [f32; 3], u8);

/// `ColliderConstants.FloorColliderData`.
const FLOOR: Data = ("cf_j_root", 100.0, 0.0, [0.0, -100.01, 0.0], 0);
/// `ColliderConstants.ArmColliderDataList`.
const ARMS: [Data; 6] = [
    ("cf_s_hand_L", 0.02, 0.075, [-0.03, -0.005, 0.0], 0),
    ("cf_s_hand_R", 0.02, 0.075, [0.03, -0.005, 0.0], 0),
    ("cf_s_forearm02_L", 0.025, 0.25, [-0.03, 0.0, 0.0], 0),
    ("cf_s_forearm02_R", 0.025, 0.25, [0.03, 0.0, 0.0], 0),
    ("cf_s_arm02_L", 0.025, 0.25, [0.0, 0.0, 0.0], 0),
    ("cf_s_arm02_R", 0.025, 0.25, [0.0, 0.0, 0.0], 0),
];
/// `ColliderConstants.LegColliderDataList`.
const LEGS: [Data; 6] = [
    ("cf_s_thigh01_L", 0.095, 0.32, [0.05, -0.1, -0.015], 1),
    ("cf_s_thigh01_R", 0.095, 0.32, [-0.05, -0.1, -0.015], 1),
    ("cf_s_thigh01_L", 0.095, 0.32, [0.01, -0.125, -0.015], 1),
    ("cf_s_thigh01_R", 0.095, 0.32, [-0.01, -0.125, -0.015], 1),
    ("cf_s_thigh02_L", 0.083, 0.35, [-0.0065, 0.0, -0.012], 1),
    ("cf_s_thigh02_R", 0.083, 0.35, [0.0065, 0.0, -0.012], 1),
];
/// `ColliderConstants.BreastDBComments`.
const BREAST_COMMENTS: [&str; 2] = ["右胸", "左胸"];
/// Index of the bust size in `shapeValueBody`.
pub const BREAST_SIZE: usize = 4;

/// The colliders the plugin created, as indices into the collider list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Added {
    pub floor: Option<usize>,
    pub arms: Vec<usize>,
    pub legs: Vec<usize>,
}

/// `AddCollider` for every entry: a collider at the bone, which the game
/// gets by hanging a new object under it. A missing bone gives none.
pub fn add(colliders: &mut Vec<Collider>, bones: &HashMap<String, usize>) -> Added {
    let mut one = |(bone, radius, height, center, direction): Data| {
        let node = *bones.get(bone)?;
        colliders.push(Collider {
            node,
            center: Vec3::from(center),
            radius,
            height,
            direction,
            inside: false,
            enabled: true,
        });
        Some(colliders.len() - 1)
    };
    Added {
        floor: one(FLOOR),
        arms: ARMS.into_iter().filter_map(&mut one).collect(),
        legs: LEGS.into_iter().filter_map(&mut one).collect(),
    }
}

fn include(list: &mut Vec<usize>, collider: usize) {
    if !list.contains(&collider) {
        list.push(collider);
    }
}

/// What the plugin does to one `DynamicBone`.
pub fn dynamic_bone(def: &mut DynamicBoneDef, scene: &Scene, settings: Settings, added: &Added) {
    if let (true, Some(floor)) = (settings.floor, added.floor) {
        include(&mut def.colliders, floor);
    }
    if scene.nodes[def.node].name != "ct_clothesBot" {
        return;
    }
    if settings.skirt {
        for &leg in &added.legs {
            include(&mut def.colliders, leg);
        }
    }
    // `UpdateSkirtDB`: the chain numbered 0 in `cf_j_sk_00_00`.
    let Some(root) = def.root else { return };
    let chain = scene.nodes[root]
        .name
        .split('_')
        .nth(3)
        .and_then(|p| p.parse::<i32>().ok());
    if chain == Some(0) {
        def.freeze_axis = u8::from(settings.skirt);
    }
}

/// What the plugin does to one `DynamicBone_Ver02`. `breast_size` is the
/// card's `shapeValueBody[4]`.
pub fn ver02(def: &mut Ver02Def, settings: Settings, added: &Added, breast_size: f32) {
    if !BREAST_COMMENTS.contains(&def.comment.as_str()) {
        return;
    }
    if let (true, true, Some(floor)) = (settings.floor, settings.breast, added.floor) {
        include(&mut def.colliders, floor);
    }
    if settings.breast {
        for &arm in &added.arms {
            include(&mut def.colliders, arm);
        }
    }
    let radius = |factor: f32| if settings.breast { factor * breast_size } else { 0.0 };
    for pattern in &mut def.patterns {
        if let Some(p) = pattern.params.get_mut(0) {
            p.collision_radius = radius(0.1);
        }
        if let Some(p) = pattern.params.get_mut(1) {
            p.collision_radius = radius(0.08);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::{BoneParam, Pattern};
    use rk_card::PluginData;

    fn on() -> Settings {
        Settings {
            breast: true,
            floor: true,
            skirt: true,
        }
    }

    #[test]
    fn studio_starts_off_and_the_card_decides() {
        assert_eq!(read(&ExtData::new()), Settings::default());
        let mut p = PluginData::default();
        let mut ext = ExtData::new();
        ext.insert(KEY.into(), p.clone());
        assert_eq!(read(&ext), Settings::default());
        p.data.insert("BreastCollidersEnabled".into(), Value::Boolean(true));
        p.data.insert("SkirtCollidersEnabled".into(), Value::Boolean(true));
        ext.insert(KEY.into(), p);
        assert_eq!(
            read(&ext),
            Settings {
                breast: true,
                floor: false,
                skirt: true
            }
        );
    }

    fn scene() -> (Scene, HashMap<String, usize>) {
        let mut s = Scene::default();
        let root = s.add_node("cf_j_root", None, Default::default());
        for name in ["cf_s_hand_L", "cf_s_thigh01_L", "cf_s_thigh02_R"] {
            s.add_node(name, Some(root), Default::default());
        }
        let bones = s.name_map(root);
        (s, bones)
    }

    #[test]
    fn colliders_sit_on_the_bones_that_exist() {
        let (_, bones) = scene();
        let mut list = vec![Collider::default()];
        let added = add(&mut list, &bones);
        assert_eq!(added.floor, Some(1));
        assert_eq!(added.arms, vec![2]);
        // Both colliders of the left thigh, one of the right.
        assert_eq!(added.legs, vec![3, 4, 5]);
        assert_eq!(list[1].radius, 100.0);
        assert_eq!(list[1].center, Vec3::new(0.0, -100.01, 0.0));
        assert_eq!(list[3].direction, 1);
        assert_eq!(list[3].node, bones["cf_s_thigh01_L"]);
    }

    #[test]
    fn skirts_get_legs_and_floor() {
        let (mut s, bones) = scene();
        let mut list = Vec::new();
        let added = add(&mut list, &bones);
        let bot = s.add_node("ct_clothesBot", None, Default::default());
        let front = s.add_node("cf_j_sk_00_00", Some(bot), Default::default());
        let side = s.add_node("cf_j_sk_02_00", Some(bot), Default::default());
        let hair = s.add_node("ct_hairB", None, Default::default());
        let def = |node, root, freeze_axis| DynamicBoneDef {
            node,
            root: Some(root),
            freeze_axis,
            colliders: vec![7],
            ..Default::default()
        };

        let mut d = def(bot, front, 0);
        dynamic_bone(&mut d, &s, on(), &added);
        assert_eq!(d.colliders, vec![7, 0, 2, 3, 4]);
        assert_eq!(d.freeze_axis, 1);
        // Applying twice adds nothing.
        dynamic_bone(&mut d, &s, on(), &added);
        assert_eq!(d.colliders.len(), 5);

        let mut d = def(bot, side, 2);
        dynamic_bone(&mut d, &s, on(), &added);
        assert_eq!(d.freeze_axis, 2);

        let mut d = def(hair, front, 1);
        dynamic_bone(&mut d, &s, on(), &added);
        assert_eq!(d.colliders, vec![7, 0]);
        assert_eq!(d.freeze_axis, 1);

        // Off: only the front chain's axis is touched.
        let mut d = def(bot, front, 1);
        dynamic_bone(&mut d, &s, Settings::default(), &added);
        assert_eq!(d.colliders, vec![7]);
        assert_eq!(d.freeze_axis, 0);
    }

    #[test]
    fn bust_gets_arms_and_a_radius() {
        let (_, bones) = scene();
        let mut list = Vec::new();
        let added = add(&mut list, &bones);
        let def = |comment: &str| Ver02Def {
            comment: comment.into(),
            patterns: vec![Pattern {
                params: vec![
                    BoneParam {
                        collision_radius: 0.5,
                        ..Default::default()
                    };
                    3
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut d = def("右胸");
        ver02(&mut d, on(), &added, 0.5);
        assert_eq!(d.colliders, vec![0, 1]);
        let radii: Vec<f32> = d.patterns[0].params.iter().map(|p| p.collision_radius).collect();
        assert_eq!(radii, vec![0.05, 0.04, 0.5]);

        // Breast off: no colliders, even the floor, and no radius.
        let mut d = def("左胸");
        ver02(
            &mut d,
            Settings {
                floor: true,
                ..Default::default()
            },
            &added,
            0.5,
        );
        assert!(d.colliders.is_empty());
        assert_eq!(d.patterns[0].params[0].collision_radius, 0.0);

        // Other components (the hips) are left alone.
        let mut d = def("尻");
        ver02(&mut d, on(), &added, 0.5);
        assert!(d.colliders.is_empty());
        assert_eq!(d.patterns[0].params[0].collision_radius, 0.5);
    }
}
