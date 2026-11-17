// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Packages anyone can make: `rk package new` lays out a folder with its
//! `package.toml`, the maker puts content in its folders, and
//! `rk package build` checks the folder and writes the `.rkpkg` that
//! Renkatsu installs (Window, Packages, Install from a file).
//!
//! ```text
//! my-poses/
//!   package.toml        id, name, version, description
//!   abdata/             game style bundles, at the paths the game uses,
//!                       with their list files (as a zipmod's abdata)
//!   cards/female/       character cards
//!   cards/male/
//!   mods/               zipmods, read with Sideloader's rules
//!   animations/ poses/ maps/ items/ scenes/
//! ```

use anyhow::{bail, ensure, Context, Result};
use rk_packages::manifest::{folder, valid_id, valid_path, Manifest, EXTENSION, FORMAT};
use rk_packages::pack::Pack;
use rk_packages::size_text;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The family of packages people make themselves.
pub const KIND: &str = "custom";

/// The folders a new package starts with.
const START: [&str; 9] = [
    "abdata",
    "cards/female",
    "cards/male",
    folder::MODS,
    folder::ANIMATIONS,
    folder::POSES,
    folder::MAPS,
    folder::ITEMS,
    folder::SCENES,
];

/// The manifest of a new package.
pub fn manifest(id: &str, name: &str) -> Manifest {
    Manifest {
        format: FORMAT,
        id: id.to_owned(),
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        kind: KIND.to_owned(),
        description: String::new(),
        requires_renkatsu: rk_packages::APP_VERSION.to_owned(),
        source: String::new(),
        default_card: None,
        contents: BTreeMap::new(),
    }
}

/// Lays out a new package in `dir`, which must not hold files yet.
pub fn new(dir: &Path, id: &str, name: Option<&str>) -> Result<()> {
    ensure!(
        valid_id(id),
        "the id {id:?} must be lower case letters, digits and dashes, such as my-poses"
    );
    if std::fs::read_dir(dir).is_ok_and(|mut d| d.next().is_some()) {
        bail!("{} already holds files; choose an empty or new folder", dir.display());
    }
    let name = name.map_or_else(|| id.replace('-', " "), str::to_owned);
    for sub in START {
        let path = dir.join(sub);
        std::fs::create_dir_all(&path).with_context(|| format!("creating {}", path.display()))?;
    }
    let file = dir.join(rk_core::PACKAGE_FILE);
    std::fs::write(&file, manifest(id, &name).to_text()?).with_context(|| format!("writing {}", file.display()))?;
    println!("made {} with {}", dir.display(), rk_core::PACKAGE_FILE);
    println!("fill in its description, put content in its folders, then run:");
    println!("  rk package build {} --out <folder for the .rkpkg>", dir.display());
    Ok(())
}

/// Every file below `dir`, as package paths with forward slashes.
fn files(dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).with_context(|| format!("reading {}", d.display()))? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(dir)
                .expect("below the package folder")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push((rel, path));
        }
    }
    out.sort();
    Ok(out)
}

/// How many things of each kind the files hold, as the manifest counts
/// them: bundles, cards, and the files of the other folders.
pub fn contents(paths: &[&str]) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for path in paths {
        let (top, rest) = path.split_once('/').unwrap_or((path, ""));
        let kind = match top {
            folder::ABDATA if rest.ends_with(".unity3d") => "bundles",
            folder::CARDS if rest.to_ascii_lowercase().ends_with(".png") => "cards",
            folder::ANIMATIONS => "animations",
            folder::POSES => "poses",
            folder::MAPS => "maps",
            folder::ITEMS => "items",
            folder::SCENES => "scenes",
            folder::MODS if rest.to_ascii_lowercase().ends_with(".zipmod") => "mods",
            _ => continue,
        };
        *counts.entry(kind.to_owned()).or_insert(0) += 1;
    }
    counts
}

/// The files of a package folder that go into the package, and the ones
/// that cannot, with the reason.
pub fn sort_files(all: Vec<(String, PathBuf)>) -> (Vec<(String, PathBuf)>, Vec<String>) {
    let mut take = Vec::new();
    let mut refused = Vec::new();
    for (rel, path) in all {
        if rel == rk_core::PACKAGE_FILE || rel == rk_packages::manifest::CHECKSUMS {
            continue;
        }
        let name = rel.rsplit('/').next().unwrap_or(&rel);
        // Files the system leaves behind are not content.
        if name.starts_with('.') || name.eq_ignore_ascii_case("thumbs.db") || name.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }
        if valid_path(&rel) {
            take.push((rel, path));
        } else {
            refused.push(rel);
        }
    }
    (take, refused)
}

