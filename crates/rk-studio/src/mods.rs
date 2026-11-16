// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The zipmods a card needs, and bringing them into a project. A project
//! carries its cards' mods so that it still opens where the game has other
//! mods, or none of them.
//!
//! What a card needs is worked out as `rk resolve` does: every item the
//! card names, in every outfit, is looked up in the item database, and the
//! mods the found items come from are the ones needed. To these come the
//! mods the card's Sideloader records name by GUID, the mods that bring
//! the shaders its MaterialEditor data switches to, and the mod of the body
//! its Uncensor Selector data asks for.

use anyhow::{Context, Result};
use rk_card::{map_get, Card, Value};
use rk_vfs::index::{mod_info, version_key, Migration, MigrationKind};
use rk_vfs::{resolve::card_items, Game, Lookup};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A zipmod on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModFile {
    pub guid: String,
    pub version: String,
    pub path: PathBuf,
    pub size: u64,
}

/// The mods a card needs: the installed ones, one per GUID, and the GUIDs
/// of the ones that are not installed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Needed {
    pub mods: Vec<ModFile>,
    pub missing: Vec<String>,
}

/// The installed mods a GUID named by a card leads to, following the
/// migrations other mods declare for it, and whether it leads nowhere.
/// A GUID that a mod tells Sideloader to strip stands for the game's own
/// items and needs nothing.
pub fn settle(guid: &str, installed: &dyn Fn(&str) -> bool, migrations: &[Migration]) -> (Vec<String>, bool) {
    let mut found = Vec::new();
    if installed(guid) {
        found.push(guid.to_owned());
    }
    let mut stripped = false;
    for m in migrations.iter().filter(|m| m.guid_old == guid) {
        match m.kind {
            MigrationKind::StripAll => stripped = true,
            _ if installed(&m.guid_new) && !found.contains(&m.guid_new) => found.push(m.guid_new.clone()),
            _ => {}
        }
    }
    let missing = found.is_empty() && !stripped;
    (found, missing)
}

/// The shaders a card's MaterialEditor data switches materials to.
fn shader_names(card: &Card) -> Vec<String> {
    let Some(plugin) = card.ext.get(rk_chara::material_editor::KEY) else {
        return Vec::new();
    };
    let Some(Value::Array(list)) = plugin.nested("MaterialShaderList") else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|e| map_get(e, "ShaderName").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

/// Works out the mods a card needs from an opened game.
pub fn needed(game: &Game, card: &Card) -> Needed {
    let index = &game.assets.index;
    let mut used: BTreeSet<u32> = BTreeSet::new();
    let mut missing: BTreeSet<String> = BTreeSet::new();

    // Every part of every outfit, the face, the body and the hair.
    let items = card_items(&game.items, card);
    for slot in &items.slots {
        match &slot.lookup {
            Lookup::Found(item) => used.extend(item.mod_idx),
            Lookup::MissingMod { guid, .. } => {
                missing.insert(guid.clone());
            }
            Lookup::Missing { .. } => {}
        }
    }
    // Every mod the Sideloader records name, also for what no slot above
    // covers, such as the accessories plugins add.
    let position = |guid: &str| index.mods.iter().position(|m| m.guid == guid);
    for info in items.infos.values().filter(|i| !i.guid.is_empty()) {
        let (found, lost) = settle(&info.guid, &|g| position(g).is_some(), &index.migrations);
        used.extend(found.iter().filter_map(|g| position(g)).map(|i| i as u32));
        if lost {
            missing.insert(info.guid.clone());
        }
    }
    // Shaders MaterialEditor takes from zipmods.
    for name in shader_names(card) {
        used.extend(index.me_shaders.get(&name).and_then(|s| s.mod_idx));
    }
    // The body Uncensor Selector was told to use.
    let uncensors = game.assets.uncensors();
    if let Some(guid) = rk_chara::uncensor_selector::stored_body(&card.ext, uncensors) {
        match uncensors.body(&guid) {
            Some(body) => used.extend(body.mod_idx),
            None => {
                missing.insert(guid);
            }
        }
    }

    let mods = used
        .into_iter()
        .map(|m| {
            let info = &index.mods[m as usize];
            // A mod of the game is listed relative to it, any other in full.
            let path = index.game_dir.join(&info.path);
            ModFile {
                guid: info.guid.clone(),
                version: info.version.clone(),
                size: path.metadata().map_or(0, |m| m.len()),
                path,
            }
        })
        .collect();
    Needed {
        mods,
        missing: missing.into_iter().collect(),
    }
}

/// The zipmods a folder holds, read from their manifests.
pub fn in_folder(dir: &Path) -> Vec<ModFile> {
    let mut files = Vec::new();
    for ext in ["zipmod", "zip"] {
        rk_core::walk_files(dir, ext, &mut files);
    }
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let info = mod_info(&path)?;
            Some(ModFile {
                guid: info.guid,
                version: info.version,
                size: path.metadata().map_or(0, |m| m.len()),
                path,
            })
        })
        .collect()
}

