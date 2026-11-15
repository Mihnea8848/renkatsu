// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Pregnancy plugin (`KK_Pregnancy`): a card stores the week of a
//! pregnancy, and the body grows with it.
//!
//! `PregnancyBoneEffect` is an ABMX bone effect. For a pregnant character
//! (`Week > 0`) it returns, per bone, its full-term modifier interpolated
//! from the identity by `GetPregnancyEffectPercent`:
//! `clamp01((Week - 1) / (LeaveSchoolWeek - 1))`, and 0 past
//! `LeaveSchoolWeek`. ABMX combines an effect with the card's own modifier
//! of the same bone (`BoneModifier.CombineModifiers`). Inflation, the other
//! input of the belly, only changes inside H scenes and is 0 here.

use crate::abmx::{BoneModifier, Modifier};
use glam::Vec3;
use rk_card::{ExtData, Value};

pub const KEY: &str = "KK_Pregnancy";

/// `PregnancyData.LeaveSchoolWeek`.
pub const LEAVE_SCHOOL_WEEK: i64 = 41;

type Full = (&'static str, [f32; 3], [f32; 3], [f32; 3]);

/// `_bellyFullValues`: bone, scale, position offset, rotation in degrees.
/// Every length modifier is 1.
const BELLY: [Full; 9] = [
    ("cf_s_spine01", [1.62, 1.5, 1.9], [0.0, 0.0, 0.05], [10.0, 0.0, 0.0]),
    ("cf_s_spine02", [1.0, 1.0, 1.0], [0.0, 0.0, 0.01], [-5.0, 0.0, 0.0]),
    ("cf_s_waist01", [1.25, 1.55, 1.7], [0.0, 0.02, 0.12], [15.0, 0.0, 0.0]),
    ("cf_s_waist02", [1.17, 1.0, 1.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
    ("cf_d_sk_00_00", [1.0, 1.0, 1.0], [0.0, -0.02, 0.14], [-10.0, 0.0, 0.0]),
    (
        "cf_d_sk_07_00",
        [1.0, 1.0, 1.0],
        [-0.03, -0.02, 0.12],
        [-10.0, 0.0, 0.0],
    ),
    ("cf_d_sk_01_00", [1.0, 1.0, 1.0], [0.03, -0.02, 0.12], [-10.0, 0.0, 0.0]),
    ("cf_d_sk_06_00", [1.0, 1.0, 1.0], [-0.03, 0.0, 0.05], [0.0, 0.0, 0.0]),
    ("cf_d_sk_02_00", [1.0, 1.0, 1.0], [0.03, 0.0, 0.05], [0.0, 0.0, 0.0]),
];

/// `_pregnancyFullValues`: bone and scale; nothing else is changed.
const PREGNANCY: [(&str, [f32; 3]); 18] = [
    ("cf_d_bust01_L", [1.2, 1.2, 1.2]),
    ("cf_d_bust01_R", [1.2, 1.2, 1.2]),
    ("cf_d_bnip01_L", [1.2, 1.2, 1.2]),
    ("cf_d_bnip01_R", [1.2, 1.2, 1.2]),
    ("cf_s_siri_L", [1.1, 1.0, 1.2]),
    ("cf_s_siri_R", [1.1, 1.0, 1.2]),
    ("cf_s_thigh00_L", [1.04, 1.0, 1.04]),
    ("cf_s_thigh00_R", [1.04, 1.0, 1.04]),
    ("cf_s_thigh01_L", [1.04, 1.0, 1.04]),
    ("cf_s_thigh01_R", [1.04, 1.0, 1.04]),
    ("cf_s_thigh02_L", [1.04, 1.0, 1.04]),
    ("cf_s_thigh02_R", [1.04, 1.0, 1.04]),
    ("cf_s_thigh03_L", [1.04, 1.0, 1.04]),
    ("cf_s_thigh03_R", [1.04, 1.0, 1.04]),
    ("cf_s_leg01_L", [1.04, 1.0, 1.04]),
    ("cf_s_leg01_R", [1.04, 1.0, 1.04]),
    ("cf_s_leg02_L", [1.04, 1.0, 1.04]),
    ("cf_s_leg02_R", [1.04, 1.0, 1.04]),
];

/// The stored week; 0 (not pregnant) without data.
pub fn week(ext: &ExtData) -> i64 {
    ext.get(KEY)
        .and_then(|p| p.data.get("Week"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

/// `GetPregnancyEffectPercent`.
pub fn effect_percent(week: i64) -> f32 {
    if week > LEAVE_SCHOOL_WEEK {
        return 0.0;
    }
    ((week as f32 - 1.0) / (LEAVE_SCHOOL_WEEK as f32 - 1.0)).clamp(0.0, 1.0)
}

/// `LerpModifier`.
fn lerp(scale: [f32; 3], position: [f32; 3], rotation: [f32; 3], t: f32) -> Modifier {
    Modifier {
        scale: Vec3::ONE.lerp(Vec3::from(scale), t),
        length: 1.0,
        position: Vec3::ZERO.lerp(Vec3::from(position), t),
        rotation: Vec3::ZERO.lerp(Vec3::from(rotation), t),
    }
}

/// The bone effects for a card in its stored state: none unless pregnant.
pub fn effects(ext: &ExtData) -> Vec<(&'static str, Modifier)> {
    let week = week(ext);
    if week <= 0 {
        return Vec::new();
    }
    let t = effect_percent(week);
    // The belly size is max(percent, inflation + percent / 2) with no
    // inflation, which is the percent again.
    let belly = BELLY.iter().map(|&(bone, s, p, r)| (bone, lerp(s, p, r, t)));
    let rest = PREGNANCY
        .iter()
        .map(|&(bone, s)| (bone, lerp(s, [0.0; 3], [0.0; 3], t)));
    rest.chain(belly).collect()
}

/// Adds bone effects to the card's ABMX modifiers the way
/// `CombineModifiers` does: scales and lengths multiply, offsets and
/// rotations add. A bone without a body modifier gets one.
pub fn combine(mods: &mut Vec<BoneModifier>, effects: &[(&str, Modifier)]) {
    for (bone, e) in effects {
        match mods.iter_mut().find(|m| m.location < 10 && m.bone == *bone) {
            Some(m) => {
                for d in &mut m.per_outfit {
                    d.scale *= e.scale;
                    d.length *= e.length;
                    d.position += e.position;
                    d.rotation += e.rotation;
                }
            }
            None => mods.push(BoneModifier {
                bone: bone.to_string(),
                per_outfit: vec![*e],
                location: 1,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_card::PluginData;

    fn ext(week: i64) -> ExtData {
        let mut p = PluginData::default();
        p.data.insert("Week".into(), Value::from(week));
        let mut ext = ExtData::new();
        ext.insert(KEY.into(), p);
        ext
    }

    #[test]
    fn percent_follows_the_week() {
        assert_eq!(effect_percent(1), 0.0);
        assert!((effect_percent(21) - 0.5).abs() < 1e-6);
        assert_eq!(effect_percent(41), 1.0);
        // Past the leave week the body is back to normal.
        assert_eq!(effect_percent(42), 0.0);
    }

    #[test]
    fn only_pregnant_cards_get_effects() {
        assert!(effects(&ExtData::new()).is_empty());
        assert!(effects(&ext(0)).is_empty());
        let full = effects(&ext(41));
        assert_eq!(full.len(), 27);
        let spine = full.iter().find(|(b, _)| *b == "cf_s_spine01").unwrap().1;
        assert_eq!(spine.scale, Vec3::new(1.62, 1.5, 1.9));
        assert_eq!(spine.position, Vec3::new(0.0, 0.0, 0.05));
        assert_eq!(spine.rotation, Vec3::new(10.0, 0.0, 0.0));
        let half = effects(&ext(21));
        let bust = half.iter().find(|(b, _)| *b == "cf_d_bust01_L").unwrap().1;
        assert!((bust.scale.x - 1.1).abs() < 1e-6);
    }

    #[test]
    fn effects_combine_with_card_modifiers() {
        let mut mods = vec![BoneModifier {
            bone: "cf_s_waist02".into(),
            per_outfit: vec![Modifier {
                scale: Vec3::new(2.0, 1.0, 1.0),
                length: 1.0,
                position: Vec3::new(0.0, 0.1, 0.0),
                rotation: Vec3::ZERO,
            }],
            location: 1,
        }];
        combine(&mut mods, &effects(&ext(41)));
        assert_eq!(mods.len(), 27);
        let waist = &mods[0].per_outfit[0];
        assert!((waist.scale.x - 2.34).abs() < 1e-6);
        assert_eq!(waist.position, Vec3::new(0.0, 0.1, 0.0));
    }
}
