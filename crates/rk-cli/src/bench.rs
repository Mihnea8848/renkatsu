// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Runs the reference comparison over a list of cards and prints one line
//! per card, then the data differences grouped across all cards, so one run
//! shows which kind of mistake affects how many cards.

use crate::reference::{run, Summary};
use crate::ui;
use anyhow::Result;
use rk_chara::BuildOptions;
use rk_core::Config;
use rk_render::Renderer;
use rk_vfs::Game;
use std::collections::BTreeMap;
use std::path::Path;

/// The property, texture or renderer a difference line is about, without
/// the renderer and material it was found on: `"o_top[0] _Color: ..."`
/// becomes `"_Color"`.
fn subject(kind: &str, line: &str) -> String {
    match kind {
        "property" | "texture" | "texture placement" => line
            .split(':')
            .next()
            .and_then(|head| head.split_whitespace().last())
            .unwrap_or(line)
            .to_string(),
        _ => kind.to_string(),
    }
}

/// Card counts per kind and subject of difference.
fn tally(summaries: &[Summary]) -> BTreeMap<(String, String), usize> {
    let mut counts = BTreeMap::new();
    for s in summaries {
        let mut seen = std::collections::BTreeSet::new();
        for (kind, lines) in &s.state.groups {
            for l in lines {
                seen.insert((kind.clone(), subject(kind, l)));
            }
        }
        for key in seen {
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    counts
}

/// A list line is a card path, optionally followed by `|<outfit>`, as the
/// capture plugin reads it.
fn split_outfit(line: &str) -> (&str, Option<usize>) {
    match line.rsplit_once('|') {
        Some((path, n)) => match n.trim().parse() {
            Ok(n) => (path.trim(), Some(n)),
            Err(_) => (line, None),
        },
        None => (line, None),
    }
}

pub fn bench(list: &Path, captures: &Path, out_dir: &Path, show: &[String]) -> Result<()> {
    let cfg = Config::load()?;
    let root = cfg.game_dir.join("UserData/chara/female");
    let game = Game::open(&cfg)?;
    let mut renderer = Renderer::new()?;
    std::fs::create_dir_all(out_dir)?;
    let opts = BuildOptions::default();
    let mut summaries = Vec::new();
    let lines: Vec<String> = std::fs::read_to_string(list)?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    let started = std::time::Instant::now();
    crate::ui::status("Comparing", format!("{} cards with their captures", lines.len()));
    let bar = ui::bar(lines.len() as u64);
    for line in &lines {
        let (path, outfit) = split_outfit(line);
        let card = root.join(path);
        let mut stem = card.file_stem().unwrap_or_default().to_string_lossy().to_string();
        if let Some(n) = outfit {
            stem = format!("{stem}.outfit{n}");
        }
        bar.set_message(stem.clone());
        let capture = captures.join(format!("{stem}.json"));
        if !capture.exists() {
            crate::ui::above(
                &bar,
                format!("{} {stem}: no capture", ui::dim(format!("{:>12}", "skipped"))),
            );
            bar.inc(1);
            continue;
        }
        let out = out_dir.join(format!("{stem}.png"));
        match run(&game, &mut renderer, &card, &capture, &out, &opts, &[], true, false) {
            Ok(s) => {
                let views: Vec<String> = s
                    .views
                    .iter()
                    .map(|(n, mean, outline)| {
                        format!(
                            "{} {} {}",
                            ui::dim(n),
                            ui::mean(*mean),
                            ui::dim(format!("outline {:.1}% off", outline * 100.0))
                        )
                    })
                    .collect();
                crate::ui::above(
                    &bar,
                    ui::line(
                        "Compared",
                        &format!(
                            "{:<32} {}  order {}  data {}  warnings {}",
                            s.name.chars().take(32).collect::<String>(),
                            views.join("  "),
                            ui::count(s.queues_differ),
                            ui::count(s.state.differences()),
                            ui::count(s.warnings.len())
                        ),
                    ),
                );
                for (kind, lines) in &s.state.groups {
                    for l in lines {
                        if show.iter().any(|w| *w == *kind || *w == subject(kind, l)) {
                            ui::above(&bar, format!("{:>14}{}", "", ui::dim(format!("{kind}: {l}"))));
                        }
                    }
                }
                summaries.push(s);
            }
            Err(e) => crate::ui::above(
                &bar,
                format!(
                    "{} {stem}: {e:#}",
                    console::style(format!("{:>12}", "failed")).red().bold()
                ),
            ),
        }
        bar.inc(1);
        // Both caches hold every card's data otherwise; a long list would
        // run out of memory.
        game.assets.clear();
        renderer.clear_textures();
    }
    bar.finish_and_clear();
    ui::status(
        "Finished",
        format!(
            "{} cards in {:.1} min, pictures in {}",
            summaries.len(),
            started.elapsed().as_secs_f32() / 60.0,
            out_dir.display()
        ),
    );
    let mut views: BTreeMap<&str, (f32, f32, usize)> = BTreeMap::new();
    for s in &summaries {
        for (name, mean, outline) in &s.views {
            let v = views.entry(name.as_str()).or_default();
            *v = (v.0 + mean, v.1 + outline, v.2 + 1);
        }
    }
    for (name, (mean, outline, n)) in views {
        let n = n as f32;
        ui::status(
            "Average",
            format!(
                "{name} {} match, outline {:.1}% off",
                ui::mean(mean / n).to_string().trim_start(),
                100.0 * outline / n
            ),
        );
    }
    let counts = tally(&summaries);
    if !counts.is_empty() {
        ui::heading(format!(
            "Data differences across {} cards (cards affected)",
            summaries.len()
        ));
        let mut rows: Vec<_> = counts.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for ((kind, subject), n) in rows.iter().take(40) {
            if kind == subject {
                println!("  {n:>4}  {kind}");
            } else {
                println!("  {n:>4}  {kind} {subject}");
            }
        }
    }
    let mut warnings: BTreeMap<&str, usize> = BTreeMap::new();
    for s in &summaries {
        for w in &s.warnings {
            *warnings.entry(w.split(':').next().unwrap_or(w)).or_default() += 1;
        }
    }
    if !warnings.is_empty() {
        ui::heading("Warnings (times seen)");
        for (w, n) in warnings {
            println!("  {n:>4}  {w}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_can_name_an_outfit() {
        assert_eq!(split_outfit("a/b.png|6"), ("a/b.png", Some(6)));
        assert_eq!(split_outfit("a/b.png"), ("a/b.png", None));
        assert_eq!(split_outfit("odd|name.png"), ("odd|name.png", None));
    }

    #[test]
    fn differences_group_by_what_they_are_about() {
        assert_eq!(subject("property", "o_top[0] _Color: game (1), ours (0)"), "_Color");
        assert_eq!(subject("texture", "hair[1] _MainTex: game a, ours b"), "_MainTex");
        assert_eq!(subject("cast shadows", "o_body: game On, ours Off"), "cast shadows");
    }
}
