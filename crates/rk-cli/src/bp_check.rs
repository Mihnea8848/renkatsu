// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! `rk bp-check`: BetterPenetration against CharaStudio captures taken with
//! a partner (the capture plugin's `partner=` and `bp=` fields). Both
//! characters are posed from the capture's own bones, so the animation is
//! out of the comparison; then Renkatsu's targets and shaft aim run on top
//! and are measured against what the plugin did, in millimetres. The mouth
//! pull is followed through a sequence's frames, and the colliders each of
//! her dynamic bones received are counted against ours.

use anyhow::{Context, Result};
use glam::{Mat4, Quat, Vec3};
use rk_chara::better_penetration::{self as bp, Mode};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A captured bone: its local transform and its place in the world.
struct Bone {
    local: (Vec3, Quat, Vec3),
    world: Mat4,
}

fn floats(v: &Value) -> Vec<f32> {
    v.as_array()
        .map(|a| a.iter().filter_map(Value::as_f64).map(|f| f as f32).collect())
        .unwrap_or_default()
}

/// The bones of a capture by name, the first of each name as the game's
/// lookups find it.
fn bones(v: &Value) -> HashMap<String, Bone> {
    let mut out = HashMap::new();
    for b in v["bones"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let Some(name) = b["name"].as_str() else { continue };
        let (t, r, s, w) = (floats(&b["t"]), floats(&b["r"]), floats(&b["s"]), floats(&b["world"]));
        if t.len() != 3 || r.len() != 4 || s.len() != 3 || w.len() != 16 {
            continue;
        }
        out.entry(name.to_owned()).or_insert(Bone {
            local: (
                Vec3::from_slice(&t),
                Quat::from_xyzw(r[0], r[1], r[2], r[3]),
                Vec3::from_slice(&s),
            ),
            // The capture stores rows; glam stores columns.
            world: Mat4::from_cols_slice(&w).transpose(),
        });
    }
    out
}

/// Sets every bone the capture has to its captured local transform.
fn pose(chara: &mut rk_chara::Character, captured: &HashMap<String, Bone>) -> usize {
    let mut seen = std::collections::HashSet::new();
    let mut set = 0;
    for node in &mut chara.scene.nodes {
        if !seen.insert(node.name.clone()) {
            continue;
        }
        if let Some(b) = captured.get(&node.name) {
            node.local.pos = b.local.0;
            node.local.rot = b.local.1;
            node.local.scale = b.local.2;
            set += 1;
        }
    }
    set
}

/// The value of a field of the capture's list line.
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split('|')
        .map(str::trim)
        .find_map(|f| f.strip_prefix(key)?.strip_prefix('='))
}

/// How far, in millimetres, our bone is from the captured one.
fn off_mm(chara: &rk_chara::Character, world: &[Mat4], captured: &HashMap<String, Bone>, name: &str) -> Option<f32> {
    let node = chara.scene.find(chara.top, name)?;
    let game = captured.get(name)?.world.w_axis;
    Some((world[node].w_axis - game).truncate().length() * 1000.0)
}

struct Case {
    her: Value,
    him: Value,
}

fn read(base: &Path) -> Result<Case> {
    let text = |ext: &str| -> Result<Value> {
        let path = PathBuf::from(format!("{}{ext}", base.display()));
        let text = std::fs::read_to_string(&path).with_context(|| format!("could not read {}", path.display()))?;
        Ok(serde_json::from_str(&text)?)
    };
    Ok(Case {
        her: text(".json")?,
        him: text(".partner.json")?,
    })
}

