// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! What a package says about itself: `package.toml` at the root of the
//! archive and of the installed folder.

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// The package layout this build reads and writes.
pub const FORMAT: u32 = 1;

/// The file extension of a package archive.
pub const EXTENSION: &str = "rkpkg";

/// The file with the SHA-256 of every other file of the package.
pub const CHECKSUMS: &str = "checksums.txt";

/// The folders of a package, by the kind of content they hold. `abdata`
/// is laid over the game's folder of that name; the others hold files a
/// project imports, as it imports them from disk.
pub mod folder {
    /// Game data: bundles under the paths the game has them at.
    pub const ABDATA: &str = "abdata";
    /// Character cards, in `female` and `male`.
    pub const CARDS: &str = "cards";
    pub const MAPS: &str = "maps";
    pub const ANIMATIONS: &str = "animations";
    pub const POSES: &str = "poses";
    pub const ITEMS: &str = "items";
    pub const SCENES: &str = "scenes";
    /// Zipmods, read with Sideloader's rules as the game's own are.
    /// Packages with mods need Renkatsu [`MODS_SINCE`] or newer.
    pub const MODS: &str = "mods";
    /// Every folder a package may have at its root.
    pub const ALL: [&str; 8] = [ABDATA, CARDS, MAPS, ANIMATIONS, POSES, ITEMS, SCENES, MODS];
    /// The first Renkatsu that reads a package's `mods` folder.
    pub const MODS_SINCE: &str = "0.1.4";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// The layout of the package; see [`FORMAT`].
    pub format: u32,
    /// The name the package is installed and listed under: lower case
    /// letters, digits and dashes.
    pub id: String,
    /// The name people read.
    pub name: String,
    pub version: String,
    /// The family the package belongs to, such as `starter-content`.
    pub kind: String,
    #[serde(default)]
    pub description: String,
    /// The oldest Renkatsu that can use the package.
    pub requires_renkatsu: String,
    /// Where the content comes from, for the user to read.
    #[serde(default)]
    pub source: String,
    /// The card new projects start with, relative to the package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_card: Option<String>,
    /// How many things of each kind the package holds, by the kind's
    /// name: `cards`, `bundles`, and later `maps`, `animations`, `poses`
    /// and `items`.
    #[serde(default)]
    pub contents: BTreeMap<String, u64>,
}

/// A version as numbers: `1.10.2` is `[1, 10, 2]`.
pub fn version_key(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// Whether an id can name a folder: lower case letters, digits and dashes.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest> {
        let manifest: Manifest = toml::from_str(text).context("reading package.toml")?;
        manifest.check()?;
        Ok(manifest)
    }

    pub fn read(file: &Path) -> Result<Manifest> {
        let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
        Self::parse(&text).with_context(|| format!("in {}", file.display()))
    }

    pub fn to_text(&self) -> Result<String> {
        Ok(toml::to_string(self)?)
    }

    /// Refuses a manifest this build cannot use, whatever Renkatsu it asks for.
    pub fn check(&self) -> Result<()> {
        ensure!(
            self.format == FORMAT,
            "the package has format {}, this Renkatsu reads format {FORMAT}",
            self.format
        );
        ensure!(
            valid_id(&self.id),
            "the package id {:?} is not made of lower case letters, digits and dashes",
            self.id
        );
        ensure!(!self.name.trim().is_empty(), "the package has no name");
        ensure!(!version_key(&self.version).is_empty(), "the package has no version");
        Ok(())
    }

    /// Refuses a package made for a newer Renkatsu than `app`.
    pub fn check_app(&self, app: &str) -> Result<()> {
        if version_key(&self.requires_renkatsu) > version_key(app) {
            bail!(
                "{} {} needs Renkatsu {} or newer, this is {app}",
                self.name,
                self.version,
                self.requires_renkatsu
            );
        }
        Ok(())
    }

    /// One line on what the package holds: "38 cards, 612 bundles".
    pub fn holds(&self) -> String {
        let parts: Vec<String> = self
            .contents
            .iter()
            .filter(|(_, n)| **n > 0)
            .map(|(kind, n)| {
                let kind = if *n == 1 { kind.trim_end_matches('s') } else { kind };
                format!("{n} {kind}")
            })
            .collect();
        parts.join(", ")
    }
}

/// Whether a path inside a package is one it may have: relative, with
/// forward slashes, no `..`, and either a file of the package itself or
/// under one of its folders.
pub fn valid_path(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains(':') {
        return false;
    }
    if path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return false;
    }
    match path.split_once('/') {
        None => path == rk_core::PACKAGE_FILE || path == CHECKSUMS,
        Some((top, _)) => folder::ALL.contains(&top),
    }
}

#[cfg(test)]
pub(crate) fn sample(id: &str, version: &str) -> Manifest {
    Manifest {
        format: FORMAT,
        id: id.to_owned(),
        name: "Sample".to_owned(),
        version: version.to_owned(),
        kind: "starter-content".to_owned(),
        description: "A package for tests".to_owned(),
        requires_renkatsu: "0.1.0".to_owned(),
        source: String::new(),
        default_card: None,
        contents: BTreeMap::from([("cards".to_owned(), 1), ("bundles".to_owned(), 2)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_is_readable_text_and_reads_back() {
        let mut manifest = sample("starter-characters", "1.0.0");
        manifest.default_card = Some("cards/female/Default.png".to_owned());
        let text = manifest.to_text().unwrap();
        assert!(text.contains("id = \"starter-characters\""), "{text}");
        assert!(text.contains("[contents]"), "{text}");
        assert_eq!(Manifest::parse(&text).unwrap(), manifest);
        assert_eq!(manifest.holds(), "2 bundles, 1 card");
    }

    #[test]
    fn a_manifest_without_optional_keys_is_read() {
        let text = "format = 1\nid = \"maps\"\nname = \"Maps\"\nversion = \"2.1\"\nkind = \"starter-content\"\nrequires_renkatsu = \"0.2.0\"\n";
        let manifest = Manifest::parse(text).unwrap();
        assert!(manifest.contents.is_empty());
        assert_eq!(manifest.default_card, None);
        assert_eq!(manifest.holds(), "");
    }

    #[test]
    fn bad_manifests_are_refused() {
        let mut other_format = sample("a", "1");
        other_format.format = 2;
        assert!(other_format.check().is_err());
        assert!(sample("Has Spaces", "1").check().is_err());
        assert!(sample("../up", "1").check().is_err());
        assert!(sample("a", "").check().is_err());
        assert!(Manifest::parse("not toml at all").is_err());
    }

    #[test]
    fn a_package_for_a_newer_renkatsu_is_refused() {
        let mut manifest = sample("a", "1");
        manifest.requires_renkatsu = "0.2.0".to_owned();
        assert!(manifest.check_app("0.1.9").is_err());
        assert!(manifest.check_app("0.2.0").is_ok());
        assert!(manifest.check_app("0.10.0").is_ok());
    }

    #[test]
    fn paths_stay_inside_the_packages_folders() {
        for good in [
            "package.toml",
            "checksums.txt",
            "abdata/chara/oo_base.unity3d",
            "cards/female/Default.png",
            "maps/school.toml",
        ] {
            assert!(valid_path(good), "{good}");
        }
        for bad in [
            "",
            "/etc/passwd",
            "../outside",
            "abdata/../../outside",
            "abdata\\chara",
            "C:/windows",
            "other/file",
            "loose.txt",
            "abdata//x",
        ] {
            assert!(!valid_path(bad), "{bad}");
        }
    }
}
