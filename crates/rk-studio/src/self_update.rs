// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Install Update: downloads a newer Renkatsu in the background, writes it
//! over the running one and starts it again.

use anyhow::Result;
use rk_packages::update::Release;
use rk_packages::{Stage, Task};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

pub struct Updating {
    task: Arc<Task>,
    done: Receiver<Result<PathBuf>>,
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
            failed: None,
        }
    }

    /// Whether the update is still under way.
    pub fn busy(&self) -> bool {
        self.failed.is_none()
    }

    /// What the line under "Update available" says while it works.
    pub fn text(&self) -> String {
        if self.failed.is_some() {
            return "Update failed, try again".to_owned();
        }
        match self.task.progress() {
            (Stage::Downloading, done, total) if total > 0 => {
                format!("Downloading {}%", done * 100 / total)
            }
            (Stage::Installing, ..) => "Installing".to_owned(),
            _ => "Getting the update".to_owned(),
        }
    }

    /// Looks whether the work is done. When it is, starts the new program
    /// and closes this one.
    pub fn poll(&mut self, ctx: &egui::Context) {
        if self.failed.is_some() {
            return;
        }
        match self.done.try_recv() {
            Ok(Ok(program)) => {
                match std::process::Command::new(&program)
                    .args(std::env::args_os().skip(1))
                    .spawn()
                {
                    Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    Err(e) => self.failed = Some(format!("Renkatsu is updated but did not start again: {e}")),
                }
            }
            Ok(Err(e)) => self.failed = Some(format!("{e:#}")),
            Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(250)),
        }
    }
}
