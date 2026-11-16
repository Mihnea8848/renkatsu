// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! SHA-256 of files and streams, as lower case hex, and the checksum
//! list of a package.

use crate::Task;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

/// A SHA-256 being worked out.
#[derive(Default)]
pub struct Hasher(Sha256);

impl Hasher {
    pub fn new() -> Hasher {
        Hasher::default()
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    /// The hash as 64 lower case hex digits.
    pub fn hex(self) -> String {
        self.0.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Hasher::new();
    hasher.update(bytes);
    hasher.hex()
}

/// The SHA-256 of what a reader gives, and how many bytes that was. The
/// task counts the bytes and can stop the work.
pub fn sha256_reader(mut reader: impl Read, task: Option<&Task>) -> Result<(String, u64)> {
    let mut hasher = Hasher::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut size = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
        if let Some(task) = task {
            task.add(n as u64)?;
        }
    }
    Ok((hasher.hex(), size))
}

pub fn sha256_file(file: &Path, task: Option<&Task>) -> Result<(String, u64)> {
    let f = std::fs::File::open(file).with_context(|| format!("opening {}", file.display()))?;
    sha256_reader(f, task).with_context(|| format!("reading {}", file.display()))
}

/// Whether two hashes are the same, whatever their case.
pub fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// The checksum list as text: a hash, two spaces and a path per line, in
/// path order, as `sha256sum` writes and checks it.
pub fn checksums_text(sums: &BTreeMap<String, String>) -> String {
    sums.iter().map(|(path, sum)| format!("{sum}  {path}\n")).collect()
}

/// Reads a checksum list: path to hash.
pub fn parse_checksums(text: &str) -> Result<BTreeMap<String, String>> {
    let mut sums = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let (sum, path) = line
            .split_once("  ")
            .with_context(|| format!("checksum line without a path: {line}"))?;
        anyhow::ensure!(
            sum.len() == 64 && sum.chars().all(|c| c.is_ascii_hexdigit()),
            "not a SHA-256: {sum}"
        );
        sums.insert(path.to_owned(), sum.to_ascii_lowercase());
    }
    Ok(sums)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_match_the_known_values() {
        assert_eq!(
            sha256_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let dir = crate::scratch("hash");
        let file = dir.join("abc.txt");
        std::fs::write(&file, b"abc").unwrap();
        let (sum, size) = sha256_file(&file, None).unwrap();
        assert_eq!(size, 3);
        assert!(same(
            &sum,
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
        ));
        assert!(!same(&sum, &sha256_bytes(b"abd")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_checksum_list_reads_back() {
        let sums = BTreeMap::from([
            ("package.toml".to_owned(), sha256_bytes(b"a")),
            ("cards/female/A B.png".to_owned(), sha256_bytes(b"b")),
        ]);
        let text = checksums_text(&sums);
        assert!(text.starts_with(&format!("{}  cards/female/A B.png\n", sha256_bytes(b"b"))));
        assert_eq!(parse_checksums(&text).unwrap(), sums);
        assert!(parse_checksums("short  file\n").is_err());
        assert!(parse_checksums("nospace\n").is_err());
    }
}
