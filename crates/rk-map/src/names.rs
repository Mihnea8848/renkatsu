// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! English names for the maps of the game and of kPlug, which CharaStudio
//! lists in Japanese. A map is known by its scene bundle, which stays the
//! same whatever number a list gives it.

/// The scene bundles of the game's and kPlug's maps with their English
/// names, in the order of CharaStudio's list.
const NAMES: [(&str, &str); 45] = [
    ("map/scene/myroom.unity3d", "Bedroom"),
    ("map/scene/1f.unity3d", "1F Hallway"),
    ("map/scene/2f.unity3d", "2F Hallway"),
    ("map/scene/3f.unity3d", "3F Hallway"),
    ("map/scene/1-1.unity3d", "Classroom 1-1"),
    ("map/scene/2-1.unity3d", "Classroom 2-1"),
    ("map/scene/2-2.unity3d", "Classroom 2-2"),
    ("map/scene/3-1.unity3d", "Classroom 3-1"),
    ("map/scene/1ftoilet.unity3d", "1F Girls' Toilet"),
    ("map/scene/2ftoilet.unity3d", "2F Girls' Toilet"),
    ("map/scene/3ftoilet.unity3d", "3F Girls' Toilet"),
    ("map/scene/staffadultroom.unity3d", "Staff Room"),
    ("map/scene/toiletmale.unity3d", "Staff Toilet (Men)"),
    ("map/scene/healthroom.unity3d", "Infirmary"),
    ("map/scene/libraryroom.unity3d", "Library"),
    ("map/scene/staffroom.unity3d", "Club Room"),
    ("map/scene/staffcomicresearchroom.unity3d", "Manga Club"),
    ("map/scene/japanesestyleroom.unity3d", "Japanese Room"),
    ("map/scene/gymnasiumwarehouse.unity3d", "Gym Storeroom"),
    ("map/scene/gymnasium.unity3d", "Gym"),
    ("map/scene/courtyard.unity3d", "Courtyard"),
    ("map/scene/ground.unity3d", "Sports Ground"),
    ("map/scene/rooftop.unity3d", "Rooftop"),
    ("map/scene/pool.unity3d", "Pool"),
    ("map/scene/diningroom.unity3d", "Cafeteria"),
    ("map/scene/shawerroom.unity3d", "Shower Room"),
    ("map/scene/lockerroom.unity3d", "Locker Room"),
    ("map/scene/backyard.unity3d", "Backyard"),
    ("map/scene/livestage_s.unity3d", "Live Stage"),
    ("map/scene/gymnasiumlive.unity3d", "Gym Concert"),
    ("map/scene/toiletpeep.unity3d", "Girls' Toilet Stall"),
    ("map/scene/shawerroompeep.unity3d", "Shower Stall"),
    ("map/scene/lockerroompeep.unity3d", "Inside a Locker"),
    ("studio/map/bushitu.unity3d", "Empty Club Room"),
    ("studio/map/akikyoushitu.unity3d", "Empty Classroom"),
    ("map/scene/lovehotel.unity3d", "Love Hotel"),
    ("map/scene/karaoke.unity3d", "Karaoke"),
    ("map/scene/church.unity3d", "Church"),
    ("map/scene/traincrowded.unity3d", "Crowded Train"),
    ("map/scene/train.unity3d", "Train"),
    ("map/scene/park.unity3d", "Park"),
    ("map/scene/warehouse.unity3d", "Equipment Storeroom"),
    ("map/scene/thegirlroom.unity3d", "Girl's Room"),
    ("map/scene/thehotsprings.unity3d", "Hot Springs"),
    ("map/scene/thebeach.unity3d", "Beach"),
];

/// The English name of the map whose scene bundle is `bundle`, for the
/// maps of the game and of kPlug.
pub fn english(bundle: &str) -> Option<&'static str> {
    let bundle = rk_vfs::index::norm(bundle);
    NAMES.iter().find(|(b, _)| *b == bundle).map(|(_, name)| *name)
}

/// The name a map shows under: its English name when there is one, else
/// the name its list gives it.
pub fn shown(entry: &crate::list::Entry) -> String {
    english(&entry.bundle).map_or_else(|| entry.name.trim().to_owned(), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_are_named_by_their_bundle() {
        assert_eq!(english("map/scene/rooftop.unity3d"), Some("Rooftop"));
        assert_eq!(english("Map\\Scene\\MyRoom.unity3d"), Some("Bedroom"));
        assert_eq!(english("map/scene/someone_elses.unity3d"), None);
        // Every bundle is named once.
        let mut bundles: Vec<&str> = NAMES.iter().map(|(b, _)| *b).collect();
        bundles.sort_unstable();
        bundles.dedup();
        assert_eq!(bundles.len(), NAMES.len());
    }
}
