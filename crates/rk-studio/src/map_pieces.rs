// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The pieces of a map that can be moved: its furniture and props, which
//! the game left out of its baked light. Walls, floors, the sky and the
//! scenery stay the map itself.
//!
//! The game names pieces after its files (`desk00 (3)`,
//! `mo_koi_map00_yuka00_table00_t`, `p_o_koi_hoko`): the names here keep
//! the word that says what a piece is, in English where it is known, and
//! the rest as it is, so that a mod's map reads the same way.

use glam::Vec3;

/// A movable piece of a loaded map.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    /// Its place in the map's list of objects, which the renderer moves
    /// it by.
    pub index: usize,
    /// The game's name of it, and which of the pieces of that name it is.
    pub name: String,
    pub nth: u32,
    /// Where it stands in the map: the middle of its foot.
    pub rest: Vec3,
    /// What it is, as the Outliner names it: `Desk`, `Chair`.
    pub label: String,
}

/// The largest a movable piece is across, in metres: furniture, not
/// buildings.
const LARGEST: f32 = 4.0;

/// Words of the game's names that say where a piece belongs, not what it
/// is.
fn placement_word(word: &str) -> bool {
    let bare = word.trim_end_matches(|c: char| c.is_ascii_digit());
    matches!(bare, "mo" | "o" | "p" | "koi" | "map" | "yuka" | "wall" | "kabe")
}

/// Words that make a piece part of the building.
fn structure_word(word: &str) -> bool {
    let bare = word.trim_end_matches(|c: char| c.is_ascii_digit());
    matches!(
        bare,
        "wall" | "kabe" | "sotokabe" | "yane" | "tenjo" | "sky" | "back" | "balckback"
    )
}

/// The game's Japanese words for what pieces are, in English.
fn english(word: &str) -> Option<&'static str> {
    Some(match word {
        "isu" => "Chair",
        "hana" => "Flower",
        "kuki" => "Stem",
        "kusa" => "Grass",
        "kadan" => "Flower bed",
        "makura" => "Pillow",
        "katen" => "Curtain",
        "zikanwari" => "Timetable",
        "kyoutaku" => "Teacher's desk",
        "hon" => "Book",
        "tana" => "Shelf",
        "tukue" => "Desk",
        "kagami" => "Mirror",
        "gomibako" => "Bin",
        "kokuban" => "Blackboard",
        "rokka" => "Locker",
        "benti" => "Bench",
        _ => return None,
    })
}

/// What a piece is called by its game name, or `None` for a name that
/// says nothing of what it is.
pub fn label(name: &str) -> Option<String> {
    let base = name.split(" (").next().unwrap_or(name);
    let words: Vec<&str> = base
        .split('_')
        .filter(|w| !w.is_empty() && !placement_word(w))
        .map(|w| w.trim_end_matches(|c: char| c.is_ascii_digit()))
        .filter(|w| w.len() > 1)
        .collect();
    let first = *words.first()?;
    let text = match english(first) {
        Some(known) => known.to_owned(),
        None => {
            let mut chars = first.chars();
            let head = chars.next()?.to_uppercase().collect::<String>();
            head + chars.as_str()
        }
    };
    Some(text)
}

/// A group's name for a folder of several: `Desks`, `Flower beds`.
pub fn plural(label: &str) -> String {
    if label.ends_with('s') || label.ends_with("Grass") {
        label.to_owned()
    } else {
        format!("{label}s")
    }
}

/// The movable pieces of `map`: not in the baked light, no larger than
/// furniture, not part of the building, and named for what they are.
pub fn movable(map: &rk_render::map::MapScene) -> Vec<Piece> {
    let mut seen: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for (index, o) in map.objects.iter().enumerate() {
        let nth = {
            let n = seen.entry(o.name.as_str()).or_insert(0);
            *n += 1;
            *n - 1
        };
        if o.lightmap.is_some() || o.mesh.positions.is_empty() || !map.seen(o) {
            continue;
        }
        if o.name.split('_').any(structure_word) {
            continue;
        }
        let (lo, hi) = o.mesh.bounds;
        let (lo, hi) = (Vec3::from(lo), Vec3::from(hi));
        if (hi - lo).max_element() > LARGEST {
            continue;
        }
        let Some(label) = label(&o.name) else { continue };
        out.push(Piece {
            index,
            name: o.name.clone(),
            nth,
            rest: Vec3::new((lo.x + hi.x) / 2.0, lo.y, (lo.z + hi.z) / 2.0),
            label,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_are_named_for_what_they_are() {
        assert_eq!(label("desk00 (3)").as_deref(), Some("Desk"));
        assert_eq!(label("isu").as_deref(), Some("Chair"));
        assert_eq!(label("mo_koi_map00_yuka00_table00_t").as_deref(), Some("Table"));
        assert_eq!(label("p_o_koi_kadan").as_deref(), Some("Flower bed"));
        assert_eq!(label("o_koi_map08_zikanwari").as_deref(), Some("Timetable"));
        // A mod's word stays as it is.
        assert_eq!(label("p_o_koi_nisu").as_deref(), Some("Nisu"));
        // A name that says only where it is, names nothing.
        assert_eq!(label("mo_koi_map14_yuka00_t"), None);
        assert_eq!(plural("Desk"), "Desks");
        assert_eq!(plural("Grass"), "Grass");
    }
}