/// Checks a package folder and writes `<id>-<version>.rkpkg` into `out`.
pub fn build(dir: &Path, out: &Path) -> Result<()> {
    let mut manifest = Manifest::read(&dir.join(rk_core::PACKAGE_FILE)).with_context(|| {
        format!(
            "{} is not a package folder; make one with rk package new",
            dir.display()
        )
    })?;
    let (take, refused) = sort_files(files(dir)?);
    if !refused.is_empty() {
        for rel in &refused {
            eprintln!("  {rel}");
        }
        let folders = folder::ALL.join(", ");
        bail!(
            "{} files above are outside the package's folders ({folders}); move or remove them",
            refused.len()
        );
    }
    ensure!(!take.is_empty(), "the package folder holds no content yet");
    let paths: Vec<&str> = take.iter().map(|(rel, _)| rel.as_str()).collect();
    manifest.contents = contents(&paths);
    // Older Renkatsu refuses a mods folder, so such a package says it
    // needs one that reads it.
    if manifest.contents.contains_key("mods")
        && rk_packages::manifest::version_key(&manifest.requires_renkatsu)
            < rk_packages::manifest::version_key(folder::MODS_SINCE)
    {
        manifest.requires_renkatsu = folder::MODS_SINCE.to_owned();
    }
    if let Some(card) = &manifest.default_card {
        ensure!(
            paths.contains(&card.as_str()),
            "default_card {card} is not a file of the package"
        );
    }
    let mut pack = Pack::new(manifest.clone());
    for (rel, path) in &take {
        pack.add_file(rel, path)?;
    }
    let file = out.join(format!("{}-{}.{EXTENSION}", manifest.id, manifest.version));
    let built = pack.write(&file, &mut |_| {})?;
    println!("built {}", built.file.display());
    let holds = match manifest.holds() {
        // Lists and other files only, such as a mod's new poses made of
        // animations the game already has.
        h if h.is_empty() => "lists and data".to_owned(),
        h => h,
    };
    println!("  {} {}, {holds}", manifest.name, manifest.version);
    println!(
        "  {} files, {} to share, {} installed",
        built.files,
        size_text(built.size),
        size_text(built.installed_size)
    );
    println!("  sha256 {}", built.sha256);
    println!("install it in Renkatsu Studio with Window, Packages, Install from a file,");
    println!("or with: rk package install {}", built.file.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_is_counted_by_its_folder() {
        let counts = contents(&[
            "abdata/studio/anime/00.unity3d",
            "abdata/list/studio/00.unity3d",
            "abdata/studio/info/readme.txt",
            "cards/female/A.png",
            "cards/male/B.PNG",
            "poses/wave.png",
            "mods/Some poses v1.0.zipmod",
        ]);
        assert_eq!(counts["mods"], 1);
        assert_eq!(counts["bundles"], 2);
        assert_eq!(counts["cards"], 2);
        assert_eq!(counts["poses"], 1);
        assert!(!counts.contains_key("animations"));
    }

    #[test]
    fn files_outside_the_folders_are_refused_and_system_files_skipped() {
        let p = |s: &str| (s.to_owned(), PathBuf::from(s));
        let (take, refused) = sort_files(vec![
            p("package.toml"),
            p("abdata/studio/anime/00.unity3d"),
            p("notes.txt"),
            p("cards/.DS_Store"),
            p("stuff/a.png"),
        ]);
        assert_eq!(take.len(), 1);
        assert_eq!(refused, ["notes.txt", "stuff/a.png"]);
    }

    #[test]
    fn a_new_package_builds_and_reads_back() {
        let root = std::env::temp_dir().join(format!("rk-custom-package-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("my-poses");
        new(&dir, "my-poses", Some("My poses")).unwrap();
        assert!(new(&dir, "my-poses", None).is_err(), "the folder holds files now");
        assert!(new(&root.join("x"), "My Poses", None).is_err(), "not an id");
        assert!(build(&dir, &root).is_err(), "nothing in it yet");
        std::fs::create_dir_all(dir.join("abdata/studio/anime")).unwrap();
        std::fs::write(dir.join("abdata/studio/anime/00.unity3d"), b"bundle").unwrap();
        std::fs::write(dir.join("cards/female/A.png"), b"card").unwrap();
        build(&dir, &root).unwrap();
        let file = root.join("my-poses-1.0.0.rkpkg");
        let store = rk_packages::Store::at(&root.join("data"));
        let installed = store.install(&file, &rk_packages::Task::new()).unwrap();
        assert_eq!(installed.manifest.name, "My poses");
        assert_eq!(installed.manifest.kind, KIND);
        assert_eq!(installed.manifest.contents["bundles"], 1);
        assert_eq!(installed.manifest.contents["cards"], 1);
        // A stray file at the top of the folder stops the build.
        std::fs::write(dir.join("notes.txt"), b"").unwrap();
        assert!(build(&dir, &root).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
