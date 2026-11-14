// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Compares our render with a picture captured inside the game by the plugin
//! in `tools/reference-capture`, using the camera and light the capture
//! recorded.

use anyhow::{bail, Context, Result};
use glam::{Quat, Vec3};
use rk_card::Card;
use rk_chara::BuildOptions;
use rk_core::Config;
use rk_render::{load_png, save_png, Camera, Environment, Renderer};
use rk_vfs::Game;
use serde_json::Value;
use std::path::Path;

const GAP: u32 = 6;

fn floats<const N: usize>(v: &Value) -> Result<[f32; N]> {
    let list = v.as_array().context("expected a list of numbers")?;
    if list.len() != N {
        bail!("expected {N} numbers, found {}", list.len());
    }
    let mut out = [0.0; N];
    for (o, x) in out.iter_mut().zip(list) {
        *o = x.as_f64().context("expected a number")? as f32;
    }
    Ok(out)
}

/// What a capture file says about the camera and the light.
pub struct Capture {
    pub width: u32,
    pub height: u32,
    /// One camera per captured picture, with the view's name.
    pub views: Vec<(String, Camera)>,
    pub background: [f32; 4],
    /// From the scene towards the first directional light.
    pub light_dir: Option<[f32; 3]>,
    pub light_intensity: f32,
    /// Number of the shading ramp the scene used.
    pub ramp: Option<i32>,
    /// Scene shadow colour, and how deep shadows are (0 to 1).
    pub shadow: Option<([f32; 3], f32)>,
    /// How dark and how wide lines are.
    pub line_depth: Option<f32>,
    pub line_width: Option<f32>,
}

impl Capture {
    pub fn parse(text: &str) -> Result<Self> {
        let v: Value = serde_json::from_str(text)?;
        let size: [f32; 2] = floats(&v["size"])?;
        let cam = &v["camera"];
        let fov_y_deg = cam["fov"].as_f64().context("camera fov")? as f32;
        let mut views = Vec::new();
        for view in v["views"].as_array().context("views")? {
            let eye = Vec3::from(floats::<3>(&view["position"])?);
            let rot = Quat::from_array(floats::<4>(&view["rotation"])?);
            let camera = Camera {
                eye,
                target: eye + rot * Vec3::Z,
                fov_y_deg,
            };
            views.push((view["name"].as_str().context("view name")?.to_owned(), camera));
        }
        let mut light_dir = None;
        let mut light_intensity = 1.0;
        for l in v["lights"].as_array().map(Vec::as_slice).unwrap_or_default() {
            if l["type"] == "Directional" {
                let d = Vec3::from(floats::<3>(&l["direction"])?);
                light_dir = Some((-d).to_array());
                light_intensity = l["intensity"].as_f64().unwrap_or(1.0) as f32;
                break;
            }
        }
        Ok(Self {
            width: size[0] as u32,
            height: size[1] as u32,
            views,
            background: floats(&cam["background"])?,
            light_dir,
            light_intensity,
            ramp: v["studio"]["ramp_g"].as_i64().map(|n| n as i32),
            // The game stores depth as one minus the value in alpha.
            shadow: floats::<4>(&v["globals"]["_ambientshadowG"])
                .ok()
                .map(|c| ([c[0], c[1], c[2]], 1.0 - c[3])),
            line_depth: floats::<4>(&v["globals"]["_LineColorG"]).ok().map(|c| 1.0 - c[3]),
            line_width: v["globals"]["_linewidthG"].as_f64().map(|n| n as f32),
        })
    }
}

/// How far two pictures of the same size are apart.
pub struct Difference {
    /// Pixels that are not background in either picture.
    pub covered: usize,
    /// Share of those that are background in exactly one picture.
    pub outline_mismatch: f32,
    /// Mean colour difference over the covered pixels, 0 to 255.
    pub mean: f32,
    /// One value per pixel, 0 to 255.
    pub map: Vec<u8>,
}

