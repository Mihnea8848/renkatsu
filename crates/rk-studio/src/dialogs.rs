// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The questions the editor asks before it goes on: whether to copy a
//! card's mods into the project, what to do with unsaved changes, and
//! where a new project goes. One is open at a time, over the dimmed
//! window. The system's own file dialogs, which pick the cards and
//! folders, are here too.

use crate::mods::{count_text, size_text, Plan};
use crate::project;
use crate::state::{Look, State};
use egui::{Align, Layout, RichText, Ui};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};

/// A card on its way into the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// The card, anywhere on disk.
    pub path: PathBuf,
    /// Put its character in the scene once it is in the project.
    pub place: bool,
    /// Ask before copying mods, unless the project says not to.
    pub ask: bool,
    /// The name it goes by in the project; the card's own when `None`.
    pub name: Option<String>,
}

/// An import that waits for the user's word on its mods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingImport {
    pub import: Import,
    /// The name the character will show under.
    pub title: String,
    pub plan: Plan,
    /// GUIDs the card names that are not installed.
    pub missing: Vec<String>,
    pub dont_ask: bool,
}

/// What was asked for when unsaved changes stood in the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum After {
    New,
    /// Open a project: this one, or the one the user picks.
    Open(Option<PathBuf>),
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    Import(PendingImport),
    Unsaved(After),
    /// The name and place of a new project, or of a copy of this one.
    Project {
        save_as: bool,
        name: String,
        folder: PathBuf,
        problem: Option<String>,
    },
}

/// What a system file dialog was opened for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Card,
    Project,
    /// The folder a new project goes into.
    Folder,
}

/// A system file dialog that is open. It runs on a thread of its own, so
/// the window keeps drawing while the user chooses.
pub struct Picker {
    pub what: Pick,
    answer: Receiver<Option<PathBuf>>,
}

impl Picker {
    /// Opens the dialog, starting in `start` when that folder exists.
    pub fn open(what: Pick, start: Option<PathBuf>, ctx: &egui::Context) -> Picker {
        let (send, answer) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new().set_title(match what {
                Pick::Card => "Import card",
                Pick::Project => "Open project",
                Pick::Folder => "Project location",
            });
            if let Some(dir) = start.filter(|d| d.is_dir()) {
                dialog = dialog.set_directory(dir);
            }
            let picked = match what {
                Pick::Card => dialog.add_filter("Koikatsu cards", &["png"]).pick_file(),
                Pick::Project => dialog
                    .add_filter("Renkatsu projects", &[crate::project::EXTENSION])
                    .pick_file(),
                Pick::Folder => dialog.pick_folder(),
            };
            let _ = send.send(picked);
            ctx.request_repaint();
        });
        Picker { what, answer }
    }

    /// The answer once the dialog closed: a path, or `None` when the user
    /// cancelled. `None` while it is still open.
    pub fn poll(&self) -> Option<Option<PathBuf>> {
        match self.answer.try_recv() {
            Ok(picked) => Some(picked),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(None),
        }
    }
}

/// What the user chose in a dialog.
enum Choice {
    /// Nothing yet.
    Wait,
    Cancel,
    /// The first button: import, save, create.
    Accept,
    /// Go on without saving.
    Discard,
    /// Look for another folder.
    Browse,
}

const WIDTH: f32 = 460.0;

fn heading(ui: &mut Ui, k: &Look, text: &str) {
    ui.label(RichText::new(text).font(k.bold(k.d.text + 1.0)).color(k.t.text_bright));
    ui.add_space(6.0);
}

/// The buttons along a dialog's foot, the first one at the right.
fn buttons(ui: &mut Ui, names: &[(&str, bool)]) -> Option<usize> {
    let mut clicked = None;
    ui.add_space(10.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        for (i, (name, enabled)) in names.iter().enumerate() {
            let button = egui::Button::new(*name).min_size(egui::Vec2::new(84.0, 24.0));
            if ui.add_enabled(*enabled, button).clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

fn import(ui: &mut Ui, k: &Look, pending: &mut PendingImport) -> Choice {
    let t = &k.t;
    heading(ui, k, &format!("Import {}", pending.title));
    let n = pending.plan.copy.len();
    ui.label(format!(
        "This card needs {} that the project does not have yet. Importing copies {} into the project, \
         which adds {}.",
        count_text(n),
        if n == 1 { "it" } else { "them" },
        size_text(pending.plan.bytes())
    ));
    ui.add_space(6.0);
    egui::Frame::new()
        .fill(t.input)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("mods to copy")
                .max_height(150.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for m in &pending.plan.copy {
                        ui.horizontal(|ui| {
                            let file = m.path.file_name().unwrap_or_default().to_string_lossy();
                            let size = size_text(m.size);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(RichText::new(size).font(k.small()).color(t.text_dim));
                                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                    ui.add(egui::Label::new(RichText::new(file).font(k.small())).truncate())
                                        .on_hover_text(format!("{} {}", m.guid, m.version));
                                });
                            });
                        });
                    }
                });
        });
    if pending.plan.present > 0 {
        ui.add_space(6.0);
        let have = pending.plan.present;
        ui.label(
            RichText::new(format!(
                "{} it needs {} in the project already.",
                count_text(have),
                if have == 1 { "is" } else { "are" }
            ))
            .color(t.text_dim),
        );
    }
    if !pending.missing.is_empty() {
        ui.add_space(6.0);
        let lost = pending.missing.len();
        ui.label(
            RichText::new(format!(
                "{} the card names {} not installed and cannot be copied; the character may miss parts:",
                count_text(lost),
                if lost == 1 { "is" } else { "are" }
            ))
            .color(t.accent),
        );
        let shown: Vec<&str> = pending.missing.iter().take(6).map(String::as_str).collect();
        let mut text = shown.join(", ");
        if lost > shown.len() {
            text.push_str(&format!(" and {} more", lost - shown.len()));
        }
        ui.label(RichText::new(text).font(k.small()).color(t.text_dim));
    }
    ui.add_space(8.0);
    ui.checkbox(&mut pending.dont_ask, "Do not ask again for this project");
    match buttons(ui, &[("Cancel", true), ("Import", true)]) {
        Some(0) => Choice::Cancel,
        Some(_) => Choice::Accept,
        None => Choice::Wait,
    }
}

