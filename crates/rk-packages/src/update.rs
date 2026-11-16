// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Whether a newer Renkatsu is out: the latest release on GitHub, read
//! from its public API. Nothing about the user is sent; the request only
//! names the program and its version, as every download does.

use crate::manifest::version_key;
use anyhow::{Context, Result};
use std::io::Read;

/// The latest release of the repository on GitHub. Releases of content,
/// such as Starter Content, are not marked as the latest.
pub const LATEST: &str = "https://api.github.com/repos/Mihnea8848/renkatsu/releases/latest";

/// A release of Renkatsu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Its version, without the `v` of the tag.
    pub version: String,
    /// Its page, where the installers are.
    pub url: String,
}

/// Reads a release from the API's answer.
pub fn parse(json: &str) -> Result<Release> {
    let v: serde_json::Value = serde_json::from_str(json).context("reading the release")?;
    let tag = v["tag_name"].as_str().context("the release has no tag")?;
    Ok(Release {
        version: tag.trim_start_matches('v').to_owned(),
        url: v["html_url"].as_str().unwrap_or_default().to_owned(),
    })
}

/// The latest release when it is newer than `running`, the version of the
/// program that asks.
pub fn newer(release: Release, running: &str) -> Option<Release> {
    (version_key(&release.version) > version_key(running)).then_some(release)
}

/// Asks GitHub for the latest release.
pub fn latest() -> Result<Release> {
    let mut answer = crate::download::agent()
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .call()
        .context("asking GitHub for the latest release")?;
    let mut text = String::new();
    answer.body_mut().as_reader().take(1 << 20).read_to_string(&mut text)?;
    parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_release_is_told_and_the_same_or_older_is_not() {
        let release = parse(r#"{"tag_name": "v0.1.10", "html_url": "https://example.org/r", "draft": false}"#).unwrap();
        assert_eq!(release.version, "0.1.10");
        assert_eq!(newer(release.clone(), "0.1.9"), Some(release.clone()));
        assert_eq!(newer(release.clone(), "0.1.10"), None);
        assert_eq!(newer(release, "0.2.0"), None);
        assert!(parse("{}").is_err());
        assert!(parse("not json").is_err());
    }
}
