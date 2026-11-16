// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Packages in the studio: the Packages window, which lists, downloads,
//! installs, checks and removes them, and the window that imports a
//! package's cards into the project.
//!
//! Packages belong to the user, not to a project: they are installed
//! once, in Renkatsu's data folder, and every project builds its
//! characters from them. The work on them runs on a thread of its own,
//! so the window keeps drawing and can stop it.

use crate::dialogs::{Import, Pick};
use crate::state::{Look, State};
use crate::widgets::hair_h;
use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, Ui, Vec2};
use rk_packages::{size_text, Installed, Known, Stage, Store, Task};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

/// A package that is installed, with what the list shows of it.
#[derive(Debug, Clone)]
pub struct Row {
    pub package: Installed,
    /// Bytes on disk, counted once when the list is read.
    pub size: u64,
}

/// Work on a package that is under way.
struct Job {
    /// The package's name, for the progress line.
    name: String,
    task: Arc<Task>,
    /// What to say when it is done, or what went wrong.
    result: Receiver<Result<String, String>>,
}

/// A card of an installed package, for the import window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageCard {
    pub path: PathBuf,
    /// The character's name, or the file's when the card has none.
    pub name: String,
    /// The name of the package it is in.
    pub package: String,
}

/// The window that imports cards from installed packages.
#[derive(Default)]
pub struct Browser {
    pub cards: Vec<PackageCard>,
    pub chosen: BTreeSet<usize>,
    pub search: String,
    thumbs: HashMap<PathBuf, Option<TextureHandle>>,
}

#[derive(Default)]
pub struct Packages {
    /// The Packages window shows.
    pub open: bool,
    /// The import window, while it shows.
    pub browser: Option<Browser>,
    store: Option<Store>,
    pub installed: Vec<Row>,
    known: Vec<Known>,
    job: Option<Job>,
    /// What the last piece of work said, and whether it failed.
    pub note: Option<(String, bool)>,
    /// What a check found wrong, one line per file.
    problems: Vec<String>,
    /// The packages changed since the editor last looked.
    changed: bool,
    /// A package file the user picked, to install at the next frame.
    pub picked: Option<PathBuf>,
}

/// What the project selector shows of the packages.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Offer {
    /// Starter Content: Characters is installed.
    pub starter: bool,
    /// Bytes its download takes, when Renkatsu knows where to get it.
    pub download: Option<u64>,
    /// The work under way, as a line of text.
    pub busy: Option<String>,
}

/// The names cards show under: each card's character name, or its file
/// name where the card has none or shares its name with another card,
/// as the game's extras all carry the default character's name.
pub fn card_names(cards: &[(String, Option<String>)]) -> Vec<String> {
    let named = |name: &Option<String>| name.clone().filter(|n| !n.trim().is_empty());
    cards
        .iter()
        .map(|(stem, name)| match named(name) {
            Some(name)
                if cards
                    .iter()
                    .filter(|(_, other)| named(other).as_ref() == Some(&name))
                    .count()
                    == 1 =>
            {
                name
            }
            _ => stem.clone(),
        })
        .collect()
}

/// The cards of some packages, with the names they show under.
pub fn cards_of(packages: &[Installed]) -> Vec<PackageCard> {
    let mut found = Vec::new();
    for package in packages {
        for path in package.cards() {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            let name = rk_card::Card::load(&path).ok().map(|c| c.full_name());
            found.push((path, package.manifest.name.clone(), (stem, name)));
        }
    }
    let names: Vec<(String, Option<String>)> = found.iter().map(|f| f.2.clone()).collect();
    found
        .into_iter()
        .zip(card_names(&names))
        .map(|((path, package, _), name)| PackageCard { path, name, package })
        .collect()
}

impl Packages {
    /// The user's packages, read from disk.
    pub fn new() -> Packages {
        let mut packages = Packages {
            store: Store::user().ok(),
            known: rk_packages::known(),
            ..Default::default()
        };
        packages.refresh();
        packages.changed = false;
        packages
    }

    /// Reads the installed packages again.
    pub fn refresh(&mut self) {
        self.installed = self
            .store
            .as_ref()
            .map(Store::installed)
            .unwrap_or_default()
            .into_iter()
            .map(|package| Row {
                size: package.size(),
                package,
            })
            .collect();
        self.changed = true;
    }

