// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Compares the runtime state the reference capture plugin records (material
//! properties, textures, shadow flags, skinning bones) with the character
//! Renkatsu builds, and lists the differences as text. A value we do not
//! store is counted as unknown, not as a difference: Unity then uses the
//! shader's default, which the material may well match.

use rk_chara::Scene;
use serde_json::Value;
use std::collections::BTreeMap;

/// Largest difference two numbers may have and still count as equal.
const TOLERANCE: f32 = 0.002;

#[derive(Default)]
pub struct Report {
    pub compared: usize,
    pub unknown: usize,
    /// Differences grouped by kind, each with its lines.
    pub groups: BTreeMap<String, Vec<String>>,
}

impl Report {
    fn differ(&mut self, kind: &str, line: String) {
        self.groups.entry(kind.to_string()).or_default().push(line);
    }

    pub fn differences(&self) -> usize {
        self.groups.values().map(Vec::len).sum()
    }

    /// Adds another report's counts and lines to this one.
    pub fn merge(&mut self, other: Report) {
        self.compared += other.compared;
        self.unknown += other.unknown;
        for (kind, lines) in other.groups {
            self.groups.entry(kind).or_default().extend(lines);
        }
    }
}

/// Largest gap in percent two blend shape weights may have.
const SHAPE_TOLERANCE: f32 = 0.05;

/// Blend shape weights on clothing, game against ours, in percent. Face
/// shapes are left out: they come from Studio's expression, which `rk
/// reference` copies from the capture.
pub fn compare_clothing_shapes(capture: &Value, scene: &Scene) -> Report {
    let mut rep = Report::default();
    let rank = hierarchy_order(scene);
    let mut seen: BTreeMap<(String, String), usize> = BTreeMap::new();
    for r in capture["renderers"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let key = (
            r["name"].as_str().unwrap_or("").to_string(),
            bare(r["materials"][0]["name"].as_str().unwrap_or("")).to_string(),
        );
        let nth = seen.entry(key).or_insert(0);
        let ours = renderer_for(r, scene, &rank, *nth);
        *nth += 1;
        let Some(ours) = ours else { continue };
        if !matches!(ours.part, rk_chara::scene::Part::Clothes(_)) || ours.mesh.blend_shapes.is_empty() {
            continue;
        }
        let mut game = vec![0.0f32; ours.mesh.blend_shapes.len()];
        for s in r["shapes"].as_array().map(Vec::as_slice).unwrap_or_default() {
            if let (Some(i), Some(w)) = (s[0].as_u64(), s[1].as_f64()) {
                if let Some(slot) = game.get_mut(i as usize) {
                    *slot = w as f32;
                }
            }
        }
        let rname = r["name"].as_str().unwrap_or("");
        for (i, g) in game.iter().enumerate() {
            let mine = ours.shape_weights.get(i).copied().unwrap_or(0.0) * 100.0;
            rep.compared += 1;
            if (g - mine).abs() > SHAPE_TOLERANCE {
                let name = &ours.mesh.blend_shapes[i].name;
                rep.differ("blend shape", format!("{rname} {name}: game {g:.1}%, ours {mine:.1}%"));
            }
        }
    }
    rep
}

fn floats(v: &Value) -> Option<Vec<f32>> {
    match v {
        Value::Number(n) => Some(vec![n.as_f64()? as f32]),
        Value::Array(a) => a.iter().map(|x| x.as_f64().map(|f| f as f32)).collect(),
        _ => None,
    }
}

/// True when every component is within the tolerance.
fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= TOLERANCE)
}

fn show(v: &[f32]) -> String {
    let parts: Vec<String> = v.iter().map(|x| format!("{x:.3}")).collect();
    format!("({})", parts.join(", "))
}

/// Names Unity gives instanced materials, without the suffixes.
fn bare(name: &str) -> &str {
    name.split(" (Instance)").next().unwrap_or(name)
}

/// Each node's place in a depth-first walk of the scene, children in order:
/// the order `GetComponentsInChildren` lists renderers in, which the capture
/// keeps.
fn hierarchy_order(scene: &Scene) -> Vec<usize> {
    let mut rank = vec![usize::MAX; scene.nodes.len()];
    let mut next = 0;
    let mut stack: Vec<usize> = (0..scene.nodes.len())
        .filter(|&i| scene.nodes[i].parent.is_none())
        .rev()
        .collect();
    while let Some(n) = stack.pop() {
        rank[n] = next;
        next += 1;
        stack.extend(scene.nodes[n].children.iter().rev());
    }
    rank
}

