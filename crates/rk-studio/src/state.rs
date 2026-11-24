// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! What the shell remembers between frames: the look in use, the selection,
//! the state of the viewport's overlays, the project that is open, its
//! scene being edited and the commands that change them.

use crate::content;
use crate::design::{Spec, TitleBar};
use crate::dialogs::{After, Import, Modal, PendingImport, Pick, Picker, ProjectForm};
use crate::jobs::{Item, Jobs, Show, Stage};
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
    /// Adds an object that is ready as it is: a light or an effect.
    AddContent(Content),
    /// Add the character of one of the project's cards, named by its path
    /// relative to the project.
    AddCard(PathBuf),
    /// Put one of the project's maps in the scene, named by its sidecar
    /// relative to the project.
    AddMap(PathBuf),
    /// Open the Maps library to import a map into the project alone.
    MapLibrary,
    /// Pick a map file and copy it into the project.
    ImportMapFile,
    /// Give the selected character an animation, or take its animation
    /// off.
    Animate(Option<crate::scene::Animation>),
    /// Give the selected character the pose saved in this file of the
    /// project.
    LoadPose(String),
    /// Save the selected character's pose in the project's Poses folder.
    SavePose,
    /// Save the selected character's keys as an animation of the project.
    SaveKeys,
    /// Give the selected character the keys saved in this file of the
    /// project, in place of her own.
    LoadKeys(String),
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
    /// Take the selection out of what it sits in or is attached to, to
    /// the top of the scene. It stays where it is.
    Unparent,
    /// Attach the selection to an object, or to a bone of a character
    /// by the bone's name.
    Attach(ObjectId, Option<String>),
    /// Attach the rest of the selection to the object selected last.
    AttachToLast,
    /// Drop the selection onto the surface below it.
    SnapToFloor,
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

/// A project on its way in: the selector's window shows what loads, and
/// the editor appears when all of it is there.
pub struct Opening {
    /// The project's name.
    pub name: String,
    /// The step, the count and the list, as the splash shows them.
    pub progress: crate::jobs::Progress,
    started: std::time::Instant,
    /// Seconds the content took to open, once it is open.
    content: Option<f32>,
    /// The viewport drew every character once.
    drawn: bool,
    /// The parts of the bar: the content, the characters, the viewport.
    parts: [crate::jobs::Progress; 3],
    /// The most textures seen waiting for the GPU.
    textures: usize,
    /// A sample for a picture of the window: nothing moves it on.
    pub pictured: bool,
}

impl Opening {
    pub fn new(name: &str, wake: Option<egui::Context>) -> Opening {
        let progress = crate::jobs::Progress::new(wake);
        // The characters are most of the wait.
        let parts = [progress.child(0.1), progress.child(0.7), progress.child(0.2)];
        Opening {
            name: name.to_owned(),
            parts,
            textures: 0,
            progress,
            started: std::time::Instant::now(),
            content: None,
            drawn: false,
            pictured: false,
        }
    }

    /// A project halfway open, to picture its splash.
    pub fn sample() -> Opening {
        let mut opening = Opening::new("DevTest1", None);
        opening.pictured = true;
        let item = |name: &str, stage, note: &str| Item {
            name: name.to_owned(),
            stage,
            note: note.to_owned(),
        };
        opening.progress.set_items(vec![
            item("Content", Stage::Done, "0.2 s"),
            item("Loaded Default female", Stage::Done, "3.4 s"),
            item("Maria Kujou", Stage::Running, ""),
            item("Viewport", Stage::Waiting, ""),
        ]);
        opening.progress.step("Loading Maria Kujou");
        opening.progress.set(2, 4);
        opening.parts[0].set(1, 1);
        opening.parts[1].set(1, 2);
        opening
    }
}

pub struct State {
    /// The window's title bar.
    pub title_bar: TitleBar,
    /// How far the library's maps start scrolled, for a picture of them.
    pub map_scroll: Option<f32>,
    /// The pictures of maps at their times of day, and their sidecars.
    pub map_pictures: crate::map_library::Pictures,
    /// When the fly speed last changed, in egui's seconds, for the meter
    /// that shows it and fades away.
    pub speed_shown: Option<f64>,
    /// When the lens last changed (Alt and the wheel), for its meter.
    pub lens_shown: Option<f64>,
    /// The sun is being turned with its shortcut held: one step of the
    /// history until it is let go, though no button is down.
    pub sun_turning: bool,
    /// The sun's dome shows without the key held, for pictures.
    pub sun_preview: bool,
    /// Mirror the selected character's pose once she is built, for
    /// pictures.
    pub mirror_pending: bool,
    /// `--bake-pose`: bake the animation once the character is built.
    pub bake_pending: bool,
    /// `--save-pose`: save the pose once the character is built.
    pub save_pose_pending: bool,
    /// `--ik-move`: targets to move once the character is built.
    pub ik_pending: Vec<(usize, glam::Vec3)>,
    pub ik_wanted: bool,
    /// The bone the Pose page shows, by name.
    pub pose_bone: Option<String>,
    /// Whether Forward kinematics is open in the Pose palette, showing its
    /// parts.
    pub fk_open: bool,
    /// The Pose palette's Inverse kinematics category is open.
    pub ik_open: bool,
    /// The IK target chosen for the gizmo, by its place in the layer.
    pub pose_ik: Option<usize>,
    /// What the Animator panel's skeleton is searched by.
    pub animator_search: String,
    /// The project that is open; `Project::none()` while the project
    /// selector shows.
    pub project: Project,
    /// The project selector, while it shows in place of the editor.
    pub selector: Option<Selector>,
    /// The object that was just added: the camera goes to it once the
    /// viewport has something of it to look at.
    pub focus_pending: Option<ObjectId>,
    /// Characters to set on the map's floor once its meshes are there,
    /// where it puts them: the map just came, or they did.
    pub settle: Vec<ObjectId>,
    /// The scene being edited, with the selection and the undo history.
    pub doc: Document,
    /// The scene as it was last saved, to tell unsaved changes by.
    saved: Scene,
    /// The game's files, opened with the project's mods.
    pub games: Games,
    /// The work that runs off the window's thread.
    pub jobs: Jobs,
    /// The project that is still opening, while its splash shows.
    pub opening: Option<Opening>,
    /// The mods folder the content is being opened with, by a job.
    content_pending: Option<PathBuf>,
    /// The mods folder the content could not be opened with.
    content_failed: Option<PathBuf>,
    /// Imports whose mods are being copied.
    importing: usize,
    /// The package work that shows as a job.
    package_job: Option<crate::jobs::Tracker>,
    /// The news of the home page.
    pub news: Vec<rk_packages::news::Post>,
    /// The user's installed packages, with their windows.
    pub packages: crate::packages::Packages,
    /// The question that is open over the window.
    pub modal: Option<Modal>,
    /// The system file dialog that is open.
    pub picker: Option<Picker>,
    /// Cards waiting to be imported, one after the other.
    pub imports: VecDeque<Import>,
    /// The Maps library, while it is open.
    pub map_library: Option<crate::map_library::Shelf>,
    /// The editor runs from a script: it asks nothing and leaves the
    /// user's settings as they are.
    pub scripted: bool,
    /// A newer Renkatsu on GitHub, when the check at start found one.
    pub update: Option<rk_packages::update::Release>,
    /// Install Update at work, or stopped with why.
    pub updating: Option<crate::self_update::Updating>,
    /// The updated program to start once Renkatsu has quit.
    pub restart_into: Option<PathBuf>,
    /// A key copied, to paste on a bone.
    pub key_clipboard: Option<rk_chara::pose::BoneEdit>,
    /// Playing a sequence with keys starts over at its end.
    pub loop_sequence: bool,
    /// The character whose keys play alone, and where they are.
    pub solo: Option<ObjectId>,
    pub solo_time: f32,
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
    /// Something asked for the Animator panel: it is opened if it is not
    /// shown, and brought forward.
    pub show_animator: bool,
    /// The folder open in the Content panel, relative to the project: one
    /// of the project's folders or a folder the user made below one.
    pub subfolder: String,
    /// The folders opened before and after the open one, for the mouse's
    /// back and forward buttons.
    pub folder_back: Vec<String>,
    pub folder_forward: Vec<String>,
    /// The file or folder being renamed in the Content panel, with the
    /// text so far.
    pub content_renaming: Option<(String, String)>,
    /// The renaming in Content started in the folder tree.
    pub renaming_in_tree: bool,
    /// The panel last used, which keys such as F2 act in.
    pub focus: crate::panels::Panel,
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
    pub mode_tool: [usize; 7],
    /// The Select mode's filter: only characters, items, or lights and
    /// cameras (as `Kind::Light`) are picked; none picks anything.
    pub pick_filter: Option<Kind>,
    /// The active mode's tool palette is open.
    pub palette_open: bool,
    /// The palette stays open while working in the viewport.
    pub palette_pinned: bool,
    pub rail: Rail,
    /// Every viewport overlay is hidden.
    pub overlays_hidden: bool,
    /// The floating icons of lights and cameras are hidden (and
    /// not picked in the viewport).
    pub icons_hidden: bool,
    /// The viewport takes the place of all docked panels.
    pub maximised: bool,
    /// Index into `modes::TOOLS`.
    pub tool: usize,
    pub snap: bool,
    /// Selecting and the transform gizmos in the viewport.
    pub gizmo: crate::gizmo::Gizmos,
    /// What the play, pause and stop buttons set: whether the scene's
    /// time runs. Animations and physics both go by it.
    pub playback: Playback,
    /// Seconds the scene has played since it was last stopped.
    pub clock: f32,

