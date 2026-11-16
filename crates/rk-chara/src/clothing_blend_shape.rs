// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Clothing BlendShape plugin. A clothing item made for it carries a
//! `ChaClothesBlendShape` component that lists up to four blend shapes, each
//! on a set of skinned meshes. When the item loads, every listed shape the
//! meshes have is set to 20; the plugin then sets the weights the card saved
//! for this outfit, item category and shoe type. A card without an entry
//! gets 20 for all four.

use rk_card::{map_get, ExtData, Value};

pub const KEY: &str = "nakay.kk.ClothingBlendShape";

/// Class of the component on clothing items.
pub const SCRIPT: &str = "ChaClothesBlendShape";

/// Weight Unity's `SetBlendShapeWeight` gets when nothing else is saved, on
/// its 0 to 100 scale.
pub const DEFAULT_WEIGHT: f32 = 20.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub coordinate: i64,
    pub category: i64,
    pub shoes_type: i64,
    pub weights: [f32; 4],
}

/// The card's saved entries, all outfits.
pub fn read(ext: &ExtData) -> Vec<Entry> {
    let Some(Value::Array(list)) = ext.get(KEY).and_then(|p| p.nested("ClothesDataSet")) else {
        return Vec::new();
    };
    list.iter().filter_map(entry).collect()
}

fn entry(v: &Value) -> Option<Entry> {
    let int = |k: &str| map_get(v, k).and_then(Value::as_i64);
    let mut weights = [DEFAULT_WEIGHT; 4];
    if let Some(Value::Array(a)) = map_get(v, "ShapeWeight") {
        for (w, x) in weights.iter_mut().zip(a) {
            if let Some(f) = x.as_f64() {
                *w = f as f32;
            }
        }
    }
    Some(Entry {
        coordinate: int("Coordinate")?,
        category: int("Category")?,
        shoes_type: int("ShoesType").unwrap_or(0),
        weights,
    })
}

/// The weights for an item, as `GetClothesData` picks them: the entry for
/// this outfit, category and shoe type (1 for the outdoor shoes, slot 8),
/// else the plugin's default.
pub fn weights_for(entries: &[Entry], outfit: usize, category: i32, clothes_kind: usize) -> [f32; 4] {
    let shoes_type = i64::from(clothes_kind == 8);
    entries
        .iter()
        .find(|e| e.coordinate == outfit as i64 && e.category == i64::from(category) && e.shoes_type == shoes_type)
        .map_or([DEFAULT_WEIGHT; 4], |e| e.weights)
}

/// One of the component's four slots, its meshes already found.
pub struct Slot {
    pub named: bool,
    /// Scene renderer index and the number of blend shapes its mesh has.
    pub meshes: Vec<(usize, usize)>,
    pub shape: usize,
}

/// The weight each listed mesh's shape ends up with, 0 to 1 as the scene
/// stores it: `(renderer, shape, weight)`.
pub fn settle(slots: &[Slot], weights: [f32; 4]) -> Vec<(usize, usize, f32)> {
    let mut out = Vec::new();
    for (i, s) in slots.iter().enumerate() {
        // A shape index past any mesh's shapes disables the whole slot.
        if s.meshes.iter().any(|&(_, count)| count <= s.shape) {
            continue;
        }
        // The card's weight only reaches slots with a name; the rest keep
        // the 20 set when the item loaded.
        let w = if s.named { weights[i] } else { DEFAULT_WEIGHT };
        for &(r, _) in &s.meshes {
            out.push((r, s.shape, w / 100.0));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_matching_entry_wins_and_outdoor_shoes_are_their_own_type() {
        let e = |coordinate, category, shoes_type, w| Entry {
            coordinate,
            category,
            shoes_type,
            weights: [w; 4],
        };
        let list = [e(2, 105, 0, 50.0), e(0, 113, 1, 70.0), e(0, 113, 0, 10.0)];
        assert_eq!(weights_for(&list, 2, 105, 0), [50.0; 4]);
        assert_eq!(weights_for(&list, 0, 113, 8), [70.0; 4]);
        assert_eq!(weights_for(&list, 0, 113, 7), [10.0; 4]);
        assert_eq!(weights_for(&list, 1, 105, 0), [DEFAULT_WEIGHT; 4]);
    }

    #[test]
    fn slots_follow_the_plugins_rules() {
        let slots = [
            Slot {
                named: true,
                meshes: vec![(4, 3), (5, 2)],
                shape: 1,
            },
            // Index past the second mesh's shapes: the slot is off.
            Slot {
                named: true,
                meshes: vec![(4, 3), (5, 2)],
                shape: 2,
            },
            // No name: keeps the default.
            Slot {
                named: false,
                meshes: vec![(6, 1)],
                shape: 0,
            },
        ];
        let got = settle(&slots, [60.0, 70.0, 80.0, 90.0]);
        assert_eq!(got, vec![(4, 1, 0.6), (5, 1, 0.6), (6, 0, 0.2)]);
    }

    #[test]
    fn entries_are_read_from_maps() {
        let m = Value::Map(vec![
            ("Category".into(), 111.into()),
            ("Coordinate".into(), 6.into()),
            ("ShapeWeight".into(), Value::Array(vec![59.5.into(), 0.0.into()])),
            ("ShoesType".into(), 0.into()),
        ]);
        let e = entry(&m).unwrap();
        assert_eq!((e.coordinate, e.category, e.shoes_type), (6, 111, 0));
        assert_eq!(e.weights, [59.5, 0.0, 20.0, 20.0]);
    }
}
