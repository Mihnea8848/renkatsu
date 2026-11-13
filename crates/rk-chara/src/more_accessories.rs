// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Accessory slots beyond the game's twenty, saved by the MoreAccessories
//! plugin. Recent versions write them straight into the card's accessory
//! list. Older ones keep them in an XML document, one set per outfit, which
//! is what this module reads.

use rk_card::{AccessoryPart, Array2, ExtData, Value};
use std::collections::HashMap;

pub const KEY: &str = "moreAccessories";

/// Extra accessories per outfit index, in slot order after the first twenty.
pub fn read(ext: &ExtData) -> HashMap<usize, Vec<AccessoryPart>> {
    let mut out = HashMap::new();
    let Some(Value::String(xml)) = ext.get(KEY).and_then(|p| p.data.get("additionalAccessories")) else {
        return out;
    };
    let Some(Ok(doc)) = xml.as_str().map(roxmltree::Document::parse) else {
        return out;
    };
    for set in doc.root_element().children().filter(|n| n.has_tag_name("accessorySet")) {
        let Some(outfit) = set.attribute("type").and_then(|t| t.parse::<usize>().ok()) else {
            continue;
        };
        let parts: Vec<AccessoryPart> = set
            .children()
            .filter(|n| n.has_tag_name("accessory"))
            .map(|n| part(&n))
            .collect();
        out.insert(outfit, parts);
    }
    out
}

fn part(n: &roxmltree::Node) -> AccessoryPart {
    let num = |name: &str, default: f32| n.attribute(name).and_then(|v| v.parse::<f32>().ok()).unwrap_or(default);
    // addMoveRCx: R is the move target, C is position, rotation or scale.
    let mut moves = Vec::with_capacity(6);
    for r in 0..2 {
        for c in 0..3 {
            let default = if c == 2 { 1.0 } else { 0.0 };
            moves.push(["x", "y", "z"].map(|axis| num(&format!("addMove{r}{c}{axis}"), default)));
        }
    }
    AccessoryPart {
        kind: n.attribute("type").and_then(|v| v.parse().ok()).unwrap_or(120),
        id: n.attribute("id").and_then(|v| v.parse().ok()).unwrap_or(0),
        parent_key: n.attribute("parentKey").unwrap_or("").to_string(),
        add_move: Array2(2, 3, moves),
        color: (0..4)
            .map(|i| ["r", "g", "b", "a"].map(|ch| num(&format!("color{i}{ch}"), 1.0)))
            .collect(),
        hide_category: n.attribute("hideCategory").and_then(|v| v.parse().ok()).unwrap_or(0),
        no_shake: n.attribute("noShake") == Some("true"),
    }
}
