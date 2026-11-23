// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! A character's pose made by hand: a layer of edits on top of whatever
//! the animation (or, without one, the character as built) puts the bones
//! in. The animation itself is never changed, and the same edits hold on
//! every frame of it.
//!
//! Each edited bone, by name, carries a turn, a shift and a scale over
//! its pose of the frame: the turn is applied in the bone's own space after
//! the pose's (`rotation * turn`, as CharaStudio's rotate handle turns a
//! bone), the shift is added, the scale multiplies. Plain values, so that
//! keys over time can drive them later.

use crate::scene::{Scene, Trs};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One character's edits.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// Edited bones by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fk: BTreeMap<String, BoneEdit>,
}

impl Layer {
    /// Whether the layer changes nothing.
    pub fn is_empty(&self) -> bool {
        self.fk.values().all(BoneEdit::is_identity)
    }

    /// The edits with the nodes of their bones in `scene`, found by name
    /// below `from` (the first in depth-first order, as the game's
    /// `FindLoop` finds them); bones the character lacks are left out.
    pub fn resolve(&self, scene: &Scene, from: usize) -> Vec<(usize, BoneEdit)> {
        self.fk
            .iter()
            .filter(|(_, e)| !e.is_identity())
            .filter_map(|(name, e)| Some((scene.find(from, name)?, *e)))
            .collect()
    }
}

/// The edit of one bone, over its pose of the frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoneEdit {
    /// Turn in the bone's own space, as x, y, z, w.
    #[serde(default = "no_turn", skip_serializing_if = "is_no_turn")]
    pub rot: Quat,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pos: Vec3,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: Vec3,
}

impl Default for BoneEdit {
    fn default() -> Self {
        BoneEdit {
            rot: Quat::IDENTITY,
            pos: Vec3::ZERO,
            scale: Vec3::ONE,
        }
    }
}

impl BoneEdit {
    pub fn is_identity(&self) -> bool {
        is_no_turn(&self.rot) && is_zero(&self.pos) && is_one(&self.scale)
    }

    /// The bone's local transform with the edit on `base`.
    pub fn on(&self, base: Trs) -> Trs {
        Trs {
            pos: base.pos + self.pos,
            rot: (base.rot * self.rot).normalize(),
            scale: base.scale * self.scale,
        }
    }
}

fn no_turn() -> Quat {
    Quat::IDENTITY
}

fn is_no_turn(q: &Quat) -> bool {
    *q == Quat::IDENTITY || *q == -Quat::IDENTITY
}

fn one() -> Vec3 {
    Vec3::ONE
}

fn is_one(v: &Vec3) -> bool {
    *v == Vec3::ONE
}

fn is_zero(v: &Vec3) -> bool {
    *v == Vec3::ZERO
}

/// Puts resolved edits on the bones as they stand.
pub fn apply(scene: &mut Scene, edits: &[(usize, BoneEdit)]) {
    for &(node, edit) in edits {
        if let Some(n) = scene.nodes.get_mut(node) {
            n.local = edit.on(n.local);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edit_turns_the_bone_in_its_own_space_after_its_pose() {
        let base = Trs {
            pos: Vec3::new(0.0, 1.0, 0.0),
            rot: Quat::from_rotation_y(0.5),
            scale: Vec3::ONE,
        };
        let edit = BoneEdit {
            rot: Quat::from_rotation_x(0.3),
            pos: Vec3::new(0.1, 0.0, 0.0),
            scale: Vec3::splat(2.0),
        };
        let out = edit.on(base);
        assert!(out.rot.abs_diff_eq(base.rot * edit.rot, 1e-6));
        assert_eq!(out.pos, Vec3::new(0.1, 1.0, 0.0));
        assert_eq!(out.scale, Vec3::splat(2.0));
        // No edit changes nothing.
        let same = BoneEdit::default().on(base);
        assert!(same.rot.abs_diff_eq(base.rot, 1e-6) && same.pos == base.pos && same.scale == base.scale);
    }

    #[test]
    fn a_layer_reads_back_and_leaves_out_what_is_unchanged() {
        let mut layer = Layer::default();
        assert!(layer.is_empty());
        layer.fk.insert(
            "cf_j_arm00_L".into(),
            BoneEdit {
                rot: Quat::from_rotation_z(-1.0),
                ..Default::default()
            },
        );
        let text = toml::to_string(&layer).unwrap();
        assert!(!text.contains("pos") && !text.contains("scale"), "{text}");
        let back: Layer = toml::from_str(&text).unwrap();
        assert_eq!(back, layer);
        assert!(!back.is_empty());
    }

    #[test]
    fn resolve_finds_bones_by_name_and_skips_the_missing() {
        let mut scene = Scene::default();
        let root = scene.add_node("cf_j_root", None, Trs::default());
        let arm = scene.add_node("cf_j_arm00_L", Some(root), Trs::default());
        let mut layer = Layer::default();
        let turn = BoneEdit {
            rot: Quat::from_rotation_z(0.4),
            ..Default::default()
        };
        layer.fk.insert("cf_j_arm00_L".into(), turn);
        layer.fk.insert("cf_j_tail".into(), turn);
        let edits = layer.resolve(&scene, root);
        assert_eq!(edits, vec![(arm, turn)]);
        apply(&mut scene, &edits);
        assert!(scene.nodes[arm].local.rot.abs_diff_eq(turn.rot, 1e-6));
    }
}
