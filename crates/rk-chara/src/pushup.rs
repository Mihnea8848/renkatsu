// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Pushup plugin (KK_Pushup 1.4.0): while a top or bra is fully on and
//! has the effect enabled, the plugin replaces the ten bust entries of
//! `ChaFileBody.shapeValueBody` (and bust softness and weight) with values
//! computed from the outfit's settings, then lets the game's own shape code
//! run. Formulas are `PushupController.CalculatePush` and
//! `CalculatePushFromClothes`.

use rk_card::{map_get, ExtData, Value};

pub const KEY: &str = "com.deathweasel.bepinex.pushup";

/// `shapeValueBody` index of the first bust value (`PushupConstants`).
pub const SHAPE_FIRST: usize = 4;

/// `Pushup.BodyData`: the bust values the plugin controls.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BodyData {
    pub size: f32,
    pub vertical_position: f32,
    pub horizontal_angle: f32,
    pub horizontal_position: f32,
    pub vertical_angle: f32,
    pub depth: f32,
    pub roundness: f32,
    pub softness: f32,
    pub weight: f32,
    pub areola_depth: f32,
    pub nipple_width: f32,
    pub nipple_depth: f32,
}

impl BodyData {
    /// `BodyData(ChaFileBody)`.
    pub fn from_body(shapes: &[f32], softness: f32, weight: f32) -> Self {
        let s = |i: usize| shapes.get(i).copied().unwrap_or(0.0);
        Self {
            size: s(4),
            vertical_position: s(5),
            horizontal_angle: s(6),
            horizontal_position: s(7),
            vertical_angle: s(8),
            depth: s(9),
            roundness: s(10),
            areola_depth: s(11),
            nipple_width: s(12),
            nipple_depth: s(13),
            softness,
            weight,
        }
    }

    /// The values for `shapeValueBody[4..=13]`, as `MapBodyInfoToChaFile`
    /// writes them.
    pub fn shapes(&self) -> [f32; 10] {
        [
            self.size,
            self.vertical_position,
            self.horizontal_angle,
            self.horizontal_position,
            self.vertical_angle,
            self.depth,
            self.roundness,
            self.areola_depth,
            self.nipple_width,
            self.nipple_depth,
        ]
    }

    fn parse(v: &Value) -> Self {
        let f = |k: &str| map_get(v, k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
        Self {
            size: f("Size"),
            vertical_position: f("VerticalPosition"),
            horizontal_angle: f("HorizontalAngle"),
            horizontal_position: f("HorizontalPosition"),
            vertical_angle: f("VerticalAngle"),
            depth: f("Depth"),
            roundness: f("Roundness"),
            softness: f("Softness"),
            weight: f("Weight"),
            areola_depth: f("AreolaDepth"),
            nipple_width: f("NippleWidth"),
            nipple_depth: f("NippleDepth"),
        }
    }
}

/// `Pushup.ClothData`: the settings of one top or bra.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClothData {
    /// The bust values used in advanced mode.
    pub body: BodyData,
    pub firmness: f32,
    pub lift: f32,
    pub push_together: f32,
    pub squeeze: f32,
    pub center_nipples: f32,
    pub enable: bool,
    pub flatten_nipples: bool,
    pub use_advanced: bool,
}

impl ClothData {
    /// `ClothData(BodyData)`: what the plugin makes for an outfit without
    /// an entry. The other fields are the plugin's config defaults
    /// (`Pushup.Start`); a changed config file is not read.
    pub fn new(body: BodyData) -> Self {
        Self {
            body,
            firmness: 0.9,
            lift: 0.6,
            push_together: 0.55,
            squeeze: 0.6,
            center_nipples: 0.5,
            enable: false,
            flatten_nipples: true,
            use_advanced: false,
        }
    }

    fn parse(v: &Value) -> Self {
        let d = Self::new(BodyData::default());
        let f = |k: &str, def: f32| map_get(v, k).and_then(Value::as_f64).map_or(def, |x| x as f32);
        let flag = |k: &str, def: bool| map_get(v, k).and_then(Value::as_bool).unwrap_or(def);
        Self {
            body: BodyData::parse(v),
            firmness: f("Firmness", d.firmness),
            lift: f("Lift", d.lift),
            push_together: f("PushTogether", d.push_together),
            squeeze: f("Squeeze", d.squeeze),
            center_nipples: f("CenterNipples", d.center_nipples),
            enable: flag("EnablePushup", d.enable),
            flatten_nipples: flag("FlattenNipples", d.flatten_nipples),
            use_advanced: flag("UseAdvanced", d.use_advanced),
        }
    }
}