/// Our renderer for a captured one: same name, and among several with that
/// name the ones whose first material matches. `nth` counts the captured
/// renderers seen before with the same name and first material, so two
/// accessories with the same mesh names pair up in hierarchy order.
pub fn renderer_for<'a>(r: &Value, scene: &'a Scene, rank: &[usize], nth: usize) -> Option<&'a rk_chara::RendererInst> {
    let name = r["name"].as_str()?;
    let first = bare(r["materials"][0]["name"].as_str().unwrap_or(""));
    // The game only lists renderers that are switched on.
    let mut same_name: Vec<_> = scene.renderers.iter().filter(|x| x.name == name && x.enabled).collect();
    same_name.sort_by_key(|x| rank.get(x.node).copied().unwrap_or(usize::MAX));
    let same_material: Vec<_> = same_name
        .iter()
        .copied()
        .filter(|x| x.materials.first().is_some_and(|m| m.name == first))
        .collect();
    same_material
        .get(nth)
        .or_else(|| same_material.first())
        .or_else(|| same_name.first())
        .copied()
}

/// Textures Renkatsu builds itself, whose names cannot match the game's.
fn built_here(name: &str) -> bool {
    name.ends_with("_composed") || name.starts_with("material_editor_") || name.contains("overlay")
}

/// Textures the capture lists without a name: ones built at run time, loose
/// PNGs Sideloader loads from a zipmod, and none at all, where Unity shows
/// the shader's default texture (our `default_*` images).
fn unnamed_in_game(name: &str) -> bool {
    built_here(name) || name.ends_with(".png") || name.starts_with("default_")
}

