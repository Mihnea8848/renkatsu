// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Result;
use rk_card::Card;
use rk_chara::BuildOptions;
use rk_core::Config;
use rk_render::{bounds, save_png, Camera, Environment, Renderer};
use rk_vfs::Game;
use std::path::Path;
use std::time::Instant;

pub struct Args<'a> {
    pub card: &'a Path,
    pub out: &'a Path,
    pub outfit: usize,
    pub width: u32,
    pub height: u32,
    pub yaw: f32,
    pub no_clothes: bool,
    pub no_accessories: bool,
    pub outdoor_shoes: bool,
    pub list: bool,
}

pub fn render(a: Args) -> Result<()> {
    let t = Instant::now();
    let game = Game::open(&Config::load()?)?;
    let card = Card::load(a.card)?;
    let opts = BuildOptions {
        outfit: a.outfit,
        clothes: !a.no_clothes,
        accessories: !a.no_accessories,
        outdoor_shoes: a.outdoor_shoes,
    };
    let chara = rk_chara::build(&game, &card, &opts)?;
    let t_build = t.elapsed();
    let draws = chara.scene.pose();
    if a.list {
        for d in &draws {
            let r = d.renderer;
            println!("{:?} {} ({} verts)", r.part, r.name, d.positions.len());
            for m in &r.materials {
                let mut tex: Vec<_> = m
                    .textures
                    .iter()
                    .map(|(k, v)| format!("{k}={}", v.image.name))
                    .collect();
                tex.sort();
                println!("    {} [{}] {}", m.name, m.shader, tex.join(" "));
            }
        }
    }
    let (min, max) = bounds(&draws);
    let camera = Camera::frame(min, max, a.width as f32 / a.height as f32, a.yaw);
    let mut renderer = Renderer::new()?;
    let env = Environment {
        ramp: chara.ramp.clone(),
        ..Default::default()
    };
    let pixels = renderer.render(&draws, &camera, &env, a.width, a.height, [0.93, 0.93, 0.95, 1.0])?;
    save_png(a.out, a.width, a.height, &pixels)?;
    println!(
        "{}: {} meshes, {} nodes, built in {:.2}s, total {:.2}s -> {}",
        card.full_name(),
        draws.len(),
        chara.scene.nodes.len(),
        t_build.as_secs_f32(),
        t.elapsed().as_secs_f32(),
        a.out.display()
    );
    let e = chara.material_edits;
    if e != Default::default() {
        println!(
            "  material overrides: {} floats, {} colours, {} textures, {} shaders, {} renderers",
            e.floats, e.colors, e.textures, e.shaders, e.renderers
        );
    }
    for w in &chara.warnings {
        println!("  warning: {w}");
    }
    Ok(())
}
