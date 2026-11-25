// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Where the game's own H scenes aim the penis, for each position and each
//! state of it (`Lookat_dan`). Every row of the `h/list/AnimationInfo_NN`
//! tables, one table per H mode, names the male's controller and a
//! look-at file (`fileLookAt`); that file gives, state by state, the
//! female's bone the shaft looks at: `k_f_kokan_00` for her vagina,
//! `k_f_ana_00` for her anus, `k_f_head_00` for her mouth, another bone
//! (her chest in a titjob) or none. BetterPenetration's main-game half
//! follows the same names. Renkatsu links a shaft by them, so a pair
//! playing an H position links on its own.
//!
//! Positions AnimationLoader adds borrow the file of their donor; mods that
//! ship rows and files of their own in `h/list/` join the game's, as
//! `GlobalMethod.LoadAllListText` joins every bundle's copy of a table.

use crate::better_penetration::Mode;
use rk_unityfs::{class, UObject};
use rk_vfs::Game;
use std::collections::HashMap;

/// A table's text from every bundle under `folder` that has one, in the
/// bundles' order (`GlobalMethod.LoadAllListText`).
pub fn list_text(game: &Game, folder: &str, file: &str) -> String {
    let folder = rk_vfs::index::norm(folder);
    let mut paths: Vec<&String> = game
        .assets
        .index
        .bundles
        .keys()
        .filter(|p| p.starts_with(folder.trim_end_matches('/')) && p.ends_with(".unity3d"))
        .collect();
    paths.sort();
    let mut out = String::new();
    for path in paths {
        // The bundle a mod replaces is read as the mod has it.
        let Some(bundle) = game.assets.bundles(path).into_iter().next() else {
            continue;
        };
        for f in &bundle.files {
            for info in f.sf.objects_of(class::TEXT_ASSET) {
                if f.sf.name(info).is_some_and(|n| n.eq_ignore_ascii_case(file)) {
                    let text: Option<UObject> = f.sf.read(info).ok();
                    if let Some(t) = text.as_ref().and_then(|o| o.str("m_Script")) {
                        out.push_str(t);
                        if !out.ends_with('\n') {
                            out.push('\n');
                        }
                    }
                }
            }
        }
    }
    out
}

/// The cells of a table's rows, tab-separated (`GlobalMethod.GetListString`).
fn rows(text: &str) -> impl Iterator<Item = Vec<&str>> {
    text.split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split('\t').collect())
}

/// What a look-at bone means for the shaft.
pub fn mode_of(look_at: &str) -> Option<Mode> {
    match look_at {
        "k_f_kokan_00" => Some(Mode::Vaginal),
        "k_f_ana_00" => Some(Mode::Anal),
        "k_f_head_00" => Some(Mode::Oral),
        _ => None,
    }
}

/// The aims of every H position, by the male's controller.
#[derive(Debug, Default, Clone)]
pub struct Aims {
    /// Male controller (lowercase) to its look-at file (lowercase).
    files: HashMap<String, String>,
    /// Look-at file to each state's look-at bone, empty for none.
    states: HashMap<String, HashMap<String, String>>,
}

/// The `AnimationInfo` columns read here: the position's id, the male's
/// own controller and its look-at file (`HSceneProc.CreateAllAnimationList`).
const ID: usize = 1;
const MALE_CONTROLLER: usize = 15;
const LOOK_AT: usize = 19;

impl Aims {
    pub fn read(game: &Game) -> Aims {
        let added = rk_vfs::animation_loader::load(&game.assets.index);
        Self::from_tables(
            |file| list_text(game, "h/list/", file),
            added.iter().map(|p| (p.mode, p.controller_male.as_str(), p.donor)),
        )
    }

    /// From the tables as `text` gives them by name, and the positions
    /// AnimationLoader adds as (mode, male controller, donor id).
    pub fn from_tables<'a>(
        text: impl Fn(&str) -> String,
        added: impl IntoIterator<Item = (i32, &'a str, i32)>,
    ) -> Aims {
        let mut aims = Aims::default();
        // Each mode's positions by id, for the donors.
        let mut by_id: HashMap<(i32, i32), String> = HashMap::new();
        for mode in 0..10 {
            let table = text(&format!("AnimationInfo_{mode:02}"));
            for row in rows(&table) {
                let cell = |i: usize| row.get(i).map(|c| c.trim()).unwrap_or_default();
                let (controller, file) = (cell(MALE_CONTROLLER), cell(LOOK_AT));
                if let Ok(id) = cell(ID).parse::<i32>() {
                    by_id.insert((mode, id), file.to_ascii_lowercase());
                }
                if !controller.is_empty() && !file.is_empty() {
                    aims.files
                        .insert(controller.to_ascii_lowercase(), file.to_ascii_lowercase());
                }
            }
        }
        for (mode, controller, donor) in added {
            if let Some(file) = by_id.get(&(mode, donor)).filter(|f| !f.is_empty()) {
                aims.files
                    .entry(controller.to_ascii_lowercase())
                    .or_insert_with(|| file.clone());
            }
        }
        let files: Vec<String> = aims.files.values().cloned().collect();
        for file in files {
            if aims.states.contains_key(&file) {
                continue;
            }
            // Motion, female, look-at bone, ... (`Lookat_dan.LoadList`).
            let table = text(&file);
            let states = rows(&table)
                .filter_map(|row| Some((row.first()?.trim().to_owned(), row.get(2)?.trim().to_owned())))
                .collect();
            aims.states.insert(file, states);
        }
        aims
    }