    /// Properties sections that are open.
    pub sections: [bool; 22],
    /// Expression presets: Renkatsu's own, then ExpressionControl's from
    /// the game folder; read when first shown.
    pub expressions: Option<(
        Vec<rk_chara::expression_control::Preset>,
        Vec<rk_chara::expression_control::Preset>,
    )>,
    /// The name a new expression preset is saved under.
    pub expression_name: String,
    /// The page of a character's Properties: the scene's settings, the
    /// card's profile or its likes.
    pub property_tab: usize,
    /// What each card says about its character, by the card on disk.
    profiles: std::collections::HashMap<PathBuf, Option<crate::profile::Profile>>,
    /// The project's animations, for the Content panel and Properties.
    pub animations: crate::animations::Shelf,

    pub scene_search: String,
    pub property_search: String,
    /// What is typed into the search of the menu that chooses a bone.
    pub bone_search: String,
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
    /// The keys of every action: the defaults with the user's changes.
    pub keys: crate::keymap::Keymap,
    pub shortcuts_open: bool,
    /// Help, About Renkatsu is open.
    pub about_open: bool,
    pub shortcuts_search: String,
    /// The action and the place the Keyboard shortcuts window waits for
    /// keys for.
    pub key_capture: Option<(crate::keymap::Action, usize)>,
    /// The inputs shown in the viewport's corner.
    pub inputs: Vec<crate::keymap::Input>,
    /// The settings changed and wait to be written.
    pub settings_changed: bool,
    /// The folders folded away in the Content panel's tree; the project's
    /// own is the empty path.
    pub content_folded: HashSet<String>,
    /// Seconds per frame, smoothed, for the status bar.
    pub frame_time: f32,
}

impl State {
    /// The kinds of physics the Play menu's switches hold still.
    pub fn physics_still(&self) -> Vec<rk_chara::physics::Belongs> {
        rk_chara::physics::Belongs::ALL
            .into_iter()
            .zip(self.settings.physics)
            .filter(|(_, on)| !on)
            .map(|(belongs, _)| belongs)
            .collect()
    }

    /// Hands the face shots the Content panel asked for to their thread,
    /// with the content the cards are built from.
    pub fn take_face_shots(&mut self) {
        if self.library.wanted.is_empty() {
            return;
        }
        // The shots wait for the content, which a job opens.
        let Some(game) = self.content() else {
            return;
        };
        let wanted = std::mem::take(&mut self.library.wanted);
        let backdrop = self.library.backdrop;
        let Some(mugshots) = self.library.mugshots.as_mut() else {
            return;
        };
        for (card, shot) in wanted {
            mugshots.ask(&card, game.clone(), shot, backdrop);
        }
    }

    /// What a project's card says about its character, read once.
    pub fn profile(&mut self, card: &Path) -> Option<crate::profile::Profile> {
        let path = self.project.resolve(card);
        self.profiles
            .entry(path)
            .or_insert_with_key(|path| {
                rk_card::Card::load(path)
                    .ok()
                    .map(|c| crate::profile::Profile::read(&c.parameter))
            })
            .clone()
    }

    /// The content characters are built from, when it is open. A job
    /// opens it; a scripted run opens it here and waits.
    pub fn content(&mut self) -> Option<std::sync::Arc<rk_vfs::Game>> {
        let mods = self.project.mods_dir();
        if self.scripted {
            return self.games.shared(&mods).ok();
        }
        self.games.ready(&mods)
    }

    /// Moves the background work on, once a frame at the window's time
    /// `now`: takes in what finished, and starts what the project needs.
    pub fn tick(&mut self, now: f64) {
        for apply in self.jobs.poll(now) {
            apply(self);
        }
        self.want_content();
        self.track_packages();
        self.advance_opening();
    }

    /// Has the content of the open project opened by a job, unless it is
    /// open, on its way or known to fail.
    fn want_content(&mut self) {
        if self.project.is_none() || self.scripted {
            return;
        }
        let mods = self.project.mods_dir();
        let same = |known: &Option<PathBuf>| known.as_deref() == Some(mods.as_path());
        if self.games.ready(&mods).is_some() || same(&self.content_pending) || same(&self.content_failed) {
            return;
        }
        self.content_pending = Some(mods.clone());
        // While a project opens its splash lists the content.
        let show = if self.opening.is_some() {
            Show::Quiet
        } else {
            Show::Notice
        };
        let theirs = mods.clone();
        self.jobs.spawn(
            "Opening the content",
            show,
            move |_| rk_vfs::Game::open_user(&[theirs]).map(std::sync::Arc::new),
            move |st: &mut State, result| {
                if st.content_pending.as_deref() == Some(mods.as_path()) {
                    st.content_pending = None;
                }
                // The project may have changed while the content opened.
                if st.project.is_none() || st.project.mods_dir() != mods {
                    return;
                }
                match result {
                    Ok(game) => st.games.install(&mods, game),
                    Err(e) => {
                        st.log.error(format!("The content could not be opened: {e:#}"));
                        st.content_failed = Some(mods);
                    }
                }
            },
        );
    }

    /// Shows the work on a package, which has a thread of its own, as a
    /// job: its stage, its bytes, and a way to stop it.
    fn track_packages(&mut self) {
        let Some((name, task)) = self.packages.task() else {
            if let Some(job) = self.package_job.take() {
                let ended = match &self.packages.note {
                    Some((text, true)) => Err(text.clone()),
                    Some((text, false)) => {
                        job.progress.step(text);
                        Ok(())
                    }
                    None => Ok(()),
                };
                job.end(ended);
            }
            return;
        };
        if self.package_job.is_none() {
            let name = name.to_owned();
            self.package_job = Some(self.jobs.track(&name, Show::Notice));
        }
        if let Some(job) = &self.package_job {
            let (stage, done, total) = task.progress();
            job.progress.step(stage.label());
            job.progress.set_bytes(done.min(total), total);
            if job.progress.cancelled() {
                task.cancel();
            }
        }
    }

