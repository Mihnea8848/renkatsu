// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Accessory Clothes plugin (`KK_AccessoryClothes`): accessories that
//! are skinned to the body like clothes.
//!
//! Such a prefab carries a `ChaAccessoryClothes` script beside its
//! `ChaAccessoryComponent`, with one field, `ArmatureRoot`: the prefab's own
//! copy of the body skeleton. `ChaAccessoryClothes.Start` runs
//! `aaWeightsBody.AssignedWeightsAndSetBounds(rend, "cf_j_root", bounds,
//! A_ROOTBONE)` on every renderer in `rendNormal` and then destroys the
//! armature. The renderers in `rendAlpha` and `rendHair` are left alone.

use crate::scene::{Bounds, Scene};
use std::collections::HashMap;

/// Class name of the plugin's script on the prefab.
pub const SCRIPT: &str = "ChaAccessoryClothes";
/// The game's script on the same object, whose `rendNormal` gets bound.
pub const COMPONENT: &str = "ChaAccessoryComponent";
/// The script's only field: the transform of the prefab's own skeleton.
pub const ARMATURE_ROOT: &str = "ArmatureRoot";
/// The renderer list of `ChaAccessoryComponent` the plugin walks.
pub const RENDERERS: &str = "rendNormal";
/// The object `AssignedWeightsAndSetBounds` deletes below each renderer.
const DEL_TOP: &str = "cf_j_root";

/// Binds the renderers to the body skeleton and destroys the armature.
///
/// `dict` is `aaWeightsBody.dictBone` (body bones by name), `root_bone` the
/// `A_ROOTBONE` reference object. As `AssignedWeightsAndSetBoundsLoop` does
/// for a renderer and its children: a bone the body lacks becomes null, the
/// culling box becomes the character's, and the root bone becomes
/// `root_bone`, or without one the body bone named like the old root bone.
pub fn bind(
    scene: &mut Scene,
    renderers: &[usize],
    armature_root: Option<usize>,
    dict: &HashMap<String, usize>,
    root_bone: Option<usize>,
) {
    for &listed in renderers {
        let top = scene.renderers[listed].node;
        let below = scene.subtree(top);
        for ri in 0..scene.renderers.len() {
            let node = scene.renderers[ri].node;
            if !scene.renderers[ri].skinned || !below.contains(&node) {
                continue;
            }
            let bones = scene.renderers[ri]
                .bones
                .iter()
                .map(|b| b.and_then(|b| dict.get(&scene.nodes[b].name).copied()))
                .collect();
            let old_root = scene.renderers[ri]
                .root_bone
                .and_then(|n| dict.get(&scene.nodes[n].name).copied());
            let r = &mut scene.renderers[ri];
            r.bones = bones;
            r.bounds = Some(Bounds::CHARA);
            if root_bone.is_some() {
                r.root_bone = root_bone;
            } else if old_root.is_some() {
                r.root_bone = old_root;
            }
        }
        if let Some(del) = scene.find(top, DEL_TOP) {
            scene.remove(del);
        }
    }
    if let Some(armature) = armature_root {
        scene.remove(armature);
    }
}
