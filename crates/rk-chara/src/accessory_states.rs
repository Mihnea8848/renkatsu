// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Accessory States plugin (`KK_Accessory_States`): accessories bound
//! to a piece of clothing or to a custom group are shown only in some of
//! its states.
//!
//! The plugin keeps, per outfit, a `Slotdata` for each bound accessory
//! slot: the binding (0 to 8 a clothes kind, 9 and up a custom group, -1
//! none), the state ranges it is shown in, and which shoes it belongs to
//! (0 indoor, 1 outdoor, 2 both). `CharaEvent.ChangedOutfit` and
//! `Custom_Groups` then call `ChaControl.SetAccessoryState(slot, show)`
//! with `show = shoes match && state inside a range`.
//!
//! A character added in Studio has every piece of clothing in state 0 and
//! every custom group in state 0, so that is what is evaluated here.
//!
//! Cards written by madevil's AccStateSync (`madevil.kk.ass`) are read
//! when the plugin has no data of its own, through the plugin's own
//! conversion (`AccStateSync.Migration` and
//! `CoordinateData.Accstatesyncconvert`). That conversion is ported as it
//! is, including that it numbers the state ranges by position in the list
//! of visible states instead of by state.
//!
//! Not ported: version 0 of the plugin's own data (`Migrator.MigrateV0`)
//! and AccStateSync data older than version 2. Such cards are treated as
//! having no data.

use rk_card::{map_get, ExtData, Value};

pub const KEY: &str = "Accessory_States";
pub const ASS_KEY: &str = "madevil.kk.ass";

/// `Slotdata`.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub binding: i64,
    pub states: Vec<[i64; 2]>,
    pub shoe_type: u8,
    pub parented: bool,
}

/// `CoordinateData`: the slots in stored order, and the kinds that have a
/// group name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Coordinate {
    pub slots: Vec<(usize, Slot)>,
    pub names: Vec<i64>,
}

/// `AccStateSync.TriggerProperty`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Trigger {
    coordinate: i64,
    slot: i64,
    kind: i64,
    state: i64,
    visible: bool,
}

/// `AccStateSync.TriggerGroup`, as far as it is used.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Group {
    coordinate: i64,
    kind: i64,
}

/// The conversion threw; the plugin's `OnReload` then ends early and
/// nothing is bound.
#[derive(Debug, PartialEq)]
struct Thrown;

fn int(v: &Value, key: &str) -> Option<i64> {
    map_get(v, key).and_then(Value::as_i64)
}

fn entries(v: &Value) -> &[(Value, Value)] {
    match v {
        Value::Map(m) => m,
        _ => &[],
    }
}

fn slot_data(v: &Value) -> Slot {
    // `Slotdata` defaults: no binding, shown in every state, both shoes.
    let states = map_get(v, "_state").and_then(Value::as_array).map(|ranges| {
        ranges
            .iter()
            .filter_map(|r| {
                let r = r.as_array()?;
                Some([r.first()?.as_i64()?, r.get(1)?.as_i64()?])
            })
            .collect()
    });
    Slot {
        binding: int(v, "_binding").unwrap_or(-1),
        states: states.unwrap_or_else(|| vec![[0, 3]]),
        shoe_type: int(v, "_shoetype").unwrap_or(2) as u8,
        parented: map_get(v, "_parented").and_then(Value::as_bool).unwrap_or(false),
    }
}

/// One outfit of the plugin's own version 1 data.
fn own(table: &Value, outfit: usize) -> Coordinate {
    let Some((_, data)) = entries(table).iter().find(|(k, _)| k.as_u64() == Some(outfit as u64)) else {
        return Coordinate::default();
    };
    Coordinate {
        slots: map_get(data, "_slotinfo")
            .map(entries)
            .unwrap_or_default()
            .iter()
            .filter_map(|(k, v)| Some((k.as_u64()? as usize, slot_data(v))))
            .collect(),
        names: map_get(data, "_names")
            .map(entries)
            .unwrap_or_default()
            .iter()
            .filter_map(|(k, _)| k.as_i64())
            .collect(),
    }
}

