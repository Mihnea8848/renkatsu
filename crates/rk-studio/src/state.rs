// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! What the shell remembers between frames: the look in use, the selection,
//! the state of the viewport's overlays, the project that is open, its
//! scene being edited and the commands that change them.

use crate::design::{Spec, TitleBar};
use crate::dialogs::{After, Import, Modal, PendingImport, Pick, Picker, ProjectForm};
use crate::kinds::Kind;
use crate::library::Library;
use crate::mods::{self, Games};
use crate::project::{self, Project, Start};
use crate::scene::{Content, Document, ObjectId, Scene};
use crate::selector::Selector;
use crate::theme::{Theme, ICONS, SEMIBOLD};
use anyhow::Result;
use egui::{Color32, FontFamily, FontId, TextureHandle};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

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
    /// Add the character of one of the project's cards, named by its path
    /// relative to the project.
    AddCard(PathBuf),
    /// Save the scene and the project.
    Save,
    /// Save a copy of the project under another name and work on that.
    SaveAs,
    NewProject,
    /// Open a project: the one named, or one the user picks.
    OpenProject(Option<PathBuf>),
    /// Pick a card and bring it into the project.
    ImportCard,
    /// Close the project and go back to the project selector.
    ProjectSelector,
    /// Pick a picture as the project's icon.
    SetIcon,
    Quit,
}

/// The card a new project starts with, as Blender starts with a cube, and
/// the name its character goes by.
const DEFAULT_CARD: &str = "Simple.png";
const DEFAULT_NAME: &str = "Renkatsu-Chan";

