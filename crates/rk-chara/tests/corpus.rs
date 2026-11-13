// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks plugin data decoding across the local card library.

use rk_card::Card;

#[test]
fn abmx_data_decodes() {
    let Some(game) = rk_core::test_game_dir() else { return };
    let mut files = Vec::new();
    rk_core::walk_files(&game.join("UserData/chara"), "png", &mut files);
    let (mut with_key, mut decoded, mut modifiers) = (0, 0, 0usize);
    for p in &files {
        let Ok(card) = Card::load(p) else { continue };
        let Some(plugin) = card.ext.get(rk_chara::abmx::KEY) else {
            continue;
        };
        if plugin.bytes("boneData").is_none() {
            continue;
        }
        with_key += 1;
        let mods = rk_chara::abmx::read(&card.ext);
        // An empty list is valid; a blob that fails to decode is not.
        if plugin.lz4("boneData").is_some() {
            decoded += 1;
        }
        modifiers += mods.len();
    }
    eprintln!("ABMX: {with_key} cards carry data, {decoded} decoded, {modifiers} bone modifiers in total");
    assert_eq!(with_key, decoded);
}

/// Every card that stores overlay images must yield at least one overlay
/// for some outfit: a layout the reader does not understand shows up here.
#[test]
fn overlay_data_decodes() {
    use rayon::prelude::*;
    use rk_chara::overlays;
    let Some(game) = rk_core::test_game_dir() else { return };
    let mut files = Vec::new();
    rk_core::walk_files(&game.join("UserData/chara"), "png", &mut files);
    let results: Vec<(bool, usize, bool, usize)> = files
        .par_iter()
        .filter_map(|p| Card::load(p).ok())
        .map(|card| {
            let outfits = card.coordinates.len().max(1);
            let has_skin = card.ext.get(overlays::SKIN_KEY).is_some_and(|p| {
                p.data
                    .values()
                    .any(|v| matches!(v, rk_card::Value::Binary(b) if b.len() > 64))
            });
            let skin: usize = (0..outfits).map(|o| overlays::skin(&card.ext, o).len()).sum();
            let has_clothes = card
                .ext
                .get(overlays::CLOTHES_KEY)
                .is_some_and(|p| p.bytes("Overlays").is_some_and(|b| b.len() > 64));
            let clothes: usize = (0..outfits).map(|o| overlays::clothes(&card.ext, o).len()).sum();
            (has_skin, skin, has_clothes, clothes)
        })
        .collect();
    let with_skin = results.iter().filter(|r| r.0).count();
    let skin_ok = results.iter().filter(|r| r.0 && r.1 > 0).count();
    let with_clothes = results.iter().filter(|r| r.2).count();
    let clothes_ok = results.iter().filter(|r| r.2 && r.3 > 0).count();
    eprintln!(
        "skin overlays: {skin_ok} of {with_skin} cards decoded; clothes overlays: {clothes_ok} of {with_clothes}"
    );
    assert_eq!(skin_ok, with_skin);
    assert_eq!(clothes_ok, with_clothes);
}