    /// Where the position whose male plays `controller` aims at `state`:
    /// `None` when the game does not know the position, `Some(None)` when
    /// it knows it and the shaft is not in her there.
    pub fn aim(&self, controller: &str, state: &str) -> Option<Option<Mode>> {
        let file = self.files.get(&controller.to_ascii_lowercase())?;
        let bone = self.states.get(file)?.get(state);
        Some(bone.and_then(|b| mode_of(b)))
    }

    /// How many positions it knows.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables(name: &str) -> String {
        let row = |id: &str, controller: &str, look: &str| {
            let mut cells = vec![""; 30];
            cells[0] = "name";
            cells[ID] = id;
            cells[MALE_CONTROLLER] = controller;
            cells[LOOK_AT] = look;
            cells.join("\t")
        };
        match name {
            "AnimationInfo_01" => [row("0", "khh_m_00", ""), row("7", "khh_m_07", "dan_khh_07")].join("\r\n"),
            "AnimationInfo_02" => [row("0", "khs_m_00", "dan_khs_00"), row("2", "khs_m_02", "dan_khs_02")].join("\r\n"),
            "dan_khh_07" => "Idle\t0\t\t0\r\nWLoop\t0\tk_f_head_00\t1\t0\r\n".into(),
            "dan_khs_00" => "Idle\t0\t\t0\r\nWLoop\t0\tk_f_kokan_00\t1\t0\r\nPull\t0\t\t0\r\n".into(),
            "dan_khs_02" => "A_WLoop\t0\tk_f_ana_00\t1\t0\r\nWLoop\t0\tk_f_kokan_00\t1\t0\r\n".into(),
            _ => String::new(),
        }
    }

    #[test]
    fn each_state_aims_where_the_games_file_says() {
        let aims = Aims::from_tables(tables, [(2, "khs_m_n98", 2), (1, "khh_m_x", 0)]);
        assert_eq!(aims.aim("khs_m_00", "WLoop"), Some(Some(Mode::Vaginal)));
        assert_eq!(aims.aim("KHS_M_00", "Idle"), Some(None), "out of her while idle");
        assert_eq!(aims.aim("khs_m_00", "Pull"), Some(None));
        assert_eq!(aims.aim("khs_m_02", "A_WLoop"), Some(Some(Mode::Anal)));
        assert_eq!(aims.aim("khh_m_07", "WLoop"), Some(Some(Mode::Oral)));
        assert_eq!(aims.aim("khh_m_00", "WLoop"), None, "a handjob has no look-at file");
        // AnimationLoader's positions take their donor's file.
        assert_eq!(aims.aim("khs_m_n98", "A_WLoop"), Some(Some(Mode::Anal)));
        assert_eq!(aims.aim("khh_m_x", "WLoop"), None, "its donor has no file");
        assert_eq!(aims.aim("mixamo", "Take 001"), None);
    }

    #[test]
    fn the_games_positions_read() {
        let Ok(game) = Game::open_user(&[]) else {
            return;
        };
        let aims = Aims::read(&game);
        if aims.is_empty() {
            eprintln!("skipped: no H lists");
            return;
        }
        eprintln!("{} positions aim", aims.len());
        assert_eq!(aims.aim("khs_m_00", "WLoop"), Some(Some(Mode::Vaginal)));
        assert_eq!(aims.aim("khs_m_02", "A_WLoop"), Some(Some(Mode::Anal)));
        assert_eq!(aims.aim("khh_m_07", "WLoop"), Some(Some(Mode::Oral)));
        assert_eq!(aims.aim("khh_m_11", "WLoop"), Some(None), "a titjob aims at her chest");
        // CharaStudio's own H entries name the same controllers.
        let library = crate::animations::Library::read(&game);
        let mut known = 0;
        let mut h = 0;
        for e in library.entries.iter().filter(|e| e.h && e.group % 2 == 1) {
            h += 1;
            if aims.aim(&e.override_controller, &e.state).is_some() {
                known += 1;
            }
        }
        eprintln!("{known} of {h} male H entries known");
        let bj = library.find(3, 116, 1).expect("Studio's one-handed blowjob, his half");
        assert_eq!(
            aims.aim(&bj.override_controller, &bj.state),
            Some(Some(Mode::Oral)),
            "{bj:?}"
        );
        let doggy = library.find(5, 145, 3).expect("Studio's doggy-style anal, his half");
        assert_eq!(
            aims.aim(&doggy.override_controller, &doggy.state),
            Some(Some(Mode::Anal)),
            "{doggy:?}"
        );
    }
}