    /// Whether the packages changed since this was last asked.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    fn find(&self, id: &str) -> Option<&Row> {
        self.installed.iter().find(|r| r.package.manifest.id == id)
    }

    /// The card new projects start with, from the first package that
    /// names one.
    pub fn default_card(&self) -> Option<PathBuf> {
        self.installed.iter().find_map(|r| r.package.default_card())
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// What the project selector shows.
    pub fn offer(&self) -> Offer {
        let id = rk_packages::STARTER_CHARACTERS;
        Offer {
            starter: self.find(id).is_some(),
            download: self.known.iter().find(|k| k.id == id).map(Known::size),
            busy: self.job.as_ref().map(|job| progress_text(&job.task)),
        }
    }

    fn run(
        &mut self,
        name: &str,
        ctx: &egui::Context,
        work: impl FnOnce(&Store, &Task) -> anyhow::Result<String> + Send + 'static,
    ) {
        if self.job.is_some() {
            return;
        }
        let Some(store) = self.store.clone() else {
            self.note = Some(("No data folder is known to keep packages in".to_owned(), true));
            return;
        };
        let task = Arc::new(Task::new());
        let (send, result) = channel();
        let (theirs, ctx) = (task.clone(), ctx.clone());
        std::thread::spawn(move || {
            let done = work(&store, &theirs).map_err(|e| format!("{e:#}"));
            let _ = send.send(done);
            ctx.request_repaint();
        });
        self.note = None;
        self.problems.clear();
        self.job = Some(Job {
            name: name.to_owned(),
            task,
            result,
        });
    }

    /// Downloads a known package and installs it.
    pub fn download(&mut self, id: &str, ctx: &egui::Context) {
        let Some(known) = self.known.iter().find(|k| k.id == id).cloned() else {
            self.note = Some((format!("Renkatsu does not know where to download {id}"), true));
            return;
        };
        let name = known.name.clone();
        self.run(&name, ctx, move |store, task| {
            let installed = rk_packages::download::fetch(&known, store, task)?;
            Ok(format!(
                "{} {} is installed",
                installed.manifest.name, installed.manifest.version
            ))
        });
    }

    /// Installs a package from a file the user picked.
    pub fn install(&mut self, file: &Path, ctx: &egui::Context) {
        let file = file.to_owned();
        let name = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
        self.run(&name, ctx, move |store, task| {
            let installed = store.install(&file, task)?;
            Ok(format!(
                "{} {} is installed",
                installed.manifest.name, installed.manifest.version
            ))
        });
    }

    fn verify(&mut self, id: &str, ctx: &egui::Context) {
        let Some(name) = self.find(id).map(|r| r.package.manifest.name.clone()) else {
            return;
        };
        let id = id.to_owned();
        self.run(&name.clone(), ctx, move |store, task| {
            let problems = store.verify(&id, task)?;
            if problems.is_empty() {
                Ok(format!("{name} is sound: every file matches its checksum"))
            } else {
                anyhow::bail!("{name} is damaged; install it again\n{}", problems.join("\n"))
            }
        });
    }

    fn remove(&mut self, id: &str, ctx: &egui::Context) {
        let Some(name) = self.find(id).map(|r| r.package.manifest.name.clone()) else {
            return;
        };
        let id = id.to_owned();
        self.run(&name.clone(), ctx, move |store, task| {
            // One step: a removal is not stopped halfway.
            task.begin(Stage::Removing, 0);
            store.remove(&id)?;
            Ok(format!("{name} is removed"))
        });
    }

    /// Takes the result of the work under way, once it is done.
    pub fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        let result = match job.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the work stopped without a word".to_owned()),
        };
        let cancelled = job.task.cancelled();
        self.job = None;
        match result {
            Ok(text) => self.note = Some((text, false)),
            Err(_) if cancelled => self.note = Some(("Stopped".to_owned(), false)),
            Err(text) => {
                let mut lines = text.lines();
                self.note = Some((lines.next().unwrap_or_default().to_owned(), true));
                self.problems = lines.map(str::to_owned).collect();
            }
        }
        self.refresh();
    }

    /// Opens the import window on the cards of the installed packages.
    pub fn browse(&mut self) {
        let packages: Vec<Installed> = self.installed.iter().map(|r| r.package.clone()).collect();
        self.browser = Some(Browser {
            cards: cards_of(&packages),
            ..Default::default()
        });
    }
}

/// The stage of some work and how far it is: "Downloading 120 MB of 850 MB".
fn progress_text(task: &Task) -> String {
    let (stage, done, total) = task.progress();
    match stage {
        Stage::Waiting | Stage::Removing => stage.label().to_owned(),
        _ if total == 0 => stage.label().to_owned(),
        _ => format!(
            "{} {} of {}",
            stage.label(),
            size_text(done.min(total)),
            size_text(total)
        ),
    }
}

/// What the user asked of a package in the list.
enum Ask {
    Download(String),
    Verify(String),
    Remove(String),
}

/// The editor dims behind a window drawn like the project selector, and
/// takes no clicks. Returns whether the dimmed part was clicked.
fn veil(ctx: &egui::Context, id: &str) -> bool {
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new((id, "veil")))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(120));
            resp.clicked()
        })
        .inner
}

