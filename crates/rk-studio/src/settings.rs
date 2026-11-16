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

/// How the project selector lays out the recent projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectView {
    /// Tiles with large pictures.
    #[default]
    Grid,
    /// Rows with the details in columns.
    List,
}

/// How many projects the list of recent ones keeps.
const RECENT: usize = 10;

/// A project in the list of recent ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RecentText")]
pub struct Recent {
    /// The project folder.
    pub path: PathBuf,
    /// When it was last opened, in seconds since 1970; 0 when not known.
    pub opened: u64,
}

/// A recent project as the settings file may hold it: the folder alone, as
/// the first builds wrote it, or with the time.
#[derive(Deserialize)]
#[serde(untagged)]
enum RecentText {
    Path(PathBuf),
    Entry {
        path: PathBuf,
        #[serde(default)]
        opened: u64,
    },
}

impl From<RecentText> for Recent {
    fn from(text: RecentText) -> Recent {
        match text {
            RecentText::Path(path) => Recent { path, opened: 0 },
            RecentText::Entry { path, opened } => Recent { path, opened },
        }
    }
}

/// Now, in seconds since 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub frame_rate: FrameRate,
    /// Start on the project selector rather than in the last project.
    pub show_project_selector: bool,
    /// The view of the recent projects the selector was left in.
    pub project_view: ProjectView,
    /// The projects opened last, the latest first.
    pub recent_projects: Vec<Recent>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            frame_rate: FrameRate::default(),
            show_project_selector: true,
            project_view: ProjectView::default(),
            recent_projects: Vec::new(),
        }
    }
}

impl Settings {
    /// Puts a project at the head of the recent ones, as opened now.
    pub fn opened(&mut self, project: &std::path::Path) {
        self.opened_at(project, now());
    }

    /// Puts a project at the head of the recent ones, as opened at `time`.
    pub fn opened_at(&mut self, project: &std::path::Path, time: u64) {
        self.forget(project);
        self.recent_projects.insert(
            0,
            Recent {
                path: project.to_owned(),
                opened: time,
            },
        );
        self.recent_projects.truncate(RECENT);
    }

    /// Takes a project off the list of recent ones. Its files stay.
    pub fn forget(&mut self, project: &std::path::Path) {
        self.recent_projects.retain(|r| r.path != project);
    }

    /// The project opened last, when it is still where it was.
    pub fn last_project(&self) -> Option<&PathBuf> {
        self.recent_projects
            .first()
            .map(|r| &r.path)
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

    fn paths(s: &Settings) -> Vec<PathBuf> {
        s.recent_projects.iter().map(|r| r.path.clone()).collect()
    }

    #[test]
    fn settings_read_back_and_missing_keys_take_defaults() {
        let mut s = Settings {
            frame_rate: FrameRate::PowerSaving,
            show_project_selector: false,
            project_view: ProjectView::List,
            recent_projects: Vec::new(),
        };
        s.opened_at(
            std::path::Path::new("/home/someone/Documents/Renkatsu/First"),
            1_700_000_000,
        );
        let text = toml::to_string(&s).unwrap();
        assert!(text.contains("power-saving"), "{text}");
        assert!(text.contains("show_project_selector = false"), "{text}");
        assert!(text.contains("project_view = \"list\""), "{text}");
        // The selector starts on the tiles until the user chooses the list.
        assert_eq!(Settings::default().project_view, ProjectView::Grid);
        assert!(text.contains("opened = 1700000000"), "{text}");
        assert_eq!(toml::from_str::<Settings>(&text).unwrap(), s);
        assert_eq!(toml::from_str::<Settings>("").unwrap(), Settings::default());
        assert_eq!(Settings::default().frame_rate, FrameRate::Monitor);
        // The selector shows at the start unless the user turned it off.
        assert!(Settings::default().show_project_selector);
        assert!(
            toml::from_str::<Settings>("frame_rate = \"monitor\"")
                .unwrap()
                .show_project_selector
        );
    }

    #[test]
    fn a_list_of_folders_alone_is_still_read() {
        let old: Settings = toml::from_str("recent_projects = [\"/a/First\", \"/a/Second\"]").unwrap();
        assert_eq!(paths(&old), [PathBuf::from("/a/First"), PathBuf::from("/a/Second")]);
        assert!(old.recent_projects.iter().all(|r| r.opened == 0));
    }

    #[test]
    fn recent_projects_are_listed_latest_first_and_once() {
        let mut s = Settings::default();
        for (time, name) in ["a", "b", "c", "b"].into_iter().enumerate() {
            s.opened_at(std::path::Path::new(name), time as u64);
        }
        assert_eq!(paths(&s), [PathBuf::from("b"), PathBuf::from("c"), PathBuf::from("a")]);
        assert_eq!(s.recent_projects[0].opened, 3);
        s.forget(std::path::Path::new("c"));
        assert_eq!(paths(&s), [PathBuf::from("b"), PathBuf::from("a")]);
        for n in 0..20 {
            s.opened(std::path::Path::new(&format!("p{n}")));
        }
        assert_eq!(s.recent_projects.len(), RECENT);
        assert_eq!(s.recent_projects[0].path, PathBuf::from("p19"));
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