pub struct State {
    /// The window's title bar.
    pub title_bar: TitleBar,
    /// When the fly speed last changed, in egui's seconds, for the meter
    /// that shows it and fades away.
    pub speed_shown: Option<f64>,
    /// The project that is open; `Project::none()` while the project
    /// selector shows.
    pub project: Project,
    /// The project selector, while it shows in place of the editor.
    pub selector: Option<Selector>,
    /// The object that was just added: the camera goes to it once the
    /// viewport has something of it to look at.
    pub focus_pending: Option<ObjectId>,
    /// The scene being edited, with the selection and the undo history.
    pub doc: Document,
    /// The scene as it was last saved, to tell unsaved changes by.
    saved: Scene,
    /// The game's files, opened with the project's mods.
    pub games: Games,
    /// The question that is open over the window.
    pub modal: Option<Modal>,
    /// The system file dialog that is open.
    pub picker: Option<Picker>,
    /// Cards waiting to be imported, one after the other.
    pub imports: VecDeque<Import>,
    /// The editor runs from a script: it asks nothing and leaves the
    /// user's settings as they are.
    pub scripted: bool,
    /// The window may close.
    pub quit: bool,
    /// Folders whose content is folded away in the Outliner.
    pub folded: HashSet<ObjectId>,
    /// The whole scene is folded away in the Outliner.
    pub scene_folded: bool,
    /// The object being renamed in the Outliner, with the text so far.
    pub renaming: Option<(ObjectId, String)>,
    /// The Content panel was asked for and should come to the front.
    pub show_content: bool,
    /// The project's characters, for the Content panel.
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
    /// The folder open in the Content panel: the characters, then the
    /// other kinds of asset.
    pub folder: usize,
    /// The card selected in the Content panel, relative to the project.
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
    /// The editor on a project and its scene.
    pub fn new(project: Project, scene: Scene) -> Self {
        State {
            title_bar: TitleBar::A,
            speed_shown: None,
            library: Library::new(&project),
            project,
            selector: None,
            focus_pending: None,
            saved: scene.clone(),
            doc: Document::new(scene),
            games: Games::default(),
            modal: None,
            picker: None,
            imports: VecDeque::new(),
            scripted: false,
            quit: false,
            folded: HashSet::new(),
            scene_folded: false,
            renaming: None,
            show_content: false,
            log: Log::new(),
            mode: 0,
            mode_tool: [0, 0, 1, 0, 0, 0],
            palette_open: false,
            palette_pinned: true,
            rail: Rail::Named,
            overlays_hidden: false,
            maximised: false,
            tool: 0,
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
                    self.focus_pending = self.doc.duplicate(id);
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
                    self.focus_pending = Some(id);
                }
            }
            Command::AddCard(card) => self.add_card(card),
            Command::Save => {
                self.save();
            }
            // The rest need the window, and go through `ask`.
            Command::SaveAs
            | Command::NewProject
            | Command::OpenProject(_)
            | Command::ImportCard
            | Command::ProjectSelector
            | Command::SetIcon
            | Command::Quit => {}
        }
        if self
            .renaming
            .as_ref()
            .is_some_and(|(id, _)| self.doc.scene().get(*id).is_none())
        {
            self.renaming = None;
        }
    }

    /// Carries out a command that may open a dialog: the ones of the File
    /// menu. Every other command runs as it is.
    pub fn ask(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::SaveAs => {
                let folder = self.project.root.parent().map(Path::to_owned).or_else(project::home);
                self.modal = Some(Modal::Project(ProjectForm {
                    save_as: true,
                    name: format!("{} copy", self.project.file.name),
                    folder: folder.unwrap_or_default(),
                    start: Start::default(),
                    problem: None,
                }));
            }
            Command::ProjectSelector => self.request(After::Selector, ctx),
            Command::SetIcon => self.pick(Pick::Icon, None, ctx),
            Command::NewProject => self.request(After::New, ctx),
            // Open project goes through the project selector, which lists the
            // recent projects and can pick any other.
            Command::OpenProject(None) => self.request(After::Selector, ctx),
            Command::OpenProject(path) => self.request(After::Open(path), ctx),
            Command::Quit => self.request(After::Quit, ctx),
            Command::ImportCard => {
                let cards = rk_core::Config::load()
                    .ok()
                    .map(|c| c.userdata().join("chara").join("female"));
                self.pick(Pick::Card, cards, ctx);
            }
            other => self.run(other),
        }
    }

    /// Whether the scene has changes that are not saved.
    pub fn dirty(&self) -> bool {
        *self.doc.scene() != self.saved
    }

    /// Saves the scene and the project. Returns whether it worked.
    pub fn save(&mut self) -> bool {
        if self.project.is_none() {
            return false;
        }
        match self.project.save_scene(self.doc.scene()) {
            Ok(()) => {
                self.saved = self.doc.scene().clone();
                self.log.info(format!("Saved {}", self.project.file.name));
                self.take_picture();
                true
            }
            Err(e) => {
                self.log.error(format!("The project could not be saved: {e:#}"));
                false
            }
        }
    }

    /// Takes the picture of the scene that the project selector shows for
    /// the project, from the viewport's camera. Without a viewport, as in
    /// tests, the picture stays as it is.
    fn take_picture(&mut self) {
        let Some(live) = self.live.as_mut() else { return };
        if self.project.is_none() {
            return;
        }
        let taken = live
            .snapshot(self.doc.scene(), project::THUMBNAIL_SIZE, [0.22, 0.22, 0.23, 1.0])
            .and_then(|pixels| self.project.set_thumbnail(pixels.as_deref()));
        if let Err(e) = taken {
            self.log
                .warn(format!("The picture of the scene could not be saved: {e:#}"));
        }
    }

    /// The project is about to be left: when the scene is as it was saved,
    /// its picture is taken again, so that it shows characters that were
    /// still building at the save.
    fn leave(&mut self) {
        if !self.dirty() && self.live.as_ref().is_some_and(|live| live.has_characters()) {
            self.take_picture();
        }
    }

    /// Goes on with something unless unsaved changes stand in the way,
    /// when the user is asked about them first.
    fn request(&mut self, after: After, ctx: &egui::Context) {
        if self.dirty() && !self.scripted {
            self.modal = Some(Modal::Unsaved(after));
        } else {
            self.proceed(after, ctx);
        }
    }

    /// Goes on with what the unsaved changes held up.
    pub fn proceed(&mut self, after: After, ctx: &egui::Context) {
        match after {
            After::New => self.modal = Some(Modal::Project(ProjectForm::new())),
            After::Open(Some(path)) => self.open_or_tell(&path),
            After::Open(None) => {
                let start = self.project.root.parent().map(Path::to_owned).or_else(project::home);
                self.pick(Pick::Project, start, ctx);
            }
            After::Selector => self.show_selector(),
            After::Quit => {
                self.leave();
                self.quit = true;
            }
        }
    }

    /// Opens a system file dialog, unless one is open.
    pub fn pick(&mut self, what: Pick, start: Option<PathBuf>, ctx: &egui::Context) {
        if self.picker.is_none() {
            self.picker = Some(Picker::open(what, start, ctx));
        }
    }

    /// Takes what a system file dialog answered, once it closed.
    pub fn poll_picker(&mut self) {
        let Some(answer) = self.picker.as_ref().and_then(Picker::poll) else {
            return;
        };
        let what = self.picker.take().map(|p| p.what);
        let (Some(what), Some(path)) = (what, answer) else {
            return;
        };
        match what {
            Pick::Card => self.imports.push_back(Import {
                path,
                place: false,
                ask: true,
                name: None,
            }),
            Pick::Project => self.open_or_tell(&path),
            Pick::Icon => {
                let set = match self.selector.as_mut().and_then(|s| s.icon_for.take()) {
                    Some(root) => project::set_icon_in(&root, Some(&path)),
                    None if self.project.is_none() => Ok(()),
                    None => self.project.set_icon(Some(&path)),
                };
                match (set, &mut self.selector) {
                    (Ok(()), Some(selector)) => selector.refresh(&self.settings),
                    (Ok(()), None) => self.log.info(format!("{} is the project's icon now", path.display())),
                    (Err(e), Some(selector)) => selector.problem = Some(format!("{e:#}")),
                    (Err(e), None) => self.log.error(format!("The icon could not be set: {e:#}")),
                }
            }
            Pick::GameFolder => match rk_core::Config::save_user(&path) {
                Ok(cfg) => {
                    self.log
                        .info(format!("Characters are read from {} now", cfg.game_dir.display()));
                    if let Some(selector) = &mut self.selector {
                        selector.game = Some(cfg.game_dir);
                        selector.problem = None;
                    }
                }
                Err(e) => match &mut self.selector {
                    Some(selector) => selector.problem = Some(format!("{e:#}")),
                    None => self.log.error(format!("{e:#}")),
                },
            },
            Pick::Folder => {
                let form = match (&mut self.modal, &mut self.selector) {
                    (Some(Modal::Project(form)), _) => Some(form),
                    (None, Some(selector)) => Some(&mut selector.form),
                    _ => None,
                };
                if let Some(form) = form {
                    form.folder = path;
                    form.problem = None;
                }
            }
        }
    }

    /// Makes another project the open one, with its scene.
    fn adopt(&mut self, project: Project, scene: Scene) {
        self.leave();
        self.focus_pending = None;
        self.project = project;
        self.saved = scene.clone();
        self.doc = Document::new(scene);
        let eager = self.library.eager;
        self.library = Library::new(&self.project);
        self.library.eager = eager;
        self.folded.clear();
        self.scene_folded = false;
        self.renaming = None;
        self.asset = None;
        self.folder = 0;
        self.imports.clear();
        self.selector = None;
        if let Some(live) = self.live.as_mut() {
            live.reset();
        }
        self.remember();
    }

    /// Closes the project and shows the project selector in its place.
    pub fn show_selector(&mut self) {
        self.adopt(Project::none(), Scene::new());
        self.modal = None;
        self.preferences_open = false;
        let mut selector = Selector::new(&self.settings);
        selector.eager = self.scripted;
        self.selector = Some(selector);
    }

    /// Opens a project and, when that fails, says so where the user looks:
    /// in the project selector while it shows, else in the console.
    pub fn open_or_tell(&mut self, path: &Path) {
        if let Err(e) = self.open_project(path) {
            let problem = format!("{} could not be opened: {e:#}", path.display());
            match &mut self.selector {
                Some(selector) => selector.problem = Some(problem),
                None => self.log.error(problem),
            }
        }
    }

    /// Puts the open project at the head of the recent ones.
    pub fn remember(&mut self) {
        if self.scripted || self.project.is_none() {
            return;
        }
        self.settings.opened(&self.project.root);
        if let Err(e) = self.settings.save() {
            self.log
                .warn(format!("The list of recent projects could not be saved: {e:#}"));
        }
    }

    /// Opens the project in a folder.
    pub fn open_project(&mut self, path: &Path) -> Result<()> {
        let (project, scene, note) = Project::open(path)?;
        self.log.info(format!("Opened {}", project.file.name));
        if let Some(note) = note {
            self.log.warn(note);
        }
        self.adopt(project, scene);
        Ok(())
    }

    /// Makes a new project in a folder and opens it: the key light and,
    /// when the start asks for her and the game has her, the default
    /// character in the scene.
    pub fn new_project(&mut self, root: &Path, name: &str, start: Start) -> Result<()> {
        let project = Project::create(root, name)?;
        self.adopt(project, Scene::new());
        self.start_fresh(start == Start::RenkatsuChan);
        Ok(())
    }

    /// Fills the project that was just made: the default character joins
    /// the key light, unless the caller brings characters of its own, and
    /// the result is saved as where the project starts.
    pub fn start_fresh(&mut self, default_character: bool) {
        self.log.info(format!(
            "Made {} in {}",
            self.project.file.name,
            self.project.root.display()
        ));
        if default_character {
            self.add_default_character();
        }
        // What a new project starts with is where its history starts.
        self.doc.clear_history();
        self.save();
    }

    /// Saves a copy of the project in another folder and goes on there.
    pub fn save_as(&mut self, root: &Path, name: &str) -> Result<()> {
        let scene = self.doc.scene().clone();
        let mut copy = self.project.copy_to(root, name)?;
        copy.save_scene(&scene)?;
        self.log.info(format!("Saved {name} in {}", root.display()));
        let selected = self.doc.selected;
        self.adopt(copy, scene);
        self.doc.selected = selected;
        Ok(())
    }

    /// Imports the game's default card as Renkatsu-Chan and puts her in
    /// the scene. The card is read from the user's own game; without it
    /// the project starts with the key light alone.
    fn add_default_character(&mut self) {
        let card = match rk_core::Config::load() {
            Ok(config) => config.userdata().join("chara").join("female").join(DEFAULT_CARD),
            Err(e) => {
                self.log
                    .warn(format!("The project starts without {DEFAULT_NAME}: {e:#}"));
                return;
            }
        };
        if !card.is_file() {
            self.log.warn(format!(
                "The project starts without {DEFAULT_NAME}: the game has no {}",
                card.display()
            ));
            return;
        }
        self.begin_import(Import {
            path: card,
            place: true,
            ask: false,
            name: Some(DEFAULT_NAME.to_owned()),
        });
    }

    /// Starts the next import that waits, once no dialog is open.
    pub fn next_import(&mut self) {
        if self.modal.is_none() {
            if let Some(import) = self.imports.pop_front() {
                self.begin_import(import);
            }
        }
    }

    /// Brings a card into the project: works out the mods it needs and
    /// either asks about copying them or goes ahead.
    fn begin_import(&mut self, import: Import) {
        let card = match rk_card::Card::load(&import.path) {
            Ok(card) => card,
            Err(e) => {
                self.log.error(format!(
                    "{} is not a card that can be read: {e:#}",
                    import.path.display()
                ));
                return;
            }
        };
        let stem = import.path.file_stem().map(|s| s.to_string_lossy().into_owned());
        let title = import
            .name
            .clone()
            .or_else(|| Some(card.full_name()).filter(|n| !n.trim().is_empty()))
            .or(stem)
            .unwrap_or_else(|| "Character".to_owned());
        let mods_dir = self.project.mods_dir();
        let (plan, missing) = match self.games.get(&mods_dir) {
            Ok(game) => {
                let needed = mods::needed(game, &card);
                (mods::plan(&needed.mods, &mods::in_folder(&mods_dir)), needed.missing)
            }
            Err(e) => {
                self.log
                    .warn(format!("The mods {title} needs could not be worked out: {e:#}"));
                (mods::Plan::default(), Vec::new())
            }
        };
        let ask = import.ask && self.project.file.ask_before_copying_mods;
        let pending = PendingImport {
            import,
            title,
            plan,
            missing,
            dont_ask: false,
        };
        if ask && !pending.plan.copy.is_empty() {
            self.modal = Some(Modal::Import(pending));
        } else {
            self.finish_import(pending);
        }
    }

    /// The user agreed to an import and the mods it copies.
    pub fn confirm_import(&mut self, pending: PendingImport) {
        if pending.dont_ask {
            self.project.file.ask_before_copying_mods = false;
        }
        self.finish_import(pending);
    }

    /// Copies an import's mods and its card into the project, and puts
    /// the character in the scene when that was asked for.
    fn finish_import(&mut self, pending: PendingImport) {
        let PendingImport {
            import,
            title,
            plan,
            missing,
            ..
        } = pending;
        if !plan.copy.is_empty() {
            match mods::copy(&plan, &self.project.mods_dir()) {
                Ok(()) => self.log.info(format!(
                    "Copied {} for {title} into the project: {}",
                    mods::count_text(plan.copy.len()),
                    mods::size_text(plan.bytes())
                )),
                Err(e) => self
                    .log
                    .error(format!("The mods of {title} could not be copied: {e:#}")),
            }
        }
        for guid in &missing {
            self.log
                .warn(format!("{title} names the mod {guid}, which is not installed"));
        }
        let file = match self.project.import_card(&import.path, &title) {
            Ok(file) => file,
            Err(e) => {
                self.log.error(format!("{title} could not be imported: {e:#}"));
                return;
            }
        };
        if let Err(e) = self.project.save() {
            self.log.error(format!("The project file could not be written: {e:#}"));
        }
        self.library.refresh(&self.project);
        self.log.info(format!("Imported {title} as {file}"));
        let file = PathBuf::from(file);
        self.asset = Some(file.clone());
        if import.place {
            self.add_card(file);
        } else {
            self.show_content = true;
        }
    }

    /// Adds the character of one of the project's cards to the scene,
    /// under the name the card has in the project.
    fn add_card(&mut self, card: PathBuf) {
        let listed = self.library.characters.iter().find(|c| c.file == card);
        let name = match listed {
            Some(entry) => entry.name.clone(),
            None => {
                self.log
                    .error(format!("{} is not one of the project's cards", card.display()));
                return;
            }
        };
        let id = self.doc.add_character(&name, card);
        self.unfold_to(id);
        // The camera goes to her once she is built.
        self.focus_pending = Some(id);
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
