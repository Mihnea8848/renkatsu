// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Building Starter Content from a Koikatsu install, and the package
//! commands of `rk`.
//!
//! What is official is decided from the game's own data. Unity writes a
//! list of the bundles of every build: the files without an extension in
//! `abdata` (`abdata` for the base game, `add01` and up for its updates
//! and add-ons, `studio00` and up for Studio). A bundle is official when
//! one of these lists names it. What a repack or a mod puts into `abdata`
//! afterwards is in no list, and zipmods never are in `abdata` at all, so
//! neither gets into a package. The same lists name the bundles each
//! bundle depends on, which is how a package gets the shaders and
//! textures its parts use.

use anyhow::{bail, ensure, Context, Result};
use rk_packages::manifest::{folder, Manifest, FORMAT};
use rk_packages::pack::{self, Pack};
use rk_packages::{size_text, Known, Part, Store, Task};
use rk_unityfs::{class, Bundle, SerializedFile, UValue};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The version of the Starter Content packages this build makes.
const STARTER_VERSION: &str = "1.1.0";
/// The version of Starter Content: Poses this build makes.
const POSES_VERSION: &str = "1.0.0";
/// The release the packages are uploaded to, apart from the app's own.
const RELEASE: &str = "https://github.com/Mihnea8848/renkatsu/releases/download/starter-content-1";
/// GitHub takes release files under 2 GB; parts stay well below.
const PART_SIZE: u64 = 1900 * 1024 * 1024;

/// One of the game's lists of bundles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BundleList {
    /// The list's file name in `abdata`: `abdata`, `add01`, `studio00`.
    pub name: String,
    /// The bundles it names, as paths under `abdata`.
    pub bundles: Vec<String>,
    /// For each bundle, the bundles it needs, as indices into `bundles`.
    pub needs: Vec<Vec<usize>>,
}

/// Whether a file in `abdata` has the name of one of the game's bundle
/// lists: `abdata`, `abdata_01`, `add01`, `studio00` and so on.
pub fn is_list_name(name: &str) -> bool {
    let numbered = |rest: &str| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
    name == "abdata"
        || name.strip_prefix("abdata_").is_some_and(numbered)
        || name.strip_prefix("add").is_some_and(numbered)
        || name.strip_prefix("studio").is_some_and(numbered)
}

fn norm(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_lowercase()
}

/// The official bundles: every path a list names, lower case, with the
/// list that names it first.
pub fn official(lists: &[BundleList]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for list in lists {
        for bundle in &list.bundles {
            out.entry(norm(bundle)).or_insert_with(|| list.name.clone());
        }
    }
    out
}

/// Content that never goes into a package, whatever asks for it: the H
/// scenes, the voices and sounds, and the accessories for the crotch.
pub fn never(path: &str) -> bool {
    path.starts_with("h/")
        || path.starts_with("sound/")
        || path.starts_with("chara/ao_kokan")
        || path.starts_with("chara/thumb/ao_kokan")
}

/// What Starter Content: Characters & Clothing is made of: every part a character
/// wears or is made of, the lists that name the parts, the shape tables,
/// and CharaStudio's colour filters (the sheets in `studio/info` and the
/// tables in `studio/filter`), without which the CharaStudio look has no
/// colour filter.
pub fn character_part(path: &str) -> bool {
    path.starts_with("chara/")
        || path.starts_with("list/characustom/")
        || path.starts_with("list/shapecorrect/")
        || path == "list/customshape.unity3d"
        || path.starts_with("studio/info/")
        || path.starts_with("studio/filter/")
}

/// What Starter Content: Poses is made of: CharaStudio's animations (the
/// bundles of `studio/anime`), the sheets in `studio/info` that list them
/// and the maker's poses. The H scenes' animations live under `h/`, which no package takes;
/// the sheets that list those for Studio are in the same files as the
/// others and are never read.
pub fn pose_part(path: &str) -> bool {
    path.starts_with("studio/anime/") || path.starts_with("studio/info/") || path == MAKER_POSES
}

