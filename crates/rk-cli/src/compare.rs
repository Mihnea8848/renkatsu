// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact sheets: our render of each card beside the image stored in the
//! card file, which the game rendered itself.

use anyhow::{bail, Result};
use glam::Vec3;
use rk_card::Card;
use rk_chara::{BuildOptions, Part};
use rk_core::Config;
use rk_render::{bounds, load_png, save_png, Camera, Renderer};
use rk_vfs::Game;
use std::path::{Path, PathBuf};

const CELL_W: u32 = 378;
const CELL_H: u32 = 528;
const GAP: u32 = 6;
const PAIRS_PER_ROW: u32 = 4;
const BACKGROUND: [u8; 4] = [40, 40, 46, 255];

struct Sheet {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Sheet {
    fn new(pairs: u32) -> Self {
        let rows = pairs.div_ceil(PAIRS_PER_ROW);
        let width = PAIRS_PER_ROW * (2 * CELL_W + 2 * GAP) + GAP;
        let height = rows * (CELL_H + GAP) + GAP;
        Self {
            width,
            height,
            rgba: BACKGROUND.repeat((width * height) as usize),
        }
    }

    /// Top-left corner of a cell: `side` 0 is the game image, 1 our render.
    fn origin(index: u32, side: u32) -> (u32, u32) {
        let (col, row) = (index % PAIRS_PER_ROW, index / PAIRS_PER_ROW);
        (
            GAP + col * (2 * CELL_W + 2 * GAP) + side * (CELL_W + GAP / 2),
            GAP + row * (CELL_H + GAP),
        )
    }

    /// Draws an image scaled to fill the cell, bilinear.
    fn blit(&mut self, index: u32, side: u32, w: u32, h: u32, src: &[u8]) {
        if w == 0 || h == 0 {
            return;
        }
        let (ox, oy) = Self::origin(index, side);
        for y in 0..CELL_H {
            let fy = (y as f32 + 0.5) * h as f32 / CELL_H as f32 - 0.5;
            let y0 = fy.floor().clamp(0.0, (h - 1) as f32) as u32;
            let y1 = (y0 + 1).min(h - 1);
            let ty = (fy - y0 as f32).clamp(0.0, 1.0);
            for x in 0..CELL_W {
                let fx = (x as f32 + 0.5) * w as f32 / CELL_W as f32 - 0.5;
                let x0 = fx.floor().clamp(0.0, (w - 1) as f32) as u32;
                let x1 = (x0 + 1).min(w - 1);
                let tx = (fx - x0 as f32).clamp(0.0, 1.0);
                let px = |xx: u32, yy: u32, c: usize| src[((yy * w + xx) * 4) as usize + c] as f32;
                let o = (((oy + y) * self.width + ox + x) * 4) as usize;
                for c in 0..3 {
                    let top = px(x0, y0, c) * (1.0 - tx) + px(x1, y0, c) * tx;
                    let bot = px(x0, y1, c) * (1.0 - tx) + px(x1, y1, c) * tx;
                    self.rgba[o + c] = (top * (1.0 - ty) + bot * ty + 0.5) as u8;
                }
                self.rgba[o + 3] = 255;
            }
        }
    }

    fn fill(&mut self, index: u32, side: u32, color: [u8; 4]) {
        let (ox, oy) = Self::origin(index, side);
        for y in 0..CELL_H {
            for x in 0..CELL_W {
                let o = (((oy + y) * self.width + ox + x) * 4) as usize;
                self.rgba[o..o + 4].copy_from_slice(&color);
            }
        }
    }
}

/// Evenly spaced sample of the cards in the game folder, in path order.
fn sample_cards(cfg: &Config, count: usize) -> Vec<PathBuf> {
    let mut all = Vec::new();
    rk_core::walk_files(&cfg.userdata().join("chara"), "png", &mut all);
    all.sort();
    let cards: Vec<PathBuf> = all
        .into_iter()
        .filter(|p| Card::load(p).is_ok_and(|c| c.image_len > 0))
        .collect();
    if cards.len() <= count {
        return cards;
    }
    (0..count).map(|i| cards[i * cards.len() / count].clone()).collect()
}

pub fn compare(cards: Vec<PathBuf>, count: usize, out: &Path) -> Result<()> {
    let cfg = Config::load()?;
    let game = Game::open(&cfg)?;
    let cards = if cards.is_empty() {
        sample_cards(&cfg, count)
    } else {
        cards
    };
    if cards.is_empty() {
        bail!("no cards to compare");
    }
    let mut renderer = Renderer::new()?;
    let mut sheet = Sheet::new(cards.len() as u32);
    for (i, path) in cards.iter().enumerate() {
        let i = i as u32;
        let file = std::fs::read(path)?;
        let card = match Card::parse(&file) {
            Ok(c) => c,
            Err(e) => {
                println!("{}: {e}", path.display());
                sheet.fill(i, 1, [90, 30, 30, 255]);
                continue;
            }
        };
        if let Ok((w, h, rgba)) = load_png(&file[..card.image_len]) {
            sheet.blit(i, 0, w, h, &rgba);
        }
        let chara = match rk_chara::build(&game, &card, &BuildOptions::default()) {
            Ok(c) => c,
            Err(e) => {
                println!("{}: {e:#}", path.display());
                sheet.fill(i, 1, [90, 30, 30, 255]);
                continue;
            }
        };
        let draws = chara.scene.pose();
        // Frame head to hips, like the card image.
        let head: Vec<_> = draws.iter().filter(|d| d.renderer.part == Part::Head).collect();
        let (all_min, all_max) = bounds(&draws);
        let (min, max) = if head.is_empty() {
            (all_min, all_max)
        } else {
            let mut lo = Vec3::splat(f32::MAX);
            let mut hi = Vec3::splat(f32::MIN);
            for d in &head {
                for p in &d.positions {
                    lo = lo.min(Vec3::from(*p));
                    hi = hi.max(Vec3::from(*p));
                }
            }
            let h = hi.y - lo.y;
            (
                Vec3::new(-0.01, lo.y - 2.4 * h, lo.z),
                Vec3::new(0.01, hi.y + 0.35 * h, hi.z),
            )
        };
        let camera = Camera::frame(min, max, CELL_W as f32 / CELL_H as f32, 0.0);
        renderer.clear_textures();
        let pixels = renderer.render(&draws, &camera, CELL_W, CELL_H, [0.93, 0.93, 0.95, 1.0])?;
        sheet.blit(i, 1, CELL_W, CELL_H, &pixels);
        println!(
            "{:>3} {} ({} warnings)",
            i,
            path.file_name().unwrap_or_default().to_string_lossy(),
            chara.warnings.len()
        );
    }
    save_png(out, sheet.width, sheet.height, &sheet.rgba)?;
    println!(
        "wrote {} ({} cards; game image left, Renkatsu right)",
        out.display(),
        cards.len()
    );
    Ok(())
}
