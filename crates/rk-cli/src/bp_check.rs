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

pub fn check(dir: &Path, only: &[String], bones_listed: bool) -> Result<()> {
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
    let mut pulls: HashMap<String, bp::MouthPull> = HashMap::new();
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
        // Renkatsu's body-following end, against Studio's at her waist.
        if mode != Mode::Oral {
            use rk_chara::body_collision as bc;
            let him_world_now = him.scene.world().0;
            let start = him_world_now[shaft.points[0]].w_axis.truncate();
            let options = bc::BodyOptions::default();
            let points = bc::CollisionPoints::find(&her_world, |n| names.get(n).copied(), &options);
            let up = names
                .get(mode.entry_bone())
                .map(|&b| her_world[b].y_axis.truncate().normalize_or_zero());
            let inner = names.get(bc::INNER_TARGET).map(|&b| her_world[b].w_axis.truncate());
            if let (Some(points), Some(up), Some(inner)) = (points, up, inner) {
                let dan = bc::Dan {
                    start,
                    base_length: shaft.base_length,
                    penetration: true,
                    bp_points: true,
                    last_end: Vec3::ZERO,
                    last_end_vector: Vec3::ZERO,
                };
                let partner = bc::Partner {
                    reference_position: entry,
                    reference_up: up,
                    inner_position: inner,
                    points: &points,
                };
                let t = bc::adjust_dan_to_target(mode, &dan, &partner, &settings, &options, 1.0 / 60.0);
                let turned = (end - entry).angle_between(t.end - entry).to_degrees();
                println!(
                    "  body-following end: {:.1} mm from Studio's, its direction {turned:.1} deg apart",
                    t.end.distance(end) * 1000.0
                );
            }
        }
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
        if bones_listed {
            for &p in shaft.points.iter().chain(&shaft.virtual_points) {
                let n = &him.scene.nodes[p].name;
                if let Some(b) = him_bones.get(n) {
                    let (os, or, _) = him_world[p].to_scale_rotation_translation();
                    let (gs, gr, _) = b.world.to_scale_rotation_translation();
                    println!(
                        "      {n:<16} scale ours {os:.4} studio {gs:.4}  turn {:.2} deg",
                        or.angle_between(gr).to_degrees()
                    );
                }
            }
        }
        println!("  shaft worst {:.2} mm at {}", worst.0, worst.1);
        // The colliders each of her dynamic bones got from him.
        let ours = shaft.partner_guests(&him.physics, &him.scene, &her.physics, &her.scene, mode, &settings);
        // Several sit on one object, so they are told apart by their root.
        let root_of = |b: usize| match &her.physics.bones[b] {
            rk_chara::physics::Bone::Dynamic(d) => d.def.root.map(|r| her.scene.nodes[r].name.clone()),
            _ => None,
        };
        let mut same = 0;
        let mut differ = Vec::new();
        for db in case.him["her_dynamic_bones"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let db_name = db["name"].as_str().unwrap_or_default();
            if !["Vagina", "cf_j_ana", "Belly"].iter().any(|k| db_name.contains(k)) {
                continue;
            }
            let root = db["root"].as_str().unwrap_or_default();
            let given = ours
                .iter()
                .find(|(b, _)| {
                    bp::bone_name(&her.physics, &her.scene, *b) == Some(db_name) && root_of(*b).as_deref() == Some(root)
                })
                .map_or(0, |(_, g)| g.len()) as i64;
            let studio = db["colliders"].as_i64().unwrap_or(-1);
            if studio == given {
                same += 1;
            } else {
                differ.push(format!("{db_name} ({root}): Studio {studio}, ours {given}"));
            }
        }
        println!("  her dynamic bones: {same} got the same colliders as in Studio");
        for d in differ {
            println!("    differs: {d}");
        }
        // Her vagina's bones once the dynamic bones settle on this pose,
        // with his colliders and without: how far each lands from where the
        // plugin's left it.
        if mode == Mode::Vaginal {
            let watched: Vec<(usize, String)> = her
                .scene
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| {
                    n.name.starts_with("cf_J_Vagina") && !n.name.contains("Pivot") && !n.name.contains("root")
                })
                .filter(|(_, n)| her_bones.contains_key(&n.name))
                .map(|(i, n)| (i, n.name.clone()))
                .collect();
            let settled = |her: &mut rk_chara::Character, guests: rk_chara::physics::Guests| {
                // As in Studio, she stands in the pose before the plugin
                // links, so her particles are in place when the shaft comes.
                pose(her, &her_bones);
                her.physics.set_guests(Vec::new());
                her.physics.settle(&mut her.scene, 120, rk_chara::physics::FRAME_TIME);
                her.physics.set_guests(guests);
                her.physics.settle(&mut her.scene, 120, rk_chara::physics::FRAME_TIME);
                let (world, _) = her.scene.world();
                let offs: Vec<f32> = watched
                    .iter()
                    .map(|(i, n)| (world[*i].w_axis - her_bones[n].world.w_axis).truncate().length() * 1000.0)
                    .collect();
                if bones_listed {
                    for ((i, n), o) in watched.iter().zip(&offs) {
                        let studio_local = her_bones[n].local.0;
                        println!(
                            "      {n:<28} {o:7.2} mm  ours local {:?}  studio local {:?}",
                            her.scene.nodes[*i].local.pos, studio_local
                        );
                    }
                }
                let worst = offs.iter().copied().fold(0.0, f32::max);
                let mean = offs.iter().sum::<f32>() / offs.len().max(1) as f32;
                (worst, mean)
            };
            let with = settled(her, ours.clone());
            let without = settled(her, Vec::new());
            println!(
                "  her {} vagina bones, mm from Studio's: with the shaft worst {:.2}, mean {:.2}; without it worst {:.2}, mean {:.2}",
                watched.len(),
                with.0,
                with.1,
                without.0,
                without.1
            );
        }
        // Her mouth through a sequence. The capture steps 1/30 s a frame
        // and keeps every third; between two kept frames the plugin's pull
        // adds up to the change of distance over them, unless it reached a
        // limit on the way. The first frame sets where ours starts.
        if mode == Mode::Oral {
            let (Some(mouth), Some(node)) = (her_bones.get(bp::MOUTH_BONE), her.scene.find(her.top, bp::MOUTH_BONE))
            else {
                continue;
            };
            let start = him_world[shaft.points[0]].w_axis.truncate();
            let along = bp::mouth_local(&her_world[node], (entry - start).normalize_or_zero());
            let at = mouth.local.0;
            let studio = at.dot(along);
            let distance = start.distance(entry);
            let pull = pulls.entry(sequence.clone()).or_insert_with(|| {
                let mut p = bp::MouthPull::new(shaft.base_length);
                p.amount = studio;
                p.last_distance = distance;
                p
            });
            pull.step_by(start, entry, shaft.base_length, true, &settings, 1.0 / 30.0);
            println!(
                "  mouth: Studio {:.2} mm along the shaft ({:.2} across), ours {:.2} mm; the root {:.1} mm from the entry",
                studio * 1000.0,
                (at - along * studio).length() * 1000.0,
                pull.amount * 1000.0,
                distance * 1000.0
            );
        }
    }
    Ok(())
}
