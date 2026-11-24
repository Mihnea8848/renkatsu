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
    #[serde(default, rename = "track", skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<Track>,
}

/// The keys of one object of the scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub object: ObjectId,
    /// Its name when the track was made, for whoever reads the file.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default)]
    pub keys: Keys,
}

impl Default for Sequence {
    fn default() -> Sequence {
        Sequence {
            format: FORMAT,
            length: ten(),
            fps: thirty(),
            tracks: Vec::new(),
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
                });
                self.tracks.len() - 1
            }
        };
        &mut self.tracks[at]
    }

    /// Drops the tracks left without keys.
    pub fn tidy(&mut self) {
        self.tracks.retain(|t| !t.keys.is_empty());
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

    #[test]
    fn times_land_on_frames_within_the_sequence() {
        let s = Sequence::default();
        assert!((s.snap(1.01) - 1.0).abs() < 1e-6);
        assert!((s.snap(1.02) - 31.0 / 30.0).abs() < 1e-6);
        assert_eq!(s.snap(-3.0), 0.0);
        assert_eq!(s.snap(99.0), 10.0);
    }
}
