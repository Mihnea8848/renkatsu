// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! A scene's sequence: what changes in it over time, in a file of its own
//! (`.rkseq`, TOML text) beside the scene, which names it. A character's
//! track holds the keys of her pose by hand (`rk_chara::keys`); maps'
//! objects, lights, cameras and effects get tracks of their own later.
//! The game's premade animations stay `.rkanim` files: a sequence keys
//! over them, never in them.

use crate::scene::ObjectId;
use anyhow::{Context, Result};
use rk_chara::keys::Keys;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The extension of a sequence file.
pub const EXTENSION: &str = "rkseq";
/// The version of the sequence files this build writes.
pub const FORMAT: u32 = 1;

fn ten() -> f32 {
    10.0
}

fn thirty() -> f32 {
    30.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sequence {
    pub format: u32,
    /// Seconds it lasts.
    #[serde(default = "ten")]
    pub length: f32,
    /// Frames a second, which keys are set on.
    #[serde(default = "thirty")]
    pub fps: f32,
    /// The frames playing stays between, when it loops a part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_range: Option<[i32; 2]>,
    #[serde(default, rename = "track", skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<Track>,
    /// Named frames, in order.
    #[serde(default, rename = "marker", skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
}

/// A named frame of the sequence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub frame: i32,
    pub name: String,
}

fn yes() -> bool {
    true
}

/// A character's premade clip sampled into keys of its own: while `on`
/// they pose her in the clip's place, each an edit over the bones as
/// loaded, and the keys by hand go over them as they go over the clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BakedClip {
    #[serde(default = "yes")]
    pub on: bool,
    #[serde(default)]
    pub keys: Keys,
}

/// The prefix of a baked bone's name where the two layers' keys are
/// told apart by name: `baked/cf_j_neck`.
pub const BAKED: &str = "baked/";

/// The keys of one object of the scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub object: ObjectId,
    /// Its name when the track was made, for whoever reads the file.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default)]
    pub keys: Keys,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baked: Option<BakedClip>,
}

impl Track {
    /// The keys a name is of: the baked clip's for `baked/BONE`, else the
    /// keys by hand, with the name as the keys know it.
    pub fn list(&self, name: &str) -> Option<&Vec<rk_chara::keys::Key>> {
        match name.strip_prefix(BAKED) {
            Some(bone) => self.baked.as_ref()?.keys.get(bone),
            None => self.keys.get(name),
        }
    }

    /// As `list`, to change the keys; a name without keys gets a list.
    pub fn list_mut(&mut self, name: &str) -> Option<&mut Vec<rk_chara::keys::Key>> {
        match name.strip_prefix(BAKED) {
            Some(bone) => Some(self.baked.as_mut()?.keys.entry(bone.to_owned()).or_default()),
            None => Some(self.keys.entry(name.to_owned()).or_default()),
        }
    }

    /// The baked clip's keys while they pose the character.
    pub fn baked_on(&self) -> Option<&Keys> {
        self.baked.as_ref().filter(|b| b.on).map(|b| &b.keys)
    }
}

impl Default for Sequence {
    fn default() -> Sequence {
        Sequence {
            format: FORMAT,
            length: ten(),
            fps: thirty(),
            loop_range: None,
            tracks: Vec::new(),
            markers: Vec::new(),
        }
    }
}