/// The bra and top settings of one outfit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outfit {
    pub bra: ClothData,
    pub top: ClothData,
}

fn entry(table: Option<Value>, outfit: usize, base: &BodyData) -> ClothData {
    table
        .as_ref()
        .and_then(Value::as_map)
        .and_then(|m| m.iter().find(|(k, _)| k.as_u64() == Some(outfit as u64)))
        .filter(|(_, v)| v.is_map())
        .map(|(_, v)| ClothData::parse(v))
        .unwrap_or_else(|| ClothData::new(*base))
}

/// The settings saved in the card for `outfit`; `None` when the card has no
/// Pushup data. `base` is the card's own bust (`BaseData`), which missing
/// entries start from (`GetBraData`, `GetTopData`).
pub fn for_outfit(ext: &ExtData, outfit: usize, base: &BodyData) -> Option<Outfit> {
    let data = ext.get(KEY)?;
    Some(Outfit {
        bra: entry(data.nested("Pushup_BraData"), outfit, base),
        top: entry(data.nested("Pushup_TopData"), outfit, base),
    })
}

/// A clothing kind's state type as `ChaControl.AddClothesStateKind` reads
/// it from the item list; 3 stands for no item.
fn state_type(v: Option<i32>) -> u8 {
    v.and_then(|v| u8::try_from(v).ok()).unwrap_or(3)
}

/// What decides whether the top and bra are clothing kinds with a state
/// (`ChaControl.IsClothesStateKind(0)` and `(2)`).
#[derive(Debug, Clone, Copy, Default)]
pub struct StateKinds {
    /// `StateType` list column of the top, bottom, bra and shorts.
    pub top: Option<i32>,
    pub bottom: Option<i32>,
    pub bra: Option<i32>,
    pub shorts: Option<i32>,
    /// The bottom carries a top piece (`n_top_a`, `S_CBOT_T_DEF`).
    pub bottom_has_top: bool,
    /// The shorts carry a top piece (`n_top_a`, `S_UWB_T_DEF`).
    pub shorts_have_top: bool,
    /// `ChaControl.notBra` and `notShorts`.
    pub not_bra: bool,
    pub not_shorts: bool,
}

impl StateKinds {
    /// `AddClothesStateKind` for the top: the kind exists when its state
    /// type is 0 to 2. A bottom of type 0 with a top piece lends its type.
    /// The changes that function makes to the other kind of each pair
    /// cannot change the result here and are left out.
    pub fn top_kind(&self) -> bool {
        let mut t = state_type(self.top);
        if t != 0 && self.bottom_has_top && state_type(self.bottom) == 0 {
            t = 0;
        }
        t <= 2
    }

    /// `AddClothesStateKind` for the bra.
    pub fn bra_kind(&self) -> bool {
        let mut b = if self.not_bra { 3 } else { state_type(self.bra) };
        let s = if !self.not_bra || !self.not_shorts {
            state_type(self.shorts)
        } else {
            3
        };
        if b != 0 && b != 2 && self.shorts_have_top {
            match s {
                0 => b = 0,
                1 if b == 3 => b = 1,
                _ => {}
            }
        }
        b <= 2
    }
}

/// `RecalculateBody` with every piece of clothing fully on: the bust the
/// body takes, or `None` when the card's own values stay. `top_kind` and
/// `bra_kind` say whether each is a clothing kind with a state.
pub fn bust(base: &BodyData, outfit: &Outfit, top_kind: bool, bra_kind: bool) -> Option<BodyData> {
    let top = top_kind && outfit.top.enable;
    let bra = bra_kind && outfit.bra.enable;
    let (b, t) = (&outfit.bra, &outfit.top);
    // CalculatePush.
    let cloth = match (top, bra) {
        (false, false) => return None,
        (false, true) => *b,
        (true, false) => *t,
        (true, true) if t.use_advanced => *t,
        (true, true) if b.use_advanced => *b,
        (true, true) => ClothData {
            firmness: b.firmness.max(t.firmness),
            lift: b.lift.max(t.lift),
            squeeze: b.squeeze.max(t.squeeze),
            push_together: b.push_together.max(t.push_together),
            center_nipples: b.center_nipples.max(t.center_nipples),
            flatten_nipples: b.flatten_nipples || t.flatten_nipples,
            enable: true,
            ..ClothData::new(BodyData::default())
        },
    };
    Some(from_clothes(base, &cloth))
}

