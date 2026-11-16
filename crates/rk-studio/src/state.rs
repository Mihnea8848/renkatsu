// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! What the shell remembers between frames: the look in use, the selection,
//! the state of the viewport's overlays, the scene being edited and the
//! commands that change it.

use crate::design::{Spec, TitleBar};
use crate::kinds::Kind;
use crate::library::Library;
use crate::scene::{Content, Document, ObjectId, Scene};
use crate::theme::{Theme, ICONS, SEMIBOLD};
use egui::{Color32, FontFamily, FontId, TextureHandle};
use std::collections::HashSet;
use std::path::PathBuf;

/// The theme and the chosen design, passed to everything that draws.
#[derive(Debug, Clone, Copy)]
pub struct Look {
    pub t: Theme,
    pub d: Spec,
}

impl Look {
    /// Body text.
    pub fn font(&self) -> FontId {
        FontId::proportional(self.d.text)
    }

    /// Captions and secondary text.
    pub fn small(&self) -> FontId {
        FontId::proportional(self.d.small)
    }

    pub fn bold(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
    }

    pub fn icons(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(ICONS.into()))
    }

    pub fn mono(&self) -> FontId {
        FontId::monospace(self.d.text - 1.0)
    }

    pub fn radius(&self) -> f32 {
        f32::from(self.t.radius)
    }

    /// Height of fields and buttons inside a row.
    pub fn field(&self) -> f32 {
        self.d.row - 2.0
    }
}

/// How much of the mode rail shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rail {
    /// Icons with the mode names.
    Named,
    /// Icons only, the names in tooltips.
    Icons,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Physics {
    Stopped,
    Playing,
    Paused,
}

/// The console's lines: a level (0 information, 1 warning, 2 error), the
/// time since the editor started and the text.
pub struct Log {
    start: std::time::Instant,
    pub lines: Vec<(usize, String, String)>,
}

impl Log {
    pub fn new() -> Log {
        Log {
            start: std::time::Instant::now(),
            lines: Vec::new(),
        }
    }

    fn line(&mut self, level: usize, text: String) {
        let seconds = self.start.elapsed().as_secs();
        let time = format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60);
        self.lines.push((level, time, text));
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.line(0, text.into());
    }

    pub fn warn(&mut self, text: impl Into<String>) {
        self.line(1, text.into());
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.line(2, text.into());
    }
}

/// Something the menus, the keys and the panels ask of the scene.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Undo,
    Redo,
    /// Delete the selection.
    Delete,
    /// Copy the selection.
    Duplicate,
    /// Hide the selection, or show it again.
    ToggleHidden,
    /// Start renaming the selection in the Outliner.
    Rename,
    /// Add an object of a kind. A character needs a card, so asking for
    /// one shows the cards instead.
    Add(Kind),
    /// Add the character of a card.
    AddCard(PathBuf),
}

pub struct State {
    /// The window's title bar.
    pub title_bar: TitleBar,
    /// When the fly speed last changed, in egui's seconds, for the meter
    /// that shows it and fades away.
    pub speed_shown: Option<f64>,
    /// The scene being edited, with the selection and the undo history.
    pub doc: Document,
    /// Folders whose content is folded away in the Outliner.
    pub folded: HashSet<ObjectId>,
    /// The whole scene is folded away in the Outliner.
    pub scene_folded: bool,
    /// The object being renamed in the Outliner, with the text so far.
    pub renaming: Option<(ObjectId, String)>,
    /// The Content panel was asked for and should come to the front.
    pub show_content: bool,
    /// The cards on disk, for the Content panel.
    pub library: Library,
    pub log: Log,

    /// Index into `modes::MODES`.
    pub mode: usize,
    /// The active tool of each mode, counted through its sections.
    pub mode_tool: [usize; 6],
    /// The active mode's tool palette is open.
    pub palette_open: bool,
    /// The palette stays open while working in the viewport.
    pub palette_pinned: bool,
    pub rail: Rail,
    /// Every viewport overlay is hidden.
    pub overlays_hidden: bool,
    /// The viewport takes the place of all docked panels.
    pub maximised: bool,
    /// Index into `modes::TOOLS`.
    pub tool: usize,
    pub snap: bool,
    pub physics: Physics,

    /// Properties sections that are open.
    pub sections: [bool; 5],

    pub scene_search: String,
    pub property_search: String,
    pub content_search: String,
    pub console_search: String,
    pub command_search: String,
    /// The folder open in the Content panel, counted through the card
    /// folders and then the other kinds of asset.
    pub folder: usize,
    /// The card selected in the Content panel.
    pub asset: Option<PathBuf>,
    /// Console levels shown: information, warnings, errors.
    pub levels: [bool; 3],