fn unsaved(ui: &mut Ui, k: &Look, scene: &str, after: &After) -> Choice {
    heading(ui, k, "Unsaved changes");
    ui.label(format!(
        "{scene} has changes that are not saved. Save them before {}?",
        match after {
            After::New => "making a new project",
            After::Open(_) => "opening another project",
            After::Quit => "quitting",
        }
    ));
    match buttons(ui, &[("Cancel", true), ("Discard", true), ("Save", true)]) {
        Some(0) => Choice::Cancel,
        Some(1) => Choice::Discard,
        Some(_) => Choice::Accept,
        None => Choice::Wait,
    }
}

fn project_form(
    ui: &mut Ui,
    k: &Look,
    save_as: bool,
    name: &mut String,
    folder: &std::path::Path,
    problem: Option<&str>,
) -> Choice {
    let t = &k.t;
    heading(ui, k, if save_as { "Save project as" } else { "New project" });
    let mut choice = Choice::Wait;
    egui::Grid::new("project form")
        .num_columns(2)
        .spacing([10.0, 8.0])
        .show(ui, |ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(name).desired_width(WIDTH - 110.0));
            ui.end_row();
            ui.label("Location");
            ui.horizontal(|ui| {
                if ui.button("Browse").clicked() {
                    choice = Choice::Browse;
                }
                ui.add(egui::Label::new(RichText::new(folder.display().to_string()).color(t.text_dim)).truncate());
            });
            ui.end_row();
        });
    ui.add_space(6.0);
    let target = folder.join(project::file_name(name));
    ui.label(
        RichText::new(format!("The project's folder will be {}", target.display()))
            .font(k.small())
            .color(t.text_dim),
    );
    if let Some(problem) = problem {
        ui.add_space(4.0);
        ui.label(RichText::new(problem).color(t.accent));
    }
    let ready = !name.trim().is_empty();
    match buttons(
        ui,
        &[("Cancel", true), (if save_as { "Save" } else { "Create" }, ready)],
    ) {
        Some(0) => Choice::Cancel,
        Some(_) => Choice::Accept,
        None => choice,
    }
}

/// Draws the open dialog and carries out what the user chose in it.
pub fn show(ctx: &egui::Context, k: &Look, st: &mut State) {
    let Some(mut modal) = st.modal.take() else { return };
    let scene = st.doc.scene().name.clone();
    let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::symmetric(16, 14));
    let shown = egui::Modal::new(egui::Id::new("dialog")).frame(frame).show(ctx, |ui| {
        ui.set_width(WIDTH);
        match &mut modal {
            Modal::Import(pending) => import(ui, k, pending),
            Modal::Unsaved(after) => unsaved(ui, k, &scene, after),
            Modal::Project {
                save_as,
                name,
                folder,
                problem,
            } => project_form(ui, k, *save_as, name, folder, problem.as_deref()),
        }
    });
    let choice = match shown.inner {
        Choice::Wait if shown.should_close() => Choice::Cancel,
        other => other,
    };
    match (choice, modal) {
        (Choice::Wait, modal) => st.modal = Some(modal),
        (Choice::Cancel, _) => {}
        (Choice::Browse, modal) => {
            if let Modal::Project { folder, .. } = &modal {
                st.pick(Pick::Folder, Some(folder.clone()), ctx);
            }
            st.modal = Some(modal);
        }
        (Choice::Accept, Modal::Import(pending)) => st.confirm_import(pending),
        (Choice::Accept, Modal::Unsaved(after)) => {
            if st.save() {
                st.proceed(after, ctx);
            }
        }
        (Choice::Discard, Modal::Unsaved(after)) => st.proceed(after, ctx),
        (
            Choice::Accept,
            Modal::Project {
                save_as, name, folder, ..
            },
        ) => {
            let root = folder.join(project::file_name(&name));
            let done = if save_as {
                st.save_as(&root, name.trim())
            } else {
                st.new_project(&root, name.trim())
            };
            if let Err(e) = done {
                st.modal = Some(Modal::Project {
                    save_as,
                    name,
                    folder,
                    problem: Some(format!("{e:#}")),
                });
            }
        }
        (Choice::Discard, _) => {}
    }
}