/// The parts of such a window: its header, body and foot.
struct Frame {
    head: Rect,
    body: Rect,
    foot: Rect,
}

const HEAD: f32 = 52.0;
const FOOT: f32 = 56.0;

/// Paints the window's panels and its header, and says whether its close
/// cell was clicked.
fn frame(ui: &mut Ui, k: &Look, window: Rect, title: &str, under: &str) -> (Frame, bool) {
    let t = &k.t;
    let head = Rect::from_min_size(window.min, Vec2::new(window.width(), HEAD));
    let foot = Rect::from_min_max(Pos2::new(window.left(), window.bottom() - FOOT), window.max);
    let body = Rect::from_min_max(head.left_bottom(), foot.right_top());
    let p = ui.painter();
    p.rect_filled(window, 0.0, t.panel);
    p.rect_filled(foot, 0.0, t.header);
    hair_h(p, head.left(), head.right(), head.bottom() - 1.0, t.line);
    hair_h(p, foot.left(), foot.right(), foot.top(), t.divider);
    p.rect_stroke(window, 0.0, Stroke::new(1.0, t.border), StrokeKind::Outside);
    p.text(
        Pos2::new(head.left() + 20.0, head.center().y - 8.0),
        egui::Align2::LEFT_CENTER,
        title,
        k.bold(15.0),
        t.text_bright,
    );
    p.text(
        Pos2::new(head.left() + 20.0, head.center().y + 10.0),
        egui::Align2::LEFT_CENTER,
        under,
        k.small(),
        t.text_dim,
    );
    let cell = Rect::from_center_size(Pos2::new(head.right() - 26.0, head.center().y), Vec2::splat(28.0));
    let x = ui.interact(cell, ui.id().with((title, "close")), Sense::click());
    if x.hovered() {
        ui.painter().rect_filled(cell, k.radius(), t.control);
    }
    crate::widgets::glyph(
        ui.painter(),
        k,
        cell.center(),
        egui_phosphor::regular::X,
        14.0,
        if x.hovered() { t.text_bright } else { t.text_dim },
    );
    (Frame { head, body, foot }, x.clicked())
}

/// The window's place: centred, as large as `want` where the screen allows.
fn placed(ctx: &egui::Context, want: Vec2, least: Vec2) -> Rect {
    let screen = ctx.content_rect();
    let size = Vec2::new(
        (screen.width() - 80.0).clamp(least.x, want.x),
        (screen.height() - 80.0).clamp(least.y, want.y),
    );
    Rect::from_center_size(screen.center(), size)
}

/// One package in the window: its mark, name, version, what it is, its
/// sizes, and the buttons that act on it. Returns what was asked for.
fn package_card(
    ui: &mut Ui,
    k: &Look,
    rect: Rect,
    known: Option<&Known>,
    row: Option<&Row>,
    free: bool,
) -> Option<Ask> {
    let t = &k.t;
    let (id, name) = match (row, known) {
        (Some(r), _) => (r.package.manifest.id.clone(), r.package.manifest.name.clone()),
        (None, Some(n)) => (n.id.clone(), n.name.clone()),
        (None, None) => return None,
    };
    let newer = match (row, known) {
        (Some(r), Some(n)) => {
            rk_packages::manifest::version_key(&n.version)
                > rk_packages::manifest::version_key(&r.package.manifest.version)
        }
        _ => false,
    };
    let version = row.map_or_else(
        || known.map_or(String::new(), |n| n.version.clone()),
        |r| r.package.manifest.version.clone(),
    );
    // The short description of the list Renkatsu ships comes first; the
    // installed manifest's may be from an older, longer version.
    let about = known
        .map(|n| n.description.as_str())
        .or(row.map(|r| r.package.manifest.description.as_str()))
        .unwrap_or_default()
        .to_owned();
    let size = match (row, known) {
        (Some(r), _) => size_text(r.size),
        (None, Some(n)) => format!("{} download", size_text(n.size())),
        (None, None) => String::new(),
    };
    let facts = format!("Version {version}, {size}");

    let mark = Rect::from_min_size(rect.min + Vec2::new(20.0, 14.0), Vec2::splat(52.0));
    let p = ui.painter();
    p.rect_filled(mark, 0.0, t.input);
    crate::widgets::glyph(
        p,
        k,
        mark.center(),
        egui_phosphor::regular::PACKAGE,
        24.0,
        if row.is_some() { t.select_bright } else { t.text_dim },
    );
    let x = mark.right() + 16.0;
    let buttons_w = 3.0 * 96.0 + 2.0 * 8.0;
    let text_w = rect.right() - 20.0 - buttons_w - 16.0 - x;
    p.text(
        Pos2::new(x, mark.top() + 8.0),
        egui::Align2::LEFT_CENTER,
        &name,
        k.bold(k.d.text + 1.0),
        t.text_bright,
    );
    crate::selector::line(ui, Pos2::new(x, mark.top() + 27.0), text_w, &about, k.font(), t.text);
    let under = Pos2::new(x, mark.top() + 45.0);
    let shown = crate::selector::line_width(ui, &facts, k.small());
    crate::selector::line(ui, under, text_w, &facts, k.small(), t.text_dim);
    if let (true, Some(n)) = (newer, known) {
        let note = format!("Update available: {}", n.version);
        crate::selector::line(
            ui,
            under + Vec2::new(shown + 14.0, 0.0),
            (text_w - shown - 14.0).max(0.0),
            &note,
            k.small(),
            t.accent,
        );
    }

    // The buttons, from the right.
    let mut ask = None;
    let mut right = rect.right() - 20.0;
    let mut place = |text: &str, primary: bool, ui: &mut Ui| {
        let r = Rect::from_min_size(Pos2::new(right - 96.0, mark.top() + 12.0), Vec2::new(96.0, 28.0));
        right -= 104.0;
        crate::selector::button(ui, k, r, text, primary, free)
    };
    match row {
        Some(_) => {
            if place("Remove", false, ui) {
                ask = Some(Ask::Remove(id.clone()));
            }
            if place("Verify", false, ui) {
                ask = Some(Ask::Verify(id.clone()));
            }
            if newer && place("Update", true, ui) {
                ask = Some(Ask::Download(id.clone()));
            }
        }
        None => {
            if place("Download", true, ui) {
                ask = Some(Ask::Download(id.clone()));
            }
        }
    }
    ask
}

