// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! CharaStudio's hand patterns, as `Studio.HandAnimeCtrl` plays them.
//!
//! Each hand has an Animator of its own on `cf_s_hand_L` or `cf_s_hand_R`
//! that plays one clip over the fingers, after the body's animation. The
//! clips come from `HandAnime_<hand>_<n>` sheets in the `studio/info`
//! bundles (`Info.LoadHandAnimeInfo`): rows of a number, a name, a bundle,
//! a controller and a clip. Pattern 0 has no row, so the hand's Animator
//! is switched off and the fingers follow the animation.

use crate::animations::{sheet_rows, Rows};
use crate::motion::{Bound, Motion, Parameters, Skeleton};
use crate::scene::Scene;
use anyhow::{Context, Result};
use rk_unityfs::class;
use rk_vfs::Game;
use std::collections::BTreeMap;

/// The node each hand's Animator is on, left then right.
pub const HAND_NODES: [&str; 2] = ["cf_s_hand_L", "cf_s_hand_R"];

/// One row of a hand's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandAnime {
    pub no: i32,
    pub bundle: String,
    pub controller: String,
    /// The state `Animator.Play` starts, named as its clip.
    pub clip: String,
}

impl HandAnime {
    /// A name to show, from the clip's.
    pub fn label(&self) -> String {
        label(&self.clip).map_or_else(|| format!("Pattern {}", self.no), str::to_owned)
    }
}

/// The game's clips by the names they are given here.
pub fn label(clip: &str) -> Option<&'static str> {
    Some(match clip {
        "goo" => "Fist",
        "scissors" => "Peace",
        "par" => "Open",
        "grip_hard" => "Tight grip",
        "grip_normal" => "Grip",
        "grip_soft" => "Soft grip",
        "par_soft" => "Relaxed open",
        "par_grip" => "Half open",
        "gun_grip" => "Gun",
        "soft_grip" => "Loose fist",
        "dirty_grip" => "Claw",
        "natural_pose" => "Natural",
        "hug" => "Hug",
        "index_finger1" => "Point",
        "index_finger2" => "Point down",
        "thumb_finger" => "Thumb",
        "ok" => "OK",
        "soft_hold" => "Soft hold",
        "go" => "Five",
        "hard_hold" => "Hard hold",
        "par_straight" => "Flat",
        _ => return None,
    })
}

/// Both hands' lists, left then right, by number.
pub type Lists = [BTreeMap<i32, HandAnime>; 2];

/// Adds the rows of one sheet, as `LoadHandAnimeInfo` does: a row whose
/// number does not parse is skipped, a later row replaces an earlier one.
pub fn add_sheet(rows: &Rows, list: &mut BTreeMap<i32, HandAnime>) {
    for row in rows.iter().skip(1) {
        let cell = |i: usize| row.get(i).map_or("", |c| c.as_str());
        let Ok(no) = cell(0).trim().parse::<i32>() else {
            continue;
        };
        list.insert(
            no,
            HandAnime {
                no,
                bundle: cell(2).to_owned(),
                controller: cell(3).to_owned(),
                clip: cell(4).to_owned(),
            },
        );
    }
}

/// The hand and order of a sheet's name, `HandAnime_<hand>_<n>`, any case.
pub fn sheet_name(name: &str) -> Option<(usize, i32)> {
    let mut parts = name.split('_');
    if !parts.next()?.eq_ignore_ascii_case("handanime") {
        return None;
    }
    let hand = parts.next()?.parse::<usize>().ok()?;
    let order = parts.next()?.parse::<i32>().ok()?;
    (parts.next().is_none() && hand < 2).then_some((hand, order))
}

/// Both hands' lists from the game and the mods: every `studio/info`
/// bundle in sorted order, its sheets by hand and then by number.
pub fn read(game: &Game) -> Lists {
    let mut paths: Vec<&String> = game
        .assets
        .index
        .bundles
        .keys()
        .filter(|p| p.starts_with("studio/info/"))
        .collect();
    paths.sort();
    let mut lists: Lists = Default::default();
    for path in paths {
        let mut sheets: BTreeMap<(usize, i32), Vec<Rows>> = BTreeMap::new();
        // The game's bundle first, then the mods' of the same name.
        for bundle in game.assets.bundles(path).into_iter().rev() {
            for file in &bundle.files {
                for info in file.sf.objects_of(class::MONO_BEHAVIOUR) {
                    let Some(at) = file.sf.name(info).as_deref().and_then(sheet_name) else {
                        continue;
                    };
                    if let Ok(sheet) = file.sf.read(info) {
                        sheets.entry(at).or_default().push(sheet_rows(&sheet));
                    }
                }
            }
        }
        for ((hand, _), all) in sheets {
            for rows in all {
                add_sheet(&rows, &mut lists[hand]);
            }
        }
    }
    lists
}

/// A hand's pattern ready to play on one character.
#[derive(Debug, Clone)]
pub struct HandClip {
    pub pattern: i32,
    bound: Bound,
    duration: f32,
    loops: bool,
}

