// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The maps a project owns. Importing a map copies its scene bundle out of
//! the game, with any other bundle it reads from, into a folder of its own
//! under `Maps`, together with the game's picture of it and a small text
//! file beside them, the map's sidecar. The sidecar says what the map is
//! and holds what would otherwise be read from the game, such as the table
//! of walls that hide, so the project draws its maps without Koikatsu.
//!
//! ```text
//! Maps/
//!   Rooftop/
//!     Rooftop.rkmap     the sidecar: names, number, times of day, walls
//!     rooftop.unity3d   the scene bundle as the game has it
//!     thumbnail.png     the game's picture of the map
//! ```
//!
//! Scenes name a map by its sidecar, relative to the project folder.

use crate::project::{file_name, files_below, relative_to, Project};
use crate::scene::MapTime;
use anyhow::{bail, Context, Result};
use rk_vfs::Game;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// The project folder maps go in.
pub const MAPS: &str = "Maps";
/// The extension of a map's sidecar.
pub const EXTENSION: &str = "rkmap";
/// The version of the sidecar this build writes.
pub const FORMAT: u32 = 1;
/// The picture of a map, beside its sidecar.
const THUMBNAIL: &str = "thumbnail.png";

/// A bundle of a map, as copied into the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    /// Where the game keeps it, under `abdata`, which is where the map
    /// looks for it.
    pub path: String,
    /// The copy, beside the sidecar.
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
    pub fn read(path: &Path) -> Result<MapFile> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
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

/// A map of the game on its way into the project: what is copied.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub entry: rk_map::list::Entry,
    /// The name it will show under.
    pub name: String,
    /// The scene bundle, then the bundles it reads from, with their sizes.
    pub bundles: Vec<(String, u64)>,
}

impl Plan {
    /// The size of everything that will be copied.
    pub fn bytes(&self) -> u64 {
        self.bundles.iter().map(|b| b.1).sum()
    }
}

/// Works out what importing a map of the game copies.
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

/// A file name for a bundle's copy: the bundle's own file name, made
/// different from the ones taken.
fn copy_name(path: &str, taken: &[Bundle]) -> String {
    let base = file_name(path.rsplit(['/', '\\']).next().unwrap_or(path));
    let (stem, ext) = base.rsplit_once('.').map_or((base.as_str(), ""), |(s, e)| (s, e));
    let mut name = base.clone();
    let mut n = 1;
    while name == THUMBNAIL || taken.iter().any(|b| b.file.eq_ignore_ascii_case(&name)) {
        n += 1;
        name = if ext.is_empty() {
            format!("{stem} {n}")
        } else {
            format!("{stem} {n}.{ext}")
        };
    }
    name
}

/// Copies a map of the game into the project: its bundles, its picture
/// and its sidecar, in a folder of its own named after it. `counted` hears
/// of every piece copied, in bytes, and stops the copy with an error. A
/// map the project has already is not copied again. Returns the sidecar,
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
    let base = file_name(&plan.name);
    let mut folder = maps.join(&base);
    let mut n = 1;
    while folder.exists() {
        n += 1;
        folder = maps.join(format!("{base} {n}"));
    }
    std::fs::create_dir_all(&folder).with_context(|| format!("creating {}", folder.display()))?;
    let copied = copy_in(game, plan, &folder, counted);
    let sidecar = match copied {
        Ok(sidecar) => sidecar,
        Err(e) => {
            // Half a map is no map.
            let _ = std::fs::remove_dir_all(&folder);
            return Err(e);
        }
    };
    relative_to(&project.root, &sidecar).context("the map is outside the project")
}

/// Copies what `import` copies into `folder` and writes the sidecar there.
fn copy_in(game: &Game, plan: &Plan, folder: &Path, counted: &mut dyn FnMut(u64) -> Result<()>) -> Result<PathBuf> {
    let entry = &plan.entry;
    let mut bundles: Vec<Bundle> = Vec::new();
    for (path, _) in &plan.bundles {
        let file = copy_name(path, &bundles);
        let (mut from, _) = game.assets.bundle_file(path)?;
        let target = folder.join(&file);
        let mut to = std::fs::File::create(&target).with_context(|| format!("writing {}", target.display()))?;
        let mut buffer = vec![0u8; 1 << 20];
        loop {
            let n = from.read(&mut buffer).with_context(|| format!("reading {path}"))?;
            if n == 0 {
                break;
            }
            to.write_all(&buffer[..n])
                .with_context(|| format!("writing {}", target.display()))?;
            counted(n as u64)?;
        }
        bundles.push(Bundle {
            path: rk_vfs::index::norm(path),
            file,
        });
    }
    let times: Vec<MapTime> = rk_map::scene::times(game, &entry.bundle)?
        .into_iter()
        .map(MapTime::of)
        .collect();
    let first = if times.contains(&MapTime::Day) {
        MapTime::Day
    } else {
        times.first().copied().unwrap_or_default()
    };
    let thumbnail = rk_map::thumbnail::picture(game, &entry.bundle, first.time_of_day())
        .and_then(|picture| write_png(&folder.join(THUMBNAIL), &picture).ok())
        .map(|()| THUMBNAIL.to_owned());
    let walls = rk_map::vanish::table(game, &entry.vanish_bundle, &entry.vanish_file).unwrap_or_default();
    let map = MapFile {
        format: FORMAT,
        name: plan.name.clone(),
        listed_name: entry.name.trim().to_owned(),
        number: entry.number,
        level: entry.level.clone(),
        source: entry.source.label().to_owned(),
        times,
        thumbnail,
        walls,
        bundles,
    };
    let sidecar = folder.join(format!(
        "{}.{EXTENSION}",
        folder.file_name().unwrap_or_default().to_string_lossy()
    ));
    map.write(&sidecar)?;
    Ok(sidecar)
}

fn write_png(path: &Path, picture: &rk_map::thumbnail::Picture) -> Result<()> {
    let file = std::fs::File::create(path).with_context(|| format!("writing {}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), picture.width, picture.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&picture.rgba)?;
    Ok(())
}

/// Builds a map of the project from its own copy of the bundles, at a
/// time of day, ready to upload. With `compressed`, the textures stay as
/// the game stores them, for a GPU that draws them so.
pub fn build(sidecar: &Path, time: MapTime, compressed: bool) -> Result<rk_render::map::MapScene> {
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
            thumbnail: Some(THUMBNAIL.into()),
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

    #[test]
    fn copies_get_names_of_their_own() {
        let taken = vec![Bundle {
            path: "a/rooftop.unity3d".into(),
            file: "rooftop.unity3d".into(),
        }];
        assert_eq!(copy_name("b/rooftop.unity3d", &taken), "rooftop 2.unity3d");
        assert_eq!(copy_name("map/scene/park.unity3d", &taken), "park.unity3d");
    }
}
