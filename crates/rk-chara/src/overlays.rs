// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Images saved by the overlay plugins. Skin overlays (KSOX) are painted
//! under or over the built body, face and eye textures. Clothes overlays
//! (KCOX) are painted onto a clothing piece's built textures, replace them,
//! or replace its colour mask.

use crate::material_editor::decode;
use crate::scene::Image;
use rk_card::{ExtData, PluginData, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub const SKIN_KEY: &str = "KSOX";
pub const CLOTHES_KEY: &str = "KCOX";

/// Where a skin overlay goes. The numbers are the plugin's own.
pub mod kind {
    pub const BODY_OVER: i64 = 1;
    pub const FACE_OVER: i64 = 2;
    pub const BODY_UNDER: i64 = 3;
    pub const FACE_UNDER: i64 = 4;
    pub const EYE_UNDER: i64 = 5;
    pub const EYE_OVER: i64 = 6;
    pub const EYE_UNDER_L: i64 = 7;
    pub const EYE_OVER_L: i64 = 8;
    pub const EYE_UNDER_R: i64 = 9;
    pub const EYE_OVER_R: i64 = 10;
    pub const EYEBROW_UNDER: i64 = 20;
    pub const EYELINE_UNDER: i64 = 30;

    pub fn from_name(name: &str) -> Option<i64> {
        Some(match name {
            "BodyOver" => BODY_OVER,
            "FaceOver" => FACE_OVER,
            "BodyUnder" => BODY_UNDER,
            "FaceUnder" => FACE_UNDER,
            "EyeUnder" => EYE_UNDER,
            "EyeOver" => EYE_OVER,
            _ => return None,
        })
    }
}

/// Entry for an outfit in a per-outfit table. Tables that are not split by
/// outfit only have one entry, which then applies to all of them.
fn for_outfit(table: &Value, outfit: usize) -> Option<&Value> {
    let entries = table.as_map()?;
    entries
        .iter()
        .find(|(k, _)| k.as_i64() == Some(outfit as i64))
        .or_else(|| entries.first().filter(|_| entries.len() == 1))
        .map(|(_, v)| v)
}

fn bytes<'a>(plugin: &'a PluginData, key: &str) -> Option<&'a [u8]> {
    plugin.bytes(key).filter(|b| !b.is_empty())
}

/// Skin overlays for an outfit, by kind.
pub fn skin(ext: &ExtData, outfit: usize) -> HashMap<i64, Arc<Image>> {
    let mut out = HashMap::new();
    let Some(plugin) = ext.get(SKIN_KEY) else { return out };
    let mut put = |kind: i64, data: &[u8]| {
        if let Some(image) = decode(data, format!("skin_overlay_{kind}")) {
            out.insert(kind, Arc::new(image));
        }
    };
    match plugin.nested("Lookup") {
        // Current layout: outfit -> kind -> texture id, textures stored apart.
        Some(table) => {
            let Some(Value::Map(kinds)) = for_outfit(&table, outfit) else {
                return out;
            };
            for (k, id) in kinds {
                let (Some(k), Some(id)) = (k.as_i64(), id.as_i64()) else {
                    continue;
                };
                if let Some(data) = bytes(plugin, &format!("_TextureID_{id}")) {
                    put(k, data);
                }
            }
        }
        // Oldest layout: one image per kind, keyed by the kind's name.
        None => {
            for (name, value) in &plugin.data {
                if let (Some(k), Value::Binary(data)) = (kind::from_name(name), value) {
                    if !data.is_empty() {
                        put(k, data);
                    }
                }
            }
        }
    }
    // "Both eyes" entries stand in for the per-eye ones.
    for (both, sides) in [
        (kind::EYE_UNDER, [kind::EYE_UNDER_L, kind::EYE_UNDER_R]),
        (kind::EYE_OVER, [kind::EYE_OVER_L, kind::EYE_OVER_R]),
    ] {
        if let Some(image) = out.get(&both).cloned() {
            for side in sides {
                out.entry(side).or_insert_with(|| image.clone());
            }
        }
    }
    out
}

#[derive(Clone)]
pub struct ClothOverlay {
    pub image: Option<Arc<Image>>,
    /// Replace the piece's texture instead of painting over it.
    pub replace: bool,
}

/// Name the plugin uses for the colour mask of a clothing piece.
pub fn mask_id(clothes_id: &str) -> String {
    format!("Colormask_{clothes_id}")
}

/// Names the plugin gives the nine clothing slots and the three sub-parts.
pub const MAIN_IDS: [&str; 9] = [
    "ct_clothesTop",
    "ct_clothesBot",
    "ct_bra",
    "ct_shorts",
    "ct_gloves",
    "ct_panst",
    "ct_socks",
    "ct_shoes_inner",
    "ct_shoes_outer",
];
pub const SUB_IDS: [&str; 3] = ["ct_top_parts_A", "ct_top_parts_B", "ct_top_parts_C"];

/// Clothes overlays for an outfit, by clothing id.
pub fn clothes(ext: &ExtData, outfit: usize) -> HashMap<String, ClothOverlay> {
    let mut out = HashMap::new();
    let Some(table) = ext.get(CLOTHES_KEY).and_then(|p| p.nested("Overlays")) else {
        return out;
    };
    // Per outfit when the keys are numbers, one shared set when they are names.
    let per_outfit = table
        .as_map()
        .is_some_and(|m| m.iter().any(|(k, _)| k.as_i64().is_some()));
    let Some(Value::Map(entries)) = (if per_outfit {
        for_outfit(&table, outfit)
    } else {
        Some(&table)
    }) else {
        return out;
    };
    for (name, data) in entries {
        let (Some(name), Some(fields)) = (name.as_str(), data.as_array()) else {
            continue;
        };
        let image = match fields.first() {
            Some(Value::Binary(b)) if !b.is_empty() => decode(b, format!("clothes_overlay_{name}")).map(Arc::new),
            _ => None,
        };
        let replace = fields.get(1).and_then(Value::as_bool).unwrap_or(false);
        if image.is_some() || replace {
            out.insert(name.to_string(), ClothOverlay { image, replace });
        }
    }
    out
}