/// What an import does to a project's mods.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Mods to copy into the project.
    pub copy: Vec<ModFile>,
    /// Older versions in the project that the copies replace.
    pub replace: Vec<PathBuf>,
    /// How many of the needed mods the project already has.
    pub present: usize,
}

impl Plan {
    /// The size of everything that will be copied.
    pub fn bytes(&self) -> u64 {
        self.copy.iter().map(|m| m.size).sum()
    }
}

/// Decides which of the needed mods to copy into a project that holds
/// `have`: one file per GUID, as Sideloader loads them. A mod the project
/// has in the same or a newer version stays; an older one is replaced.
pub fn plan(needed: &[ModFile], have: &[ModFile]) -> Plan {
    let mut plan = Plan::default();
    let mut seen: Vec<&str> = Vec::new();
    for wanted in needed {
        // The same GUID twice in what is needed: the newer one counts.
        if seen.contains(&wanted.guid.as_str()) {
            continue;
        }
        seen.push(&wanted.guid);
        let wanted = needed
            .iter()
            .filter(|m| m.guid == wanted.guid)
            .max_by_key(|m| version_key(&m.version))
            .expect("the mod itself");
        let held: Vec<&ModFile> = have.iter().filter(|m| m.guid == wanted.guid).collect();
        let newest = held.iter().map(|m| version_key(&m.version)).max();
        if held.iter().any(|m| m.path == wanted.path) || newest.is_some_and(|v| v >= version_key(&wanted.version)) {
            plan.present += 1;
        } else {
            plan.copy.push(wanted.clone());
            plan.replace.extend(held.iter().map(|m| m.path.clone()));
        }
    }
    plan
}

/// Copies the planned mods into a project's mods folder and removes the
/// older versions they replace. A file name in use by another mod gets a
/// number.
pub fn copy(plan: &Plan, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    for old in &plan.replace {
        std::fs::remove_file(old).with_context(|| format!("removing {}", old.display()))?;
    }
    for m in &plan.copy {
        let stem = m.path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let ext = m.path.extension().unwrap_or_default().to_string_lossy().into_owned();
        let target = (1..)
            .map(|n| match n {
                1 => dir.join(format!("{stem}.{ext}")),
                n => dir.join(format!("{stem} {n}.{ext}")),
            })
            .find(|p| !p.exists())
            .expect("some number is free");
        std::fs::copy(&m.path, &target).with_context(|| format!("copying {}", m.path.display()))?;
    }
    Ok(())
}

/// A size in bytes as people read it.
pub fn size_text(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= KB * KB * KB {
        format!("{:.1} GB", b / (KB * KB * KB))
    } else if b >= KB * KB {
        format!("{:.1} MB", b / (KB * KB))
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} bytes")
    }
}

/// "1 mod" or "3 mods".
pub fn count_text(n: usize) -> String {
    if n == 1 {
        "1 mod".to_owned()
    } else {
        format!("{n} mods")
    }
}

/// The game's files, opened when they are first needed, with the mods of
/// one project searched together with the game's own.
#[derive(Default)]
pub struct Games {
    game: Option<Game>,
    mods: PathBuf,
}