pub fn difference(a: &[u8], b: &[u8], background: [u8; 3]) -> Difference {
    let is_back = |p: &[u8]| (0..3).all(|c| p[c].abs_diff(background[c]) <= 6);
    let mut map = Vec::with_capacity(a.len() / 4);
    let (mut covered, mut one_sided, mut sum) = (0usize, 0usize, 0u64);
    for (pa, pb) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        let d = (0..3).map(|c| pa[c].abs_diff(pb[c]) as u32).sum::<u32>() / 3;
        map.push(d as u8);
        let (ba, bb) = (is_back(pa), is_back(pb));
        if ba && bb {
            continue;
        }
        covered += 1;
        sum += d as u64;
        if ba != bb {
            one_sided += 1;
        }
    }
    let n = covered.max(1) as f32;
    Difference {
        covered,
        outline_mismatch: one_sided as f32 / n,
        mean: sum as f32 / n,
        map,
    }
}

pub fn reference(card_path: &Path, capture_path: &Path, out: &Path) -> Result<()> {
    let capture_text = std::fs::read_to_string(capture_path)?;
    let cap = Capture::parse(&capture_text)?;
    let (w, h) = (cap.width, cap.height);
    let game = Game::open(&Config::load()?)?;
    let card = Card::load(card_path)?;
    let mut chara = rk_chara::build(&game, &card, &BuildOptions::default())?;
    let shaped = apply_shapes(&capture_shapes(&capture_text)?, &mut chara.scene);
    let gazes = apply_iris_placement(&capture_iris(&capture_text)?, &mut chara.scene);
    let draws = chara.scene.pose();
    let defaults = Environment::default();
    let (shadow_color, shadow_depth) = cap.shadow.unwrap_or((defaults.shadow_color, defaults.shadow_depth));
    let env = Environment {
        ramp: cap
            .ramp
            .and_then(|id| rk_chara::build::ramp(&game, id))
            .or_else(|| chara.ramp.clone()),
        shadow_color,
        shadow_depth,
        line_depth: cap.line_depth.unwrap_or(defaults.line_depth),
        line_width: cap.line_width.unwrap_or(defaults.line_width),
        light_dir: cap.light_dir,
        light_intensity: cap.light_intensity,
        ..defaults
    };
    println!(
        "  scene: ramp {:?}, shadow depth {:.2}, line depth {:.2}, line width {:.3}",
        cap.ramp, env.shadow_depth, env.line_depth, env.line_width
    );
    let back = cap.background.map(f64::from);
    let back8 = [0, 1, 2].map(|c| (cap.background[c] * 255.0 + 0.5) as u8);
    let mut renderer = Renderer::new()?;
    println!("{}: game, Renkatsu, difference", card.full_name());

    for (name, camera) in &cap.views {
        let picture = capture_path.with_extension(format!("{name}.plain.png"));
        let (gw, gh, game_px) = load_png(&std::fs::read(&picture).with_context(|| picture.display().to_string())?)?;
        if (gw, gh) != (w, h) {
            bail!("{} is {gw}x{gh}, the capture says {w}x{h}", picture.display());
        }
        let ours = renderer.render(&draws, camera, &env, w, h, back)?;
        let diff = difference(&game_px, &ours, back8);

        // White is equal, red is far apart.
        let sheet_w = 3 * w + 4 * GAP;
        let sheet_h = h + 2 * GAP;
        let mut sheet = [40u8, 40, 46, 255].repeat((sheet_w * sheet_h) as usize);
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let d = (diff.map[i] as u32 * 4).min(255) as u8;
                let panels = [
                    [game_px[i * 4], game_px[i * 4 + 1], game_px[i * 4 + 2], 255],
                    [ours[i * 4], ours[i * 4 + 1], ours[i * 4 + 2], 255],
                    [255, 255 - d, 255 - d, 255],
                ];
                for (n, px) in panels.iter().enumerate() {
                    let o = (((GAP + y) * sheet_w + GAP + n as u32 * (w + GAP) + x) * 4) as usize;
                    sheet[o..o + 4].copy_from_slice(px);
                }
            }
        }
        let stem = out.file_stem().unwrap_or_default().to_string_lossy();
        let file = out.with_file_name(format!("{stem}-{name}.png"));
        save_png(&file, sheet_w, sheet_h, &sheet)?;
        println!("  {name} -> {}", file.display());
        println!(
            "    character pixels: {} ({:.1}% of the picture)",
            diff.covered,
            100.0 * diff.covered as f32 / (w * h) as f32
        );
        println!(
            "    outline mismatch: {:.1}% of them are character in only one picture",
            100.0 * diff.outline_mismatch
        );
        println!("    mean colour difference: {:.1} of 255", diff.mean);
    }
    for warning in &chara.warnings {
        println!("  warning: {warning}");
    }
    println!(
        "  expression: blend shapes of {shaped} meshes and the iris placement of {gazes} eyes set from the capture"
    );
    compare_queues(&capture_text, &chara.scene)?;
    compare_bones(&capture_text, &chara.scene)?;
    Ok(())
}

