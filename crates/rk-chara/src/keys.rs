// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Keys of a pose by hand over time: each bone has keys of its own, an
//! edit (`crate::pose::BoneEdit`) at a second, and between two keys the
//! edit goes from one to the other as the first key's ease says. Before
//! the first key and after the last a bone holds that key's edit.

use crate::pose::BoneEdit;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How an edit goes from a key to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ease {
    /// Slow out of the key and slow into the next.
    #[default]
    Smooth,
    /// At an even pace.
    Linear,
    /// Holds the key until the next.
    Step,
}

impl Ease {
    /// The share of the way at `t`, from 0 to 1, of the time between two
    /// keys.
    pub fn share(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Smooth => t * t * (3.0 - 2.0 * t),
            Ease::Linear => t,
            Ease::Step => 0.0,
        }
    }
}

/// A bone's edit at a second.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub time: f32,
    #[serde(flatten)]
    pub edit: BoneEdit,
    #[serde(default, skip_serializing_if = "is_smooth")]
    pub ease: Ease,
}

fn is_smooth(e: &Ease) -> bool {
    *e == Ease::Smooth
}

/// The keys of one character, by bone name, each bone's in time order.
pub type Keys = BTreeMap<String, Vec<Key>>;

/// Sets a key: the bone's edit at `time`, in place of a key at the same
/// time (within `near` seconds), keeping the keys in order.
pub fn set(keys: &mut Keys, bone: &str, time: f32, edit: BoneEdit, near: f32) {
    let list = keys.entry(bone.to_owned()).or_default();
    match list.iter_mut().find(|k| (k.time - time).abs() <= near) {
        Some(key) => key.edit = edit,
        None => {
            let at = list.partition_point(|k| k.time < time);
            list.insert(
                at,
                Key {
                    time,
                    edit,
                    ease: Ease::default(),
                },
            );
        }
    }
}

/// Takes away the bone's key at `time` (within `near` seconds). Returns
/// whether there was one.
pub fn remove(keys: &mut Keys, bone: &str, time: f32, near: f32) -> bool {
    let Some(list) = keys.get_mut(bone) else { return false };
    let before = list.len();
    list.retain(|k| (k.time - time).abs() > near);
    let removed = list.len() != before;
    if list.is_empty() {
        keys.remove(bone);
    }
    removed
}

/// A bone's edit at `time` from its keys; `None` for a bone without keys.
pub fn sample(list: &[Key], time: f32) -> Option<BoneEdit> {
    let first = list.first()?;
    if time <= first.time {
        return Some(first.edit);
    }
    let next = list.partition_point(|k| k.time <= time);
    let Some(b) = list.get(next) else {
        return list.last().map(|k| k.edit);
    };
    let a = &list[next - 1];
    let span = b.time - a.time;
    let t = if span > 0.0 {
        a.ease.share((time - a.time) / span)
    } else {
        1.0
    };
    Some(blend(a.edit, b.edit, t))
}

/// The edit at `time` of keys that hold from one to the next: the last
/// key's at or before it, the first's before that.
pub fn sample_held(list: &[Key], time: f32) -> Option<BoneEdit> {
    let next = list.partition_point(|k| k.time <= time + 1e-6);
    list.get(next.saturating_sub(1)).map(|k| k.edit)
}

/// The edit at `time` of a name's keys, held for a value that holds
/// (`crate::channels::Channel::held`).
pub fn sample_named(name: &str, list: &[Key], time: f32) -> Option<BoneEdit> {
    match crate::channels::Channel::of_key(name) {
        Some(c) if c.held() => sample_held(list, time),
        _ => sample(list, time),
    }
}

/// Every keyed bone's and value's edit at `time`.
pub fn pose_at(keys: &Keys, time: f32) -> BTreeMap<String, BoneEdit> {
    keys.iter()
        .filter_map(|(name, list)| Some((name.clone(), sample_named(name, list, time)?)))
        .collect()
}

