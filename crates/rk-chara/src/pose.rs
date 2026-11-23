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

use crate::bone_info::Part;
use crate::scene::{Scene, Trs};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One character's edits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// Whether the bones are posed by hand (FK) at all, as CharaStudio's
    /// FK switch: off by default, the edits kept but not applied.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fk_on: bool,
    /// The FK parts switched on, as CharaStudio's: a bone of its FK table
    /// takes its edit only while one of its parts is on. Bones outside the
    /// table take theirs whenever FK is on.
    #[serde(default = "start_parts", skip_serializing_if = "is_start_parts")]
    pub fk_parts: Vec<Part>,
    /// Edited bones by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fk: BTreeMap<String, BoneEdit>,
}

impl Default for Layer {
    fn default() -> Self {
        Layer {
            fk_on: false,
            fk_parts: start_parts(),
            fk: BTreeMap::new(),
        }
    }
}

fn start_parts() -> Vec<Part> {
    Part::START.to_vec()
}

// serde's `skip_serializing_if` hands it the field as it is.
#[allow(clippy::ptr_arg)]
fn is_start_parts(parts: &Vec<Part>) -> bool {
    let mut sorted = parts.clone();
    sorted.sort();
    let mut start = start_parts();
    start.sort();
    sorted == start
}

impl Layer {
    /// Whether the layer holds nothing: FK off, its parts as they start and
    /// no edits kept.
    pub fn is_empty(&self) -> bool {
        !self.fk_on && is_start_parts(&self.fk_parts) && self.fk.values().all(BoneEdit::is_identity)
    }

    /// Whether a bone of the FK table, in `parts`, takes its edit: one of
    /// its parts is on.
    pub fn part_on(&self, parts: &[Part]) -> bool {
        parts.iter().any(|p| self.fk_parts.contains(p))
    }

    /// The edits to apply, with the nodes of their bones in `scene`, found
    /// by name below `from` (the first in depth-first order, as the game's
    /// `FindLoop` finds them); none while FK is off, and bones the
    /// character lacks are left out.
    pub fn resolve(&self, scene: &Scene, from: usize) -> Vec<(usize, BoneEdit)> {
        self.resolve_parts(scene, from, |_| None)
    }