/// `CalculatePushFromClothes`.
fn from_clothes(base: &BodyData, c: &ClothData) -> BodyData {
    if c.use_advanced {
        return c.body;
    }
    let squeezed = 1.0 - c.squeeze < base.depth;
    let tilt = 0.5 - base.vertical_angle;
    BodyData {
        softness: base.softness.min(1.0 - c.firmness),
        vertical_position: base.vertical_position.max(c.lift),
        horizontal_angle: base.horizontal_angle.min(1.0 - c.push_together),
        horizontal_position: base.horizontal_position.min(1.0 - c.push_together),
        depth: if squeezed { 1.0 - c.squeeze } else { base.depth },
        size: if squeezed {
            base.size + (base.depth - (1.0 - c.squeeze)) / 10.0
        } else {
            base.size
        },
        nipple_depth: if c.flatten_nipples { 0.0 } else { base.nipple_depth },
        areola_depth: if c.flatten_nipples { 0.0 } else { base.areola_depth },
        nipple_width: base.nipple_width,
        vertical_angle: 0.5 - (tilt - tilt * c.center_nipples),
        weight: base.weight,
        roundness: base.roundness,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_card::PluginData;

    fn s(v: &str) -> Value {
        Value::from(v)
    }

    fn base() -> BodyData {
        // Indices 4..=13 are 0.6, 0.4, 0.7, 0.3, 0.2, 0.8, 0.5, 0.3, 0.45, 0.35.
        let shapes = [0.0, 0.0, 0.0, 0.0, 0.6, 0.4, 0.7, 0.3, 0.2, 0.8, 0.5, 0.3, 0.45, 0.35];
        BodyData::from_body(&shapes, 0.5, 0.6)
    }

    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
    }

    #[test]
    fn reads_body_values_by_index() {
        let b = base();
        close(b.size, 0.6);
        close(b.vertical_position, 0.4);
        close(b.depth, 0.8);
        close(b.nipple_depth, 0.35);
        assert_eq!(b.shapes()[0], b.size);
        assert_eq!(b.shapes()[7], b.areola_depth);
        assert_eq!(b.shapes()[9], b.nipple_depth);
    }

    #[test]
    fn nothing_when_not_worn_or_disabled() {
        let mut o = Outfit {
            bra: ClothData::new(base()),
            top: ClothData::new(base()),
        };
        assert_eq!(bust(&base(), &o, true, true), None);
        o.top.enable = true;
        assert_eq!(bust(&base(), &o, false, true), None);
        assert!(bust(&base(), &o, true, false).is_some());
    }

    #[test]
    fn basic_mode_clamps_towards_the_settings() {
        let mut o = Outfit {
            bra: ClothData::new(base()),
            top: ClothData::new(base()),
        };
        o.bra.enable = true;
        let p = bust(&base(), &o, false, true).unwrap();
        // Firmness 0.9: softness at most 0.1.
        close(p.softness, 0.1);
        // Lift 0.6 is above 0.4.
        close(p.vertical_position, 0.6);
        // Push together 0.55: angle at most 0.45, position 0.3 stays.
        close(p.horizontal_angle, 0.45);
        close(p.horizontal_position, 0.3);
        // Squeeze 0.6: depth at most 0.4, the rest goes into size.
        close(p.depth, 0.4);
        close(p.size, 0.6 + (0.8 - 0.4) / 10.0);
        close(p.nipple_depth, 0.0);
        close(p.areola_depth, 0.0);
        close(p.nipple_width, 0.45);
        // Centering 0.5 halves the distance of 0.2 from 0.5.
        close(p.vertical_angle, 0.35);
        close(p.weight, 0.6);
        close(p.roundness, 0.5);
    }

    #[test]
    fn basic_mode_keeps_values_already_inside() {
        let mut body = base();
        body.softness = 0.05;
        body.vertical_position = 0.9;
        body.depth = 0.1;
        let mut o = Outfit {
            bra: ClothData::new(body),
            top: ClothData::new(body),
        };
        o.top.enable = true;
        o.top.flatten_nipples = false;
        let p = bust(&body, &o, true, false).unwrap();
        close(p.softness, 0.05);
        close(p.vertical_position, 0.9);
        close(p.depth, 0.1);
        close(p.size, 0.6);
        close(p.nipple_depth, 0.35);
        close(p.areola_depth, 0.3);
    }

    #[test]
    fn both_worn_takes_the_larger_settings_or_advanced() {
        let mut o = Outfit {
            bra: ClothData::new(base()),
            top: ClothData::new(base()),
        };
        o.bra.enable = true;
        o.top.enable = true;
        o.bra.lift = 0.7;
        o.top.lift = 0.65;
        o.top.squeeze = 0.9;
        o.bra.flatten_nipples = false;
        let p = bust(&base(), &o, true, true).unwrap();
        close(p.vertical_position, 0.7);
        close(p.depth, 0.1);
        close(p.nipple_depth, 0.0);

        // The bra's advanced values win when the top is not advanced.
        o.bra.use_advanced = true;
        o.bra.body.size = 0.11;
        assert_eq!(bust(&base(), &o, true, true), Some(o.bra.body));
        // The top's advanced values win over the bra's.
        o.top.use_advanced = true;
        o.top.body.size = 0.22;
        assert_eq!(bust(&base(), &o, true, true), Some(o.top.body));
        // Advanced on the piece that is not worn does nothing.
        o.top.use_advanced = false;
        let p = bust(&base(), &o, true, false).unwrap();
        close(p.vertical_position, 0.65);
    }

    #[test]
    fn state_kinds_follow_the_item_lists() {
        let k = StateKinds {
            top: Some(0),
            bra: Some(1),
            ..Default::default()
        };
        assert!(k.top_kind() && k.bra_kind());
        // No item, or a state type outside 0..=2.
        let k = StateKinds {
            top: Some(3),
            ..Default::default()
        };
        assert!(!k.top_kind() && !k.bra_kind());
        // A bottom of type 0 with a top piece makes the top a kind.
        let k = StateKinds {
            bottom: Some(0),
            bottom_has_top: true,
            ..Default::default()
        };
        assert!(k.top_kind());
        let k = StateKinds {
            bottom: Some(1),
            bottom_has_top: true,
            ..Default::default()
        };
        assert!(!k.top_kind());
        // A top that forbids the bra removes the kind, unless the shorts
        // carry a top piece.
        let mut k = StateKinds {
            bra: Some(0),
            not_bra: true,
            ..Default::default()
        };
        assert!(!k.bra_kind());
        k.shorts = Some(1);
        k.shorts_have_top = true;
        assert!(k.bra_kind());
        k.not_shorts = true;
        assert!(!k.bra_kind());
    }

    #[test]
    fn reads_the_outfit_entries() {
        let cloth = Value::Map(vec![
            (s("Size"), Value::F32(0.25)),
            (s("Lift"), Value::F32(0.75)),
            (s("EnablePushup"), Value::Boolean(true)),
            (s("UseAdvanced"), Value::Boolean(true)),
        ]);
        let table = Value::Map(vec![(Value::from(1), cloth)]);
        let mut bytes = Vec::new();
        rmpv::encode::write_value(&mut bytes, &table).unwrap();
        let mut data = PluginData::default();
        data.data.insert("Pushup_TopData".into(), Value::Binary(bytes));
        let mut ext = ExtData::new();
        assert_eq!(for_outfit(&ext, 1, &base()), None);
        ext.insert(KEY.into(), data);

        let o = for_outfit(&ext, 1, &base()).unwrap();
        close(o.top.body.size, 0.25);
        close(o.top.lift, 0.75);
        // Keys missing from the entry keep the plugin defaults.
        close(o.top.firmness, 0.9);
        assert!(o.top.enable && o.top.use_advanced && o.top.flatten_nipples);
        // No bra table: a disabled entry made from the card's bust.
        assert_eq!(o.bra, ClothData::new(base()));
        // Another outfit has no entry.
        assert_eq!(for_outfit(&ext, 0, &base()).unwrap().top, ClothData::new(base()));
    }
}
