// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! `rk map`: the maps CharaStudio lists, and what each map's scene holds.

use anyhow::{anyhow, Result};
use rayon::prelude::*;
use rk_map::list::{self, Entry};
use rk_map::scene::{self, MapInfo};
use rk_map::unity::{FogMode, LightKind};
use rk_vfs::Game;

const MB: f64 = 1024.0 * 1024.0;

/// Lists the maps. With `scan`, opens every one and adds what it holds.
pub fn ls(missing: bool, scan: bool) -> Result<()> {
    let game = Game::open_user(&[])?;
    let maps = list::read(&game, missing);
    if !scan {
        for m in &maps {
            println!(
                "{:>5}  {:<28} {:<34} {:<20} {}",
                m.number,
                m.name,
                m.bundle,
                m.level,
                m.source.label()
            );
        }
        println!("{} maps", maps.len());
        return Ok(());
    }
    // A few at a time: a map's bundle is held in memory while it is read,
    // and the content cache lets go of it afterwards.
    let pool = rayon::ThreadPoolBuilder::new().num_threads(4).build()?;
    let mut found: Vec<(&Entry, Result<MapInfo>)> = Vec::new();
    for batch in maps.chunks(4) {
        found.extend(pool.install(|| {
            batch
                .par_iter()
                .map(|m| (m, scene::inspect(&game, &m.bundle, &m.level)))
                .collect::<Vec<_>>()
        }));
        // Only between batches: a map being read still needs its files.
        game.assets.clear();
    }
    println!(
        "{:>5}  {:<24} {:>6} {:>5} {:>7} {:>7} {:>5} {:>5} {:>4} {:>6} {:>5} {:>4}  {:<12} sky",
        "no", "name", "MB", "rend", "verts", "tris", "mats", "texs", "lmap", "lights", "probe", "sun", "fog"
    );
    let mut failed = 0;
    for (m, info) in &found {
        match info {
            Ok(i) => println!(
                "{:>5}  {:<24} {:>6.1} {:>5} {:>7} {:>7} {:>5} {:>5} {:>4} {:>6} {:>5} {:>4}  {:<12} {}",
                m.number,
                clip(&m.name, 24),
                i.bytes as f64 / MB,
                i.mesh_renderers + i.skinned_renderers,
                i.vertices,
                i.triangles,
                i.materials,
                i.textures,
                i.lightmaps.len(),
                i.lights.len(),
                i.reflection_probes.len(),
                i.sun.as_ref().map_or(0, |s| s.times.len()),
                fog(i),
                i.skybox_shader.as_deref().unwrap_or("-"),
            ),
            Err(e) => {
                failed += 1;
                println!("{:>5}  {:<24} failed: {e:#}", m.number, clip(&m.name, 24));
            }
        }
    }
    // What the maps need from the renderer, over all of them.
    let read: Vec<&MapInfo> = found.iter().filter_map(|(_, i)| i.as_ref().ok()).collect();
    let mut shaders = std::collections::BTreeMap::<&str, usize>::new();
    let mut built_in = std::collections::BTreeMap::<&str, usize>::new();
    let mut formats = std::collections::BTreeMap::<i32, usize>::new();
    for i in &read {
        for (s, n) in &i.shaders {
            *shaders.entry(s).or_default() += n;
        }
        for (s, n) in &i.built_in_meshes {
            *built_in.entry(s).or_default() += n;
        }
        for l in &i.lightmaps {
            *formats.entry(l.format).or_default() += 1;
        }
    }
    let mut shaders: Vec<_> = shaders.into_iter().collect();
    shaders.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    println!();
    println!("{} maps, {} read, {failed} failed", found.len(), read.len());
    let missing: Vec<String> = found
        .iter()
        .filter_map(|(m, i)| {
            let n = i.as_ref().ok()?.missing_meshes;
            (n > 0).then(|| format!("{} ({n})", m.number))
        })
        .collect();
    println!("meshes not found: {}", missing.join(", "));
    println!("built-in meshes: {built_in:?}");
    println!("lightmap formats: {formats:?}");
    println!("shaders by materials:");
    for (s, n) in shaders {
        println!("  {n:>5}  {s}");
    }
    Ok(())
}