    /// Starts the splash of a project that was just adopted.
    pub fn begin_opening(&mut self) {
        if self.scripted || self.project.is_none() {
            return;
        }
        self.opening = Some(Opening::new(&self.project.file.name, self.jobs.wake.clone()));
    }

    /// Moves the opening of a project on: the content, its characters
    /// and their first draw. The editor shows when all of it is there.
    fn advance_opening(&mut self) {
        let Some(mut opening) = self.opening.take() else { return };
        if opening.pictured {
            self.opening = Some(opening);
            return;
        }
        if opening.progress.cancelled() {
            self.show_selector();
            return;
        }
        let mods = self.project.mods_dir();
        let failed = self.content_failed.as_deref() == Some(mods.as_path());
        let ready = self.games.ready(&mods).is_some();
        if ready && opening.content.is_none() {
            opening.content = Some(opening.started.elapsed().as_secs_f32());
        }
        let content = match (opening.content, failed) {
            (Some(took), _) => (Stage::Done, format!("{took:.1} s")),
            (None, true) => (Stage::Failed, String::new()),
            (None, false) => (Stage::Running, String::new()),
        };
        let mut items = vec![Item {
            name: "Content".to_owned(),
            stage: content.0,
            note: content.1,
        }];
        let mut step = "Opening the content".to_owned();

        // Cards still on their way into the project.
        let importing = !self.imports.is_empty() || self.importing > 0 || self.modal.is_some();
        let mut building = false;
        let mut uploads = 0;
        if let Some(live) = self.live.as_mut() {
            live.show_builds = Show::Quiet;
            let clock = crate::live::Clock {
                time: self.clock,
                ..crate::live::Clock::stopped()
            };
            let scene = self.doc.scene();
            live.follow(
                scene,
                &self.project,
                &mut self.games,
                &mut self.jobs,
                clock,
                &mut self.log,
            );
            // The draw sends the textures to the GPU, a few a frame, and
            // makes what the first frame of the editor would wait for.
            if let Err(e) = live.paint(scene, [64, 64], [0.22, 0.22, 0.23, 1.0]) {
                self.log.error(format!("The viewport could not be drawn: {e:#}"));
            }
            uploads = live.uploads_left();
            for object in scene.objects().iter().filter(|o| o.kind() == Kind::Character) {
                use crate::live::Progress as Built;
                let built = live.progress(object.id);
                let (stage, note) = match built.clone() {
                    Built::Ready(took) => (Stage::Done, format!("{took:.1} s")),
                    Built::Failed => (Stage::Failed, String::new()),
                    Built::Waiting if failed => (Stage::Failed, String::new()),
                    Built::Waiting => (Stage::Waiting, String::new()),
                    Built::Building | Built::Uploading => (Stage::Running, String::new()),
                };
                if stage == Stage::Running || stage == Stage::Waiting {
                    if !building {
                        step = format!("Loading {}", object.name);
                    }
                    building = true;
                }
                items.push(Item {
                    name: match built {
                        Built::Ready(_) => format!("Loaded {}", object.name),
                        _ => object.name.clone(),
                    },
                    stage,
                    note,
                });
            }
            if ready && !building && !importing {
                opening.drawn = true;
            }
        } else {
            opening.drawn = ready || failed;
        }
        let waiting = importing || building || !(ready || failed);
        items.push(Item {
            name: "Viewport".to_owned(),
            stage: match (opening.drawn, waiting) {
                (true, _) => Stage::Done,
                (false, true) => Stage::Waiting,
                (false, false) => Stage::Running,
            },
            note: if uploads > 0 {
                format!("{uploads} textures")
            } else {
                String::new()
            },
        });
        let done = items.iter().filter(|i| i.stage == Stage::Done).count();
        // The bar: each part by how far it is.
        let characters = items.len() - 2;
        let built = items[1..=characters]
            .iter()
            .filter(|i| matches!(i.stage, Stage::Done | Stage::Failed))
            .count();
        opening.textures = opening.textures.max(uploads);
        let [content, people, viewport] = &opening.parts;
        content.set(u64::from(ready || failed), 1);
        if characters == 0 {
            people.set(u64::from(ready || failed), 1);
        } else {
            people.set(built as u64, characters as u64);
        }
        if opening.drawn {
            viewport.set(1, 1);
        } else {
            viewport.set((opening.textures - uploads) as u64, opening.textures.max(1) as u64);
        }
        opening.progress.set(done as u64, items.len() as u64);
        opening.progress.set_items(items);
        opening.progress.step(&step);
        if (opening.drawn || failed) && !waiting {
            if let Some(live) = self.live.as_mut() {
                live.show_builds = Show::Notice;
            }
            return;
        }
        self.opening = Some(opening);
    }

    /// Reads the news and the newest release: what the cache has at
    /// once, and what GitHub says by a job when the cache is old. Nothing
    /// waits for the answer.
    pub fn fetch_feed(&mut self) {
        use rk_packages::news::{Cache, Wanted};
        let wanted = Wanted {
            news: self.settings.show_news,
            updates: self.settings.check_updates,
        };
        if !wanted.news {
            self.news.clear();
        }
        if self.scripted || !(wanted.news || wanted.updates) {
            return;
        }
        const RUNNING: &str = env!("CARGO_PKG_VERSION");
        let now = crate::settings::now();
        let file = Cache::file();
        let mut cache = Cache::load(&file);
        let feed = cache.feed(wanted, RUNNING, now);
        self.news = feed.posts;
        self.update = feed.update.or(self.update.take());
        self.jobs.spawn(
            "News",
            Show::Quiet,
            move |_| {
                if cache.refresh(wanted, now) {
                    // A cache that cannot be written is asked for again.
                    let _ = cache.save(&file);
                }
                Ok(cache.feed(wanted, RUNNING, now))
            },
            |st: &mut State, result| {
                let Ok(feed) = result else { return };
                if st.settings.show_news {
                    st.news = feed.posts;
                }
                if let Some(release) = feed.update.filter(|_| st.settings.check_updates) {
                    if st.update.as_ref() != Some(&release) {
                        st.log
                            .info(format!("Renkatsu {} is out: {}", release.version, release.url));
                    }
                    st.update = Some(release);
                }
            },
        );
    }

