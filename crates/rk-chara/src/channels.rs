// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Values of a pose that take keys beside the bones: the hands' patterns,
//! the face and the skin effects. A key holds its value in the place of a
//! bone edit (`crate::keys::Key`), under a name of its own (`face/mouth`,
//! `hand/left`, `skin/WetBody`). Patterns, levels and switches hold from
//! one key to the next, since a value between two patterns means nothing;
//! amounts ease as bones do.

use crate::face::Face;
use crate::pose::{BoneEdit, Layer};
use glam::Vec3;

/// A value that takes keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    Eyebrows,
    Eyes,
    Mouth,
    EyesOpen,
    Blink,
    MouthOpen,
    Tears,
    Blush,
    Highlights,
    EyeRoll,
    SmallIrises,
    Shaking,
    /// A hand's pattern, 0 left and 1 right.
    Hand(usize),
    /// A skin effect, by its place in `crate::skin_effects::EFFECTS`.
    Skin(usize),
}

/// The face's values in their order.
pub const FACE: [Channel; 12] = [
    Channel::Eyebrows,
    Channel::Eyes,
    Channel::Mouth,
    Channel::EyesOpen,
    Channel::Blink,
    Channel::MouthOpen,
    Channel::Tears,
    Channel::Blush,
    Channel::Highlights,
    Channel::EyeRoll,
    Channel::SmallIrises,
    Channel::Shaking,
];

impl Channel {
    /// Its name among the keys.
    pub fn key(self) -> String {
        match self {
            Channel::Hand(0) => "hand/left".into(),
            Channel::Hand(_) => "hand/right".into(),
            Channel::Skin(i) => format!("skin/{}", crate::skin_effects::EFFECTS.get(i).map_or("", |e| e.key)),
            face => format!("face/{}", face.face_name()),
        }
    }

    fn face_name(self) -> &'static str {
        match self {
            Channel::Eyebrows => "eyebrows",
            Channel::Eyes => "eyes",
            Channel::Mouth => "mouth",
            Channel::EyesOpen => "eyes_open",
            Channel::Blink => "blink",
            Channel::MouthOpen => "mouth_open",
            Channel::Tears => "tears",
            Channel::Blush => "blush",
            Channel::Highlights => "highlights",
            Channel::EyeRoll => "eye_roll",
            Channel::SmallIrises => "small_irises",
            Channel::Shaking => "shaking",
            Channel::Hand(_) | Channel::Skin(_) => "",
        }
    }

    /// The value a key name is of, if any.
    pub fn of_key(name: &str) -> Option<Channel> {
        if let Some(face) = name.strip_prefix("face/") {
            return FACE.into_iter().find(|c| c.face_name() == face);
        }
        match name {
            "hand/left" => return Some(Channel::Hand(0)),
            "hand/right" => return Some(Channel::Hand(1)),
            _ => {}
        }
        let skin = name.strip_prefix("skin/")?;
        crate::skin_effects::EFFECTS
            .iter()
            .position(|e| e.key == skin)
            .map(Channel::Skin)
    }

    /// Whether it holds from key to key rather than easing.
    pub fn held(self) -> bool {
        !matches!(
            self,
            Channel::EyesOpen | Channel::MouthOpen | Channel::Blush | Channel::EyeRoll | Channel::SmallIrises
        )
    }

    /// The name shown.
    pub fn label(self) -> String {
        match self {
            Channel::Eyebrows => "Eyebrows".into(),
            Channel::Eyes => "Eyes".into(),
            Channel::Mouth => "Mouth".into(),
            Channel::EyesOpen => "Eyes open".into(),
            Channel::Blink => "Blink".into(),
            Channel::MouthOpen => "Mouth open".into(),
            Channel::Tears => "Tears".into(),
            Channel::Blush => "Blush".into(),
            Channel::Highlights => "Highlights".into(),
            Channel::EyeRoll => "Eye roll".into(),
            Channel::SmallIrises => "Small irises".into(),
            Channel::Shaking => "Shaking".into(),
            Channel::Hand(0) => "Left hand".into(),
            Channel::Hand(_) => "Right hand".into(),
            Channel::Skin(i) => crate::skin_effects::EFFECTS.get(i).map_or("Skin", |e| e.name).into(),
        }
    }

    /// Its value in a layer, the face it starts with standing in for a
    /// face the layer does not set.
    pub fn get(self, layer: &Layer, start: &Face) -> f32 {
        let face = layer.face.unwrap_or(*start);
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        match self {
            Channel::Eyebrows => face.eyebrows as f32,
            Channel::Eyes => face.eyes as f32,
            Channel::Mouth => face.mouth as f32,
            Channel::EyesOpen => face.eyes_open,
            Channel::Blink => flag(face.blink),
            Channel::MouthOpen => face.mouth_open,
            Channel::Tears => f32::from(face.tears),
            Channel::Blush => face.blush,
            Channel::Highlights => flag(!face.hide_highlight),
            Channel::EyeRoll => face.eye_y,
            Channel::SmallIrises => face.eye_small,
            Channel::Shaking => flag(face.shaking),
            Channel::Hand(side) => layer.hands.get(side).copied().unwrap_or(0) as f32,
            Channel::Skin(i) => layer.skin.get(i).copied().map_or(0.0, f32::from),
        }
    }

    /// Gives it a value in a layer.
    pub fn set(self, layer: &mut Layer, start: &Face, value: f32) {
        let whole = value.round() as i32;
        if let Channel::Hand(side) = self {
            if let Some(h) = layer.hands.get_mut(side) {
                *h = whole;
            }
            return;
        }
        if let Channel::Skin(i) = self {
            if let Some(l) = layer.skin.get_mut(i) {
                *l = whole.clamp(0, 9) as u8;
            }
            return;
        }
        let face = layer.face.get_or_insert(*start);
        match self {
            Channel::Eyebrows => face.eyebrows = whole,
            Channel::Eyes => face.eyes = whole,
            Channel::Mouth => face.mouth = whole,
            Channel::EyesOpen => face.eyes_open = value,
            Channel::Blink => face.blink = whole != 0,
            Channel::MouthOpen => face.mouth_open = value,
            Channel::Tears => face.tears = whole.clamp(0, 3) as u8,
            Channel::Blush => face.blush = value,
            Channel::Highlights => face.hide_highlight = whole == 0,
            Channel::EyeRoll => face.eye_y = value,
            Channel::SmallIrises => face.eye_small = value,
            Channel::Shaking => face.shaking = whole != 0,
            Channel::Hand(_) | Channel::Skin(_) => {}
        }
    }

    /// Every value that takes keys, the face first, then the hands and the
    /// skin effects.
    pub fn all() -> Vec<Channel> {
        let mut all = FACE.to_vec();
        all.extend([Channel::Hand(0), Channel::Hand(1)]);
        all.extend((0..crate::skin_effects::EFFECTS.len()).map(Channel::Skin));
        all
    }
}

