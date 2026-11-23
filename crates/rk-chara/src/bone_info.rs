// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The bones CharaStudio poses by hand (FK), as its `Info.LoadBoneInfo`
//! reads them and `AddObjectAssist.InitBone` finds them on a character.
//!
//! Every bundle under `studio/info/`, in sorted order, may hold a sheet
//! `Bone_<bundle name>`. Its rows after the first are a number, the bone's
//! object name, a name to show, a group and a level; a later row with the
//! same number replaces an earlier one. The group says which of the seven
//! FK parts the bone belongs to; level 2 bones are left out on women.

use crate::animations::{sheet_rows, Rows};
use crate::scene::Scene;
use rk_unityfs::class;
use rk_vfs::Game;
use std::collections::BTreeMap;

/// One bone of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoneInfo {
    pub no: i32,
    /// The object's name in the character.
    pub bone: String,
    /// The name CharaStudio shows.
    pub name: String,
    /// The sheet's group number, 0 to 13.
    pub group: i32,
    pub level: i32,
}

impl BoneInfo {
    /// The bone's group flags (`OIBoneInfo.BoneGroup`), as `InitBone` sets
    /// them: the legs and arms are body bones too.
    pub fn flags(&self) -> u16 {
        match self.group {
            0..=4 => (1 << self.group) | flags::BODY,
            7..=9 => flags::HAIR,
            10 => flags::NECK,
            11 | 12 => flags::BREAST,
            13 => flags::SKIRT,
            g @ 0..=15 => 1 << g,
            _ => 0,
        }
    }

    /// Whether the bone hangs below the hair's parent rather than the body.
    pub fn in_hair(&self) -> bool {
        (7..=9).contains(&self.group)
    }

    /// The FK parts that switch the bone on, in `FKCtrl.parts` order.
    pub fn parts(&self) -> Vec<Part> {
        Part::ALL
            .into_iter()
            .filter(|p| self.flags() & p.flags() != 0)
            .collect()
    }
}

/// `OIBoneInfo.BoneGroup`.
pub mod flags {
    pub const BODY: u16 = 1;
    pub const RIGHT_LEG: u16 = 2;
    pub const LEFT_LEG: u16 = 4;
    pub const RIGHT_ARM: u16 = 8;
    pub const LEFT_ARM: u16 = 0x10;
    pub const RIGHT_HAND: u16 = 0x20;
    pub const LEFT_HAND: u16 = 0x40;
    pub const HAIR: u16 = 0x80;
    pub const NECK: u16 = 0x100;
    pub const BREAST: u16 = 0x200;
    pub const SKIRT: u16 = 0x400;
}

/// The seven FK parts CharaStudio switches on and off, in its order
/// (`FKCtrl.parts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Hair,
    Neck,
    Breast,
    Body,
    RightHand,
    LeftHand,
    Skirt,
}

impl Part {
    pub const ALL: [Part; 7] = [
        Part::Hair,
        Part::Neck,
        Part::Breast,
        Part::Body,
        Part::RightHand,
        Part::LeftHand,
        Part::Skirt,
    ];

    /// The parts FK starts with switched on, as CharaStudio's
    /// `activeFK`: the neck and the body.
    pub const START: [Part; 2] = [Part::Neck, Part::Body];

    pub fn flags(self) -> u16 {
        match self {
            Part::Hair => flags::HAIR,
            Part::Neck => flags::NECK,
            Part::Breast => flags::BREAST,
            Part::Body => flags::BODY,
            Part::RightHand => flags::RIGHT_HAND,
            Part::LeftHand => flags::LEFT_HAND,
            Part::Skirt => flags::SKIRT,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Part::Hair => "Hair",
            Part::Neck => "Neck",
            Part::Breast => "Breast",
            Part::Body => "Body",
            Part::RightHand => "Right hand",
            Part::LeftHand => "Left hand",
            Part::Skirt => "Skirt",
        }
    }
}

/// Adds the rows of one `Bone_` sheet, as `LoadBoneInfo` does: a row whose
/// number does not parse or whose bone is empty is skipped, a later row
/// replaces an earlier one of the same number.
pub fn add_sheet(rows: &Rows, table: &mut BTreeMap<i32, BoneInfo>) {
    for row in rows.iter().skip(1) {
        let cell = |i: usize| row.get(i).map_or("", |c| c.as_str());
        let Ok(no) = cell(0).trim().parse::<i32>() else {
            continue;
        };
        let bone = cell(1);
        if bone.is_empty() {
            continue;
        }
        let (Ok(group), Ok(level)) = (cell(3).trim().parse(), cell(4).trim().parse()) else {
            continue;
        };
        table.insert(
            no,
            BoneInfo {
                no,
                bone: bone.to_owned(),
                name: cell(2).to_owned(),
                group,
                level,
            },
        );
    }
}

