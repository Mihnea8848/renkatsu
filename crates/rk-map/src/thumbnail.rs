// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The pictures the game shows of its maps. The game's own map list
//! (`MapInfo`, in the bundles under `map/list/mapinfo/`) names each map's
//! scene bundle and a picture in the `map/thumbnail/` bundles, such as
//! `sp_map_23_00`. The last number is the time of day: 00, 01 and 02 for
//! day, evening and night, where the map has them.

use crate::unity::TimeOfDay;
use rk_unityfs::{class, UValue};
use rk_vfs::Game;
use std::collections::HashMap;

/// A picture, as RGBA bytes with the top row first.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Where the game's map list puts the picture of every map it pictures:
/// by scene bundle, the picture's bundle and name.
pub fn table(game: &Game) -> HashMap<String, (String, String)> {
    let mut paths: Vec<&String> = game
        .assets
        .index
        .bundles
        .keys()
        .filter(|p| p.starts_with("map/list/mapinfo/"))
        .collect();
    paths.sort();
    // A later list replaces an earlier one's row, as the game reads them.
    let mut found = HashMap::new();
    for path in paths {
        for loaded in game.assets.bundles(path).into_iter().rev() {
            for file in &loaded.files {
                for info in file.sf.objects_of(class::MONO_BEHAVIOUR) {
                    let Ok(o) = file.sf.read(info) else { continue };
                    for row in o.array("param").unwrap_or_default() {
                        let Some(row) = row.as_object() else { continue };
                        let text = |key: &str| row.get(key).and_then(UValue::as_str).unwrap_or_default().to_owned();
                        let (from, name) = (text("ThumbnailBundle"), text("ThumbnailAsset"));
                        if !from.is_empty() && !name.is_empty() {
                            found.insert(rk_vfs::index::norm(&text("AssetBundleName")), (from, name));
                        }
                    }
                }
            }
        }
    }
    found
}

/// Where the game's map list puts the picture of the map whose scene
/// bundle is `bundle`: the picture's bundle and name.
pub fn listed(game: &Game, bundle: &str) -> Option<(String, String)> {
    table(game).remove(&rk_vfs::index::norm(bundle))
}

/// The name of the picture of a time of day: the listed name with its
/// last number changed to the time's.
pub fn name_at(listed: &str, time: TimeOfDay) -> String {
    let number = match time {
        TimeOfDay::Day => 0,
        TimeOfDay::Evening => 1,
        TimeOfDay::Night => 2,
    };
    match listed.rsplit_once('_') {
        Some((stem, end)) if !end.is_empty() && end.chars().all(|c| c.is_ascii_digit()) => {
            format!("{stem}_{number:02}")
        }
        _ => listed.to_owned(),
    }
}

/// The game's picture of the map whose scene bundle is `bundle`, at the
/// time of day `time`, or the one its list names when there is none for
/// that time. `None` for a map the list does not picture.
pub fn picture(game: &Game, bundle: &str, time: TimeOfDay) -> Option<Picture> {
    picture_of(game, &listed(game, bundle)?, time)
}

/// Whether the game has a picture of a listed map at the time of day
/// `time`, which it has for the times the map has.
pub fn has_time(game: &Game, listed: &(String, String), time: TimeOfDay) -> bool {
    let (from, name) = listed;
    game.assets
        .find(from, &name_at(name, time), Some(class::TEXTURE2D), None)
        .is_some()
}

/// The picture of a listed map, from `table`, at the time of day `time`,
/// or the one the list names when there is none for that time.
pub fn picture_of(game: &Game, listed: &(String, String), time: TimeOfDay) -> Option<Picture> {
    let (from, name) = listed;
    let texture = game
        .assets
        .find(from, &name_at(name, time), Some(class::TEXTURE2D), None)
        .or_else(|| game.assets.find(from, name, Some(class::TEXTURE2D), None))?;
    let (tex, pixels) = game.assets.texture(&texture).ok()?;
    let (width, height) = (tex.width, tex.height);
    let row = width as usize * 4;
    if width == 0 || height == 0 || pixels.len() < row * height as usize {
        return None;
    }
    // Unity keeps the bottom row first.
    let rgba = pixels[..row * height as usize]
        .chunks_exact(row)
        .rev()
        .flatten()
        .copied()
        .collect();
    Some(Picture { width, height, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_number_of_a_picture_is_its_time() {
        assert_eq!(name_at("sp_map_23_00", TimeOfDay::Night), "sp_map_23_02");
        assert_eq!(name_at("sp_map_34_02", TimeOfDay::Day), "sp_map_34_00");
        assert_eq!(name_at("picture", TimeOfDay::Evening), "picture");
    }
}
