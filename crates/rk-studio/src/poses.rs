// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Poses saved by hand: `.rkpose` files in the project's Poses folder, one
//! character's pose layer and the animation it lies over, if any, at the
//! point it was held. A pose is for a character alone; what a scene does
//! over time is its sequence's.
//!
//! Renkatsu 0.1.4 gave its links to animations the same extension and
//! kept them in the same folder: a pose says its `format`, which a link
//! never has.

use crate::project::{self, Project};
use crate::scene::Animation;
use anyhow::{Context, Result};
use rk_chara::pose::Layer;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The extension of a pose file.
pub const EXTENSION: &str = "rkpose";
/// The version of the pose files this build writes.
pub const FORMAT: u32 = 1;

/// A pose file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pose {
    pub format: u32,
    /// The animation the pose lies over, starting where it was held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Animation>,
    #[serde(default)]
    pub layer: Layer,
}

impl Pose {
    pub fn new(base: Option<Animation>, layer: Layer) -> Pose {
        Pose {
            format: FORMAT,
            base,
            layer,
        }
    }
}

/// Whether a file's text is a pose rather than an old link to an
/// animation.
pub fn is_pose_text(text: &str) -> bool {
    text.parse::<toml::Table>().is_ok_and(|t| t.contains_key("format"))
}

/// Whether `path` is a pose file.
pub fn is_pose_file(path: &Path) -> bool {
    path.extension().is_some_and(|x| x.eq_ignore_ascii_case(EXTENSION))
        && std::fs::read_to_string(path).is_ok_and(|t| is_pose_text(&t))
}

pub fn read(path: &Path) -> Result<Pose> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let pose: Pose = toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
    anyhow::ensure!(
        pose.format <= FORMAT,
        "{} was saved by a newer Renkatsu",
        path.display()
    );
    Ok(pose)
}

pub fn write(path: &Path, pose: &Pose) -> Result<()> {
    let text = toml::to_string_pretty(pose).context("writing the pose")?;
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// The project's poses, at any depth of its Poses folder, by their path
/// relative to the project, in order.
pub fn scan(project: &Project) -> Vec<String> {
    let mut files = Vec::new();
    project::files_below(&project.folder(project::POSES), &mut files);
    let mut found: Vec<String> = files
        .into_iter()
        .filter(|p| is_pose_file(p))
        .filter_map(|p| project::relative_to(&project.root, &p))
        .collect();
    found.sort_by_key(|r| r.to_lowercase());
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_chara::pose::BoneEdit;

    #[test]
    fn a_pose_is_written_and_read_back() {
        let mut layer = Layer {
            fk_on: true,
            ..Layer::default()
        };
        layer.fk.insert(
            "cf_j_neck".into(),
            BoneEdit {
                rot: glam::Quat::from_rotation_y(0.4),
                ..BoneEdit::default()
            },
        );
        let base = Animation {
            start: 0.25,
            ..Animation::maker_pose()
        };
        let pose = Pose::new(Some(base), layer);
        let dir = std::env::temp_dir().join(format!("renkatsu-poses-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Waving.rkpose");
        write(&path, &pose).unwrap();
        assert_eq!(read(&path).unwrap(), pose);
        assert!(is_pose_file(&path));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_old_link_to_an_animation_is_not_a_pose() {
        let link = toml::to_string(&Animation::maker_pose()).unwrap();
        assert!(!is_pose_text(&link));
        assert!(is_pose_text(
            &toml::to_string(&Pose::new(None, Layer::default())).unwrap()
        ));
    }
}