/// CharaStudio's table of FK bones from the game and the mods' bundles of
/// the same names, by number.
pub fn read(game: &Game) -> BTreeMap<i32, BoneInfo> {
    let mut paths: Vec<&String> = game
        .assets
        .index
        .bundles
        .keys()
        .filter(|p| p.starts_with("studio/info/"))
        .collect();
    paths.sort();
    let mut table = BTreeMap::new();
    for path in paths {
        let stem = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".unity3d");
        let wanted = format!("Bone_{stem}");
        // The game's bundle first, then the mods' of the same name.
        for bundle in game.assets.bundles(path).into_iter().rev() {
            for file in &bundle.files {
                for info in file.sf.objects_of(class::MONO_BEHAVIOUR) {
                    let Some(name) = file.sf.name(info) else { continue };
                    if !name.eq_ignore_ascii_case(&wanted) {
                        continue;
                    }
                    if let Ok(sheet) = file.sf.read(info) {
                        add_sheet(&sheet_rows(&sheet), &mut table);
                    }
                }
            }
        }
    }
    table
}

/// A bone of the table found on a character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FkBone {
    pub info: BoneInfo,
    /// Its node in the character's scene.
    pub node: usize,
}

/// The table's bones on one character, as `InitBone` finds them: hair bones
/// below `cf_J_FaceUp_ty` (the hair's parent), the others below `cf_j_root`
/// under the body's Animator, the first of a name in depth-first order;
/// level 2 bones are left out on women (`sex` 1). Bones not found are left
/// out, in table order.
pub fn on_character(
    table: &BTreeMap<i32, BoneInfo>,
    scene: &Scene,
    animator: usize,
    top: usize,
    female: bool,
) -> Vec<FkBone> {
    let body = scene.find(animator, "cf_j_root");
    let hair = scene.find(top, "cf_J_FaceUp_ty");
    table
        .values()
        .filter(|info| !(female && info.level == 2))
        .filter_map(|info| {
            let from = if info.in_hair() { hair } else { body }?;
            let node = scene.find(from, &info.bone)?;
            Some(FkBone {
                info: info.clone(),
                node,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[&str]) -> Vec<String> {
        cells.iter().map(|c| (*c).to_owned()).collect()
    }

    #[test]
    fn a_sheet_reads_as_load_bone_info_does() {
        let sheet = vec![
            row(&["No", "Bone", "Name", "Group", "Level"]),
            row(&["1", "cf_j_hips", "Hips", "0", "0"]),
            row(&["2", "", "nothing", "0", "0"]),
            row(&["x", "cf_j_x", "not a number", "0", "0"]),
            row(&["3", "cf_j_arm00_L", "Left arm", "4", "0"]),
        ];
        let mut table = BTreeMap::new();
        add_sheet(&sheet, &mut table);
        assert_eq!(table.keys().copied().collect::<Vec<_>>(), [1, 3]);
        // A later sheet replaces a bone of the same number.
        add_sheet(
            &vec![row(&["No"]), row(&["1", "cf_j_waist01", "Waist", "0", "1"])],
            &mut table,
        );
        assert_eq!(table[&1].bone, "cf_j_waist01");
    }

    #[test]
    fn groups_map_to_flags_and_parts_as_init_bone_does() {
        let info = |group| BoneInfo {
            no: 0,
            bone: String::new(),
            name: String::new(),
            group,
            level: 0,
        };
        assert_eq!(info(0).flags(), flags::BODY);
        assert_eq!(info(4).flags(), flags::LEFT_ARM | flags::BODY);
        assert_eq!(info(5).flags(), flags::RIGHT_HAND);
        assert_eq!(info(6).flags(), flags::LEFT_HAND);
        assert_eq!(info(8).flags(), flags::HAIR);
        assert_eq!(info(10).flags(), flags::NECK);
        assert_eq!(info(12).flags(), flags::BREAST);
        assert_eq!(info(13).flags(), flags::SKIRT);
        // Arms and legs switch on with the body.
        assert_eq!(info(2).parts(), [Part::Body]);
        assert_eq!(info(9).parts(), [Part::Hair]);
        assert!(info(8).in_hair() && !info(10).in_hair());
    }
}
