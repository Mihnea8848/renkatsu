// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The project selector: the small window Renkatsu shows when it starts,
//! before any project is open, after Unreal's project browser. It lists the
//! recent projects as tiles or as rows, makes a new project and opens one
//! from disk. It reads only the settings and a few small files of each
//! project, never the game, so that it is there at once.
//!
//! A project shows under the icon its user gave it, else under the picture
//! of its scene that the editor took at the last save, else under a plain
//! mark. A card's picture never stands for a project.

use crate::dialogs::{Pick, ProjectForm};
use crate::project::{self, Start};
use crate::settings::{self, ProjectView, Recent, Settings};
use crate::state::{Look, State};
use crate::widgets::{self, boxed, glyph, hair_h, hair_v};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, Ui, UiBuilder, Vec2};
use egui_phosphor::regular as icon;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The size of the selector's window.
pub const WINDOW: Vec2 = Vec2::new(1040.0, 680.0);

/// A recent project as the selector lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The project folder.
    pub root: PathBuf,
    pub name: String,
    /// The folder holds no project any more.
    pub missing: bool,
    /// When it was last opened, in seconds since 1970; 0 when not known.
    pub opened: u64,
    pub scenes: usize,
    pub characters: usize,
    /// The picture that stands for the project: its icon, else the picture
    /// of its last saved scene.
    pub picture: Option<PathBuf>,
    /// The picture is the user's icon, which is shown whole.
    pub icon: bool,
}

/// The recent projects, in the order of the settings. A project that is
/// gone stays in the list, marked, under its folder's name.
pub fn entries(recent: &[Recent]) -> Vec<Entry> {
    recent
        .iter()
        .map(|r| {
            let folder = r.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            match project::summary(&r.path) {
                Some(summary) => Entry {
                    root: r.path.clone(),
                    name: summary.name,
                    missing: false,
                    opened: r.opened,
                    scenes: summary.scenes,
                    characters: summary.characters,
                    icon: summary.icon.is_some(),
                    picture: summary.icon.or(summary.thumbnail),
                },
                None => Entry {
                    root: r.path.clone(),
                    name: folder,
                    missing: true,
                    opened: r.opened,
                    scenes: 0,
                    characters: 0,
                    icon: false,
                    picture: None,
                },
            }
        })
        .collect()
}

/// The entries a search leaves, by index: those whose name or folder holds
/// the text, without regard to case. An empty search leaves them all.
pub fn matching(entries: &[Entry], search: &str) -> Vec<usize> {
    let needle = search.trim().to_lowercase();
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            needle.is_empty()
                || e.name.to_lowercase().contains(&needle)
                || e.root.to_string_lossy().to_lowercase().contains(&needle)
        })
        .map(|(i, _)| i)
        .collect()
}

/// How long ago a time was, in words: "5 minutes ago", "Yesterday", and
/// the date once it is more than a few weeks. Empty when the time is not
/// known.
pub fn ago(now: u64, then: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    if then == 0 {
        return String::new();
    }
    let count = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match now.saturating_sub(then) {
        s if s < MINUTE => "Just now".to_owned(),
        s if s < HOUR => count(s / MINUTE, "minute"),
        s if s < DAY => count(s / HOUR, "hour"),
        s if s < 2 * DAY => "Yesterday".to_owned(),
        s if s < 7 * DAY => count(s / DAY, "day"),
        s if s < 35 * DAY => count(s / (7 * DAY), "week"),
        _ => date(then),
    }
}

/// A time as a date, "16 Nov 2026", in universal time.
fn date(seconds: u64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // Days to a date of the calendar, after Howard Hinnant's algorithm.
    let z = (seconds / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{day} {} {year}", MONTHS[(month - 1) as usize])
}

/// A folder as the user reads it: the home folder as a tilde.
fn shown_path(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if !cfg!(windows) => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

/// A count with its noun: "No scenes", "1 scene", "3 scenes".
pub fn counted(n: usize, noun: &str) -> String {
    match n {
        0 => format!("No {noun}s"),
        1 => format!("1 {noun}"),
        n => format!("{n} {noun}s"),
    }
}

/// What a project holds, in a few words: "1 scene, 2 characters".
pub fn contents(scenes: usize, characters: usize) -> String {
    format!(
        "{}, {}",
        counted(scenes, "scene"),
        counted(characters, "character").to_lowercase()
    )
}

/// The parts of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Recent,
    New,
}

/// How long the content takes to fade in and rise into place, a row or a
/// tile to answer the pointer, and the two views to cross over, in seconds
/// at egui's usual animation time.
const APPEAR: f32 = 0.22;
const ANSWER: f32 = 0.12;
const CROSS: f32 = 0.18;

/// A duration in step with egui's animation time, so that a style that
/// turns animations off, as scripted pictures do, turns these off too.
fn secs(ctx: &egui::Context, seconds: f32) -> f32 {
    seconds * ctx.global_style().animation_time * 12.0
}

/// Fast at first and slowing into the end, `x` from 0 to 1.
fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

pub struct Selector {
    pub entries: Vec<Entry>,
    /// The selected entry, by index.
    pub selected: Option<usize>,
    pub search: String,
    pub tab: Tab,
    /// The new project, which the window fills in in place.
    pub form: ProjectForm,
    /// What went wrong with the last thing asked for.
    pub problem: Option<String>,
    /// The project a picture is being picked for as its icon.
    pub icon_for: Option<PathBuf>,
    /// Pictures by file; `None` for one that did not load.
    thumbs: HashMap<PathBuf, Option<TextureHandle>>,
    /// A picture was loaded this frame; the next one waits a frame.
    loaded: bool,
    /// Load every picture at once, for scripted pictures of the window.
    pub eager: bool,
    /// The game's default card, looked for once.
    default_card: Option<Option<PathBuf>>,
    /// Seconds the content of the open part has been showing, for its
    /// fade; it starts over when the part changes.
    age: f32,
    /// Seconds the window has been showing, for its fade.
    window_age: f32,
}

impl Selector {
    pub fn new(settings: &Settings) -> Selector {
        let mut selector = Selector {
            entries: Vec::new(),
            selected: None,
            search: String::new(),
            tab: Tab::Recent,
            form: ProjectForm::new(),
            problem: None,
            icon_for: None,
            thumbs: HashMap::new(),
            loaded: false,
            eager: false,
            default_card: None,
            age: 0.0,
            window_age: 0.0,
        };
        selector.refresh(settings);
        selector
    }

    /// Lists the recent projects again, with their pictures. The selection
    /// stays on its project when that is still listed, else it goes to the
    /// first one that can be opened.
    pub fn refresh(&mut self, settings: &Settings) {
        let before = self.selected.and_then(|i| self.entries.get(i)).map(|e| e.root.clone());
        self.entries = entries(&settings.recent_projects);
        self.thumbs.clear();
        self.selected = before
            .and_then(|root| self.entries.iter().position(|e| e.root == root))
            .or_else(|| self.entries.iter().position(|e| !e.missing))
            .or(if self.entries.is_empty() { None } else { Some(0) });
    }

    /// Takes a project off the list, in the settings too.
    pub fn remove(&mut self, settings: &mut Settings, root: &Path) {
        settings.forget(root);
        self.refresh(settings);
    }

    fn chosen(&self) -> Option<&Entry> {
        self.selected.and_then(|i| self.entries.get(i))
    }

    /// The entries the search leaves, with the selection kept among them.
    fn shown(&mut self) -> Vec<usize> {
        let shown = matching(&self.entries, &self.search);
        if !self.selected.is_some_and(|i| shown.contains(&i)) {
            self.selected = shown.first().copied();
        }
        shown
    }

    fn show_tab(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.age = 0.0;
        }
    }

    /// A picture from a PNG file. Pictures load one a frame, so there may
    /// be none yet; the window is then drawn again.
    fn thumb(&mut self, ctx: &egui::Context, file: &Path) -> Option<TextureHandle> {
        if !self.thumbs.contains_key(file) {
            if self.loaded && !self.eager {
                ctx.request_repaint();
                return None;
            }
            self.loaded = true;
            let texture = crate::library::picture(file).map(|image| {
                ctx.load_texture(
                    format!("project {}", file.display()),
                    image,
                    egui::TextureOptions::LINEAR,
                )
            });
            self.thumbs.insert(file.to_owned(), texture);
        }
        self.thumbs.get(file).cloned().flatten()
    }

    /// The game's default card, for the picture of the start it stands for.
    fn default_card(&mut self) -> Option<PathBuf> {
        self.default_card
            .get_or_insert_with(|| {
                rk_core::Config::load()
                    .ok()
                    .map(|c| c.userdata().join("chara").join("female").join("Simple.png"))
                    .filter(|p| p.is_file())
            })
            .clone()
    }
}

