// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The H positions AnimationLoader adds to the main game, from the
//! `<AnimationLoader>` element of zipmod manifests. Each names the male's
//! and female's controllers and the H mode, and a donor position of that
//! mode whose data it borrows (`DonorPoseId`): the look-at file, the hit
//! files and the rest. What each zipmod holds is remembered in a cache file
//! of its own, keyed by path, size and time, as the uncensors are.

use crate::index::{cache_file, stamp, FileStamp, Index};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;

/// One added position.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    /// The H mode, as `HFlag.EMode` numbers them (houshi 1, sonyu 2, ...).
    pub mode: i32,
    pub controller_male: String,
    pub controller_female: String,
    /// The vanilla position of the same mode it borrows its data from.
    pub donor: i32,
}

/// `HFlag.EMode` by its name.
fn mode_number(name: &str) -> Option<i32> {
    const MODES: [&str; 10] = [
        "aibu",
        "houshi",
        "sonyu",
        "masturbation",
        "peeping",
        "lesbian",
        "houshi3p",
        "sonyu3p",
        "houshi3pmmf",
        "sonyu3pmmf",
    ];
    let name = name.trim().to_ascii_lowercase();
    MODES.iter().position(|m| *m == name).map(|i| i as i32)
}

/// The positions in one manifest.
pub fn parse(manifest: &str) -> Vec<Position> {
    let Ok(doc) = roxmltree::Document::parse(manifest) else {
        return Vec::new();
    };
    let text = |node: roxmltree::Node, name: &str| {
        node.children()
            .find(|c| c.tag_name().name().eq_ignore_ascii_case(name))
            .and_then(|c| c.text())
            .map(|t| t.trim().to_owned())
            .unwrap_or_default()
    };
    doc.descendants()
        .filter(|n| n.tag_name().name() == "AnimationLoader")
        .flat_map(|al| al.descendants().filter(|n| n.tag_name().name() == "Animation"))
        .filter_map(|a| {
            Some(Position {
                mode: mode_number(&text(a, "Mode"))?,
                controller_male: text(a, "ControllerMale"),
                controller_female: text(a, "ControllerFemale"),
                donor: text(a, "DonorPoseId").parse().ok()?,
            })
        })
        .collect()
}

/// Every position the mods in use add.
pub fn load(index: &Index) -> Vec<Position> {
    #[derive(Default, Serialize, Deserialize)]
    struct Cache {
        version: u32,
        zips: HashMap<String, (FileStamp, Vec<Position>)>,
    }
    const VERSION: u32 = 1;
    let file = cache_file(&index.game_dir, "animation_loader");
    let mut cache: Cache = std::fs::read(&file)
        .ok()
        .and_then(|b| rmp_serde::from_slice(&b).ok())
        .filter(|c: &Cache| c.version == VERSION)
        .unwrap_or_default();
    cache.version = VERSION;
    let stamps: Vec<FileStamp> = index
        .mods
        .iter()
        .map(|m| stamp(&index.game_dir.join(&m.path)))
        .collect();
    let todo: Vec<usize> = (0..index.mods.len())
        .filter(|&i| cache.zips.get(&index.mods[i].path).is_none_or(|(s, _)| *s != stamps[i]))
        .collect();
    let scanned: Vec<(usize, Vec<Position>)> = todo
        .par_iter()
        .map(|&i| {
            let read = || -> Option<String> {
                let f = std::fs::File::open(index.game_dir.join(&index.mods[i].path)).ok()?;
                let mut zip = zip::ZipArchive::new(std::io::BufReader::new(f)).ok()?;
                let mut manifest = String::new();
                zip.by_name("manifest.xml").ok()?.read_to_string(&mut manifest).ok()?;
                Some(manifest)
            };
            (i, read().map(|m| parse(&m)).unwrap_or_default())
        })
        .collect();
    if !scanned.is_empty() {
        for (i, data) in scanned {
            cache.zips.insert(index.mods[i].path.clone(), (stamps[i].clone(), data));
        }
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        if let Ok(bytes) = rmp_serde::to_vec(&cache) {
            std::fs::write(&file, bytes).ok();
        }
    }
    index
        .mods
        .iter()
        .filter_map(|m| cache.zips.get(&m.path))
        .flat_map(|(_, p)| p.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_lists_its_positions_with_their_donors() {
        let manifest = r#"<manifest><guid>x</guid><AnimationLoader><kk>
            <Animation>
                <PathMale>h/anim/male/Leg_hold_M.unity3d</PathMale>
                <ControllerFemale>khs_f_n98</ControllerFemale>
                <ControllerMale>khs_m_n98</ControllerMale>
                <Mode>sonyu</Mode>
                <DonorPoseId>28</DonorPoseId>
            </Animation>
            <Animation><ControllerMale>khh_m_x</ControllerMale><Mode>houshi</Mode><DonorPoseId>7</DonorPoseId></Animation>
            <Animation><ControllerMale>bad</ControllerMale><Mode>nothing</Mode><DonorPoseId>1</DonorPoseId></Animation>
        </kk></AnimationLoader></manifest>"#;
        let p = parse(manifest);
        assert_eq!(p.len(), 2, "an unknown mode is skipped");
        assert_eq!(
            (p[0].mode, p[0].donor, p[0].controller_male.as_str()),
            (2, 28, "khs_m_n98")
        );
        assert_eq!((p[1].mode, p[1].donor), (1, 7));
        assert!(parse("not xml").is_empty());
    }
}
