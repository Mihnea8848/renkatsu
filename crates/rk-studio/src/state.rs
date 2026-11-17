// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! What the shell remembers between frames: the look in use, the selection,
//! the state of the viewport's overlays, the project that is open, its
//! scene being edited and the commands that change them.

use crate::content;
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

/// Whether the scene's time runs: the one transport for everything in a
/// scene that takes time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
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
    /// Give the selected character an animation, or take its animation
    /// off.
    Animate(Option<crate::scene::Animation>),
    /// Choose animations of the installed packages, of Koikatsu and of
    /// mods, and copy them into the project.
    ImportAnimations,
    /// Pick an animation file or a zipmod and copy animations from it
    /// into the project.
    ImportAnimationFile,
    /// Something done to a file or a folder of the project.
    File(FileOp),
    /// Copy the selection with everything below it, to paste.
    Copy,
    /// Copy the selection, then delete it.
    Cut,
    /// Paste what was copied beside the selection.
    Paste,
    /// Take the selection out of its folder, to the top of the scene.
    Unparent,
    /// Add a folder inside the selected folder.
    AddChildFolder,
    /// Select everything directly inside the selected folder.
    SelectChildren,
    /// Turn the camera to the selection.
    Focus,
    /// Save the scene and the project.
    Save,
    /// Save a copy of the project under another name and work on that.
    SaveAs,
    NewProject,
    /// Open a project: the one named, or one the user picks.
    OpenProject(Option<PathBuf>),
    /// Pick a card and bring it into the project.
    ImportCard,
    /// Choose cards of the installed packages and bring them into the
    /// project.
    ImportFromPackage,
    /// Show the Packages window.
    Packages,
    /// Close the project and go back to the project selector.
    ProjectSelector,
    /// Pick a picture as the project's icon.
    SetIcon,
    Quit,
}

/// What the Content panel does to a file or a folder of the project. Paths
/// are relative to the project, with forward slashes.
#[derive(Debug, Clone, PartialEq)]
pub enum FileOp {
    /// Give it another name.
    Rename(String, String),
    /// Move it into a folder.
    Move(String, String),
    Duplicate(String),
    /// Move it to the project's trash.
    Delete(String),
    /// Make a folder inside this one, and start naming it.
    NewFolder(String),
    Copy(String),
    Cut(String),
    /// Paste what was copied or cut into this folder.
    Paste(String),
    /// Show it in the system's file manager.
    Reveal(String),
    /// Make a picture the project's icon.
    SetIcon(String),
    /// Copy a linked animation into the project for good.
    CopyIn(String),
}

/// The card a new project starts with, as Blender starts with a cube, and
/// the name its character goes by.
const DEFAULT_CARD: &str = "Simple.png";
const DEFAULT_NAME: &str = "Renkatsu-Chan";