/// A value as a key holds it.
pub fn as_edit(value: f32) -> BoneEdit {
    BoneEdit {
        pos: Vec3::new(value, 0.0, 0.0),
        ..BoneEdit::default()
    }
}

/// The value a key holds.
pub fn of_edit(edit: BoneEdit) -> f32 {
    edit.pos.x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_channel_round_trips_its_name_and_value() {
        let start = Face::default();
        for channel in Channel::all() {
            assert_eq!(Channel::of_key(&channel.key()), Some(channel), "{}", channel.key());
            let mut layer = Layer::default();
            let value = if channel.held() { 1.0 } else { 0.25 };
            channel.set(&mut layer, &start, value);
            assert_eq!(channel.get(&layer, &start), value, "{}", channel.key());
            assert_eq!(of_edit(as_edit(value)), value);
        }
        assert_eq!(Channel::of_key("cf_j_neck"), None);
        assert_eq!(Channel::of_key("ik/LeftHand"), None);
    }

    #[test]
    fn keyed_patterns_hold_until_the_next_key_and_amounts_ease() {
        use crate::keys::{pose_at, set, Keys};
        let mut keys = Keys::new();
        set(&mut keys, &Channel::Mouth.key(), 0.0, as_edit(0.0), 0.01);
        set(&mut keys, &Channel::Mouth.key(), 2.0, as_edit(10.0), 0.01);
        set(&mut keys, &Channel::Blush.key(), 0.0, as_edit(0.0), 0.01);
        set(&mut keys, &Channel::Blush.key(), 2.0, as_edit(1.0), 0.01);
        set(&mut keys, &Channel::Hand(0).key(), 1.0, as_edit(3.0), 0.01);
        let start = Face::default();
        let at = |time: f32| {
            let mut layer = Layer::default();
            layer.with_keys_from(pose_at(&keys, time), &start);
            layer
        };
        let half = at(1.0);
        assert_eq!(half.face.unwrap().mouth, 0, "a pattern holds until its next key");
        assert!((half.face.unwrap().blush - 0.5).abs() < 1e-4, "an amount eases");
        assert_eq!(half.hands, [3, 0]);
        assert_eq!(at(2.0).face.unwrap().mouth, 10);
        assert_eq!(at(0.5).hands, [3, 0], "before its first key a value holds that key");
    }

    #[test]
    fn patterns_hold_and_amounts_ease() {
        assert!(Channel::Mouth.held() && Channel::Skin(3).held() && Channel::Blink.held());
        assert!(!Channel::MouthOpen.held() && !Channel::Blush.held());
    }
}
