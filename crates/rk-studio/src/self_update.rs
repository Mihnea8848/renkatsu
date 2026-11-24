// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Install Update: downloads a newer Renkatsu in the background, writes it
//! over the running one and starts it again. With a project open it waits
//! to be restarted, so that nothing unsaved is lost: the restart goes
//! through quitting, which asks to save, and opens the project again.

use anyhow::Result;
use rk_packages::update::Release;
use rk_packages::{Stage, Task};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

pub struct Updating {
    task: Arc<Task>,
    done: Receiver<Result<PathBuf>>,
    /// The new program, once it is written and waits to be started.
    ready: Option<PathBuf>,
    /// Why the update stopped, once it has.
    pub failed: Option<String>,
}

impl Updating {
    pub fn start(release: Release, ctx: &egui::Context) -> Updating {
        let task = Arc::new(Task::new());
        let (send, done) = channel();
        let (work, ctx) = (task.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = send.send(rk_packages::update::install(&release, &work));
            ctx.request_repaint();
        });
        Updating {
            task,
            done,
            ready: None,
            failed: None,
        }
    }

    /// Whether the update is still downloading or installing.
    pub fn busy(&self) -> bool {
        self.failed.is_none() && self.ready.is_none()
    }

    /// The new program, once it waits to be started.
    pub fn ready(&self) -> Option<&Path> {
        self.ready.as_deref()
    }

    /// What the update's line says.
    pub fn text(&self) -> String {
        if self.failed.is_some() {
            return "Update failed, try again".to_owned();
        }
        if self.ready.is_some() {
            return "Restart to update".to_owned();
        }
        match self.task.progress() {
            (Stage::Downloading, done, total) if total > 0 => {
                format!("Downloading {}%", done * 100 / total)
            }
            (Stage::Installing, ..) => "Installing".to_owned(),
            _ => "Getting the update".to_owned(),
        }
    }

    /// Looks whether the work is done. When it is, with `restart` the new
    /// program starts and this one closes; without, it waits for
    /// `restart_into`.
    pub fn poll(&mut self, ctx: &egui::Context, restart: bool) {
        if self.failed.is_some() || self.ready.is_some() {
            return;
        }
        match self.done.try_recv() {
            Ok(Ok(program)) => {
                if restart {
                    match start(&program, None) {
                        Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        Err(e) => self.failed = Some(format!("Renkatsu is updated but did not start again: {e}")),
                    }
                } else {
                    self.ready = Some(program);
                    ctx.request_repaint();
                }
            }
            Ok(Err(e)) => self.failed = Some(format!("{e:#}")),
            Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(250)),
        }
    }
}

/// Starts `program` with this one's arguments, opening `project` when this
/// one was not started on a project.
pub fn start(program: &Path, project: Option<&Path>) -> std::io::Result<()> {
    std::process::Command::new(program)
        .args(arguments(std::env::args_os().skip(1).collect(), project))
        .spawn()
        .map(|_| ())
}

/// The arguments to start the new program with: this one's, and the
/// project open now where they name none.
fn arguments(mut args: Vec<OsString>, project: Option<&Path>) -> Vec<OsString> {
    let named = args.iter().any(|a| a == "--project");
    if let (false, Some(project)) = (named, project) {
        args.push("--project".into());
        args.push(project.into());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restart_opens_the_project_that_was_open() {
        let project = Path::new("/home/a/Renkatsu/After school");
        assert_eq!(
            arguments(Vec::new(), Some(project)),
            vec![
                OsString::from("--project"),
                OsString::from("/home/a/Renkatsu/After school")
            ]
        );
        let started = vec![OsString::from("--project"), OsString::from("/x")];
        assert_eq!(arguments(started.clone(), Some(project)), started);
        assert!(arguments(Vec::new(), None).is_empty());
    }
}
