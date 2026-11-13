// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Result;
use rk_card::Card;
use rk_core::Config;
use rk_vfs::{resolve::card_items, Game, Lookup};
use std::path::Path;
use std::time::Instant;

pub fn index() -> Result<()> {
    let t = Instant::now();
    let game = Game::open(&Config::load()?)?;
    let idx = &game.assets.index;
    let (vanilla, modded) = game.items.counts();
    println!("indexed in {:.1}s", t.elapsed().as_secs_f32());
    println!("mods: {} loaded, {} skipped", idx.mods.len(), idx.skipped.len());
    println!(
        "bundles: {} virtual paths, {} game CAB names",
        idx.bundles.len(),
        idx.cabs.len()
    );
    println!(
        "items: {vanilla} game, {modded} from mods, in {} lists",
        idx.lists.len()
    );
    let mut reasons = std::collections::BTreeMap::<String, usize>::new();
    for (_, why) in &idx.skipped {
        let key = if why.starts_with("superseded") {
            "superseded by another version".to_string()
        } else {
            why.clone()
        };
        *reasons.entry(key).or_default() += 1;
    }
    for (why, n) in reasons {
        println!("  skipped {n}: {why}");
    }
    Ok(())
}

pub fn resolve(path: &Path, all_outfits: bool) -> Result<()> {
    let game = Game::open(&Config::load()?)?;
    let card = Card::load(path)?;
    let items = card_items(&game.items, &card);
    println!(
        "{} ({} outfits, {} resolver records)",
        card.full_name(),
        card.coordinates.len(),
        items.infos.len()
    );
    let (mut found, mut missing) = (0, 0);
    for s in &items.slots {
        let show = all_outfits || !s.label.starts_with("outfit") || s.label.starts_with("outfit0.");
        match &s.lookup {
            Lookup::Found(item) => {
                found += 1;
                if show {
                    let src = game.assets.mod_guid(item.mod_idx).unwrap_or("game");
                    let ab = item.get("MainAB").or_else(|| item.get("MainTexAB")).unwrap_or("-");
                    let asset = item.get("MainData").or_else(|| item.get("MainTex")).unwrap_or("-");
                    println!(
                        "  {:<22} {:>3}:{:<10} {:<28} {ab} :: {asset}  [{src}]",
                        s.label,
                        s.category,
                        s.id,
                        item.name()
                    );
                }
            }
            Lookup::MissingMod { guid, slot } => {
                missing += 1;
                println!(
                    "  {:<22} {:>3}:{:<10} MISSING MOD {guid} (item {slot})",
                    s.label, s.category, s.id
                );
            }
            Lookup::Missing { id } => {
                missing += 1;
                println!(
                    "  {:<22} {:>3}:{:<10} MISSING game item {id}",
                    s.label, s.category, s.id
                );
            }
        }
    }
    println!("{found} resolved, {missing} missing");
    Ok(())
}
