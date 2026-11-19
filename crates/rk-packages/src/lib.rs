// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Renkatsu packages: content installed once per user and shared by every
//! project, such as Starter Content.
//!
//! A package is a zip archive (`.rkpkg`) with `package.toml`, which says
//! what it is, `checksums.txt`, with the SHA-256 of every file, and its
//! content in folders by kind: `abdata` for game data laid over the
//! game's own, `cards` for character cards, `animations` for Renkatsu
//! animation files in folders by group and category, and `maps`, `poses`
//! (poses made by hand with FK and IK), `items` and `scenes` for what
//! comes later.
//!
//! Installed, a package is that archive unpacked into a folder of the
//! user's data folder named after its id. This crate reads and writes
//! packages, installs, checks and removes them, and downloads the ones
//! the app knows about. It does not know what the content means: the
//! virtual file system reads `abdata`, and the studio imports the rest.

pub mod download;
pub mod hash;
pub mod index;
pub mod manifest;
pub mod news;
pub mod pack;
pub mod payload;
pub mod store;
pub mod update;

pub use index::{known, Known, Part};
pub use manifest::{Manifest, EXTENSION, FORMAT};
pub use store::{Installed, Store};

use anyhow::{bail, Result};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

/// The Renkatsu this crate is part of, which packages are checked against.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The id of the package with the characters of Starter Content.
pub const STARTER_CHARACTERS: &str = "starter-characters";
/// The package that holds CharaStudio's animations, static and moving.
/// Its id dates from when Renkatsu called them poses and stays, since an
/// id names the package on disk for every version of it.
pub const STARTER_POSES: &str = "starter-poses";

/// What a long piece of work is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Waiting,
    Downloading,
    Checking,
    Installing,
    Removing,
}

impl Stage {
    const ALL: [Stage; 5] = [
        Stage::Waiting,
        Stage::Downloading,
        Stage::Checking,
        Stage::Installing,
        Stage::Removing,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Stage::Waiting => "Waiting",
            Stage::Downloading => "Downloading",
            Stage::Checking => "Checking",
            Stage::Installing => "Installing",
            Stage::Removing => "Removing",
        }
    }
}

/// A download, install or check as another thread sees it: how far it
/// is, and a way to stop it. The work reports here and looks here for
/// the word to stop.
#[derive(Debug, Default)]
pub struct Task {
    stage: AtomicU8,
    done: AtomicU64,
    total: AtomicU64,
    cancel: AtomicBool,
}

impl Task {
    pub fn new() -> Task {
        Task::default()
    }

    /// The stage, and the bytes done of the bytes it has to do.
    pub fn progress(&self) -> (Stage, u64, u64) {
        let stage = Stage::ALL[self.stage.load(Ordering::Relaxed) as usize % Stage::ALL.len()];
        (
            stage,
            self.done.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        )
    }

    /// Asks the work to stop; it does at its next step.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Starts a stage that has `total` bytes to do.
    pub fn begin(&self, stage: Stage, total: u64) {
        let at = Stage::ALL.iter().position(|s| *s == stage).unwrap_or(0);
        self.stage.store(at as u8, Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
    }

    /// Counts bytes done, and fails when the work was asked to stop.
    pub(crate) fn add(&self, bytes: u64) -> Result<()> {
        self.done.fetch_add(bytes, Ordering::Relaxed);
        if self.cancelled() {
            bail!("cancelled");
        }
        Ok(())
    }
}

/// A size in bytes as people read it.
pub fn size_text(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= KB * KB * KB {
        format!("{:.2} GB", b / (KB * KB * KB))
    } else if b >= KB * KB {
        format!("{:.0} MB", b / (KB * KB))
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} bytes")
    }
}

#[cfg(test)]
pub(crate) fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rk-packages-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_counts_and_stops_when_asked() {
        let task = Task::new();
        assert_eq!(task.progress(), (Stage::Waiting, 0, 0));
        task.begin(Stage::Downloading, 100);
        task.add(40).unwrap();
        assert_eq!(task.progress(), (Stage::Downloading, 40, 100));
        task.cancel();
        assert!(task.add(1).is_err());
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(size_text(512), "512 bytes");
        assert_eq!(size_text(900 * 1024 * 1024), "900 MB");
        assert_eq!(size_text(3 * 1024 * 1024 * 1024 / 2), "1.50 GB");
    }
}