/// What the user asked of the selector.
enum Act {
    Open(PathBuf),
    /// Take a project off the list.
    Remove(PathBuf),
    OpenOther,
    /// Look for another folder for the new project.
    Browse,
    /// Make the project of the form.
    Create,
    /// Pick a picture as a project's icon.
    SetIcon(PathBuf),
    /// Take a project's icon away.
    ClearIcon(PathBuf),
    /// Show the recent projects as tiles or as rows.
    View(ProjectView),
}

fn apply(act: Act, st: &mut State, ctx: &egui::Context) {
    let save = |st: &mut State| {
        if !st.scripted {
            if let Err(e) = st.settings.save() {
                st.log.warn(format!("The settings could not be saved: {e:#}"));
            }
        }
    };
    match act {
        Act::Open(path) => st.open_or_tell(&path),
        Act::Remove(path) => {
            if let Some(selector) = &mut st.selector {
                selector.remove(&mut st.settings, &path);
            }
            save(st);
        }
        Act::View(view) => {
            st.settings.project_view = view;
            save(st);
        }
        Act::OpenOther => st.proceed(crate::dialogs::After::Open(None), ctx),
        Act::Browse => {
            let folder = st.selector.as_ref().map(|s| s.form.folder.clone());
            st.pick(Pick::Folder, folder, ctx);
        }
        Act::SetIcon(root) => {
            if let Some(selector) = &mut st.selector {
                selector.icon_for = Some(root);
            }
            st.pick(Pick::Icon, None, ctx);
        }
        Act::ClearIcon(root) => {
            let cleared = project::set_icon_in(&root, None);
            if let Some(selector) = &mut st.selector {
                selector.problem = cleared.err().map(|e| format!("{e:#}"));
                selector.refresh(&st.settings);
            }
        }
        Act::Create => {
            let Some(form) = st.selector.as_ref().map(|s| s.form.clone()) else {
                return;
            };
            if let Err(e) = st.new_project(&form.root(), form.name.trim(), form.start) {
                if let Some(selector) = &mut st.selector {
                    selector.form.problem = Some(format!("{e:#}"));
                }
            }
        }
    }
}

/// The selector as the content of its window.
pub fn page(ui: &mut Ui, k: &Look, st: &mut State) {
    let Some(mut sel) = st.selector.take() else { return };
    sel.loaded = false;
    let dt = ui.input(|i| i.stable_dt).min(1.0 / 30.0);
    sel.age += dt;
    sel.window_age += dt;
    let free = st.modal.is_none() && st.picker.is_none();
    let act = window(ui, k, &mut sel, st.settings.project_view, settings::now(), free);
    st.selector = Some(sel);
    if let Some(act) = act {
        apply(act, st, ui.ctx());
    }
}

// What the window draws with.

/// One line of text from `pos`, centred on it vertically and cut with an
/// ellipsis at `width`.
fn line(ui: &Ui, pos: Pos2, width: f32, text: &str, font: FontId, color: Color32) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = ui.painter().layout_job(job);
    let pos = Pos2::new(pos.x, (pos.y - galley.size().y / 2.0).round());
    ui.painter().galley(pos, galley, color);
}

/// A boxed button; the primary one of a view is filled with the selection
/// colour. Returns whether it was clicked.
fn button(ui: &mut Ui, k: &Look, rect: Rect, text: &str, primary: bool, enabled: bool) -> bool {
    let t = &k.t;
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(rect, ui.id().with(("selector button", text)), sense);
    let hot = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("hot"), enabled && resp.hovered(), secs(ui.ctx(), ANSWER));
    let (fill, color) = match (enabled, primary) {
        (false, _) => (t.header, t.text_dim),
        (true, true) => (mix(t.select, t.select_bright, hot), Color32::WHITE),
        (true, false) => (mix(t.control, t.hover, hot), mix(t.text, t.text_bright, hot)),
    };
    let p = ui.painter();
    p.rect_filled(rect, k.radius(), fill);
    p.text(rect.center(), Align2::CENTER_CENTER, text, k.font(), color);
    resp.clicked()
}

/// A row of a side column: an icon, a name and the keys that do the same.
/// `on` marks the part of the window that shows.
fn side_row(ui: &mut Ui, k: &Look, rect: Rect, glyph_name: &str, text: &str, keys: &str, on: bool) -> bool {
    let t = &k.t;
    let resp = ui.interact(rect, ui.id().with(("selector side", text)), Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 0.0, t.panel);
        p.rect_filled(
            Rect::from_min_max(rect.min, Pos2::new(rect.left() + 2.0, rect.bottom())),
            0.0,
            t.select,
        );
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, t.row_hover);
    }
    let color = if on || resp.hovered() { t.text_bright } else { t.text };
    let y = rect.center().y;
    let tint = if on { t.select_bright } else { color };
    glyph(p, k, Pos2::new(rect.left() + 26.0, y), glyph_name, 16.0, tint);
    p.text(
        Pos2::new(rect.left() + 44.0, y),
        Align2::LEFT_CENTER,
        text,
        k.font(),
        color,
    );
    if !keys.is_empty() {
        p.text(
            Pos2::new(rect.right() - 14.0, y),
            Align2::RIGHT_CENTER,
            keys,
            k.small(),
            t.text_dim,
        );
    }
    resp.clicked()
}

