// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared configuration for Renkatsu tools.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Where the user's own Koikatsu install lives. Renkatsu ships no game
/// assets; everything is read from this directory at runtime.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub game_dir: PathBuf,
}

impl Config {
    /// Resolution order: `RENKATSU_GAME_DIR`, then `renkatsu.toml` in the
    /// current directory or any parent.
    pub fn load() -> Result<Self> {
        if let Some(dir) = std::env::var_os("RENKATSU_GAME_DIR") {
            return Self::from_dir(PathBuf::from(dir));
        }
        let mut dir = std::env::current_dir()?;
        loop {
            let candidate = dir.join("renkatsu.toml");
            if candidate.is_file() {
                let text =
                    std::fs::read_to_string(&candidate).with_context(|| format!("reading {}", candidate.display()))?;
                let cfg: Config = toml::from_str(&text).with_context(|| format!("parsing {}", candidate.display()))?;
                return Self::from_dir(cfg.game_dir);
            }
            if !dir.pop() {
                break;
            }
        }
        bail!("no game directory configured: set RENKATSU_GAME_DIR or create renkatsu.toml with `game_dir = \"...\"`")
    }

    pub fn from_dir(game_dir: PathBuf) -> Result<Self> {
        if !game_dir.join("abdata").is_dir() {
            bail!(
                "{} does not look like a Koikatsu install (no abdata folder)",
                game_dir.display()
            );
        }
        Ok(Self { game_dir })
    }

    pub fn abdata(&self) -> PathBuf {
        self.game_dir.join("abdata")
    }
    pub fn mods(&self) -> PathBuf {
        self.game_dir.join("mods")
    }
    pub fn userdata(&self) -> PathBuf {
        self.game_dir.join("UserData")
    }
}

/// Convenience for tests: the configured game dir, or `None` so corpus tests
/// can skip on machines without an install.
pub fn test_game_dir() -> Option<PathBuf> {
    Config::load().ok().map(|c| c.game_dir)
}

pub fn walk_files(root: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_files(&p, ext, out);
        } else if p
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case(ext))
        {
            out.push(p);
        }
    }
}
