// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! `rk capture-check`: works through a folder written by the capture
//! plugin's batch (`tools/reference-capture`, `out/batch`). Each case names
//! its own card, so no list is needed. Cases with a character and no map
//! are rendered and compared as `rk reference` does; cases taken in a map
//! are listed with what they recorded, since Renkatsu draws no maps yet.

use crate::reference::{run, Summary};
use crate::ui;
use anyhow::{Context, Result};
use rk_chara::BuildOptions;
use rk_core::Config;
use rk_render::Renderer;
use rk_vfs::Game;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The capture files of a folder, in name order, that `only` lets through.
fn cases(dir: &Path, only: &[String]) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| dir.display().to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            only.is_empty() || only.iter().any(|o| name.contains(o.as_str()))
        })
        .collect();
    files.sort();
    Ok(files)
}

/// The card file a capture names. The game runs under Wine on Linux and
/// writes `Z:\home\...` with either slash; `Z:` is the root folder.
fn card_path(written: &str) -> PathBuf {
    let slashes = written.replace('\\', "/");
    match slashes.strip_prefix("Z:").or_else(|| slashes.strip_prefix("z:")) {
        Some(rest) if rest.starts_with('/') => PathBuf::from(rest),
        _ => PathBuf::from(slashes),
    }
}

fn numbers(v: &Value) -> String {
    let list: Vec<String> = v
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|n| format!("{:.2}", n.as_f64().unwrap_or(0.0)))
        .collect();
    list.join(" ")
}

/// What a capture taken in a map says about the map, as lines of text:
/// the time of day in use, each time of day's sun, fog and colour filter,
/// the filter that ended up on the camera, and which lights reach which
/// layers.
pub fn map_lines(v: &Value) -> Vec<String> {
    let map = &v["map"];
    let globals = &v["globals"];
    let text = |v: &Value| v.as_str().unwrap_or("").to_owned();
    let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
    let renderers = list(&map["renderers"]);
    let shown = renderers.iter().filter(|r| r["shown"] == true).count();
    let mut out = vec![format!(
        "map {} ({}), {}: {} of {} renderers shown, {} lightmaps, {} reflection probes",
        map["number"],
        text(&map["scene"]),
        text(&map["sun_type"]),
        shown,
        renderers.len(),
        list(&map["lightmaps"]).len(),
        list(&map["reflection_probes"]).len()
    )];
    for t in list(&map["sun_light_info"]["times"]) {
        out.push(format!(
            "  {}: sun {} x {:.2} at {}, fog {} {} to {}, filter {} blend {:.2}",
            text(&t["type"]),
            numbers(&t["color"]),
            t["intensity"].as_f64().unwrap_or(0.0),
            numbers(&t["angle"]),
            if t["fog"] == true { "on" } else { "off" },
            t["fog_start"],
            t["fog_end"],
            text(&t["lut"]),
            t["lut_blend_amount"].as_f64().unwrap_or(0.0)
        ));
    }
    out.push(format!(
        "  on the camera: filter {} (Studio's number {}), blend {:.2}; fog {}; skybox {}",
        text(&globals["ace_lut"]),
        v["studio"]["ace_no"],
        globals["ace_blend_amount"].as_f64().unwrap_or(0.0),
        if globals["fog"] == true { "on" } else { "off" },
        text(&globals["skybox"])
    ));
    let layers = list(&v["layers"]);
    for l in list(&v["lights"]) {
        let mask = l["culling_mask"].as_i64().unwrap_or(0);
        let reached: Vec<String> = (0..32)
            .filter(|bit| mask >> bit & 1 == 1)
            .filter_map(|bit| layers.get(bit).and_then(Value::as_str))
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();
        out.push(format!(
            "  light {} ({}) x {:.2}, shadows {}: {}",
            text(&l["name"]),
            text(&l["type"]),
            l["intensity"].as_f64().unwrap_or(0.0),
            text(&l["shadows"]),
            if mask == -1 {
                "every layer".to_owned()
            } else {
                reached.join(", ")
            }
        ));
    }
    out
}

/// One case's scores as a line.
fn scores(s: &Summary) -> String {
    let views: Vec<String> = s
        .views
        .iter()
        .map(|(name, mean, outline)| format!("{name} {} outline {:.1}%", ui::mean(*mean), 100.0 * outline))
        .collect();
    let mm = |units: f32| units * 1000.0;
    let mut bones = format!("bones mean {:.1} mm", mm(s.bones.mean));
    if let Some((name, off)) = &s.bones.worst {
        bones += &format!(", worst {name} {:.1} mm", mm(*off));
    }
    if let Some(off) = s.bones.skirt {
        bones += &format!(", skirt {:.1} mm", mm(off));
    }
    if let Some(off) = s.bones.bust {
        bones += &format!(", bust {:.1} mm", mm(off));
    }
    format!(
        "{}; {bones}; draw order differs on {}, data on {}",
        views.join(", "),
        ui::count(s.queues_differ),
        ui::count(s.state.differences())
    )
}