fn version() -> String {
    format!("Version {}", env!("CARGO_PKG_VERSION"))
}

/// The part of a picture that fills a frame of another shape: the middle
/// in width, and near the top in height, as texture coordinates.
fn cover_uv(picture: Vec2, frame: Vec2) -> Rect {
    let (p, f) = (picture.x / picture.y.max(1.0), frame.x / frame.y.max(1.0));
    if p > f {
        let w = f / p;
        Rect::from_min_max(Pos2::new(0.5 - w / 2.0, 0.0), Pos2::new(0.5 + w / 2.0, 1.0))
    } else {
        let h = p / f;
        let top = (1.0 - h) * 0.12;
        Rect::from_min_max(Pos2::new(0.0, top), Pos2::new(1.0, top + h))
    }
}

/// The rectangle a picture takes when it is shown whole in a frame.
fn contain_rect(picture: Vec2, frame: Rect) -> Rect {
    let scale = (frame.width() / picture.x.max(1.0)).min(frame.height() / picture.y.max(1.0));
    Rect::from_center_size(frame.center(), (picture * scale).round())
}

const WHOLE: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));

/// The picture that stands for a project: its icon, shown whole, else the
/// picture of its last saved scene, filling the frame, else a plain mark.
/// `lit` is 0 at rest and 1 under the pointer or when chosen.
fn project_picture(ui: &Ui, k: &Look, sel: &mut Selector, entry: &Entry, rect: Rect, lit: f32) {
    let t = &k.t;
    ui.painter().rect_filled(rect, 0.0, t.input);
    let texture = match (&entry.picture, entry.missing) {
        (Some(file), false) => sel.thumb(ui.ctx(), file),
        _ => None,
    };
    let tint = mix(Color32::from_gray(214), Color32::WHITE, lit);
    match texture {
        Some(texture) if entry.icon => {
            let fit = contain_rect(texture.size_vec2(), rect.shrink((rect.height() * 0.08).round()));
            ui.painter().image(texture.id(), fit, WHOLE, tint);
        }
        Some(texture) => {
            let uv = cover_uv(texture.size_vec2(), rect.size());
            ui.painter().image(texture.id(), rect, uv, tint);
        }
        None => {
            let (mark, color) = if entry.missing {
                (icon::WARNING, t.accent)
            } else {
                (icon::CUBE, mix(t.border, t.hover, lit))
            };
            let size = (rect.height() * 0.3).clamp(14.0, 44.0);
            glyph(ui.painter(), k, rect.center(), mark, size, color);
        }
    }
}

/// How far a tile or a row is under the pointer and how far it is chosen,
/// each 0 to 1, eased over a moment.
fn answer(ui: &Ui, id: egui::Id, hovered: bool, on: bool) -> (f32, f32) {
    let ctx = ui.ctx();
    let time = secs(ctx, ANSWER);
    (
        ctx.animate_bool_with_time(id.with("hover"), hovered, time),
        ctx.animate_bool_with_time(id.with("chosen"), on, time),
    )
}

/// The keys of the recent projects: the arrows move the selection, by
/// `columns` up and down; Enter opens it; and the keys of the File menu
/// make and open projects.
fn keys(ui: &Ui, sel: &mut Selector, shown: &[usize], columns: usize) -> Option<Act> {
    use egui::{Key, Modifiers};
    let ctx = ui.ctx();
    if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::N)) {
        sel.show_tab(Tab::New);
        return None;
    }
    if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::O)) {
        return Some(Act::OpenOther);
    }
    let typing = ctx.egui_wants_keyboard_input();
    let mut step = 0isize;
    ctx.input(|i| {
        if i.key_pressed(Key::ArrowDown) {
            step += columns as isize;
        }
        if i.key_pressed(Key::ArrowUp) {
            step -= columns as isize;
        }
        if columns > 1 && !typing {
            step += isize::from(i.key_pressed(Key::ArrowRight));
            step -= isize::from(i.key_pressed(Key::ArrowLeft));
        }
    });
    if step != 0 && !shown.is_empty() {
        let at = sel.selected.and_then(|s| shown.iter().position(|i| *i == s));
        let to = match at {
            Some(at) => (at as isize + step).clamp(0, shown.len() as isize - 1) as usize,
            None => 0,
        };
        sel.selected = Some(shown[to]);
    }
    if ctx.input(|i| i.key_pressed(Key::Enter)) {
        return sel.chosen().filter(|e| !e.missing).map(|e| Act::Open(e.root.clone()));
    }
    None
}

/// The menu of a project under the right mouse button.
fn entry_menu(resp: &egui::Response, entry: &Entry, act: &mut Option<Act>) {
    resp.context_menu(|ui| {
        let mut item = |ui: &mut Ui, enabled: bool, text: &str, chosen: Act| {
            if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
                *act = Some(chosen);
                ui.close();
            }
        };
        item(ui, !entry.missing, "Open", Act::Open(entry.root.clone()));
        ui.separator();
        item(ui, !entry.missing, "Set icon", Act::SetIcon(entry.root.clone()));
        if entry.icon {
            item(ui, true, "Remove icon", Act::ClearIcon(entry.root.clone()));
        }
        ui.separator();
        item(ui, true, "Remove from the list", Act::Remove(entry.root.clone()));
    });
}

/// What a list says when it has nothing to show.
fn nothing(ui: &Ui, k: &Look, sel: &Selector, rect: Rect) {
    let t = &k.t;
    let (title, hint) = if sel.entries.is_empty() {
        (
            "No projects yet".to_owned(),
            "Make a new project, or open one from disk.",
        )
    } else {
        (
            format!("No project matches \"{}\"", sel.search.trim()),
            "The search looks at names and folders.",
        )
    };
    let c = Pos2::new(rect.center().x, rect.top() + (rect.height() * 0.4).min(120.0));
    let p = ui.painter();
    p.text(c, Align2::CENTER_CENTER, title, k.bold(k.d.text + 1.0), t.text);
    p.text(
        c + Vec2::new(0.0, 22.0),
        Align2::CENTER_CENTER,
        hint,
        k.font(),
        t.text_dim,
    );
}

/// A scrolling column in `rect`, `height` tall inside. The content gets
/// the rectangle its rows are laid out in.
fn scroll(ui: &mut Ui, rect: Rect, id: &str, height: f32, content: impl FnOnce(&mut Ui, Rect)) {
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            let (inner, _) = ui.allocate_exact_size(Vec2::new(rect.width(), height), Sense::hover());
            content(ui, inner);
        });
}

