// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Dynamic Bone Distribution Editor plugin, for accessories. Per outfit
//! the card stores `AccessoryEdits<outfit>`: a list of entries, each keyed by
//! the accessory slot and the path from the accessory's object down to the
//! DynamicBone's root bone, with the edit as a list of separately packed
//! fields. Loading an edit sets the DynamicBone's force, weight and enabled
//! state when the field is there; the distribution curves and base values it
//! can also hold are not read yet, and no card in the test set uses them.

use rk_card::{ExtData, Value};

pub const KEY: &str = "org.njaecha.plugins.dbde";

#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub slot: usize,
    /// Path from the accessory's object to the DynamicBone's root bone.
    pub path: String,
    pub force: Option<[f32; 3]>,
    pub weight: Option<f32>,
    /// `SetActive` with field 6.
    pub active: Option<bool>,
}

/// One packed field, decoded; empty fields count as missing.
fn field(blobs: &[Value], i: usize) -> Option<Value> {
    match blobs.get(i)? {
        Value::Binary(b) if !b.is_empty() => rmpv::decode::read_value(&mut &b[..]).ok(),
        _ => None,
    }
}

fn vec3(v: &Value) -> Option<[f32; 3]> {
    let f = |x: &Value| x.as_f64().map(|f| f as f32);
    match v {
        Value::Array(a) if a.len() >= 3 => Some([f(&a[0])?, f(&a[1])?, f(&a[2])?]),
        Value::Map(_) => Some([
            f(rk_card::map_get(v, "x")?)?,
            f(rk_card::map_get(v, "y")?)?,
            f(rk_card::map_get(v, "z")?)?,
        ]),
        _ => None,
    }
}

fn entry(v: &Value) -> Option<Edit> {
    let Value::Array(pair) = v else { return None };
    let Some(Value::Array(key)) = pair.first() else {
        return None;
    };
    // The edit itself is packed again: a list of packed fields.
    let blobs = match pair.get(1)? {
        Value::Binary(b) => match rmpv::decode::read_value(&mut &b[..]).ok()? {
            Value::Array(a) => a,
            _ => return None,
        },
        Value::Array(a) => a.clone(),
        _ => return None,
    };
    let blobs = &blobs[..];
    Some(Edit {
        slot: key.first()?.as_u64()? as usize,
        path: key.get(1)?.as_str()?.to_string(),
        force: field(blobs, 3).as_ref().and_then(vec3),
        weight: field(blobs, 7).and_then(|v| v.as_f64()).map(|f| f as f32),
        active: field(blobs, 6).and_then(|v| v.as_bool()),
    })
}

/// The accessory edits of one outfit.
pub fn for_outfit(ext: &ExtData, outfit: usize) -> Vec<Edit> {
    match ext.get(KEY).and_then(|p| p.nested(&format!("AccessoryEdits{outfit}"))) {
        Some(Value::Array(list)) => list.iter().filter_map(entry).collect(),
        _ => Vec::new(),
    }
}

/// The edit for a DynamicBone of an accessory, the first that matches as
/// DBDE's lookup does.
pub fn find<'a>(edits: &'a [Edit], slot: usize, path: &str) -> Option<&'a Edit> {
    edits.iter().find(|e| e.slot == slot && e.path == path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packed(v: Value) -> Value {
        let mut b = Vec::new();
        rmpv::encode::write_value(&mut b, &v).unwrap();
        Value::Binary(b)
    }

    #[test]
    fn force_weight_and_state_are_read_from_their_fields() {
        let mut blobs = vec![Value::Binary(vec![0x80]); 10];
        blobs[2] = Value::Nil;
        blobs[3] = packed(Value::Array(vec![0.0.into(), (-0.0005).into(), 0.0006.into()]));
        blobs[6] = packed(true.into());
        blobs[7] = packed(0.5.into());
        let e = entry(&Value::Array(vec![
            Value::Array(vec![1.into(), "N_move/hair/root".into()]),
            packed(Value::Array(blobs)),
        ]))
        .unwrap();
        assert_eq!((e.slot, e.path.as_str()), (1, "N_move/hair/root"));
        assert_eq!(e.force, Some([0.0, -0.0005, 0.0006]));
        assert_eq!((e.weight, e.active), (Some(0.5), Some(true)));
        assert!(find(std::slice::from_ref(&e), 1, "N_move/hair/root").is_some());
        assert!(find(std::slice::from_ref(&e), 0, "N_move/hair/root").is_none());
    }
}