/// The poses of the character maker. CharaStudio's own lists do not name
/// them, but lists that mods and packages bring play them under names of
/// their own.
const MAKER_POSES: &str = "custom/custompose.unity3d";

/// The bundles that hold the game's own characters as cards.
pub fn holds_cards(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    (path.starts_with("custom/") && file.starts_with("presets_"))
        || path.starts_with("action/fixchara/")
        || path.starts_with("action/mobchara/")
}

/// The bundles chosen for a package, and what was asked for and refused.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// Paths under `abdata`, lower case.
    pub take: BTreeSet<String>,
    /// Bundles a taken bundle needs that never go into a package, with
    /// the bundle that needs them.
    pub refused: BTreeSet<(String, String)>,
}

/// Chooses the official bundles `want` asks for and, through the lists,
/// every bundle these need.
pub fn select(lists: &[BundleList], want: &dyn Fn(&str) -> bool) -> Selection {
    let mut out = Selection::default();
    for list in lists {
        let names: Vec<String> = list.bundles.iter().map(|b| norm(b)).collect();
        let mut todo: Vec<usize> = (0..names.len())
            .filter(|&i| want(&names[i]) && !never(&names[i]))
            .collect();
        let mut seen: BTreeSet<usize> = todo.iter().copied().collect();
        while let Some(i) = todo.pop() {
            out.take.insert(names[i].clone());
            for &need in list.needs.get(i).into_iter().flatten() {
                let Some(name) = names.get(need) else { continue };
                if never(name) {
                    out.refused.insert((name.clone(), names[i].clone()));
                } else if seen.insert(need) {
                    todo.push(need);
                }
            }
        }
    }
    out
}

/// Reads one of the game's bundle lists.
fn read_list(path: &Path) -> Result<BundleList> {
    let bundle = Bundle::open(path)?;
    let mut list = BundleList {
        name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        ..Default::default()
    };
    for node in bundle.nodes.iter().filter(|n| n.is_serialized()) {
        let file = SerializedFile::parse(bundle.read_node(node)?)?;
        for object in file.objects_of(class::ASSET_BUNDLE_MANIFEST) {
            let value = file.read(object)?;
            let mut names: BTreeMap<i64, String> = BTreeMap::new();
            for entry in value.array("AssetBundleNames").unwrap_or_default() {
                if let (Some(i), Some(name)) = (
                    entry.get("first").and_then(UValue::as_i64),
                    entry.get("second").and_then(UValue::as_str),
                ) {
                    names.insert(i, name.to_owned());
                }
            }
            let at: BTreeMap<i64, usize> = names.keys().enumerate().map(|(n, i)| (*i, n)).collect();
            let mut needs = vec![Vec::new(); names.len()];
            for entry in value.array("AssetBundleInfos").unwrap_or_default() {
                let (Some(i), Some(info)) = (entry.get("first").and_then(UValue::as_i64), entry.get("second")) else {
                    continue;
                };
                let Some(&slot) = at.get(&i) else { continue };
                needs[slot] = info
                    .get("AssetBundleDependencies")
                    .and_then(UValue::as_array)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(UValue::as_i64)
                    .filter_map(|d| at.get(&d).copied())
                    .collect();
            }
            list.bundles = names.into_values().collect();
            list.needs = needs;
        }
    }
    ensure!(!list.bundles.is_empty(), "{} names no bundles", path.display());
    Ok(list)
}

/// The game's bundle lists, in name order.
fn read_lists(abdata: &Path) -> Result<Vec<BundleList>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(abdata)
        .with_context(|| format!("reading {}", abdata.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(is_list_name))
        .collect();
    files.sort();
    let mut lists = Vec::new();
    for file in files {
        match read_list(&file) {
            Ok(list) => lists.push(list),
            Err(e) => eprintln!("left out {}: {e:#}", file.display()),
        }
    }
    ensure!(
        !lists.is_empty(),
        "{} has none of the game's bundle lists",
        abdata.display()
    );
    Ok(lists)
}

/// A card found in one of the game's bundles.
struct FoundCard {
    /// The name of the asset that holds it.
    asset: String,
    female: bool,
    bytes: Vec<u8>,
}