impl HandClip {
    /// Loads a pattern's clip from the game and binds it below the hand's
    /// node of `scene`.
    pub fn load(game: &Game, anime: &HandAnime, scene: &Scene, top: usize, hand: usize) -> Result<HandClip> {
        let motion = crate::motion::load(
            game,
            &anime.bundle,
            &anime.controller,
            &anime.clip,
            None,
            &Parameters::default(),
        )?;
        HandClip::bind(anime.no, &motion, scene, top, hand)
    }

    /// Binds a loaded clip below the hand's node.
    pub fn bind(pattern: i32, motion: &Motion, scene: &Scene, top: usize, hand: usize) -> Result<HandClip> {
        let node = scene
            .find(top, HAND_NODES[hand])
            .with_context(|| format!("the character has no {}", HAND_NODES[hand]))?;
        Ok(HandClip {
            pattern,
            bound: motion.bind(&Skeleton::of(scene, node)),
            duration: motion.duration(),
            loops: motion.loops(),
        })
    }

    /// Puts the hand in its pattern `seconds` after it started, as the
    /// hand's Animator plays it from its start.
    pub fn apply(&self, scene: &mut Scene, seconds: f32) {
        let passes = if self.duration > 0.0 {
            seconds / self.duration
        } else {
            0.0
        };
        let phase = if self.loops {
            passes.rem_euclid(1.0)
        } else {
            passes.clamp(0.0, 1.0)
        };
        self.bound.apply(scene, phase);
    }

    /// How many bone properties the pattern writes.
    pub fn tracks(&self) -> usize {
        self.bound.tracks()
    }

    /// Curves whose bone the hand does not have.
    pub fn unbound(&self) -> usize {
        self.bound.unbound
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[&str]) -> Vec<String> {
        cells.iter().map(|c| (*c).to_owned()).collect()
    }

    #[test]
    fn the_game_has_21_patterns_a_hand_that_bend_the_fingers() {
        let Ok(game) = rk_vfs::Game::open_user(&[]) else {
            eprintln!("skipped: no game");
            return;
        };
        let lists = read(&game);
        for (hand, side) in [(0, "L"), (1, "R")] {
            let list = &lists[hand];
            assert!(list.len() >= 21, "{} patterns", list.len());
            assert!(!list.contains_key(&0), "pattern 0 follows the animation");
            assert_eq!(list[&1].clip, "goo");
            assert_eq!(list[&1].controller, format!("cf_hand_{side}_00"));
            assert_eq!(list[&21].clip, "par_straight");
        }
        let Some(dir) = rk_core::Config::load().ok().map(|c| c.game_dir) else {
            return;
        };
        let Ok(card) = rk_card::Card::load(dir.join("UserData/chara/female/Simple.png")) else {
            eprintln!("skipped: no Simple.png");
            return;
        };
        let options = crate::BuildOptions {
            clothes: false,
            accessories: false,
            physics: false,
            ..Default::default()
        };
        let mut chara = crate::build(&game, &card, &options).expect("Simple builds");
        for hand in 0..2 {
            let finger = ["cf_j_index02_L", "cf_j_index02_R"][hand];
            let finger = chara.scene.find(chara.top, finger).expect("an index finger");
            let before = chara.scene.nodes[finger].local.rot;
            let fist = HandClip::load(&game, &lists[hand][&1], &chara.scene, chara.top, hand).expect("the fist loads");
            assert!(fist.tracks() >= 24, "the finger bones, turned and scaled");
            assert_eq!(fist.unbound(), 0, "every curve finds its bone below the hand");
            fist.apply(&mut chara.scene, 0.0);
            let after = chara.scene.nodes[finger].local.rot;
            assert!(before.angle_between(after) > 0.5, "a fist bends the index finger");
        }
    }

    #[test]
    fn sheet_names_give_the_hand_and_order() {
        assert_eq!(sheet_name("HandAnime_00_00"), Some((0, 0)));
        assert_eq!(sheet_name("handanime_01_03"), Some((1, 3)));
        assert_eq!(sheet_name("HandAnime_02_00"), None);
        assert_eq!(sheet_name("Bone_00"), None);
        assert_eq!(sheet_name("HandAnime_00"), None);
    }

    #[test]
    fn a_sheet_reads_as_load_hand_anime_info_does() {
        let mut list = BTreeMap::new();
        add_sheet(
            &vec![
                row(&["No", "Name", "Bundle", "File", "Clip"]),
                row(&["1", "", "studio/base/00.unity3d", "cf_hand_L_00", "goo"]),
                row(&["x", "", "a", "b", "c"]),
                row(&["2", "", "studio/base/00.unity3d", "cf_hand_L_00", "scissors"]),
            ],
            &mut list,
        );
        add_sheet(
            &vec![row(&["No"]), row(&["2", "", "mod.unity3d", "hands", "wave"])],
            &mut list,
        );
        assert_eq!(list.len(), 2);
        assert_eq!(list[&1].label(), "Fist");
        assert_eq!(list[&2].bundle, "mod.unity3d");
        assert_eq!(list[&2].label(), "Pattern 2");
        assert!(!list.contains_key(&0));
    }
}
