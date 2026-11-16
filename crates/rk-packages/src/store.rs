// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The user's installed packages: a folder with one folder per package,
//! named after the package's id.

use crate::hash::{self, parse_checksums, same, Hasher};
use crate::manifest::{folder, valid_path, Manifest, CHECKSUMS};
use crate::{pack, Stage, Task, APP_VERSION};
use anyhow::{bail, ensure, Context, Result};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// The folder packages are installed into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    pub dir: PathBuf,
}

/// A package that is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

impl Installed {
    /// The package in a folder, when the folder holds one.
    pub fn read(dir: &Path) -> Result<Installed> {
        Ok(Installed {
            manifest: Manifest::read(&dir.join(rk_core::PACKAGE_FILE))?,
            dir: dir.to_owned(),
        })
    }

    /// Bytes the package takes on disk.
    pub fn size(&self) -> u64 {
        let mut files = Vec::new();
        walk(&self.dir, &mut files);
        files.iter().filter_map(|f| f.metadata().ok()).map(|m| m.len()).sum()
    }

    /// The package's character cards, in path order.
    pub fn cards(&self) -> Vec<PathBuf> {
        let mut cards = Vec::new();
        rk_core::walk_files(&self.dir.join(folder::CARDS), "png", &mut cards);
        cards.sort();
        cards
    }

    /// The card new projects start with, when the package names one that
    /// it holds.
    pub fn default_card(&self) -> Option<PathBuf> {
        let card = self.manifest.default_card.as_deref()?;
        Some(self.dir.join(card)).filter(|p| valid_path(card) && p.is_file())
    }
}

impl Store {
    pub fn at(dir: &Path) -> Store {
        Store { dir: dir.to_owned() }
    }

    /// The user's own packages folder.
    pub fn user() -> Result<Store> {
        let dir = rk_core::packages_dir().context("no data folder is known for packages")?;
        Ok(Store { dir })
    }

    /// The installed packages, by name. A folder whose package file does
    /// not read is left out.
    pub fn installed(&self) -> Vec<Installed> {
        let mut found: Vec<Installed> = rk_core::package_roots_in(&self.dir)
            .iter()
            .filter_map(|dir| Installed::read(dir).ok())
            .collect();
        found.sort_by_key(|p| p.manifest.name.to_lowercase());
        found
    }

    pub fn find(&self, id: &str) -> Option<Installed> {
        self.installed().into_iter().find(|p| p.manifest.id == id)
    }

    /// The folder downloads and joined parts wait in.
    pub fn downloads(&self) -> PathBuf {
        self.dir.join(".downloads")
    }

    /// Installs a package from its archive, or from the first part of one
    /// that was cut into parts, in place of the version that is installed.
    pub fn install(&self, archive: &Path, task: &Task) -> Result<Installed> {
        if let Some((parts, whole)) = pack::parts_of(archive) {
            let joined = self.downloads().join(whole.file_name().unwrap_or_default());
            std::fs::create_dir_all(self.downloads())?;
            pack::join(&parts, &joined)?;
            let done = self.install_whole(&joined, task);
            let _ = std::fs::remove_file(&joined);
            return done;
        }
        self.install_whole(archive, task)
    }