/// The cards a bundle holds as text assets: the ones that read as cards.
fn cards_in(path: &Path) -> Result<Vec<FoundCard>> {
    let bundle = Bundle::open(path)?;
    let mut cards = Vec::new();
    for node in bundle.nodes.iter().filter(|n| n.is_serialized()) {
        let file = SerializedFile::parse(bundle.read_node(node)?)?;
        for object in file.objects_of(class::TEXT_ASSET) {
            let value = file.read(object)?;
            let (Some(name), Some(bytes)) = (value.str("m_Name"), value.bytes("m_Script")) else {
                continue;
            };
            if let Ok(card) = rk_card::Card::parse(bytes) {
                cards.push(FoundCard {
                    asset: name.to_owned(),
                    female: card.sex() != 0,
                    bytes: bytes.to_vec(),
                });
            }
        }
    }
    cards.sort_by(|a, b| a.asset.cmp(&b.asset));
    Ok(cards)
}

/// A file name people can read for a card, from the bundle and the asset
/// it came from: `Default female.png`, `Story c-8.png`, `Extra tia a.png`.
fn card_file(bundle: &str, asset: &str) -> String {
    let group = match bundle {
        b if b.starts_with("action/fixchara/") => "Story ",
        b if b.starts_with("action/mobchara/") => "Extra ",
        _ => "",
    };
    // The maker's presets are named `ill_Default_Female`.
    let asset = asset.strip_prefix("ill_").unwrap_or(asset);
    let mut clean: String = asset
        .chars()
        .map(|c| match c {
            c if c.is_ascii_alphanumeric() || c == '-' => c.to_ascii_lowercase(),
            _ => ' ',
        })
        .collect();
    if group.is_empty() {
        if let Some(first) = clean.get_mut(..1) {
            first.make_ascii_uppercase();
        }
    }
    format!("{group}{clean}.png")
}

/// Loose files under `abdata`, by their lower case path.
fn on_disk(abdata: &Path) -> BTreeMap<String, PathBuf> {
    let mut files = Vec::new();
    rk_core::walk_files(abdata, "unity3d", &mut files);
    files
        .into_iter()
        .map(|p| {
            let rel = p.strip_prefix(abdata).unwrap_or(&p).to_string_lossy().into_owned();
            (norm(&rel), p)
        })
        .collect()
}

/// One of the Starter Content packages: what it is called and which of
/// the game's bundles it asks for.
struct Starter {
    id: &'static str,
    name: &'static str,
    version: &'static str,
    /// What the package holds, for its own manifest.
    description: &'static str,
    /// What having it installed does, added to the description in the
    /// list of packages to download.
    gives: &'static str,
    /// What the bundles are, in the lines printed while choosing them.
    part: &'static str,
    want: fn(&str) -> bool,
    /// The game's own characters come along as cards.
    cards: bool,
    /// The animations of CharaStudio's list are counted.
    poses: bool,
}

/// Builds Starter Content: Characters & Clothing from the configured Koikatsu
/// install into `out`, or only says what it would hold.
pub fn starter_characters(out: &Path, list_only: bool, part_size: Option<u64>) -> Result<()> {
    starter(
        &Starter {
            id: rk_packages::STARTER_CHARACTERS,
            name: "Starter Content: Characters & Clothing",
            version: STARTER_VERSION,
            description: "Koikatsu's own bodies, faces, hair, clothes and accessories, with its updates and add-ons, \
                          and its default characters.",
            gives: "Characters made of these are built without a Koikatsu install.",
            part: "a character part",
            want: character_part,
            cards: true,
            poses: false,
        },
        out,
        list_only,
        part_size,
    )
}

/// Builds Starter Content: Poses from the configured Koikatsu install into
/// `out`, or only says what it would hold.
pub fn starter_poses(out: &Path, list_only: bool, part_size: Option<u64>) -> Result<()> {
    starter(
        &Starter {
            id: rk_packages::STARTER_POSES,
            name: "Starter Content: Animations",
            version: POSES_VERSION,
            description: "CharaStudio's poses and animations for standing, sitting, talking, walking, dancing and \
                          fighting, without the H scenes' animations.",
            gives: "Characters are posed and animated without a Koikatsu install.",
            part: "an animation",
            want: pose_part,
            cards: false,
            poses: true,
        },
        out,
        list_only,
        part_size,
    )
}

