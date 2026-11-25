// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! KK_Ahegao (1.12): in the main game's H scenes, after a number of
//! climaxes, a fast loop or climax state sets the face it is configured
//! with: eyebrow, eye and mouth patterns, the eyes rolled up, tears and
//! blush. Studio never runs it. Renkatsu offers that face as a preset,
//! read from the plugin's own settings, `BepInEx/config/KK_Ahegao.cfg`;
//! its eye roll, which also carries the picture in the irises, is
//! `crate::face`'s.

use crate::expression_control::Preset;
use crate::face::Face;
use std::collections::HashMap;
use std::path::Path;

/// The face KK_Ahegao sets, from its settings' text. A part its settings
/// leave alone (`Set Eyebrows = false` and the like) keeps a new face's.
pub fn face_of(cfg: &str) -> Face {
    let values: HashMap<&str, &str> = cfg
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && !l.starts_with('['))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect();
    let on = |key: &str| values.get(key).is_none_or(|v| v.eq_ignore_ascii_case("true"));
    let number = |key: &str, default: f32| values.get(key).and_then(|v| v.parse().ok()).unwrap_or(default);
    // The plugin's defaults where its file says nothing.
    let mut face = Face::default();
    if on("Set Eyebrows") {
        face.eyebrows = number("Ahegao Eyebrow ID", 2.0) as i32;
    }
    if on("Set Eyes") {
        face.eyes = number("Ahegao Eye ID", 25.0) as i32;
    }
    if on("Set Mouth") {
        face.mouth = number("Ahegao Mouth ID", 24.0) as i32;
    }
    if on("Eye Rolling") {
        face.eye_y = number("Eye Roll Amount", 0.25).clamp(0.0, 0.5);
    }
    if on("Set Tears") {
        face.tears = number("Tears Level", 0.0).clamp(0.0, 3.0) as u8;
    }
    if on("Set Blush") {
        face.blush = number("Blush Level", 0.0).clamp(0.0, 1.0);
    }
    face
}

/// The plugin's face as a preset, when the game folder has the plugin.
pub fn of_game(game_dir: &Path) -> Vec<Preset> {
    let installed = game_dir.join("BepInEx/plugins/KK_Ahegao.dll").exists();
    let cfg = std::fs::read_to_string(game_dir.join("BepInEx/config/KK_Ahegao.cfg"));
    if !installed && cfg.is_err() {
        return Vec::new();
    }
    vec![Preset {
        name: "Ahegao".into(),
        face: face_of(&cfg.unwrap_or_default()),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plugins_settings_make_its_face() {
        let cfg = "[Settings]\n## How many\nAhegao Count = 3\nSet Eyebrows = true\nAhegao Eyebrow ID = 2\n\
                   Ahegao Eye ID = 25\nAhegao Mouth ID = 24\nEye Rolling = true\nEye Roll Amount = 0.25\n\
                   Set Tears = true\nTears Level = 2\nSet Blush = false\nBlush Level = 0.8\n";
        let f = face_of(cfg);
        assert_eq!((f.eyebrows, f.eyes, f.mouth, f.tears), (2, 25, 24, 2));
        assert_eq!(f.eye_y, 0.25);
        assert_eq!(f.blush, Face::default().blush, "Set Blush off leaves the blush alone");
        // Without a file, the plugin's own defaults.
        let d = face_of("");
        assert_eq!((d.eyebrows, d.eyes, d.mouth, d.eye_y), (2, 25, 24, 0.25));
    }
}
