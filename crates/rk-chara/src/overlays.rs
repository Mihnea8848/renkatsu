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

/// Entry for an outfit in a per-outfit table. A table that is not split by
/// outfit has one entry under a key that is not a number, and that entry
/// applies to all outfits. An entry for another outfit never does.
fn for_outfit(table: &Value, outfit: usize) -> Option<&Value> {
    let entries = table.as_map()?;
    entries
        .iter()
        .find(|(k, _)| k.as_i64() == Some(outfit as i64))
        .or_else(|| {
            entries
                .first()
                .filter(|(k, _)| entries.len() == 1 && k.as_i64().is_none())
        })
        .map(|(_, v)| v)
}

fn bytes<'a>(plugin: &'a PluginData, key: &str) -> Option<&'a [u8]> {
    plugin.bytes(key).filter(|b| !b.is_empty())
}

/// Data of the retired KK_CharaOverlaysBasedOnCoordinate plugin, which the
/// overlay plugin imports for cards that have none of its own.
pub const COBOC_KEY: &str = "com.jim60105.kk.charaoverlaysbasedoncoordinate";

/// `KoiSkinOverlayController.TryImportCOBOC`: the PNG of each overlay
/// kind for `outfit`. `AllCharaOverlayTable` maps outfit and kind to an
/// index into `AllCharaResources`; a kind an outfit lacks is taken from
/// the first outfit stored. `None` when the data is not there or broken,
/// in which case the plugin does not import.
pub fn coboc(ext: &ExtData, outfit: usize) -> Option<HashMap<i64, Vec<u8>>> {
    let plugin = ext.get(COBOC_KEY)?;
    let Value::Map(table) = plugin.data.get("AllCharaOverlayTable")? else {
        return None;
    };
    let Value::Array(resources) = plugin.nested("AllCharaResources")? else {
        return None;
    };
    let kinds_of = |entry: &Value| -> Option<HashMap<i64, Vec<u8>>> {
        let Value::Map(kinds) = entry else { return None };
        kinds
            .iter()
            .map(|(k, id)| {
                let Value::Binary(png) = resources.get(id.as_u64()? as usize)? else {
                    return None;
                };
                Some((k.as_i64()?, png.clone()))
            })
            .collect()
    };
    // Every outfit is converted, and one bad entry fails the import.
    let mut first: Option<HashMap<i64, Vec<u8>>> = None;
    let mut wanted = HashMap::new();
    for (k, entry) in table {
        let mut kinds = kinds_of(entry)?;
        match &first {
            None => first = Some(kinds.clone()),
            Some(first) => {
                for (kind, png) in first {
                    kinds.entry(*kind).or_insert_with(|| png.clone());
                }
            }
        }
        if k.as_u64() == Some(outfit as u64) {
            wanted = kinds;
        }
    }
    Some(wanted)
}