    pub viewport_image: Option<TextureHandle>,
    /// The scene as the viewport draws it, when the window has a GPU
    /// device to draw with.
    pub live: Option<crate::live::Live>,
    /// The physics state the last frame saw, to reset on a stop.
    pub last_physics: Physics,
    /// What surrounds the still picture, taken from its corner so that it
    /// reads as a viewport filled edge to edge.
    pub backdrop: Color32,
    /// The user's settings, saved when the Preferences window changes them.
    pub settings: crate::settings::Settings,
    /// Whether the window waits for the display, as chosen when it opened.
    pub vsync_at_start: bool,
    pub preferences_open: bool,
    /// Seconds per frame, smoothed, for the status bar.
    pub frame_time: f32,
}

impl State {
    pub fn new() -> Self {
        State {
            title_bar: TitleBar::A,
            speed_shown: None,
            doc: Document::new(Scene::new()),
            folded: HashSet::new(),
            scene_folded: false,
            renaming: None,
            show_content: false,
            library: Library::new(),
            log: Log::new(),
            mode: 2,
            mode_tool: [0, 0, 1, 0, 0, 0],
            palette_open: true,
            palette_pinned: true,
            rail: Rail::Named,
            overlays_hidden: false,
            maximised: false,
            tool: 2,
            snap: true,
            physics: Physics::Stopped,
            sections: [true, true, true, false, true],
            scene_search: String::new(),
            property_search: String::new(),
            content_search: String::new(),
            console_search: String::new(),
            command_search: String::new(),
            folder: 0,
            asset: None,
            levels: [true; 3],
            viewport_image: None,
            live: None,
            last_physics: Physics::Stopped,
            backdrop: Color32::from_rgb(0x0f, 0x0f, 0x0f),
            settings: Default::default(),
            vsync_at_start: true,
            preferences_open: false,
            frame_time: 0.0,
        }
    }

    /// The kind and name of the selected object.
    pub fn selection(&self) -> Option<(Kind, String)> {
        self.doc.selection().map(|o| (o.kind(), o.name.clone()))
    }

    /// Carries out a command on the scene.
    pub fn run(&mut self, command: Command) {
        let selected = self.doc.selected;
        match command {
            Command::Undo => {
                self.doc.undo();
            }
            Command::Redo => {
                self.doc.redo();
            }
            Command::Delete => {
                if let Some(id) = selected {
                    self.doc.delete(id);
                }
            }
            Command::Duplicate => {
                if let Some(id) = selected {
                    self.doc.duplicate(id);
                }
            }
            Command::ToggleHidden => {
                if let Some((id, visible)) = self.doc.selection().map(|o| (o.id, o.visible)) {
                    self.doc.set_visible(id, !visible);
                }
            }
            Command::Rename => {
                self.renaming = self.doc.selection().map(|o| (o.id, o.name.clone()));
            }
            Command::Add(Kind::Character) => self.show_content = true,
            Command::Add(kind) => {
                if let Some(content) = Content::of_kind(kind) {
                    let name = match kind {
                        Kind::Folder => "Folder",
                        Kind::Light => "Light",
                        Kind::Camera => "Camera",
                        Kind::Item => "Item",
                        _ => "Map",
                    };
                    let id = self.doc.add(name, content);
                    self.unfold_to(id);
                }
            }
            Command::AddCard(card) => self.add_card(card),
        }
        if self
            .renaming
            .as_ref()
            .is_some_and(|(id, _)| self.doc.scene().get(*id).is_none())
        {
            self.renaming = None;
        }
    }

    /// Adds the character of a card to the scene, named after the card.
    fn add_card(&mut self, card: PathBuf) {
        let stem = card.file_stem().map(|s| s.to_string_lossy().into_owned());
        let name = match rk_card::Card::load(&card) {
            Ok(data) => Some(data.full_name()).filter(|n| !n.is_empty()).or(stem),
            Err(e) => {
                self.log
                    .error(format!("{} is not a card that can be read: {e:#}", card.display()));
                return;
            }
        };
        let id = self.doc.add_character(name.as_deref().unwrap_or("Character"), card);
        self.unfold_to(id);
    }

    /// Opens the folders around an object so that its row shows.
    fn unfold_to(&mut self, id: ObjectId) {
        self.scene_folded = false;
        let mut at = self.doc.scene().get(id).and_then(|o| o.parent);
        while let Some(folder) = at {
            self.folded.remove(&folder);
            at = self.doc.scene().get(folder).and_then(|o| o.parent);
        }
    }

    /// Switches to a mode; choosing the active mode again folds its tools.
    pub fn pick_mode(&mut self, mode: usize) {
        if self.mode == mode {
            self.palette_open = !self.palette_open;
        } else {
            self.mode = mode;
            self.palette_open = true;
        }
    }
}