// The window, after Unreal's project browser: a column chooses between
// the recent projects and a new one; the recent projects are tiles or rows;
// what is chosen is named along the foot, by the button that opens it.

const SIDE: f32 = 208.0;
const HEAD: f32 = 52.0;
const FOOT: f32 = 64.0;
const GAP: f32 = 12.0;
/// The least width of a tile; its picture has the shape of a screen.
const TILE_MIN: f32 = 240.0;
const CAPTION: f32 = 50.0;
const ROW: f32 = 58.0;

/// How far something has come in, 0 to 1, `age` seconds after it showed.
fn arrival(ui: &Ui, age: f32) -> f32 {
    let time = secs(ui.ctx(), APPEAR);
    if time <= 0.0 || age >= time {
        return 1.0;
    }
    ui.ctx().request_repaint();
    ease_out(age / time)
}

fn window(ui: &mut Ui, k: &Look, sel: &mut Selector, view: ProjectView, now: u64, free: bool) -> Option<Act> {
    let t = &k.t;
    let full = ui.max_rect();
    let mut act = None;

    let side = Rect::from_min_max(full.min, Pos2::new(full.left() + SIDE, full.bottom()));
    let main = Rect::from_min_max(Pos2::new(side.right(), full.top()), full.max);
    let head = Rect::from_min_size(main.min, Vec2::new(main.width(), HEAD));
    let foot = Rect::from_min_max(Pos2::new(main.left(), main.bottom() - FOOT), main.max);
    let body = Rect::from_min_max(head.left_bottom(), foot.right_top());
    {
        let p = ui.painter();
        p.rect_filled(side, 0.0, t.well);
        p.rect_filled(main, 0.0, t.panel);
        p.rect_filled(foot, 0.0, t.header);
        hair_v(p, side.right() - 1.0, side.top(), side.bottom(), t.divider);
        hair_h(p, head.left(), head.right(), head.bottom() - 1.0, t.line);
        hair_h(p, foot.left(), foot.right(), foot.top(), t.divider);
    }
    // The window's content fades in when it shows.
    let shown = arrival(ui, sel.window_age);
    ui.set_opacity(shown);

    let row = |n: f32| {
        Rect::from_min_size(
            Pos2::new(side.left(), side.top() + 12.0 + n * 36.0),
            Vec2::new(SIDE - 1.0, 36.0),
        )
    };
    let recent = sel.tab == Tab::Recent;
    if side_row(
        ui,
        k,
        row(0.0),
        icon::CLOCK_COUNTER_CLOCKWISE,
        "Recent projects",
        "",
        recent,
    ) {
        sel.show_tab(Tab::Recent);
    }
    if side_row(ui, k, row(1.0), icon::PLUS, "New project", "Ctrl+N", !recent) {
        sel.show_tab(Tab::New);
    }
    let y = row(2.0).top() + 8.0;
    hair_h(ui.painter(), side.left() + 14.0, side.right() - 15.0, y, t.border);
    let open = row(2.0).translate(Vec2::new(0.0, 17.0));
    if side_row(ui, k, open, icon::FOLDER_OPEN, "Open project", "Ctrl+O", false) {
        act = Some(Act::OpenOther);
    }
    ui.painter().text(
        Pos2::new(side.left() + 18.0, side.bottom() - 18.0),
        Align2::LEFT_CENTER,
        version(),
        k.small(),
        t.text_dim,
    );

    // The open part comes in with a fade and a slight rise.
    let come = arrival(ui, sel.age);
    let mut part = ui.new_child(UiBuilder::new().max_rect(main));
    part.multiply_opacity(come);
    let rise = Vec2::new(0.0, ((1.0 - come) * 10.0).round());
    part.painter().text(
        Pos2::new(head.left() + 20.0, head.center().y),
        Align2::LEFT_CENTER,
        if sel.tab == Tab::Recent {
            "Recent projects"
        } else {
            "New project"
        },
        k.bold(15.0),
        t.text_bright,
    );
    let more = match sel.tab {
        Tab::Recent => recent_part(&mut part, k, sel, view, now, free, head, body, foot, rise),
        Tab::New => new_part(&mut part, k, sel, free, body, foot, rise),
    };
    act.or(more)
}

/// The two cells that choose between tiles and rows.
fn view_toggle(ui: &mut Ui, k: &Look, right: f32, y: f32, view: ProjectView) -> Option<ProjectView> {
    let t = &k.t;
    let size = Vec2::new(28.0, k.field());
    let frame = Rect::from_min_size(
        Pos2::new(right - size.x * 2.0, y - size.y / 2.0),
        Vec2::new(size.x * 2.0, size.y),
    );
    boxed(ui.painter(), frame, k.radius(), t.input, t.border);
    let mut chosen = None;
    for (n, (which, mark, tip)) in [
        (ProjectView::Grid, icon::SQUARES_FOUR, "Show as tiles"),
        (ProjectView::List, icon::LIST, "Show as a list"),
    ]
    .into_iter()
    .enumerate()
    {
        let cell = Rect::from_min_size(frame.min + Vec2::new(n as f32 * size.x, 0.0), size);
        let resp = ui
            .interact(cell, ui.id().with(("view", n)), Sense::click())
            .on_hover_text(tip);
        let (hover, on) = answer(ui, resp.id, resp.hovered(), view == which);
        let inner = cell.shrink(2.0);
        ui.painter().rect_filled(
            inner,
            k.radius() - 1.0,
            mix(Color32::TRANSPARENT, t.control, on.max(hover * 0.6)),
        );
        let color = mix(mix(t.text_dim, t.text, hover), t.text_bright, on);
        glyph(ui.painter(), k, cell.center(), mark, 15.0, color);
        if resp.clicked() && view != which {
            chosen = Some(which);
        }
    }
    chosen
}

