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
