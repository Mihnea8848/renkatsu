// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Where Renkatsu goes on Linux and how it is put there: for the user who
//! runs the installer, with nothing outside the home folder and no root.
//!
//! ```text
//! ~/.local/share/renkatsu/app/         the programs, icon and licences
//! ~/.local/bin/renkatsu, rk             links to the programs
//! ~/.local/share/applications/          the application menu entry
//! ~/.local/share/icons/hicolor/...      the icon
//! ~/Desktop/renkatsu.desktop            the desktop shortcut, if chosen
//! ```

use anyhow::{Context, Result};
use rk_packages::payload::{self, File};
use std::path::{Path, PathBuf};

/// The folders an installation uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub app: PathBuf,
    pub bin: PathBuf,
    pub applications: PathBuf,
    pub icons: PathBuf,
    pub desktop: Option<PathBuf>,
}

/// What the user chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub menu: bool,
    pub desktop: bool,
}

impl Layout {
    /// The folders of the user who runs the installer.
    pub fn user() -> Option<Layout> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let data = match std::env::var_os("XDG_DATA_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => home.join(".local/share"),
        };
        let config = match std::env::var_os("XDG_CONFIG_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => home.join(".config"),
        };
        let desktop = std::fs::read_to_string(config.join("user-dirs.dirs"))
            .ok()
            .and_then(|text| desktop_of(&text, &home))
            .unwrap_or_else(|| home.join("Desktop"));
        Some(Layout::under(&home, &data, Some(desktop)))
    }

    pub fn under(home: &Path, data: &Path, desktop: Option<PathBuf>) -> Layout {
        Layout {
            app: data.join("renkatsu/app"),
            bin: home.join(".local/bin"),
            applications: data.join("applications"),
            icons: data.join("icons/hicolor/256x256/apps"),
            desktop,
        }
    }

    /// Whether Renkatsu is installed here already.
    pub fn installed(&self) -> bool {
        self.app.join("renkatsu").is_file()
    }

    fn entry(&self) -> PathBuf {
        self.applications.join("renkatsu.desktop")
    }
}

/// The desktop folder named by the desktop's `user-dirs.dirs` file.
fn desktop_of(text: &str, home: &Path) -> Option<PathBuf> {
    let value = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("XDG_DESKTOP_DIR="))?
        .trim()
        .trim_matches('"');
    let path = match value.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(value),
    };
    (path.is_absolute() && path != home).then_some(path)
}

/// The menu entry, which starts the installed program.
pub fn desktop_entry(program: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Renkatsu Studio\nGenericName=Scene editor for Koikatsu characters\n\
         Comment=Pose and render Koikatsu characters\nExec=\"{}\"\nIcon=renkatsu\nTerminal=false\n\
         Categories=Graphics;3DGraphics;\nStartupWMClass=renkatsu\n",
        program.display()
    )
}

/// A script that takes the installation away again, kept with the
/// programs. Projects, settings and packages stay.
pub fn uninstall_script(layout: &Layout) -> String {
    let mut lines = vec![
        "#!/bin/sh".to_owned(),
        "# Removes Renkatsu. Projects, settings and packages are kept.".to_owned(),
        format!(
            "rm -f '{}/renkatsu' '{}/rk'",
            layout.bin.display(),
            layout.bin.display()
        ),
        format!("rm -f '{}'", layout.entry().display()),
        format!("rm -f '{}/renkatsu.png'", layout.icons.display()),
    ];
    if let Some(desktop) = &layout.desktop {
        lines.push(format!("rm -f '{}/renkatsu.desktop'", desktop.display()));
    }
    lines.push(format!("rm -rf '{}'", layout.app.display()));
    lines.push("echo \"Renkatsu is uninstalled. Projects, settings and packages were kept.\"".to_owned());
    lines.join("\n") + "\n"
}

