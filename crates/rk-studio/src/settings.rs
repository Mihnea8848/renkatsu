// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Studio settings that belong to the user rather than a project, kept in a
//! small text file in the user's configuration folder: `~/.config/renkatsu`
//! on Linux (or `$XDG_CONFIG_HOME/renkatsu`), `%APPDATA%\Renkatsu` on Windows.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How often the editor redraws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FrameRate {
    /// Every frame, in step with the display's refresh rate.
    #[default]
    Monitor,
    /// Every frame, as fast as the GPU allows, without waiting for the
    /// display. Mostly for measuring.
    Uncapped,
    /// Only when something changes, to save power.
    PowerSaving,
}

impl FrameRate {
    pub const ALL: [FrameRate; 3] = [FrameRate::Monitor, FrameRate::Uncapped, FrameRate::PowerSaving];

    pub fn label(self) -> &'static str {
        match self {
            FrameRate::Monitor => "Monitor refresh rate",
            FrameRate::Uncapped => "Uncapped",
            FrameRate::PowerSaving => "Power saving",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            FrameRate::Monitor => "Redraws every frame, in step with the display.",
            FrameRate::Uncapped => "Redraws as fast as the GPU allows, without waiting for the display.",
            FrameRate::PowerSaving => "Redraws only when something changes.",
        }
    }

    /// Whether the window waits for the display before showing a frame.
    /// This is chosen when the window opens, so changing it needs a restart.
    pub fn vsync(self) -> bool {
        self != FrameRate::Uncapped
    }

    /// Whether the editor draws a new frame even when nothing changed.
    pub fn continuous(self) -> bool {
        self != FrameRate::PowerSaving
    }
}

/// How many projects the list of recent ones keeps.
const RECENT: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub frame_rate: FrameRate,
    /// The folders of the projects opened last, the latest first.
    pub recent_projects: Vec<PathBuf>,
}

impl Settings {
    /// Puts a project at the head of the recent ones.
    pub fn opened(&mut self, project: &std::path::Path) {
        self.recent_projects.retain(|p| p != project);
        self.recent_projects.insert(0, project.to_owned());
        self.recent_projects.truncate(RECENT);
    }

    /// The project opened last, when it is still where it was.
    pub fn last_project(&self) -> Option<&PathBuf> {
        self.recent_projects
            .first()
            .filter(|p| crate::project::Project::exists(p))
    }

    /// The settings file, or `None` when no configuration folder is known.
    pub fn path() -> Option<PathBuf> {
        let dir = if cfg!(windows) {
            PathBuf::from(std::env::var_os("APPDATA")?).join("Renkatsu")
        } else {
            match std::env::var_os("XDG_CONFIG_HOME") {
                Some(dir) if !dir.is_empty() => PathBuf::from(dir).join("renkatsu"),
                _ => PathBuf::from(std::env::var_os("HOME")?).join(".config/renkatsu"),
            }
        };
        Some(dir.join("studio.toml"))
    }

    /// The saved settings, or the defaults when there are none or the file
    /// can't be read.
    pub fn load() -> Settings {
        Settings::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Settings::path().ok_or_else(|| anyhow::anyhow!("no configuration folder"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, toml::to_string(self)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_read_back_and_missing_keys_take_defaults() {
        let s = Settings {
            frame_rate: FrameRate::PowerSaving,
            recent_projects: vec![PathBuf::from("/home/someone/Documents/Renkatsu/First")],
        };
        let text = toml::to_string(&s).unwrap();
        assert!(text.contains("power-saving"), "{text}");
        assert_eq!(toml::from_str::<Settings>(&text).unwrap(), s);
        assert_eq!(toml::from_str::<Settings>("").unwrap(), Settings::default());
        assert_eq!(Settings::default().frame_rate, FrameRate::Monitor);
    }

    #[test]
    fn recent_projects_are_listed_latest_first_and_once() {
        let mut s = Settings::default();
        for name in ["a", "b", "c", "b"] {
            s.opened(std::path::Path::new(name));
        }
        assert_eq!(
            s.recent_projects,
            [PathBuf::from("b"), PathBuf::from("c"), PathBuf::from("a")]
        );
        for n in 0..20 {
            s.opened(std::path::Path::new(&format!("p{n}")));
        }
        assert_eq!(s.recent_projects.len(), RECENT);
        assert_eq!(s.recent_projects[0], PathBuf::from("p19"));
        // A project that is gone is not opened again.
        assert_eq!(s.last_project(), None);
        let dir = std::env::temp_dir().join(format!("renkatsu-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::project::Project::create(&dir, "Here").unwrap();
        s.opened(&dir);
        assert_eq!(s.last_project(), Some(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_uncapped_skips_vsync_and_only_power_saving_waits_for_changes() {
        assert!(FrameRate::Monitor.vsync() && FrameRate::Monitor.continuous());
        assert!(!FrameRate::Uncapped.vsync() && FrameRate::Uncapped.continuous());
        assert!(FrameRate::PowerSaving.vsync() && !FrameRate::PowerSaving.continuous());
    }
}