/// The animations CharaStudio's sheets list in the chosen bundles, as the
/// editor will list them, and how many of the chosen animation bundles no
/// sheet names.
fn listed_poses(disk: &BTreeMap<String, PathBuf>, take: &BTreeSet<String>) -> Result<rk_chara::animations::Library> {
    let mut library = rk_chara::animations::Library::default();
    for path in take.iter().filter(|p| p.starts_with("studio/info/")) {
        let Some(file) = disk.get(path) else { continue };
        let bundle = Bundle::open(file)?;
        let mut sheets = Vec::new();
        for node in bundle.nodes.iter().filter(|n| n.is_serialized()) {
            let serialized = SerializedFile::parse(bundle.read_node(node)?)?;
            for object in serialized.objects_of(class::MONO_BEHAVIOUR) {
                let Some(name) = serialized.name(object) else { continue };
                if name.to_ascii_lowercase().starts_with("anime") {
                    sheets.push((name, rk_chara::animations::sheet_rows(&serialized.read(object)?)));
                }
            }
        }
        let stem = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".unity3d");
        library.add_bundle(Some(stem), &sheets, &rk_chara::animations::Source::Game, &|bundle| {
            take.contains(&norm(bundle))
        });
    }
    Ok(library)
}

fn starter(spec: &Starter, out: &Path, list_only: bool, part_size: Option<u64>) -> Result<()> {
    let wanted = spec.want;
    let cfg = rk_core::Config::load()?;
    let abdata = cfg.abdata();
    let lists = read_lists(&abdata)?;
    let official = official(&lists);
    let disk = on_disk(&abdata);
    println!(
        "{} bundle lists name {} official bundles; {} bundles are on disk",
        lists.len(),
        official.len(),
        disk.len()
    );
    let added: Vec<&String> = disk
        .keys()
        .filter(|p| wanted(p) && !official.contains_key(*p))
        .collect();
    for path in &added {
        println!("left out, in none of the game's lists: {path}");
    }

    let selection = select(&lists, &wanted);
    for (need, by) in &selection.refused {
        println!("left out, never packaged: {need} (asked for by {by})");
    }
    for path in official.keys().filter(|p| wanted(p) && never(p)) {
        println!("left out, never packaged: {path}");
    }
    for path in selection.take.iter().filter(|p| !wanted(p)) {
        println!("taken because {} needs it: {path}", spec.part);
    }
    let mut manifest = Manifest {
        format: FORMAT,
        id: spec.id.to_owned(),
        name: spec.name.to_owned(),
        version: spec.version.to_owned(),
        kind: "starter-content".to_owned(),
        description: spec.description.to_owned(),
        requires_renkatsu: rk_packages::APP_VERSION.to_owned(),
        source: "Koikatsu by Illusion: the official game data, without mods, voices or H content".to_owned(),
        default_card: None,
        contents: BTreeMap::new(),
    };
    let mut pack = Pack::new(manifest.clone());
    let mut by_list: BTreeMap<&str, (usize, u64)> = BTreeMap::new();
    let mut bytes = 0u64;
    for path in &selection.take {
        let Some(file) = disk.get(path) else {
            bail!("the game's lists name {path}, which this install lacks; the package would be incomplete");
        };
        let size = file.metadata()?.len();
        bytes += size;
        let entry = by_list.entry(official[path].as_str()).or_default();
        entry.0 += 1;
        entry.1 += size;
        pack.add_file(&format!("{}/{path}", folder::ABDATA), file)?;
    }
    for (list, (count, size)) in &by_list {
        println!("  {list:<10} {count:>4} bundles  {:>9}", size_text(*size));
    }

    // The game's own characters, as the cards they are stored as.
    let mut cards = 0u64;
    for path in official.keys().filter(|p| spec.cards && holds_cards(p) && !never(p)) {
        let Some(file) = disk.get(path) else { continue };
        for card in cards_in(file).with_context(|| format!("reading {path}"))? {
            let sex = if card.female { "female" } else { "male" };
            let name = format!("{}/{sex}/{}", folder::CARDS, card_file(path, &card.asset));
            if list_only {
                println!("  card {name}  ({} from {path})", card.asset);
            }
            // The default character of the maker is the one new projects
            // start with.
            if card.asset == "ill_Default_Female" {
                manifest.default_card = Some(name.clone());
            }
            bytes += card.bytes.len() as u64;
            pack.add_bytes(&name, card.bytes)?;
            cards += 1;
        }
    }
    manifest
        .contents
        .insert("bundles".to_owned(), selection.take.len() as u64);
    let mut holds = format!("{} bundles", selection.take.len());
    if spec.cards {
        manifest.contents.insert("cards".to_owned(), cards);
        holds += &format!(" and {cards} cards");
    }
    if spec.poses {
        let library = listed_poses(&disk, &selection.take)?;
        for category in library.categories() {
            if list_only {
                println!("  {:>3} {}", category.count, category.name);
            }
        }
        // Nothing the editor would list may come from outside the package.
        for pose in &library.entries {
            ensure!(
                selection.take.contains(&norm(&pose.bundle)) && !never(&norm(&pose.bundle)),
                "{} is listed from {}, which is not in the package",
                pose.name,
                pose.bundle
            );
        }
        manifest
            .contents
            .insert("poses".to_owned(), library.entries.len() as u64);
        holds += &format!(" with {} poses and animations", library.entries.len());
    }
    *pack.manifest_mut() = manifest.clone();
    println!("{}: {holds}, {} before compression", manifest.name, size_text(bytes));
    if list_only {
        return Ok(());
    }

    let file = out.join(format!(
        "{}-{}.{}",
        manifest.id,
        manifest.version,
        rk_packages::EXTENSION
    ));
    let bar = crate::ui::bar(bytes * 2);
    let built = pack.write(&file, &mut |done| bar.set_position(done))?;
    bar.finish_and_clear();
    let parts = pack::split(&file, part_size.unwrap_or(PART_SIZE))?;
    println!(
        "wrote {} files, {} ({} bytes), {} installed, SHA-256 {}",
        built.files,
        size_text(built.size),
        built.size,
        size_text(built.installed_size),
        built.sha256
    );
    let mut known = Known {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: format!("{} {}", manifest.description, spec.gives),
        installed_size: built.installed_size,
        parts: Vec::new(),
    };
    for part in &parts {
        let (sha256, size) = rk_packages::hash::sha256_file(part, None)?;
        let name = part.file_name().unwrap_or_default().to_string_lossy();
        println!("  {}  {}", part.display(), size_text(size));
        known.parts.push(Part {
            url: format!("{RELEASE}/{name}"),
            size,
            sha256,
        });
    }
    if parts.len() > 1 {
        println!("cut into {} parts to stay under GitHub's 2 GB a file", parts.len());
    }
    println!("\nUpload the files above to the release, and put this in crates/rk-packages/packages.toml:\n");
    println!("{}", rk_packages::index::to_text(&[known])?);
    Ok(())
}