    /// The editor on a project and its scene.
    pub fn new(project: Project, scene: Scene) -> Self {
        State {
            title_bar: TitleBar::A,
            map_scroll: None,
            map_pictures: crate::map_library::Pictures::default(),
            speed_shown: None,
            lens_shown: None,
            sun_turning: false,
            sun_preview: false,
            mirror_pending: false,
            bake_pending: false,
            save_pose_pending: false,
            ik_pending: Vec::new(),
            ik_wanted: false,
            pose_bone: None,
            fk_open: false,
            ik_open: false,
            pose_ik: None,
            animator_search: String::new(),
            library: Library::new(&project),
            project,
            selector: None,
            focus_pending: None,
            settle: Vec::new(),
            saved: scene.clone(),
            doc: Document::new(scene),
            games: Games::default(),
            jobs: Jobs::default(),
            opening: None,
            content_pending: None,
            content_failed: None,
            importing: 0,
            package_job: None,
            news: Vec::new(),
            packages: crate::packages::Packages::new(),
            modal: None,
            picker: None,
            imports: VecDeque::new(),
            map_library: None,
            scripted: false,
            update: None,
            updating: None,
            restart_into: None,
            key_clipboard: None,
            loop_sequence: true,
            solo: None,
            solo_time: 0.0,
            quit: false,
            folded: HashSet::new(),
            scene_folded: false,
            renaming: None,
            show_content: false,
            show_animator: false,
            subfolder: project::FOLDERS[0].to_owned(),
            folder_back: Vec::new(),
            folder_forward: Vec::new(),
            content_renaming: None,
            renaming_in_tree: false,
            focus: crate::panels::Panel::Viewport,
            clipboard: None,
            scene_clipboard: Vec::new(),
            importer: None,
            log: Log::new(),
            mode: 0,
            mode_tool: [0, 0, 1, 0, 0, 0, 0],
            pick_filter: None,
            palette_open: false,
            palette_pinned: true,
            rail: Rail::Named,
            overlays_hidden: false,
            icons_hidden: false,
            maximised: false,
            tool: 0,
            snap: true,
            gizmo: Default::default(),
            playback: Playback::Stopped,
            clock: 0.0,
            expressions: None,
            expression_name: String::new(),
            sections: [
                true, true, true, false, true, true, true, true, true, true, true, true, true, true, true, true, true,
                true, true, true, true, false,
            ],
            property_tab: 0,
            profiles: Default::default(),
            animations: Default::default(),
            scene_search: String::new(),
            property_search: String::new(),
            bone_search: String::new(),
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
            keys: Default::default(),
            shortcuts_open: false,
            about_open: false,
            shortcuts_search: String::new(),
            key_capture: None,
            inputs: Vec::new(),
            settings_changed: false,
            content_folded: HashSet::new(),
            frame_time: 0.0,
        }
    }

    /// The kind and name of the selected object.
    pub fn selection(&self) -> Option<(Kind, String)> {
        self.doc.selection().map(|o| (o.kind(), o.name.clone()))
    }

    /// Carries out a command on the scene.
    /// Attaches objects to another object, or to a bone of a character
    /// by the bone's name. They stay where they are.
    pub fn attach(&mut self, ids: &[ObjectId], to: ObjectId, bone: Option<&str>) {
        let ids: Vec<ObjectId> = ids.iter().copied().filter(|id| *id != to).collect();
        // Where the bone is now; a character that is not built yet has
        // its bones at its origin until it is.
        let pose = bone.map(|name| {
            let live = self.live.as_ref();
            (name, live.and_then(|l| l.bone(to, name)).unwrap_or_default())
        });
        if self.doc.attach(&ids, to, pose) > 0 {
            self.folded.remove(&to);
        }
    }

    /// Snaps a character onto another, as an H animation wants the pair.
    pub fn snap(&mut self, id: ObjectId, to: ObjectId) {
        if self.doc.snap(id, to) {
            self.folded.remove(&to);
        }
    }