/// Prints what one map's scene holds. `which` is its number or level name.
pub fn info(which: &str) -> Result<()> {
    let game = Game::open_user(&[])?;
    let maps = list::read(&game, true);
    let m = maps
        .iter()
        .find(|m| m.number.to_string() == which || m.level.eq_ignore_ascii_case(which))
        .ok_or_else(|| anyhow!("no map {which}"))?;
    println!("{} {} ({}), from {}", m.number, m.name, m.level, m.source.label());
    println!("bundle {}", m.bundle);
    if !m.vanish_bundle.is_empty() {
        println!("walls that hide: {} {}", m.vanish_bundle, m.vanish_file);
    }
    let i = scene::inspect(&game, &m.bundle, &m.level)?;
    println!(
        "{:.1} MB; {} objects, {} mesh and {} skinned renderers, {} vertices, {} triangles",
        i.bytes as f64 / MB,
        i.game_objects,
        i.mesh_renderers,
        i.skinned_renderers,
        i.vertices,
        i.triangles
    );
    println!(
        "{} materials, {} textures ({:.1} MB stored), formats {:?}",
        i.materials,
        i.textures,
        i.texture_bytes as f64 / MB,
        i.texture_formats
    );
    for (s, n) in &i.shaders {
        println!("  {n:>4}  {s}");
    }
    if !i.built_in_meshes.is_empty() {
        println!("built-in meshes {:?}", i.built_in_meshes);
    }
    if i.missing_meshes > 0 {
        println!("{} meshes not found: {}", i.missing_meshes, i.missing_where.join("; "));
    }
    if i.empty_renderers > 0 {
        println!("{} renderers without a mesh", i.empty_renderers);
    }
    if let Some(r) = &i.render_settings {
        println!(
            "sky {} ({:?} ambient x {:.2}), reflection {} x {:.2}",
            i.skybox_shader.as_deref().unwrap_or("none"),
            r.ambient_mode,
            r.ambient_intensity,
            if r.reflection_mode == 0 {
                "from the sky"
            } else {
                "custom"
            },
            r.reflection_intensity
        );
        println!("scene fog: {}", fog(&i));
    }
    for l in &i.lights {
        let kind = match l.kind {
            LightKind::Directional => "directional",
            LightKind::Point => "point",
            LightKind::Spot => "spot",
            LightKind::Area => "area",
        };
        println!(
            "light {kind}, colour {:.2} {:.2} {:.2} x {:.2}, range {:.1}, shadows {}, layers {:#x}, mode {}{}",
            l.color[0],
            l.color[1],
            l.color[2],
            l.intensity,
            l.range,
            l.shadows,
            l.culling_mask,
            l.lightmapping,
            if l.enabled { "" } else { ", off" }
        );
    }
    if let Some(s) = &i.lightmap_settings {
        println!(
            "{} lightmaps, mode {}: {}",
            s.lightmaps.len(),
            s.mode,
            i.lightmaps
                .iter()
                .map(|l| format!("{}x{} f{}", l.width, l.height, l.format))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!(
        "renderers naming a lightmap: {}, LightmapPrefab entries: {}",
        i.lightmapped_renderers,
        i.lightmap_prefab.len()
    );
    println!(
        "{} reflection probes ({} with box projection), {} cubemaps, {} light probe sets",
        i.reflection_probes.len(),
        i.reflection_probes.iter().filter(|p| p.box_projection).count(),
        i.cubemaps,
        i.light_probe_sets
    );
    if let Some(sun) = &i.sun {
        for t in &sun.times {
            println!(
                "{}: sun {:.2} {:.2} {:.2} x {:.2} at {:.1} {:.1} {:.1}, fog {}, {} objects only then, filter {}",
                t.time.map_or("?", |t| t.name()),
                t.color[0],
                t.color[1],
                t.color[2],
                t.intensity,
                t.angle[0],
                t.angle[1],
                t.angle[2],
                if t.fog {
                    format!("{:.1} to {:.1}", t.fog_start, t.fog_end)
                } else {
                    "off".to_owned()
                },
                t.visible.len(),
                if t.lut.path_id != 0 { "yes" } else { "none" }
            );
        }
    } else {
        println!("no SunLightInfo");
    }
    println!(
        "moving: {} animators, {} particle systems, {} video players",
        i.animators, i.particle_systems, i.video_players
    );
    let scripts: Vec<String> = i.scripts.iter().map(|(s, n)| format!("{s} {n}")).collect();
    println!("scripts: {}", scripts.join(", "));
    Ok(())
}

fn fog(i: &MapInfo) -> String {
    match &i.render_settings {
        Some(r) if r.fog => match r.fog_mode {
            FogMode::Linear => format!("{:.0}-{:.0}", r.fog_start, r.fog_end),
            FogMode::Exponential => format!("exp {:.3}", r.fog_density),
            FogMode::ExponentialSquared => format!("exp2 {:.3}", r.fog_density),
        },
        _ => "off".to_owned(),
    }
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
