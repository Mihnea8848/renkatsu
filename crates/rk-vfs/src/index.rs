// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Scan of the game's `abdata` folder and every zipmod, cached on disk.
//!
//! The index records where each bundle lives (a loose file or an entry in a
//! zipmod), which internal file names (CABs) the game bundles contain so
//! cross-bundle references can be followed, and every item list.

use anyhow::{Context, Result};
use rayon::prelude::*;
use rk_core::Config;
use rk_unityfs::{class, Bundle, SerializedFile};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const CACHE_VERSION: u32 = 5;
const GAME_NAMES: [&str; 3] = ["koikatsu", "koikatu", "コイカツ"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModInfo {
    /// Path of the zipmod relative to the game directory.
    pub path: String,
    pub guid: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub website: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationKind {
    /// One item moved to another mod and ID.
    Migrate,
    /// Every item of the old mod now lives under the new GUID.
    MigrateAll,
    /// Forget the old GUID and treat the IDs as game items.
    StripAll,
}

/// A manifest record redirecting items that used to belong to another mod.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Migration {
    pub kind: MigrationKind,
    pub guid_old: String,
    pub guid_new: String,
    pub category: i32,
    pub id_old: i32,
    pub id_new: i32,
}

/// An item list: one category of selectable things (tops, hair, pupils...).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListData {
    /// Index into [`Index::mods`]; `None` for the game's own lists.
    pub mod_idx: Option<u32>,
    pub category: i32,
    pub distribution: i32,
    pub keys: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleLoc {
    /// `None`: `path` is relative to `abdata`. `Some`: entry name inside that zipmod.
    pub mod_idx: Option<u32>,
    pub path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct FileStamp {
    size: u64,
    mtime: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ZipScan {
    stamp: FileStamp,
    /// `None` when the archive is unreadable or not meant for this game.
    info: Option<ModInfo>,
    skipped: Option<String>,
    bundles: Vec<(String, String)>,
    lists: Vec<ListData>,
    migrations: Vec<Migration>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct GameScan {
    stamp: FileStamp,
    cabs: Vec<String>,
    lists: Vec<ListData>,
}

#[derive(Default, Serialize, Deserialize)]
struct Cache {
    version: u32,
    game_dir: String,
    game: HashMap<String, GameScan>,
    zips: HashMap<String, ZipScan>,
}

#[derive(Debug, Default)]
pub struct Index {
    pub game_dir: PathBuf,
    pub mods: Vec<ModInfo>,
    /// Virtual path under `abdata` (lowercase, forward slashes) to its
    /// sources, zipmods first.
    pub bundles: HashMap<String, Vec<BundleLoc>>,
    /// Lowercase CAB name to the game bundle's virtual path.
    pub cabs: HashMap<String, String>,
    pub lists: Vec<ListData>,
    pub migrations: Vec<Migration>,
    pub skipped: Vec<(String, String)>,
}

fn stamp(p: &Path) -> FileStamp {
    let Ok(md) = p.metadata() else {
        return FileStamp::default();
    };
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    FileStamp { size: md.len(), mtime }
}

fn same(a: &FileStamp, b: &FileStamp) -> bool {
    a.size == b.size && a.mtime == b.mtime
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

pub fn norm(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_lowercase()
}

/// `ChaListData` as the game stores it in list bundles (MessagePack).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChaListData {
    #[serde(default)]
    category_no: i32,
    #[serde(default)]
    distribution_no: i32,
    #[serde(default)]
    lst_key: Vec<String>,
    #[serde(default)]
    dict_list: HashMap<i32, Vec<String>>,
}

fn scan_game_bundle(path: &Path, is_list: bool) -> Result<GameScan> {
    let b = Bundle::open(path)?;
    let mut scan = GameScan {
        stamp: stamp(path),
        ..Default::default()
    };
    for n in &b.nodes {
        if n.is_serialized() {
            scan.cabs.push(n.path.to_lowercase());
        }
    }
    if is_list {
        for n in b.nodes.iter().filter(|n| n.is_serialized()) {
            let f = SerializedFile::parse(b.read_node(n)?)?;
            let mut texts: Vec<_> = f.objects_of(class::TEXT_ASSET).collect();
            texts.sort_by_key(|o| o.path_id);
            for o in texts {
                let v = f.read(o)?;
                let Some(bytes) = v.bytes("m_Script") else { continue };
                let Ok(d) = rmp_serde::from_slice::<ChaListData>(bytes) else {
                    continue;
                };
                let mut rows: Vec<(i32, Vec<String>)> = d.dict_list.into_iter().collect();
                rows.sort_by_key(|(k, _)| *k);
                scan.lists.push(ListData {
                    mod_idx: None,
                    category: d.category_no,
                    distribution: d.distribution_no,
                    keys: d.lst_key,
                    rows: rows.into_iter().map(|(_, r)| r).collect(),
                });
            }
        }
    }
    Ok(scan)
}

/// Sideloader's CSV list layout: category, distribution, file path, keys, rows.
fn parse_csv_list(text: &str) -> Option<ListData> {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines();
    let first = |l: &str| l.split(',').next().unwrap_or("").trim().to_string();
    let category = first(lines.next()?).parse().ok()?;
    let distribution = first(lines.next()?).parse().unwrap_or(0);
    let _file_path = lines.next()?;
    let keys: Vec<String> = lines.next()?.trim().split(',').map(|s| s.trim().to_string()).collect();
    let mut rows = Vec::new();
    for l in lines {
        let l = l.trim();
        if !l.contains(',') {
            break;
        }
        rows.push(l.split(',').map(|s| s.to_string()).collect());
    }
    Some(ListData {
        mod_idx: None,
        category,
        distribution,
        keys,
        rows,
    })
}

fn scan_zipmod(path: &Path) -> ZipScan {
    let mut scan = ZipScan {
        stamp: stamp(path),
        ..Default::default()
    };
    let run = |scan: &mut ZipScan| -> Result<()> {
        let mut zip = zip::ZipArchive::new(std::io::BufReader::new(std::fs::File::open(path)?))?;
        let mut manifest = String::new();
        zip.by_name("manifest.xml")
            .context("no manifest.xml")?
            .read_to_string(&mut manifest)?;
        let doc = roxmltree::Document::parse(manifest.trim_start_matches('\u{feff}')).context("manifest.xml")?;
        let root = doc.root_element();
        let field = |name: &str| {
            root.children()
                .find(|c| c.has_tag_name(name))
                .and_then(|c| c.text())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let games: Vec<String> = root
            .children()
            .filter(|c| c.has_tag_name("game"))
            .filter_map(|c| c.text())
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        if !games.is_empty() && !games.iter().any(|g| GAME_NAMES.contains(&g.as_str())) {
            scan.skipped = Some(format!("made for {}", games.join(", ")));
            return Ok(());
        }
        let guid = field("guid");
        anyhow::ensure!(!guid.is_empty(), "manifest has no guid");
        for info in root
            .children()
            .filter(|c| c.has_tag_name("migrationInfo"))
            .flat_map(|m| m.children())
        {
            if !info.has_tag_name("info") {
                continue;
            }
            let attr = |n: &str| info.attribute(n).map(str::trim).filter(|v| !v.is_empty());
            let kind = match attr("migrationType") {
                Some("MigrateAll") => MigrationKind::MigrateAll,
                Some("StripAll") => MigrationKind::StripAll,
                _ => MigrationKind::Migrate,
            };
            let Some(guid_old) = attr("guidOld") else { continue };
            let mut m = Migration {
                kind,
                guid_old: guid_old.to_string(),
                guid_new: attr("guidNew").unwrap_or(&guid).to_string(),
                category: 0,
                id_old: 0,
                id_new: 0,
            };
            if kind == MigrationKind::Migrate {
                let (Some(cat), Some(old), Some(new)) = (
                    attr("category").and_then(crate::cat::from_name),
                    attr("idOld").and_then(|v| v.parse().ok()),
                    attr("idNew").and_then(|v| v.parse().ok()),
                ) else {
                    continue;
                };
                (m.category, m.id_old, m.id_new) = (cat, old, new);
            }
            scan.migrations.push(m);
        }
        scan.info = Some(ModInfo {
            path: String::new(),
            guid,
            name: field("name"),
            version: field("version"),
            author: field("author"),
            website: field("website"),
        });
        let names: Vec<String> = zip
            .file_names()
            .filter_map(|n| n.ok())
            .map(|n| n.into_owned())
            .collect();
        for name in names {
            let lower = norm(&name);
            let Some(inner) = lower.strip_prefix("abdata/") else {
                continue;
            };
            if lower.ends_with(".unity3d") {
                scan.bundles.push((inner.to_string(), name.clone()));
            } else if inner.starts_with("list/characustom/") && lower.ends_with(".csv") {
                let mut text = String::new();
                if zip.by_name(&name)?.read_to_string(&mut text).is_ok() {
                    if let Some(l) = parse_csv_list(&text) {
                        scan.lists.push(l);
                    }
                }
            }
        }
        Ok(())
    };
    if let Err(e) = run(&mut scan) {
        scan.info = None;
        scan.skipped = Some(format!("{e:#}"));
    }
    scan
}

/// Orders manifest versions the loose way mods use them (`v1.3.3`, `1.10`).
fn version_key(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect()
}

fn cache_path(game_dir: &Path) -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    // Stable, dependency-free hash of the game path.
    let mut h = 0xcbf29ce484222325u64;
    for b in game_dir.to_string_lossy().bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    base.join("renkatsu").join(format!("index-{h:016x}.bin"))
}

impl Index {
    /// Loads the cached index, rescanning only files that changed.
    pub fn load(cfg: &Config) -> Result<Self> {
        let cache_file = cache_path(&cfg.game_dir);
        let mut cache: Cache = std::fs::read(&cache_file)
            .ok()
            .and_then(|b| rmp_serde::from_slice(&b).ok())
            .filter(|c: &Cache| c.version == CACHE_VERSION && c.game_dir == cfg.game_dir.to_string_lossy())
            .unwrap_or_default();
        cache.version = CACHE_VERSION;
        cache.game_dir = cfg.game_dir.to_string_lossy().into_owned();
        let mut dirty = false;

        // Game bundles.
        let abdata = cfg.abdata();
        let mut files = Vec::new();
        rk_core::walk_files(&abdata, "unity3d", &mut files);
        let todo: Vec<&PathBuf> = files
            .iter()
            .filter(|p| {
                !cache
                    .game
                    .get(&rel(&abdata, p))
                    .is_some_and(|s| same(&s.stamp, &stamp(p)))
            })
            .collect();
        let scanned: Vec<(String, GameScan)> = todo
            .par_iter()
            .map(|p| {
                let key = rel(&abdata, p);
                let is_list = norm(&key).starts_with("list/characustom/");
                let scan = scan_game_bundle(p, is_list).unwrap_or_else(|_| GameScan {
                    stamp: stamp(p),
                    ..Default::default()
                });
                (key, scan)
            })
            .collect();
        dirty |= !scanned.is_empty();
        cache.game.extend(scanned);
        let live: std::collections::HashSet<String> = files.iter().map(|p| rel(&abdata, p)).collect();
        let before = cache.game.len();
        cache.game.retain(|k, _| live.contains(k));
        dirty |= before != cache.game.len();

        // Zipmods.
        let mut zips = Vec::new();
        for ext in ["zipmod", "zip"] {
            rk_core::walk_files(&cfg.mods(), ext, &mut zips);
        }
        let todo: Vec<&PathBuf> = zips
            .iter()
            .filter(|p| {
                !cache
                    .zips
                    .get(&rel(&cfg.game_dir, p))
                    .is_some_and(|s| same(&s.stamp, &stamp(p)))
            })
            .collect();
        let scanned: Vec<(String, ZipScan)> = todo
            .par_iter()
            .map(|p| (rel(&cfg.game_dir, p), scan_zipmod(p)))
            .collect();
        dirty |= !scanned.is_empty();
        cache.zips.extend(scanned);
        let live: std::collections::HashSet<String> = zips.iter().map(|p| rel(&cfg.game_dir, p)).collect();
        let before = cache.zips.len();
        cache.zips.retain(|k, _| live.contains(k));
        dirty |= before != cache.zips.len();

        if dirty {
            if let Some(dir) = cache_file.parent() {
                std::fs::create_dir_all(dir).ok();
            }
            std::fs::write(&cache_file, rmp_serde::to_vec(&cache)?).ok();
        }
        Ok(Self::build(cfg, cache))
    }

    fn build(cfg: &Config, cache: Cache) -> Self {
        let mut idx = Index {
            game_dir: cfg.game_dir.clone(),
            ..Default::default()
        };

        // Game files, in path order so "first list wins" matches the game.
        let mut game: Vec<_> = cache.game.into_iter().collect();
        game.sort_by_key(|g| norm(&g.0));
        for (path, scan) in game {
            let vpath = norm(&path);
            for cab in scan.cabs {
                idx.cabs.entry(cab).or_insert_with(|| vpath.clone());
            }
            idx.bundles
                .entry(vpath)
                .or_default()
                .push(BundleLoc { mod_idx: None, path });
            idx.lists.extend(scan.lists);
        }

        // Zipmods: one per GUID, the highest version wins.
        let mut zips: Vec<_> = cache.zips.into_iter().collect();
        zips.sort_by(|a, b| a.0.cmp(&b.0));
        let mut best: HashMap<String, usize> = HashMap::new();
        for (i, (path, scan)) in zips.iter().enumerate() {
            match &scan.info {
                Some(info) => {
                    let slot = best.entry(info.guid.clone()).or_insert(i);
                    let cur = zips[*slot].1.info.as_ref().unwrap();
                    if *slot != i && version_key(&info.version) >= version_key(&cur.version) {
                        idx.skipped
                            .push((zips[*slot].0.clone(), format!("superseded by {path}")));
                        *slot = i;
                    } else if *slot != i {
                        idx.skipped
                            .push((path.clone(), format!("superseded by {}", zips[*slot].0)));
                    }
                }
                None => idx
                    .skipped
                    .push((path.clone(), scan.skipped.clone().unwrap_or_default())),
            }
        }
        let keep: std::collections::HashSet<usize> = best.values().copied().collect();
        for (i, (path, scan)) in zips.into_iter().enumerate() {
            if !keep.contains(&i) {
                continue;
            }
            let mod_idx = idx.mods.len() as u32;
            let mut info = scan.info.unwrap();
            info.path = path;
            idx.mods.push(info);
            for (vpath, entry) in scan.bundles {
                idx.bundles.entry(vpath).or_default().insert(
                    0,
                    BundleLoc {
                        mod_idx: Some(mod_idx),
                        path: entry,
                    },
                );
            }
            idx.migrations.extend(scan.migrations);
            for mut l in scan.lists {
                l.mod_idx = Some(mod_idx);
                idx.lists.push(l);
            }
        }
        idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_list_follows_sideloader_layout() {
        let text = "\u{feff}122\r\n0\r\nAssets/list.bytes\r\nID,Kind,Name,MainAB\r\n501,1,Hair A,chara/a.unity3d\r\n506,1,Hair B,chara/b.unity3d\r\n\r\nignored,after,blank\r\n";
        let l = parse_csv_list(text).unwrap();
        assert_eq!(l.category, 122);
        assert_eq!(l.distribution, 0);
        assert_eq!(l.keys, ["ID", "Kind", "Name", "MainAB"]);
        assert_eq!(l.rows.len(), 2);
        assert_eq!(l.rows[1][2], "Hair B");
    }

    #[test]
    fn csv_list_rejects_garbage() {
        assert!(parse_csv_list("not a number\n0\npath\nID\n").is_none());
        assert!(parse_csv_list("").is_none());
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(version_key("v1.10") > version_key("v1.9"));
        assert!(version_key("2") > version_key("1.6.5"));
        assert_eq!(version_key("rev21.2"), vec![21, 2]);
        assert!(version_key("") < version_key("0.1"));
    }

    #[test]
    fn paths_are_normalised() {
        assert_eq!(norm("Chara\\Mod\\A.unity3d"), "chara/mod/a.unity3d");
        assert_eq!(norm("/abdata/x"), "abdata/x");
    }
}
