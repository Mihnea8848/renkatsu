// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The bones of a character that objects are attached to: the main ones
//! under readable names, and the menu that chooses one.

use crate::state::Look;
use crate::widgets;
use egui::Ui;

/// The bones things are attached to most, by their names in the game's
/// skeleton, with the names shown for them.
pub const MAIN: [(&str, &str); 8] = [
    ("cf_j_head", "Head"),
    ("cf_j_neck", "Neck"),
    ("cf_j_spine03", "Chest"),
    ("cf_j_waist01", "Hips"),
    ("cf_j_hand_L", "Left hand"),
    ("cf_j_hand_R", "Right hand"),
    ("cf_j_foot_L", "Left foot"),
    ("cf_j_foot_R", "Right foot"),
];

/// The name a bone is shown under: a readable one for the main bones,
/// the skeleton's own for the rest.
pub fn label(bone: &str) -> &str {
    MAIN.iter()
        .find(|(name, _)| *name == bone)
        .map_or(bone, |(_, label)| label)
}

/// The main bones as menu items, with the chosen one marked. Returns the
/// bone that was clicked.
pub fn main_items(ui: &mut Ui, k: &Look, chosen: Option<&str>) -> Option<&'static str> {
    let mut clicked = None;
    for (name, label) in MAIN {
        if widgets::check_item(ui, k, chosen == Some(name), label, "").clicked() {
            clicked = Some(name);
        }
    }
    clicked
}

/// The menu that chooses a bone: none, the main bones, then every bone
/// of the character under a search. Returns what was chosen: a bone, or
/// `Some(None)` for none.
pub fn menu(
    ui: &mut Ui,
    k: &Look,
    search: &mut String,
    all: &[String],
    chosen: Option<&str>,
) -> Option<Option<String>> {
    let mut choice = None;
    if widgets::check_item(ui, k, chosen.is_none(), "None", "").clicked() {
        choice = Some(None);
    }
    ui.separator();
    if let Some(name) = main_items(ui, k, chosen) {
        choice = Some(Some(name.to_owned()));
    }
    if !all.is_empty() {
        ui.separator();
        widgets::search(ui, k, search, "Search", 200.0);
        let wanted = search.trim().to_lowercase();
        let matches = |name: &&String| {
            wanted.is_empty() || name.to_lowercase().contains(&wanted) || label(name).to_lowercase().contains(&wanted)
        };
        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
            ui.set_min_width(200.0);
            for name in all.iter().filter(matches) {
                if widgets::check_item(ui, k, chosen == Some(name), name, "").clicked() {
                    choice = Some(Some(name.clone()));
                }
            }
        });
    }
    if choice.is_some() {
        ui.close();
    }
    choice
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_bones_have_readable_names() {
        assert_eq!(label("cf_j_hand_L"), "Left hand");
        assert_eq!(label("cf_j_thumb01_L"), "cf_j_thumb01_L");
    }
}