impl Sequence {
    /// The track of an object, when it has one.
    pub fn track(&self, object: ObjectId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.object == object)
    }

    /// The track of an object, made when it has none yet.
    pub fn track_mut(&mut self, object: ObjectId, name: &str) -> &mut Track {
        let at = match self.tracks.iter().position(|t| t.object == object) {
            Some(at) => at,
            None => {
                self.tracks.push(Track {
                    object,
                    name: name.to_owned(),
                    keys: Keys::new(),
                    baked: None,
                });
                self.tracks.len() - 1
            }
        };
        &mut self.tracks[at]
    }

    /// Drops the tracks left without keys.
    pub fn tidy(&mut self) {
        for track in &mut self.tracks {
            track.keys.retain(|_, list| !list.is_empty());
            if let Some(baked) = &mut track.baked {
                baked.keys.retain(|_, list| !list.is_empty());
            }
        }
        self.tracks.retain(|t| !t.keys.is_empty() || t.baked.is_some());
    }

    /// `seconds` on the nearest frame, within the sequence.
    pub fn snap(&self, seconds: f32) -> f32 {
        let fps = self.fps.max(1.0);
        ((seconds * fps).round() / fps).clamp(0.0, self.length.max(0.0))
    }

    /// Half a frame: keys closer than this are at the same time.
    pub fn near(&self) -> f32 {
        0.5 / self.fps.max(1.0)
    }

    pub fn read(path: &Path) -> Result<Sequence> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let sequence: Sequence = toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
        anyhow::ensure!(
            sequence.format <= FORMAT,
            "{} was saved by a newer Renkatsu",
            path.display()
        );
        Ok(sequence)
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("writing the sequence")?;
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
    }
}

/// The extension of a character's keys saved as an animation of the
/// project, to give to any character.
pub const KEYS_EXTENSION: &str = "rkkeys";

/// A character's keys as a file of their own, in the project's
/// Animations folder beside the game's premade animations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeysFile {
    pub format: u32,
    #[serde(default = "ten")]
    pub length: f32,
    #[serde(default = "thirty")]
    pub fps: f32,
    #[serde(default)]
    pub keys: Keys,
}

impl KeysFile {
    pub fn read(path: &Path) -> Result<KeysFile> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let file: KeysFile = toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
        anyhow::ensure!(
            file.format <= FORMAT,
            "{} was saved by a newer Renkatsu",
            path.display()
        );
        Ok(file)
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("writing the keys")?;
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
    }
}

