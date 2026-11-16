// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

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
const HANDLED: [&str; 21] = [
    rk_vfs::items::RESOLVER_KEY,
    rk_chara::uncensor_selector::KEY,
    rk_chara::invisible_body::KEY,
    rk_chara::pregnancy::KEY,
    rk_chara::clothing_unlocker::KEY,
    rk_chara::eye_control::KEY,
    rk_chara::colliders::KEY,
    rk_chara::accessory_states::KEY,
    rk_chara::accessory_states::ASS_KEY,
    rk_chara::overlays::COBOC_KEY,
    rk_chara::edit_fangs::KEY,
    rk_chara::clothing_blend_shape::KEY,
    rk_chara::dbde::KEY,
    rk_chara::abmx::KEY,
    rk_chara::material_editor::KEY,
    rk_chara::more_accessories::KEY,
    rk_chara::overlays::SKIN_KEY,
    rk_chara::overlays::CLOTHES_KEY,
    rk_chara::pushup::KEY,
    rk_chara::hair_accessories::KEY,
    rk_chara::dynamic_bone_editor::KEY,
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
                // Empty material slots are not drawn, in the game or ours.
                for m in rend.materials.iter().filter(|m| !m.absent) {
                    let kind = classify(&m.shader);
                    let ported = if rk_render::material::PORTED.contains(&m.shader.as_str()) {
                        "ported     "
                    } else {
                        "approximate"
                    };
                    s.shaders.push(format!(
                        "{ported} {:?} <- {}",
                        kind,
                        if m.shader.is_empty() { "(none)" } else { &m.shader }
                    ));
                    let textured = m.textures.contains_key("_MainTex") || m.textures.contains_key("_ColorMask");
                    let colored = m.colors.get("_Color").is_some_and(|c| c[..3] != [1.0, 1.0, 1.0]);
                    // Glass lenses are clear by design: their shader shades
                    // with _Color4 and the glass ramp, not a texture.
                    let glass = m.shader == "Shader Forge/toon_glasses_lod0";
                    if matches!(kind, Kind::Item | Kind::Skin) && !textured && !colored && !glass {
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
    let started = Instant::now();
    let bar = crate::ui::bar(cards.len() as u64);
    // Small batches, with the bundle cache emptied in between.
    for batch in cards.chunks(threads.max(1) * 2) {
        if let Some(name) = batch.last().and_then(|p| p.file_stem()) {
            bar.set_message(name.to_string_lossy().to_string());
        }
        let results: Vec<_> = pool.install(|| batch.par_iter().map(&summarise).collect());
        game.assets.clear();
        bar.inc(batch.len() as u64);
        for (path, res) in batch.iter().zip(results) {
            r.cards += 1;
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            let s = match res {
                Ok(s) => s,
                Err(e) => {
                    crate::ui::above(&bar, crate::ui::line_fail(&format!("{name}: {e}")));
                    *r.failed.entry(e.chars().take(90).collect()).or_default() += 1;
                    continue;
                }
            };
            crate::ui::above(
                &bar,
                crate::ui::line(
                    "Built",
                    &format!(
                        "{name} {}",
                        crate::ui::dim(format!("{:.1}s, {} warnings", s.seconds, s.warnings.len()))
                    ),
                ),
            );
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

    bar.finish_and_clear();
    let built = r.seconds.len();
    crate::ui::status(
        "Built",
        format!(
            "{built} of {} cards in {:.1} min, {} failed",
            r.cards,
            started.elapsed().as_secs_f32() / 60.0,
            crate::ui::count(r.cards - built)
        ),
    );
    for (e, n) in top(&r.failed, 8) {
        crate::ui::fail(format!("{n} cards: {e}"));
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
    crate::ui::heading(format!(
        "Warnings: {} of {built} built cards have at least one",
        r.cards_with_warnings
    ));
    for (w, n) in top(&r.warnings, 25) {
        println!("  {n:>5}  {w}");
    }
    crate::ui::heading("Materials by shader (ported from the game's code, or approximate)");
    for (s, n) in top(&r.shaders, 40) {
        let line = if s.starts_with("ported") {
            console::style(s.as_str()).green()
        } else {
            console::style(s.as_str()).yellow()
        };
        println!("  {n:>5}  {line}");
    }
    crate::ui::heading(format!(
        "Materials that draw plain white (no texture, no colour): {} cards affected",
        r.cards_with_blank
    ));
    for (s, n) in top(&r.blank, 15) {
        println!("  {n:>5}  {s}");
    }
    crate::ui::heading(format!(
        "Skinned meshes with no bone found: {}",
        crate::ui::count(r.unbound)
    ));
    crate::ui::heading("Plugin data in cards that nothing reads yet (cards carrying it)");
    for (k, n) in top(&r.plugins, 40) {
        println!("  {n:>5}  {k}");
    }
    Ok(())
}