/// The edit `t` of the way from `a` to `b`: the turn the shortest way
/// round, place and size in a straight line.
pub fn blend(a: BoneEdit, b: BoneEdit, t: f32) -> BoneEdit {
    BoneEdit {
        rot: a.rot.slerp(b.rot, t).normalize(),
        pos: a.pos.lerp(b.pos, t),
        scale: a.scale.lerp(b.scale, t),
    }
}

/// The times that have a key of any bone, in order, once each.
pub fn times(keys: &Keys) -> Vec<f32> {
    let mut all: Vec<f32> = keys.values().flatten().map(|k| k.time).collect();
    all.sort_by(f32::total_cmp);
    all.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Quat, Vec3};

    fn turned(degrees: f32) -> BoneEdit {
        BoneEdit {
            rot: Quat::from_rotation_y(degrees.to_radians()),
            ..BoneEdit::default()
        }
    }

    fn degrees(e: BoneEdit) -> f32 {
        let (axis, angle) = e.rot.to_axis_angle();
        angle.to_degrees() * axis.y.signum()
    }

    #[test]
    fn a_bone_goes_from_key_to_key_and_holds_outside_them() {
        let mut keys = Keys::new();
        set(&mut keys, "cf_j_neck", 2.0, turned(90.0), 0.01);
        set(&mut keys, "cf_j_neck", 0.0, turned(0.0), 0.01);
        let list = &keys["cf_j_neck"];
        assert_eq!(list.iter().map(|k| k.time).collect::<Vec<_>>(), [0.0, 2.0]);
        assert!(degrees(sample(list, -1.0).unwrap()).abs() < 1e-3);
        assert!(
            (degrees(sample(list, 1.0).unwrap()) - 45.0).abs() < 1e-3,
            "half way, smooth"
        );
        assert!((degrees(sample(list, 0.5).unwrap()) - 90.0 * Ease::Smooth.share(0.25)).abs() < 1e-3);
        assert!((degrees(sample(list, 5.0).unwrap()) - 90.0).abs() < 1e-3);
        assert_eq!(sample(&[], 1.0), None);
    }

    #[test]
    fn a_key_at_the_same_time_is_replaced_and_keys_can_go() {
        let mut keys = Keys::new();
        set(&mut keys, "a", 1.0, turned(10.0), 0.01);
        set(&mut keys, "a", 1.005, turned(20.0), 0.01);
        assert_eq!(keys["a"].len(), 1);
        assert!((degrees(keys["a"][0].edit) - 20.0).abs() < 1e-3);
        set(&mut keys, "b", 3.0, turned(5.0), 0.01);
        assert_eq!(times(&keys), [1.0, 3.0]);
        assert!(remove(&mut keys, "a", 1.0, 0.01));
        assert!(!keys.contains_key("a"), "a bone without keys is gone");
        assert!(!remove(&mut keys, "a", 1.0, 0.01));
    }

    #[test]
    fn a_step_holds_and_place_and_size_go_straight() {
        let a = BoneEdit {
            pos: Vec3::ZERO,
            scale: Vec3::ONE,
            ..turned(0.0)
        };
        let b = BoneEdit {
            pos: Vec3::new(0.0, 0.2, 0.0),
            scale: Vec3::splat(2.0),
            ..turned(60.0)
        };
        let list = [
            Key {
                time: 0.0,
                edit: a,
                ease: Ease::Step,
            },
            Key {
                time: 1.0,
                edit: b,
                ease: Ease::Linear,
            },
        ];
        assert_eq!(sample(&list, 0.99), Some(a));
        let mid = blend(a, b, 0.5);
        assert!(mid.pos.abs_diff_eq(Vec3::new(0.0, 0.1, 0.0), 1e-6));
        assert!(mid.scale.abs_diff_eq(Vec3::splat(1.5), 1e-6));
    }

    #[test]
    fn keys_are_written_and_read_back() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct File {
            keys: Keys,
        }
        let mut keys = Keys::new();
        set(&mut keys, "cf_j_arm00_L", 0.5, turned(30.0), 0.01);
        keys.get_mut("cf_j_arm00_L").unwrap()[0].ease = Ease::Step;
        let file = File { keys };
        let text = toml::to_string(&file).unwrap();
        assert_eq!(toml::from_str::<File>(&text).unwrap(), file);
    }
}