    /// Drops each selected object straight down onto the first surface
    /// below its lowest point: a character lands by its feet. One step
    /// of the history.
    fn snap_to_floor(&mut self) {
        use crate::surface::{floor_below, Surface};
        let scene = self.doc.scene();
        let live = self.live.as_ref();
        let mut dropped = Vec::new();
        for id in self.doc.selected_all() {
            let Some(mut object) = scene.get(id).cloned() else {
                continue;
            };
            let origin = scene.world(id).w_axis.truncate();
            let lowest = live.and_then(|l| l.lowest(id)).unwrap_or(origin);
            // Everything but the object itself and what goes with it.
            let others = live.map(|l| l.surface(scene, scene.subtree(id)));
            let surfaces: Vec<&dyn Surface> = others.iter().map(|s| s as &dyn Surface).collect();
            let ground = (!live.is_some_and(|l| l.on_map())).then_some(0.0);
            let Some(floor) = floor_below(lowest, &surfaces, ground) else {
                continue;
            };
            let fall = floor - lowest.y;
            let landed = scene
                .parent_world(id)
                .inverse()
                .transform_point3(origin + glam::Vec3::Y * fall);
            if landed.is_finite() {
                object.transform.position = crate::scene::clean(landed);
                dropped.push(object);
            }
        }
        for object in dropped {
            self.doc.edit(object, true);
        }
        self.doc.end_gesture();
    }

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
            Command::Attach(to, bone) => {
                let all = self.doc.selected_all();
                self.attach(&all, to, bone.as_deref());
            }
            Command::AttachToLast => {
                if let [rest @ .., last] = &self.doc.selected_all()[..] {
                    self.attach(rest, *last, None);
                }
            }
            Command::SnapToFloor => self.snap_to_floor(),
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
                // In Content, the file or folder chosen there; elsewhere the
                // selected object.
                let asset = self.asset.as_ref().map(|a| a.to_string_lossy().replace('\\', "/"));
                match asset {
                    Some(rel) if self.focus == crate::panels::Panel::Content && content::changeable(&rel) => {
                        let name = content::stem(&rel).to_owned();
                        self.content_renaming = Some((rel, name));
                        self.renaming_in_tree = false;
                    }
                    _ => self.renaming = self.doc.selection().map(|o| (o.id, o.name.clone())),
                }
            }
            Command::Add(Kind::Character) => self.show_folder(0),
            Command::Add(Kind::Map) => self.open_map_library(),
            Command::Add(kind) => {
                if let Some(content) = Content::of_kind(kind) {
                    let name = match kind {
                        Kind::Folder => "Folder",
                        Kind::Light => "Light",
                        Kind::Camera => "Camera",
                        _ => "Item",
                    };
                    let id = self.doc.add(name, content);
                    self.unfold_to(id);
                    self.focus_pending = Some(id);
                }
            }
            Command::AddContent(content) => {
                let id = self.doc.add(content.label(), content);
                self.unfold_to(id);
                self.focus_pending = Some(id);
            }
            Command::AddCard(card) => self.add_card(card),
            Command::AddMap(file) => self.add_map(file),
            Command::MapLibrary => self.open_map_import(),
            Command::Animate(animation) => {
                if let Some(mut object) = self.doc.selection().cloned() {
                    if let Content::Character { animation: worn, .. } = &mut object.content {
                        *worn = animation.map(Box::new);
                        self.doc.edit(object, false);
                    }
                }
            }
            Command::LoadPose(rel) => self.load_pose(&rel),
            Command::SavePose => self.save_pose(),
            Command::SaveKeys => self.save_keys(),
            Command::LoadKeys(rel) => self.load_keys(&rel),
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
            | Command::ImportMapFile
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
            Command::ImportFromPackage => self.packages.browse(rk_core::Config::load().ok().as_ref()),
            Command::ImportAnimations => self.importer = Some(crate::anim_import::Importer::new(None)),
            Command::ImportAnimationFile => self.pick(Pick::Animation, None, ctx),
            Command::ImportMapFile => self.pick(Pick::Map, None, ctx),
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
                first: false,
            }),
            Pick::Project => self.open_or_tell(&path),
            Pick::Animation => self.import_animation_file(&path),
            Pick::Map => self.import_map_file(&path),
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
        let mugshots = self.library.mugshots.take();
        self.library = Library::new(&self.project);
        self.library.eager = eager;
        self.library.mugshots = mugshots;
        self.animations.refresh(&self.project);
        self.subfolder = project::FOLDERS[0].to_owned();
        self.folder_back.clear();
        self.folder_forward.clear();
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
        self.opening = None;
        self.content_failed = None;
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
        self.begin_opening();
        Ok(())
    }

    /// Makes a new project in a folder and opens it: the key light and,
    /// when the start asks for her and the game has her, the default
    /// character in the scene.
    pub fn new_project(&mut self, root: &Path, name: &str, start: Start) -> Result<()> {
        let project = Project::create(root, name)?;
        self.adopt(project, Scene::new());
        self.start_fresh(start == Start::RenkatsuChan);
        self.begin_opening();
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
        if self.scripted {
            let mut copy = self.project.copy_to(root, name)?;
            copy.save_scene(&scene)?;
            self.saved_as(copy, scene);
            return Ok(());
        }
        // The copy can be large: the window waits for it with a bar.
        let project = self.project.clone();
        let (root, name) = (root.to_owned(), name.to_owned());
        self.jobs.spawn(
            &format!("Saving {name}"),
            Show::Wait,
            move |p| {
                p.step(&root.display().to_string());
                let mut copy = project.copy_to(&root, &name)?;
                copy.save_scene(&scene)?;
                Ok((copy, scene))
            },
            |st: &mut State, result| match result {
                Ok((copy, scene)) => st.saved_as(copy, scene),
                Err(e) => st.log.error(format!("The copy could not be saved: {e:#}")),
            },
        );
        Ok(())
    }

    /// Goes on in the copy a Save as made.
    fn saved_as(&mut self, copy: Project, scene: Scene) {
        self.log
            .info(format!("Saved {} in {}", copy.file.name, copy.root.display()));
        let selected = self.doc.selected;
        self.adopt(copy, scene);
        self.doc.selected = selected;
    }

    /// A click on the update: installs it, again after it failed, and once
    /// it is installed restarts into it by quitting, which asks to save.
    pub fn update_clicked(&mut self, ctx: &egui::Context) {
        match &self.updating {
            Some(u) if u.busy() => {}
            Some(u) if u.ready().is_some() => {
                self.restart_into = u.ready().map(Path::to_owned);
                self.ask(Command::Quit, ctx);
            }
            _ => {
                if let Some(release) = self.update.clone() {
                    self.updating = Some(crate::self_update::Updating::start(release, ctx));
                }
            }
        }
    }

    /// What characters are built from changed: the Koikatsu folder was
    /// chosen, or a package was installed or removed. The content is
    /// opened again at its next use, and the characters that could not be
    /// built get another try.
    pub fn content_changed(&mut self) {
        self.games.reset();
        self.map_pictures.forget_game();
        self.content_failed = None;
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
        let import = Import {
            path: card,
            place: true,
            ask: false,
            name: Some(DEFAULT_NAME.to_owned()),
            first: true,
        };
        if self.scripted {
            self.begin_import(import);
        } else {
            // It waits for the content, which a job opens.
            self.imports.push_back(import);
        }
    }

    /// Starts the next import that waits, once no dialog is open.
    pub fn next_import(&mut self) {
        if self.modal.is_some() || self.importing > 0 || self.imports.is_empty() {
            return;
        }
        // The mods a card needs are worked out from the content, which a
        // job opens: the import waits for it.
        let mods = self.project.mods_dir();
        let failed = self.content_failed.as_deref() == Some(mods.as_path());
        if !self.scripted && !failed && self.games.ready(&mods).is_none() {
            return;
        }
        if let Some(import) = self.imports.pop_front() {
            self.begin_import(import);
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
        if plan.copy.is_empty() {
            self.imported(import, title, missing);
            return;
        }
        let dir = self.project.mods_dir();
        let what = format!(
            "{}, {}",
            mods::count_text(plan.copy.len()),
            mods::size_text(plan.bytes())
        );
        if self.scripted {
            let copied = mods::copy(&plan, &dir);
            self.copied_mods(&title, &what, copied);
            self.imported(import, title, missing);
            return;
        }
        // The copy can be large: a job does it, and can be stopped.
        self.importing += 1;
        let (total, said) = (plan.bytes(), what.clone());
        self.jobs.spawn(
            &format!("Importing {title}"),
            Show::Notice,
            move |p| {
                p.set_bytes(0, total);
                mods::copy_counted(&plan, &dir, &mut |bytes| {
                    p.add(bytes);
                    p.check()
                })?;
                p.step(&said);
                Ok(())
            },
            move |st: &mut State, copied| {
                st.importing = st.importing.saturating_sub(1);
                if copied.as_ref().is_err_and(crate::jobs::was_cancelled) {
                    st.log.info(format!("The import of {title} was cancelled"));
                    return;
                }
                st.copied_mods(&title, &what, copied);
                st.imported(import, title, missing);
            },
        );
    }

    /// Says how the copy of an import's mods went.
    fn copied_mods(&mut self, title: &str, what: &str, copied: Result<()>) {
        match copied {
            Ok(()) => self.log.info(format!("Copied {what} for {title} into the project")),
            Err(e) => self
                .log
                .error(format!("The mods of {title} could not be copied: {e:#}")),
        }
    }

    /// Brings the card of an import into the project, once its mods are
    /// there, and puts the character in the scene when that was asked for.
    fn imported(&mut self, import: Import, title: String, missing: Vec<String>) {
        if self.project.is_none() {
            return;
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
        if import.first {
            // What a new project starts with is where its history starts.
            self.doc.clear_history();
            self.save();
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
    /// Opens a folder in Content as the user goes there: the one it leaves
    /// is kept to go back to.
    pub fn go_to_folder(&mut self, rel: &str) {
        if rel != self.subfolder {
            self.folder_back.push(self.subfolder.clone());
            self.folder_forward.clear();
        }
        self.open_folder(rel);
    }

    /// Goes back to the folder opened before, or forward again to the one
    /// left by going back. Folders that are gone are passed over.
    pub fn folder_history(&mut self, back: bool) {
        loop {
            let next = if back {
                self.folder_back.pop()
            } else {
                self.folder_forward.pop()
            };
            let Some(rel) = next else { return };
            if !self.project.root.join(&rel).is_dir() {
                continue;
            }
            let left = self.subfolder.clone();
            if back {
                self.folder_forward.push(left);
            } else {
                self.folder_back.push(left);
            }
            self.open_folder(&rel);
            self.asset = None;
            return;
        }
    }

    /// Gives the selected character a saved pose: its layer, over its
    /// animation or none, as one step of the history.
    fn load_pose(&mut self, rel: &str) {
        let pose = match crate::poses::read(&self.project.root.join(rel)) {
            Ok(p) => p,
            Err(e) => {
                self.log.error(format!("The pose could not be read: {e:#}"));
                return;
            }
        };
        let Some(id) = self
            .doc
            .selection()
            .filter(|o| matches!(o.content, Content::Character { .. }))
            .map(|o| o.id)
        else {
            return;
        };
        // A bone or target with keys keys the pose's value at the playhead,
        // so that the pose shows there.
        let keyed: Vec<(String, rk_chara::pose::BoneEdit)> = self
            .doc
            .scene()
            .sequence
            .track(id)
            .map(|t| t.keys.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .map(|name| {
                let edit = match rk_chara::pose::ik_key_target(&name) {
                    Some(target) => pose.layer.ik.get(target).map(|t| t.as_edit()).unwrap_or_default(),
                    None => pose.layer.fk.get(&name).copied().unwrap_or_default(),
                };
                (name, edit)
            })
            .collect();
        crate::keying::write_many(self, id, keyed, |content| {
            if let Content::Character {
                animation, pose_layer, ..
            } = content
            {
                *animation = pose.base.map(Box::new);
                *pose_layer = Some(Box::new(pose.layer)).filter(|l| !l.is_empty());
            }
        });
    }

    /// Saves the selected character's pose in the Poses folder, named
    /// after her, its animation starting where it is now; the file is
    /// shown in the Content panel, ready to be renamed.
    fn save_pose(&mut self) {
        if self.project.is_none() {
            return;
        }
        let Some(object) = self.doc.selection() else {
            return;
        };
        let Content::Character {
            animation, pose_layer, ..
        } = &object.content
        else {
            return;
        };
        let mut base = animation.as_deref().cloned();
        if let (Some(base), Some(phase)) = (&mut base, self.live.as_ref().and_then(|l| l.phase(object.id))) {
            base.start = phase;
        }
        let pose = crate::poses::Pose::new(base, pose_layer.as_deref().cloned().unwrap_or_default());
        let stem: String = object
            .name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || " -_'()".contains(c) {
                    c
                } else {
                    ' '
                }
            })
            .collect();
        let stem = stem.trim();
        let stem = if stem.is_empty() { "Pose" } else { stem };
        let dir = self.project.folder(project::POSES);
        let name = content::free_name(&dir, stem, Some(crate::poses::EXTENSION));
        let written = std::fs::create_dir_all(&dir)
            .map_err(anyhow::Error::from)
            .and_then(|()| crate::poses::write(&dir.join(&name), &pose));
        if let Err(e) = written {
            self.log.error(format!("The pose could not be saved: {e:#}"));
            return;
        }
        let rel = format!("{}/{name}", project::POSES);
        self.open_folder(project::POSES);
        self.asset = Some(PathBuf::from(&rel));
        self.content_renaming = Some((rel.clone(), content::stem(&rel).to_owned()));
        self.show_content = true;
    }

    /// Saves the selected character's keys in the Animations folder, named
    /// after her, with the sequence's length and frames; the file is shown
    /// in the Content panel, ready to be renamed.
    fn save_keys(&mut self) {
        if self.project.is_none() {
            return;
        }
        let Some(object) = self.doc.selection() else {
            return;
        };
        let sequence = &self.doc.scene().sequence;
        let Some(track) = sequence.track(object.id).filter(|t| !t.keys.is_empty()) else {
            return;
        };
        let file = crate::sequence::KeysFile {
            format: crate::sequence::FORMAT,
            length: sequence.length,
            fps: sequence.fps,
            keys: track.keys.clone(),
        };
        let stem: String = object
            .name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || " -_'()".contains(c) {
                    c
                } else {
                    ' '
                }
            })
            .collect();
        let stem = format!("{} keys", stem.trim());
        let dir = self.project.folder(project::ANIMATIONS);
        let name = content::free_name(&dir, stem.trim(), Some(crate::sequence::KEYS_EXTENSION));
        let written = std::fs::create_dir_all(&dir)
            .map_err(anyhow::Error::from)
            .and_then(|()| file.write(&dir.join(&name)));
        if let Err(e) = written {
            self.log.error(format!("The keys could not be saved: {e:#}"));
            return;
        }
        let rel = format!("{}/{name}", project::ANIMATIONS);
        self.open_folder(project::ANIMATIONS);
        self.asset = Some(PathBuf::from(&rel));
        self.content_renaming = Some((rel.clone(), content::stem(&rel).to_owned()));
        self.show_content = true;
    }

    /// Gives the selected character the keys of a file, in place of her
    /// own, as one step; the sequence grows to hold them.
    fn load_keys(&mut self, rel: &str) {
        let file = match crate::sequence::KeysFile::read(&self.project.root.join(rel)) {
            Ok(f) => f,
            Err(e) => {
                self.log.error(format!("The keys could not be read: {e:#}"));
                return;
            }
        };
        let Some(object) = self
            .doc
            .selection()
            .filter(|o| matches!(o.content, Content::Character { .. }))
        else {
            return;
        };
        let (id, name) = (object.id, object.name.clone());
        // Keys pose a character while FK is on: switched on in the same
        // step as the keys come.
        let mut object = object.clone();
        if let Content::Character { pose_layer, .. } = &mut object.content {
            let layer = pose_layer.get_or_insert_with(Default::default);
            if !layer.fk_on {
                layer.fk_on = true;
                self.doc.edit(object, true);
            }
        }
        let mut sequence = self.doc.scene().sequence.clone();
        sequence.length = sequence.length.max(file.length);
        sequence.track_mut(id, &name).keys = file.keys;
        sequence.tidy();
        self.doc.edit_sequence(sequence, false);
        self.doc.end_gesture();
    }

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
        if self.scripted || picks.len() == 1 {
            let game = if needs_game { self.games.get(&mods).ok() } else { None };
            let done = crate::animations::import(&self.project, game, picks, &folder, by_category);
            self.imported_animations(done, &folder);
            return;
        }
        // Many clips take a while: a job copies them, and can be stopped
        // between two of them.
        let game = if needs_game {
            self.games.shared(&mods).ok()
        } else {
            None
        };
        let (project, picks, total) = (self.project.clone(), picks.to_vec(), picks.len() as u64);
        let (theirs, root) = (folder.clone(), self.project.root.clone());
        self.jobs.spawn(
            "Importing animations",
            Show::Notice,
            move |p| {
                let done = crate::animations::import_watched(
                    &project,
                    game.as_deref(),
                    &picks,
                    &theirs,
                    by_category,
                    &mut |n, name| {
                        p.set(n as u64, total);
                        p.step(name);
                        !p.cancelled()
                    },
                );
                p.set(total, total);
                p.step(&crate::selector::counted(done.files.len(), "animation"));
                Ok(done)
            },
            move |st: &mut State, result| {
                // What was copied belongs to the project it was copied into.
                if let (Ok(done), true) = (result, st.project.root == root) {
                    st.imported_animations(done, &folder);
                }
            },
        );
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
    /// Copies a picked map file into the project.
    pub fn import_map_file(&mut self, path: &Path) {
        match crate::maps::copy_in(&self.project, path) {
            Ok((_, name)) => self.log.info(format!("Imported the map {name}")),
            Err(e) => self.log.error(format!("The map could not be imported: {e:#}")),
        }
    }

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
                h: None,
                from: crate::animations::From::File(path.to_owned()),
            };
            self.import_animations(&[pick], false);
        } else if extension == "zipmod" || extension == "zip" {
            self.importer = Some(crate::anim_import::Importer::new(Some(path.to_owned())));
        } else {
            self.log.error(format!(
                "{} is neither an animation file (.rkanim) nor a zipmod. A package (.rkpkg) is installed under \
                 Window, Packages first; its animations then show under Import from library.",
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
                // Named on its tile when it is made in the open folder, else
                // in the tree.
                self.renaming_in_tree = inside != self.subfolder;
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
        // On a map she stands on its floor, where the camera looks.
        let on_map = self.doc.scene().objects().iter().any(|o| o.kind() == Kind::Map);
        if on_map {
            if let Some(live) = self.live.as_ref().filter(|l| l.on_map()) {
                let at = crate::scene::clean(live.cam.orbit.camera().target);
                self.doc.move_now(id, at);
            }
            self.settle.push(id);
        }
        self.unfold_to(id);
        // The camera goes to her once she is built.
        self.focus_pending = Some(id);
    }

    /// Puts the map's movable pieces in the scene once its build knows
    /// them.
    pub fn list_map_pieces(&mut self) {
        let Some(live) = self.live.as_ref() else { return };
        let Some((map, pieces)) = live.map_pieces() else { return };
        // The doors come from the map's bundle, read beside the build.
        let Some(doors) = live.map_doors() else { return };
        let listed = matches!(
            self.doc.scene().get(map).map(|o| &o.content),
            Some(Content::Map {
                pieces_listed: true,
                ..
            }) | None
        );
        if listed || (pieces.is_empty() && doors.is_empty()) {
            return;
        }
        let pieces = pieces.to_vec();
        // Each kind's folder starts folded: a classroom has 25 desks.
        let folders = self.doc.list_pieces_now(map, &pieces, &doors);
        self.folded.extend(folders);
    }

    /// Sets the characters waiting for the map's floor on it, once the
    /// map's meshes are there, in the step that added them. The camera
    /// goes to the first.
    pub fn settle_on_map(&mut self) {
        if self.settle.is_empty() {
            return;
        }
        let Some(live) = self.live.as_ref().filter(|l| l.on_map()) else {
            return;
        };
        let waiting = std::mem::take(&mut self.settle);
        let scene = self.doc.scene();
        // The map alone: no one stands on another character.
        let characters: Vec<ObjectId> = scene
            .objects()
            .iter()
            .filter(|o| o.kind() == Kind::Character)
            .map(|o| o.id)
            .collect();
        let surface = live.surface(scene, characters);
        let view = live.cam.orbit.camera();
        let to_camera = view.eye - view.target;
        let mut placed = Vec::new();
        for id in waiting {
            if scene.get(id).is_none() {
                continue;
            }
            let at = scene.world(id).w_axis.truncate();
            let Some(spot) = crate::surface::stand_spot(at, to_camera, &surface) else {
                continue;
            };
            let local = scene.parent_world(id).inverse().transform_point3(spot);
            if local.is_finite() {
                placed.push((id, crate::scene::clean(local)));
            }
        }
        drop(surface);
        if let Some((first, _)) = placed.first() {
            self.focus_pending = Some(*first);
        }
        for (id, at) in placed {
            self.doc.move_now(id, at);
        }
    }

    /// Turns the camera's first framing of an opened scene to the nearest
    /// side from which the map's walls and furniture do not hide the
    /// character, once the map's meshes are there. A desk between her and
    /// the camera would otherwise fill the view.
    pub fn clear_start_view(&mut self) {
        let Some(live) = self.live.as_ref() else { return };
        if live.view_checked || !live.framed() {
            return;
        }
        let has_map = self.doc.scene().objects().iter().any(|o| o.kind() == Kind::Map);
        if has_map && !live.on_map() {
            return;
        }
        let (home, focus) = live.cam.home();
        let turned = match focus {
            Some(_) if has_map => {
                let scene = self.doc.scene();
                let characters = scene
                    .objects()
                    .iter()
                    .filter(|o| o.kind() == Kind::Character)
                    .map(|o| o.id)
                    .collect();
                let surface = live.surface(scene, characters);
                clear_side(home, &surface)
            }
            _ => None,
        };
        let live = self.live.as_mut().expect("looked at above");
        live.view_checked = true;
        if let Some(orbit) = turned {
            live.cam.rehome(orbit, focus);
        }
    }

    /// Opens the Maps library, to add a map to the scene.
    pub fn open_map_library(&mut self) {
        self.map_library = Some(crate::map_library::Shelf::open(self, true));
    }

    /// Opens the Maps library, to import a map into the project alone.
    pub fn open_map_import(&mut self) {
        self.map_library = Some(crate::map_library::Shelf::open(self, false));
    }

    /// Copies a map of the game, a package or a mod into the project, and
    /// with `to_scene` puts it in the scene. The copy is large, so a job
    /// does it.
    pub fn import_map(&mut self, entry: rk_map::list::Entry, to_scene: bool) {
        let name = rk_map::names::shown(&entry);
        let game = match self.games.shared(&self.project.mods_dir()) {
            Ok(game) => game,
            Err(e) => {
                self.log.error(format!("{name} could not be imported: {e:#}"));
                return;
            }
        };
        let project = self.project.clone();
        let job = format!("Importing {name}");
        let work = move |p: &crate::jobs::Progress| -> Result<(String, u64)> {
            let plan = crate::maps::plan(&game, &entry)?;
            p.set_bytes(0, plan.bytes());
            let file = crate::maps::import(&game, &project, &plan, &mut |bytes| {
                p.add(bytes);
                p.check()
            })?;
            Ok((file, plan.bytes()))
        };
        let done = move |st: &mut State, imported: Result<(String, u64)>| match imported {
            Ok((file, bytes)) => {
                st.log
                    .info(format!("Imported {name} as {file}, {}", mods::size_text(bytes)));
                if to_scene {
                    st.add_map(PathBuf::from(file));
                }
            }
            Err(e) if crate::jobs::was_cancelled(&e) => {
                st.log.info(format!("The import of {name} was cancelled"));
            }
            Err(e) => st.log.error(format!("{name} could not be imported: {e:#}")),
        };
        if self.scripted {
            let result = work(&crate::jobs::Progress::new(None));
            done(self, result);
            return;
        }
        self.jobs.spawn(&job, Show::Notice, work, done);
    }

    /// Puts a map of the project in the scene, named by its sidecar
    /// relative to the project. A scene has one map: one it has already
    /// is replaced, where it stands. The first map brings the camera to
    /// CharaStudio's starting view.
    pub fn add_map(&mut self, file: PathBuf) {
        let map = match crate::maps::MapFile::read(&self.project.resolve(&file)) {
            Ok(map) => map,
            Err(e) => {
                self.log.error(format!("The map could not be read: {e:#}"));
                return;
            }
        };
        let time = map.first_time();
        let there = self
            .doc
            .scene()
            .objects()
            .iter()
            .find(|o| o.kind() == Kind::Map)
            .cloned();
        let id = match there {
            Some(mut object) => {
                let (hide_walls, light_intensity, filter_decay, sun) = match object.content {
                    Content::Map {
                        hide_walls,
                        light_intensity,
                        filter_decay,
                        sun,
                        ..
                    } => (hide_walls, light_intensity, filter_decay, sun),
                    _ => (true, 1.0, 0.5, None),
                };
                object.name = self.doc.scene().free_name(&map.name);
                object.content = Content::Map {
                    map: file,
                    time,
                    hide_walls,
                    light_intensity,
                    filter_decay,
                    sun,
                    pieces_listed: false,
                };
                // The old map's pieces go with it; the new map lists its own.
                self.doc.remove_pieces_now(object.id);
                let id = object.id;
                self.doc.edit(object, false);
                id
            }
            None => {
                let id = self.doc.add(&map.name, Content::map(file, time));
                // The effects that follow the map, where the scene has none.
                self.doc.add_map_effects();
                // The characters stand where the map has a floor, and the
                // camera goes to the first.
                self.settle = self
                    .doc
                    .scene()
                    .objects()
                    .iter()
                    .filter(|o| {
                        o.kind() == Kind::Character && o.parent.is_none_or(|p| self.doc.scene().is_kind_folder(p))
                    })
                    .map(|o| o.id)
                    .collect();
                if let Some(live) = self.live.as_mut() {
                    live.studio_camera();
                }
                id
            }
        };
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

/// The view nearest to `orbit`, turned around its target, from which
/// nothing stands between the character and the camera: from her middle
/// and her knees if a side allows, else from her middle. `None` when
/// `orbit` is as clear as any side, or no side is.
fn clear_side(orbit: crate::camera::Orbit, surface: &dyn crate::surface::Surface) -> Option<crate::camera::Orbit> {
    let open_from = |o: &crate::camera::Orbit, drop: f32| {
        let from = o.target - glam::Vec3::Y * drop;
        let to = o.camera().eye - from;
        let length = to.length();
        surface.meet(from, to / length).is_none_or(|far| far >= length - 0.05)
    };
    // Both ways round, a twelfth of a turn at a time.
    let sides = || {
        std::iter::once(orbit).chain(
            (1..=6)
                .flat_map(|k| [k as f32, -(k as f32)])
                .map(|k| crate::camera::Orbit {
                    yaw: orbit.yaw + k * std::f32::consts::TAU / 12.0,
                    ..orbit
                }),
        )
    };
    for drops in [&[0.0, 0.45][..], &[0.0][..]] {
        if let Some(side) = sides().find(|o| drops.iter().all(|d| open_from(o, *d))) {
            return (side != orbit).then_some(side);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Place;

    #[test]
    fn keys_saved_as_an_animation_go_on_another_character() {
        let home = scratch("keys");
        let root = home.join("P");
        let project = Project::create(&root, "P").unwrap();
        let mut st = State::new(project, Scene::new());
        let anna = st.doc.add_character("Anna", "Characters/anna.png".into());
        let bea = st.doc.add_character("Bea", "Characters/bea.png".into());
        st.doc.selected = Some(anna);
        st.clock = 1.0;
        let neck = rk_chara::pose::BoneEdit {
            rot: glam::Quat::from_rotation_y(0.4),
            ..Default::default()
        };
        crate::keying::paste_key(&mut st, anna, "cf_j_neck", neck);
        st.run(Command::SaveKeys);
        let rel = "Animations/Anna keys.rkkeys".to_owned();
        assert!(root.join(&rel).is_file());
        st.doc.selected = Some(bea);
        st.run(Command::LoadKeys(rel));
        let keys = |st: &State, id| st.doc.scene().sequence.track(id).map(|t| t.keys.clone());
        assert_eq!(keys(&st, bea), keys(&st, anna));
        assert!(matches!(
            &st.doc.scene().get(bea).unwrap().content,
            Content::Character { pose_layer: Some(l), .. } if l.fk_on
        ));
        // One step: an undo takes the keys and FK away again.
        st.doc.undo();
        assert_eq!(keys(&st, bea), None);
        assert!(!matches!(
            &st.doc.scene().get(bea).unwrap().content,
            Content::Character { pose_layer: Some(l), .. } if l.fk_on
        ));
        let _ = std::fs::remove_dir_all(&home);
    }

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
    fn the_selection_drops_to_the_ground_in_one_step() {
        let mut st = State::new(Project::none(), Scene::new());
        let folder = st.doc.add("Folder", Content::Folder);
        st.doc.selected = Some(folder);
        let high = st.doc.add("High", Content::Item);
        st.doc.selected = None;
        let low = st.doc.add("Low", Content::Item);
        for (id, position) in [
            (folder, [0.0, 1.0, 0.0]),
            (high, [0.5, 2.0, 0.0]),
            (low, [1.0, -3.0, 0.0]),
        ] {
            let mut object = st.doc.scene().get(id).unwrap().clone();
            object.transform.position = position;
            st.doc.edit(object, false);
        }
        st.doc.clear_history();
        // One is three metres up in its folder, the other under the ground.
        st.doc.select_many(&[high, low]);
        st.run(Command::SnapToFloor);
        let at = |st: &State, id| st.doc.scene().world(id).w_axis.truncate();
        assert_eq!(at(&st, high), glam::Vec3::new(0.5, 0.0, 0.0));
        assert_eq!(at(&st, low), glam::Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(st.doc.scene().get(high).unwrap().transform.position, [0.5, -1.0, 0.0]);
        st.run(Command::Undo);
        assert_eq!(at(&st, high).y, 3.0);
        assert_eq!(at(&st, low).y, -3.0);
        assert!(!st.doc.can_undo());
    }

    #[test]
    fn the_selection_is_attached_to_the_object_selected_last() {
        let mut st = State::new(Project::none(), Scene::new());
        let light = st
            .doc
            .selection()
            .map(|o| o.id)
            .or(st.doc.scene().objects().first().map(|o| o.id))
            .unwrap();
        st.doc.selected = None;
        let cup = st.doc.add("Cup", Content::Item);
        st.doc.selected = None;
        let table = st.doc.add("Table", Content::Item);
        let parent = |st: &State, id| st.doc.scene().get(id).unwrap().parent;
        let items = parent(&st, table);
        st.doc.select_many(&[cup, light, table]);
        st.run(Command::AttachToLast);
        assert_eq!(
            (parent(&st, cup), parent(&st, light), parent(&st, table)),
            (Some(table), Some(table), items)
        );
        // Detached, they are at the top again.
        st.doc.select_many(&[cup, light]);
        st.run(Command::Unparent);
        assert_eq!((parent(&st, cup), parent(&st, light)), (None, None));
    }

    #[test]
    fn the_mouse_goes_back_and_forward_through_folders() {
        let home = scratch("history");
        let mut st = editor(&home);
        for folder in ["Animations/Dances", "Animations/Dances/Slow"] {
            std::fs::create_dir_all(st.project.root.join(folder)).unwrap();
        }
        st.go_to_folder("Animations");
        st.go_to_folder("Animations/Dances");
        st.go_to_folder("Animations/Dances/Slow");
        st.folder_history(true);
        assert_eq!(st.subfolder, "Animations/Dances");
        st.folder_history(true);
        assert_eq!(st.subfolder, "Animations");
        st.folder_history(false);
        assert_eq!(st.subfolder, "Animations/Dances");
        // A folder that is gone is passed over.
        std::fs::remove_dir_all(st.project.root.join("Animations/Dances/Slow")).unwrap();
        st.folder_history(false);
        assert_eq!(st.subfolder, "Animations/Dances");
        // Going somewhere new forgets the way forward.
        st.folder_history(true);
        st.go_to_folder("Characters");
        st.folder_history(false);
        assert_eq!(st.subfolder, "Characters");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn delete_acts_in_the_panel_last_worked_in() {
        let home = scratch("delete-focus");
        let mut st = editor(&home);
        let ctx = egui::Context::default();
        let light = st.doc.scene().key_light().map(|o| o.id).unwrap();
        st.doc.select_many(&[light]);
        // Working in Content: the scene's selection stays.
        st.focus = crate::panels::Panel::Content;
        st.act(crate::keymap::Action::Delete, &ctx);
        assert!(st.doc.scene().get(light).is_some());
        // In the Outliner it goes.
        st.focus = crate::panels::Panel::Outliner;
        st.act(crate::keymap::Action::Delete, &ctx);
        assert!(st.doc.scene().get(light).is_none());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_card_added_from_content_goes_into_the_characters_folder() {
        let home = scratch("content-folder");
        let mut st = editor(&home);
        std::fs::write(home.join("anna.png"), b"card").unwrap();
        let card = st.project.import_card(&home.join("anna.png"), "Anna").unwrap();
        st.project.save().unwrap();
        st.refresh_content();
        let folder_of = |st: &State| {
            let scene = st.doc.scene();
            let parent = scene.get(st.doc.selected.unwrap()).unwrap().parent;
            parent.and_then(|p| scene.get(p)).map(|o| o.name.clone())
        };
        // Whatever is selected: the key light, then an effect.
        for selected in ["Key light", "Bloom"] {
            st.doc.selected = st
                .doc
                .scene()
                .objects()
                .iter()
                .find(|o| o.name == selected)
                .map(|o| o.id);
            st.run(Command::AddCard(PathBuf::from(&card)));
            assert_eq!(
                folder_of(&st).as_deref(),
                Some("Characters"),
                "with {selected} selected"
            );
        }
        let folders = st
            .doc
            .scene()
            .objects()
            .iter()
            .filter(|o| o.name == "Characters")
            .count();
        assert_eq!(folders, 1);
        let _ = std::fs::remove_dir_all(&home);
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
        // A new scene's lights and effects, in their folders.
        let start = names(&st);
        let with = |rows: &[(usize, String)]| [start.clone(), rows.to_vec()].concat();
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
            with(&[row(0, "Folder"), row(1, "Camera"), row(1, "Folder 2")])
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
            with(&[row(0, "Folder"), row(0, "Camera"), row(0, "Folder 2")])
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
            names(&st)[start.len() + 3..],
            [row(0, "Folder 3"), row(1, "Camera 2"), row(1, "Folder 4")]
        );
        st.run(Command::Cut);
        assert_eq!(names(&st).len(), start.len() + 3);
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
        assert_eq!(names(&st), with(&[row(0, "Folder")]));
        st.run(Command::Undo);
        assert_eq!(names(&st).len(), start.len() + 3);
        assert!(st.doc.scene().can_place(camera, Place::Into(None)));
        let _ = std::fs::remove_dir_all(&home);
    }
}
