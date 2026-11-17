// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

use anyhow::Result;
use rk_card::Card;
use rk_chara::BuildOptions;
use rk_render::shadow::ShadowSettings;
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
    pub no_physics: bool,
    pub list: bool,
    /// Draw the light's real-time shadows.
    pub shadows: bool,
    /// The states of the clothes, as `rk clothes` reads them.
    pub clothes: Option<&'a str>,
    /// An animation as `group/category/number`.
    pub pose: Option<&'a str>,
    /// Where in the animation, in passes.
    pub time: f32,
}

pub fn render(a: Args) -> Result<()> {
    let t = Instant::now();
    let game = Game::open_user(&[])?;
    let card = Card::load(a.card)?;
    let opts = BuildOptions {
        outfit: a.outfit,
        clothes: !a.no_clothes,
        accessories: !a.no_accessories,
        outdoor_shoes: a.outdoor_shoes,
        physics: !a.no_physics,
        ..Default::default()
    };
    let mut chara = rk_chara::build(&game, &card, &opts)?;
    let t_build = t.elapsed();
    let mut states = [rk_chara::clothes_state::ON; rk_chara::clothes_state::KINDS];
    if let Some(spec) = a.clothes {
        states = crate::pose::parse_states(spec, &chara.clothes.worn)?;
        chara.set_clothes(&states);
        println!("clothes states {states:?}");
    }
    if let Some(spec) = a.pose {
        use anyhow::Context;
        let numbers: Vec<i32> = spec.split('/').filter_map(|n| n.parse().ok()).collect();
        let [group, category, number] = numbers[..] else {
            anyhow::bail!("{spec}: expected group/category/number");
        };
        let library = rk_chara::animations::Library::read(&game);
        let pose = library
            .find(group, category, number)
            .with_context(|| format!("CharaStudio's list has no animation {spec}"))?;
        let motion = rk_chara::motion::load(&game, &pose.bundle, &pose.controller, &pose.state, &chara.parameters)?;
        let bound = motion.bind(&rk_chara::motion::Skeleton::of(&chara.scene, chara.animator));
        if let Some(heels) = &mut chara.heels {
            heels.animation(Some(&pose.controller), Some(&pose.state));
        }
        let phase = motion.phase(a.time, false);
        // The frame as the game shows it: the animation, then the physics.
        if !a.no_physics {
            for _ in 0..opts.settle_frames {
                chara.physics.begin_frame(&mut chara.scene);
                chara.animate(&bound, phase, &states);
                chara.physics.end_frame(&mut chara.scene, opts.frame_time);
            }
        } else {
            chara.animate(&bound, phase, &states);
        }
        println!(
            "{} ({}/{}): {} clips, {:.2} s, {} bone properties, {} curves without a bone",
            pose.name,
            pose.controller,
            pose.state,
            motion.clips.len(),
            motion.duration(),
            bound.tracks(),
            bound.unbound
        );
    }
    let draws = chara.scene.pose();
    if a.list {
        for d in &draws {
            let r = d.renderer;
            println!(
                "{:?} {} ({} verts, casts {:?}, receives {})",
                r.part,
                r.name,
                d.base_positions.len(),
                r.cast_shadows,
                r.receive_shadows
            );
            for m in &r.materials {
                let mut tex: Vec<_> = m
                    .textures
                    .iter()
                    .map(|(k, v)| format!("{k}={}", v.image.name))
                    .collect();
                tex.sort();
                if m.absent {
                    println!("    (no material, not drawn)");
                    continue;
                }
                println!("    {} [{}] {}", m.name, m.shader, tex.join(" "));
            }
        }
    }
    let (min, max) = bounds(&draws);
    let camera = Camera::frame(min, max, a.width as f32 / a.height as f32, a.yaw);
    let mut renderer = Renderer::new()?;
    let env = Environment {
        ramp: chara.ramp.clone(),
        shadows: ShadowSettings {
            enabled: a.shadows,
            ..Default::default()
        },
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
