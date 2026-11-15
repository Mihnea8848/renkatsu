// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

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

/// Our renderer for a captured one: same name, and among several with that
/// name the one whose first material matches.
pub fn renderer_for<'a>(r: &Value, scene: &'a Scene) -> Option<&'a rk_chara::RendererInst> {
    let name = r["name"].as_str()?;
    let first = bare(r["materials"][0]["name"].as_str().unwrap_or(""));
    // The game only lists renderers that are switched on.
    let mut same_name = scene.renderers.iter().filter(|x| x.name == name && x.enabled);
    same_name
        .clone()
        .find(|x| x.materials.first().is_some_and(|m| m.name == first))
        .or_else(|| same_name.next())
}

/// Textures Renkatsu builds itself, whose names cannot match the game's.
fn built_here(name: &str) -> bool {
    name.ends_with("_composed") || name.starts_with("material_editor_") || name.contains("overlay")
}

pub fn compare(capture: &Value, scene: &Scene) -> Report {
    let mut rep = Report::default();
    for r in capture["renderers"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let Some(ours) = renderer_for(r, scene) else { continue };
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
                    // The game's runtime-built textures (CustomTextureCreate)
                    // have no name; ours built the same way are not a difference.
                    (true, Some(s)) if built_here(&s.image.name) => {}
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
    }

    #[test]
    fn an_empty_capture_compares_nothing() {
        let rep = compare(&serde_json::json!({"renderers": []}), &Scene::default());
        assert_eq!((rep.compared, rep.differences(), rep.unknown), (0, 0, 0));
    }
}