/// A renderer name with its blend shape weights, as shape index and 0 to 1.
pub type Shapes = (String, Vec<(usize, f32)>);

/// Blend shape weights the game had set.
pub fn capture_shapes(capture: &str) -> Result<Vec<Shapes>> {
    let v: Value = serde_json::from_str(capture)?;
    let mut out = Vec::new();
    for r in v["renderers"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let (Some(name), Some(shapes)) = (r["name"].as_str(), r["shapes"].as_array()) else {
            continue;
        };
        let mut weights = Vec::new();
        for s in shapes {
            let [index, percent] = floats::<2>(s)?;
            weights.push((index as usize, percent / 100.0));
        }
        out.push((name.to_owned(), weights));
    }
    Ok(out)
}

/// Replaces the blend shape weights of the renderers the capture names.
/// Returns how many renderers were set.
fn apply_shapes(shapes: &[Shapes], scene: &mut rk_chara::Scene) -> usize {
    let mut count = 0;
    for (name, weights) in shapes {
        for r in scene.renderers.iter_mut().filter(|r| &r.name == name) {
            r.shape_weights = vec![0.0; r.mesh.blend_shapes.len()];
            for &(index, w) in weights {
                if let Some(slot) = r.shape_weights.get_mut(index) {
                    *slot = w;
                }
            }
            count += 1;
        }
    }
    count
}

/// Scale and offset the game had on each iris texture, per renderer name.
/// They include where the eyes were looking.
pub fn capture_iris(capture: &str) -> Result<Vec<(String, [f32; 4])>> {
    let v: Value = serde_json::from_str(capture)?;
    let mut out = Vec::new();
    for r in v["renderers"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let material = &r["materials"][0];
        let shader = material["shader"].as_str().unwrap_or_default().to_lowercase();
        // The iris shader, not the one for eye whites and lashes.
        if !shader.contains("eye") || shader.contains("eyew") {
            continue;
        }
        if let (Some(name), Some(_)) = (r["name"].as_str(), material["main_tex_st"].as_array()) {
            out.push((name.to_owned(), floats::<4>(&material["main_tex_st"])?));
        }
    }
    Ok(out)
}

/// Puts the captured scale and offset on the iris textures. The highlights
/// keep their distance from the iris. Returns how many eyes were set.
fn apply_iris_placement(placements: &[(String, [f32; 4])], scene: &mut rk_chara::Scene) -> usize {
    let mut count = 0;
    for (name, st) in placements {
        for r in scene.renderers.iter_mut().filter(|r| &r.name == name) {
            let Some(m) = r.materials.first_mut() else { continue };
            let Some((old, old_scale)) = m.textures.get("_MainTex").map(|t| (t.offset, t.scale)) else {
                continue;
            };
            // The scale does not depend on the gaze, so it checks our own rule.
            println!(
                "  iris {name}: scale from the card {:.4} x {:.4}, in the game {:.4} x {:.4}",
                old_scale[0], old_scale[1], st[0], st[1]
            );
            for (key, slot) in m.textures.iter_mut() {
                if !matches!(key.as_str(), "_MainTex" | "_overtex1" | "_overtex2") {
                    continue;
                }
                slot.scale = [st[0], st[1]];
                slot.offset = [st[2] + slot.offset[0] - old[0], st[3] + slot.offset[1] - old[1]];
            }
            count += 1;
        }
    }
    count
}

