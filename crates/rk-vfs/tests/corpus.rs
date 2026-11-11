// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolves every local card against the index and reports coverage.

use rk_card::Card;
use rk_vfs::{resolve::card_items, Game, Lookup};
use std::collections::BTreeMap;

#[test]
fn local_cards_resolve() {
    let Ok(cfg) = rk_core::Config::load() else { return };
    let game = Game::open(&cfg).unwrap();
    let mut files = Vec::new();
    rk_core::walk_files(&cfg.userdata().join("chara"), "png", &mut files);
    let (mut cards, mut complete, mut slots, mut found) = (0, 0, 0usize, 0usize);
    let mut missing_mods = BTreeMap::<String, usize>::new();
    let mut missing_game = BTreeMap::<i32, usize>::new();
    for p in &files {
        let Ok(card) = Card::load(p) else { continue };
        cards += 1;
        let items = card_items(&game.items, &card);
        slots += items.slots.len();
        let mut ok = true;
        for s in &items.slots {
            match &s.lookup {
                Lookup::Found(_) => found += 1,
                Lookup::MissingMod { guid, .. } => {
                    ok = false;
                    *missing_mods.entry(guid.clone()).or_default() += 1;
                }
                Lookup::Missing { .. } => {
                    ok = false;
                    *missing_game.entry(s.category).or_default() += 1;
                }
            }
        }
        complete += ok as usize;
    }
    eprintln!("{cards} cards, {complete} fully resolved; {found}/{slots} item references found");
    let mut mods: Vec<_> = missing_mods.into_iter().collect();
    mods.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    eprintln!(
        "missing mods ({}): {:?}",
        mods.len(),
        mods.iter().take(12).collect::<Vec<_>>()
    );
    eprintln!("missing game items by category: {missing_game:?}");
    assert!(found * 100 >= slots * 95, "under 95% of item references resolve");
}
