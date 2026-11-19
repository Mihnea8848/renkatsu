// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

use anyhow::Result;
use rk_card::Card;
use rk_chara::BuildOptions;
use rk_render::shadow::ShadowSettings;
use rk_render::{bounds, save_png, Camera, Environment, Renderer, StudioEffects};
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
    /// Camera position as `x,y,z`.
    pub eye: Option<&'a str>,
    /// The point the camera looks at, as `x,y,z`.
    pub look_at: Option<&'a str>,
    pub no_clothes: bool,
    pub no_accessories: bool,
    pub outdoor_shoes: bool,
    pub son: bool,
    pub censor: bool,
    pub no_physics: bool,
    pub list: bool,
    /// Draw the light's real-time shadows.
    pub shadows: bool,
    /// The states of the clothes, as `rk clothes` reads them.
    pub clothes: Option<&'a str>,
    /// The liquid levels, as five digits from 0 to 2.
    pub liquid: Option<&'a str>,
    /// An animation as `group/category/number`.
    pub pose: Option<&'a str>,
    /// Where in the animation, in passes.
    pub time: f32,
    /// Draw with the look of Renkatsu Studio's viewport.
    pub studio_look: bool,
    /// Smooth cut-out edges with alpha to coverage.
    pub alpha_to_coverage: bool,
    /// The colour behind the character, as `r,g,b` from 0 to 1.
    pub background: Option<&'a str>,
    /// A map to stand the character in, by number or name.
    pub map: Option<&'a str>,
    /// The map's time of day.
    pub map_time: &'a str,
}

/// The colour behind the character when none is asked for.
const BACKGROUND: [f64; 4] = [0.93, 0.93, 0.95, 1.0];

/// The liquid levels written as five digits, one per part of the body in
/// the game's order.
fn parse_liquid(text: &str) -> Result<rk_chara::siru::Levels> {
    use rk_chara::siru::{Levels, MAX, PARTS};
    let digits: Vec<u8> = text
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',')
        .map(|c| c.to_digit(10).map(|d| d as u8))
        .collect::<Option<_>>()
        .ok_or_else(|| anyhow::anyhow!("{text}: expected {PARTS} digits"))?;
    let levels: Levels = digits
        .try_into()
        .map_err(|_| anyhow::anyhow!("{text}: expected {PARTS} digits"))?;
    if let Some(level) = levels.iter().find(|&&l| l > MAX) {
        anyhow::bail!("liquid level {level}: the most a part takes is {MAX}");
    }
    Ok(levels)
}

/// A colour written as `r,g,b`, each from 0 to 1.
fn colour(text: &str) -> Result<[f64; 4]> {
    let [r, g, b] = point(text)?;
    anyhow::ensure!(
        [r, g, b].iter().all(|c| (0.0..=1.0).contains(c)),
        "`{text}` is not a colour; each of r,g,b goes from 0 to 1"
    );
    Ok([r.into(), g.into(), b.into(), 1.0])
}

/// The look of Renkatsu Studio's viewport: what a new CharaStudio scene has
/// on, with its first colour filter when the game has it, around a plain
/// `background` the effects leave alone.
fn studio_look(game: &Game, background: [f64; 4]) -> StudioEffects {
    let mut effects = StudioEffects::studio(rk_chara::build::color_filter(game, 0));
    effects.backdrop = Some(background);
    effects
}

/// A point written as `x,y,z`.
fn point(text: &str) -> Result<[f32; 3]> {
    let parts: Vec<f32> = text
        .split(',')
        .map(|p| p.trim().parse::<f32>())
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| anyhow::anyhow!("`{text}` is not a point; write it as x,y,z"))?;
    <[f32; 3]>::try_from(parts).map_err(|_| anyhow::anyhow!("`{text}` is not a point; write it as x,y,z"))
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
        visible_son: a.son.then_some(true),
        censor: a.censor,
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
    if let Some(spec) = a.liquid {
        let levels = parse_liquid(spec)?;
        chara.set_siru(&levels);
        println!("liquid levels {levels:?}");
        if !chara.liquid_mask() {
            println!("this body carries no liquid mask, so none of it shows");
        }
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
        let motion = rk_chara::motion::load(
            &game,
            &pose.bundle,
            &pose.controller,
            &pose.state,
            pose.over(),
            &chara.parameters,
        )?;
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
    let mut camera = Camera::frame(min, max, a.width as f32 / a.height as f32, a.yaw);
    if let Some(p) = a.look_at {
        camera.target = point(p)?.into();
    }
    if let Some(p) = a.eye {
        camera.eye = point(p)?.into();
    }
    let background = a.background.map(colour).transpose()?.unwrap_or(BACKGROUND);
    let mut renderer = Renderer::new()?;
    if let Some(which) = a.map {
        let scene = crate::map::scene(&game, which, a.map_time, renderer.supports_bc())?;
        renderer.set_map(Some(&scene));
    }
    let env = Environment {
        ramp: chara.ramp.clone(),
        effects: a.studio_look.then(|| studio_look(&game, background)),
        shadows: ShadowSettings {
            enabled: a.shadows,
            ..Default::default()
        },
        alpha_to_coverage: a.alpha_to_coverage,
        ..Default::default()
    };
    let pixels = renderer.render(&draws, &camera, &env, a.width, a.height, background)?;
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

#[cfg(test)]
mod tests {
    use super::{colour, parse_liquid, point};

    #[test]
    fn liquid_levels_are_five_digits_up_to_two() {
        assert_eq!(parse_liquid("01220").unwrap(), [0, 1, 2, 2, 0]);
        assert_eq!(parse_liquid("0,1,2,2,0").unwrap(), [0, 1, 2, 2, 0]);
        assert!(parse_liquid("0122").is_err());
        assert!(parse_liquid("012203").is_err());
        assert!(parse_liquid("01223").is_err());
        assert!(parse_liquid("0122x").is_err());
    }

    #[test]
    fn colours_are_three_numbers_from_0_to_1() {
        assert_eq!(colour("0.25, 0.5,1").unwrap(), [0.25, 0.5, 1.0, 1.0]);
        assert!(colour("0.2,0.2").is_err());
        assert!(colour("0.2,0.2,1.5").is_err());
        assert!(colour("-0.1,0,0").is_err());
    }

    #[test]
    fn points_are_three_numbers() {
        assert_eq!(point("0, -0.5,1.25").unwrap(), [0.0, -0.5, 1.25]);
        assert!(point("1,2").is_err());
        assert!(point("1,2,x").is_err());
        assert!(point("1,2,3,4").is_err());
    }
}