pub fn check(dir: &Path, only: &[String]) -> Result<()> {
    let game = rk_vfs::Game::open_user(&[])?;
    let config = rk_core::Config::load()?;
    let mut bases: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?.strip_suffix(".partner.json")?.to_owned();
            Some(p.with_file_name(name))
        })
        .filter(|p| only.is_empty() || only.iter().any(|o| p.to_string_lossy().contains(o.as_str())))
        .collect();
    bases.sort();
    // The cases of one sequence share their characters, built once.
    let mut built: HashMap<String, (rk_chara::Character, rk_chara::Character)> = HashMap::new();
    for base in bases {
        let case = read(&base)?;
        let name = base.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
        let line = case.her["case"]["line"].as_str().unwrap_or_default().to_owned();
        let sequence = name.rsplit_once(".f").map_or(name.clone(), |(s, _)| s.to_owned());
        let mode = match field(&line, "bp") {
            Some("vaginal") => Mode::Vaginal,
            Some("anal") => Mode::Anal,
            Some("oral") => Mode::Oral,
            _ => {
                println!("{name}: no bp= in the case line");
                continue;
            }
        };
        if !built.contains_key(&sequence) {
            let card = line.split('|').next().unwrap_or_default().trim();
            let partner = field(&line, "partner").unwrap_or_default();
            let her_card = rk_card::Card::load(config.game_dir.join("UserData/chara/female").join(card))?;
            let him_card = rk_card::Card::load(config.game_dir.join("UserData/chara/male").join(partner))?;
            let her_opts = rk_chara::BuildOptions {
                uncensor_body: field(&line, "body").map(str::to_owned),
                ..Default::default()
            };
            let her = rk_chara::build(&game, &her_card, &her_opts)?;
            let him = rk_chara::build(&game, &him_card, &rk_chara::BuildOptions::default())?;
            built.insert(sequence.clone(), (her, him));
        }
        let (her, him) = built.get_mut(&sequence).expect("built above");
        let her_bones = bones(&case.her);
        let him_bones = bones(&case.him);
        let set = (pose(her, &her_bones), pose(him, &him_bones));
        let (her_world, _) = her.scene.world();
        println!(
            "{name}: {} posed from {} and {} captured bones; her hips {:.2} mm off",
            mode.label(),
            set.0,
            set.1,
            off_mm(her, &her_world, &her_bones, "cf_j_hips").unwrap_or(f32::NAN)
        );
        let link = &case.him["bp"];
        println!(
            "  Studio linked {} (entry {}, end {})",
            ["isKokan", "isAna", "isOral"]
                .iter()
                .filter(|k| link[**k].as_bool() == Some(true))
                .copied()
                .collect::<Vec<_>>()
                .join(", "),
            link["danEntryParentName"].as_str().unwrap_or("none"),
            link["danEndParentName"].as_str().unwrap_or("none"),
        );
        // The two targets, against where NodesConstraints held his.
        let names = bp::bones_by_name(&her.scene, her.top);
        let Some((entry, end)) = bp::targets(mode, &her_world, |n| names.get(n).copied()) else {
            println!("  no targets on her: her body lacks {}", mode.entry_bone());
            continue;
        };
        for (label, ours, bone) in [("entry", entry, "k_f_dan_entry"), ("end", end, "k_f_dan_end")] {
            if let Some(b) = him_bones.get(bone) {
                println!(
                    "  {label} target {:.2} mm off",
                    (b.world.w_axis.truncate() - ours).length() * 1000.0
                );
            }
        }
        // The shaft aimed as we aim it, point by point.
        let Some(shaft) = him.shaft.clone() else {
            println!("  his body has no BetterPenetration shaft here");
            continue;
        };
        let settings = him.better_penetration;
        shaft.aim(&mut him.scene, entry, end, &settings, mode == Mode::Vaginal);
        let (him_world, _) = him.scene.world();
        let mut worst = (0.0f32, String::new());
        let mut points = Vec::new();
        for &p in shaft.points.iter().chain(&shaft.virtual_points).chain(shaft.end.iter()) {
            let bone = him.scene.nodes[p].name.clone();
            if let Some(mm) = off_mm(him, &him_world, &him_bones, &bone) {
                points.push(format!(
                    "{}={mm:.1}",
                    bone.trim_start_matches("cm_J_").trim_end_matches("_00")
                ));
                if mm > worst.0 {
                    worst = (mm, bone);
                }
            }
        }
        println!("  shaft points, mm off: {}", points.join(" "));
        println!("  shaft worst {:.2} mm at {}", worst.0, worst.1);
        // The colliders each of her dynamic bones got from him.
        let ours = shaft.partner_guests(&him.physics, &him.scene, &her.physics, &her.scene, mode, &settings);
        for db in case.him["her_dynamic_bones"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let db_name = db["name"].as_str().unwrap_or_default();
            if !["Vagina", "cf_j_ana", "Belly"].iter().any(|k| db_name.contains(k)) {
                continue;
            }
            let given = ours
                .iter()
                .find(|(b, _)| bp::bone_name(&her.physics, &her.scene, *b) == Some(db_name))
                .map_or(0, |(_, g)| g.len());
            println!(
                "  her {db_name}: Studio {} colliders, ours {given}",
                db["colliders"].as_i64().unwrap_or(-1)
            );
        }
        // Where the plugin held her mouth, along the shaft and across it.
        if mode == Mode::Oral {
            let (Some(mouth), Some(node)) = (her_bones.get(bp::MOUTH_BONE), her.scene.find(her.top, bp::MOUTH_BONE))
            else {
                continue;
            };
            let start = him_world[shaft.points[0]].w_axis.truncate();
            let along = bp::mouth_local(&her_world[node], (entry - start).normalize_or_zero());
            let at = mouth.local.0;
            println!(
                "  mouth: Studio {:.2} mm along the shaft, {:.2} mm across; the shaft's root {:.1} mm from the entry",
                at.dot(along) * 1000.0,
                (at - along * at.dot(along)).length() * 1000.0,
                start.distance(entry) * 1000.0
            );
        }
    }
    Ok(())
}
