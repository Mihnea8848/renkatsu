// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parses every card and coordinate file in the local install. Skipped when
//! no game directory is configured.

use rayon::prelude::*;
use rk_card::{Card, CardError, CoordinateFile};
use std::collections::BTreeMap;

#[test]
fn all_local_cards_parse() {
    let Some(game) = rk_core::test_game_dir() else {
        eprintln!("no game dir configured, skipping");
        return;
    };
    let mut files = Vec::new();
    rk_core::walk_files(&game.join("UserData/chara"), "png", &mut files);
    let results: Vec<_> = files.par_iter().map(|p| (p, Card::load(p))).collect();
    let mut failures = BTreeMap::<String, Vec<String>>::new();
    let (mut ok, mut plain) = (0, 0);
    for (p, r) in &results {
        match r {
            Ok(c) => {
                ok += 1;
                assert!(!c.coordinates.is_empty(), "{}: no coordinates", p.display());
                assert!(c.face.shape_value_face.len() >= 52, "{}: face shapes", p.display());
                assert!(c.body.shape_value_body.len() >= 44, "{}: body shapes", p.display());
            }
            Err(CardError::NoTrailer | CardError::Product(_) | CardError::Marker(_)) => plain += 1,
            Err(e) => failures.entry(e.to_string()).or_default().push(p.display().to_string()),
        }
    }
    eprintln!(
        "cards: {ok} parsed, {plain} not Koikatsu cards, {} failed",
        failures.values().map(Vec::len).sum::<usize>()
    );
    for (e, ps) in &failures {
        eprintln!("  {e}: {} files, e.g. {}", ps.len(), ps[0]);
    }
    assert!(ok > 0);
    assert!(failures.is_empty());
}

#[test]
fn all_local_coordinates_parse() {
    let Some(game) = rk_core::test_game_dir() else { return };
    let mut files = Vec::new();
    rk_core::walk_files(&game.join("UserData/coordinate"), "png", &mut files);
    let mut failures = Vec::new();
    let mut ok = 0;
    for p in &files {
        match CoordinateFile::load(p) {
            Ok(_) => ok += 1,
            Err(CardError::NoTrailer) => {}
            Err(e) => failures.push(format!("{}: {e}", p.display())),
        }
    }
    eprintln!("coordinates: {ok} parsed, {} failed", failures.len());
    for f in failures.iter().take(10) {
        eprintln!("  {f}");
    }
    assert!(failures.is_empty());
}
