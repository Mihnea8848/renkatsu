// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Shared configuration for Renkatsu tools.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Where the user's own Koikatsu install lives. Renkatsu ships no game
/// assets; everything is read from this directory at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub game_dir: PathBuf,
}

impl Config {
    /// Resolution order: `RENKATSU_GAME_DIR`, then `renkatsu.toml` in the
    /// current directory or any parent, then `renkatsu.toml` in the user's
    /// configuration folder (`user_file`), which the studio writes.
    pub fn load() -> Result<Self> {
        if let Some(dir) = std::env::var_os("RENKATSU_GAME_DIR") {
            return Self::from_dir(PathBuf::from(dir));
        }
        let mut dir = std::env::current_dir()?;
        loop {
            let candidate = dir.join("renkatsu.toml");
            if candidate.is_file() {
                return Self::read(&candidate);
            }
            if !dir.pop() {
                break;
            }
        }
        if let Some(file) = Self::user_file().filter(|f| f.is_file()) {
            return Self::read(&file);
        }
        bail!(
            "no game directory configured: choose the Koikatsu folder in Renkatsu Studio, set RENKATSU_GAME_DIR, \
             or create renkatsu.toml with `game_dir = \"...\"`"
        )
    }

    fn read(file: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
        let cfg: Config = toml::from_str(&text).with_context(|| format!("parsing {}", file.display()))?;
        Self::from_dir(cfg.game_dir)
    }

    /// The user's own `renkatsu.toml`: `~/.config/renkatsu` on Linux (or
    /// `$XDG_CONFIG_HOME/renkatsu`), `%APPDATA%\Renkatsu` on Windows. `None`
    /// when no configuration folder is known.
    pub fn user_file() -> Option<PathBuf> {
        let dir = if cfg!(windows) {
            PathBuf::from(std::env::var_os("APPDATA")?).join("Renkatsu")
        } else {
            match std::env::var_os("XDG_CONFIG_HOME") {
                Some(dir) if !dir.is_empty() => PathBuf::from(dir).join("renkatsu"),
                _ => PathBuf::from(std::env::var_os("HOME")?).join(".config/renkatsu"),
            }
        };
        Some(dir.join("renkatsu.toml"))
    }

    /// Checks `game_dir` and writes it to the user's `renkatsu.toml`, where
    /// every Renkatsu tool finds it from then on.
    pub fn save_user(game_dir: &Path) -> Result<Self> {
        let cfg = Self::from_dir(game_dir.to_owned())?;
        let file = Self::user_file().context("no configuration folder is known")?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        std::fs::write(&file, toml::to_string(&cfg)?).with_context(|| format!("writing {}", file.display()))?;
        Ok(cfg)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_config_file_names_a_folder_that_must_hold_abdata() {
        let dir = std::env::temp_dir().join(format!("rk-core-config-{}", std::process::id()));
        let game = dir.join("Koikatsu");
        std::fs::create_dir_all(&game).unwrap();
        let file = dir.join("renkatsu.toml");
        std::fs::write(&file, toml::to_string(&Config { game_dir: game.clone() }).unwrap()).unwrap();
        assert!(Config::read(&file).is_err(), "no abdata yet");
        std::fs::create_dir_all(game.join("abdata")).unwrap();
        assert_eq!(Config::read(&file).unwrap().game_dir, game);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