    fn install_whole(&self, archive: &Path, task: &Task) -> Result<Installed> {
        let file = std::fs::File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
        let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))
            .with_context(|| format!("{} is not a package", archive.display()))?;
        let mut text = String::new();
        zip.by_name(rk_core::PACKAGE_FILE)
            .with_context(|| format!("{} has no package.toml", archive.display()))?
            .read_to_string(&mut text)?;
        let manifest = Manifest::parse(&text)?;
        manifest.check_app(APP_VERSION)?;
        let mut text = String::new();
        zip.by_name(CHECKSUMS)
            .context("the package has no checksums.txt")?
            .read_to_string(&mut text)?;
        let sums = parse_checksums(&text)?;

        // Every file is one the package may have and has a checksum, and
        // every checksum has its file.
        let mut total = 0u64;
        let mut seen = 0usize;
        for i in 0..zip.len() {
            let entry = zip.by_index(i)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name()?.into_owned();
            ensure!(valid_path(&name), "the package holds {name}, which it may not");
            if name != CHECKSUMS {
                ensure!(sums.contains_key(&name), "the package has no checksum for {name}");
                seen += 1;
            }
            total += entry.size();
        }
        ensure!(seen == sums.len(), "the package lacks files its checksums name");

        std::fs::create_dir_all(&self.dir).with_context(|| format!("creating {}", self.dir.display()))?;
        let staging = self.dir.join(format!(".installing-{}", manifest.id));
        let _ = std::fs::remove_dir_all(&staging);
        task.begin(Stage::Installing, total);
        let unpacked = unpack(&mut zip, &sums, &staging, task);
        if let Err(e) = unpacked {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
        // The new version takes the place of the old one in one step.
        let place = self.dir.join(&manifest.id);
        let old = self.dir.join(format!(".removing-{}", manifest.id));
        let _ = std::fs::remove_dir_all(&old);
        if place.exists() {
            std::fs::rename(&place, &old).with_context(|| format!("moving {} away", place.display()))?;
        }
        std::fs::rename(&staging, &place).with_context(|| format!("moving the package to {}", place.display()))?;
        let _ = std::fs::remove_dir_all(&old);
        Installed::read(&place)
    }

    /// Takes an installed package away.
    pub fn remove(&self, id: &str) -> Result<()> {
        ensure!(crate::manifest::valid_id(id), "{id:?} is not a package id");
        let place = self.dir.join(id);
        ensure!(
            place.join(rk_core::PACKAGE_FILE).is_file(),
            "{id} is not installed in {}",
            self.dir.display()
        );
        // Out of the list first, so that nothing reads it half removed.
        let old = self.dir.join(format!(".removing-{id}"));
        let _ = std::fs::remove_dir_all(&old);
        std::fs::rename(&place, &old).with_context(|| format!("moving {} away", place.display()))?;
        std::fs::remove_dir_all(&old).with_context(|| format!("removing {}", old.display()))
    }

    /// Checks an installed package against its checksums: what is wrong
    /// with it, one line per file, or nothing.
    pub fn verify(&self, id: &str, task: &Task) -> Result<Vec<String>> {
        let package = self.find(id).with_context(|| format!("{id} is not installed"))?;
        let text = std::fs::read_to_string(package.dir.join(CHECKSUMS)).context("the package has no checksums.txt")?;
        let sums = parse_checksums(&text)?;
        let total = sums
            .keys()
            .filter_map(|p| package.dir.join(p).metadata().ok())
            .map(|m| m.len())
            .sum();
        task.begin(Stage::Checking, total);
        let mut problems = Vec::new();
        for (path, sum) in &sums {
            let file = package.dir.join(path);
            if !file.is_file() {
                problems.push(format!("{path} is missing"));
                continue;
            }
            let (found, _) = hash::sha256_file(&file, Some(task))?;
            if !same(&found, sum) {
                problems.push(format!("{path} has changed"));
            }
        }
        let mut files = Vec::new();
        walk(&package.dir, &mut files);
        for file in files {
            let path = file
                .strip_prefix(&package.dir)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            if path != CHECKSUMS && !sums.contains_key(&path) {
                problems.push(format!("{path} does not belong to the package"));
            }
        }
        problems.sort();
        Ok(problems)
    }
}