pub fn compare(capture: &Value, scene: &Scene) -> Report {
    let mut rep = Report::default();
    let rank = hierarchy_order(scene);
    let mut seen: BTreeMap<(String, String), usize> = BTreeMap::new();
    for r in capture["renderers"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let key = (
            r["name"].as_str().unwrap_or("").to_string(),
            bare(r["materials"][0]["name"].as_str().unwrap_or("")).to_string(),
        );
        let nth = seen.entry(key).or_insert(0);
        let ours = renderer_for(r, scene, &rank, *nth);
        *nth += 1;
        let Some(ours) = ours else { continue };
        let rname = r["name"].as_str().unwrap_or("");
        if let Some(cast) = r["cast"].as_str() {
            rep.compared += 1;
            let mine = format!("{:?}", ours.cast_shadows);
            if mine != cast {
                rep.differ("cast shadows", format!("{rname}: game {cast}, ours {mine}"));
            }
        }
        if let Some(receive) = r["receive"].as_bool() {
            rep.compared += 1;
            if receive != ours.receive_shadows {
                rep.differ(
                    "receive shadows",
                    format!("{rname}: game {receive}, ours {}", ours.receive_shadows),
                );
            }
        }
        if let Some(verts) = r["vertices"].as_array() {
            rep.compared += 1;
            let game: Vec<[f32; 3]> = verts
                .iter()
                .filter_map(floats)
                .filter(|v| v.len() == 3)
                .map(|v| [v[0], v[1], v[2]])
                .collect();
            let far = game
                .iter()
                .zip(&ours.mesh.positions)
                .map(|(a, b)| (0..3).map(|k| (a[k] - b[k]).abs()).fold(0.0f32, f32::max))
                .fold(0.0f32, f32::max);
            if game.len() != ours.mesh.positions.len() || far > 1e-4 {
                rep.differ(
                    "mesh vertices",
                    format!(
                        "{rname}: {} in the game, {} ours, apart by up to {far:.5}",
                        game.len(),
                        ours.mesh.positions.len()
                    ),
                );
            }
        }
        if let Some(bones) = r["bones"].as_array() {
            rep.compared += 1;
            let game: Vec<&str> = bones.iter().map(|b| b.as_str().unwrap_or("")).collect();
            let mine: Vec<&str> = ours
                .bones
                .iter()
                .map(|b| b.map(|i| scene.nodes[i].name.as_str()).unwrap_or(""))
                .collect();
            if game != mine {
                let first = game.iter().zip(&mine).position(|(a, b)| a != b);
                let detail = match first {
                    Some(i) => format!("first at {i}: game {:?}, ours {:?}", game[i], mine[i]),
                    None => format!("game has {}, ours {}", game.len(), mine.len()),
                };
                rep.differ("skinning bones", format!("{rname}: {detail}"));
            }
        }
        for (i, m) in r["materials"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let Some(mine) = ours.materials.get(i) else {
                rep.differ(
                    "missing material",
                    format!("{rname}[{i}] {}", bare(m["name"].as_str().unwrap_or(""))),
                );
                continue;
            };
            let at = format!("{rname}[{i}]");
            if let Some(shader) = m["shader"].as_str() {
                rep.compared += 1;
                // Unity fills an empty slot with its default material when the
                // renderer's materials are read; ours stays empty.
                let placeholder = mine.absent && shader == "Standard";
                if shader != mine.shader && !placeholder {
                    rep.differ("shader", format!("{at}: game {shader}, ours {}", mine.shader));
                }
            }
            for (prop, value) in m["props"].as_object().into_iter().flatten() {
                let Some(game) = floats(value) else { continue };
                let ours_value: Option<Vec<f32>> = if game.len() == 1 {
                    mine.floats.get(prop).map(|f| vec![*f])
                } else {
                    mine.colors.get(prop).map(|c| c.to_vec())
                };
                let Some(ours_value) = ours_value else {
                    rep.unknown += 1;
                    continue;
                };
                rep.compared += 1;
                if !close(&game, &ours_value) {
                    rep.differ(
                        "property",
                        format!("{at} {prop}: game {}, ours {}", show(&game), show(&ours_value)),
                    );
                }
            }
            for (prop, t) in m["textures"].as_object().into_iter().flatten() {
                let game_name = t["name"].as_str().unwrap_or("");
                let slot = mine.textures.get(prop);
                match (game_name.is_empty(), slot) {
                    (true, None) => {}
                    (true, Some(s)) if unnamed_in_game(&s.image.name) => {}
                    (true, Some(s)) => {
                        rep.compared += 1;
                        rep.differ("texture", format!("{at} {prop}: game none, ours {}", s.image.name));
                    }
                    (false, None) => {
                        rep.compared += 1;
                        rep.differ("texture", format!("{at} {prop}: game {game_name}, ours none"));
                    }
                    (false, Some(s)) => {
                        if !built_here(&s.image.name) {
                            rep.compared += 1;
                            if !s.image.name.eq_ignore_ascii_case(game_name) {
                                rep.differ(
                                    "texture",
                                    format!("{at} {prop}: game {game_name}, ours {}", s.image.name),
                                );
                            }
                        }
                        if let Some(st) = floats(&t["st"]) {
                            rep.compared += 1;
                            let mine_st = [s.scale[0], s.scale[1], s.offset[0], s.offset[1]];
                            if !close(&st, &mine_st) {
                                rep.differ(
                                    "texture placement",
                                    format!("{at} {prop}: game {}, ours {}", show(&st), show(&mine_st)),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    rep
}

/// Prints the summary and the first lines of each kind of difference.
pub fn print(rep: &Report, per_kind: usize) {
    println!(
        "  state: {} values compared, {} differ, {} not stored by us",
        rep.compared,
        rep.differences(),
        rep.unknown
    );
    for (kind, lines) in &rep.groups {
        println!("    {kind}: {}", lines.len());
        for l in lines.iter().take(per_kind) {
            println!("      {l}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_colours_compare_within_tolerance() {
        assert!(close(&[0.5, 0.25], &[0.501, 0.2495]));
        assert!(!close(&[0.5], &[0.51]));
        assert!(!close(&[0.5, 0.5], &[0.5]));
        assert_eq!(floats(&serde_json::json!(0.75)), Some(vec![0.75]));
        assert_eq!(floats(&serde_json::json!([1, 0.5])), Some(vec![1.0, 0.5]));
        assert_eq!(floats(&serde_json::json!("x")), None);
    }

    #[test]
    fn instance_suffixes_and_built_textures_are_recognised() {
        assert_eq!(bare("cf_m_body (Instance) (Instance)"), "cf_m_body");
        assert_eq!(bare("hair.MECopy1 (Instance)"), "hair.MECopy1");
        assert!(built_here("clothes_composed"));
        assert!(built_here("material_editor_5"));
        assert!(!built_here("cf_body_00_t"));
        assert!(unnamed_in_game("kaguya.png"));
        assert!(unnamed_in_game("default_black"));
        assert!(!unnamed_in_game("cf_body_00_t"));
    }

    #[test]
    fn renderers_are_ranked_in_hierarchy_order() {
        let node = |name: &str, parent: Option<usize>, children: Vec<usize>| rk_chara::scene::Node {
            name: name.into(),
            parent,
            children,
            local: Default::default(),
            active: true,
        };
        // 0 has children 2 then 1; a depth-first walk visits 0, 2, 3, 1.
        let scene = Scene {
            nodes: vec![
                node("root", None, vec![2, 1]),
                node("late", Some(0), vec![]),
                node("early", Some(0), vec![3]),
                node("inner", Some(2), vec![]),
            ],
            ..Default::default()
        };
        assert_eq!(hierarchy_order(&scene), vec![0, 3, 1, 2]);
    }

    #[test]
    fn an_empty_capture_compares_nothing() {
        let rep = compare(&serde_json::json!({"renderers": []}), &Scene::default());
        assert_eq!((rep.compared, rep.differences(), rep.unknown), (0, 0, 0));
    }
}