/// Skin overlays for an outfit, by kind.
pub fn skin(ext: &ExtData, outfit: usize) -> HashMap<i64, Arc<Image>> {
    let mut out = HashMap::new();
    let mut put = |kind: i64, data: &[u8]| {
        if let Some(image) = decode(data, format!("skin_overlay_{kind}")) {
            out.insert(kind, Arc::new(image));
        }
    };
    // Without data of its own, or with data in the legacy layout, the
    // plugin first tries the other plugin's.
    let own = ext.get(SKIN_KEY);
    let imported = if own.is_none_or(|p| p.version <= 1) {
        coboc(ext, outfit)
    } else {
        None
    };
    if let Some(imported) = &imported {
        for (kind, png) in imported {
            if !png.is_empty() {
                put(*kind, png);
            }
        }
    }
    let Some(plugin) = own.filter(|_| imported.is_none()) else {
        return both_eyes(out);
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
    both_eyes(out)
}

/// "Both eyes" entries stand in for the per-eye ones.
fn both_eyes(mut out: HashMap<i64, Arc<Image>>) -> HashMap<i64, Arc<Image>> {
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

/// Names the plugin gives the masks a top brings (its MaskKind), in the
/// order body, bra, inner layer.
pub const MASK_IDS: [&str; 3] = ["BodyMask", "BraMask", "InnerMask"];

/// The mask a material ends up with. The plugin hooks Material.SetTexture
/// for `_AlphaMask` on the body, the bra and the top's inner layer and swaps
/// in its own image when the card has one, whatever the game passed.
pub fn mask(overlays: &HashMap<String, ClothOverlay>, kind: &str, game: Option<Arc<Image>>) -> Option<Arc<Image>> {
    overlays.get(kind).and_then(|o| o.image.clone()).or(game)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_plugins_overlays_are_imported() {
        use rk_card::PluginData;
        let table = |pairs: Vec<(i64, Vec<(i64, i64)>)>| {
            Value::Map(
                pairs
                    .into_iter()
                    .map(|(outfit, kinds)| {
                        let kinds = kinds.into_iter().map(|(k, id)| (k.into(), id.into())).collect();
                        (outfit.into(), Value::Map(kinds))
                    })
                    .collect(),
            )
        };
        let ext = |table: Value| {
            let resources = Value::Array(vec![
                Value::Binary(Vec::new()),
                Value::Binary(vec![1]),
                Value::Binary(vec![2]),
            ]);
            let mut bytes = Vec::new();
            rmpv::encode::write_value(&mut bytes, &resources).unwrap();
            let mut p = PluginData::default();
            p.data.insert("AllCharaOverlayTable".into(), table);
            p.data.insert("AllCharaResources".into(), Value::Binary(bytes));
            let mut ext = ExtData::new();
            ext.insert(COBOC_KEY.into(), p);
            ext
        };
        let e = ext(table(vec![(0, vec![(1, 1), (2, 0)]), (1, vec![(1, 2)])]));
        let first = coboc(&e, 0).unwrap();
        assert_eq!(first[&1], vec![1]);
        assert!(first[&2].is_empty());
        // The second outfit lacks kind 2 and takes the first outfit's.
        let second = coboc(&e, 1).unwrap();
        assert_eq!(second[&1], vec![2]);
        assert!(second.contains_key(&2));
        // An outfit that is not stored has nothing.
        assert!(coboc(&e, 5).unwrap().is_empty());
        // An index past the resources fails the import as a whole.
        assert!(coboc(&ext(table(vec![(0, vec![(1, 1)]), (1, vec![(1, 9)])])), 0).is_none());
        assert!(coboc(&ExtData::new(), 0).is_none());
        // With current data of its own the plugin does not import.
        let mut both = e.clone();
        both.insert(
            SKIN_KEY.into(),
            PluginData {
                version: 2,
                ..Default::default()
            },
        );
        assert!(skin(&both, 0).is_empty());
    }

    #[test]
    fn a_saved_mask_replaces_the_games() {
        let image = |name: &str| {
            Arc::new(Image {
                name: name.into(),
                width: 1,
                height: 1,
                rgba: vec![255; 4],
                srgb: true,
                repeat: true,
            })
        };
        let mut saved = HashMap::new();
        saved.insert(
            "InnerMask".to_string(),
            ClothOverlay {
                image: Some(image("saved")),
                replace: false,
            },
        );
        // An entry without an image, as "replace" alone leaves, changes nothing.
        saved.insert(
            "BraMask".to_string(),
            ClothOverlay {
                image: None,
                replace: true,
            },
        );
        let name = |kind: &str, game: Option<Arc<Image>>| mask(&saved, kind, game).map(|i| i.name.clone());
        assert_eq!(name("InnerMask", Some(image("game"))).as_deref(), Some("saved"));
        // The plugin's image is used even when the item brings no mask.
        assert_eq!(name("InnerMask", None).as_deref(), Some("saved"));
        assert_eq!(name("BodyMask", Some(image("game"))).as_deref(), Some("game"));
        assert_eq!(name("BraMask", Some(image("game"))).as_deref(), Some("game"));
        assert_eq!(name("BodyMask", None), None);
    }

    #[test]
    fn an_entry_for_another_outfit_does_not_apply() {
        let entry = |key: Value| Value::Map(vec![(key, Value::from("data"))]);

        // Only outfit 6 has an entry.
        let table = entry(Value::from(6));
        assert!(for_outfit(&table, 0).is_none());
        assert_eq!(for_outfit(&table, 6), Some(&Value::from("data")));

        // One entry under a name is shared by all outfits.
        let shared = entry(Value::from("all"));
        assert_eq!(for_outfit(&shared, 3), Some(&Value::from("data")));

        // Several entries: only an exact match counts.
        let two = Value::Map(vec![
            (Value::from(0), Value::from("a")),
            (Value::from(1), Value::from("b")),
        ]);
        assert_eq!(for_outfit(&two, 1), Some(&Value::from("b")));
        assert!(for_outfit(&two, 2).is_none());
    }
}