/// The Packages window, drawn like the project selector.
pub fn show(ctx: &egui::Context, k: &Look, st: &mut State) {
    st.packages.poll();
    if let Some(file) = st.packages.picked.take() {
        st.packages.open = true;
        st.packages.install(&file, ctx);
    }
    if st.packages.take_changed() {
        st.packages_changed();
    }
    if !st.packages.open {
        return;
    }
    let t = k.t;
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape)) | veil(ctx, "packages");
    let mut ask = None;
    let mut pick = false;
    let mut cancel = false;
    let packages = &st.packages;
    let free = !packages.busy() && st.picker.is_none();
    let window = placed(ctx, Vec2::new(760.0, 520.0), Vec2::new(560.0, 380.0));
    egui::Area::new(egui::Id::new("packages"))
        .order(egui::Order::Foreground)
        .fixed_pos(window.min)
        .show(ctx, |ui| {
            ui.set_min_size(window.size());
            let (f, x) = frame(
                ui,
                k,
                window,
                "Packages",
                "Content installed once and used by every project",
            );
            close |= x;

            // The packages Renkatsu knows, then the others that are installed.
            let mut rows: Vec<(Option<&Known>, Option<&Row>)> = packages
                .known
                .iter()
                .map(|known| (Some(known), packages.find(&known.id)))
                .collect();
            rows.extend(
                packages
                    .installed
                    .iter()
                    .filter(|row| !packages.known.iter().any(|n| n.id == row.package.manifest.id))
                    .map(|row| (None, Some(row))),
            );
            const ROW: f32 = 80.0;
            let mut y = f.body.top();
            for (known, row) in rows.iter().copied() {
                let rect = Rect::from_min_size(Pos2::new(f.body.left(), y), Vec2::new(f.body.width(), ROW));
                if let Some(a) = package_card(ui, k, rect, known, row, free) {
                    ask = Some(a);
                }
                hair_h(
                    ui.painter(),
                    rect.left() + 20.0,
                    rect.right() - 20.0,
                    rect.bottom() - 1.0,
                    t.line,
                );
                y += ROW;
            }
            if rows.is_empty() {
                ui.painter().text(
                    f.body.center(),
                    egui::Align2::CENTER_CENTER,
                    "No package is installed",
                    k.font(),
                    t.text_dim,
                );
            }
            // What a check found wrong.
            if packages.job.is_none() && !packages.problems.is_empty() {
                let mut py = y + 14.0;
                for problem in packages.problems.iter().take(6) {
                    crate::selector::line(
                        ui,
                        Pos2::new(f.body.left() + 20.0, py),
                        f.body.width() - 40.0,
                        problem,
                        k.small(),
                        t.text_dim,
                    );
                    py += 16.0;
                }
                if packages.problems.len() > 6 {
                    let more = format!("and {} more", packages.problems.len() - 6);
                    crate::selector::line(
                        ui,
                        Pos2::new(f.body.left() + 20.0, py),
                        f.body.width() - 40.0,
                        &more,
                        k.small(),
                        t.text_dim,
                    );
                }
            }

            // The foot: the work under way with its bar, or the last
            // result, or where packages are kept; and Install from a file.
            let install = Rect::from_min_size(
                Pos2::new(f.foot.right() - 20.0 - 150.0, f.foot.center().y - 15.0),
                Vec2::new(150.0, 30.0),
            );
            let left = f.foot.left() + 20.0;
            let text_w = install.left() - 20.0 - left;
            if let Some(job) = &packages.job {
                let (_, done, total) = job.task.progress();
                let fraction = if total == 0 { 0.0 } else { done as f32 / total as f32 };
                let stop = install;
                let text = format!("{}: {}", job.name, progress_text(&job.task));
                crate::selector::line(
                    ui,
                    Pos2::new(left, f.foot.center().y - 7.0),
                    text_w - 110.0,
                    &text,
                    k.font(),
                    t.text,
                );
                let bar = Rect::from_min_size(Pos2::new(left, f.foot.center().y + 9.0), Vec2::new(text_w - 110.0, 4.0));
                ui.painter().rect_filled(bar, 0.0, t.input);
                ui.painter().rect_filled(
                    Rect::from_min_size(bar.min, Vec2::new(bar.width() * fraction.clamp(0.0, 1.0), bar.height())),
                    0.0,
                    t.select,
                );
                if crate::selector::button(ui, k, stop, "Stop", false, !job.task.cancelled()) {
                    cancel = true;
                }
                // The numbers move while the work goes on.
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            } else {
                let (text, color) = match (&packages.note, &packages.store) {
                    (Some((text, failed)), _) => (text.clone(), if *failed { t.accent } else { t.text }),
                    (None, Some(store)) => (format!("Packages are kept in {}", store.dir.display()), t.text_dim),
                    (None, None) => (String::new(), t.text_dim),
                };
                crate::selector::line(ui, Pos2::new(left, f.foot.center().y), text_w, &text, k.font(), color);
                if crate::selector::button(ui, k, install, "Install from a file", false, free) {
                    pick = true;
                }
            }
            let _ = f.head;
        });
    if cancel {
        if let Some(job) = &st.packages.job {
            job.task.cancel();
        }
    }
    match ask {
        Some(Ask::Download(id)) => st.packages.download(&id, ctx),
        Some(Ask::Verify(id)) => st.packages.verify(&id, ctx),
        Some(Ask::Remove(id)) => st.packages.remove(&id, ctx),
        None => {}
    }
    if pick {
        st.pick(Pick::Package, None, ctx);
    }
    if close {
        st.packages.open = false;
    }
}

