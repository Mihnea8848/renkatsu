// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The ExpressionControl plugin (`KK_ExpressionControl` 0.2.7.9): named
//! faces to pick from. Its presets live in `BepInEx/plugins/
//! ExpressionControl.xml`, each an `<item>` under `<Expressions>` with the
//! eyebrow, eye and mouth patterns, the eyes' and mouth's opening, blink,
//! hidden highlights, blush, tears, and its own eye roll (`eyeY`) and
//! smaller irises (`eyeSmall`); `face.rs` shows those two. Picking one sets
//! all of it at once (`SetExpression`). Renkatsu keeps presets of its own
//! in the same shape, in `expressions.toml` beside the studio's settings.

use crate::face::Face;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A named face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub face: Face,
}

/// The plugin's own preset names in English: the leading word, then
/// whatever number or share follows it (`笑顔2`, `涙Lv1`, `頬50%`).
const NAMES: &[(&str, &str)] = &[
    ("デフォルト", "Default"),
    ("笑顔", "Smile"),
    ("切ない", "Aching"),
    ("照れ", "Shy"),
    ("怒り", "Angry"),
    ("真剣", "Serious"),
    ("つまらない", "Bored"),
    ("苦しい", "Pained"),
    ("嫌悪", "Disgusted"),
    ("思案", "Thinking"),
    ("悲しい", "Sad"),
    ("泣き", "Crying"),
    ("焦り", "Flustered"),
    ("落胆", "Dejected"),
    ("困る", "Troubled"),
    ("ドヤ", "Smug"),
    ("緊張", "Nervous"),
    ("痛い", "Hurt"),
    ("感じ", "Aroused"),
    ("涙Lv", "Tears"),
    ("頬", "Blush"),
    ("瞳Y", "Eye roll"),
    ("瞳小", "Small irises"),
];

/// A preset's name in English when it is one of the plugin's own; any
/// other name stays as it is.
pub fn english(name: &str) -> String {
    let found = NAMES
        .iter()
        .filter(|(jp, _)| name.starts_with(jp))
        .max_by_key(|(jp, _)| jp.len());
    match found {
        Some((jp, en)) => {
            let rest = name[jp.len()..].trim();
            if rest.is_empty() {
                (*en).to_owned()
            } else {
                format!("{en} {rest}")
            }
        }
        None => name.to_owned(),
    }
}

/// The presets of an `ExpressionControl.xml`, in its order. Values it does
/// not store keep a new status's.
pub fn read_xml(text: &str) -> Result<Vec<Preset>> {
    let doc = roxmltree::Document::parse(text).context("ExpressionControl.xml is not XML")?;
    let Some(list) = doc.descendants().find(|n| n.has_tag_name("Expressions")) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for item in list.children().filter(|n| n.has_tag_name("item")) {
        let text = |key: &str| item.attribute(key);
        let int = |key: &str, default: i32| text(key).and_then(|v| v.trim().parse().ok()).unwrap_or(default);
        let float = |key: &str, default: f32| text(key).and_then(|v| v.trim().parse().ok()).unwrap_or(default);
        let flag = |key: &str, default: bool| text(key).map_or(default, |v| v.trim().eq_ignore_ascii_case("true"));
        let start = Face::default();
        out.push(Preset {
            name: english(text("name").unwrap_or_default()),
            face: Face {
                eyebrows: int("eyebrowPtn", start.eyebrows),
                eyes: int("eyesPtn", start.eyes),
                mouth: int("mouthPtn", start.mouth),
                eyes_open: float("eyeOpen", start.eyes_open),
                blink: flag("blink", start.blink),
                mouth_open: float("mouthOpen", start.mouth_open),
                tears: int("tearLv", 0).clamp(0, 3) as u8,
                blush: float("hohoAka", start.blush),
                hide_highlight: flag("hideHighlight", false),
                eye_y: float("eyeY", 0.0),
                eye_small: float("eyeSmall", 0.0),
            },
        });
    }
    Ok(out)
}