pub fn check(
    dir: &Path,
    out_dir: &Path,
    only: &[String],
    studio: bool,
    alpha_to_coverage: bool,
    game_skirts: bool,
) -> Result<()> {
    let files = cases(dir, only)?;
    anyhow::ensure!(!files.is_empty(), "no capture files in {}", dir.display());
    let game = Game::open(&Config::load()?)?;
    let mut renderer = Renderer::new()?;
    std::fs::create_dir_all(out_dir)?;
    let opts = BuildOptions {
        advanced_skirts: !game_skirts,
        ..Default::default()
    };
    let (mut compared, mut stored, mut failed) = (0, 0, 0);
    for file in &files {
        let stem = file.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(file)?).with_context(|| file.display().to_string())?;
        let card = v["card"].as_str().unwrap_or("");
        if !v["map"].is_null() || card.is_empty() {
            stored += 1;
            println!("{stem}: stored only");
            for line in map_lines(&v) {
                println!("  {line}");
            }
            continue;
        }
        let out = out_dir.join(format!("{stem}.png"));
        // A case without the pictures with effects has only the plain ones.
        let with_effects = studio && v["case"]["studio_pictures"] != false;
        match run(
            &game,
            &mut renderer,
            &card_path(card),
            file,
            &out,
            &opts,
            &[],
            true,
            with_effects,
            alpha_to_coverage,
            false,
        ) {
            Ok(s) => {
                compared += 1;
                println!("{stem}: {}", scores(&s));
            }
            Err(e) => {
                failed += 1;
                println!("{stem}: failed: {e:#}");
            }
        }
        renderer.clear_textures();
    }
    println!(
        "{compared} compared, {stored} stored only, {failed} failed; pictures in {}",
        out_dir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cases_are_the_json_files_in_name_order() {
        let dir = std::env::temp_dir().join(format!("rk-capture-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["b.json", "a.legs.json", "a.body.plain.png", "index.txt"] {
            std::fs::write(dir.join(name), "{}").unwrap();
        }
        let names = |only: &[String]| -> Vec<String> {
            cases(&dir, only)
                .unwrap()
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
                .collect()
        };
        assert_eq!(names(&[]), ["a.legs.json", "b.json"]);
        assert_eq!(names(&["legs".to_owned()]), ["a.legs.json"]);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(cases(&dir, &[]).is_err());
    }

    #[test]
    fn card_paths_written_under_wine_are_read() {
        assert_eq!(
            card_path(r"Z:\home\me\Games\Koikatsu\UserData/chara/female\[a]/b c.png"),
            PathBuf::from("/home/me/Games/Koikatsu/UserData/chara/female/[a]/b c.png")
        );
        assert_eq!(card_path("/home/me/x.png"), PathBuf::from("/home/me/x.png"));
        assert_eq!(card_path(r"C:\Games\x.png"), PathBuf::from("C:/Games/x.png"));
    }

    #[test]
    fn map_lines_tell_the_times_of_day_and_the_lights() {
        let v: Value = serde_json::from_str(
            r#"{
            "studio": {"ace_no": 3},
            "globals": {"ace_lut": "lut_night", "ace_blend_amount": 0, "fog": true, "skybox": "sky"},
            "layers": ["Default", "", "", "", "Water", "", "", "", "", "", "Chara", "Map"],
            "lights": [
                {"name": "Directional Chara", "type": "Directional", "intensity": 1, "shadows": "Soft", "culling_mask": 1024},
                {"name": "Sun", "type": "Directional", "intensity": 1.5, "shadows": "Soft", "culling_mask": 2064},
                {"name": "Lamp", "type": "Point", "intensity": 2, "shadows": "None", "culling_mask": -1}],
            "map": {"number": 8, "scene": "2-1", "sun_type": "Night",
                "lightmaps": [{}, {}], "reflection_probes": [],
                "renderers": [{"shown": true}, {"shown": false}, {"shown": true}],
                "sun_light_info": {"times": [
                    {"type": "DayTime", "color": [0.88, 0.99, 0.95, 1], "intensity": 1.5, "angle": [50, 30, 0],
                     "fog": true, "fog_start": 0, "fog_end": 90, "lut": "lut_day", "lut_blend_amount": 0}]}}
        }"#,
        )
        .unwrap();
        let lines = map_lines(&v);
        assert_eq!(
            lines[0],
            "map 8 (2-1), Night: 2 of 3 renderers shown, 2 lightmaps, 0 reflection probes"
        );
        assert_eq!(
            lines[1],
            "  DayTime: sun 0.88 0.99 0.95 1.00 x 1.50 at 50.00 30.00 0.00, fog on 0 to 90, filter lut_day blend 0.00"
        );
        assert_eq!(
            lines[2],
            "  on the camera: filter lut_night (Studio's number 3), blend 0.00; fog on; skybox sky"
        );
        assert_eq!(
            lines[3],
            "  light Directional Chara (Directional) x 1.00, shadows Soft: Chara"
        );
        assert_eq!(lines[4], "  light Sun (Directional) x 1.50, shadows Soft: Water, Map");
        assert_eq!(lines[5], "  light Lamp (Point) x 2.00, shadows None: every layer");
    }
}
