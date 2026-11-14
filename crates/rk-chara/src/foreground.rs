// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Whether brows and eyes are drawn in front of the hair.
//!
//! Each face layer of a head carries a script with two render queues, one for
//! "in front" and one for "behind", and front hair is drawn between the two.
//! A game setting picks the side for all characters, and a card can override
//! it for its brows and for its eyes.
//!
//! The numbers used here are the ones of the community's Hair Shadows Fix,
//! which every Better Repack install has: it moves front hair and the "in
//! front" queues so hair can receive shadows. Cards are made with it.

use rk_unityfs::{UObject, UValue};

/// Render queue of front hair with the Hair Shadows Fix.
pub const FRONT_HAIR_QUEUE: i32 = 2475;

/// The shader the fix moves.
pub const FRONT_HAIR_SHADER: &str = "Shader Forge/main_hair_front";

/// What Better Repack ships in the game's settings: neither in front.
pub const DEFAULT_IN_FRONT: bool = false;

/// Queues of one material of a face layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Queues {
    /// Index of the material on the renderer.
    pub material: usize,
    pub in_front: i32,
    pub behind: i32,
}

/// Reads a `SetRenderQueue_Custom` script.
pub fn read(o: &UObject) -> Option<Vec<Queues>> {
    let mut out = Vec::new();
    for entry in o.array("m_queueDatas")? {
        let entry = entry.as_object()?;
        out.push(Queues {
            material: entry.i64("id")? as usize,
            in_front: entry.get("m_queues").and_then(UValue::as_i64)? as i32,
            behind: entry.get("m_queuesBackup").and_then(UValue::as_i64)? as i32,
        });
    }
    Some(out)
}

/// Applies the Hair Shadows Fix to the queues stored for a face layer.
pub fn with_hair_shadows_fix(renderer: &str, mut stored: Vec<Queues>) -> Vec<Queues> {
    let first = match renderer {
        "cf_O_mayuge" | "cf_O_eyeline" | "cf_O_eyeline_low" | "cf_Ohitomi_L02" | "cf_Ohitomi_R02" => 2474,
        "cf_Ohitomi_L" | "cf_Ohitomi_R" => 2472,
        _ => return stored,
    };
    let Some(q) = stored.first_mut() else { return stored };
    q.in_front = first;
    if renderer == "cf_O_eyeline" {
        // The lash shadow is the second material; the game stores no entry for it.
        stored.truncate(1);
        stored.push(Queues {
            material: 1,
            in_front: 2473,
            behind: 2902,
        });
    }
    stored
}

/// True for the layer the brow setting controls; the rest follow the eyes.
pub fn is_brow(renderer: &str) -> bool {
    renderer == "cf_O_mayuge"
}

/// The card's choice (1 behind, 2 in front, anything else follows the
/// setting) against the game setting.
pub fn in_front(card_flag: u8, setting: bool) -> bool {
    match card_flag {
        1 => false,
        2 => true,
        _ => setting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(in_front: i32, behind: i32) -> Vec<Queues> {
        vec![Queues {
            material: 0,
            in_front,
            behind,
        }]
    }

    #[test]
    fn card_flag_overrides_the_setting() {
        assert!(!in_front(0, false));
        assert!(in_front(0, true));
        assert!(!in_front(1, true));
        assert!(in_front(2, false));
    }

    #[test]
    fn fix_moves_the_in_front_queues_below_front_hair() {
        let brow = with_hair_shadows_fix("cf_O_mayuge", stored(2800, 2900));
        assert_eq!(brow, stored(2474, 2900));
        let white = with_hair_shadows_fix("cf_Ohitomi_L", stored(2801, 2901));
        assert_eq!(white, stored(2472, 2901));
        // Every "in front" queue is below front hair, every "behind" above.
        for q in brow.iter().chain(&white) {
            assert!(q.in_front < FRONT_HAIR_QUEUE && q.behind > FRONT_HAIR_QUEUE);
        }
        // Upper lashes get an entry for their shadow material.
        let lash = with_hair_shadows_fix("cf_O_eyeline", stored(2803, 2903));
        assert_eq!(
            lash,
            vec![
                Queues {
                    material: 0,
                    in_front: 2474,
                    behind: 2903
                },
                Queues {
                    material: 1,
                    in_front: 2473,
                    behind: 2902
                },
            ]
        );
        // Other renderers are left alone.
        assert_eq!(
            with_hair_shadows_fix("cf_O_gag_eye_00", stored(2802, 2902)),
            stored(2802, 2902)
        );
        assert!(with_hair_shadows_fix("cf_O_mayuge", Vec::new()).is_empty());
        assert!(is_brow("cf_O_mayuge") && !is_brow("cf_O_eyeline"));
    }
}
