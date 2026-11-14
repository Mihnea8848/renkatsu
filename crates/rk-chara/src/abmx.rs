// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bone modifiers saved by the ABMX plugin: per-bone scale, length, offset
//! and rotation applied on top of the slider result.

use crate::scene::{euler_deg, Scene};
use glam::Vec3;
use rk_card::{ExtData, Value};
use std::collections::HashMap;

pub const KEY: &str = "KKABMPlugin.ABMData";

#[derive(Debug, Clone, Copy)]
pub struct Modifier {
    pub scale: Vec3,
    pub length: f32,
    pub position: Vec3,
    pub rotation: Vec3,
}

#[derive(Debug, Clone)]
pub struct BoneModifier {
    pub bone: String,
    /// One entry shared by every outfit, or one per outfit.
    pub per_outfit: Vec<Modifier>,
    /// 0 unknown, 1 body, 10 and up: accessory slot + 10.
    pub location: i64,
}

impl BoneModifier {
    /// The entry for `outfit`: the shared one, or that outfit's own.
    pub fn for_outfit(&self, outfit: usize) -> Option<&Modifier> {
        if self.per_outfit.len() == 1 {
            self.per_outfit.first()
        } else {
            self.per_outfit.get(outfit)
        }
    }
}

/// Whether a body modifier changes the scale of `bone` in `outfit`
/// (`BoneModifierData.HasScale`).
pub fn scales(mods: &[BoneModifier], outfit: usize, bone: &str) -> bool {
    mods.iter()
        .filter(|m| m.location < 10 && m.bone == bone)
        .filter_map(|m| m.for_outfit(outfit))
        .any(|d| d.scale != Vec3::ONE)
}

fn vec3(v: &Value) -> Option<Vec3> {
    let a = v.as_array()?;
    Some(Vec3::new(
        a.first()?.as_f64()? as f32,
        a.get(1)?.as_f64()? as f32,
        a.get(2)?.as_f64()? as f32,
    ))
}

fn modifier(v: &Value) -> Option<Modifier> {
    let a = v.as_array()?;
    Some(Modifier {
        scale: vec3(a.first()?)?,
        length: a.get(1)?.as_f64()? as f32,
        position: a.get(2).and_then(vec3).unwrap_or(Vec3::ZERO),
        rotation: a.get(3).and_then(vec3).unwrap_or(Vec3::ZERO),
    })
}

/// Reads the modifiers stored in a card.
pub fn read(ext: &ExtData) -> Vec<BoneModifier> {
    let Some(plugin) = ext.get(KEY) else { return Vec::new() };
    let Some(data) = plugin.lz4("boneData") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match data {
        // List of [name, modifiers] with an optional location.
        Value::Array(items) => {
            for item in items {
                let Some(a) = item.as_array() else { continue };
                let (Some(name), Some(mods)) = (a.first().and_then(Value::as_str), a.get(1).and_then(Value::as_array))
                else {
                    continue;
                };
                out.push(BoneModifier {
                    bone: name.to_string(),
                    per_outfit: mods.iter().filter_map(modifier).collect(),
                    location: a.get(2).and_then(Value::as_i64).unwrap_or(0),
                });
            }
        }
        // Oldest layout: map of bone name to a single modifier.
        Value::Map(entries) => {
            for (k, v) in entries {
                if let (Some(name), Some(m)) = (k.as_str(), modifier(&v)) {
                    out.push(BoneModifier {
                        bone: name.to_string(),
                        per_outfit: vec![m],
                        location: 0,
                    });
                }
            }
        }
        _ => {}
    }
    out
}

/// Applies modifiers to the bones in `bones` (and, for accessory-scoped
/// ones, to the nodes of that accessory), using each bone's current local
/// transform as the baseline.
pub fn apply(
    scene: &mut Scene,
    mods: &[BoneModifier],
    outfit: usize,
    bones: &HashMap<String, usize>,
    accessories: &HashMap<usize, HashMap<String, usize>>,
) {
    for m in mods {
        let Some(data) = m.for_outfit(outfit) else { continue };
        let node = if m.location >= 10 {
            accessories
                .get(&((m.location - 10) as usize))
                .and_then(|a| a.get(&m.bone))
        } else {
            bones.get(&m.bone)
        };
        let Some(&node) = node else { continue };
        let local = &mut scene.nodes[node].local;
        local.scale *= data.scale;
        if data.rotation != Vec3::ZERO {
            local.rot *= euler_deg(data.rotation.x, data.rotation.y, data.rotation.z);
        }
        local.pos = local.pos * data.length + data.position;
    }
}
