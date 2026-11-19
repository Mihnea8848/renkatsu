// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The More Outfits plugin (`KK_MoreOutfits`): a character can have more
//! outfits than the game's seven, and the extra ones carry a name.
//!
//! The outfits themselves need nothing from the plugin. It only resizes
//! `ChaFile.coordinate` (`Plugin.AddCoordinateSlot`), and a card keeps
//! however many coordinates it was saved with, so they arrive on their
//! own.
//!
//! What the card does carry is `CoordinateNames`, a MessagePack map of
//! outfit number to name. `MoreOutfitsController.GetCoodinateName` reads
//! it and falls back to `Outfit <n>`, counting from one.
//!
//! The game's own seven keep the game's own names: the plugin's maker UI
//! only ever writes at `OriginalCoordinateLength` and above
//! (`CoordinateType` has seven values), and the dropdowns that show a
//! stored name are the ones listing the outfits past those seven. Some
//! cards do carry a name below seven anyway; it is left unused here, as
//! the plugin leaves it.

use rk_card::{ExtData, Value};
use std::collections::BTreeMap;

pub const KEY: &str = "com.deathweasel.bepinex.moreoutfits";

/// `MoreOutfits.Plugin.OriginalCoordinateLength`: how many outfits the
/// game itself has, which are the ones it names.
pub const GAME_OUTFITS: usize = 7;

/// `MoreOutfitsController.CoordinateNames`, by outfit number. A name for
/// an outfit the card does not have is dropped, as are blank names.
pub fn names(ext: &ExtData, outfits: usize) -> BTreeMap<usize, String> {
    let mut out = BTreeMap::new();
    let Some(Value::Map(entries)) = ext.get(KEY).and_then(|p| p.nested("CoordinateNames")) else {
        return out;
    };
    for (key, value) in entries {
        // MessagePack writes the dictionary's `int` keys as integers; a
        // card written through a JSON-ish path has them as strings.
        let index = match &key {
            Value::String(s) => s.as_str().and_then(|s| s.parse::<usize>().ok()),
            other => other.as_i64().and_then(|i| usize::try_from(i).ok()),
        };
        let (Some(index), Some(name)) = (index, value.as_str()) else {
            continue;
        };
        if index < outfits && !name.trim().is_empty() {
            out.insert(index, name.to_owned());
        }
    }
    out
}

/// The name to show for an outfit: the game's own for its seven, else the
/// one the card gives, else `Outfit <n>` counting from one.
pub fn outfit_name(index: usize, named: &BTreeMap<usize, String>) -> String {
    const GAME: [&str; GAME_OUTFITS] = [
        "In school",
        "After school",
        "Gym",
        "Swimsuit",
        "Club",
        "Casual",
        "Sleepover",
    ];
    if let Some(name) = GAME.get(index) {
        return (*name).to_owned();
    }
    named
        .get(&index)
        .cloned()
        .unwrap_or_else(|| format!("Outfit {}", index + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_card::PluginData;

    fn ext(pairs: Vec<(Value, Value)>) -> ExtData {
        let mut p = PluginData {
            version: 1,
            ..Default::default()
        };
        let mut bytes = Vec::new();
        rmpv::encode::write_value(&mut bytes, &Value::Map(pairs)).unwrap();
        p.data.insert("CoordinateNames".into(), Value::Binary(bytes));
        let mut ext = ExtData::new();
        ext.insert(KEY.into(), p);
        ext
    }

    #[test]
    fn names_come_from_the_card_whichever_way_the_keys_were_written() {
        let integers = ext(vec![
            (Value::from(7), Value::from("Baseball")),
            (Value::from(8), Value::from("  ")),
            (Value::from(9), Value::from("Past the end")),
        ]);
        let got = names(&integers, 8);
        assert_eq!(got.len(), 1);
        assert_eq!(got[&7], "Baseball");
        // Strings for keys read the same.
        let strings = ext(vec![(Value::from("7"), Value::from("Baseball"))]);
        assert_eq!(names(&strings, 8), got);
        // Nothing stored, nothing named.
        assert!(names(&ExtData::new(), 8).is_empty());
    }

    #[test]
    fn the_games_seven_keep_the_games_names() {
        let named = names(
            &ext(vec![
                (Value::from(4), Value::from("Racing Queen")),
                (Value::from(7), Value::from("Baseball")),
            ]),
            9,
        );
        assert_eq!(outfit_name(0, &named), "In school");
        // A name the plugin itself would never write is not used.
        assert_eq!(outfit_name(4, &named), "Club");
        assert_eq!(outfit_name(7, &named), "Baseball");
        // An extra outfit with no name is numbered from one.
        assert_eq!(outfit_name(8, &named), "Outfit 9");
    }
}