#[allow(clippy::too_many_arguments)]
fn recent_part(
    ui: &mut Ui,
    k: &Look,
    sel: &mut Selector,
    view: ProjectView,
    now: u64,
    free: bool,
    head: Rect,
    body: Rect,
    foot: Rect,
    rise: Vec2,
) -> Option<Act> {
    let t = &k.t;
    let mut act = view_toggle(ui, k, head.right() - 20.0, head.center().y, view).map(Act::View);
    let search = Rect::from_min_size(
        Pos2::new(
            head.right() - 20.0 - 56.0 - 10.0 - 230.0,
            head.center().y - k.field() / 2.0,
        ),
        Vec2::new(230.0, k.field()),
    );
    let mut child = ui.new_child(UiBuilder::new().max_rect(search));
    widgets::search(&mut child, k, &mut sel.search, "Search projects", search.width());

    let shown = sel.shown();
    let area = body.shrink2(Vec2::new(20.0, 0.0));
    let columns = (((area.width() + GAP) / (TILE_MIN + GAP)).floor() as usize).max(1);
    if free && act.is_none() {
        act = keys(ui, sel, &shown, if view == ProjectView::Grid { columns } else { 1 });
    }
    if shown.is_empty() {
        nothing(ui, k, sel, body);
    }
    // The two views cross over when the user changes between them.
    let list = ui.ctx().animate_bool_with_time_and_easing(
        ui.id().with("project view"),
        view == ProjectView::List,
        secs(ui.ctx(), CROSS),
        egui::emath::easing::cubic_out,
    );
    for (opacity, is_list) in [(1.0 - list, false), (list, true)] {
        if opacity <= 0.0 {
            continue;
        }
        let mut layer = ui.new_child(
            UiBuilder::new()
                .max_rect(area.translate(rise))
                .id_salt(("view", is_list)),
        );
        layer.set_clip_rect(body.intersect(ui.clip_rect()));
        layer.multiply_opacity(opacity);
        let rect = area.translate(rise);
        let more = if is_list {
            rows(&mut layer, k, sel, &shown, now, rect)
        } else {
            tiles(&mut layer, k, sel, &shown, now, rect, columns)
        };
        // Only the view that is coming in is acted on.
        if (opacity >= 0.5) && more.is_some() {
            act = more;
        }
    }

    // The foot names what is chosen, by the button that opens it.
    let y = foot.center().y;
    let button_size = Vec2::new(104.0, 30.0);
    let open = Rect::from_min_size(
        Pos2::new(foot.right() - 20.0 - button_size.x, y - button_size.y / 2.0),
        button_size,
    );
    let chosen = sel.chosen().cloned();
    let mut text_right = open.left() - 16.0;
    if let Some(entry) = &chosen {
        if entry.missing {
            let remove = Rect::from_min_size(
                Pos2::new(open.left() - 10.0 - 150.0, open.top()),
                Vec2::new(150.0, button_size.y),
            );
            text_right = remove.left() - 16.0;
            if button(ui, k, remove, "Remove from the list", false, true) {
                act = Some(Act::Remove(entry.root.clone()));
            }
        }
        let x = foot.left() + 20.0;
        let w = text_right - x;
        let (second, color) = match (&sel.problem, entry.missing) {
            (Some(problem), _) => (problem.clone(), t.accent),
            (None, true) => (
                format!("No project is at {} any more", shown_path(&entry.root)),
                t.accent,
            ),
            (None, false) => (shown_path(&entry.root), t.text_dim),
        };
        line(
            ui,
            Pos2::new(x, y - 9.0),
            w,
            &entry.name,
            k.bold(k.d.text),
            t.text_bright,
        );
        line(ui, Pos2::new(x, y + 9.0), w, &second, k.small(), color);
    } else if let Some(problem) = &sel.problem {
        line(
            ui,
            Pos2::new(foot.left() + 20.0, y),
            text_right - foot.left() - 20.0,
            problem,
            k.font(),
            t.accent,
        );
    }
    let can_open = chosen.as_ref().is_some_and(|e| !e.missing);
    if button(ui, k, open, "Open", true, can_open) {
        act = chosen.map(|e| Act::Open(e.root));
    }
    act
}

/// What a click, a double click and the right button do on a project.
fn entry_clicks(resp: &egui::Response, sel: &mut Selector, index: usize, entry: &Entry, act: &mut Option<Act>) {
    if resp.clicked() || resp.secondary_clicked() {
        sel.selected = Some(index);
        sel.problem = None;
    }
    if resp.double_clicked() && !entry.missing {
        *act = Some(Act::Open(entry.root.clone()));
    }
    entry_menu(resp, entry, act);
}

/// The recent projects as tiles: the picture, then the name over what the
/// project holds and when it was last opened.
fn tiles(
    ui: &mut Ui,
    k: &Look,
    sel: &mut Selector,
    shown: &[usize],
    now: u64,
    area: Rect,
    columns: usize,
) -> Option<Act> {
    let t = &k.t;
    let mut act = None;
    let width = ((area.width() + GAP) / columns as f32).floor() - GAP;
    let tile = Vec2::new(width, (width * 9.0 / 16.0).round() + CAPTION);
    let height = 16.0 + shown.len().div_ceil(columns) as f32 * (tile.y + GAP) + 4.0;
    let entries = sel.entries.clone();
    scroll(ui, area, "project tiles", height, |ui, inner| {
        for (n, index) in shown.iter().copied().enumerate() {
            let entry = &entries[index];
            let cell = Vec2::new((n % columns) as f32, (n / columns) as f32);
            let rect = Rect::from_min_size(
                inner.min + Vec2::new(0.0, 16.0) + cell * (tile + Vec2::splat(GAP)),
                tile,
            );
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let resp = ui.interact(rect, ui.id().with(("tile", index)), Sense::click());
            entry_clicks(&resp, sel, index, entry, &mut act);
            let (hover, on) = answer(ui, resp.id, resp.hovered(), sel.selected == Some(index));
            let picture = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.bottom() - CAPTION));
            let caption = Rect::from_min_max(picture.left_bottom(), rect.max);
            project_picture(ui, k, sel, entry, picture, hover.max(on));
            ui.painter()
                .rect_filled(caption, 0.0, mix(mix(t.control, t.hover, hover), t.select, on));
            let name = mix(
                if entry.missing { t.text_dim } else { t.text_bright },
                Color32::WHITE,
                on,
            );
            let small = mix(t.text_dim, Color32::from_white_alpha(210), on);
            let x = caption.left() + 10.0;
            let w = caption.width() - 20.0;
            line(
                ui,
                Pos2::new(x, caption.top() + 16.0),
                w,
                &entry.name,
                k.bold(k.d.text),
                name,
            );
            let y = caption.top() + 34.0;
            if entry.missing {
                line(
                    ui,
                    Pos2::new(x, y),
                    w,
                    "Missing",
                    k.small(),
                    mix(t.accent, Color32::WHITE, on),
                );
            } else {
                let time = ui
                    .painter()
                    .text(
                        Pos2::new(caption.right() - 10.0, y),
                        Align2::RIGHT_CENTER,
                        ago(now, entry.opened),
                        k.small(),
                        small,
                    )
                    .left();
                line(
                    ui,
                    Pos2::new(x, y),
                    time - 10.0 - x,
                    &contents(entry.scenes, entry.characters),
                    k.small(),
                    small,
                );
            }
            let p = ui.painter();
            p.rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0, t.hover.gamma_multiply(hover * (1.0 - on))),
                StrokeKind::Inside,
            );
            p.rect_stroke(
                rect,
                0.0,
                Stroke::new(2.0, t.select.gamma_multiply(on)),
                StrokeKind::Inside,
            );
        }
    });
    act
}