/// Installs a package from its archive into the user's packages.
pub fn install(archive: &Path) -> Result<()> {
    let store = Store::user()?;
    let installed = store.install(archive, &Task::new())?;
    let holds = installed.manifest.holds();
    println!(
        "installed {} {} in {} ({}{})",
        installed.manifest.name,
        installed.manifest.version,
        installed.dir.display(),
        size_text(installed.size()),
        if holds.is_empty() {
            String::new()
        } else {
            format!(": {holds}")
        }
    );
    Ok(())
}

/// Downloads a known package and installs it.
pub fn download(id: &str) -> Result<()> {
    let known = rk_packages::known()
        .into_iter()
        .find(|k| k.id == id)
        .with_context(|| format!("{id} is not a package Renkatsu knows where to download"))?;
    println!(
        "downloading {} {}, {}",
        known.name,
        known.version,
        size_text(known.size())
    );
    let installed = rk_packages::download::fetch(&known, &Store::user()?, &Task::new())?;
    println!("installed in {}", installed.dir.display());
    Ok(())
}

/// Lists the installed packages and the ones that can be downloaded.
pub fn list() -> Result<()> {
    let store = Store::user()?;
    println!("packages in {}", store.dir.display());
    let installed = store.installed();
    for package in &installed {
        println!(
            "  {:<22} {:<8} {:>9}  {} ({})",
            package.manifest.id,
            package.manifest.version,
            size_text(package.size()),
            package.manifest.name,
            package.manifest.holds()
        );
    }
    for known in rk_packages::known() {
        if !installed.iter().any(|p| p.manifest.id == known.id) {
            println!(
                "  {:<22} {:<8} {:>9}  {} (not installed)",
                known.id,
                known.version,
                size_text(known.size()),
                known.name
            );
        }
    }
    Ok(())
}

