// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Clothing Unlocker plugin (`KK_ClothingUnlocker`): an outfit can be
//! marked so that tops no longer remove the bottom or the bra, and bras no
//! longer remove the shorts.
//!
//! The plugin patches `ListInfoBase.GetInfo` and `GetInfoInt`: once
//! `ChangeClothesTop`, `Bot`, `Bra` or `Shorts` has run for a character
//! whose current outfit is unlocked, the list columns `NotBra`,
//! `Coordinate` and `HideShorts` read as 0 for every item. The game loads
//! the top first, so every such lookup of a load is covered.

use rk_card::{ExtData, Value};

pub const KEY: &str = "com.deathweasel.bepinex.clothingunlocker";

/// The list columns `Hooks.CheckOverride` replaces.
pub const OVERRIDDEN: [&str; 3] = ["NotBra", "Coordinate", "HideShorts"];

/// Whether the card unlocks `outfit` (`GetClothingUnlocked`); false
/// without data for it.
pub fn unlocked(ext: &ExtData, outfit: usize) -> bool {
    let Some(table) = ext.get(KEY).and_then(|p| p.nested("ClothingUnlocked")) else {
        return false;
    };
    let Value::Map(entries) = table else { return false };
    entries
        .iter()
        .find(|(k, _)| k.as_u64() == Some(outfit as u64))
        .and_then(|(_, v)| v.as_bool())
        .unwrap_or(false)
}

/// What the game reads from list column `key` of an item whose stored
/// value is `value`.
pub fn info_int(unlocked: bool, key: &str, value: Option<i32>) -> Option<i32> {
    if unlocked && OVERRIDDEN.contains(&key) {
        Some(0)
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_card::PluginData;

    fn ext(pairs: &[(i64, bool)]) -> ExtData {
        let map = Value::Map(
            pairs
                .iter()
                .map(|&(k, v)| (Value::from(k), Value::Boolean(v)))
                .collect(),
        );
        let mut bytes = Vec::new();
        rmpv::encode::write_value(&mut bytes, &map).unwrap();
        let mut p = PluginData::default();
        p.data.insert("ClothingUnlocked".into(), Value::Binary(bytes));
        let mut ext = ExtData::new();
        ext.insert(KEY.into(), p);
        ext
    }

    #[test]
    fn flag_is_per_outfit() {
        let ext = ext(&[(0, false), (5, true)]);
        assert!(!unlocked(&ext, 0));
        assert!(unlocked(&ext, 5));
        assert!(!unlocked(&ext, 3));
        assert!(!unlocked(&ExtData::new(), 5));
    }

    #[test]
    fn unlocked_outfits_read_zero() {
        assert_eq!(info_int(true, "Coordinate", Some(2)), Some(0));
        assert_eq!(info_int(true, "NotBra", Some(1)), Some(0));
        assert_eq!(info_int(true, "HideShorts", None), Some(0));
        assert_eq!(info_int(true, "Kind", Some(1)), Some(1));
        assert_eq!(info_int(false, "Coordinate", Some(2)), Some(2));
    }
}