    /// As `resolve`, leaving out the bones of FK parts that are off:
    /// `parts_of` gives the parts of a bone of the FK table by name, none
    /// for a bone outside it.
    pub fn resolve_parts(
        &self,
        scene: &Scene,
        from: usize,
        parts_of: impl Fn(&str) -> Option<Vec<Part>>,
    ) -> Vec<(usize, BoneEdit)> {
        if !self.fk_on {
            return Vec::new();
        }
        self.fk
            .iter()
            .filter(|(_, e)| !e.is_identity())
            .filter(|(name, _)| parts_of(name).is_none_or(|parts| self.part_on(&parts)))
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

/// The name of a bone's twin on the other side (`_L` and `_R` swapped),
/// or the bone itself when it lies in the middle.
pub fn twin_name(name: &str) -> String {
    if let Some(stem) = name.strip_suffix("_L") {
        format!("{stem}_R")
    } else if let Some(stem) = name.strip_suffix("_R") {
        format!("{stem}_L")
    } else {
        name.to_owned()
    }
}

/// A turn reflected across the character's middle (the plane x = 0 of its
/// own space): about the reflected axis, by the same angle.
pub fn reflect(q: Quat) -> Quat {
    Quat::from_xyzw(q.x, -q.y, -q.z, q.w)
}

/// The edits that mirror bones onto others: for each `(source, target)`,
/// the target turns away from its rest pose as the source does, reflected
/// across the character's middle. Everything is in the space of `top`, the
/// character's root, and goes through the rest pose (`rest`, each node's
/// local transform as built), so it holds however the two sides' bones are
/// laid. The edits are over the bones' poses of the frame without their
/// edits (`applied`, the edits the bones carry now), parents first.
pub fn mirrored(
    scene: &Scene,
    rest: &[Trs],
    top: usize,
    applied: &[(usize, BoneEdit)],
    pairs: &[(usize, usize)],
) -> Vec<(usize, Quat)> {
    let (world, _) = scene.world();
    let turn_of = |n: usize| world[n].to_scale_rotation_translation().1;
    let top_turn = turn_of(top).inverse();
    // Turns in the character's own space: as posed now, and at rest.
    let posed = |n: usize| (top_turn * turn_of(n)).normalize();
    let at_rest = |mut n: usize| {
        let mut q = Quat::IDENTITY;
        while n != top {
            q = rest[n].rot * q;
            match scene.nodes[n].parent {
                Some(p) => n = p,
                None => break,
            }
        }
        q.normalize()
    };
    let depth = |mut n: usize| {
        let mut d = 0;
        while let Some(p) = scene.nodes[n].parent {
            d += 1;
            n = p;
        }
        d
    };
    let mut order: Vec<(usize, usize)> = pairs.to_vec();
    order.sort_by_key(|(_, t)| depth(*t));
    let mut now: std::collections::HashMap<usize, Quat> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for (source, target) in order {
        let away = posed(source) * at_rest(source).inverse();
        let wanted = (reflect(away) * at_rest(target)).normalize();
        let parent = match scene.nodes[target].parent {
            Some(p) if p != top => now.get(&p).copied().unwrap_or_else(|| posed(p)),
            _ => Quat::IDENTITY,
        };
        let local = (parent.inverse() * wanted).normalize();
        let edit = applied
            .iter()
            .find(|(n, _)| *n == target)
            .map_or(Quat::IDENTITY, |(_, e)| e.rot);
        let base = scene.nodes[target].local.rot * edit.inverse();
        out.push((target, (base.inverse() * local).normalize()));
        now.insert(target, wanted);
    }
    out
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
    fn a_bone_takes_its_edit_only_while_one_of_its_parts_is_on() {
        let mut scene = Scene::default();
        let root = scene.add_node("cf_j_root", None, Trs::default());
        let hair = scene.add_node("cf_j_hair01", Some(root), Trs::default());
        let arm = scene.add_node("cf_j_arm00_L", Some(root), Trs::default());
        let other = scene.add_node("cf_j_tang", Some(root), Trs::default());
        let mut layer = Layer {
            fk_on: true,
            ..Default::default()
        };
        let turn = BoneEdit {
            rot: Quat::from_rotation_z(0.4),
            ..Default::default()
        };
        for name in ["cf_j_hair01", "cf_j_arm00_L", "cf_j_tang"] {
            layer.fk.insert(name.into(), turn);
        }
        let parts_of = |name: &str| match name {
            "cf_j_hair01" => Some(vec![Part::Hair]),
            "cf_j_arm00_L" => Some(vec![Part::Body]),
            _ => None,
        };
        // Hair starts off; the body is on; a bone outside the table follows FK.
        let nodes = |l: &Layer| {
            l.resolve_parts(&scene, root, parts_of)
                .into_iter()
                .map(|(n, _)| n)
                .collect::<Vec<_>>()
        };
        assert_eq!(nodes(&layer), vec![arm, other]);
        layer.fk_parts.push(Part::Hair);
        assert_eq!(nodes(&layer).len(), 3);
        assert!(nodes(&layer).contains(&hair));
        // The parts as they start are not written.
        assert!(!toml::to_string(&Layer::default()).unwrap().contains("fk_parts"));
        assert!(toml::to_string(&layer).unwrap().contains("hair"));
    }

    /// A small body whose two arms mirror each other at rest, as a
    /// character's do: the arms' turns reflected, their hands' places too.
    fn body() -> (Scene, Vec<Trs>, [usize; 5]) {
        let mut scene = Scene::default();
        let root = scene.add_node("root", None, Trs::default());
        let hips = scene.add_node("cf_j_hips", Some(root), Trs::default());
        let arm_turn = Quat::from_euler(glam::EulerRot::YXZ, 0.3, -0.2, 0.9);
        let trs = |pos: Vec3, rot: Quat| Trs {
            pos,
            rot,
            scale: Vec3::ONE,
        };
        let arm_l = scene.add_node("cf_j_arm00_L", Some(hips), trs(Vec3::new(0.2, 1.4, 0.0), arm_turn));
        let arm_r = scene.add_node(
            "cf_j_arm00_R",
            Some(hips),
            trs(Vec3::new(-0.2, 1.4, 0.0), reflect(arm_turn)),
        );
        let hand_l = scene.add_node(
            "cf_j_hand_L",
            Some(arm_l),
            trs(Vec3::new(0.3, 0.05, 0.1), Quat::IDENTITY),
        );
        let _hand_r = scene.add_node(
            "cf_j_hand_R",
            Some(arm_r),
            trs(Vec3::new(-0.3, 0.05, 0.1), Quat::IDENTITY),
        );
        let rest = scene.nodes.iter().map(|n| n.local).collect();
        (scene, rest, [root, hips, arm_l, arm_r, hand_l])
    }

    #[test]
    fn mirroring_puts_the_twin_where_the_mirror_shows_it() {
        let (mut scene, rest, [root, _, arm_l, arm_r, hand_l]) = body();
        // The left arm raised by hand.
        let edit = BoneEdit {
            rot: Quat::from_euler(glam::EulerRot::YXZ, 0.4, 0.7, -0.5),
            ..Default::default()
        };
        apply(&mut scene, &[(arm_l, edit)]);
        let found = mirrored(&scene, &rest, root, &[(arm_l, edit)], &[(arm_l, arm_r)]);
        assert_eq!(found.len(), 1);
        let edits: Vec<(usize, BoneEdit)> = found
            .iter()
            .map(|(n, rot)| {
                (
                    *n,
                    BoneEdit {
                        rot: *rot,
                        ..Default::default()
                    },
                )
            })
            .collect();
        apply(&mut scene, &edits);
        let (world, _) = scene.world();
        let hand_r = scene.find(root, "cf_j_hand_R").unwrap();
        let left = world[hand_l].w_axis.truncate();
        let right = world[hand_r].w_axis.truncate();
        assert!(
            right.abs_diff_eq(Vec3::new(-left.x, left.y, left.z), 1e-4),
            "{left} {right}"
        );
    }

    #[test]
    fn mirroring_twice_gives_the_pose_back() {
        let (mut scene, rest, [root, _, arm_l, arm_r, _]) = body();
        let start = BoneEdit {
            rot: Quat::from_euler(glam::EulerRot::YXZ, -0.6, 0.2, 0.8),
            ..Default::default()
        };
        apply(&mut scene, &[(arm_l, start)]);
        let swap = [(arm_l, arm_r), (arm_r, arm_l)];
        let once = mirrored(&scene, &rest, root, &[(arm_l, start)], &swap);
        let as_edits = |list: &[(usize, Quat)]| -> Vec<(usize, BoneEdit)> {
            list.iter()
                .map(|(n, rot)| {
                    (
                        *n,
                        BoneEdit {
                            rot: *rot,
                            ..Default::default()
                        },
                    )
                })
                .collect()
        };
        // Pose the swapped edits from the bones as they rest.
        for (n, r) in scene.nodes.iter_mut().zip(&rest) {
            n.local = *r;
        }
        let once = as_edits(&once);
        apply(&mut scene, &once);
        let twice = mirrored(&scene, &rest, root, &once, &swap);
        let back = twice.iter().find(|(n, _)| *n == arm_l).unwrap().1;
        assert!(
            back.abs_diff_eq(start.rot, 1e-4) || back.abs_diff_eq(-start.rot, 1e-4),
            "{back}"
        );
        assert_eq!(twin_name("cf_j_arm00_L"), "cf_j_arm00_R");
        assert_eq!(twin_name("cf_j_spine01"), "cf_j_spine01");
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
        // Kept but not applied while FK is off.
        assert!(layer.resolve(&scene, root).is_empty() && !layer.is_empty());
        layer.fk_on = true;
        let edits = layer.resolve(&scene, root);
        assert_eq!(edits, vec![(arm, turn)]);
        apply(&mut scene, &edits);
        assert!(scene.nodes[arm].local.rot.abs_diff_eq(turn.rot, 1e-6));
    }
}
