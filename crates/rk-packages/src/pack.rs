// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Writing a package archive, and cutting one into parts that fit where
//! it is hosted.
//!
//! The same files give the same archive, byte for byte: entries are
//! written in path order with one fixed date and one fixed mode, and
//! nothing of the machine or the day goes in.

use crate::hash::{self, checksums_text};
use crate::manifest::{valid_path, Manifest, CHECKSUMS};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

enum Source {
    File(PathBuf),
    Bytes(Vec<u8>),
}

/// A package being put together.
pub struct Pack {
    manifest: Manifest,
    files: BTreeMap<String, Source>,
}

/// A package that was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    pub file: PathBuf,
    /// Bytes of the archive.
    pub size: u64,
    pub sha256: String,
    /// Files in the archive, the manifest and the checksums included.
    pub files: usize,
    /// Bytes of the files once installed.
    pub installed_size: u64,
}

impl Pack {
    pub fn new(manifest: Manifest) -> Pack {
        Pack {
            manifest,
            files: BTreeMap::new(),
        }
    }

    pub fn manifest_mut(&mut self) -> &mut Manifest {
        &mut self.manifest
    }

    /// Adds a file on disk under a path of the package.
    pub fn add_file(&mut self, path: &str, source: &Path) -> Result<()> {
        ensure!(valid_path(path), "{path} is not a path a package may have");
        self.files.insert(path.to_owned(), Source::File(source.to_owned()));
        Ok(())
    }

    /// Adds a file from memory under a path of the package.
    pub fn add_bytes(&mut self, path: &str, bytes: Vec<u8>) -> Result<()> {
        ensure!(valid_path(path), "{path} is not a path a package may have");
        self.files.insert(path.to_owned(), Source::Bytes(bytes));
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Writes the archive. `progress` is told the bytes read so far.
    pub fn write(&self, out: &Path, progress: &mut dyn FnMut(u64)) -> Result<Built> {
        self.manifest.check()?;
        let manifest = self.manifest.to_text()?.into_bytes();
        // The checksums go in first, so every file is read twice: once
        // for its hash, once for the archive.
        let mut sums = BTreeMap::new();
        let mut installed_size = manifest.len() as u64;
        let mut read = 0u64;
        sums.insert(rk_core::PACKAGE_FILE.to_owned(), hash::sha256_bytes(&manifest));
        for (path, source) in &self.files {
            let (sum, size) = match source {
                Source::Bytes(bytes) => (hash::sha256_bytes(bytes), bytes.len() as u64),
                Source::File(file) => hash::sha256_file(file, None)?,
            };
            installed_size += size;
            read += size;
            progress(read);
            sums.insert(path.clone(), sum);
        }
        let checksums = checksums_text(&sums).into_bytes();
        installed_size += checksums.len() as u64;

        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let file = std::fs::File::create(out).with_context(|| format!("creating {}", out.display()))?;
        let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
        let options = |path: &str| {
            // Pictures are compressed already.
            let method = if path.ends_with(".png") {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            };
            zip::write::SimpleFileOptions::default()
                .compression_method(method)
                .last_modified_time(zip::DateTime::default())
                .unix_permissions(0o644)
                .large_file(true)
        };
        zip.start_file(rk_core::PACKAGE_FILE, options(rk_core::PACKAGE_FILE))?;
        zip.write_all(&manifest)?;
        zip.start_file(CHECKSUMS, options(CHECKSUMS))?;
        zip.write_all(&checksums)?;
        let mut buf = vec![0u8; 1 << 16];
        for (path, source) in &self.files {
            zip.start_file(path.as_str(), options(path))?;
            match source {
                Source::Bytes(bytes) => {
                    zip.write_all(bytes)?;
                    read += bytes.len() as u64;
                }
                Source::File(file) => {
                    let mut f = std::fs::File::open(file).with_context(|| format!("opening {}", file.display()))?;
                    loop {
                        let n = f.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        zip.write_all(&buf[..n])?;
                        read += n as u64;
                    }
                }
            }
            progress(read);
        }
        zip.finish()?.flush()?;
        let (sha256, size) = hash::sha256_file(out, None)?;
        Ok(Built {
            file: out.to_owned(),
            size,
            sha256,
            files: self.files.len() + 2,
            installed_size,
        })
    }
}

/// The name of part `n` of an archive, counted from 1: `name.rkpkg.001`.
pub fn part_name(file: &Path, n: usize) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(format!(".{n:03}"));
    PathBuf::from(name)
}