/// The cards the search leaves, as indices.
pub fn matching(cards: &[PackageCard], search: &str) -> Vec<usize> {
    let search = search.trim().to_lowercase();
    (0..cards.len())
        .filter(|&i| {
            let stem = cards[i]
                .path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            search.is_empty() || cards[i].name.to_lowercase().contains(&search) || stem.contains(&search)
        })
        .collect()
}

/// The window that imports cards from the installed packages, drawn like
/// the project selector: a header with the search, the cards in a grid,
/// and a foot with what is chosen and the buttons.
pub fn browser(ctx: &egui::Context, k: &Look, st: &mut State) {
    let Some(mut browser) = st.packages.browser.take() else {
        return;
    };
    let t = k.t;
    let eager = st.scripted;
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let mut import = false;
    let mut to_packages = false;

    // The editor dims behind the window and takes no clicks.
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("package browser veil"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(120));
            if resp.clicked() {
                close = true;
            }
        });

    let size = Vec2::new(
        (screen.width() - 80.0).clamp(560.0, 820.0),
        (screen.height() - 80.0).clamp(420.0, 620.0),
    );
    let window = Rect::from_center_size(screen.center(), size);
    egui::Area::new(egui::Id::new("package browser"))
        .order(egui::Order::Foreground)
        .fixed_pos(window.min)
        .show(ctx, |ui| {
            ui.set_min_size(size);
            const HEAD: f32 = 52.0;
            const FOOT: f32 = 56.0;
            let head = Rect::from_min_size(window.min, Vec2::new(size.x, HEAD));
            let foot = Rect::from_min_max(Pos2::new(window.left(), window.bottom() - FOOT), window.max);
            let body = Rect::from_min_max(head.left_bottom(), foot.right_top());
            {
                let p = ui.painter();
                p.rect_filled(window, 0.0, t.panel);
                p.rect_filled(foot, 0.0, t.header);
                hair_h(p, head.left(), head.right(), head.bottom() - 1.0, t.line);
                hair_h(p, foot.left(), foot.right(), foot.top(), t.divider);
                p.rect_stroke(window, 0.0, Stroke::new(1.0, t.border), StrokeKind::Outside);
            }

            // The header: the title, where the cards come from, the search
            // and the close cell.
            let p = ui.painter();
            p.text(
                Pos2::new(head.left() + 20.0, head.center().y - 8.0),
                egui::Align2::LEFT_CENTER,
                "Import characters",
                k.bold(15.0),
                t.text_bright,
            );
            let mut packages: Vec<&str> = browser.cards.iter().map(|c| c.package.as_str()).collect();
            packages.dedup();
            let from = match packages.len() {
                0 => "No package with characters is installed".to_owned(),
                1 => format!("From {}", packages[0]),
                n => format!("From {n} packages"),
            };
            p.text(
                Pos2::new(head.left() + 20.0, head.center().y + 10.0),
                egui::Align2::LEFT_CENTER,
                from,
                k.small(),
                t.text_dim,
            );
            let x_cell = Rect::from_center_size(Pos2::new(head.right() - 26.0, head.center().y), Vec2::splat(28.0));
            let x = ui.interact(x_cell, ui.id().with("close"), Sense::click());
            if x.hovered() {
                ui.painter().rect_filled(x_cell, k.radius(), t.control);
            }
            crate::widgets::glyph(
                ui.painter(),
                k,
                x_cell.center(),
                egui_phosphor::regular::X,
                14.0,
                if x.hovered() { t.text_bright } else { t.text_dim },
            );
            if x.clicked() {
                close = true;
            }
            if !browser.cards.is_empty() {
                let search = Rect::from_min_size(
                    Pos2::new(x_cell.left() - 12.0 - 240.0, head.center().y - k.field() / 2.0),
                    Vec2::new(240.0, k.field()),
                );
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(search));
                crate::widgets::search(&mut child, k, &mut browser.search, "Search characters", 240.0);
            }

            if browser.cards.is_empty() {
                let c = body.center();
                ui.painter().text(
                    c - Vec2::new(0.0, 30.0),
                    egui::Align2::CENTER_CENTER,
                    "No installed package has characters",
                    k.bold(k.d.text),
                    t.text_bright,
                );
                ui.painter().text(
                    c - Vec2::new(0.0, 8.0),
                    egui::Align2::CENTER_CENTER,
                    "Starter Content: Characters brings Koikatsu's own characters.",
                    k.font(),
                    t.text_dim,
                );
                let open = Rect::from_center_size(c + Vec2::new(0.0, 28.0), Vec2::new(150.0, 30.0));
                if crate::selector::button(ui, k, open, "Open Packages", true, true) {
                    to_packages = true;
                }
            } else {
                // The cards, evenly spaced in as many columns as fit.
                let shown = matching(&browser.cards, &browser.search);
                const GAP: f32 = 14.0;
                let thumb = Vec2::new(116.0, (116.0 * 352.0 / 252.0_f32).round());
                let tile = Vec2::new(thumb.x, thumb.y + 42.0);
                let inner = body.shrink2(Vec2::new(20.0, 0.0));
                let columns = ((inner.width() + GAP) / (tile.x + GAP)).floor().max(1.0) as usize;
                let used = columns as f32 * tile.x + (columns - 1) as f32 * GAP;
                let left = inner.left() + ((inner.width() - used) / 2.0).floor();
                let rows = shown.len().div_ceil(columns);
                let mut area = ui.new_child(egui::UiBuilder::new().max_rect(body));
                area.set_clip_rect(body);
                let mut loaded = false;
                egui::ScrollArea::vertical()
                    .id_salt("package cards")
                    .auto_shrink(false)
                    .max_height(body.height())
                    .show(&mut area, |ui| {
                        let height = 18.0 + rows as f32 * (tile.y + GAP);
                        let (space, _) = ui.allocate_exact_size(Vec2::new(body.width(), height), Sense::hover());
                        if shown.is_empty() {
                            ui.painter().text(
                                Pos2::new(body.center().x, space.top() + 40.0),
                                egui::Align2::CENTER_CENTER,
                                "No character matches the search",
                                k.font(),
                                t.text_dim,
                            );
                        }
                        for (n, &i) in shown.iter().enumerate() {
                            let min = Pos2::new(
                                left + (n % columns) as f32 * (tile.x + GAP),
                                space.top() + 18.0 + (n / columns) as f32 * (tile.y + GAP),
                            );
                            let rect = Rect::from_min_size(min, tile);
                            if !ui.is_rect_visible(rect) {
                                continue;
                            }
                            let card = &browser.cards[i];
                            let resp = ui.interact(rect, ui.id().with(("package card", i)), Sense::click());
                            if resp.clicked() && !browser.chosen.remove(&i) {
                                browser.chosen.insert(i);
                            }
                            if resp.double_clicked() {
                                browser.chosen.insert(i);
                                import = true;
                            }
                            // Pictures load one a frame, so the window opens at once.
                            if !browser.thumbs.contains_key(&card.path) && (eager || !loaded) {
                                loaded = true;
                                let texture = crate::library::picture(&card.path).map(|image| {
                                    ui.ctx().load_texture(
                                        format!("package card {}", card.path.display()),
                                        image,
                                        egui::TextureOptions::LINEAR,
                                    )
                                });
                                browser.thumbs.insert(card.path.clone(), texture);
                                ui.ctx().request_repaint();
                            }
                            let on = browser.chosen.contains(&i);
                            let picture = Rect::from_min_size(rect.min, thumb);
                            let p = ui.painter();
                            p.rect_filled(picture, 0.0, t.input);
                            match browser.thumbs.get(&card.path) {
                                Some(Some(texture)) => {
                                    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                                    let tint = if on || resp.hovered() {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_gray(222)
                                    };
                                    p.image(texture.id(), picture, uv, tint);
                                }
                                _ => crate::widgets::glyph(
                                    p,
                                    k,
                                    picture.center(),
                                    egui_phosphor::regular::USER,
                                    32.0,
                                    t.border,
                                ),
                            }
                            let outline = if on {
                                Some(Stroke::new(2.0, t.select))
                            } else if resp.hovered() {
                                Some(Stroke::new(1.0, t.hover))
                            } else {
                                None
                            };
                            if let Some(stroke) = outline {
                                p.rect_stroke(picture, 0.0, stroke, StrokeKind::Outside);
                            }
                            if on {
                                let badge = Pos2::new(picture.right() - 13.0, picture.top() + 13.0);
                                p.circle_filled(badge, 9.0, t.select);
                                crate::widgets::glyph(p, k, badge, egui_phosphor::regular::CHECK, 11.0, Color32::WHITE);
                            }
                            let sex = if card.path.components().any(|c| c.as_os_str() == "male") {
                                "Male"
                            } else {
                                "Female"
                            };
                            crate::selector::line(
                                ui,
                                Pos2::new(rect.left(), picture.bottom() + 13.0),
                                tile.x,
                                &card.name,
                                k.font(),
                                if on { t.text_bright } else { t.text },
                            );
                            crate::selector::line(
                                ui,
                                Pos2::new(rect.left(), picture.bottom() + 30.0),
                                tile.x,
                                sex,
                                k.small(),
                                t.text_dim,
                            );
                            resp.on_hover_text(&card.name);
                        }
                    });
            }

            // The foot: what is chosen, and the buttons.
            let n = browser.chosen.len();
            let text = match n {
                0 if browser.cards.is_empty() => String::new(),
                0 => "Click characters to choose them, or double click one to import it".to_owned(),
                1 => "1 character chosen. It is copied into the project.".to_owned(),
                n => format!("{n} characters chosen. They are copied into the project."),
            };
            ui.painter().text(
                Pos2::new(foot.left() + 20.0, foot.center().y),
                egui::Align2::LEFT_CENTER,
                text,
                k.font(),
                t.text_dim,
            );
            let import_rect = Rect::from_min_size(
                Pos2::new(foot.right() - 20.0 - 104.0, foot.center().y - 15.0),
                Vec2::new(104.0, 30.0),
            );
            let cancel_rect = import_rect.translate(Vec2::new(-104.0 - 8.0, 0.0));
            if crate::selector::button(ui, k, cancel_rect, "Cancel", false, true) {
                close = true;
            }
            if crate::selector::button(ui, k, import_rect, "Import", true, n > 0) {
                import = true;
            }
        });

    if to_packages {
        st.packages.open = true;
        return;
    }
    if import && !browser.chosen.is_empty() {
        for &i in &browser.chosen {
            st.imports.push_back(Import {
                path: browser.cards[i].path.clone(),
                place: false,
                ask: true,
                // The name the window showed, which tells namesakes apart.
                name: Some(browser.cards[i].name.clone()),
            });
        }
        return;
    }
    if !close {
        st.packages.browser = Some(browser);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(name: &str, file: &str) -> PackageCard {
        PackageCard {
            path: PathBuf::from("/packages/starter/cards/female").join(file),
            name: name.to_owned(),
            package: "Starter Content: Characters".to_owned(),
        }
    }

    #[test]
    fn the_search_looks_at_names_and_files() {
        let cards = [
            card("Sakura Aoi", "Default female.png"),
            card("Kobayashi", "Story c-8.png"),
        ];
        assert_eq!(matching(&cards, ""), [0, 1]);
        assert_eq!(matching(&cards, " sakura"), [0]);
        assert_eq!(matching(&cards, "STORY"), [1]);
        assert!(matching(&cards, "nobody").is_empty());
    }

    /// A package with two cards, installed in a folder of its own.
    fn installed_in(home: &Path) -> Packages {
        let manifest = rk_packages::Manifest {
            format: rk_packages::FORMAT,
            id: rk_packages::STARTER_CHARACTERS.to_owned(),
            name: "Starter Content: Characters".to_owned(),
            version: "1.0.0".to_owned(),
            kind: "starter-content".to_owned(),
            description: String::new(),
            requires_renkatsu: "0.1.0".to_owned(),
            source: String::new(),
            default_card: Some("cards/female/Default female.png".to_owned()),
            contents: Default::default(),
        };
        let mut pack = rk_packages::pack::Pack::new(manifest);
        // Not cards that read: they show under their file names.
        pack.add_bytes("cards/female/Default female.png", b"her card".to_vec())
            .unwrap();
        pack.add_bytes("cards/male/Story player.png", b"his card".to_vec())
            .unwrap();
        let archive = home.join("starter.rkpkg");
        pack.write(&archive, &mut |_| {}).unwrap();
        let store = Store::at(&home.join("packages"));
        store.install(&archive, &Task::new()).unwrap();
        let mut packages = Packages {
            store: Some(store),
            ..Default::default()
        };
        packages.refresh();
        packages
    }

    #[test]
    fn a_packages_cards_are_listed_and_imported_into_a_project() {
        let home = std::env::temp_dir().join(format!("renkatsu-studio-packages-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let mut packages = installed_in(&home);
        assert!(packages.take_changed());
        assert!(packages.offer().starter);
        let root = home.join("packages").join(rk_packages::STARTER_CHARACTERS);
        let default = root.join("cards/female/Default female.png");
        assert_eq!(packages.default_card(), Some(default.clone()));

        packages.browse();
        let browser = packages.browser.as_ref().unwrap();
        let listed: Vec<(&str, &str)> = browser
            .cards
            .iter()
            .map(|c| (c.name.as_str(), c.package.as_str()))
            .collect();
        assert_eq!(
            listed,
            [
                ("Default female", "Starter Content: Characters"),
                ("Story player", "Starter Content: Characters"),
            ]
        );

        // The card is copied into the project as one from disk is, and
        // the package keeps its own.
        let mut project = crate::project::Project::create(&home.join("P"), "P").unwrap();
        let file = project.import_card(&browser.cards[0].path, "Renkatsu-Chan").unwrap();
        assert_eq!(file, "Characters/Default female.png");
        assert_eq!(std::fs::read(home.join("P").join(&file)).unwrap(), b"her card");
        assert!(default.is_file());
        assert_eq!(project.characters()[0].name, "Renkatsu-Chan");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn new_projects_start_with_the_games_default_card_else_the_packages() {
        let home = std::env::temp_dir().join(format!("renkatsu-studio-default-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let package = Some(home.join("packages/starter/cards/female/Default female.png"));
        // No game at all, or a game without its default card.
        assert_eq!(crate::state::default_card(None, package.clone()), package);
        assert_eq!(crate::state::default_card(None, None), None);
        let game = rk_core::Config {
            game_dir: home.join("Koikatsu"),
        };
        assert_eq!(crate::state::default_card(Some(&game), package.clone()), package);
        let cards = game.userdata().join("chara").join("female");
        std::fs::create_dir_all(&cards).unwrap();
        std::fs::write(cards.join("Simple.png"), b"card").unwrap();
        assert_eq!(
            crate::state::default_card(Some(&game), package),
            Some(cards.join("Simple.png"))
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn namesakes_and_nameless_cards_show_under_their_file_names() {
        let card = |stem: &str, name: Option<&str>| (stem.to_owned(), name.map(str::to_owned));
        let names = card_names(&[
            card("Default female", Some("Haruno Chika")),
            card("Extra tia a", Some("Haruno Chika")),
            card("Story c-1", Some("Rina Robert")),
            card("Story c-2", Some("  ")),
            card("Not a card", None),
        ]);
        assert_eq!(
            names,
            [
                "Default female",
                "Extra tia a",
                "Rina Robert",
                "Story c-2",
                "Not a card"
            ]
        );
    }

    #[test]
    fn work_under_way_reads_as_a_stage_and_sizes() {
        let task = Task::new();
        assert_eq!(progress_text(&task), "Waiting");
        let offer = Packages::default().offer();
        assert_eq!(
            offer,
            Offer {
                starter: false,
                download: None,
                busy: None
            }
        );
    }
}
