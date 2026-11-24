// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Keys of poses by hand in the studio: what a bone shows and where its
//! edits go. A bone with keys shows its keys at the playhead, and an edit
//! of it sets its key there, as an animation editor's auto key does; a
//! bone without keys shows and takes its edit in the pose layer. So what
//! shows is what plays.

use crate::scene::{Content, ObjectId};
use crate::sequence::Track;
use crate::state::State;
use rk_chara::keys;
use rk_chara::pose::BoneEdit;

/// The time keys are set at: the scene's clock on the sequence's frames.
pub fn now(st: &State) -> f32 {
    st.doc.scene().sequence.snap(st.clock)
}

fn track(st: &State, id: ObjectId) -> Option<&Track> {
    st.doc.scene().sequence.track(id)
}

/// Whether a bone of a character has keys.
pub fn keyed(st: &State, id: ObjectId, bone: &str) -> bool {
    track(st, id).is_some_and(|t| t.keys.contains_key(bone))
}

/// The edit a bone shows: its keys' at the playhead, or the pose layer's.
pub fn shown(st: &State, id: ObjectId, bone: &str) -> BoneEdit {
    if let Some(edit) = track(st, id)
        .and_then(|t| t.keys.get(bone))
        .and_then(|list| keys::sample(list, st.clock))
    {
        return edit;
    }
    match st.doc.scene().get(id).map(|o| &o.content) {
        Some(Content::Character {
            pose_layer: Some(l), ..
        }) => l.fk.get(bone).copied().unwrap_or_default(),
        _ => BoneEdit::default(),
    }
}

/// Gives a bone an edit: its key at the playhead when it has keys, else
/// the pose layer's. With `held` it joins the step of a drag.
pub fn write(st: &mut State, id: ObjectId, bone: &str, edit: BoneEdit, held: bool) {
    if keyed(st, id, bone) {
        let at = now(st);
        let mut sequence = st.doc.scene().sequence.clone();
        let near = sequence.near();
        let name = st.doc.scene().get(id).map(|o| o.name.clone()).unwrap_or_default();
        keys::set(&mut sequence.track_mut(id, &name).keys, bone, at, edit, near);
        st.doc.edit_sequence(sequence, held);
        return;
    }
    let Some(mut object) = st.doc.scene().get(id).cloned() else {
        return;
    };
    let Content::Character { pose_layer, .. } = &mut object.content else {
        return;
    };
    let layer = pose_layer.get_or_insert_with(Default::default);
    if edit.is_identity() {
        layer.fk.remove(bone);
    } else {
        layer.fk.insert(bone.to_owned(), edit);
    }
    if layer.is_empty() {
        *pose_layer = None;
    }
    st.doc.edit(object, held);
}

/// Keys `bones` at the playhead, each with the edit it shows, as one step.
pub fn set_keys(st: &mut State, id: ObjectId, bones: &[String]) {
    let at = now(st);
    let edits: Vec<(String, BoneEdit)> = bones.iter().map(|b| (b.clone(), shown(st, id, b))).collect();
    let mut sequence = st.doc.scene().sequence.clone();
    let near = sequence.near();
    let name = st.doc.scene().get(id).map(|o| o.name.clone()).unwrap_or_default();
    let track = sequence.track_mut(id, &name);
    for (bone, edit) in edits {
        keys::set(&mut track.keys, &bone, at, edit, near);
    }
    st.doc.edit_sequence(sequence, false);
}

/// Takes away the keys of `bones` at the playhead, as one step. Returns
/// whether there were any.
pub fn delete_keys(st: &mut State, id: ObjectId, bones: &[String]) -> bool {
    let at = now(st);
    let mut sequence = st.doc.scene().sequence.clone();
    let near = sequence.near();
    let Some(track) = sequence.tracks.iter_mut().find(|t| t.object == id) else {
        return false;
    };
    let mut any = false;
    for bone in bones {
        any |= keys::remove(&mut track.keys, bone, at, near);
    }
    if any {
        sequence.tidy();
        st.doc.edit_sequence(sequence, false);
    }
    any
}

/// Changes a character's keys as one step of the history; `change`
/// returns whether it changed anything.
fn change_keys(st: &mut State, id: ObjectId, change: impl FnOnce(&mut keys::Keys, f32) -> bool) -> bool {
    let mut sequence = st.doc.scene().sequence.clone();
    let near = sequence.near();
    let Some(track) = sequence.tracks.iter_mut().find(|t| t.object == id) else {
        return false;
    };
    if !change(&mut track.keys, near) {
        return false;
    }
    sequence.tidy();
    st.doc.edit_sequence(sequence, false)
}

/// How a bone's key at `time` eases into the next.
pub fn set_ease(st: &mut State, id: ObjectId, bone: &str, time: f32, ease: keys::Ease) {
    change_keys(st, id, |all, near| {
        let Some(key) = all
            .get_mut(bone)
            .and_then(|l| l.iter_mut().find(|k| (k.time - time).abs() <= near))
        else {
            return false;
        };
        key.ease = ease;
        true
    });
}

/// Moves a bone's key from `from` to `to`, over any key there.
pub fn move_key(st: &mut State, id: ObjectId, bone: &str, from: f32, to: f32) {
    let to = st.doc.scene().sequence.snap(to);
    change_keys(st, id, |all, near| {
        let Some(key) = all
            .get(bone)
            .and_then(|l| l.iter().find(|k| (k.time - from).abs() <= near))
            .copied()
        else {
            return false;
        };
        keys::remove(all, bone, from, near);
        keys::set(all, bone, to, key.edit, near);
        if let Some(moved) = all
            .get_mut(bone)
            .and_then(|l| l.iter_mut().find(|k| (k.time - to).abs() <= near))
        {
            moved.ease = key.ease;
        }
        true
    });
}

