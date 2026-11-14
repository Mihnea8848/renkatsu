// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings saved by the Hair Accessory Customizer plugin for accessories
//! that carry a hair component: whether they take the character's hair
//! colours, keep the hair gloss, and their outline and ornament colours.

use rk_card::{map_get, ExtData, Value};
use std::collections::HashMap;

pub const KEY: &str = "com.deathweasel.bepinex.hairaccessorycustomizer";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HairAccessory {
    /// Take the colours of the character's first hair part.
    pub color_match: bool,
    /// Keep the hair gloss texture on the strands.
    pub hair_gloss: bool,
    /// Outline colour when the colours are not matched.
    pub outline_color: [f32; 4],
    /// Colour of the renderers in the hair component's `rendAccessory`.
    pub accessory_color: [f32; 4],
    /// Length rate given to the hair component.
    pub hair_length: f32,
}

/// The plugin's defaults for keys missing from an entry.
impl Default for HairAccessory {
    fn default() -> Self {
        Self {
            color_match: true,
            hair_gloss: true,
            outline_color: [0.0, 0.0, 0.0, 1.0],
            accessory_color: [1.0, 0.0, 0.0, 1.0],
            hair_length: 0.0,
        }
    }
}

/// A Unity colour as MessagePack writes it: four floats, or a map of them.
fn color(v: &Value) -> Option<[f32; 4]> {
    let f = |x: &Value| x.as_f64().map(|f| f as f32);
    match v {
        Value::Array(a) if a.len() >= 4 => Some([f(&a[0])?, f(&a[1])?, f(&a[2])?, f(&a[3])?]),
        Value::Map(_) => Some([
            f(map_get(v, "r")?)?,
            f(map_get(v, "g")?)?,
            f(map_get(v, "b")?)?,
            f(map_get(v, "a")?)?,
        ]),
        _ => None,
    }
}

fn entry(v: &Value) -> HairAccessory {
    let d = HairAccessory::default();
    let flag = |k: &str, def: bool| map_get(v, k).and_then(Value::as_bool).unwrap_or(def);
    HairAccessory {
        color_match: flag("ColorMatch", d.color_match),
        hair_gloss: flag("HairGloss", d.hair_gloss),
        outline_color: map_get(v, "OutlineColor").and_then(color).unwrap_or(d.outline_color),
        accessory_color: map_get(v, "AccessoryColor")
            .and_then(color)
            .unwrap_or(d.accessory_color),
        hair_length: map_get(v, "HairLength")
            .and_then(Value::as_f64)
            .map_or(d.hair_length, |f| f as f32),
    }
}

/// Settings for the accessories of one outfit, by accessory slot.
pub fn for_outfit(ext: &ExtData, outfit: usize) -> HashMap<usize, HairAccessory> {
    let Some(table) = ext.get(KEY).and_then(|p| p.nested("HairAccessories")) else {
        return HashMap::new();
    };
    parse(&table, outfit)
}

fn parse(table: &Value, outfit: usize) -> HashMap<usize, HairAccessory> {
    let Some(slots) = table
        .as_map()
        .and_then(|m| m.iter().find(|(k, _)| k.as_u64() == Some(outfit as u64)))
        .and_then(|(_, v)| v.as_map())
    else {
        return HashMap::new();
    };
    slots
        .iter()
        .filter_map(|(k, v)| Some((k.as_u64()? as usize, entry(v))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: Vec<(Value, Value)>) -> Value {
        Value::Map(pairs)
    }

    #[test]
    fn entries_are_read_per_outfit_and_slot() {
        let silver = Value::Array(vec![0.8.into(), 0.8.into(), 0.9.into(), 1.0.into()]);
        let info = map(vec![
            ("ColorMatch".into(), false.into()),
            ("HairGloss".into(), false.into()),
            ("OutlineColor".into(), silver.clone()),
            ("HairLength".into(), 0.25.into()),
        ]);
        let table = map(vec![(
            Value::from(0),
            map(vec![(Value::from(3), info), (Value::from(5), map(vec![]))]),
        )]);
        let got = parse(&table, 0);
        assert_eq!(got.len(), 2);
        let a = got[&3];
        assert!(!a.color_match && !a.hair_gloss);
        assert!((a.outline_color[2] - 0.9).abs() < 1e-6);
        assert_eq!(a.hair_length, 0.25);
        // Missing keys take the plugin's defaults.
        assert_eq!(a.accessory_color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(got[&5], HairAccessory::default());
        // Another outfit has no entries.
        assert!(parse(&table, 1).is_empty());
    }

    #[test]
    fn colours_can_be_maps() {
        let c = map(vec![
            ("r".into(), 0.1.into()),
            ("g".into(), 0.2.into()),
            ("b".into(), 0.3.into()),
            ("a".into(), 1.0.into()),
        ]);
        let got = color(&c).unwrap();
        assert!((got[1] - 0.2).abs() < 1e-6);
    }
}