/// AccStateSync version 6: the lists are stored as they are used.
fn ass_v6(p: &rk_card::PluginData) -> (Vec<Trigger>, Vec<Group>) {
    let mut triggers = Vec::new();
    let mut groups = Vec::new();
    let Some(Value::Array(list)) = p.nested("TriggerPropertyList") else {
        return (triggers, groups);
    };
    if list.is_empty() {
        return (triggers, groups);
    }
    for t in &list {
        triggers.push(Trigger {
            coordinate: int(t, "Coordinate").unwrap_or(0),
            slot: int(t, "Slot").unwrap_or(0),
            kind: int(t, "RefKind").unwrap_or(0),
            state: int(t, "RefState").unwrap_or(0),
            visible: map_get(t, "Visible").and_then(Value::as_bool).unwrap_or(true),
        });
    }
    if let Some(Value::Array(list)) = p.nested("TriggerGroupList") {
        for g in &list {
            groups.push(Group {
                coordinate: int(g, "Coordinate").unwrap_or(0),
                kind: int(g, "Kind").unwrap_or(0),
            });
        }
    }
    (triggers, groups)
}

/// AccStateSync versions 2 to 5 (`Migration.ConvertCharaPluginData` and
/// `Migrate`): one entry per slot with a kind, a group and four flags.
fn ass_old(p: &rk_card::PluginData) -> Result<(Vec<Trigger>, Vec<Group>), Thrown> {
    let mut triggers = Vec::new();
    let mut groups = Vec::new();
    let Some(table) = p.nested("CharaTriggerInfo") else {
        return Ok((triggers, groups));
    };
    // The migration only walks the seven outfits of the base game.
    for outfit in 0..7i64 {
        let Some((_, info)) = entries(&table).iter().find(|(k, _)| k.as_i64() == Some(outfit)) else {
            continue;
        };
        let mut parts: Vec<(i64, String, i64, Vec<bool>)> = map_get(info, "Parts")
            .map(entries)
            .unwrap_or_default()
            .iter()
            .map(|(_, part)| {
                (
                    int(part, "Kind").unwrap_or(-1),
                    map_get(part, "Group").and_then(Value::as_str).unwrap_or("").to_string(),
                    int(part, "Slot").unwrap_or(0),
                    map_get(part, "State")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().map(|b| b.as_bool().unwrap_or(false)).collect())
                        .unwrap_or_else(|| vec![true, false, false, false]),
                )
            })
            .collect();
        parts.sort_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)));
        let mut custom: Vec<String> = Vec::new();
        for (kind, group, slot, state) in parts {
            let flag = |i: usize| state.get(i).copied().ok_or(Thrown);
            let mut push = |kind: i64, state: i64, visible: bool| {
                triggers.push(Trigger {
                    coordinate: outfit,
                    slot,
                    kind,
                    state,
                    visible,
                })
            };
            if (0..=8).contains(&kind) {
                for i in 0..4 {
                    push(kind, i as i64, flag(i)?);
                }
            } else if kind >= 9 {
                // Custom groups are renumbered from 9 in order of use.
                let at = match custom.iter().position(|g| *g == group) {
                    Some(at) => at,
                    None => {
                        custom.push(group.clone());
                        custom.len() - 1
                    }
                };
                push(9 + at as i64, 0, flag(0)?);
                push(9 + at as i64, 1, flag(3)?);
            }
        }
        groups.extend((0..custom.len()).map(|at| Group {
            coordinate: outfit,
            kind: 9 + at as i64,
        }));
    }
    Ok((triggers, groups))
}