/// The plugin's presets in a game folder, when it has the plugin.
pub fn of_game(game_dir: &Path) -> Vec<Preset> {
    let path = game_dir.join("BepInEx/plugins/ExpressionControl.xml");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| read_xml(&text).ok())
        .unwrap_or_default()
}

/// The file of Renkatsu's own presets.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default, rename = "expression")]
    presets: Vec<Preset>,
}

/// Renkatsu's own presets, none when the file is missing.
pub fn read_saved(path: &Path) -> Result<Vec<Preset>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(toml::from_str::<Saved>(&text)
            .with_context(|| format!("{} is not a list of expressions", path.display()))?
            .presets),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
    }
}

/// Writes Renkatsu's own presets.
pub fn save(path: &Path, presets: &[Preset]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string(&Saved {
        presets: presets.to_vec(),
    })?;
    std::fs::write(path, text).with_context(|| format!("could not write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plugins_presets_read_as_faces() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<root>
  <KeyConfig><showGUI>L</showGUI></KeyConfig>
  <Expressions>
    <item guid="a" name="Default" eyebrowPtn="0" eyesPtn="0" mouthPtn="0" hideHighlight="false" blink="true" eyeOpen="1" mouthOpen="0" hohoAka="0" tearLv="0" eyeY="0" eyeSmall="0" />
    <item guid="b" name="Rolled" eyebrowPtn="3" eyesPtn="12" mouthPtn="24" hideHighlight="true" blink="false" eyeOpen="0.6" mouthOpen="0.8" hohoAka="1" tearLv="2" eyeY="0.3" eyeSmall="0.2" />
  </Expressions>
</root>"#;
        let presets = read_xml(xml).unwrap();
        assert_eq!(presets.len(), 2);
        assert_eq!(presets[0].face, Face::default());
        let f = presets[1].face;
        assert_eq!((f.eyebrows, f.eyes, f.mouth, f.tears), (3, 12, 24, 2));
        assert!(f.hide_highlight && !f.blink);
        assert_eq!(
            (f.eyes_open, f.mouth_open, f.blush, f.eye_y, f.eye_small),
            (0.6, 0.8, 1.0, 0.3, 0.2)
        );
    }

    #[test]
    fn the_installed_plugins_presets_read() {
        let Some(dir) = rk_core::Config::load().ok().map(|c| c.game_dir) else {
            return;
        };
        if !dir.join("BepInEx/plugins/ExpressionControl.xml").exists() {
            eprintln!("skipped: no ExpressionControl.xml");
            return;
        }
        let presets = of_game(&dir);
        assert!(!presets.is_empty());
        assert!(presets.iter().all(|p| !p.name.is_empty()));
    }

    #[test]
    fn the_plugins_names_read_in_english() {
        assert_eq!(english("デフォルト"), "Default");
        assert_eq!(english("笑顔2"), "Smile 2");
        assert_eq!(english("涙Lv3"), "Tears 3");
        assert_eq!(english("頬50%"), "Blush 50%");
        assert_eq!(english("瞳小25%"), "Small irises 25%");
        assert_eq!(english("瞳Y10%"), "Eye roll 10%");
        assert_eq!(english("My face"), "My face");
    }

    #[test]
    fn own_presets_round_trip() {
        let dir = std::env::temp_dir().join(format!("rk-expressions-{}", std::process::id()));
        let path = dir.join("expressions.toml");
        assert!(read_saved(&path).unwrap().is_empty());
        let presets = vec![Preset {
            name: "Shy".into(),
            face: Face {
                eyes: 8,
                blush: 0.7,
                ..Face::default()
            },
        }];
        save(&path, &presets).unwrap();
        assert_eq!(read_saved(&path).unwrap(), presets);
        std::fs::remove_dir_all(dir).ok();
    }
}