/// The recent projects as rows: the picture, the name over the folder,
/// then the scenes, the characters and when it was last opened.
fn rows(ui: &mut Ui, k: &Look, sel: &mut Selector, shown: &[usize], now: u64, area: Rect) -> Option<Act> {
    let t = &k.t;
    let mut act = None;
    let columns = Rect::from_min_size(area.min + Vec2::new(0.0, 16.0), Vec2::new(area.width(), 26.0));
    let opened_x = area.right() - 124.0;
    let characters_x = opened_x - 108.0;
    let scenes_x = characters_x - 84.0;
    let picture_size = Vec2::new(80.0, 45.0);
    let name_x = area.left() + 10.0 + picture_size.x + 14.0;
    ui.painter().rect_filled(columns, 0.0, t.header);
    for (x, text) in [
        (name_x, "Name"),
        (scenes_x, "Scenes"),
        (characters_x, "Characters"),
        (opened_x, "Last opened"),
    ] {
        ui.painter().text(
            Pos2::new(x, columns.center().y),
            Align2::LEFT_CENTER,
            text,
            k.small(),
            t.text,
        );
    }
    let list = Rect::from_min_max(columns.left_bottom(), area.max);
    let entries = sel.entries.clone();
    scroll(ui, list, "project rows", shown.len() as f32 * ROW + 4.0, |ui, inner| {
        for (n, index) in shown.iter().copied().enumerate() {
            let entry = &entries[index];
            let rect = Rect::from_min_size(
                inner.min + Vec2::new(0.0, n as f32 * ROW),
                Vec2::new(inner.width(), ROW),
            );
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let resp = ui.interact(rect, ui.id().with(("row", index)), Sense::click());
            entry_clicks(&resp, sel, index, entry, &mut act);
            let (hover, on) = answer(ui, resp.id, resp.hovered(), sel.selected == Some(index));
            let p = ui.painter();
            p.rect_filled(
                rect,
                0.0,
                mix(mix(Color32::TRANSPARENT, t.row_hover, hover), t.select_soft, on),
            );
            p.rect_filled(
                Rect::from_min_max(rect.min, Pos2::new(rect.left() + 2.0, rect.bottom())),
                0.0,
                t.select.gamma_multiply(on),
            );
            hair_h(p, rect.left(), rect.right(), rect.bottom() - 1.0, t.line);
            let y = rect.center().y;
            let picture = Rect::from_min_size(
                Pos2::new(rect.left() + 10.0, (y - picture_size.y / 2.0).round()),
                picture_size,
            );
            project_picture(ui, k, sel, entry, picture, hover.max(on));
            let name = if entry.missing { t.text_dim } else { t.text_bright };
            let dim = mix(t.text_dim, t.text, on);
            let name_w = scenes_x - name_x - 20.0;
            line(
                ui,
                Pos2::new(name_x, y - 9.0),
                name_w,
                &entry.name,
                k.bold(k.d.text),
                name,
            );
            line(
                ui,
                Pos2::new(name_x, y + 9.0),
                name_w,
                &shown_path(&entry.root),
                k.small(),
                dim,
            );
            if entry.missing {
                line(ui, Pos2::new(scenes_x, y), 200.0, "Missing", k.font(), t.accent);
                continue;
            }
            for (x, text) in [
                (scenes_x, counted(entry.scenes, "scene")),
                (characters_x, counted(entry.characters, "character")),
                (opened_x, ago(now, entry.opened)),
            ] {
                line(ui, Pos2::new(x, y), 120.0, &text, k.font(), dim);
            }
        }
    });
    act
}