/// Takes away a bone's key at `time`.
pub fn delete_key_at(st: &mut State, id: ObjectId, bone: &str, time: f32) {
    change_keys(st, id, |all, near| keys::remove(all, bone, time, near));
}

/// Takes away every key of a bone.
pub fn delete_bone_keys(st: &mut State, id: ObjectId, bone: &str) {
    change_keys(st, id, |all, _| all.remove(bone).is_some());
}

/// Takes away every key of a character.
pub fn clear_keys(st: &mut State, id: ObjectId) {
    change_keys(st, id, |all, _| {
        let had = !all.is_empty();
        all.clear();
        had
    });
}

/// Keys a bone at the playhead with `edit`, as one step.
pub fn paste_key(st: &mut State, id: ObjectId, bone: &str, edit: BoneEdit) {
    let at = now(st);
    let mut sequence = st.doc.scene().sequence.clone();
    let near = sequence.near();
    let name = st.doc.scene().get(id).map(|o| o.name.clone()).unwrap_or_default();
    keys::set(&mut sequence.track_mut(id, &name).keys, bone, at, edit, near);
    st.doc.edit_sequence(sequence, false);
}

/// The bones a palette's key buttons act on: the chosen bone, or every
/// bone the character's pose has an edit or a key for.
pub fn bones_to_key(st: &State, id: ObjectId) -> Vec<String> {
    if let Some(bone) = st.pose_bone.clone() {
        return vec![bone];
    }
    let mut bones: Vec<String> = match st.doc.scene().get(id).map(|o| &o.content) {
        Some(Content::Character {
            pose_layer: Some(l), ..
        }) => l.fk.keys().cloned().collect(),
        _ => Vec::new(),
    };
    if let Some(t) = track(st, id) {
        bones.extend(t.keys.keys().cloned());
    }
    bones.sort();
    bones.dedup();
    bones
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use crate::scene::Scene;
    use glam::Quat;

    fn turned(degrees: f32) -> BoneEdit {
        BoneEdit {
            rot: Quat::from_rotation_y(degrees.to_radians()),
            ..BoneEdit::default()
        }
    }

    #[test]
    fn an_edit_of_a_keyed_bone_sets_its_key_and_others_go_in_the_pose() {
        let mut st = State::new(Project::none(), Scene::new());
        let her = st.doc.add_character("Her", "Characters/her.png".into());
        // Unkeyed: the pose layer takes it.
        write(&mut st, her, "cf_j_neck", turned(10.0), false);
        assert_eq!(shown(&st, her, "cf_j_neck"), turned(10.0));
        assert!(!keyed(&st, her, "cf_j_neck"));
        // Keyed at 1 s, then edited at 2 s: a second key, the pose layer
        // left as it was.
        st.clock = 1.0;
        set_keys(&mut st, her, &["cf_j_neck".to_owned()]);
        st.clock = 2.0;
        write(&mut st, her, "cf_j_neck", turned(40.0), false);
        let keys = &st.doc.scene().sequence.track(her).unwrap().keys["cf_j_neck"];
        assert_eq!(keys.iter().map(|k| k.time).collect::<Vec<_>>(), [1.0, 2.0]);
        assert!((shown(&st, her, "cf_j_neck").rot.dot(turned(40.0).rot).abs() - 1.0).abs() < 1e-6);
        let layer = match &st.doc.scene().get(her).unwrap().content {
            Content::Character { pose_layer, .. } => pose_layer.clone().unwrap(),
            _ => unreachable!(),
        };
        assert_eq!(layer.fk["cf_j_neck"], turned(10.0));
        // A key undoes like any edit, and a deleted key leaves no track.
        st.doc.undo();
        assert_eq!(st.doc.scene().sequence.track(her).unwrap().keys["cf_j_neck"].len(), 1);
        st.clock = 1.0;
        assert!(delete_keys(&mut st, her, &["cf_j_neck".to_owned()]));
        assert!(st.doc.scene().sequence.track(her).is_none());
    }

    #[test]
    fn keys_move_ease_paste_and_go() {
        let mut st = State::new(Project::none(), Scene::new());
        let her = st.doc.add_character("Her", "Characters/her.png".into());
        let neck = "cf_j_neck".to_owned();
        st.clock = 1.0;
        write(&mut st, her, &neck, turned(20.0), false);
        set_keys(&mut st, her, std::slice::from_ref(&neck));
        let list = |st: &State| st.doc.scene().sequence.track(her).map(|t| t.keys[&neck].clone());
        // Moved to the playhead, keeping how it eases.
        set_ease(&mut st, her, &neck, 1.0, keys::Ease::Step);
        move_key(&mut st, her, &neck, 1.0, 2.5);
        let moved = list(&st).unwrap();
        assert_eq!((moved.len(), moved[0].time, moved[0].ease), (1, 2.5, keys::Ease::Step));
        // Pasted at the playhead on another bone.
        st.clock = 0.5;
        paste_key(&mut st, her, "cf_j_head", turned(5.0));
        assert_eq!(
            st.doc.scene().sequence.track(her).unwrap().keys["cf_j_head"][0].time,
            0.5
        );
        delete_key_at(&mut st, her, "cf_j_head", 0.5);
        assert!(!st
            .doc
            .scene()
            .sequence
            .track(her)
            .unwrap()
            .keys
            .contains_key("cf_j_head"));
        delete_bone_keys(&mut st, her, &neck);
        assert!(st.doc.scene().sequence.track(her).is_none());
        set_keys(&mut st, her, std::slice::from_ref(&neck));
        clear_keys(&mut st, her);
        assert!(st.doc.scene().sequence.tracks.is_empty());
    }
}