/// Checks an installed package against its checksums.
pub fn verify(id: &str) -> Result<()> {
    let problems = Store::user()?.verify(id, &Task::new())?;
    for problem in &problems {
        println!("{problem}");
    }
    ensure!(problems.is_empty(), "{id} is damaged: install it again");
    println!("{id} is sound");
    Ok(())
}

pub fn remove(id: &str) -> Result<()> {
    Store::user()?.remove(id)?;
    println!("removed {id}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(name: &str, bundles: &[&str], needs: &[&[usize]]) -> BundleList {
        BundleList {
            name: name.to_owned(),
            bundles: bundles.iter().map(|b| b.to_string()).collect(),
            needs: needs.iter().map(|n| n.to_vec()).collect(),
        }
    }

    fn lists() -> Vec<BundleList> {
        vec![
            list(
                "abdata",
                &[
                    "chara/oo_base.unity3d",
                    "chara/mm_base.unity3d",
                    "h/common/00_00.unity3d",
                    "map/shader.unity3d",
                    "sound/data/pcm/c00.unity3d",
                    "custom/presets_f_00.unity3d",
                    "chara/ao_kokan_00.unity3d",
                    "List/Characustom/00.unity3d",
                ],
                // The body needs its materials, which need a shader kept
                // with the maps and, oddly, something of the H scenes.
                &[&[1], &[3, 2], &[], &[], &[], &[], &[], &[]],
            ),
            list(
                "add50",
                &[
                    "chara/co_top_50.unity3d",
                    "h/list/50.unity3d",
                    "chara/thumb/co_top_50.unity3d",
                ],
                &[&[], &[], &[]],
            ),
        ]
    }

    #[test]
    fn only_what_the_games_lists_name_is_official() {
        let official = official(&lists());
        // What a repack or a mod added is in no list.
        for added in ["chara/oo_baseori.unity3d", "chara/some_mod/top.unity3d"] {
            assert!(!official.contains_key(added), "{added}");
        }
        assert_eq!(official["chara/oo_base.unity3d"], "abdata");
        assert_eq!(official["chara/co_top_50.unity3d"], "add50");
        // Paths are compared without case, as the game runs on Windows.
        assert!(official.contains_key("list/characustom/00.unity3d"));
        assert_eq!(official.len(), 11);
    }

    #[test]
    fn the_games_lists_are_told_by_their_names() {
        for name in ["abdata", "abdata_01", "add01", "add100", "studio00", "studio38"] {
            assert!(is_list_name(name), "{name}");
        }
        for name in [
            "abdata.txt",
            "add",
            "addon",
            "studio",
            "add01.bak",
            "mods",
            "add_01",
            "chara",
        ] {
            assert!(!is_list_name(name), "{name}");
        }
    }

    #[test]
    fn characters_take_their_parts_and_what_these_need_and_never_h_or_sound() {
        let selection = select(&lists(), &character_part);
        let take: Vec<&str> = selection.take.iter().map(String::as_str).collect();
        assert_eq!(
            take,
            [
                "chara/co_top_50.unity3d",
                "chara/mm_base.unity3d",
                "chara/oo_base.unity3d",
                "chara/thumb/co_top_50.unity3d",
                "list/characustom/00.unity3d",
                // Not a character part itself: the materials need it.
                "map/shader.unity3d",
            ]
        );
        assert_eq!(
            selection.refused,
            BTreeSet::from([("h/common/00_00.unity3d".to_owned(), "chara/mm_base.unity3d".to_owned())])
        );
        assert!(take.iter().all(|p| !never(p)));
        // The CharaStudio look's colour filter comes along.
        assert!(character_part("studio/info/00.unity3d") && character_part("studio/filter/00.unity3d"));
        assert!(!character_part("studio/map/00.unity3d"));
    }

    #[test]
    fn poses_take_studios_animations_and_lists_and_never_h_or_sound() {
        let lists = vec![
            list(
                "studio00",
                &[
                    "studio/anime/00.unity3d",
                    "studio/info/00.unity3d",
                    "studio/base/00.unity3d",
                    "h/anim/female/00_00_00.unity3d",
                    "sound/data/se/studio/00.unity3d",
                    "studio/item/00.unity3d",
                ],
                // The animations need a base bundle and, if a list said so,
                // something of the H scenes and a sound.
                &[&[2, 3, 4], &[], &[], &[], &[], &[]],
            ),
            list(
                "abdata",
                &[
                    "custom/custompose.unity3d",
                    "custom/presets_f_00.unity3d",
                    "h/anim/male/01_00_00.unity3d",
                    "chara/oo_base.unity3d",
                ],
                &[&[], &[], &[], &[]],
            ),
        ];
        let chosen = select(&lists, &pose_part);
        let take: Vec<&str> = chosen.take.iter().map(String::as_str).collect();
        assert_eq!(
            take,
            [
                "custom/custompose.unity3d",
                "studio/anime/00.unity3d",
                "studio/base/00.unity3d",
                "studio/info/00.unity3d",
            ]
        );
        let refused: Vec<&str> = chosen.refused.iter().map(|(need, _)| need.as_str()).collect();
        assert_eq!(
            refused,
            ["h/anim/female/00_00_00.unity3d", "sound/data/se/studio/00.unity3d"]
        );
        // A bundle a repack added is in no list, so nothing can choose it.
        assert!(pose_part("studio/anime/99.unity3d"));
        assert!(!official(&lists).contains_key("studio/anime/99.unity3d"));
        assert!(!pose_part("h/anim/female/00_00_00.unity3d"));
        assert!(!pose_part("studio/item/00.unity3d"));
    }

    #[test]
    fn h_scenes_voices_and_crotch_accessories_are_never_packaged() {
        for path in [
            "h/anim/female/00.unity3d",
            "sound/data/pcm/c00.unity3d",
            "chara/ao_kokan_00.unity3d",
            "chara/thumb/ao_kokan_00.unity3d",
        ] {
            assert!(never(path), "{path}");
        }
        for path in [
            "chara/ao_head_00.unity3d",
            "chara/thumb/ao_head_00.unity3d",
            "map/shader.unity3d",
        ] {
            assert!(!never(path), "{path}");
        }
        // Whatever asks for everything still gets none of them.
        let all = select(&lists(), &|_| true);
        assert!(all.take.iter().all(|p| !never(p)));
        assert_eq!(all.take.len(), 7);
    }

    #[test]
    fn the_games_own_characters_come_from_its_preset_bundles() {
        for path in [
            "custom/presets_f_00.unity3d",
            "custom/54/presets_m_ex.unity3d",
            "action/fixchara/00.unity3d",
            "action/mobchara/20.unity3d",
        ] {
            assert!(holds_cards(path), "{path}");
        }
        for path in [
            "custom/custom_etc.unity3d",
            "chara/oo_base.unity3d",
            "action/chara.unity3d",
        ] {
            assert!(!holds_cards(path), "{path}");
        }
        assert_eq!(
            card_file("custom/presets_f_00.unity3d", "ill_Default_Female"),
            "Default female.png"
        );
        assert_eq!(card_file("action/fixchara/00.unity3d", "c-8"), "Story c-8.png");
        assert_eq!(card_file("action/mobchara/00.unity3d", "tia_a/b"), "Extra tia a b.png");
    }
}