/// Cuts an archive into parts of at most `max` bytes beside it and takes
/// the whole away. An archive that fits stays as it is. The parts, put
/// end to end, are the archive.
pub fn split(file: &Path, max: u64) -> Result<Vec<PathBuf>> {
    ensure!(max > 0, "parts need a size");
    let size = file
        .metadata()
        .with_context(|| format!("reading {}", file.display()))?
        .len();
    if size <= max {
        return Ok(vec![file.to_owned()]);
    }
    let mut source = std::fs::File::open(file)?;
    let mut parts = Vec::new();
    let mut left = size;
    while left > 0 {
        let part = part_name(file, parts.len() + 1);
        let take = left.min(max);
        let mut out = std::fs::File::create(&part).with_context(|| format!("creating {}", part.display()))?;
        std::io::copy(&mut (&mut source).take(take), &mut out)?;
        left -= take;
        parts.push(part);
    }
    std::fs::remove_file(file)?;
    Ok(parts)
}

/// The parts an archive was cut into, when `file` is its first part
/// (`name.rkpkg.001`): the parts in order, and the archive's own name.
pub fn parts_of(file: &Path) -> Option<(Vec<PathBuf>, PathBuf)> {
    if file.extension()?.to_str()? != "001" {
        return None;
    }
    let whole = file.with_extension("");
    let mut parts = Vec::new();
    loop {
        let part = part_name(&whole, parts.len() + 1);
        if !part.is_file() {
            break;
        }
        parts.push(part);
    }
    Some((parts, whole))
}

/// Puts parts end to end into one file.
pub fn join(parts: &[PathBuf], out: &Path) -> Result<()> {
    let mut whole = std::fs::File::create(out).with_context(|| format!("creating {}", out.display()))?;
    for part in parts {
        let mut f = std::fs::File::open(part).with_context(|| format!("opening {}", part.display()))?;
        std::io::copy(&mut f, &mut whole)?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn sample_pack(dir: &Path, id: &str, version: &str) -> PathBuf {
    let mut manifest = crate::manifest::sample(id, version);
    manifest.default_card = Some("cards/female/Default.png".to_owned());
    let mut pack = Pack::new(manifest);
    let bundle = dir.join("source.unity3d");
    std::fs::write(&bundle, vec![7u8; 5000]).unwrap();
    pack.add_file("abdata/chara/oo_base.unity3d", &bundle).unwrap();
    pack.add_bytes("abdata/list/characustom/00.unity3d", b"list".to_vec())
        .unwrap();
    pack.add_bytes("cards/female/Default.png", b"card".to_vec()).unwrap();
    let out = dir.join(format!("{id}-{version}.rkpkg"));
    pack.write(&out, &mut |_| {}).unwrap();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_files_give_the_same_archive() {
        let dir = crate::scratch("pack");
        let first = sample_pack(&dir, "sample", "1.0.0");
        let (sum, _) = hash::sha256_file(&first, None).unwrap();
        std::fs::remove_file(&first).unwrap();
        // Another day: the files' own dates do not matter.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let again = sample_pack(&dir, "sample", "1.0.0");
        assert_eq!(hash::sha256_file(&again, None).unwrap().0, sum);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_lists_itself_and_refuses_paths_outside() {
        let dir = crate::scratch("pack-list");
        let file = sample_pack(&dir, "sample", "1.0.0");
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&file).unwrap()).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().unwrap().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "package.toml",
                "checksums.txt",
                "abdata/chara/oo_base.unity3d",
                "abdata/list/characustom/00.unity3d",
                "cards/female/Default.png",
            ]
        );
        let mut text = String::new();
        zip.by_name(CHECKSUMS).unwrap().read_to_string(&mut text).unwrap();
        let sums = hash::parse_checksums(&text).unwrap();
        assert_eq!(sums.len(), 4);
        assert_eq!(sums["cards/female/Default.png"], hash::sha256_bytes(b"card"));
        let mut pack = Pack::new(crate::manifest::sample("a", "1"));
        assert!(pack.add_bytes("../outside", Vec::new()).is_err());
        assert!(pack.add_bytes("loose.txt", Vec::new()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_is_cut_into_parts_and_put_together_again() {
        let dir = crate::scratch("split");
        let file = dir.join("big.rkpkg");
        let bytes: Vec<u8> = (0..2500u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&file, &bytes).unwrap();
        assert_eq!(split(&file, 5000).unwrap(), std::slice::from_ref(&file));
        let parts = split(&file, 1000).unwrap();
        assert_eq!(parts.len(), 3);
        assert!(!file.exists());
        assert_eq!(parts[2].metadata().unwrap().len(), 500);
        assert_eq!(parts[0].file_name().unwrap(), "big.rkpkg.001");
        let (found, whole) = parts_of(&parts[0]).unwrap();
        assert_eq!((found, whole.clone()), (parts.clone(), file.clone()));
        assert!(parts_of(&parts[1]).is_none());
        join(&parts, &whole).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
