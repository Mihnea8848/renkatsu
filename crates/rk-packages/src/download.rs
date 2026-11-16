// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Downloading a known package: each part over HTTPS into the store's
//! downloads folder, checked against its SHA-256, then installed.
//!
//! A download that stopped goes on from where it was when it is asked
//! for again. `file://` addresses are read from disk, for tests and for
//! a copy of the files on a local drive.

use crate::hash::{same, Hasher};
use crate::index::{Known, Part};
use crate::store::{Installed, Store};
use crate::{pack, Stage, Task};
use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) fn agent() -> ureq::Agent {
    // rustls with ring, and the certificates the system trusts. Another
    // part of the program may have chosen the provider already.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .https_only(true)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(120)))
        .user_agent(concat!("renkatsu/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// A part's bytes from `from` on, and whether the source honoured `from`.
fn open(url: &str, from: u64) -> Result<(Box<dyn Read>, bool)> {
    if let Some(path) = url.strip_prefix("file://") {
        use std::io::Seek;
        let mut file = std::fs::File::open(path).with_context(|| format!("opening {path}"))?;
        file.seek(std::io::SeekFrom::Start(from))?;
        return Ok((Box::new(file), true));
    }
    let mut request = agent().get(url);
    if from > 0 {
        request = request.header("Range", format!("bytes={from}-"));
    }
    let response = request.call().with_context(|| format!("asking for {url}"))?;
    let resumed = response.status().as_u16() == 206;
    Ok((Box::new(response.into_body().into_reader()), resumed))
}

/// Downloads one part to `file`, going on from what is there already,
/// and checks it against its hash.
fn part(part: &Part, file: &Path, task: &Task) -> Result<()> {
    let partial = PathBuf::from(format!("{}.part", file.display()));
    let mut hasher = Hasher::new();
    let mut have = 0u64;
    // What a stopped download left is hashed again and kept.
    if let Ok(mut f) = std::fs::File::open(&partial) {
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            have += n as u64;
        }
    }
    if have > part.size {
        have = 0;
    }
    let (mut body, resumed) = if have == part.size {
        (Box::new(std::io::empty()) as Box<dyn Read>, true)
    } else {
        open(&part.url, have)?
    };
    if !resumed {
        have = 0;
        hasher = Hasher::new();
    }
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(have == 0)
        .append(have > 0)
        .open(&partial)
        .with_context(|| format!("writing {}", partial.display()))?;
    task.add(have)?;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = body
            .read(&mut buf)
            .with_context(|| format!("downloading {}", part.url))?;
        if n == 0 {
            break;
        }
        have += n as u64;
        if have > part.size {
            drop(out);
            let _ = std::fs::remove_file(&partial);
            bail!("{} is larger than the {} bytes it should have", part.url, part.size);
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        task.add(n as u64)?;
    }
    out.flush()?;
    drop(out);
    if have < part.size {
        bail!(
            "the download of {} stopped at {have} of {} bytes; ask again to go on",
            part.url,
            part.size
        );
    }
    if !same(&hasher.hex(), &part.sha256) {
        let _ = std::fs::remove_file(&partial);
        bail!("{} is not the file that was expected: its SHA-256 differs", part.url);
    }
    std::fs::rename(&partial, file)?;
    Ok(())
}

/// Downloads a known package into the store's downloads folder and gives
/// the archive, whole.
pub fn download(known: &Known, store: &Store, task: &Task) -> Result<PathBuf> {
    let dir = store.downloads();
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let whole = dir.join(known.file_name());
    task.begin(Stage::Downloading, known.size());
    if let [only] = known.parts.as_slice() {
        part(only, &whole, task)?;
        return Ok(whole);
    }
    let mut files = Vec::new();
    for (n, p) in known.parts.iter().enumerate() {
        let file = pack::part_name(&whole, n + 1);
        part(p, &file, task)?;
        files.push(file);
    }
    pack::join(&files, &whole)?;
    for file in files {
        let _ = std::fs::remove_file(file);
    }
    Ok(whole)
}

/// Downloads a known package, installs it and clears the download away.
pub fn fetch(known: &Known, store: &Store, task: &Task) -> Result<Installed> {
    let archive = download(known, store, task)?;
    let installed = store.install(&archive, task);
    // A cancelled install keeps the download, which is sound, for next time.
    if installed.is_ok() || !task.cancelled() {
        let _ = std::fs::remove_file(&archive);
    }
    let installed = installed?;
    anyhow::ensure!(
        installed.manifest.id == known.id,
        "the download held {} and not {}",
        installed.manifest.id,
        known.id
    );
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::sha256_file;
    use crate::pack::sample_pack;

    fn known_from(parts: &[PathBuf]) -> Known {
        Known {
            id: "sample".to_owned(),
            name: "Sample".to_owned(),
            version: "1.0.0".to_owned(),
            description: String::new(),
            installed_size: 0,
            parts: parts
                .iter()
                .map(|p| {
                    let (sha256, size) = sha256_file(p, None).unwrap();
                    Part {
                        url: format!("file://{}", p.display()),
                        size,
                        sha256,
                    }
                })
                .collect(),
        }
    }

    #[test]
    fn a_known_package_is_downloaded_checked_and_installed() {
        let dir = crate::scratch("download");
        let archive = sample_pack(&dir, "sample", "1.0.0");
        let known = known_from(std::slice::from_ref(&archive));
        let store = Store::at(&dir.join("packages"));
        let task = Task::new();
        let installed = fetch(&known, &store, &task).unwrap();
        assert_eq!(installed.manifest.id, "sample");
        assert!(store.verify("sample", &Task::new()).unwrap().is_empty());
        // Nothing of the download stays behind.
        assert!(std::fs::read_dir(store.downloads()).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_package_in_parts_is_downloaded_and_joined() {
        let dir = crate::scratch("download-parts");
        let archive = sample_pack(&dir, "sample", "1.0.0");
        let parts = pack::split(&archive, 400).unwrap();
        assert!(parts.len() > 1);
        let known = known_from(&parts);
        let store = Store::at(&dir.join("packages"));
        let task = Task::new();
        fetch(&known, &store, &task).unwrap();
        assert_eq!(task.progress().0, Stage::Installing);
        assert!(store.find("sample").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_with_another_hash_is_refused() {
        let dir = crate::scratch("download-bad");
        let archive = sample_pack(&dir, "sample", "1.0.0");
        let mut known = known_from(std::slice::from_ref(&archive));
        known.parts[0].sha256 = "0".repeat(64);
        let store = Store::at(&dir.join("packages"));
        let err = fetch(&known, &store, &Task::new()).unwrap_err();
        assert!(format!("{err:#}").contains("SHA-256"), "{err:#}");
        assert!(store.installed().is_empty());
        assert!(std::fs::read_dir(store.downloads()).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Needs the network, so it only runs when asked for:
    /// `cargo test -p rk-packages -- --ignored`.
    #[test]
    #[ignore]
    fn https_answers_whole_and_from_an_offset() {
        let url = "https://raw.githubusercontent.com/rust-lang/rust/1.0.0/COPYRIGHT";
        let mut whole = Vec::new();
        open(url, 0).unwrap().0.read_to_end(&mut whole).unwrap();
        assert!(whole.len() > 1000);
        let (mut body, resumed) = open(url, 1000).unwrap();
        let mut rest = Vec::new();
        body.read_to_end(&mut rest).unwrap();
        assert!(resumed);
        assert_eq!(rest, whole[1000..]);
        assert!(open("http://example.org/", 0).is_err());
    }

    #[test]
    fn a_stopped_download_goes_on_from_where_it_was() {
        let dir = crate::scratch("download-resume");
        let archive = sample_pack(&dir, "sample", "1.0.0");
        let known = known_from(std::slice::from_ref(&archive));
        let store = Store::at(&dir.join("packages"));
        std::fs::create_dir_all(store.downloads()).unwrap();
        // The first 100 bytes arrived last time.
        let bytes = std::fs::read(&archive).unwrap();
        let partial = store.downloads().join(format!("{}.part", known.file_name()));
        std::fs::write(&partial, &bytes[..100]).unwrap();
        let whole = download(&known, &store, &Task::new()).unwrap();
        assert_eq!(std::fs::read(&whole).unwrap(), bytes);
        assert!(!partial.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