/// `CoordinateData.Accstatesyncconvert(TriggerProperty, TriggerGroup)` for
/// the triggers and groups of one outfit.
fn convert(triggers: &[Trigger], groups: &[Group]) -> Result<Coordinate, Thrown> {
    let mut out = Coordinate::default();
    let mut slots: Vec<i64> = Vec::new();
    for t in triggers {
        if !slots.contains(&t.slot) {
            slots.push(t.slot);
        }
    }
    for slot in slots {
        let mut list: Vec<&Trigger> = triggers.iter().filter(|t| t.slot == slot).collect();
        list.sort_by_key(|t| (t.kind, t.state));
        let inner = list.iter().any(|t| t.kind == 7 && t.visible);
        let outer = list.iter().any(|t| t.kind == 8 && t.visible);
        let shoe_type = match (inner, outer) {
            (true, true) | (false, false) => 2,
            (true, false) => 0,
            (false, true) => 1,
        };
        let mut kinds: Vec<i64> = Vec::new();
        for t in &list {
            if !kinds.contains(&t.kind) {
                kinds.push(t.kind);
            }
        }
        // A shoe kind is dropped when the slot also has a lower kind.
        for shoe in 7..=8 {
            if kinds.contains(&shoe) && kinds.iter().any(|&k| k < shoe) {
                kinds.retain(|&k| k != shoe);
            }
        }
        // The kind with the most visible states becomes the binding; the
        // first one wins a tie.
        let mut best: Option<(i64, Vec<[i64; 2]>, usize)> = None;
        for kind in kinds {
            let shown: Vec<&&Trigger> = list.iter().filter(|t| t.visible && t.kind == kind).collect();
            if shown.is_empty() {
                continue;
            }
            // The ranges count positions in `shown`, not states.
            let mut ranges = Vec::new();
            let mut from = 0;
            while from < shown.len() {
                let mut to = from;
                while to + 1 < shown.len() && shown[to + 1].state == to as i64 + 1 {
                    to += 1;
                }
                ranges.push([from as i64, to as i64]);
                from = to + 1;
            }
            if best.as_ref().is_none_or(|b| shown.len() > b.2) {
                best = Some((kind, ranges, shown.len()));
            }
        }
        // `First()` on an empty sequence throws.
        let (binding, states, _) = best.ok_or(Thrown)?;
        let data = Slot {
            binding,
            states,
            shoe_type,
            parented: false,
        };
        match out.slots.iter_mut().find(|(s, _)| *s == slot as usize) {
            Some(entry) => entry.1 = data,
            None => out.slots.push((slot as usize, data)),
        }
    }
    out.names = groups.iter().map(|g| g.kind).collect();
    Ok(out)
}

/// The data in effect for `outfit` after the plugin's `OnReload`
/// (`NowCoordinate`). `outfits` is how many outfits the card has.
pub fn coordinate(ext: &ExtData, outfit: usize, outfits: usize) -> Coordinate {
    if let Some(p) = ext.get(KEY) {
        // Own data wins; AccStateSync data is then not looked at.
        return match p.version {
            1 => p.nested("CoordinateData").map(|t| own(&t, outfit)).unwrap_or_default(),
            _ => Coordinate::default(),
        };
    }
    let Some(p) = ext.get(ASS_KEY) else {
        return Coordinate::default();
    };
    let lists = match p.version {
        6 => Ok(ass_v6(p)),
        2..=5 => ass_old(p),
        _ => return Coordinate::default(),
    };
    let Ok((triggers, groups)) = lists else {
        return Coordinate::default();
    };
    // Every outfit is converted in turn, and one that throws stops the
    // whole load before anything is put to use.
    let mut wanted = Coordinate::default();
    for o in 0..outfits {
        let t: Vec<Trigger> = triggers.iter().filter(|t| t.coordinate == o as i64).copied().collect();
        let g: Vec<Group> = groups.iter().filter(|g| g.coordinate == o as i64).copied().collect();
        match convert(&t, &g) {
            Ok(c) if o == outfit => wanted = c,
            Ok(_) => {}
            Err(Thrown) => return Coordinate::default(),
        }
    }
    wanted
}

