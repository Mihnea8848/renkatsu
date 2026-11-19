// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The maps a project owns. Importing a map builds it from the game at
//! each of its times of day and writes it to one map file under `Maps`
//! (see `rk_map::file`), with what it is and the game's picture of it
//! inside, so the project draws its maps without Koikatsu and opens them
//! without building them again.
//!
//! ```text
//! Maps/
//!   Rooftop.rkmap       the map, its description and its picture
//! ```
//!
//! Projects made with Renkatsu 0.1.8 kept a copy of the map's bundles in a
//! folder of their own, with a small text file beside them, the sidecar,
//! named like a map file. Those still open, built from the copies.
//!
//! ```text
//! Maps/
//!   Rooftop/
//!     Rooftop.rkmap     the sidecar: names, number, times of day, walls
//!     rooftop.unity3d   the scene bundle as the game has it
//!     thumbnail.png     the game's picture of the map
//! ```
//!
//! Scenes name a map by its file (or sidecar), relative to the project
//! folder.

use crate::project::{file_name, files_below, relative_to, Project};
use crate::scene::MapTime;
use anyhow::{bail, Context, Result};
use rk_vfs::Game;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The project folder maps go in.
pub const MAPS: &str = "Maps";
/// The extension of a map file, and of an older map's sidecar.
pub const EXTENSION: &str = rk_map::file::EXTENSION;
/// The version of the description this build writes: 1 was the text
/// sidecar, 2 is the description inside a map file.
pub const FORMAT: u32 = rk_map::file::META_FORMAT;

/// A bundle of a map, as copied into the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    /// Where the game keeps it, under `abdata`, which is where the map
    /// looks for it.
    pub path: String,
    /// The copy, beside the sidecar; empty in a map file, which holds
    /// no bundle.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
}

/// What a map's sidecar holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapFile {
    pub format: u32,
    /// The name the map shows under in the project.
    pub name: String,
    /// The name CharaStudio's list gives it.
    pub listed_name: String,
    /// Its number in CharaStudio's list, which CharaStudio scenes name it
    /// by.
    pub number: i32,
    /// The scene in its bundle.
    pub level: String,
    /// Who brings it: Koikatsu, a package or a mod.
    pub source: String,
    /// The times of day it has, in order.
    pub times: Vec<MapTime>,
    /// The game's picture of it, beside the sidecar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    /// The text of its table of walls that hide, as the game stores it;
    /// empty for a map without one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub walls: String,
    /// The scene bundle first, then the bundles it reads from.
    pub bundles: Vec<Bundle>,
}

impl MapFile {
    /// Reads what a map file says of its map, or an older sidecar.
    pub fn read(path: &Path) -> Result<MapFile> {
        let text = if rk_map::file::is_map_file(path) {
            rk_map::file::meta_text(path)?
        } else {
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?
        };
        let file: MapFile = toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
        if file.format > FORMAT {
            bail!(
                "{} was saved by a newer Renkatsu (format {})",
                path.display(),
                file.format
            );
        }
        if file.bundles.is_empty() {
            bail!("{} names no bundle", path.display());
        }
        Ok(file)
    }

    /// Writes an older map's text sidecar, for tests of reading one.
    #[cfg(test)]
    pub fn write(&self, path: &Path) -> Result<()> {
        std::fs::write(path, toml::to_string(self)?).with_context(|| format!("writing {}", path.display()))
    }

    /// The time a scene starts the map at: day when the map has it, else
    /// its first.
    pub fn first_time(&self) -> MapTime {
        if self.times.contains(&MapTime::Day) {
            MapTime::Day
        } else {
            self.times.first().copied().unwrap_or_default()
        }
    }

    /// The scene bundle, by where the game keeps it.
    pub fn scene_bundle(&self) -> &str {
        &self.bundles[0].path
    }
}

/// A map of the project: its sidecar, relative to the project, and what
/// it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMap {
    pub file: String,
    pub map: MapFile,
}

/// The maps a project folder owns, by name: every sidecar below its maps
/// folder that can be read.
pub fn in_project(root: &Path) -> Vec<ProjectMap> {
    let mut files = Vec::new();
    files_below(&root.join(MAPS), &mut files);
    let mut maps: Vec<ProjectMap> = files
        .iter()
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(EXTENSION)))
        .filter_map(|p| {
            Some(ProjectMap {
                file: relative_to(root, p)?,
                map: MapFile::read(p).ok()?,
            })
        })
        .collect();
    maps.sort_by_key(|m| m.map.name.to_lowercase());
    maps
}