/// Checks the draw order numbers we set ourselves against the game's.
fn compare_queues(capture: &str, scene: &rk_chara::Scene) -> Result<()> {
    let v: Value = serde_json::from_str(capture)?;
    let (mut same, mut differ) = (0, Vec::new());
    for r in v["renderers"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let Some(name) = r["name"].as_str() else { continue };
        let Some(ours) = scene.renderers.iter().find(|x| x.name == name) else {
            continue;
        };
        for (i, m) in r["materials"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let (Some(game), Some(mine)) = (m["queue"].as_i64(), ours.materials.get(i)) else {
                continue;
            };
            // Only where we set a queue; the rest comes from the shader.
            if mine.render_queue <= 0 {
                continue;
            }
            if game == mine.render_queue as i64 {
                same += 1;
            } else {
                differ.push(format!("{name}[{i}] game {game}, ours {}", mine.render_queue));
            }
        }
    }
    println!("  draw order: {same} materials match the game, {} differ", differ.len());
    for line in differ.iter().take(12) {
        println!("    {line}");
    }
    Ok(())
}

/// Lists the bones whose place or shape differs most from the capture.
fn compare_bones(capture: &str, scene: &rk_chara::Scene) -> Result<()> {
    let v: Value = serde_json::from_str(capture)?;
    let (world, _) = scene.world();
    let mut ours = std::collections::HashMap::new();
    for (i, node) in scene.nodes.iter().enumerate() {
        // Names can repeat; keep the first, as the game's lookups do.
        ours.entry(node.name.as_str()).or_insert(world[i]);
    }
    let mut seen = std::collections::HashSet::new();
    let mut rows = Vec::new();
    let (mut matched, mut missing) = (0, 0);
    for b in v["bones"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let Some(name) = b["name"].as_str() else { continue };
        if !seen.insert(name) {
            continue;
        }
        let Some(mine) = ours.get(name) else {
            missing += 1;
            continue;
        };
        matched += 1;
        // The capture stores rows; glam stores columns.
        let game = glam::Mat4::from_cols_array(&floats::<16>(&b["world"])?).transpose();
        let moved = (game.w_axis - mine.w_axis).truncate().length();
        let shape = (0..3)
            .map(|c| (game.col(c) - mine.col(c)).truncate().length())
            .fold(0.0f32, f32::max);
        rows.push((moved, shape, name));
    }
    println!("  bones: {matched} compared, {missing} only in the game");
    rows.sort_by(|a, b| (b.0 + b.1 * 0.1).total_cmp(&(a.0 + a.1 * 0.1)));
    println!("  largest differences (position in units, rotation and scale as matrix distance):");
    for (moved, shape, name) in rows.iter().take(40) {
        println!("    {name:<28} moved {moved:.4}  turned or scaled {shape:.3}");
    }
    let off = rows.iter().filter(|r| r.0 > 0.002 || r.1 > 0.02).count();
    println!("  {off} of {} bones differ noticeably", rows.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "size": [4, 2],
        "camera": {"fov": 23, "background": [0.5, 0.5, 0.5, 1]},
        "views": [{"name": "body", "position": [0, 1, 8], "rotation": [0, 1, 0, 0]},
                  {"name": "face", "position": [0, 1.5, 2], "rotation": [0, 1, 0, 0]}],
        "studio": {"ramp_g": 2},
        "globals": {"_ambientshadowG": [0.5, 0.4, 0.3, 0.74], "_LineColorG": [0.5, 0.5, 0.5, 0], "_linewidthG": 0.307},
        "lights": [
            {"type": "Point", "direction": [1, 0, 0], "intensity": 3},
            {"type": "Directional", "direction": [0, -0.6, -0.8], "intensity": 2}
        ]
    }"#;

    #[test]
    fn capture_camera_and_light() {
        let c = Capture::parse(SAMPLE).unwrap();
        assert_eq!((c.width, c.height), (4, 2));
        // Turned half a circle around the vertical axis: looks down -Z.
        assert_eq!(c.views.len(), 2);
        let (name, camera) = &c.views[0];
        assert_eq!(name, "body");
        assert!((camera.target - Vec3::new(0.0, 1.0, 7.0)).length() < 1e-5);
        assert_eq!(camera.fov_y_deg, 23.0);
        assert_eq!(c.views[1].0, "face");
        let l = c.light_dir.unwrap();
        assert!((Vec3::from(l) - Vec3::new(0.0, 0.6, 0.8)).length() < 1e-6);
        assert_eq!(c.light_intensity, 2.0);
        assert_eq!(c.ramp, Some(2));
        let (color, depth) = c.shadow.unwrap();
        assert_eq!(color, [0.5, 0.4, 0.3]);
        assert!((depth - 0.26).abs() < 1e-6);
        assert_eq!(c.line_depth, Some(1.0));
        assert_eq!(c.line_width, Some(0.307));
    }

    #[test]
    fn capture_without_lights() {
        let text = SAMPLE.replace("\"Directional\"", "\"Spot\"");
        assert!(Capture::parse(&text).unwrap().light_dir.is_none());
        assert!(Capture::parse("{}").is_err());
    }

    #[test]
    fn capture_blend_shapes() {
        let text = r#"{"renderers": [
            {"name": "face", "shapes": [[0, 30], [1, 70]]},
            {"name": "shoe"},
            {"name": "brow", "shapes": []}
        ]}"#;
        let shapes = capture_shapes(text).unwrap();
        assert_eq!(shapes.len(), 2);
        assert_eq!(shapes[0], ("face".to_owned(), vec![(0, 0.3), (1, 0.7)]));
        assert_eq!(shapes[1], ("brow".to_owned(), vec![]));
        assert!(capture_shapes("{}").unwrap().is_empty());
    }

    #[test]
    fn capture_iris_placement() {
        let text = r#"{"renderers": [
            {"name": "iris_l", "materials": [{"shader": "Shader Forge/toon_eye_lod0", "main_tex_st": [1.05, 1.1, -0.04, -0.01]}]},
            {"name": "white_l", "materials": [{"shader": "Shader Forge/toon_eyew_lod0", "main_tex_st": [1, 1, 0, 0]}]},
            {"name": "shirt", "materials": [{"shader": "Shader Forge/main_opaque", "main_tex_st": [1, 1, 0, 0]}]},
            {"name": "bare", "materials": []}
        ]}"#;
        let iris = capture_iris(text).unwrap();
        assert_eq!(iris, vec![("iris_l".to_owned(), [1.05, 1.1, -0.04, -0.01])]);
    }

    #[test]
    fn difference_counts_only_the_character() {
        let back = [10, 10, 10];
        let b = [10, 10, 10, 255];
        let red = [110, 10, 10, 255];
        let a: Vec<u8> = [b, red, red, b].concat();
        let c: Vec<u8> = [b, red, b, [13, 10, 10, 255]].concat();
        let d = difference(&a, &c, back);
        // Pixel 0 and 3 are background in both (3 is within tolerance).
        assert_eq!(d.covered, 2);
        assert_eq!(d.outline_mismatch, 0.5);
        // Pixel 1 equal, pixel 2 differs by 100 in one of three channels.
        assert_eq!(d.mean, 16.5);
        assert_eq!(d.map, vec![0, 0, 33, 1]);
    }
}
