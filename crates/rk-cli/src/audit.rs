// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builds many cards without drawing them and reports what went wrong or
//! was left out: failures, warnings, shaders, untextured materials and
//! plugin data that nothing reads yet.

use anyhow::Result;
use rayon::prelude::*;
use rk_card::Card;
use rk_chara::{BuildOptions, Part};
use rk_core::Config;
use rk_render::material::{classify, Kind};
use rk_vfs::Game;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

/// Plugin data keys that the builder reads.
const HANDLED: [&str; 6] = [
    rk_vfs::items::RESOLVER_KEY,
    rk_chara::abmx::KEY,
    rk_chara::material_editor::KEY,
    rk_chara::more_accessories::KEY,
    rk_chara::overlays::SKIN_KEY,
    rk_chara::overlays::CLOTHES_KEY,
];

#[derive(Default)]
struct Report {
    cards: usize,
    failed: BTreeMap<String, usize>,
    warnings: BTreeMap<String, usize>,
    cards_with_warnings: usize,
    shaders: BTreeMap<String, usize>,
    /// Materials that will draw plain white: no texture and no colour.
    blank: BTreeMap<String, usize>,
    cards_with_blank: usize,
    unbound: usize,
    plugins: BTreeMap<String, usize>,
    seconds: Vec<f32>,
}

/// Groups warnings that differ only in names and numbers.
fn category(w: &str) -> String {
    let head = w.split(':').next().unwrap_or(w);
    let head = head.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
    let tail = w.split_once(": ").map(|(_, t)| t).unwrap_or("");
    let tail: String = tail
        .split_whitespace()
        .take(3)
        .filter(|t| !t.chars().any(|c| c.is_ascii_digit()))
        .collect::<Vec<_>>()
        .join(" ");
    let head: String = head.chars().filter(|c| !c.is_ascii_digit()).collect();
    format!("{} / {}", head.trim().trim_start_matches("outfit."), tail)
}

fn top(map: &BTreeMap<String, usize>, n: usize) -> Vec<(&String, &usize)> {
    let mut v: Vec<_> = map.iter().collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(**c));
    v.truncate(n);
    v
}

pub fn audit(count: usize, threads: usize) -> Result<()> {
    let cfg = Config::load()?;
    let game = Game::open(&cfg)?;
    let mut all = Vec::new();
    rk_core::walk_files(&cfg.userdata().join("chara"), "png", &mut all);
    all.sort();
    let cards: Vec<PathBuf> = if all.len() <= count {
        all
    } else {
        (0..count).map(|i| all[i * all.len() / count].clone()).collect()
    };

    // Only a small summary of each character is kept: a built character
    // holds hundreds of megabytes of textures.
    struct Summary {
        seconds: f32,
        warnings: Vec<String>,
        plugins: Vec<String>,
        shaders: Vec<String>,
        blank: Vec<String>,
        unbound: usize,
    }
    let summarise =
        |path: &PathBuf| -> std::result::Result<Summary, String> {
            let card = Card::load(path).map_err(|e| format!("card: {e}"))?;
            let t = Instant::now();
            let chara = rk_chara::build(&game, &card, &BuildOptions::default()).map_err(|e| format!("build: {e:#}"))?;
            let mut s = Summary {
                seconds: t.elapsed().as_secs_f32(),
                warnings: chara.warnings.iter().map(|w| category(w)).collect(),
                plugins: card
                    .ext
                    .keys()
                    .filter(|k| !HANDLED.contains(&k.as_str()))
                    .cloned()
                    .collect(),
                shaders: Vec::new(),
                blank: Vec::new(),
                unbound: 0,
            };
            let (_, active) = chara.scene.world();
            for rend in chara.scene.renderers.iter().filter(|x| {
                x.enabled && x.cast_shadows != rk_chara::scene::ShadowCasting::ShadowsOnly && active[x.node]
            }) {
                if rend.skinned && !rend.bones.is_empty() && rend.bones.iter().all(Option::is_none) {
                    s.unbound += 1;
                }
                for m in &rend.materials {
                    let kind = classify(&m.shader);
                    s.shaders.push(format!(
                        "{:?} <- {}",
                        kind,
                        if m.shader.is_empty() { "(none)" } else { &m.shader }
                    ));
                    let textured = m.textures.contains_key("_MainTex") || m.textures.contains_key("_ColorMask");
                    let colored = m.colors.get("_Color").is_some_and(|c| c[..3] != [1.0, 1.0, 1.0]);
                    if matches!(kind, Kind::Item | Kind::Skin) && !textured && !colored {
                        let part = match rend.part {
                            Part::Body => "body",
                            Part::Head => "head",
                            Part::Hair(_) => "hair",
                            Part::Clothes(_) => "clothes",
                            Part::Accessory(_) => "accessory",
                        };
                        s.blank.push(format!("{part} [{}]", m.shader));
                    }
                }
            }
            Ok(s)
        };

    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build()?;
    let mut r = Report::default();
    // Small batches, with the bundle cache emptied in between.
    for batch in cards.chunks(threads.max(1) * 2) {
        let results: Vec<_> = pool.install(|| batch.par_iter().map(&summarise).collect());
        game.assets.clear();
        for res in results {
            r.cards += 1;
            let s = match res {
                Ok(s) => s,
                Err(e) => {
                    *r.failed.entry(e.chars().take(90).collect()).or_default() += 1;
                    continue;
                }
            };
            r.seconds.push(s.seconds);
            r.cards_with_warnings += !s.warnings.is_empty() as usize;
            r.cards_with_blank += !s.blank.is_empty() as usize;
            r.unbound += s.unbound;
            for (list, map) in [
                (s.warnings, &mut r.warnings),
                (s.plugins, &mut r.plugins),
                (s.shaders, &mut r.shaders),
                (s.blank, &mut r.blank),
            ] {
                for key in list {
                    *map.entry(key).or_default() += 1;
                }
            }
        }
    }

    let built = r.seconds.len();
    println!(
        "cards: {} sampled, {} built, {} failed",
        r.cards,
        built,
        r.cards - built
    );
    for (e, n) in top(&r.failed, 8) {
        println!("  failed {n}: {e}");
    }
    r.seconds.sort_by(|a, b| a.total_cmp(b));
    if built > 0 {
        println!(
            "build time: median {:.1}s, 90th percentile {:.1}s, slowest {:.1}s",
            r.seconds[built / 2],
            r.seconds[built * 9 / 10],
            r.seconds[built - 1]
        );
    }
    println!(
        "\nwarnings: {} of {} built cards have at least one",
        r.cards_with_warnings, built
    );
    for (w, n) in top(&r.warnings, 25) {
        println!("  {n:>5}  {w}");
    }
    println!("\nmaterials by kind and shader:");
    for (s, n) in top(&r.shaders, 40) {
        println!("  {n:>5}  {s}");
    }
    println!(
        "\nmaterials that draw plain white (no texture, no colour): {} cards affected",
        r.cards_with_blank
    );
    for (s, n) in top(&r.blank, 15) {
        println!("  {n:>5}  {s}");
    }
    println!("\nskinned meshes with no bone found: {}", r.unbound);
    println!("\nplugin data in cards that nothing reads yet (cards carrying it):");
    for (k, n) in top(&r.plugins, 40) {
        println!("  {n:>5}  {k}");
    }
    Ok(())
}