/// The project's map whose scene bundle is `bundle`, when it has one.
pub fn owned(root: &Path, bundle: &str) -> Option<ProjectMap> {
    let bundle = rk_vfs::index::norm(bundle);
    in_project(root)
        .into_iter()
        .find(|m| rk_vfs::index::norm(m.map.scene_bundle()) == bundle)
}

/// A map of the game on its way into the project.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub entry: rk_map::list::Entry,
    /// The name it will show under.
    pub name: String,
    /// The scene bundle, then the bundles it reads from, with their sizes.
    pub bundles: Vec<(String, u64)>,
}

impl Plan {
    /// The size of the bundles the map is made from, which import counts
    /// its progress in.
    pub fn bytes(&self) -> u64 {
        self.bundles.iter().map(|b| b.1).sum()
    }
}

/// Works out what importing a map of the game reads.
pub fn plan(game: &Game, entry: &rk_map::list::Entry) -> Result<Plan> {
    let mut paths = vec![entry.bundle.clone()];
    paths.extend(rk_map::scene::dependencies(game, &entry.bundle)?);
    let bundles = paths
        .into_iter()
        .map(|path| {
            let (_, size) = game.assets.bundle_file(&path)?;
            Ok((path, size))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Plan {
        entry: entry.clone(),
        name: rk_map::names::shown(entry),
        bundles,
    })
}

/// Imports a map of the game into the project: writes its map file,
/// named after it, in the maps folder. `counted` hears of the progress, in
/// bytes of the map's bundles, and stops the import with an error. A map
/// the project has already is not imported again. Returns the map file,
/// relative to the project.
pub fn import(
    game: &Game,
    project: &Project,
    plan: &Plan,
    counted: &mut dyn FnMut(u64) -> Result<()>,
) -> Result<String> {
    if let Some(there) = owned(&project.root, &plan.entry.bundle) {
        return Ok(there.file);
    }
    let maps = project.root.join(MAPS);
    std::fs::create_dir_all(&maps).with_context(|| format!("creating {}", maps.display()))?;
    let base = file_name(&plan.name);
    let mut name = base.clone();
    let mut n = 1;
    // A folder of the same name is an older map's.
    while maps.join(&name).exists() || maps.join(format!("{name}.{EXTENSION}")).exists() {
        n += 1;
        name = format!("{base} {n}");
    }
    let path = maps.join(format!("{name}.{EXTENSION}"));
    let total = plan.bytes();
    let mut told = 0;
    rk_map::file::convert(game, &plan.entry, &plan.name, None, &path, &mut |done| {
        let now = ((total as f64 * f64::from(done)) as u64).max(told);
        let step = now - told;
        told = now;
        counted(step)
    })?;
    relative_to(&project.root, &path).context("the map is outside the project")
}

/// Opens a map of the project at a time of day, ready to upload: read
/// from its map file, or built from an older map's copy of its bundles.
/// With `compressed`, the textures stay as the game stores them, for a GPU
/// that draws them so.
pub fn build(file: &Path, time: MapTime, compressed: bool) -> Result<rk_render::map::MapScene> {
    if rk_map::file::is_map_file(file) {
        if rk_map::file::is_older(file) {
            remake(file);
        }
        let mut scene = rk_map::file::read(file, time.time_of_day(), compressed)?;
        // CharaStudio leaves the map's fog off unless its fog switch is on.
        scene.light.fog = None;
        return Ok(scene);
    }
    let sidecar = file;
    let map = MapFile::read(sidecar)?;
    let folder = sidecar.parent().unwrap_or(Path::new("."));
    let files: Vec<(String, PathBuf)> = map
        .bundles
        .iter()
        .map(|b| (b.path.clone(), folder.join(&b.file)))
        .collect();
    if let Some((_, missing)) = files.iter().find(|(_, f)| !f.is_file()) {
        bail!("{} is gone", missing.display());
    }
    let game = Game::of_bundles(&files);
    // A time the map does not have falls back to one it has.
    let time = if map.times.is_empty() || map.times.contains(&time) {
        time
    } else {
        map.first_time()
    };
    let walls = (!map.walls.is_empty()).then_some(map.walls.as_str());
    let (mut scene, _) = rk_map::build::build(&game, map.scene_bundle(), time.time_of_day(), compressed, walls)?;
    // CharaStudio leaves the map's fog off unless its fog switch is on.
    scene.light.fog = None;
    Ok(scene)
}

/// Makes a map file an older Renkatsu wrote again, when the game (or the
/// mod, in the game or the project's mods) still has the map, so that it
/// draws as this build draws it. Without them the file is kept and used
/// as it is.
fn remake(file: &Path) {
    let Ok(meta) = rk_map::file::meta(file) else { return };
    let Some(scene) = meta.bundles.first() else { return };
    // The project is the folder that holds its maps folder.
    let mods: Vec<PathBuf> = file
        .ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == MAPS))
        .and_then(Path::parent)
        .map(|root| vec![root.join(crate::project::MODS)])
        .unwrap_or_default();
    let Ok(game) = Game::open_user(&mods) else { return };
    let want = rk_vfs::index::norm(&scene.path);
    let Some(entry) = rk_map::list::read(&game, false)
        .into_iter()
        .find(|e| rk_vfs::index::norm(&e.bundle) == want)
    else {
        return;
    };
    // Written beside it and moved over it only once whole.
    let _ = rk_map::file::convert(&game, &entry, &meta.name, None, file, &mut |_| Ok(()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MapFile {
        MapFile {
            format: FORMAT,
            name: "Rooftop".into(),
            listed_name: "屋上".into(),
            number: 22,
            level: "Rooftop".into(),
            source: "Koikatsu".into(),
            times: vec![MapTime::Day, MapTime::Evening, MapTime::Night],
            thumbnail: Some("thumbnail.png".into()),
            walls: "wall00_d\twall00\r\nten00_d\tten00\tlamp\r\n".into(),
            bundles: vec![Bundle {
                path: "map/scene/rooftop.unity3d".into(),
                file: "rooftop.unity3d".into(),
            }],
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("renkatsu-maps-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_sidecar_is_readable_text_and_reads_back() {
        let dir = scratch("sidecar");
        let path = dir.join("Rooftop.rkmap");
        let map = sample();
        map.write(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("name = \"Rooftop\""), "{text}");
        assert!(text.contains("times = [\"day\", \"evening\", \"night\"]"), "{text}");
        assert!(text.contains("file = \"rooftop.unity3d\""), "{text}");
        assert_eq!(MapFile::read(&path).unwrap(), map);
        // The table of walls keeps its tabs and line ends.
        assert!(MapFile::read(&path).unwrap().walls.contains("ten00\tlamp\r\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_without_walls_or_picture_reads_back() {
        let dir = scratch("plain");
        let path = dir.join("Beach.rkmap");
        let map = MapFile {
            thumbnail: None,
            walls: String::new(),
            times: vec![MapTime::Night],
            ..sample()
        };
        map.write(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("walls"), "{text}");
        let read = MapFile::read(&path).unwrap();
        assert_eq!(read, map);
        assert_eq!(read.first_time(), MapTime::Night);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_map_file_is_listed_by_its_description() {
        let home = scratch("mapfile");
        let project = Project::create(&home.join("P"), "P").unwrap();
        let maps = project.root.join(MAPS);
        std::fs::create_dir_all(&maps).unwrap();
        let path = maps.join("Rooftop.rkmap");
        let map = MapFile {
            format: FORMAT,
            thumbnail: None,
            bundles: vec![Bundle {
                path: "map/scene/rooftop.unity3d".into(),
                file: String::new(),
            }],
            ..sample()
        };
        let text = toml::to_string(&map).unwrap();
        assert!(!text.contains("file ="), "{text}");
        let mut w = rk_asset::Writer::create(&path, rk_map::file::KIND, rk_map::file::VERSION).unwrap();
        w.add("meta", text.as_bytes(), rk_asset::Pack::Raw).unwrap();
        w.finish().unwrap();
        assert_eq!(MapFile::read(&path).unwrap(), map);
        let listed = in_project(&project.root);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].file, "Maps/Rooftop.rkmap");
        assert_eq!(
            owned(&project.root, "map/scene/Rooftop.unity3d").map(|m| m.file),
            Some("Maps/Rooftop.rkmap".to_owned())
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_project_lists_its_maps_by_sidecar() {
        let home = scratch("listed");
        let project = Project::create(&home.join("P"), "P").unwrap();
        let folder = project.root.join(MAPS).join("School").join("Rooftop");
        std::fs::create_dir_all(&folder).unwrap();
        sample().write(&folder.join("Rooftop.rkmap")).unwrap();
        // Something else in the folder is not a map.
        std::fs::write(folder.join("notes.txt"), "x").unwrap();
        let maps = in_project(&project.root);
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].file, "Maps/School/Rooftop/Rooftop.rkmap");
        assert_eq!(
            owned(&project.root, "Map/Scene/Rooftop.unity3d").map(|m| m.file),
            Some("Maps/School/Rooftop/Rooftop.rkmap".to_owned())
        );
        assert!(owned(&project.root, "map/scene/myroom.unity3d").is_none());
        let _ = std::fs::remove_dir_all(&home);
    }
}
