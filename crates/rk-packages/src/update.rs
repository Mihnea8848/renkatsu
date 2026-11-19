// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Whether a newer Renkatsu is out: the latest release on GitHub. The
//! releases are asked for together with the news, in `news.rs`.

use crate::manifest::version_key;
use anyhow::{Context, Result};

/// A release of Renkatsu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Its version, without the `v` of the tag.
    pub version: String,
    /// Its page, where the installers are.
    pub url: String,
}

/// Reads one release from the API's answer.
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

/// One file of a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Its SHA-256 in hex, as GitHub gives it.
    pub sha256: String,
}

/// The files of a release, from the API's answer for it. Files without a
/// SHA-256 are left out, as they cannot be checked.
pub fn assets(json: &str) -> Result<Vec<Asset>> {
    let v: serde_json::Value = serde_json::from_str(json).context("reading the release")?;
    let list = v["assets"].as_array().context("the release lists no files")?;
    Ok(list
        .iter()
        .filter_map(|a| {
            Some(Asset {
                name: a["name"].as_str()?.to_owned(),
                url: a["browser_download_url"].as_str()?.to_owned(),
                size: a["size"].as_u64()?,
                sha256: a["digest"].as_str()?.strip_prefix("sha256:")?.to_owned(),
            })
        })
        .collect())
}

/// The end of the name of the file an update is made from on this system:
/// the Linux installer, which carries the programs, or the Windows
/// portable archive.
const OURS: &str = if cfg!(windows) {
    "-windows-x64-portable.zip"
} else {
    "-linux-x86_64-installer"
};

/// The release's file for this system.
pub fn ours(assets: &[Asset]) -> Option<&Asset> {
    assets.iter().find(|a| a.name.ends_with(OURS))
}

/// The files to install from a downloaded update: the payload of an
/// installer, or a zip whose files all sit in one folder, taken out of it.
pub fn files_of(bytes: &[u8]) -> Result<Vec<crate::payload::File>> {
    let zip = crate::payload::find(bytes).unwrap_or(bytes);
    let mut files = crate::payload::files(zip)?;
    let top = files
        .first()
        .and_then(|f| f.path.split_once('/'))
        .map(|(top, _)| format!("{top}/"));
    if let Some(top) = top.filter(|top| files.iter().all(|f| f.path.starts_with(top.as_str()))) {
        for file in &mut files {
            file.path = file.path[top.len()..].to_owned();
        }
    }
    anyhow::ensure!(
        files.iter().any(|f| f.path == program_name()),
        "the update holds no {}",
        program_name()
    );
    Ok(files)
}

fn program_name() -> &'static str {
    if cfg!(windows) {
        "renkatsu.exe"
    } else {
        "renkatsu"
    }
}

/// Downloads a release and writes its programs over the running one's,
/// in the folder it runs from. Gives the program to start again.
pub fn install(release: &Release, task: &crate::Task) -> Result<std::path::PathBuf> {
    let program = std::env::current_exe().context("finding the running program")?;
    let dir = program.parent().context("finding the program's folder")?;
    // A copy built from source is updated with its source.
    if dir.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "target") {
        anyhow::bail!("this Renkatsu was built from source; update it with git");
    }
    install_into(release, task, dir)
}

/// Downloads a release and writes its files into `dir`. Gives the program.
pub fn install_into(release: &Release, task: &crate::Task, dir: &std::path::Path) -> Result<std::path::PathBuf> {
    let url = format!("{}/tags/v{}", RELEASE_BASE, release.version);
    let json = match crate::news::get(&url, None, 1 << 20)? {
        crate::news::Answer::New(text, _) => text,
        crate::news::Answer::Same => anyhow::bail!("{url} gave nothing"),
    };
    let assets = assets(&json)?;
    let asset =
        ours(&assets).with_context(|| format!("Renkatsu {} has no download for this system", release.version))?;
    let download = std::env::temp_dir().join(&asset.name);
    task.begin(crate::Stage::Downloading, asset.size);
    let part = crate::index::Part {
        url: asset.url.clone(),
        size: asset.size,
        sha256: asset.sha256.clone(),
    };
    crate::download::part(&part, &download, task)?;
    let bytes = std::fs::read(&download).with_context(|| format!("reading {}", download.display()))?;
    let files = files_of(&bytes)?;
    task.begin(crate::Stage::Installing, files.len() as u64);
    for file in &files {
        crate::payload::write(dir, file)?;
        task.add(1)?;
    }
    let _ = std::fs::remove_file(&download);
    Ok(dir.join(program_name()))
}

/// The address of a release by its tag, below this.
const RELEASE_BASE: &str = "https://api.github.com/repos/Mihnea8848/renkatsu/releases";

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

    #[test]
    fn a_release_gives_the_file_for_this_system_and_its_programs() {
        let json = format!(
            r#"{{"assets": [
                {{"name": "Renkatsu-0.2.0{OURS}", "browser_download_url": "https://example.org/a", "size": 5, "digest": "sha256:ab"}},
                {{"name": "notes.txt", "browser_download_url": "https://example.org/b", "size": 1}}
            ]}}"#
        );
        let list = assets(&json).unwrap();
        assert_eq!(list.len(), 1, "a file without a checksum is left out");
        assert_eq!(ours(&list).unwrap().sha256, "ab");

        // A portable archive keeps its files in one folder; they come out.
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for name in [program_name(), "README.md"] {
            zip.start_file(
                format!("Renkatsu-0.2.0/{name}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            std::io::Write::write_all(&mut zip, b"x").unwrap();
        }
        let zip = zip.finish().unwrap().into_inner();
        let names: Vec<String> = files_of(&zip).unwrap().into_iter().map(|f| f.path).collect();
        assert_eq!(names, [program_name(), "README.md"]);
        // So does an installer's payload, whose files sit at the top.
        let installer = crate::payload::append(b"program", &zip);
        assert_eq!(files_of(&installer).unwrap().len(), 2);
        // A file without the program is no update.
        assert!(files_of(&crate::payload::append(b"p", &zip_of_readme())).is_err());
    }

    /// Installs the published 0.1.8 into a folder, over the network.
    #[test]
    #[ignore = "downloads a release from GitHub"]
    fn a_published_release_installs_into_a_folder() {
        let dir = std::env::temp_dir().join(format!("rk-update-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(program_name()), b"old").unwrap();
        let release = Release {
            version: "0.1.8".to_owned(),
            url: String::new(),
        };
        let program = install_into(&release, &crate::Task::new(), &dir).unwrap();
        assert!(std::fs::metadata(&program).unwrap().len() > 1_000_000);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn zip_of_readme() -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("README.md", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.finish().unwrap().into_inner()
    }
}