/// Unpacks every file into `dir`, checking each against its checksum.
fn unpack<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    sums: &BTreeMap<String, String>,
    dir: &Path,
    task: &Task,
) -> Result<()> {
    let mut buf = vec![0u8; 1 << 16];
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name()?.into_owned();
        let out = dir.join(&name);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let mut file = std::io::BufWriter::new(
            std::fs::File::create(&out).with_context(|| format!("creating {}", out.display()))?,
        );
        let mut hasher = Hasher::new();
        loop {
            let n = entry.read(&mut buf).with_context(|| format!("unpacking {name}"))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])?;
            task.add(n as u64)?;
        }
        file.flush()?;
        if let Some(sum) = sums.get(&name) {
            if !same(&hasher.hex(), sum) {
                bail!("{name} is damaged: its checksum does not match");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::sample_pack;

    #[test]
    fn a_package_is_installed_listed_checked_and_removed() {
        let dir = crate::scratch("store");
        let archive = sample_pack(&dir, "sample", "1.0.0");
        let store = Store::at(&dir.join("packages"));
        assert!(store.installed().is_empty());
        let task = Task::new();
        let installed = store.install(&archive, &task).unwrap();
        assert_eq!(installed.dir, store.dir.join("sample"));
        assert_eq!(installed.manifest.version, "1.0.0");
        let (stage, done, total) = task.progress();
        assert_eq!(stage, Stage::Installing);
        assert_eq!(done, total);
        assert_eq!(
            std::fs::read(installed.dir.join("abdata/chara/oo_base.unity3d")).unwrap(),
            vec![7u8; 5000]
        );
        assert_eq!(installed.cards(), [installed.dir.join("cards/female/Default.png")]);
        assert_eq!(
            installed.default_card(),
            Some(installed.dir.join("cards/female/Default.png"))
        );
        assert!(installed.size() > 5000);
        assert_eq!(store.installed(), std::slice::from_ref(&installed));
        assert_eq!(store.find("sample"), Some(installed.clone()));
        assert_eq!(
            rk_core::package_roots_in(&store.dir),
            std::slice::from_ref(&installed.dir)
        );

        // Sound as installed; a changed, a missing and a stray file show.
        assert!(store.verify("sample", &Task::new()).unwrap().is_empty());
        std::fs::write(installed.dir.join("cards/female/Default.png"), b"other").unwrap();
        std::fs::remove_file(installed.dir.join("abdata/list/characustom/00.unity3d")).unwrap();
        std::fs::write(installed.dir.join("abdata/stray.unity3d"), b"x").unwrap();
        assert_eq!(
            store.verify("sample", &Task::new()).unwrap(),
            [
                "abdata/list/characustom/00.unity3d is missing",
                "abdata/stray.unity3d does not belong to the package",
                "cards/female/Default.png has changed",
            ]
        );

        store.remove("sample").unwrap();
        assert!(store.installed().is_empty());
        assert!(!installed.dir.exists());
        assert!(store.remove("sample").is_err());
        assert!(store.remove("../packages").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_version_takes_the_place_of_the_installed_one() {
        let dir = crate::scratch("store-update");
        let store = Store::at(&dir.join("packages"));
        store
            .install(&sample_pack(&dir, "sample", "1.0.0"), &Task::new())
            .unwrap();
        std::fs::write(store.dir.join("sample/abdata/left-over.unity3d"), b"x").unwrap();
        let installed = store
            .install(&sample_pack(&dir, "sample", "1.1.0"), &Task::new())
            .unwrap();
        assert_eq!(installed.manifest.version, "1.1.0");
        assert!(!installed.dir.join("abdata/left-over.unity3d").exists());
        assert_eq!(store.installed().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_package_in_parts_is_installed_from_its_first_part() {
        let dir = crate::scratch("store-parts");
        let archive = sample_pack(&dir, "sample", "1.0.0");
        let parts = pack::split(&archive, 300).unwrap();
        assert!(parts.len() > 1);
        let store = Store::at(&dir.join("packages"));
        let installed = store.install(&parts[0], &Task::new()).unwrap();
        assert!(store.verify("sample", &Task::new()).unwrap().is_empty());
        assert_eq!(installed.manifest.id, "sample");
        assert!(std::fs::read_dir(store.downloads()).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn damaged_and_foreign_archives_are_refused_and_leave_nothing() {
        let dir = crate::scratch("store-bad");
        let store = Store::at(&dir.join("packages"));
        let not_zip = dir.join("not.rkpkg");
        std::fs::write(&not_zip, b"not a zip").unwrap();
        assert!(store.install(&not_zip, &Task::new()).is_err());

        // An archive whose file does not match its checksum.
        let bad = dir.join("bad.rkpkg");
        let manifest = crate::manifest::sample("bad", "1").to_text().unwrap();
        let sums = BTreeMap::from([
            ("package.toml".to_owned(), hash::sha256_bytes(manifest.as_bytes())),
            ("cards/a.png".to_owned(), hash::sha256_bytes(b"expected")),
        ]);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&bad).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in [
            ("package.toml", manifest.as_bytes()),
            (CHECKSUMS, hash::checksums_text(&sums).as_bytes()),
            ("cards/a.png", b"tampered".as_slice()),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        let err = store.install(&bad, &Task::new()).unwrap_err();
        assert!(format!("{err:#}").contains("damaged"), "{err:#}");
        assert!(store.installed().is_empty());
        assert!(!store.dir.join(".installing-bad").exists());

        // A cancelled install leaves nothing either.
        let task = Task::new();
        task.cancel();
        assert!(store.install(&sample_pack(&dir, "sample", "1"), &task).is_err());
        assert!(store.installed().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