impl Games {
    /// The game with the zipmods of `mods_dir`. Another folder than last
    /// time opens the game again; the index cache makes that quick.
    pub fn get(&mut self, mods_dir: &Path) -> Result<&Game> {
        if self.game.is_none() || self.mods != mods_dir {
            self.game = None;
            let config = rk_core::Config::load()?;
            self.game = Some(Game::open_with(&config, &[mods_dir.to_owned()])?);
            self.mods = mods_dir.to_owned();
        }
        Ok(self.game.as_ref().expect("opened above"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(guid: &str, version: &str, path: &str, size: u64) -> ModFile {
        ModFile {
            guid: guid.to_owned(),
            version: version.to_owned(),
            path: PathBuf::from(path),
            size,
        }
    }

    #[test]
    fn mods_are_copied_once_per_guid_and_the_newer_version_stays() {
        let needed = [
            file("hair", "1.2", "/game/mods/hair.zipmod", 10),
            file("top", "v2.0", "/game/mods/top v2.zipmod", 200),
            file("shoes", "1.0", "/game/mods/shoes.zipmod", 3000),
            file("ears", "1.10", "/game/mods/ears.zipmod", 40000),
        ];
        let have = [
            // The same version: nothing to do.
            file("hair", "1.2", "/project/mods/hair.zipmod", 10),
            // Older, in two files: both make way for the newer one.
            file("top", "v1.9", "/project/mods/top.zipmod", 150),
            file("top", "v1.0", "/project/mods/top old.zipmod", 100),
            // Newer than the game's: it stays.
            file("ears", "1.11", "/project/mods/ears.zipmod", 40001),
            file("unrelated", "1", "/project/mods/unrelated.zipmod", 1),
        ];
        let plan = plan(&needed, &have);
        let copied: Vec<&str> = plan.copy.iter().map(|m| m.guid.as_str()).collect();
        assert_eq!(copied, ["top", "shoes"]);
        assert_eq!(
            plan.replace,
            [
                PathBuf::from("/project/mods/top.zipmod"),
                PathBuf::from("/project/mods/top old.zipmod")
            ]
        );
        assert_eq!(plan.present, 2);
        assert_eq!(plan.bytes(), 3200);
        // Nothing is left to copy once the plan was carried out.
        let after = [have.to_vec(), plan.copy.clone()].concat();
        let after: Vec<ModFile> = after.into_iter().filter(|m| !plan.replace.contains(&m.path)).collect();
        assert!(super::plan(&needed, &after).copy.is_empty());
    }

    #[test]
    fn a_mod_needed_twice_is_copied_in_its_newer_version() {
        let needed = [
            file("hair", "1.0", "/game/mods/a.zipmod", 1),
            file("hair", "1.5", "/game/mods/b.zipmod", 2),
        ];
        let plan = plan(&needed, &[]);
        assert_eq!(plan.copy, [needed[1].clone()]);
        // A mod the project itself supplied is there already.
        let own = [file("hair", "1.5", "/project/mods/b.zipmod", 2)];
        let plan = super::plan(&own, &own);
        assert!(plan.copy.is_empty());
        assert_eq!(plan.present, 1);
    }

    #[test]
    fn copies_land_in_the_folder_and_replace_older_files() {
        let dir = std::env::temp_dir().join(format!("renkatsu-mods-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (game, project) = (dir.join("game"), dir.join("project/mods"));
        std::fs::create_dir_all(&game).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(game.join("top.zipmod"), b"new top").unwrap();
        std::fs::write(game.join("hair.zipmod"), b"hair").unwrap();
        std::fs::write(project.join("top old.zipmod"), b"old top").unwrap();
        // Another mod's file with the name of one that arrives.
        std::fs::write(project.join("hair.zipmod"), b"other").unwrap();
        let plan = Plan {
            copy: vec![
                file("top", "2", &game.join("top.zipmod").to_string_lossy(), 7),
                file("hair", "1", &game.join("hair.zipmod").to_string_lossy(), 4),
            ],
            replace: vec![project.join("top old.zipmod")],
            present: 0,
        };
        copy(&plan, &project).unwrap();
        assert_eq!(std::fs::read(project.join("top.zipmod")).unwrap(), b"new top");
        assert!(!project.join("top old.zipmod").exists());
        assert_eq!(std::fs::read(project.join("hair.zipmod")).unwrap(), b"other");
        assert_eq!(std::fs::read(project.join("hair 2.zipmod")).unwrap(), b"hair");
        // Files that are no zipmods are not taken for mods.
        assert!(in_folder(&project).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_guid_follows_migrations_to_an_installed_mod() {
        let migration = |kind, old: &str, new: &str| Migration {
            kind,
            guid_old: old.to_owned(),
            guid_new: new.to_owned(),
            category: 0,
            id_old: 0,
            id_new: 0,
        };
        let migrations = [
            migration(MigrationKind::MigrateAll, "old.hair", "new.hair"),
            migration(MigrationKind::Migrate, "old.top", "gone.top"),
            migration(MigrationKind::StripAll, "official.dlc", ""),
        ];
        let installed = |guid: &str| ["new.hair", "plain"].contains(&guid);
        assert_eq!(
            settle("plain", &installed, &migrations),
            (vec!["plain".to_owned()], false)
        );
        assert_eq!(
            settle("old.hair", &installed, &migrations),
            (vec!["new.hair".to_owned()], false)
        );
        // Moved to a mod that is not installed either: missing.
        assert_eq!(settle("old.top", &installed, &migrations), (vec![], true));
        // Stripped: the game's own items, nothing to bring.
        assert_eq!(settle("official.dlc", &installed, &migrations), (vec![], false));
        assert_eq!(settle("unknown", &installed, &migrations), (vec![], true));
    }

    #[test]
    fn sizes_and_counts_read_well() {
        assert_eq!(size_text(512), "512 bytes");
        assert_eq!(size_text(2048), "2 KB");
        assert_eq!(size_text(5 * 1024 * 1024 + 512 * 1024), "5.5 MB");
        assert_eq!(size_text(3 * 1024 * 1024 * 1024), "3.0 GB");
        assert_eq!(count_text(1), "1 mod");
        assert_eq!(count_text(12), "12 mods");
    }
}
