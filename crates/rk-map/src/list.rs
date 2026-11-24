// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The maps CharaStudio lists. Every bundle under `studio/info/` may hold
//! sheets named `Map_<file>`, and mods bring the same sheets as CSV files
//! (`abdata/studio/info/<folder>/Map_<name>.csv`). A row is the game's
//! `MapLoadInfo`: number, name, bundle, scene (level) name, manifest, and
//! the bundle and file of the table of walls that hide.

use rk_chara::animations::{mod_source, sheet_rows, source_of, Rows, Source};
use rk_unityfs::class;
use rk_vfs::Game;

/// One map of the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub number: i32,
    /// The name the game shows.
    pub name: String,
    /// The scene bundle, as a path under `abdata`.
    pub bundle: String,
    /// The level in the bundle, the `BuildPlayer-<level>` node.
    pub level: String,
    pub manifest: String,
    /// The bundle and the file of the table of walls that hide when the
    /// camera is outside them; empty when the map has none.
    pub vanish_bundle: String,
    pub vanish_file: String,
    /// Who lists it.
    pub source: Source,
}

/// The rows of map sheets after the two the game skips, as entries.
/// `listed` is who brings the sheet.
pub fn entries(sheet: &Rows, listed: &Source) -> Vec<Entry> {
    sheet
        .iter()
        .skip(2)
        .filter_map(|row| {
            let cell = |i: usize| row.get(i).map(|c| c.trim()).unwrap_or_default().to_owned();
            let number = cell(0).parse().ok()?;
            let bundle = cell(2);
            if bundle.is_empty() {
                return None;
            }
            Some(Entry {
                number,
                name: cell(1),
                bundle,
                level: cell(3),
                manifest: cell(4),
                vanish_bundle: cell(5),
                vanish_file: cell(6),
                source: listed.clone(),
            })
        })
        .collect()
}

/// Whether a sheet's name makes it a map sheet.
fn is_map_sheet(name: &str) -> bool {
    name.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("map_"))
}

/// The maps of the game, the packages and the mods, by number. A later
/// sheet replaces an earlier map of the same number, as the game reads
/// them into one table: the game's bundles first, then the mods' CSV
/// files. With `missing`, maps whose bundle is not installed are kept.
pub fn read(game: &Game, missing: bool) -> Vec<Entry> {
    let installed = |bundle: &str| missing || game.assets.index.bundles.contains_key(&rk_vfs::index::norm(bundle));
    let mut maps: Vec<Entry> = Vec::new();
    let mut add = |found: Vec<Entry>| {
        for entry in found.into_iter().filter(|e| installed(&e.bundle)) {
            match maps.iter_mut().find(|m| m.number == entry.number) {
                Some(old) => *old = entry,
                None => maps.push(entry),
            }
        }
    };
    let mut paths: Vec<&String> = game
        .assets
        .index
        .bundles
        .keys()
        .filter(|p| p.starts_with("studio/info/"))
        .collect();
    paths.sort();
    for path in paths {
        let listed = source_of(&game.assets.index, path).unwrap_or_default();
        // The game's own bundle first, then the mods' of the same name.
        for bundle in game.assets.bundles(path).into_iter().rev() {
            for file in &bundle.files {
                for info in file.sf.objects_of(class::MONO_BEHAVIOUR) {
                    let Some(name) = file.sf.name(info) else { continue };
                    if !is_map_sheet(&name) {
                        continue;
                    }
                    if let Ok(sheet) = file.sf.read(info) {
                        // A package's map comes from the package that has
                        // its scene: the lists are in more than one.
                        let mut found = entries(&sheet_rows(&sheet), &listed);
                        if matches!(listed, Source::Package(_)) {
                            for entry in &mut found {
                                if let Some(own @ Source::Package(_)) = source_of(&game.assets.index, &entry.bundle) {
                                    entry.source = own;
                                }
                            }
                        }
                        add(found);
                    }
                }
            }
        }
    }
    for sheet in game.assets.index.studio_sheets.iter().filter(|s| is_map_sheet(&s.name)) {
        let listed = sheet
            .mod_idx
            .map_or(Source::Game, |i| mod_source(&game.assets.index, i));
        add(entries(&sheet.rows, &listed));
    }
    maps.sort_by_key(|m| m.number);
    maps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(rows: &[&[&str]]) -> Rows {
        rows.iter()
            .map(|r| r.iter().map(|c| (*c).to_owned()).collect())
            .collect()
    }

    #[test]
    fn a_map_row_names_its_bundle_and_level() {
        let sheet = rows(&[
            &["番号", "名前", "バンドル", "シーン"],
            &["", "", "", ""],
            &[
                "0",
                "マイルーム",
                "map/scene/myroom.unity3d",
                "MyRoom",
                "abdata",
                "h/list/00.unity3d",
                "map_col_00",
            ],
            // Some mods leave the last columns out.
            &["101", "Beach", "map/scene/beach.unity3d", "Beach"],
            &["x", "not a row"],
        ]);
        let found = entries(&sheet, &Source::Game);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].level, "MyRoom");
        assert_eq!(found[0].vanish_file, "map_col_00");
        assert_eq!(found[1].number, 101);
        assert_eq!(found[1].vanish_bundle, "");
    }

    #[test]
    fn map_sheets_are_named_map() {
        assert!(is_map_sheet("Map_00"));
        assert!(is_map_sheet("map_ds_01"));
        assert!(!is_map_sheet("MapInfo"));
        assert!(!is_map_sheet("Anime_00_01_02"));
    }
}
