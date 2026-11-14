// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings saved by the Dynamic Bone Editor plugin: per accessory and root
//! bone, replacements for a DynamicBone's frozen axis, weight, damping,
//! elasticity, stiffness, inertia and radius.

use crate::physics::DynamicBoneDef;
use rk_card::{map_get, ExtData, Value};

pub const KEY: &str = "com.deathweasel.bepinex.dynamicboneeditor";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Edit {
    pub slot: usize,
    /// Name of the DynamicBone's root bone.
    pub bone: String,
    pub freeze_axis: Option<u8>,
    pub weight: Option<f32>,
    pub damping: Option<f32>,
    pub elasticity: Option<f32>,
    pub stiffness: Option<f32>,
    pub inertia: Option<f32>,
    pub radius: Option<f32>,
}

/// The edits for the accessories of one outfit.
pub fn for_outfit(ext: &ExtData, outfit: usize) -> Vec<Edit> {
    match ext.get(KEY).and_then(|p| p.nested("AccessoryDynamicBoneData")) {
        Some(list) => parse(&list, outfit),
        None => Vec::new(),
    }
}

fn parse(list: &Value, outfit: usize) -> Vec<Edit> {
    let float = |v: &Value, k: &str| map_get(v, k).and_then(Value::as_f64).map(|f| f as f32);
    list.as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .filter(|v| map_get(v, "CoordinateIndex").and_then(Value::as_u64) == Some(outfit as u64))
        .filter_map(|v| {
            Some(Edit {
                slot: map_get(v, "Slot")?.as_u64()? as usize,
                bone: map_get(v, "BoneName")?.as_str()?.to_string(),
                freeze_axis: map_get(v, "FreezeAxis").and_then(Value::as_u64).map(|a| a.min(3) as u8),
                weight: float(v, "Weight"),
                damping: float(v, "Damping"),
                elasticity: float(v, "Elasticity"),
                stiffness: float(v, "Stiffness"),
                inertia: float(v, "Inertia"),
                radius: float(v, "Radius"),
            })
        })
        .collect()
}

/// Applies the edits of an accessory slot to one of its DynamicBones, whose
/// root bone is named `root`. The plugin matches by that name.
pub fn apply(edits: &[Edit], slot: usize, root: &str, def: &mut DynamicBoneDef) {
    for e in edits.iter().filter(|e| e.slot == slot && e.bone == root) {
        if let Some(v) = e.freeze_axis {
            def.freeze_axis = v;
        }
        if let Some(v) = e.weight {
            def.weight = v;
        }
        if let Some(v) = e.damping {
            def.damping = v;
        }
        if let Some(v) = e.elasticity {
            def.elasticity = v;
        }
        if let Some(v) = e.stiffness {
            def.stiffness = v;
        }
        if let Some(v) = e.inertia {
            def.inert = v;
        }
        if let Some(v) = e.radius {
            def.radius = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(outfit: u64, slot: u64, bone: &str, extra: Vec<(Value, Value)>) -> Value {
        let mut pairs = vec![
            ("CoordinateIndex".into(), outfit.into()),
            ("Slot".into(), slot.into()),
            ("BoneName".into(), bone.into()),
            ("Damping".into(), Value::Nil),
        ];
        pairs.extend(extra);
        Value::Map(pairs)
    }

    #[test]
    fn edits_are_read_per_outfit_and_applied_by_slot_and_root() {
        let list = Value::Array(vec![
            entry(
                0,
                4,
                "tail_root",
                vec![("Stiffness".into(), 0.5.into()), ("FreezeAxis".into(), 2.into())],
            ),
            entry(1, 4, "tail_root", vec![("Stiffness".into(), 0.9.into())]),
        ]);
        let edits = parse(&list, 0);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].damping, None);

        let mut def = DynamicBoneDef::default();
        apply(&edits, 4, "tail_root", &mut def);
        assert_eq!(def.stiffness, 0.5);
        assert_eq!(def.freeze_axis, 2);
        // Untouched values keep the prefab's.
        assert_eq!(def.damping, DynamicBoneDef::default().damping);

        // Another slot or another root is left alone.
        let mut other = DynamicBoneDef::default();
        apply(&edits, 5, "tail_root", &mut other);
        apply(&edits, 4, "ribbon", &mut other);
        assert_eq!(other.stiffness, DynamicBoneDef::default().stiffness);
    }
}
