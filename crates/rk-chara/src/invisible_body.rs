// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Invisible Body plugin (`KK_InvisibleBody`): a card can ask for the
//! body to be hidden, leaving clothes and accessories.
//!
//! `InvisibleBodyCharaController.SetVisibleState` walks the objects below
//! `BodyTop/p_cf_body_bone/cf_j_root` and `BodyTop/p_cf_body_00/cf_o_root`
//! (and the male and low-poly twins) and turns every renderer off. The head
//! and the hair hang from the skeleton, so they go too. The walk does not
//! descend below an accessory attachment point, unless that point sits
//! directly under `ct_hairB`. Clothes are siblings of the skeleton and are
//! not reached.

use crate::scene::Scene;
use rk_card::{ExtData, Value};

pub const KEY: &str = "KK_InvisibleBody";

/// Parents of the subtrees the plugin walks, below `BodyTop`.
pub const ROOTS: [(&str, &str); 6] = [
    ("p_cf_body_bone", "cf_j_root"),
    ("p_cf_body_bone_low", "cf_j_root"),
    ("p_cf_body_00", "cf_o_root"),
    ("p_cf_body_00_low", "cf_o_root"),
    ("p_cm_body_00", "cf_o_root"),
    ("p_cm_body_00_low", "cf_o_root"),
];

/// `InvisibleBody.AccessoryAttachPoints`.
pub const ATTACH_POINTS: [&str; 54] = [
    "a_n_nip_L",
    "a_n_nip_R",
    "a_n_shoulder_L",
    "a_n_arm_L",
    "a_n_wrist_L",
    "a_n_hand_L",
    "a_n_ind_L",
    "a_n_mid_L",
    "a_n_ring_L",
    "a_n_elbo_L",
    "a_n_shoulder_R",
    "a_n_arm_R",
    "a_n_wrist_R",
    "a_n_hand_R",
    "a_n_ind_R",
    "a_n_mid_R",
    "a_n_ring_R",
    "a_n_elbo_R",
    "a_n_mouth",
    "a_n_hair_pin_R",
    "a_n_hair_pin",
    "a_n_hair_pony",
    "a_n_hair_twin_L",
    "a_n_hair_twin_R",
    "a_n_head",
    "a_n_headflont",
    "a_n_headside",
    "a_n_headtop",
    "a_n_earrings_L",
    "a_n_earrings_R",
    "a_n_nose",
    "a_n_megane",
    "a_n_neck",
    "a_n_back",
    "a_n_back_L",
    "a_n_back_R",
    "a_n_bust",
    "a_n_bust_f",
    "a_n_ana",
    "a_n_kokan",
    "a_n_dan",
    "a_n_leg_L",
    "a_n_ankle_L",
    "a_n_heel_L",
    "a_n_knee_L",
    "a_n_leg_R",
    "a_n_ankle_R",
    "a_n_heel_R",
    "a_n_knee_R",
    "a_n_waist",
    "a_n_waist_b",
    "a_n_waist_f",
    "a_n_waist_L",
    "a_n_waist_R",
];

/// The card's `Visible` flag; a card without the data has a visible body.
pub fn visible(ext: &ExtData) -> bool {
    ext.get(KEY)
        .and_then(|p| p.data.get("Visible"))
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// A direct child by name, as `Transform.Find` with one path segment.
fn child(scene: &Scene, parent: usize, name: &str) -> Option<usize> {
    scene.nodes[parent]
        .children
        .iter()
        .copied()
        .find(|&c| scene.nodes[c].name == name)
}

/// Turns off the renderers the plugin reaches from `top` (`BodyTop`).
pub fn hide(scene: &mut Scene, top: usize) {
    let mut off = vec![false; scene.nodes.len()];
    for (prefab, root) in ROOTS {
        if let Some(root) = child(scene, top, prefab).and_then(|p| child(scene, p, root)) {
            iterate(scene, root, &mut off);
        }
    }
    for r in &mut scene.renderers {
        if off[r.node] {
            r.enabled = false;
        }
    }
}

/// `IterateVisible`: the object's own renderer always goes; its children
/// are visited unless it is an attachment point whose parent is not
/// `ct_hairB`.
fn iterate(scene: &Scene, node: usize, off: &mut [bool]) {
    let n = &scene.nodes[node];
    let under_hair = n.parent.is_some_and(|p| scene.nodes[p].name == "ct_hairB");
    if !ATTACH_POINTS.contains(&n.name.as_str()) || under_hair {
        for &c in &n.children {
            iterate(scene, c, off);
        }
    }
    off[node] = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_card::PluginData;

    fn renderer(scene: &mut Scene, name: &str, parent: usize) -> usize {
        let node = scene.add_node(name, Some(parent), Default::default());
        scene.renderers.push(crate::scene::test_renderer(name, node));
        scene.renderers.len() - 1
    }

    #[test]
    fn flag_defaults_to_visible() {
        let mut ext = ExtData::new();
        assert!(visible(&ext));
        let mut p = PluginData::default();
        p.data.insert("Visible".into(), Value::Boolean(false));
        ext.insert(KEY.into(), p);
        assert!(!visible(&ext));
    }

    #[test]
    fn body_goes_clothes_and_accessories_stay() {
        let mut s = Scene::default();
        let top = s.add_node("BodyTop", None, Default::default());
        let bone = s.add_node("p_cf_body_bone", Some(top), Default::default());
        let root = s.add_node("cf_j_root", Some(bone), Default::default());
        let face = renderer(&mut s, "cf_O_face", root);
        let point = s.add_node("a_n_neck", Some(root), Default::default());
        let necklace = renderer(&mut s, "necklace", point);
        let hair_b = s.add_node("ct_hairB", Some(root), Default::default());
        let hair_point = s.add_node("a_n_hair_pin", Some(hair_b), Default::default());
        let hair = renderer(&mut s, "hair", hair_point);
        let body = s.add_node("p_cf_body_00", Some(top), Default::default());
        let o_root = s.add_node("cf_o_root", Some(body), Default::default());
        let skin = renderer(&mut s, "o_body_a", o_root);
        let clothes = s.add_node("ct_clothesTop", Some(top), Default::default());
        let shirt = renderer(&mut s, "o_top", clothes);
        // The same name deeper down is not what `Find` returns.
        let stray = s.add_node("cf_o_root", Some(clothes), Default::default());
        let stray = renderer(&mut s, "stray", stray);

        hide(&mut s, top);
        let on = |i: usize| s.renderers[i].enabled;
        assert!(!on(face) && !on(skin) && !on(hair));
        assert!(on(necklace) && on(shirt) && on(stray));
    }
}