/// Whether a file of the project is a character's keys.
pub fn is_keys(rel: &str) -> bool {
    rel.to_lowercase().ends_with(&format!(".{KEYS_EXTENSION}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_chara::pose::BoneEdit;

    #[test]
    fn a_sequence_is_written_and_read_back() {
        let mut s = Sequence::default();
        let edit = BoneEdit {
            rot: glam::Quat::from_rotation_x(0.5),
            ..BoneEdit::default()
        };
        let near = s.near();
        rk_chara::keys::set(
            &mut s.track_mut(ObjectId(7), "Renkatsu-Chan").keys,
            "cf_j_neck",
            1.0,
            edit,
            near,
        );
        s.track_mut(ObjectId(9), "Nobody");
        s.tidy();
        assert_eq!(s.tracks.len(), 1, "a track without keys goes");
        let dir = std::env::temp_dir().join(format!("renkatsu-sequence-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Main.rkseq");
        s.write(&path).unwrap();
        assert_eq!(Sequence::read(&path).unwrap(), s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A sequence as Renkatsu 0.3.0 wrote it, before the eases grew and
    /// the markers, the loop range and the baked clip came.
    const OLD: &str = r#"format = 1
length = 4.0
fps = 30.0

[[track]]
object = 75
name = "Rinka"

[track.keys]
cf_j_arm00_L = [{ time = 1.0, rot = [0.0, 0.0, 0.1736, 0.9848] }, { time = 3.0, rot = [0.0, 0.0, -0.1736, 0.9848], ease = "linear" }]
cf_j_neck = [{ time = 0.0 }, { time = 2.0, rot = [0.0, 0.2588, 0.0, 0.9659], ease = "step" }, { time = 4.0, pos = [0.0, 0.1, 0.0] }]
"#;

    #[test]
    fn an_older_sequence_reads_as_it_was_and_is_written_the_same() {
        use rk_chara::keys::{Ease, Key};
        let read: Sequence = toml::from_str(OLD).unwrap();
        assert_eq!((read.format, read.length, read.fps), (1, 4.0, 30.0));
        assert_eq!((read.loop_range, read.markers.len()), (None, 0));
        let track = &read.tracks[0];
        assert_eq!((track.object, track.name.as_str()), (ObjectId(75), "Rinka"));
        assert!(track.baked.is_none());
        let turned = |x, y, z, w| BoneEdit {
            rot: glam::Quat::from_xyzw(x, y, z, w),
            ..BoneEdit::default()
        };
        let key = |time, edit, ease| Key {
            time,
            edit,
            ease,
            handles: None,
        };
        assert_eq!(
            track.keys["cf_j_arm00_L"],
            [
                key(1.0, turned(0.0, 0.0, 0.1736, 0.9848), Ease::Smooth),
                key(3.0, turned(0.0, 0.0, -0.1736, 0.9848), Ease::Linear),
            ]
        );
        let lifted = BoneEdit {
            pos: glam::Vec3::new(0.0, 0.1, 0.0),
            ..BoneEdit::default()
        };
        assert_eq!(
            track.keys["cf_j_neck"],
            [
                key(0.0, BoneEdit::default(), Ease::Smooth),
                key(2.0, turned(0.0, 0.2588, 0.0, 0.9659), Ease::Step),
                key(4.0, lifted, Ease::Smooth),
            ]
        );
        // Written again it says nothing an older Renkatsu would not know.
        let written = toml::to_string_pretty(&read).unwrap();
        for new in ["handles", "marker", "loop_range", "baked", "ease_in", "bezier"] {
            assert!(!written.contains(new), "{new} in {written}");
        }
        assert_eq!(toml::from_str::<Sequence>(&written).unwrap(), read);
    }

    #[test]
    fn the_new_eases_handles_markers_range_and_baked_clip_come_back() {
        use rk_chara::keys::{Ease, Key};
        let mut s = Sequence {
            length: 4.0,
            loop_range: Some([15, 105]),
            ..Sequence::default()
        };
        s.markers.push(Marker {
            frame: 30,
            name: "Lean".to_owned(),
        });
        let turn = BoneEdit {
            rot: glam::Quat::from_rotation_y(0.4),
            ..BoneEdit::default()
        };
        let track = s.track_mut(ObjectId(7), "Rinka");
        track.keys.insert(
            "cf_j_head".to_owned(),
            [Ease::EaseIn, Ease::EaseOut, Ease::Bezier, Ease::Bezier]
                .into_iter()
                .enumerate()
                .map(|(i, ease)| Key {
                    time: i as f32,
                    edit: turn,
                    ease,
                    handles: (i == 3).then_some([0.2, 0.0, 0.5, 1.4]),
                })
                .collect(),
        );
        track.baked = Some(BakedClip {
            on: false,
            keys: Keys::from([("cf_j_hips".to_owned(), vec![Key::new(0.0, turn)])]),
        });
        let written = toml::to_string_pretty(&s).unwrap();
        for new in [
            "ease_in",
            "ease_out",
            "bezier",
            "handles",
            "loop_range",
            "Lean",
            "baked",
        ] {
            assert!(written.contains(new), "{new} not in {written}");
        }
        let read: Sequence = toml::from_str(&written).unwrap();
        assert_eq!(read, s);
        assert_eq!(read.tracks[0].list("baked/cf_j_hips").map(Vec::len), Some(1));
        assert!(read.tracks[0].baked_on().is_none(), "switched off");
        // A track left with only its baked clip stays.
        let mut only = read.clone();
        only.tracks[0].keys.clear();
        only.tidy();
        assert_eq!(only.tracks.len(), 1);
    }

    #[test]
    fn times_land_on_frames_within_the_sequence() {
        let s = Sequence::default();
        assert!((s.snap(1.01) - 1.0).abs() < 1e-6);
        assert!((s.snap(1.02) - 31.0 / 30.0).abs() < 1e-6);
        assert_eq!(s.snap(-3.0), 0.0);
        assert_eq!(s.snap(99.0), 10.0);
    }
}
