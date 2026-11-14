// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Result;
use rk_card::{Card, Value};
use serde_json::{json, Map, Value as J};
use std::path::Path;

/// MessagePack to JSON for display. Byte blobs that hold a nested MessagePack
/// document are expanded; other blobs are shown by size.
pub fn to_json(v: &Value, depth: usize) -> J {
    match v {
        Value::Nil => J::Null,
        Value::Boolean(b) => json!(b),
        Value::Integer(i) => i
            .as_i64()
            .map(|i| json!(i))
            .or_else(|| i.as_u64().map(|u| json!(u)))
            .unwrap_or(J::Null),
        Value::F32(f) => json!(f),
        Value::F64(f) => json!(f),
        Value::String(s) => json!(s.as_str().unwrap_or("<invalid utf8>")),
        Value::Binary(b) => {
            if depth < 6 && b.len() > 1 {
                let mut cur = &b[..];
                if let Ok(inner) = rmpv::decode::read_value(&mut cur) {
                    if cur.is_empty() && matches!(inner, Value::Map(_) | Value::Array(_)) {
                        return to_json(&inner, depth + 1);
                    }
                }
            }
            json!(format!("<{} bytes>", b.len()))
        }
        Value::Array(a) => J::Array(a.iter().map(|v| to_json(v, depth)).collect()),
        Value::Map(m) => {
            let mut o = Map::new();
            for (k, v) in m {
                let key = match k {
                    Value::String(s) => s.as_str().unwrap_or("?").to_string(),
                    other => other.to_string(),
                };
                o.insert(key, to_json(v, depth));
            }
            J::Object(o)
        }
        Value::Ext(t, b) => json!(format!("<ext {t}, {} bytes>", b.len())),
    }
}

pub fn dump(path: &Path, full_ext: bool) -> Result<()> {
    let c = Card::load(path)?;
    let coords: Vec<J> = c
        .coordinates
        .iter()
        .map(|co| {
            json!({
                "clothes": co.clothes.parts.iter().map(|p| p.id).collect::<Vec<_>>(),
                "clothesColors": co.clothes.parts.iter().map(|p| p.color_info.iter()
                    .map(|c| json!({"base": c.base_color, "pattern": c.pattern, "patternColor": c.pattern_color, "tiling": c.tiling}))
                    .collect::<Vec<_>>()).collect::<Vec<_>>(),
                "subParts": co.clothes.sub_parts_id,
                "accessories": co.accessory.parts.iter().filter(|a| !a.is_empty())
                    .map(|a| json!({"type": a.kind, "id": a.id, "parent": a.parent_key})).collect::<Vec<_>>(),
            })
        })
        .collect();
    let ext: Map<String, J> = c
        .ext
        .iter()
        .map(|(k, p)| {
            let v = if full_ext {
                J::Object(p.data.iter().map(|(dk, dv)| (dk.clone(), to_json(dv, 0))).collect())
            } else {
                json!({"version": p.version, "keys": p.data.keys().collect::<Vec<_>>()})
            };
            (k.clone(), v)
        })
        .collect();
    let out = json!({
        "kind": format!("{:?}", c.kind),
        "version": c.version,
        "name": c.full_name(),
        "sex": c.sex(),
        "blocks": c.blocks.iter().map(|b| json!({"name": b.name, "version": b.version, "size": b.size})).collect::<Vec<_>>(),
        "face": {"headId": c.face.head_id, "skinId": c.face.skin_id, "detailId": c.face.detail_id,
                 "eyebrowId": c.face.eyebrow_id, "noseId": c.face.nose_id,
                 "pupil": c.face.pupil.iter().map(|p| p.id).collect::<Vec<_>>(),
                 "shapes": c.face.shape_value_face},
        "body": {"skinId": c.body.skin_id, "detailId": c.body.detail_id, "skinMainColor": c.body.skin_main_color,
                 "shapes": c.body.shape_value_body},
        "hair": c.hair.parts.iter().map(|p| p.id).collect::<Vec<_>>(),
        "coordinates": coords,
        "parameter": to_json(&c.parameter, 0),
        "ext": ext,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