/// `ShowState`.
fn show_state(state: i64, ranges: &[[i64; 2]]) -> bool {
    ranges.iter().any(|r| state >= r[0] && state <= r[1])
}

/// The accessory slots the plugin switches off for a character whose
/// clothes and custom groups are all in state 0. `shoes` is the status'
/// shoe type (0 indoor, 1 outdoor), `parts` the number of accessory slots.
pub fn hidden(coordinate: &Coordinate, shoes: u8, parts: usize) -> Vec<usize> {
    let mut out = Vec::new();
    // Each binding is walked on its own and stops at the first slot past
    // the accessory list.
    let mut stopped: Vec<i64> = Vec::new();
    for (slot, data) in &coordinate.slots {
        let bound = (0..=8).contains(&data.binding) || coordinate.names.contains(&data.binding);
        if !bound || stopped.contains(&data.binding) {
            continue;
        }
        if *slot >= parts {
            stopped.push(data.binding);
            continue;
        }
        let shoes_match = data.shoe_type == 2 || data.shoe_type == shoes;
        if !(shoes_match && show_state(0, &data.states)) {
            out.push(*slot);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_card::PluginData;

    fn blob(v: &Value) -> Value {
        let mut bytes = Vec::new();
        rmpv::encode::write_value(&mut bytes, v).unwrap();
        Value::Binary(bytes)
    }

    fn map(pairs: Vec<(Value, Value)>) -> Value {
        Value::Map(pairs)
    }

    fn trigger(coordinate: i64, slot: i64, kind: i64, state: i64, visible: bool) -> Value {
        map(vec![
            ("Coordinate".into(), coordinate.into()),
            ("Slot".into(), slot.into()),
            ("RefKind".into(), kind.into()),
            ("RefState".into(), state.into()),
            ("Visible".into(), visible.into()),
            ("Priority".into(), 1.into()),
        ])
    }

    fn ass(version: i64, key: &str, v: Value) -> ExtData {
        let mut p = PluginData {
            version,
            ..Default::default()
        };
        p.data.insert(key.into(), blob(&v));
        let mut ext = ExtData::new();
        ext.insert(ASS_KEY.into(), p);
        ext
    }

    fn slot(binding: i64, states: &[[i64; 2]], shoe_type: u8) -> Slot {
        Slot {
            binding,
            states: states.to_vec(),
            shoe_type,
            parented: false,
        }
    }

    #[test]
    fn own_data_is_read_per_outfit() {
        let slot_v = map(vec![
            ("_binding".into(), 1.into()),
            (
                "_state".into(),
                Value::Array(vec![Value::Array(vec![1.into(), 3.into()])]),
            ),
            ("_shoetype".into(), 2.into()),
            ("_parented".into(), false.into()),
        ]);
        let table = map(vec![(
            0.into(),
            map(vec![
                ("_slotinfo".into(), map(vec![(8.into(), slot_v)])),
                ("_names".into(), map(vec![])),
            ]),
        )]);
        let mut p = PluginData {
            version: 1,
            ..Default::default()
        };
        p.data.insert("CoordinateData".into(), blob(&table));
        let mut ext = ExtData::new();
        ext.insert(KEY.into(), p);
        // AccStateSync data beside it is ignored.
        ext.insert(ASS_KEY.into(), PluginData::default());
        let c = coordinate(&ext, 0, 7);
        assert_eq!(c.slots, vec![(8, slot(1, &[[1, 3]], 2))]);
        // Shown from half off on: hidden while the bottom is fully on.
        assert_eq!(hidden(&c, 0, 20), vec![8]);
        assert!(coordinate(&ext, 1, 7).slots.is_empty());
    }

    #[test]
    fn states_and_shoes_decide() {
        let c = Coordinate {
            slots: vec![
                (0, slot(0, &[[0, 1]], 2)),
                (1, slot(0, &[[2, 3]], 2)),
                (2, slot(7, &[[0, 3]], 1)),
                (3, slot(9, &[[1, 1]], 2)),
                (4, slot(10, &[[1, 1]], 2)),
                (5, slot(-1, &[[1, 1]], 2)),
                (30, slot(0, &[[3, 3]], 2)),
                (6, slot(0, &[[3, 3]], 2)),
            ],
            names: vec![9],
        };
        // Slot 4's group has no name and slot 5 no binding: left alone.
        // Slot 30 is past the list and ends the walk of binding 0.
        assert_eq!(hidden(&c, 0, 20), vec![1, 2, 3]);
        assert_eq!(hidden(&c, 1, 20), vec![1, 3]);
    }

    #[test]
    fn version_6_lists_convert_by_position() {
        // Visible in states 0 and 1 of the bottom.
        let mut list = vec![
            trigger(0, 8, 1, 0, true),
            trigger(0, 8, 1, 1, true),
            trigger(0, 8, 1, 2, false),
            trigger(0, 8, 1, 3, false),
        ];
        // Visible only with the top off: the range still starts at 0.
        list.extend((0..4).map(|s| trigger(0, 9, 0, s, s == 3)));
        // Visible with outdoor shoes only.
        list.extend((0..4).map(|s| trigger(0, 10, 0, s, true)));
        list.extend((0..4).map(|s| trigger(0, 10, 7, s, false)));
        list.extend((0..4).map(|s| trigger(0, 10, 8, s, true)));
        let ext = ass(6, "TriggerPropertyList", Value::Array(list));
        let c = coordinate(&ext, 0, 7);
        assert_eq!(
            c.slots,
            vec![
                (8, slot(1, &[[0, 1]], 2)),
                (9, slot(0, &[[0, 0]], 2)),
                (10, slot(0, &[[0, 3]], 1)),
            ]
        );
        assert_eq!(hidden(&c, 0, 20), vec![10]);
        assert!(hidden(&c, 1, 20).is_empty());
    }

    #[test]
    fn a_slot_that_is_never_visible_stops_the_load() {
        let mut list: Vec<Value> = (0..4).map(|s| trigger(0, 10, 8, s, true)).collect();
        list.extend((0..4).map(|s| trigger(3, 2, 0, s, false)));
        let ext = ass(6, "TriggerPropertyList", Value::Array(list));
        // Outfit 3 throws, so outfit 0 gets nothing either.
        assert_eq!(coordinate(&ext, 0, 7), Coordinate::default());
        // With only three outfits the bad one is never reached.
        assert_eq!(coordinate(&ext, 0, 3).slots, vec![(10, slot(8, &[[0, 3]], 1))]);
    }

    #[test]
    fn old_data_goes_through_the_migration() {
        let part = |slot: i64, kind: i64, group: &str, state: [bool; 4]| {
            (
                Value::from(slot),
                map(vec![
                    ("Slot".into(), slot.into()),
                    ("Kind".into(), kind.into()),
                    ("Group".into(), group.into()),
                    ("State".into(), Value::Array(state.iter().map(|&b| b.into()).collect())),
                ]),
            )
        };
        let table = map(vec![(
            0.into(),
            map(vec![
                ("Index".into(), 0.into()),
                (
                    "Parts".into(),
                    map(vec![
                        part(18, 0, "", [true, true, false, false]),
                        part(3, 10, "custom_1", [false, false, false, true]),
                        part(4, 8, "", [true, true, true, true]),
                    ]),
                ),
            ]),
        )]);
        let ext = ass(5, "CharaTriggerInfo", table);
        let c = coordinate(&ext, 0, 7);
        assert_eq!(
            c.slots,
            vec![
                (18, slot(0, &[[0, 1]], 2)),
                (4, slot(8, &[[0, 3]], 1)),
                (3, slot(9, &[[0, 0]], 2)),
            ]
        );
        assert_eq!(c.names, vec![9]);
        assert_eq!(hidden(&c, 0, 20), vec![4]);
    }
}
