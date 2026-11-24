// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The BetterPenetration plugin (`KK_Studio_BetterPenetration` 4.5.5.2):
//! the male shaft aimed and bent at a target on a partner, its colliders
//! given to her dynamic bones, and the mouth pulled.
//!
//! This part is the settings, as its controller keeps them on the card and
//! in scenes (`BetterPenetrationController.OnCardBeingSaved` and
//! `OnReload`): extended data `BetterPenetrationController`, version 1.
//! A value the data does not hold keeps the controller's default.

use rk_card::{ExtData, Value};
use serde::{Deserialize, Serialize};

/// The extended data's key, the controller's name with KKAPI.
pub const KEY: &str = "BetterPenetrationController";

/// What the shaft looks for on its own (`ControllerOptions.AutoTarget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoTarget {
    #[default]
    Off,
    Vaginal,
    Anal,
    Oral,
}

impl AutoTarget {
    /// The plugin's number for it.
    pub fn of_number(n: i64) -> AutoTarget {
        match n {
            1 => AutoTarget::Vaginal,
            2 => AutoTarget::Anal,
            3 => AutoTarget::Oral,
            _ => AutoTarget::Off,
        }
    }

    pub const ALL: [AutoTarget; 4] = [AutoTarget::Off, AutoTarget::Vaginal, AutoTarget::Anal, AutoTarget::Oral];

    pub fn label(self) -> &'static str {
        match self {
            AutoTarget::Off => "Off",
            AutoTarget::Vaginal => "Vaginal",
            AutoTarget::Anal => "Anal",
            AutoTarget::Oral => "Oral",
        }
    }
}

/// One character's settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// The controller runs (`Behaviour.enabled`).
    pub enabled: bool,
    /// How much of the length gives way against a target closer than it
    /// (`danLengthSquish`).
    pub length_squish: f32,
    /// How much thicker it grows as it does (`danGirthSquish`).
    pub girth_squish: f32,
    /// The share of the length that may close in before it gives way
    /// (`squishThreshold`).
    pub squish_threshold: f32,
    /// The shaft colliders' radius and length, as shares of their own
    /// (`danRadiusScale`, `danLengthScale`).
    pub collider_radius_scale: f32,
    pub collider_length_scale: f32,
    pub auto_target: AutoTarget,
    /// The mouth's furthest push and pull, and how fast it follows and
    /// returns (`maxOralPush`, `maxOralPull`, `oralPullRate`,
    /// `oralReturnRate`).
    pub max_push: f32,
    pub max_pull: f32,
    pub pull_rate: f32,
    pub return_rate: f32,
    /// The deeper virtual points push the belly's dynamic bones, their
    /// radius times `belly_bulge_scale`.
    pub belly_bulge: bool,
    pub belly_bulge_scale: f32,
}

impl Default for Settings {
    /// The controller's defaults in `OnReload`.
    fn default() -> Self {
        Settings {
            enabled: true,
            length_squish: 0.6,
            girth_squish: 0.2,
            squish_threshold: 0.2,
            collider_radius_scale: 1.0,
            collider_length_scale: 1.0,
            auto_target: AutoTarget::Off,
            max_push: 0.002,
            max_pull: 0.01,
            pull_rate: 1.8,
            return_rate: 0.03,
            belly_bulge: false,
            belly_bulge_scale: 1.0,
        }
    }
}

/// A card's settings, the defaults where it holds none.
pub fn read(ext: &ExtData) -> Settings {
    let mut s = Settings::default();
    let Some(p) = ext.get(KEY) else { return s };
    let float = |key: &str, into: &mut f32| {
        if let Some(v) = p.data.get(key).and_then(Value::as_f64) {
            *into = v as f32;
        }
    };
    let flag = |key: &str, into: &mut bool| {
        if let Some(v) = p.data.get(key).and_then(Value::as_bool) {
            *into = v;
        }
    };
    flag("Enabled", &mut s.enabled);
    float("LengthSquish", &mut s.length_squish);
    float("GirthSquish", &mut s.girth_squish);
    float("SquishThreshold", &mut s.squish_threshold);
    float("ColliderRadiusScale", &mut s.collider_radius_scale);
    float("ColliderLengthScale", &mut s.collider_length_scale);
    if let Some(n) = p.data.get("DanAutoTarget").and_then(Value::as_i64) {
        s.auto_target = AutoTarget::of_number(n);
    }
    float("MaxPush", &mut s.max_push);
    float("MaxPull", &mut s.max_pull);
    float("PullRate", &mut s.pull_rate);
    float("ReturnRate", &mut s.return_rate);
    flag("EnableBellyBulge", &mut s.belly_bulge);
    float("BellyBulgeScale", &mut s.belly_bulge_scale);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_without_the_data_has_the_defaults() {
        assert_eq!(read(&ExtData::default()), Settings::default());
    }

    #[test]
    fn the_plugins_numbers_name_its_targets() {
        assert_eq!(AutoTarget::of_number(0), AutoTarget::Off);
        assert_eq!(AutoTarget::of_number(1), AutoTarget::Vaginal);
        assert_eq!(AutoTarget::of_number(2), AutoTarget::Anal);
        assert_eq!(AutoTarget::of_number(3), AutoTarget::Oral);
        assert_eq!(AutoTarget::of_number(9), AutoTarget::Off);
    }

    #[test]
    fn settings_keep_through_a_scene_file() {
        let s = Settings {
            auto_target: AutoTarget::Oral,
            girth_squish: 0.5,
            belly_bulge: true,
            ..Settings::default()
        };
        let text = toml::to_string(&s).unwrap();
        assert!(text.contains("auto_target = \"oral\""));
        assert_eq!(toml::from_str::<Settings>(&text).unwrap(), s);
    }

    #[test]
    fn a_saved_card_reads_its_settings() {
        let Some(dir) = rk_core::Config::load().ok().map(|c| c.game_dir) else {
            return;
        };
        let path = dir.join("UserData/chara/export/_CharacterExport_2026-05-13-22-13-10_01_Male.png");
        let Ok(card) = rk_card::Card::load(&path) else {
            eprintln!("skipped: no card with BetterPenetration data");
            return;
        };
        assert!(card.ext.contains_key(KEY), "the card carries the plugin's data");
        let s = read(&card.ext);
        assert!((0.0..=2.0).contains(&s.girth_squish) && (0.5..=1.5).contains(&s.collider_radius_scale));
    }
}