#[allow(clippy::too_many_arguments)]
fn new_part(ui: &mut Ui, k: &Look, sel: &mut Selector, free: bool, body: Rect, foot: Rect, rise: Vec2) -> Option<Act> {
    let t = &k.t;
    let mut act = None;
    let top = body.top() + rise.y;
    let x = body.left() + 20.0;
    ui.painter().text(
        Pos2::new(x, top + 26.0),
        Align2::LEFT_CENTER,
        "Start with",
        k.font(),
        t.text,
    );
    let tile = Vec2::new(256.0, 144.0 + CAPTION);
    for (n, start) in Start::ALL.into_iter().enumerate() {
        let rect = Rect::from_min_size(Pos2::new(x + n as f32 * (tile.x + GAP), top + 44.0), tile);
        let resp = ui.interact(rect, ui.id().with(("start", n)), Sense::click());
        if resp.clicked() {
            sel.form.start = start;
        }
        let (hover, on) = answer(ui, resp.id, resp.hovered(), sel.form.start == start);
        let picture = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.bottom() - CAPTION));
        let caption = Rect::from_min_max(picture.left_bottom(), rect.max);
        ui.painter().rect_filled(picture, 0.0, t.input);
        // What the project starts with: here the card is the thing itself.
        let texture = match start {
            Start::RenkatsuChan => sel.default_card().and_then(|card| sel.thumb(ui.ctx(), &card)),
            Start::Empty => None,
        };
        match texture {
            Some(texture) => {
                let uv = cover_uv(texture.size_vec2(), picture.size());
                let tint = mix(Color32::from_gray(214), Color32::WHITE, hover.max(on));
                ui.painter().image(texture.id(), picture, uv, tint);
            }
            None => {
                let mark = if start == Start::Empty {
                    icon::LIGHTBULB
                } else {
                    icon::USER
                };
                let color = mix(t.border, t.hover, hover.max(on));
                glyph(ui.painter(), k, picture.center(), mark, 44.0, color);
            }
        }
        ui.painter()
            .rect_filled(caption, 0.0, mix(mix(t.control, t.hover, hover), t.select, on));
        let w = caption.width() - 20.0;
        let left = caption.left() + 10.0;
        line(
            ui,
            Pos2::new(left, caption.top() + 16.0),
            w,
            start.label(),
            k.bold(k.d.text),
            mix(t.text_bright, Color32::WHITE, on),
        );
        let under = match start {
            Start::RenkatsuChan => "The default character",
            Start::Empty => "The key light only",
        };
        line(
            ui,
            Pos2::new(left, caption.top() + 34.0),
            w,
            under,
            k.small(),
            mix(t.text_dim, Color32::from_white_alpha(210), on),
        );
        ui.painter().rect_stroke(
            rect,
            0.0,
            Stroke::new(2.0, t.select.gamma_multiply(on)),
            StrokeKind::Inside,
        );
    }
    let notes = Pos2::new(x + 2.0 * (tile.x + GAP) + 12.0, top + 56.0);
    let width = body.right() - 20.0 - notes.x;
    line(
        ui,
        notes,
        width,
        sel.form.start.label(),
        k.bold(k.d.text + 1.0),
        t.text_bright,
    );
    let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(
        notes + Vec2::new(0.0, 16.0),
        Vec2::new(width, 220.0),
    )));
    let about = match sel.form.start {
        Start::RenkatsuChan => {
            "The project starts with Renkatsu-Chan, the game's default character, standing under a key light. \
             Her card is copied into the project."
        }
        Start::Empty => {
            "The project starts with a key light and nothing else. Characters come in with File, Import card."
        }
    };
    child.label(egui::RichText::new(about).color(t.text_dim));
    let below = Pos2::new(x, top + 44.0 + tile.y + 22.0);
    let below_w = body.right() - 20.0 - x;
    line(
        ui,
        below,
        below_w,
        &format!("The project's folder will be {}", shown_path(&sel.form.root())),
        k.small(),
        t.text_dim,
    );
    if let Some(problem) = &sel.form.problem {
        line(ui, below + Vec2::new(0.0, 22.0), below_w, problem, k.font(), t.accent);
    }

    // The foot: the name, the place and the button that makes the project.
    let y = foot.center().y;
    let h = 26.0;
    let label_y = y - 18.0;
    let top = y - 6.0;
    let small = |ui: &Ui, x: f32, text: &str| {
        ui.painter()
            .text(Pos2::new(x, label_y), Align2::LEFT_CENTER, text, k.small(), t.text_dim);
    };
    let create = Rect::from_min_size(
        Pos2::new(foot.right() - 20.0 - 104.0, top - 2.0),
        Vec2::new(104.0, 30.0),
    );
    let browse = Rect::from_min_size(Pos2::new(create.left() - 16.0 - 76.0, top), Vec2::new(76.0, h));
    let name = Rect::from_min_size(Pos2::new(foot.left() + 20.0, top), Vec2::new(220.0, h));
    let place = Rect::from_min_max(
        Pos2::new(name.right() + 12.0, top),
        Pos2::new(browse.left() - 6.0, top + h),
    );
    small(ui, name.left(), "Name");
    small(ui, place.left(), "Location");
    let field = ui.put(
        name,
        egui::TextEdit::singleline(&mut sel.form.name)
            .margin(egui::Margin::symmetric(7, 5))
            .desired_width(name.width()),
    );
    if free && ui.memory(|m| m.focused().is_none()) {
        field.request_focus();
    }
    if field.changed() {
        sel.form.problem = None;
    }
    boxed(ui.painter(), place, k.radius(), t.input, t.border);
    line(
        ui,
        Pos2::new(place.left() + 8.0, place.center().y),
        place.width() - 16.0,
        &shown_path(&sel.form.folder),
        k.font(),
        t.text,
    );
    if button(ui, k, browse, "Browse", false, true) {
        act = Some(Act::Browse);
    }
    let ready = sel.form.ready();
    let entered = free && ready && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if button(ui, k, create, "Create", true, ready) || entered {
        act = Some(Act::Create);
    }
    if free && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        sel.show_tab(Tab::Recent);
    }
    if free && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O)) {
        act = Some(Act::OpenOther);
    }
    act
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use crate::scene::Scene;

    /// A folder of its own under the system's temporary folder.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("renkatsu-selector-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A PNG picture of one grey pixel.
    fn tiny_png(path: &Path) {
        let mut encoder = png::Encoder::new(std::fs::File::create(path).unwrap(), 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[90, 90, 90, 255])
            .unwrap();
    }

    /// Settings that list `Here` (two cards, one scene, a scene picture),
    /// `Bare` and a project that is gone, in that order.
    fn recent(home: &Path) -> Settings {
        let mut here = Project::create(&home.join("Here"), "Here: one").unwrap();
        std::fs::write(home.join("Here/Characters/b.png"), b"").unwrap();
        std::fs::write(home.join("Here/Characters/A.png"), b"").unwrap();
        here.save_scene(&Scene::new()).unwrap();
        let pixels = vec![0u8; (project::THUMBNAIL_SIZE[0] * project::THUMBNAIL_SIZE[1] * 4) as usize];
        here.set_thumbnail(Some(&pixels)).unwrap();
        Project::create(&home.join("Bare"), "Bare").unwrap();
        let mut settings = Settings::default();
        settings.opened_at(&home.join("Gone"), 10);
        settings.opened_at(&home.join("Bare"), 20);
        settings.opened_at(&home.join("Here"), 30);
        settings
    }

    #[test]
    fn recent_projects_are_listed_with_the_missing_ones_marked() {
        let home = scratch("list");
        let settings = recent(&home);
        let list = entries(&settings.recent_projects);
        let brief: Vec<(&str, bool, u64, usize, usize)> = list
            .iter()
            .map(|e| (e.name.as_str(), e.missing, e.opened, e.scenes, e.characters))
            .collect();
        assert_eq!(
            brief,
            [
                ("Here: one", false, 30, 1, 2),
                ("Bare", false, 20, 0, 0),
                ("Gone", true, 10, 0, 0)
            ]
        );
        assert_eq!(contents(1, 2), "1 scene, 2 characters");
        assert_eq!(contents(0, 1), "No scenes, 1 character");
        assert_eq!(counted(0, "character"), "No characters");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_project_shows_its_icon_else_its_scene_and_never_a_card() {
        let home = scratch("pictures");
        let settings = recent(&home);
        let mut selector = Selector::new(&settings);
        // The scene's picture stands for a project without an icon; a
        // project with cards but neither picture has none.
        assert_eq!(
            selector.entries[0].picture,
            Some(home.join("Here/.renkatsu/thumbnail.png"))
        );
        assert!(!selector.entries[0].icon);
        std::fs::write(home.join("Bare/Characters/card.png"), b"").unwrap();
        selector.refresh(&settings);
        assert_eq!(
            (selector.entries[1].characters, &selector.entries[1].picture),
            (1, &None)
        );
        // The icon comes first once the project has one.
        tiny_png(&home.join("logo.png"));
        project::set_icon_in(&home.join("Here"), Some(&home.join("logo.png"))).unwrap();
        selector.refresh(&settings);
        assert_eq!(selector.entries[0].picture, Some(home.join("Here/Icon.png")));
        assert!(selector.entries[0].icon);
        project::set_icon_in(&home.join("Here"), None).unwrap();
        selector.refresh(&settings);
        assert!(!selector.entries[0].icon);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn pictures_fill_or_fit_their_frames() {
        // A picture of the frame's shape is shown whole.
        assert_eq!(cover_uv(Vec2::new(640.0, 360.0), Vec2::new(256.0, 144.0)), WHOLE);
        // A wider one loses its sides, a taller one mostly its foot.
        let wide = cover_uv(Vec2::new(400.0, 100.0), Vec2::new(100.0, 100.0));
        assert_eq!((wide.left(), wide.right(), wide.height()), (0.375, 0.625, 1.0));
        let tall = cover_uv(Vec2::new(100.0, 200.0), Vec2::new(100.0, 100.0));
        assert_eq!((tall.width(), tall.height()), (1.0, 0.5));
        assert!(tall.top() > 0.0 && tall.top() < 0.25);
        // An icon keeps its shape inside the frame.
        let frame = Rect::from_min_size(Pos2::ZERO, Vec2::new(256.0, 144.0));
        let fit = contain_rect(Vec2::new(512.0, 512.0), frame);
        assert_eq!(fit.size(), Vec2::new(144.0, 144.0));
        assert_eq!(fit.center(), frame.center());
    }

    #[test]
    fn removing_an_entry_takes_it_off_the_settings_and_keeps_the_files() {
        let home = scratch("remove");
        let mut settings = recent(&home);
        let mut selector = Selector::new(&settings);
        assert_eq!(selector.selected, Some(0));
        selector.selected = Some(1);
        selector.remove(&mut settings, &home.join("Gone"));
        assert_eq!(selector.entries.len(), 2);
        assert_eq!(settings.recent_projects.len(), 2);
        // The selection stays on its project.
        assert_eq!(selector.chosen().map(|e| e.name.as_str()), Some("Bare"));
        selector.remove(&mut settings, &home.join("Bare"));
        assert_eq!(selector.chosen().map(|e| e.name.as_str()), Some("Here: one"));
        assert!(Project::exists(&home.join("Bare")));
        // A list of missing projects alone still selects one, to remove it.
        let mut gone = Settings::default();
        gone.opened_at(&home.join("Gone"), 1);
        assert_eq!(Selector::new(&gone).selected, Some(0));
        assert_eq!(Selector::new(&Settings::default()).selected, None);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_search_looks_at_names_and_folders() {
        let home = scratch("search");
        let settings = recent(&home);
        let mut selector = Selector::new(&settings);
        assert_eq!(matching(&selector.entries, ""), [0, 1, 2]);
        assert_eq!(matching(&selector.entries, " HERE: "), [0]);
        assert_eq!(matching(&selector.entries, "bare"), [1]);
        assert_eq!(matching(&selector.entries, "renkatsu-selector-search").len(), 3);
        assert!(matching(&selector.entries, "nothing like it").is_empty());
        // The selection follows what the search leaves.
        selector.search = "gone".to_owned();
        assert_eq!(selector.shown(), [2]);
        assert_eq!(selector.selected, Some(2));
        selector.search = "zzz".to_owned();
        assert!(selector.shown().is_empty());
        assert_eq!(selector.selected, None);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn times_read_as_words_and_then_as_dates() {
        let now = 1_794_787_200; // 16 Nov 2026
        assert_eq!(ago(now, 0), "");
        assert_eq!(ago(now, now - 5), "Just now");
        assert_eq!(ago(now, now + 60), "Just now");
        assert_eq!(ago(now, now - 60), "1 minute ago");
        assert_eq!(ago(now, now - 59 * 60), "59 minutes ago");
        assert_eq!(ago(now, now - 3 * 3600), "3 hours ago");
        assert_eq!(ago(now, now - 30 * 3600), "Yesterday");
        assert_eq!(ago(now, now - 3 * 86_400), "3 days ago");
        assert_eq!(ago(now, now - 8 * 86_400), "1 week ago");
        assert_eq!(ago(now, now - 20 * 86_400), "2 weeks ago");
        assert_eq!(ago(now, now - 60 * 86_400), "17 Sep 2026");
        assert_eq!(date(0), "1 Jan 1970");
        assert_eq!(date(951_782_400), "29 Feb 2000");
        assert_eq!(date(now), "16 Nov 2026");
    }

    #[test]
    fn animations_end_where_they_should() {
        assert_eq!((ease_out(0.0), ease_out(1.0), ease_out(7.0)), (0.0, 1.0, 1.0));
        assert!(ease_out(0.5) > 0.5);
        let (a, b) = (Color32::from_rgb(10, 20, 30), Color32::from_rgb(200, 100, 0));
        assert_eq!((mix(a, b, 0.0), mix(a, b, 1.0), mix(a, b, 3.0)), (a, b, b));
    }

    #[test]
    fn the_selector_opens_and_makes_projects_and_the_editor_returns_to_it() {
        let home = scratch("state");
        let settings = recent(&home);
        let mut st = State::new(Project::none(), Scene::new());
        // Nothing here may reach the user's own settings file.
        st.scripted = true;
        st.settings = settings;
        st.show_selector();
        assert!(st.project.is_none() && !st.dirty());
        assert_eq!(st.selector.as_ref().unwrap().entries.len(), 3);
        // Nothing is saved while no project is open.
        assert!(!st.save());

        // A project that is gone stays in the selector, with the reason.
        st.open_or_tell(&home.join("Gone"));
        assert!(st.selector.as_ref().unwrap().problem.is_some());
        st.open_or_tell(&home.join("Here"));
        assert!(st.selector.is_none());
        assert_eq!(st.project.file.name, "Here: one");

        st.show_selector();
        assert!(st.project.is_none() && st.selector.is_some());

        // An empty project starts with the key light alone.
        st.new_project(&home.join("Empty"), "Empty", Start::Empty).unwrap();
        assert!(st.selector.is_none());
        assert_eq!(*st.doc.scene(), Scene::new());
        assert!(st.library.characters.is_empty());
        assert!(!st.dirty());
        assert!(Project::exists(&home.join("Empty")));
        // A folder in use is refused, and the project stays open.
        assert!(st.new_project(&home.join("Empty"), "Again", Start::Empty).is_err());
        assert_eq!(st.project.file.name, "Empty");

        // What is added to the scene is selected and waits for the camera.
        st.run(crate::state::Command::Add(crate::kinds::Kind::Light));
        assert!(st.focus_pending.is_some());
        assert_eq!(st.focus_pending, st.doc.selected);
        let light = st.doc.selected;
        st.run(crate::state::Command::Duplicate);
        assert!(st.focus_pending.is_some() && st.focus_pending != light);
        assert_eq!(st.focus_pending, st.doc.selected);
        // Leaving the project lets go of it.
        st.show_selector();
        assert_eq!(st.focus_pending, None);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_new_project_form_names_a_free_folder() {
        let home = scratch("form");
        let form = ProjectForm {
            save_as: false,
            name: "A: b".to_owned(),
            folder: home.clone(),
            start: Start::default(),
            problem: None,
        };
        assert_eq!(form.start, Start::RenkatsuChan);
        assert_eq!(form.root(), home.join("A_ b"));
        assert!(form.ready());
        assert!(!ProjectForm {
            name: "  ".to_owned(),
            ..form
        }
        .ready());
        let _ = std::fs::remove_dir_all(&home);
    }
}
