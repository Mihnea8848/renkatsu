// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The packages Renkatsu knows where to download: a small list that
//! ships inside the app, `packages.toml` beside this crate's sources.
//! The files it names are assets of a release of their own, so that an
//! app release does not carry them again.

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

/// One file to download. A package is the parts put end to end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub url: String,
    /// Bytes of the file.
    pub size: u64,
    pub sha256: String,
}

/// A package that can be downloaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Known {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// Bytes the package takes once installed.
    #[serde(default)]
    pub installed_size: u64,
    /// The files of the archive, in order; one unless it was cut.
    #[serde(rename = "part")]
    pub parts: Vec<Part>,
}

impl Known {
    /// Bytes to download.
    pub fn size(&self) -> u64 {
        self.parts.iter().map(|p| p.size).sum()
    }

    /// The name of the archive once it is whole.
    pub fn file_name(&self) -> String {
        format!("{}-{}.{}", self.id, self.version, crate::EXTENSION)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default, rename = "package")]
    packages: Vec<Known>,
}

/// Reads a list of known packages.
pub fn parse(text: &str) -> Result<Vec<Known>> {
    let file: File = toml::from_str(text).context("reading the list of packages")?;
    for package in &file.packages {
        ensure!(
            crate::manifest::valid_id(&package.id),
            "{:?} is not a package id",
            package.id
        );
        ensure!(!package.parts.is_empty(), "{} has nothing to download", package.id);
        for part in &package.parts {
            ensure!(
                part.sha256.len() == 64 && part.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                "{} has a part without a SHA-256",
                package.id
            );
            ensure!(
                part.url.starts_with("https://") || part.url.starts_with("file://"),
                "{} is not downloaded over HTTPS",
                package.id
            );
        }
    }
    Ok(file.packages)
}

/// The list as text, for the builder to print.
pub fn to_text(packages: &[Known]) -> Result<String> {
    Ok(toml::to_string(&File {
        packages: packages.to_vec(),
    })?)
}

/// The packages this build knows where to download.
pub fn known() -> Vec<Known> {
    parse(include_str!("../packages.toml")).expect("the list of packages in the app is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_in_the_app_reads_and_names_starter_content() {
        let packages = known();
        let starter = packages
            .iter()
            .find(|p| p.id == crate::STARTER_CHARACTERS)
            .expect("Starter Content: Characters is listed");
        assert_eq!(starter.name, "Starter Content: Characters");
        assert!(starter.size() > 0);
        // A release file on GitHub stays under 2 GB.
        assert!(starter.parts.iter().all(|p| p.size < 2_000_000_000));
        assert!(starter.parts.iter().all(|p| p.url.starts_with("https://github.com/")));
    }

    #[test]
    fn a_list_reads_back_and_adds_up_its_parts() {
        let sum = "0".repeat(64);
        let package = Known {
            id: "starter-maps".to_owned(),
            name: "Starter Content: Maps".to_owned(),
            version: "1.0.0".to_owned(),
            description: String::new(),
            installed_size: 3000,
            parts: vec![
                Part {
                    url: "https://example.org/maps.rkpkg.001".to_owned(),
                    size: 1000,
                    sha256: sum.clone(),
                },
                Part {
                    url: "https://example.org/maps.rkpkg.002".to_owned(),
                    size: 500,
                    sha256: sum,
                },
            ],
        };
        let text = to_text(std::slice::from_ref(&package)).unwrap();
        assert!(
            text.contains("[[package]]") && text.contains("[[package.part]]"),
            "{text}"
        );
        assert_eq!(parse(&text).unwrap(), std::slice::from_ref(&package));
        assert_eq!(package.size(), 1500);
        assert_eq!(package.file_name(), "starter-maps-1.0.0.rkpkg");
        assert!(parse("").unwrap().is_empty());
    }

    #[test]
    fn a_list_without_hashes_or_over_plain_http_is_refused() {
        let entry = |url: &str, sum: &str| {
            format!(
                "[[package]]\nid = \"a\"\nname = \"A\"\nversion = \"1\"\n[[package.part]]\nurl = \"{url}\"\nsize = 1\nsha256 = \"{sum}\"\n"
            )
        };
        let sum = "a".repeat(64);
        assert!(parse(&entry("https://example.org/a", &sum)).is_ok());
        assert!(parse(&entry("http://example.org/a", &sum)).is_err());
        assert!(parse(&entry("https://example.org/a", "")).is_err());
        assert!(parse("[[package]]\nid = \"a\"\nname = \"A\"\nversion = \"1\"\npart = []\n").is_err());
    }
}