/// The card new projects start with: the default card of the game when
/// the game has it, else the one a package brings.
pub fn default_card(game: Option<&rk_core::Config>, package: Option<PathBuf>) -> Option<PathBuf> {
    game.map(|c| c.userdata().join("chara").join("female").join(DEFAULT_CARD))
        .filter(|card| card.is_file())
        .or(package)
}

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
    /// The user's installed packages, with their windows.
    pub packages: crate::packages::Packages,
    /// The question that is open over the window.
    pub modal: Option<Modal>,
    /// The system file dialog that is open.
    pub picker: Option<Picker>,
    /// Cards waiting to be imported, one after the other.
    pub imports: VecDeque<Import>,
    /// The editor runs from a script: it asks nothing and leaves the
    /// user's settings as they are.
    pub scripted: bool,
    /// A newer Renkatsu on GitHub, when the check at start found one.
    pub update: Option<rk_packages::update::Release>,
    /// The check for a newer Renkatsu while it runs.
    pub update_check: Option<std::sync::mpsc::Receiver<Option<rk_packages::update::Release>>>,
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
    /// The folder open in the Content panel, relative to the project: one
    /// of the project's folders or a folder the user made below one.
    pub subfolder: String,
    /// The file or folder being renamed in the Content panel, with the
    /// text so far.
    pub content_renaming: Option<(String, String)>,
    /// The file or folder that was copied or cut in the Content panel,
    /// and whether it was cut.
    pub clipboard: Option<(String, bool)>,
    /// The objects that were copied in the Outliner.
    pub scene_clipboard: Vec<crate::scene::Object>,
    /// The window that imports animations, while it shows.
    pub importer: Option<crate::anim_import::Importer>,
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
    /// What the play, pause and stop buttons set: whether the scene's
    /// time runs. Animations and physics both go by it.
    pub playback: Playback,
    /// Seconds the scene has played since it was last stopped.
    pub clock: f32,

    /// Properties sections that are open.
    pub sections: [bool; 7],
    /// The project's animations, for the Content panel and Properties.
    pub animations: crate::animations::Shelf,

    pub scene_search: String,
    pub property_search: String,
    pub content_search: String,
    pub console_search: String,
    pub command_search: String,
    /// The project folder open in the Content panel, as an index into
    /// `project::FOLDERS`.
    pub folder: usize,
    /// The file selected in the Content panel, relative to the project.
    pub asset: Option<PathBuf>,
    /// Console levels shown: information, warnings, errors.
    pub levels: [bool; 3],

    pub viewport_image: Option<TextureHandle>,
    /// The scene as the viewport draws it, when the window has a GPU
    /// device to draw with.
    pub live: Option<crate::live::Live>,
    /// The transport as the last frame saw it, to reset on a stop.
    pub last_playback: Playback,
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
    /// Asks GitHub, on a thread of its own, whether a newer Renkatsu is
    /// out. Nothing waits for the answer.
    pub fn check_for_update(&mut self, ctx: &egui::Context) {
        let (send, answer) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let newer = rk_packages::update::latest()
                .ok()
                .and_then(|r| rk_packages::update::newer(r, env!("CARGO_PKG_VERSION")));
            let _ = send.send(newer);
            ctx.request_repaint();
        });
        self.update_check = Some(answer);
    }

    /// Takes the answer of the update check once it came.
    pub fn poll_update(&mut self) {
        let Some(check) = &self.update_check else { return };
        if let Ok(newer) = check.try_recv() {
            if let Some(release) = &newer {
                self.log
                    .info(format!("Renkatsu {} is out: {}", release.version, release.url));
            }
            self.update = newer;
            self.update_check = None;
        }
    }

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
            packages: crate::packages::Packages::new(),
            modal: None,
            picker: None,
            imports: VecDeque::new(),
            scripted: false,
            update: None,
            update_check: None,
            quit: false,
            folded: HashSet::new(),
            scene_folded: false,
            renaming: None,
            show_content: false,
            subfolder: project::FOLDERS[0].to_owned(),
            content_renaming: None,
            clipboard: None,
            scene_clipboard: Vec::new(),
            importer: None,
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
            playback: Playback::Stopped,
            clock: 0.0,
            sections: [true, true, true, false, true, true, true],
            animations: Default::default(),
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
            last_playback: Playback::Stopped,
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
                let all = self.doc.selected_all();
                self.doc.delete_many(&all);
            }
            Command::Duplicate => {
                if let Some(id) = selected {
                    self.focus_pending = self.doc.duplicate(id);
                }
            }
            Command::ToggleHidden => {
                if let Some(visible) = self.doc.selection().map(|o| o.visible) {
                    let all = self.doc.selected_all();
                    self.doc.set_visible_many(&all, !visible);
                }
            }
            Command::Copy => {
                if let Some(id) = selected {
                    self.scene_clipboard = self.doc.copy(id);
                }
            }
            Command::Cut => {
                if let Some(id) = selected {
                    self.scene_clipboard = self.doc.copy(id);
                    self.doc.delete(id);
                }
            }
            Command::Paste => {
                let copied = self.scene_clipboard.clone();
                if let Some(id) = self.doc.paste(&copied) {
                    self.unfold_to(id);
                    self.focus_pending = Some(id);
                }
            }
            Command::Unparent => {
                let all = self.doc.selected_all();
                self.doc.place_many(&all, crate::scene::Place::Into(None));
            }
            Command::AddChildFolder => {
                let inside = self.doc.selection().filter(|o| o.kind() == Kind::Folder).map(|o| o.id);
                let id = self.doc.add_in("Folder", Content::Folder, inside);
                self.unfold_to(id);
                self.renaming = self.doc.scene().get(id).map(|o| (id, o.name.clone()));
            }
            Command::SelectChildren => {
                if let Some(id) = selected {
                    let children = self.doc.scene().children(Some(id));
                    if !children.is_empty() {
                        self.folded.remove(&id);
                        self.doc.select_many(&children);
                    }
                }
            }
            Command::Focus => self.focus_pending = selected,
            Command::File(op) => self.file_op(op),
            Command::Rename => {
                self.renaming = self.doc.selection().map(|o| (o.id, o.name.clone()));
            }
            Command::Add(Kind::Character) => self.show_folder(0),
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
            Command::Animate(animation) => {
                if let Some(mut object) = self.doc.selection().cloned() {
                    if let Content::Character { animation: worn, .. } = &mut object.content {
                        *worn = animation;
                        self.doc.edit(object, false);
                    }
                }
            }
            Command::Save => {
                self.save();
            }
            // The rest need the window, and go through `ask`.
            Command::SaveAs
            | Command::NewProject
            | Command::OpenProject(_)
            | Command::ImportCard
            | Command::ImportFromPackage
            | Command::ImportAnimations
            | Command::ImportAnimationFile
            | Command::Packages
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
            Command::ImportFromPackage => self.packages.browse(),
            Command::ImportAnimations => self.importer = Some(crate::anim_import::Importer::new(None)),
            Command::ImportAnimationFile => self.pick(Pick::Animation, None, ctx),
            Command::Packages => self.packages.open = true,
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
            Pick::Animation => self.import_animation_file(&path),
            Pick::Package => self.packages.picked = Some(path),
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
                    self.content_changed();
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
        self.animations.refresh(&self.project);
        self.subfolder = project::FOLDERS[0].to_owned();
        self.content_renaming = None;
        self.clipboard = None;
        self.scene_clipboard.clear();
        self.importer = None;
        self.playback = Playback::Stopped;
        self.clock = 0.0;
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
        self.packages.browser = None;
        let mut selector = Selector::new(&self.settings);
        selector.eager = self.scripted;
        selector.set_starter(self.packages.default_card());
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

    /// What characters are built from changed: the Koikatsu folder was
    /// chosen, or a package was installed or removed. The content is
    /// opened again at its next use, and the characters that could not be
    /// built get another try.
    pub fn content_changed(&mut self) {
        self.games.reset();
        self.animations.refresh(&self.project);
        if let Some(importer) = &mut self.importer {
            importer.reset();
        }
        if let Some(live) = self.live.as_mut() {
            live.retry();
        }
    }

    /// A package was installed or removed.
    pub fn packages_changed(&mut self) {
        self.content_changed();
        let card = self.packages.default_card();
        if let Some(selector) = &mut self.selector {
            selector.set_starter(card);
        }
    }

    /// The card new projects start with: the default card of the user's
    /// own game, else the one of Starter Content.
    pub fn default_card(&self) -> Option<PathBuf> {
        default_card(rk_core::Config::load().ok().as_ref(), self.packages.default_card())
    }

    /// Imports the default card as Renkatsu-Chan and puts her in the
    /// scene. Without the game and without Starter Content the project
    /// starts with the key light alone.
    fn add_default_character(&mut self) {
        let Some(card) = self.default_card() else {
            self.log.warn(format!(
                "The project starts without {DEFAULT_NAME}: choose the Koikatsu folder or install Starter Content: \
                 Characters, under Window, Packages"
            ));
            return;
        };
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
            self.show_folder(0);
        }
    }

    /// Brings the Content panel to the front on one of the project's
    /// folders, by its place in `project::FOLDERS`.
    pub fn show_folder(&mut self, folder: usize) {
        self.show_content = true;
        if self.folder != folder || content::top(&self.subfolder) != project::FOLDERS[folder] {
            self.open_folder(project::FOLDERS[folder]);
        }
    }

    /// Opens a folder of the project in the Content panel.
    pub fn open_folder(&mut self, rel: &str) {
        let top = content::top(rel);
        if let Some(at) = project::FOLDERS.iter().position(|f| *f == top) {
            self.folder = at;
            self.subfolder = rel.to_owned();
            self.content_renaming = None;
        }
    }

    /// The folder imported animations go into: the open one when it is
    /// among the animations, else the project's Animations folder.
    pub fn animation_folder(&self) -> String {
        if content::top(&self.subfolder) == project::ANIMATIONS && self.project.root.join(&self.subfolder).is_dir() {
            self.subfolder.clone()
        } else {
            project::ANIMATIONS.to_owned()
        }
    }

    /// Lists the project's files again, after they changed on disk.
    pub fn refresh_content(&mut self) {
        self.library.refresh(&self.project);
        self.animations.refresh(&self.project);
        if !self.project.is_none() && !self.project.root.join(&self.subfolder).is_dir() {
            self.subfolder = project::FOLDERS[self.folder.min(project::FOLDERS.len() - 1)].to_owned();
        }
    }

    /// Copies animations into the project, as the import window asked.
    pub fn import_animations(&mut self, picks: &[crate::animations::Pick], by_category: bool) {
        if self.project.is_none() || picks.is_empty() {
            return;
        }
        let folder = self.animation_folder();
        let mods = self.project.mods_dir();
        let needs_game = picks
            .iter()
            .any(|p| matches!(p.from, crate::animations::From::Installed { .. }));
        let game = if needs_game { self.games.get(&mods).ok() } else { None };
        let done = crate::animations::import(&self.project, game, picks, &folder, by_category);
        self.imported_animations(done, &folder);
    }

    /// Says what an import of animations did and shows the folder they
    /// went into.
    pub fn imported_animations(&mut self, done: crate::animations::Imported, folder: &str) {
        for problem in &done.problems {
            self.log.error(format!("Not imported: {problem}"));
        }
        if !done.files.is_empty() {
            let what = match done.files.len() {
                1 => format!("Imported {}", content::leaf(&done.files[0])),
                n => format!("Imported {n} animations"),
            };
            let size = if done.bytes == 0 {
                "already there".to_owned()
            } else {
                mods::size_text(done.bytes)
            };
            self.log.info(format!("{what} into {folder}: {size}"));
            self.asset = done.files.last().map(PathBuf::from);
        }
        self.refresh_content();
        self.open_folder(folder);
        self.show_content = true;
    }

    /// Imports from a file the user picked: an animation file is copied
    /// in, a zipmod opens the import window on the animations it lists.
    pub fn import_animation_file(&mut self, path: &Path) {
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if extension == rk_chara::animation_file::EXTENSION {
            let pick = crate::animations::Pick {
                name: path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
                group: String::new(),
                category: String::new(),
                source: String::new(),
                from: crate::animations::From::File(path.to_owned()),
            };
            self.import_animations(&[pick], false);
        } else if extension == "zipmod" || extension == "zip" {
            self.importer = Some(crate::anim_import::Importer::new(Some(path.to_owned())));
        } else {
            self.log.error(format!(
                "{} is neither an animation file (.rkanim) nor a zipmod. A package (.rkpkg) is installed under \
                 Window, Packages first; its animations then show under Import from package.",
                path.display()
            ));
        }
    }

    /// Carries out something the Content panel asked of a file or a
    /// folder, and keeps the scene and the panels on what moved.
    pub fn file_op(&mut self, op: FileOp) {
        if self.project.is_none() {
            return;
        }
        let done: Result<Option<content::Moved>> = match op {
            FileOp::Rename(rel, name) => content::rename(&mut self.project, &rel, &name).map(Some),
            FileOp::Move(rel, folder) => content::move_into(&mut self.project, &rel, &folder).map(Some),
            FileOp::Duplicate(rel) => content::duplicate(&mut self.project, &rel).map(|copy| {
                self.asset = Some(PathBuf::from(copy));
                None
            }),
            FileOp::Delete(rel) => {
                let used = self.doc.scene().uses(&rel);
                content::trash(&mut self.project, &rel).map(|()| {
                    let name = content::leaf(&rel);
                    self.log
                        .info(format!("Moved {name} to the project's trash ({})", content::TRASH));
                    if used > 0 {
                        self.log.warn(format!("{used} objects of the scene still name {name}"));
                    }
                    if self.clipboard.as_ref().is_some_and(|(c, _)| *c == rel) {
                        self.clipboard = None;
                    }
                    if self.asset.as_deref() == Some(Path::new(&rel)) {
                        self.asset = None;
                    }
                    None
                })
            }
            FileOp::NewFolder(inside) => content::new_folder(&self.project, &inside, "New folder").map(|folder| {
                self.content_renaming = Some((folder.clone(), content::leaf(&folder).to_owned()));
                None
            }),
            FileOp::Copy(rel) => {
                self.clipboard = Some((rel, false));
                Ok(None)
            }
            FileOp::Cut(rel) => {
                self.clipboard = Some((rel, true));
                Ok(None)
            }
            FileOp::Paste(folder) => match self.clipboard.clone() {
                Some((rel, true)) => {
                    self.clipboard = None;
                    content::move_into(&mut self.project, &rel, &folder).map(Some)
                }
                Some((rel, false)) => content::copy_into(&mut self.project, &rel, &folder).map(|copy| {
                    self.asset = Some(PathBuf::from(copy));
                    None
                }),
                None => Ok(None),
            },
            FileOp::Reveal(rel) => content::reveal(&self.project.root.join(rel)).map(|()| None),
            FileOp::SetIcon(rel) => self.project.set_icon(Some(&self.project.root.join(&rel))).map(|()| {
                self.log
                    .info(format!("{} is the project's icon now", content::leaf(&rel)));
                None
            }),
            FileOp::CopyIn(rel) => self.copy_in(&rel).map(Some),
        };
        match done {
            Ok(Some(moved)) => self.follow(&moved),
            Ok(None) => {}
            Err(e) => self.log.error(format!("{e:#}")),
        }
        self.refresh_content();
    }

    /// Copies a linked animation into the project for good.
    fn copy_in(&mut self, rel: &str) -> Result<content::Moved> {
        use anyhow::Context;
        let item = self.animations.find(rel).cloned().context("no such animation")?;
        let mods = self.project.mods_dir();
        let game = self.games.get(&mods)?;
        let (old, new) = crate::animations::copy_in(&mut self.project, game, &item)?;
        let moved = content::Moved(vec![(old, new)]);
        self.project.retarget(&|old| moved.target(old))?;
        Ok(moved)
    }

    /// Files of the project moved: the scene being edited, its history,
    /// the scene as saved and what the Content panel has open follow.
    fn follow(&mut self, moved: &content::Moved) {
        if moved.0.is_empty() {
            return;
        }
        let to = |old: &str| moved.target(old);
        self.doc.retarget(&to);
        self.saved.retarget(&to);
        let slashes = |p: &Path| p.to_string_lossy().replace('\\', "/");
        if let Some(new) = self.asset.as_deref().and_then(|a| to(&slashes(a))) {
            self.asset = Some(PathBuf::from(new));
        }
        if let Some(new) = to(&self.subfolder) {
            self.subfolder = new;
        }
        if let Some((rel, _)) = &mut self.clipboard {
            if let Some(new) = to(rel) {
                *rel = new;
            }
        }
        // Built characters are known by their card's path.
        if let Some(live) = self.live.as_mut() {
            live.retry();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Place;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("renkatsu-state-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An editor on a new, empty project.
    fn editor(home: &Path) -> State {
        let mut st = State::new(Project::none(), Scene::new());
        st.scripted = true;
        st.new_project(&home.join("P"), "P", Start::Empty).unwrap();
        st
    }

    /// The card and the animation file the scene's only character names.
    fn named(st: &State) -> (String, Option<String>) {
        let character = st.doc.scene().objects().iter().find_map(|o| match &o.content {
            Content::Character { card, animation, .. } => Some((
                card.to_string_lossy().into_owned(),
                animation.as_ref().and_then(|a| a.file.clone()),
            )),
            _ => None,
        });
        character.expect("the scene has a character")
    }

    #[test]
    fn what_content_does_to_files_keeps_the_scene_on_them() {
        let home = scratch("files");
        let mut st = editor(&home);
        std::fs::write(home.join("anna.png"), b"card").unwrap();
        let card = st.project.import_card(&home.join("anna.png"), "Anna").unwrap();
        st.project.save().unwrap();
        st.refresh_content();
        rk_chara::animation_file::sample("Spin", 3.0, true)
            .write(&home.join("Spin.rkanim"))
            .unwrap();
        // An animation file is copied into the project and shown there.
        st.import_animation_file(&home.join("Spin.rkanim"));
        std::fs::remove_file(home.join("Spin.rkanim")).unwrap();
        assert_eq!(st.asset, Some(PathBuf::from("Animations/Spin.rkanim")));
        assert_eq!((st.folder, st.subfolder.as_str()), (1, "Animations"));
        assert!(std::mem::take(&mut st.show_content));
        // Anything else is refused with a word about what to do.
        st.import_animation_file(&home.join("pack.rkpkg"));
        assert!(st.log.lines.last().unwrap().2.contains("Window, Packages"));

        st.run(Command::AddCard(PathBuf::from(&card)));
        let spin = st.animations.items[0].reference();
        st.run(Command::Animate(Some(spin)));
        assert!(st.save());
        assert_eq!(
            named(&st),
            ("Characters/anna.png".into(), Some("Animations/Spin.rkanim".into()))
        );

        // Rename the card; make a folder, name it and move the animation in.
        st.run(Command::File(FileOp::Rename(card, "Anna Kato".into())));
        st.run(Command::File(FileOp::NewFolder("Animations".into())));
        let (folder, text) = st.content_renaming.clone().expect("the new folder is being named");
        assert_eq!(
            (folder.as_str(), text.as_str()),
            ("Animations/New folder", "New folder")
        );
        st.run(Command::File(FileOp::Rename(folder, "Dances".into())));
        st.asset = Some(PathBuf::from("Animations/Spin.rkanim"));
        st.run(Command::File(FileOp::Move(
            "Animations/Spin.rkanim".into(),
            "Animations/Dances".into(),
        )));
        let moved = (
            "Characters/Anna Kato.png".to_owned(),
            Some("Animations/Dances/Spin.rkanim".to_owned()),
        );
        assert_eq!(named(&st), moved);
        assert_eq!(st.asset, Some(PathBuf::from("Animations/Dances/Spin.rkanim")));
        assert_eq!(st.library.characters[0].name, "Anna Kato");
        assert_eq!(st.animations.items[0].rel, "Animations/Dances/Spin.rkanim");
        // The scene on disk followed, so nothing is unsaved, and the
        // project opens with the files where they are.
        assert!(!st.dirty());
        let (_, on_disk, _) = Project::open(&st.project.root).unwrap();
        assert_eq!(&on_disk, st.doc.scene());
        // Undoing the animation and doing it again keeps the new path.
        st.run(Command::Undo);
        assert_eq!(named(&st), (moved.0.clone(), None));
        st.run(Command::Redo);
        assert_eq!(named(&st), moved);

        // Cut and paste moves it back; copy and paste makes a copy.
        st.run(Command::File(FileOp::Cut("Animations/Dances/Spin.rkanim".into())));
        st.run(Command::File(FileOp::Paste("Animations".into())));
        assert_eq!(named(&st).1.as_deref(), Some("Animations/Spin.rkanim"));
        assert_eq!(st.clipboard, None);
        st.run(Command::File(FileOp::Copy("Animations/Spin.rkanim".into())));
        st.run(Command::File(FileOp::Paste("Animations/Dances".into())));
        st.run(Command::File(FileOp::Duplicate("Animations/Spin.rkanim".into())));
        let files: Vec<&str> = st.animations.items.iter().map(|i| i.rel.as_str()).collect();
        assert_eq!(
            files,
            [
                "Animations/Spin.rkanim",
                "Animations/Spin 2.rkanim",
                "Animations/Dances/Spin.rkanim"
            ]
        );
        assert_eq!(named(&st).1.as_deref(), Some("Animations/Spin.rkanim"));

        // Deleting what the scene uses says so, and the file can be found.
        st.run(Command::File(FileOp::Delete("Animations/Spin.rkanim".into())));
        assert!(st.project.root.join(".renkatsu/trash/1/Spin.rkanim").is_file());
        assert!(st.log.lines.last().unwrap().2.contains("1 objects of the scene"));
        assert_eq!(st.animations.items.len(), 2);
        // What cannot be done is said in the console and changes nothing.
        st.run(Command::File(FileOp::Rename("Animations".into(), "Clips".into())));
        assert_eq!(st.log.lines.last().unwrap().0, 2);
        assert!(st.project.root.join("Animations").is_dir());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_outliners_commands_act_on_the_scene() {
        let home = scratch("scene");
        let mut st = editor(&home);
        let names = |st: &State| -> Vec<(usize, String)> {
            let scene = st.doc.scene();
            scene
                .rows()
                .into_iter()
                .map(|(depth, id)| (depth, scene.get(id).unwrap().name.clone()))
                .collect()
        };
        let row = |depth: usize, name: &str| (depth, name.to_owned());
        st.run(Command::Add(Kind::Folder));
        let folder = st.doc.selected.unwrap();
        st.run(Command::Add(Kind::Camera));
        let camera = st.doc.selected.unwrap();
        // A folder inside the selected folder, which is then being named.
        st.doc.selected = Some(folder);
        st.run(Command::AddChildFolder);
        let child = st.doc.selected.unwrap();
        assert_eq!(st.renaming.as_ref().map(|r| r.0), Some(child));
        assert_eq!(
            names(&st),
            [
                row(0, "Key light"),
                row(0, "Folder"),
                row(1, "Camera"),
                row(1, "Folder 2")
            ]
        );
        // Its children are selected together, hidden together and taken
        // out of it together.
        st.doc.selected = Some(folder);
        st.run(Command::SelectChildren);
        assert_eq!(st.doc.selected_all(), [camera, child]);
        st.run(Command::ToggleHidden);
        assert!(!st.doc.scene().get(camera).unwrap().visible && !st.doc.scene().get(child).unwrap().visible);
        st.run(Command::Undo);
        st.doc.select_many(&[camera, child]);
        st.run(Command::Unparent);
        assert_eq!(
            names(&st),
            [
                row(0, "Key light"),
                row(0, "Folder"),
                row(0, "Camera"),
                row(0, "Folder 2")
            ]
        );
        st.run(Command::Undo);
        assert_eq!(st.doc.scene().get(camera).unwrap().parent, Some(folder));

        // Copy the folder, paste it at the top; cut the copy again.
        st.doc.selected = Some(folder);
        st.run(Command::Copy);
        st.doc.selected = None;
        st.run(Command::Paste);
        let pasted = st.doc.selected.unwrap();
        assert_eq!(st.focus_pending, Some(pasted));
        assert_eq!(
            names(&st)[4..],
            [row(0, "Folder 3"), row(1, "Camera 2"), row(1, "Folder 4")]
        );
        st.run(Command::Cut);
        assert_eq!(names(&st).len(), 4);
        assert_eq!(st.scene_clipboard.len(), 3);
        // Focus asks the viewport for the selection.
        st.doc.selected = Some(camera);
        st.focus_pending = None;
        st.run(Command::Focus);
        assert_eq!(st.focus_pending, Some(camera));
        // Several selected objects go with one Delete, and come back with
        // one undo.
        st.doc.select_many(&[camera, child]);
        st.run(Command::Delete);
        assert_eq!(names(&st), [row(0, "Key light"), row(0, "Folder")]);
        st.run(Command::Undo);
        assert_eq!(names(&st).len(), 4);
        assert!(st.doc.scene().can_place(camera, Place::Into(None)));
        let _ = std::fs::remove_dir_all(&home);
    }
}
