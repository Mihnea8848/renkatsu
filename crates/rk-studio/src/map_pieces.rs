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

/// The letters a word of a name starts with, which say what it is:
/// `kabe04g` is `kabe`, `tana12Bc` is `tana`.
fn stem(word: &str) -> String {
    word.chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_lowercase()
}

/// Words of the game's names that say where a piece belongs, not what it
/// is.
fn placement_word(stem: &str) -> bool {
    matches!(
        stem,
        "mo" | "o" | "p" | "koi" | "map" | "yuka" | "wall" | "kabe" | "gura" | "mi" | "kouen" | "kmichi" | "mt"
    )
}

/// Words that make a piece part of the building or the set, not
/// furniture: walls and ceilings, stall panels, blackouts, the crowds
/// and shadows painted on cards, the live stage's moving rig.
fn structure_word(stem: &str) -> bool {
    matches!(
        stem,
        "wall"
            | "kabe"
            | "sotokabe"
            | "yane"
            | "tenjo"
            | "ten"
            | "sky"
            | "back"
            | "balckback"
            | "black"
            | "front"
            | "side"
            | "doa"
            | "kousya"
            | "geo"
            | "mob"
            | "shadow"
    ) || stem.starts_with("speaker")
}

/// The game's words for what pieces are, in English.
fn english(stem: &str) -> Option<&'static str> {
    Some(match stem {
        "isu" | "chair" => "Chair",
        "maruisu" => "Stool",
        "desk" | "tukue" => "Desk",
        "dining" => "Dining table",
        "tyabu" => "Low table",
        "table" => "Table",
        "hana" | "flower" => "Flower",
        "kuki" => "Stem",
        "kusa" => "Grass",
        "ueki" => "Potted plant",
        "kadan" => "Flower bed",
        "makura" => "Pillow",
        "zabuon" => "Cushion",
        "katen" => "Curtain",
        "zikanwari" => "Timetable",
        "kyoutaku" => "Teacher's desk",
        "hon" | "book" => "Book",
        "tana" | "tanaba" => "Shelf",
        "kagami" => "Mirror",
        "gomibako" => "Bin",
        "kokuban" => "Blackboard",
        "rokka" | "rocker" => "Locker",
        "benti" => "Bench",
        "denwa" => "Telephone",
        "corn" => "Cone",
        "ball" => "Ball",
        "pen" => "Pen",
        "syoruiire" => "Document tray",
        "tyawan" => "Bowl",
        "hart" => "Heart",
        "senpu" => "Fan",
        "senbe" => "Rice cracker",
        "light" | "ramp" => "Lamp",
        "suito" => "Water bottle",
        "cos" => "Costume",
        "purabako" => "Plastic box",
        "shitugaiki" => "Outdoor unit",
        "saku" => "Fence",
        "zihan" => "Vending machine",
        "water" | "mizu" => "Water",
        "tao" => "Towel",
        "caution" => "Caution sign",
        "time" => "Clock",
        "pot" => "Pot",
        "renzi" => "Microwave",
        "syouka" => "Fire extinguisher",
        "souzi" => "Cleaning tools",
        "mat" => "Mat",
        "tube" => "Tube",
        "bed" => "Bed",
        "komono" => "Small item",
        "mike" => "Microphone",
        "cont" => "Remote control",
        "haizara" => "Ashtray",
        "marakasu" => "Maracas",
        "sofa" => "Sofa",
        "hunnsui" => "Fountain",
        "dai" => "Stand",
        "air" => "Air conditioner",
        _ => return None,
    })
}

/// What a piece is called by its game name, or `None` for a name that
/// says nothing of what it is.
pub fn label(name: &str) -> Option<String> {
    let base = name.split(" (").next().unwrap_or(name);
    // The first word that names something: known, or long enough not to
    // be one of the game's codes.
    let word = base
        .split('_')
        .map(stem)
        .filter(|w| !w.is_empty() && !placement_word(w))
        .find(|w| english(w).is_some() || w.len() > 2)?;
    Some(match english(&word) {
        Some(known) => known.to_owned(),
        None => {
            let mut chars = word.chars();
            let head = chars.next()?.to_uppercase().collect::<String>();
            head + chars.as_str()
        }
    })
}

/// Whether a name makes a piece part of the building or the set.
fn structure(name: &str) -> bool {
    name.split(['_', ' ']).any(|w| structure_word(&stem(w))) || stem(name).starts_with("speaker")
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
        if structure(&o.name) {
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
        assert_eq!(label("o_koi_map100h_00"), None);
        // Codes give way to the word after them.
        assert_eq!(label("p_o_koi_ei_mike00 (1)").as_deref(), Some("Microphone"));
        assert_eq!(label("mo_koi_map17_gura00_saku01").as_deref(), Some("Fence"));
        assert_eq!(label("p_o_koi_tana12Bc_00 (9)").as_deref(), Some("Shelf"));
        assert_eq!(label("o_kouen_isu_10").as_deref(), Some("Chair"));
        // Walls, crowds and the stage's rig are not furniture.
        assert!(structure("o_koi_map102_kabe04g_00"));
        assert!(structure("mo_koi_map70_mob057"));
        assert!(structure("spotGlow_geo"));
        assert!(structure("speakerBig_E"));
        assert!(!structure("desk00 (3)"));
        assert_eq!(plural("Desk"), "Desks");
        assert_eq!(plural("Grass"), "Grass");
    }
}
