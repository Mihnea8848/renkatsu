// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Motion trails: where the chosen bones and IK targets of the selected
//! character are at every frame of the loop range, or of the whole
//! sequence, for the viewport to draw as lines through the scene. The
//! places are worked out once and kept until what they come from changes:
//! the keys, the clip, the baked clip, the pose by hand, the bones chosen,
//! the range, or where she stands. Moving the playhead changes none of it.

use super::frame_of;
use crate::scene::{Content, ObjectId};
use crate::sequence::{Track, BAKED};
use crate::state::State;
use glam::{Mat4, Vec3};

/// As many trails as are drawn at once.
const MOST: usize = 8;

/// A bone's or an IK target's trail.
pub struct Trail {
    /// Its place in the world at each frame, from the first.
    pub points: Vec<Vec3>,
    /// The frames it has a key by hand on.
    pub keyed: Vec<i32>,
}

/// What the trails were worked out from.
struct From {
    object: ObjectId,
    names: Vec<String>,
    frames: (i32, i32),
    fps: f32,
    track: Option<Track>,
    clip: Option<Box<crate::scene::Animation>>,
    pose: Option<Box<rk_chara::pose::Layer>>,
    /// Where she stands, and whether her clip was read yet.
    place: Option<Mat4>,
    clip_read: bool,
}

/// The trails kept, and the frame their first points are of.
pub struct Trails {
    from: From,
    pub first: i32,
    pub trails: Vec<Trail>,
}

/// The bones and IK targets that get a trail: the chosen one, and those of
/// the selected keys. Values of the face and the hands have no place.
fn names(st: &State, object: ObjectId) -> Vec<String> {
    let placed = |name: &str| rk_chara::channels::Channel::of_key(name).is_none();
    let plain = |name: &str| name.strip_prefix(BAKED).unwrap_or(name).to_owned();
    let mut names: Vec<String> = super::chosen(st).iter().map(|n| plain(n)).collect();
    names.extend(
        st.anim
            .selected
            .iter()
            .filter(|key| key.0 == object)
            .map(|key| plain(&key.1)),
    );
    names.retain(|name| placed(name));
    let mut once = Vec::new();
    for name in names {
        if !once.contains(&name) && once.len() < MOST {
            once.push(name);
        }
    }
    once
}

/// Makes the trails match the scene: worked out again when what they come
/// from changed, kept otherwise, and let go while the switch is off or no
/// character is selected.
pub fn update(st: &mut State) {
    let character = st
        .doc
        .selection()
        .filter(|o| matches!(o.content, Content::Character { .. }))
        .map(|o| o.id);
    let Some(object) = character.filter(|_| st.anim.trail) else {
        st.anim.trails = None;
        return;
    };
    let scene = st.doc.scene();
    let sequence = &scene.sequence;
    let fps = sequence.fps.max(1.0);
    let frames = match sequence.loop_range {
        Some([from, to]) => (from, to),
        None => (0, frame_of(sequence.length, fps)),
    };
    let (clip, pose) = match scene.get(object).map(|o| &o.content) {
        Some(Content::Character {
            animation, pose_layer, ..
        }) => (animation, pose_layer),
        _ => (&None, &None),
    };
    let names = names(st, object);
    let place = st.live.as_ref().and_then(|l| l.ik_space(object));
    let clip_read = st.live.as_ref().is_some_and(|l| l.clip_read(object));
    // Compared where they lie; copied only when the trails are made again.
    let same = st.anim.trails.as_ref().is_some_and(|t| {
        let f = &t.from;
        f.object == object
            && f.names == names
            && f.frames == frames
            && f.fps == fps
            && f.place == place
            && f.clip_read == clip_read
            && f.track.as_ref() == sequence.track(object)
            && f.clip == *clip
            && f.pose == *pose
    });
    if same {
        return;
    }
    let from = From {
        object,
        names,
        frames,
        fps,
        track: sequence.track(object).cloned(),
        clip: clip.clone(),
        pose: pose.clone(),
        place,
        clip_read,
    };
    let times: Vec<f32> = (frames.0..=frames.1).map(|f| f as f32 / fps).collect();
    let points = st
        .live
        .as_mut()
        .and_then(|l| l.trail_points(object, st.doc.scene(), &from.names, &times))
        .unwrap_or_default();
    let trails = from
        .names
        .iter()
        .zip(points)
        .map(|(name, points)| Trail {
            points,
            keyed: from
                .track
                .as_ref()
                .and_then(|t| t.keys.get(name))
                .map(|list| list.iter().map(|k| frame_of(k.time, fps)).collect())
                .unwrap_or_default(),
        })
        .collect();
    st.anim.trails = Some(Trails {
        from,
        first: frames.0,
        trails,
    });
}
