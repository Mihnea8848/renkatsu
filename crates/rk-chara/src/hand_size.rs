// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Hand size correction. `ChaControl.LateUpdateForce` sets the local scale of
//! `cf_j_hand_L/R` to (1, 0.9, 0.9) on every high-poly female while
//! `Character.enableCorrectHandSize` is on, which `Character.Start` makes the
//! default. ABMX collects its baseline after that call, so its hand
//! modifiers multiply the corrected scale.
//!
//! Studio adds `PVCopy`, which copies the local scale of `cf_pv_hand_L/R`
//! onto the hand bones later in the same frame and so undoes the correction.
//! ABMX applies after it, so a hand with an ABMX scale modifier keeps
//! corrected scale times modifier.

use crate::abmx::BoneModifier;
use crate::scene::Scene;
use glam::Vec3;
use std::collections::HashMap;

/// Hand bone and the Studio pose target copied onto it.
pub const HANDS: [(&str, &str); 2] = [("cf_j_hand_L", "cf_pv_hand_L"), ("cf_j_hand_R", "cf_pv_hand_R")];

/// Scale the game gives the hand bones.
pub const SCALE: Vec3 = Vec3::new(1.0, 0.9, 0.9);

/// Sets the corrected scale. Run before ABMX.
pub fn correct(scene: &mut Scene, bones: &HashMap<String, usize>) {
    for (hand, _) in HANDS {
        if let Some(&node) = bones.get(hand) {
            scene.nodes[node].local.scale = SCALE;
        }
    }
}

/// Studio's `PVCopy` copies each hand's pose target onto it every frame.
/// The targets keep the asset's scale of 1, which undoes the correction,
/// unless ABMX has modifiers: then its baseline step
/// (`BoneController.CollectBaselineCo`) turns the copy off, records the
/// corrected hands and writes them into the targets, so the copy keeps the
/// correction and the modifiers multiply it. With no modifiers at all ABMX
/// returns before that step. Run after ABMX.
pub fn studio_copy(scene: &mut Scene, bones: &HashMap<String, usize>, mods: &[BoneModifier]) {
    if !mods.is_empty() {
        return;
    }
    for (hand, target) in HANDS {
        if let (Some(&node), Some(&from)) = (bones.get(hand), bones.get(target)) {
            scene.nodes[node].local.scale = scene.nodes[from].local.scale;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abmx::{self, Modifier};
    use crate::scene::Trs;

    fn skeleton() -> (Scene, HashMap<String, usize>) {
        let mut scene = Scene::default();
        let root = scene.add_node("root", None, Trs::default());
        for (hand, target) in HANDS {
            scene.add_node(hand, Some(root), Trs::default());
            scene.add_node(target, Some(root), Trs::default());
        }
        let bones = scene.name_map(root);
        (scene, bones)
    }

    fn hand_modifier(bone: &str, scale: Vec3) -> BoneModifier {
        BoneModifier {
            bone: bone.to_string(),
            per_outfit: vec![Modifier {
                scale,
                length: 1.0,
                position: Vec3::ZERO,
                rotation: Vec3::ZERO,
            }],
            location: 1,
        }
    }

    fn scale(scene: &Scene, bones: &HashMap<String, usize>, name: &str) -> Vec3 {
        scene.nodes[bones[name]].local.scale
    }

    #[test]
    fn game_scales_both_hands_and_abmx_multiplies() {
        let (mut scene, bones) = skeleton();
        let mods = [hand_modifier("cf_j_hand_L", Vec3::splat(1.02))];
        correct(&mut scene, &bones);
        abmx::apply(&mut scene, &mods, 0, &bones, &HashMap::new());
        assert!(
            (scale(&scene, &bones, "cf_j_hand_L") - Vec3::new(1.02, 0.918, 0.918))
                .abs()
                .max_element()
                < 1e-6
        );
        assert_eq!(scale(&scene, &bones, "cf_j_hand_R"), SCALE);
        assert_eq!(scale(&scene, &bones, "cf_pv_hand_L"), Vec3::ONE);
    }

    #[test]
    fn any_abmx_modifier_keeps_the_correction_in_studio() {
        let (mut scene, bones) = skeleton();
        // A modifier on another bone: both hands stay corrected.
        let mods = [hand_modifier("cf_j_head", Vec3::splat(1.1))];
        correct(&mut scene, &bones);
        abmx::apply(&mut scene, &mods, 0, &bones, &HashMap::new());
        studio_copy(&mut scene, &bones, &mods);
        assert_eq!(scale(&scene, &bones, "cf_j_hand_L"), SCALE);
        assert_eq!(scale(&scene, &bones, "cf_j_hand_R"), SCALE);
    }

    #[test]
    fn studio_copy_takes_the_target_scale() {
        let (mut scene, bones) = skeleton();
        scene.nodes[bones["cf_pv_hand_L"]].local.scale = Vec3::new(1.0, 0.5, 0.5);
        correct(&mut scene, &bones);
        studio_copy(&mut scene, &bones, &[]);
        assert_eq!(scale(&scene, &bones, "cf_j_hand_L"), Vec3::new(1.0, 0.5, 0.5));
        assert_eq!(scale(&scene, &bones, "cf_j_hand_R"), Vec3::ONE);
    }
}