/// Puts the payload's files in place. `step` hears each thing done.
pub fn install(files: &[File], layout: &Layout, options: Options, step: &mut dyn FnMut(&str)) -> Result<()> {
    step("Copying the programs");
    for file in files {
        payload::write(&layout.app, file)?;
    }
    step("Linking the programs");
    std::fs::create_dir_all(&layout.bin).with_context(|| format!("creating {}", layout.bin.display()))?;
    for name in ["renkatsu", "rk"] {
        let target = layout.app.join(name);
        if !target.is_file() {
            continue;
        }
        let link = layout.bin.join(name);
        let _ = std::fs::remove_file(&link);
        link_or_copy(&target, &link)?;
    }
    if options.menu {
        step("Adding Renkatsu Studio to the application menu");
        std::fs::create_dir_all(&layout.applications)?;
        std::fs::write(layout.entry(), desktop_entry(&layout.app.join("renkatsu")))?;
        let icon = layout.app.join("renkatsu.png");
        if icon.is_file() {
            std::fs::create_dir_all(&layout.icons)?;
            std::fs::copy(&icon, layout.icons.join("renkatsu.png"))?;
        }
    }
    if let (true, Some(desktop)) = (options.desktop, &layout.desktop) {
        step("Adding the desktop shortcut");
        std::fs::create_dir_all(desktop)?;
        let shortcut = desktop.join("renkatsu.desktop");
        std::fs::write(&shortcut, desktop_entry(&layout.app.join("renkatsu")))?;
        make_executable(&shortcut)?;
    }
    let script = layout.app.join("uninstall.sh");
    std::fs::write(&script, uninstall_script(layout))?;
    make_executable(&script)?;
    Ok(())
}

#[cfg(unix)]
fn link_or_copy(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link).with_context(|| format!("linking {}", link.display()))
}

#[cfg(not(unix))]
fn link_or_copy(target: &Path, link: &Path) -> Result<()> {
    std::fs::copy(target, link)
        .map(|_| ())
        .with_context(|| format!("copying {}", link.display()))
}

#[cfg(unix)]
fn make_executable(file: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The installer is for Linux: file modes and desktop paths are Unix ones.
    #[cfg(unix)]
    #[test]
    fn the_desktop_folder_comes_from_the_desktop_settings() {
        let home = Path::new("/home/someone");
        let text = "XDG_DESKTOP_DIR=\"$HOME/Schreibtisch\"\nXDG_DOCUMENTS_DIR=\"$HOME/Documents\"\n";
        assert_eq!(desktop_of(text, home), Some(home.join("Schreibtisch")));
        assert_eq!(desktop_of("XDG_DESKTOP_DIR=\"$HOME/\"", home), None);
        assert_eq!(desktop_of("", home), None);
    }

    #[cfg(unix)]
    #[test]
    fn an_installation_puts_everything_in_place_and_the_script_takes_it_away() {
        let root = std::env::temp_dir().join(format!("rk-installer-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let layout = Layout::under(&root, &root.join("data"), Some(root.join("Desktop")));
        let file = |path: &str, executable: bool| File {
            path: path.to_owned(),
            bytes: path.as_bytes().to_vec(),
            executable,
        };
        let files = [
            file("renkatsu", true),
            file("rk", true),
            file("renkatsu.png", false),
            file("LICENSES/OFL-1.1.txt", false),
        ];
        let mut steps = Vec::new();
        let options = Options {
            menu: true,
            desktop: true,
        };
        install(&files, &layout, options, &mut |s| steps.push(s.to_owned())).unwrap();
        assert!(layout.installed());
        assert_eq!(
            std::fs::read_link(layout.bin.join("rk")).unwrap(),
            layout.app.join("rk")
        );
        let entry = std::fs::read_to_string(layout.applications.join("renkatsu.desktop")).unwrap();
        assert!(
            entry.contains(&format!("Exec=\"{}\"", layout.app.join("renkatsu").display())),
            "{entry}"
        );
        assert!(layout.icons.join("renkatsu.png").is_file());
        assert!(root.join("Desktop/renkatsu.desktop").is_file());
        assert_eq!(steps.len(), 4);

        let status = std::process::Command::new("sh")
            .arg(layout.app.join("uninstall.sh"))
            .output()
            .unwrap();
        assert!(status.status.success());
        assert!(!layout.app.exists() && !layout.bin.join("renkatsu").exists());
        assert!(!layout.applications.join("renkatsu.desktop").exists());
        assert!(!root.join("Desktop/renkatsu.desktop").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
