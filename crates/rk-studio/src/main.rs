// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Renkatsu Studio, the editor, after the editor design: Unity's layout,
//! Unreal's modes drawn over the viewport, the menu row and docked panels
//! in the designs of `design.rs`, on the scene model of `scene.rs`, which
//! the viewport draws live.

// A window program on Windows: no console opens beside it.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod about;
mod anim_import;
mod animations;
mod animator;
mod bones;
mod brand;
mod camera;
mod card_import;
mod chrome;
mod content;
mod content_panel;
mod design;
mod dialogs;
mod gizmo;
mod gizmo_pass;
mod grid_pass;
mod jobs;
mod keycaps;
mod keying;
mod keymap;
mod kinds;
mod landing;
mod library;
mod live;
mod map_library;
mod map_menu;
mod map_pieces;
mod maps;
mod modes;
mod mods;
mod mugshot;
mod news_pictures;
mod packages;
mod panels;
mod poses;
mod preferences;
mod profile;
mod project;
mod properties;
mod scene;
mod selector;
mod self_update;
mod sequence;
mod settings;
mod shortcuts;
mod sources;
mod state;
mod surface;
mod theme;
mod timeline;
mod viewport;
mod widgets;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use design::{Design, Overlays, TitleBar};
use eframe::egui_wgpu::wgpu;
use egui_dock::{DockArea, DockState};
use panels::Panel;
use project::Project;
use state::{Command, Look, Playback, Rail, State};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Bottom {
    Content,
    Console,
    Timeline,
}

/// The ways of showing work that scripted pictures can ask for, on
/// sample jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Picture {
    /// The splash of the start.
    Startup,
    /// A project halfway open, in the selector's window.
    Opening,
    /// Notices in the editor's corner and the status bar's line.
    Notices,
}

#[derive(Parser)]
#[command(name = "renkatsu", version, about = "Renkatsu Studio")]
struct Cli {
    /// The design to show.
    #[arg(long, value_enum, default_value = "d")]
    design: Design,
    /// Picture to show in the viewport while the scene has no character.
    #[arg(long)]
    viewport_image: Option<PathBuf>,
    /// The project to open, by its folder. A folder that holds no project
    /// gets a new one. Without this the last project opens, or a new one
    /// is made in the documents folder.
    #[arg(long)]
    project: Option<PathBuf>,
    /// Card to import into the project and put in the scene at the start.
    /// May be given several times. A new project then starts without its
    /// default character.
    #[arg(long)]
    card: Vec<PathBuf>,
    /// Card to import into the project at the start as File, Import card
    /// does, with its question about mods. May be given several times.
    #[arg(long)]
    import: Vec<PathBuf>,
    /// Save a picture of the window here after a few frames, then quit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Window size, as WIDTHxHEIGHT. The editor's is 1600x900 unless this
    /// says otherwise.
    #[arg(long)]
    size: Option<String>,
    /// Where the window opens on the desktop, as X,Y, and without taking
    /// the keyboard: for scripted pictures on a screen of their own.
    #[arg(long, hide = true, value_delimiter = ',')]
    at: Vec<f32>,
    /// Start on the project selector even where the settings or a
    /// screenshot would not, to picture it.
    #[arg(long, hide = true)]
    project_selector: bool,
    /// Show work in progress in this way, on sample jobs, to picture it.
    #[arg(long, hide = true, value_enum)]
    loading: Option<Picture>,
    /// Show these news on the home page, to picture it: a news file and,
    /// after a comma, GitHub's list of releases as a JSON file.
    #[arg(long, hide = true)]
    pretend_news: Option<String>,
    /// Show a newer Renkatsu as found, to picture the notice.
    #[arg(long, hide = true)]
    pretend_update: Option<String>,
    /// Start installing the newer Renkatsu found at the start, as a click
    /// on it does, to try the update.
    #[arg(long, hide = true)]
    install_update: bool,
    /// Start with the Packages window open, to picture it.
    #[arg(long, hide = true)]
    packages: bool,
    /// Start with the window that imports from a package open, to
    /// picture it.
    #[arg(long, hide = true)]
    package_browser: bool,
    /// The mode to start in, by name.
    #[arg(long, default_value = "select")]
    mode: String,
    /// The object selected at the start, by its name or its card's file
    /// name, or "none". Without it the first `--card` character is.
    #[arg(long)]
    select: Option<String>,
    /// Start on the Pose page of the selected character with this bone
    /// chosen, to picture it.
    #[arg(long, hide = true)]
    pose_bone: Option<String>,
    /// Start on this page of Properties, by its place, to picture it.
    #[arg(long, hide = true)]
    property_tab: Option<usize>,
    /// Show the sun's dome as Ctrl+L shows it, to picture it.
    #[arg(long, hide = true)]
    sun_preview: bool,
    /// Mirror the selected character's pose once she is built, to
    /// picture it.
    #[arg(long, hide = true)]
    mirror_pose: bool,
    /// Bake the selected character's animation into its pose once it is
    /// built, to picture it.
    #[arg(long, hide = true)]
    bake_pose: bool,
    /// Switch IK on for the selected character once she is built, with
    /// this target (a name of CharaStudio's IKTargetEN) moved by X,Y,Z in
    /// her own space, as `LeftHand:0,0.15,0.1`, to picture IK.
    #[arg(long, hide = true)]
    ik_move: Vec<String>,
    /// Give the selected character these hand patterns, as `LEFT,RIGHT`
    /// numbers of CharaStudio's lists, to picture them.
    #[arg(long, hide = true)]
    hand_patterns: Option<String>,
    /// Give the selected character this face, as
    /// `EYEBROWS,EYES,MOUTH[,EYES_OPEN[,BLINK[,MOUTH_OPEN]]]`, to picture it.
    #[arg(long, hide = true)]
    face: Option<String>,
    /// Start with this IK target (a name of CharaStudio's IKTargetEN)
    /// chosen for the gizmo, to picture it.
    #[arg(long, hide = true)]
    pose_ik: Option<String>,
    /// Save the selected character's pose once she is built (after a
    /// bake or a mirror asked for), to picture it.
    #[arg(long, hide = true)]
    save_pose: bool,
    /// Give the selected character this pose file of the project at the
    /// start, to picture it.
    #[arg(long, hide = true)]
    load_pose: Option<String>,
    /// Start with the mode's tool palette open.
    #[arg(long)]
    palette_open: bool,
    /// Start with the mode rail shrunk to icons.
    #[arg(long)]
    rail_icons: bool,
    /// Start with every viewport overlay hidden.
    #[arg(long)]
    overlays_hidden: bool,
    /// Start with the viewport taking the whole window.
    #[arg(long)]
    maximised: bool,
    /// The panel shown in the bottom group at the start.
    #[arg(long, value_enum, default_value = "content")]
    bottom: Bottom,
    /// Click at X,Y before the screenshot, to picture menus and states.
    /// May be given several times; the clicks happen in order. A point
    /// that starts with `r`, as in `r120,48`, is clicked with the right
    /// button, one that starts with `d` is double clicked, and one that
    /// starts with `c` is clicked with Ctrl held.
    #[arg(long, value_parser = parse_click)]
    click: Vec<Click>,
    /// Leave the pointer at X,Y for the screenshot, to picture hover.
    #[arg(long, value_parser = parse_point)]
    hover: Option<egui::Pos2>,
    /// The window's title bar, of the three offered.
    #[arg(long, value_enum, default_value = "a")]
    title_bar: TitleBar,
    /// Start with the camera turned around the character by a drag of
    /// X,Y points, to picture a view that left its starting framing.
    #[arg(long, value_parser = parse_point, allow_hyphen_values = true)]
    camera_turn: Option<egui::Pos2>,
    /// Start with the camera here, as `EYE:TARGET`, each `x,y,z` in
    /// metres, focused on nothing, to picture a close view.
    #[arg(long, hide = true, value_parser = parse_camera, allow_hyphen_values = true)]
    camera: Option<(glam::Vec3, glam::Vec3)>,
    /// Start with the camera focused on nothing.
    #[arg(long)]
    free_camera: bool,
    /// Start with this fly speed and its meter showing.
    #[arg(long)]
    fly_speed: Option<f32>,
    /// Start with the selected character in this animation of
    /// CharaStudio's list, as GROUP/CATEGORY/NUMBER, to picture it.
    #[arg(long, hide = true, alias = "pose")]
    animation: Option<String>,
    /// Import these animations of CharaStudio's list into the project at
    /// the start, each as GROUP/CATEGORY/NUMBER or GROUP/CATEGORY for a
    /// whole category, to picture a project that has animations.
    #[arg(long, hide = true)]
    import_animation: Vec<String>,
    /// Start with the window that imports animations open, to picture it.
    #[arg(long, hide = true)]
    animation_importer: bool,
    /// Type this text before the screenshot, into whatever has the
    /// keyboard after the clicks, or after the first `--type-after` of
    /// them.
    #[arg(long, hide = true)]
    type_text: Option<String>,
    #[arg(long, hide = true)]
    type_after: Option<usize>,
    /// Start with the scene paused after this many seconds of play, to
    /// picture a frame of an animation.
    #[arg(long, hide = true)]
    scene_time: Option<f32>,
    /// Start with the Content panel on this folder, by its place in the
    /// list (1 is Animations).
    #[arg(long, hide = true)]
    folder: Option<usize>,
    /// The transform tool to start with, by name: view, select, move,
    /// rotate, scale or pose.
    #[arg(long, hide = true)]
    tool: Option<String>,
    /// The axes the gizmos follow at the start.
    #[arg(long, hide = true, value_enum, default_value = "global")]
    gizmo_space: gizmo::Space,
    /// The point the gizmos sit at at the start.
    #[arg(long, hide = true, value_enum, default_value = "origin")]
    gizmo_pivot: gizmo::Pivot,
    /// Press the left button at X1,Y1 after the clicks, drag to X2,Y2 and
    /// hold it there for the screenshot, as X1,Y1:X2,Y2: to picture a
    /// gizmo being dragged or a selection box.
    #[arg(long, hide = true, value_parser = parse_drag)]
    drag: Option<(egui::Pos2, egui::Pos2)>,
    /// Rest the pointer at X,Y after the clicks and turn the wheel by DY
    /// points (down is negative), as X,Y,DY: to picture something
    /// scrolled.
    #[arg(long, hide = true, value_parser = parse_wheel)]
    wheel: Option<(egui::Pos2, f32)>,
    /// Press these keys after the clicks, one a frame, each as the
    /// settings write a key: `ctrl+z`.
    #[arg(long, hide = true)]
    key: Vec<String>,
    /// Start with the inputs shown in the viewport's corner.
    #[arg(long, hide = true)]
    show_inputs: bool,
    /// Start with the Keyboard shortcuts window open, to picture it.
    #[arg(long, hide = true)]
    shortcuts: bool,
    /// Start with Help, About Renkatsu open, to picture it.
    #[arg(long, hide = true)]
    about: bool,
    /// Print the frames that took longer than a tenth of a second when
    /// the window closes, to measure freezes.
    #[arg(long, hide = true)]
    frame_report: bool,
    /// Close the window after this many seconds.
    #[arg(long, hide = true)]
    quit_after: Option<f32>,
    /// Import this map of CharaStudio's list into the project, by its
    /// number or its name, and put it in the scene at the start.
    #[arg(long, hide = true)]
    map: Option<String>,
    /// The map's time of day: day, evening or night.
    #[arg(long, hide = true)]
    map_time: Option<String>,
    /// Start with the Maps library open, to picture it.
    #[arg(long, hide = true)]
    map_library: bool,
    /// Start with a news post, once opened, scrolled down by this many
    /// points, to picture its pictures.
    #[arg(long, hide = true)]
    news_scroll: Option<f32>,
    /// Start with the Maps library scrolled down by this many points.
    #[arg(long, hide = true)]
    map_scroll: Option<f32>,
    /// Import this card and put it in the scene nine seconds after the
    /// start, to measure adding a character.
    #[arg(long, hide = true)]
    bench_add: Option<PathBuf>,
}

/// Puts a map of CharaStudio's list in the scene, by its number, its
/// level or its name: the project's own copy when it has one, else a copy
/// imported now. `time` is the time of day, by name.
fn add_map(state: &mut state::State, which: &str, time: Option<&str>) -> Result<()> {
    let time = match time {
        None => None,
        Some(name) => Some(
            scene::MapTime::ALL
                .into_iter()
                .find(|t| t.label().eq_ignore_ascii_case(name))
                .with_context(|| format!("{name:?} is not day, evening or night"))?,
        ),
    };
    let same = |a: &str| a.trim().eq_ignore_ascii_case(which);
    let owned = maps::in_project(&state.project.root)
        .into_iter()
        .find(|m| m.map.number.to_string() == which || same(&m.map.name) || same(&m.map.level));
    match owned {
        Some(m) => state.add_map(PathBuf::from(m.file)),
        None => {
            let game = state.games.get(&state.project.mods_dir())?;
            let entry = rk_map::list::read(game, false)
                .into_iter()
                .find(|e| {
                    e.number.to_string() == which || same(&e.level) || same(&e.name) || same(&rk_map::names::shown(e))
                })
                .with_context(|| format!("CharaStudio's list has no map {which:?}"))?;
            state.import_map(entry, true);
        }
    }
    let map = state
        .doc
        .scene()
        .objects()
        .iter()
        .find(|o| o.kind() == kinds::Kind::Map)
        .cloned()
        .context("the map could not be put in the scene")?;
    if let (Some(time), scene::Content::Map { .. }) = (time, &map.content) {
        let mut edited = map;
        if let scene::Content::Map { time: worn, .. } = &mut edited.content {
            *worn = time;
        }
        state.doc.edit(edited, false);
    }
    Ok(())
}

/// A scripted click: where, with which button, whether twice, and
/// whether with Ctrl held.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Click {
    at: egui::Pos2,
    right: bool,
    double: bool,
    ctrl: bool,
}

fn parse_click(text: &str) -> Result<Click, String> {
    let (right, double, ctrl, point) = match text.as_bytes().first() {
        Some(b'r') => (true, false, false, &text[1..]),
        Some(b'd') => (false, true, false, &text[1..]),
        Some(b'c') => (false, false, true, &text[1..]),
        _ => (false, false, false, text),
    };
    Ok(Click {
        at: parse_point(point)?,
        right,
        double,
        ctrl,
    })
}

fn parse_drag(text: &str) -> Result<(egui::Pos2, egui::Pos2), String> {
    let (from, to) = text
        .split_once(':')
        .ok_or_else(|| "a drag must look like 120,48:200,90".to_owned())?;
    Ok((parse_point(from)?, parse_point(to)?))
}

fn parse_wheel(text: &str) -> Result<(egui::Pos2, f32), String> {
    let (point, dy) = text
        .rsplit_once(',')
        .ok_or_else(|| "a wheel turn must look like 120,48,-300".to_owned())?;
    Ok((
        parse_point(point)?,
        dy.trim().parse().map_err(|_| "the turn is a number".to_owned())?,
    ))
}

fn parse_camera(text: &str) -> Result<(glam::Vec3, glam::Vec3), String> {
    let point = |t: &str| -> Option<glam::Vec3> {
        let v: Vec<f32> = t.split(',').map(|n| n.trim().parse().ok()).collect::<Option<_>>()?;
        (v.len() == 3).then(|| glam::Vec3::new(v[0], v[1], v[2]))
    };
    text.split_once(':')
        .and_then(|(eye, target)| Some((point(eye)?, point(target)?)))
        .ok_or_else(|| "a camera must look like 0,1.5,1:0,1.4,0".to_owned())
}

fn parse_point(text: &str) -> Result<egui::Pos2, String> {
    text.split_once(',')
        .and_then(|(x, y)| Some(egui::Pos2::new(x.trim().parse().ok()?, y.trim().parse().ok()?)))
        .ok_or_else(|| "a point must look like 120,48".to_owned())
}

/// Frames between two scripted clicks, so that menus have opened.
const CLICK_FRAMES: u32 = 4;

struct Studio {
    look: Look,
    state: State,
    dock: DockState<Panel>,
    bottom: Panel,
    screenshot: Option<PathBuf>,
    clicks: Vec<Click>,
    /// Text typed once the clicks are done, or once this many are.
    type_text: Option<String>,
    type_after: Option<usize>,
    hover: Option<egui::Pos2>,
    drag: Option<(egui::Pos2, egui::Pos2)>,
    /// The pointer's rest and a wheel turn once the clicks are done.
    wheel: Option<(egui::Pos2, f32)>,
    /// Keys pressed once the clicks are done, one a frame.
    keys: Vec<keymap::Chord>,
    frames: u32,
    /// The window title as last given to the desktop.
    title: String,
    /// The window was told to close.
    closing: bool,
    /// The project of a scripted run that named none: a folder of its own
    /// that goes away with the window.
    temporary: Option<PathBuf>,
    /// The size of the editor's window, for when the project selector's
    /// own smaller window gives way to it.
    editor_size: egui::Vec2,
    /// The window has the size of the project selector's own.
    small: bool,
    /// Seconds since a project opened from the selector, while the editor
    /// still fades in.
    entering: Option<f32>,
    /// When the splash of the start began to show, in the window's
    /// seconds; `None` once it is gone.
    splash: Option<f64>,
    /// The splash stays, for a picture of it.
    hold_splash: bool,
    /// The frames of the run, when they are measured.
    report: Option<FrameReport>,
    quit_after: Option<f32>,
    bench_add: Option<PathBuf>,
}

/// How long the frames of a run took, to measure freezes.
struct FrameReport {
    start: std::time::Instant,
    last: std::time::Instant,
    /// When each long frame started, in seconds, and how long it took.
    long: Vec<(f32, f32)>,
    /// Every frame's time after the first seconds, which load the scene.
    settled: Vec<f32>,
}

impl FrameReport {
    fn new() -> FrameReport {
        let now = std::time::Instant::now();
        FrameReport {
            start: now,
            last: now,
            long: Vec::new(),
            settled: Vec::new(),
        }
    }

    fn frame(&mut self) {
        let now = std::time::Instant::now();
        let gap = (now - self.last).as_secs_f32();
        if gap > 0.1 {
            self.long.push(((self.last - self.start).as_secs_f32(), gap));
        }
        if (self.last - self.start).as_secs_f32() > 8.0 {
            self.settled.push(gap);
        }
        self.last = now;
    }

    fn print(&self) {
        for (at, gap) in &self.long {
            println!("frame at {at:.2} s took {gap:.2} s");
        }
        let longest = self.long.iter().map(|l| l.1).fold(0.0, f32::max);
        let sum: f32 = self.long.iter().map(|l| l.1).sum();
        println!(
            "long frames: {}, longest {longest:.2} s, together {sum:.2} s",
            self.long.len()
        );
        // Frame times once the scene is in.
        let mut times = self.settled.clone();
        times.sort_by(f32::total_cmp);
        if !times.is_empty() {
            let mean = times.iter().sum::<f32>() / times.len() as f32;
            let p95 = times[(times.len() * 95 / 100).min(times.len() - 1)];
            println!(
                "settled frames: {}, mean {:.2} ms ({:.0} fps), 95% under {:.2} ms",
                times.len(),
                mean * 1000.0,
                1.0 / mean,
                p95 * 1000.0
            );
        }
    }
}

impl Drop for Studio {
    fn drop(&mut self) {
        if let Some(report) = &self.report {
            report.print();
        }
        if let Some(dir) = &self.temporary {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Whether the start shows the project selector rather than a project: no
/// project or card was named, and the settings ask for it. A scripted
/// picture shows it only when it names a layout.
fn starts_on_selector(cli: &Cli, settings: &settings::Settings) -> bool {
    cli.project.is_none()
        && cli.card.is_empty()
        && cli.import.is_empty()
        && (cli.project_selector || (settings.show_project_selector && cli.screenshot.is_none()))
}

/// The editor's window size, from `--size`.
fn editor_size(cli: &Cli) -> Result<egui::Vec2> {
    let text = cli.size.as_deref().unwrap_or("1600x900");
    text.split_once('x')
        .and_then(|(w, h)| Some(egui::Vec2::new(w.parse().ok()?, h.parse().ok()?)))
        .context("--size must look like 1600x900")
}

/// The project the editor starts on: none, under the project selector, or
/// the one asked for, else the last one that is still there, else a new
/// one. Returns it with its scene, whether it was just made, and its folder
/// when that is a temporary one.
fn starting_project(cli: &Cli, settings: &settings::Settings) -> Result<(State, bool, Option<PathBuf>)> {
    if starts_on_selector(cli, settings) {
        // No project is read, and the game is not looked at, until the
        // user chooses one.
        return Ok((State::new(Project::none(), scene::Scene::new()), false, None));
    }
    let fresh = |root: &std::path::Path, name: &str| -> Result<State> {
        Ok(State::new(Project::create(root, name)?, scene::Scene::new()))
    };
    let opened = |root: &std::path::Path| -> Result<State> {
        let (project, scene, note) = Project::open(root)?;
        let mut state = State::new(project, scene);
        if let Some(note) = note {
            state.log.warn(note);
        }
        Ok(state)
    };
    if let Some(root) = &cli.project {
        if Project::exists(root) {
            return Ok((opened(root)?, false, None));
        }
        let name = root
            .file_name()
            .map_or("Untitled project".to_owned(), |n| n.to_string_lossy().into_owned());
        return Ok((fresh(root, &name)?, true, None));
    }
    if cli.screenshot.is_some() {
        // A scripted picture must not depend on, or add to, the user's own
        // projects.
        let root = std::env::temp_dir().join(format!("renkatsu-scripted-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        return Ok((fresh(&root, "Untitled project")?, true, Some(root)));
    }
    let mut problem = None;
    if let Some(last) = settings.last_project() {
        match opened(last) {
            Ok(state) => return Ok((state, false, None)),
            Err(e) => problem = Some(format!("{} could not be opened: {e:#}", last.display())),
        }
    }
    let home = project::home().context("no documents folder to make a project in")?;
    let root = project::free_folder(&home, "Untitled project");
    let name = root.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let mut state = fresh(&root, &name)?;
    if let Some(problem) = problem {
        state.log.error(problem);
    }
    Ok((state, true, None))
}

impl Studio {
    fn new(cc: &eframe::CreationContext, cli: &Cli, settings: settings::Settings) -> Result<Self> {
        let look = Look {
            t: theme::THEME,
            d: cli.design.spec(),
        };
        cc.egui_ctx.set_fonts(fonts());
        cc.egui_ctx.all_styles_mut(|s| {
            look.t.apply(s, look.d.text);
            // A picture must not catch a menu halfway through fading in.
            if cli.screenshot.is_some() {
                s.animation_time = 0.0;
            }
        });

        let (mut state, fresh, temporary) = starting_project(cli, &settings)?;
        state.vsync_at_start = settings.frame_rate.vsync();
        state.settings = settings;
        state.keys = keymap::Keymap::with(&state.settings.keys);
        state.settings.show_inputs |= cli.show_inputs;
        state.shortcuts_open = cli.shortcuts;
        state.about_open = cli.about;
        state.scripted = cli.screenshot.is_some();
        state.jobs.wake = Some(cc.egui_ctx.clone());
        state.fetch_feed();
        if let Some(y) = cli.news_scroll {
            cc.egui_ctx
                .data_mut(|d| d.insert_temp(egui::Id::new(crate::landing::SCROLL_START), y));
        }
        if let Some(files) = &cli.pretend_news {
            let (posts, releases) = files.split_once(',').unwrap_or((files, ""));
            let read = |file: &str| std::fs::read_to_string(file).with_context(|| format!("reading {file}"));
            let now = settings::now();
            let posts = rk_packages::news::parse_posts(
                &read(posts)?,
                env!("CARGO_PKG_VERSION"),
                &rk_packages::news::today(now),
            )?;
            let releases = match releases {
                "" => Vec::new(),
                file => rk_packages::news::parse_releases(&read(file)?)?.0,
            };
            state.news = rk_packages::news::merge(posts, releases);
        }
        if let Some(version) = &cli.pretend_update {
            state.update = Some(rk_packages::update::Release {
                version: version.clone(),
                url: "https://github.com/Mihnea8848/renkatsu/releases".to_owned(),
            });
        }
        if cli.install_update {
            state.update_clicked(&cc.egui_ctx);
        }
        let selecting = state.project.is_none();
        if selecting {
            state.show_selector();
        }
        state.mode = modes::MODES
            .iter()
            .position(|m| m.name.eq_ignore_ascii_case(&cli.mode))
            .with_context(|| format!("unknown mode {:?}", cli.mode))?;
        state.palette_open = cli.palette_open;
        state.overlays_hidden = cli.overlays_hidden;
        state.maximised = cli.maximised;
        if cli.rail_icons {
            state.rail = Rail::Icons;
        }
        state.library.eager = cli.screenshot.is_some();
        state.packages.open = cli.packages;
        if cli.package_browser {
            state.packages.browse(rk_core::Config::load().ok().as_ref());
        }
        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            state.library.mugshots = Some(mugshot::Mugshots::new(
                render_state.device.clone(),
                render_state.queue.clone(),
                cc.egui_ctx.clone(),
            ));
            let mut live = live::Live::new(render_state)?;
            if state.scripted {
                // A picture must not catch the camera on its way, nor a
                // character halfway built.
                live.cam.glide_time = 0.0;
                live.patient = true;
            }
            state.live = Some(live);
        }
        if fresh {
            // A project that is pictured as still opening has no character yet.
            state.start_fresh(cli.card.is_empty() && cli.loading != Some(Picture::Opening));
        }
        state.remember();
        for card in &cli.card {
            rk_card::Card::load(card).with_context(|| format!("loading {}", card.display()))?;
            state.imports.push_back(dialogs::Import {
                path: card.clone(),
                place: true,
                // A scripted run has no one to answer.
                ask: !state.scripted,
                name: None,
                first: false,
            });
        }
        for card in &cli.import {
            state.imports.push_back(dialogs::Import {
                path: card.clone(),
                place: false,
                ask: true,
                name: None,
                first: false,
            });
        }
        // A scripted run imports at once; else the imports wait for the
        // content, which a job opens.
        while state.scripted && state.modal.is_none() && !state.imports.is_empty() {
            state.next_import();
        }
        if let Some(which) = &cli.map {
            add_map(&mut state, which, cli.map_time.as_deref())?;
        }
        // An import brings the Content panel forward; the start does not.
        state.show_content = false;
        let first = state
            .doc
            .scene()
            .objects()
            .iter()
            .find(|o| o.kind() == kinds::Kind::Character)
            .map(|o| o.id);
        if let Some(bone) = &cli.pose_bone {
            state.pose_bone = Some(bone.clone());
            state.property_tab = 1;
            state.fk_open = true;
            state.show_animator = true;
        }
        state.doc.selected = match cli.select.as_deref() {
            None => first,
            Some(name) if name.eq_ignore_ascii_case("none") => None,
            Some(name) => {
                let named = |o: &&scene::Object| {
                    let card = match &o.content {
                        scene::Content::Character { card, .. } => card.file_stem(),
                        _ => None,
                    };
                    o.name.eq_ignore_ascii_case(name)
                        || card.is_some_and(|stem| stem.to_string_lossy().eq_ignore_ascii_case(name))
                };
                let found = state.doc.scene().objects().iter().find(named);
                Some(
                    found
                        .with_context(|| format!("no object named {name:?} in the scene"))?
                        .id,
                )
            }
        };
        state.sun_preview = cli.sun_preview;
        state.mirror_pending = cli.mirror_pose;
        state.bake_pending = cli.bake_pose;
        if let Some(name) = &cli.pose_ik {
            state.pose_ik = Some(
                rk_chara::pose::IK_TARGET_NAMES
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(name))
                    .with_context(|| format!("no IK target {name}"))?,
            );
        }
        state.ik_wanted = !cli.ik_move.is_empty();
        for spec in &cli.ik_move {
            let (name, by) = spec.split_once(':').context("--ik-move is Target:x,y,z")?;
            let target = rk_chara::pose::IK_TARGET_NAMES
                .iter()
                .position(|n| n.eq_ignore_ascii_case(name))
                .with_context(|| format!("no IK target {name}"))?;
            let p: Vec<f32> = by.split(',').map(str::parse).collect::<Result<_, _>>()?;
            anyhow::ensure!(p.len() == 3, "--ik-move is Target:x,y,z");
            state.ik_pending.push((target, glam::Vec3::new(p[0], p[1], p[2])));
        }
        if let Some(spec) = &cli.hand_patterns {
            let p: Vec<i32> = spec.split(',').map(str::parse).collect::<Result<_, _>>()?;
            anyhow::ensure!(p.len() == 2, "--hand-patterns is LEFT,RIGHT");
            viewport::edit_fk_layer(&mut state, |l| l.hands = [p[0], p[1]]);
        }
        if let Some(spec) = &cli.face {
            let face: rk_chara::face::Face = spec.parse()?;
            viewport::edit_fk_layer(&mut state, |l| l.face = Some(face));
        }
        state.save_pose_pending = cli.save_pose;
        if let Some(tab) = cli.property_tab {
            state.property_tab = tab;
        }
        if let Some(name) = cli.tool.as_deref() {
            state.tool = modes::TOOLS
                .iter()
                .position(|(_, tool, _)| tool.eq_ignore_ascii_case(name))
                .with_context(|| format!("unknown tool {name:?}"))?;
        }
        state.gizmo.space = cli.gizmo_space;
        state.gizmo.pivot = cli.gizmo_pivot;
        // What the command line put in the scene is where the history starts.
        if !cli.import_animation.is_empty() {
            let game = state.games.get(&state.project.mods_dir())?;
            let library = rk_chara::animations::Library::read(game);
            let mut picks = Vec::new();
            for spec in &cli.import_animation {
                let numbers: Vec<i32> = spec.split('/').filter_map(|n| n.parse().ok()).collect();
                let (group, category, number) = match numbers[..] {
                    [group, category] => (group, category, None),
                    [group, category, number] => (group, category, Some(number)),
                    _ => anyhow::bail!("{spec}: an animation is named as GROUP/CATEGORY/NUMBER"),
                };
                let entries = anim_import::listed(&library, None, &|s| s.label().to_owned());
                picks.extend(
                    library
                        .entries
                        .iter()
                        .zip(&entries)
                        .filter(|(e, _)| {
                            (e.group, e.category) == (group, category) && number.is_none_or(|n| n == e.number)
                        })
                        .map(|(_, entry)| animations::Pick {
                            name: entry.name.clone(),
                            group: entry.group.clone(),
                            category: entry.category.clone(),
                            source: entry.source_name.clone(),
                            h: Some(entry.h),
                            from: entry.from.clone(),
                        }),
                );
            }
            let t0 = std::time::Instant::now();
            state.import_animations(&picks, false);
            println!(
                "frame import of {} animations took {:.2} s",
                picks.len(),
                t0.elapsed().as_secs_f32()
            );
            state.show_content = false;
        }
        if let Some(spec) = &cli.animation {
            let numbers: Vec<i32> = spec.split('/').filter_map(|n| n.parse().ok()).collect();
            let [group, category, number] = numbers[..] else {
                anyhow::bail!("{spec}: an animation is named as GROUP/CATEGORY/NUMBER");
            };
            let game = state.games.get(&state.project.mods_dir())?;
            let library = rk_chara::animations::Library::read(game);
            let entry = library
                .find(group, category, number)
                .with_context(|| format!("CharaStudio's list has no animation {spec}"))?;
            let named = scene::Animation::of(entry);
            // The project's own copy when it has one.
            let own = state.animations.of(&named).map(|i| i.reference());
            state.run(Command::Animate(Some(own.unwrap_or(named))));
        }
        if let Some(rel) = &cli.load_pose {
            state.run(Command::LoadPose(rel.clone()));
        }
        if let Some(seconds) = cli.scene_time {
            state.playback = Playback::Paused;
            state.clock = seconds;
        }
        state.animations.refresh(&state.project);
        state.open_folder(project::FOLDERS[cli.folder.unwrap_or(0).min(project::FOLDERS.len() - 1)]);
        if cli.animation_importer {
            state.importer = Some(anim_import::Importer::new(None));
        }
        state.doc.clear_history();
        if (!cli.card.is_empty() || cli.map.is_some()) && state.scripted {
            let live = state
                .live
                .as_mut()
                .context("the window has no wgpu device to draw the viewport with")?;
            // Build the characters now, so that a card that does not build
            // stops the start and the camera flags find a framed view.
            live.follow(
                state.doc.scene(),
                &state.project,
                &mut state.games,
                &mut state.jobs,
                live::Clock {
                    time: state.clock,
                    ..live::Clock::stopped()
                },
                &mut state.log,
            );
            for object in state.doc.scene().objects() {
                if let Some(why) = live.failure(object.id) {
                    anyhow::bail!("{} could not be built: {why}", object.name);
                }
            }
        }
        if let Some(live) = state.live.as_mut() {
            if let Some(drag) = cli.camera_turn {
                live.cam.orbit_drag(drag.to_vec2());
            }
            if let Some((eye, target)) = cli.camera {
                let fov_y_deg = live.cam.orbit.fov_y_deg;
                live.place_camera(&rk_render::Camera {
                    eye,
                    target,
                    fov_y_deg,
                    up: glam::Vec3::Y,
                });
            }
            if cli.free_camera {
                live.cam.release();
            }
            if let Some(speed) = cli.fly_speed {
                live.cam.speed = speed.clamp(camera::SPEED_RANGE.0, camera::SPEED_RANGE.1);
                // Shown for as long as the window lives, for the picture.
                state.speed_shown = Some(f64::INFINITY);
            }
        }
        state.title_bar = cli.title_bar;
        if cli.map_library {
            state.open_map_library();
        }
        state.map_scroll = cli.map_scroll;
        if let Some(path) = &cli.viewport_image {
            let (texture, corner) = load_texture(&cc.egui_ctx, path)?;
            state.viewport_image = Some(texture);
            state.backdrop = corner;
        }
        // A project opens behind its splash, in the selector's window.
        if !selecting {
            state.begin_opening();
        }
        match cli.loading {
            Some(Picture::Opening) => state.opening = Some(state::Opening::sample()),
            Some(Picture::Notices) => {
                use jobs::{Outcome, Show};
                let jobs = &mut state.jobs;
                jobs.pictured("Loaded Honma Yukie", Show::Notice, "", 0, 0, Outcome::Done);
                jobs.pictured("Loading Maria Kujou", Show::Notice, "", 0, 0, Outcome::Running);
                jobs.pictured(
                    "Importing animations",
                    Show::Notice,
                    "Dance 09",
                    9,
                    24,
                    Outcome::Running,
                );
            }
            _ => {}
        }
        let splash = match cli.loading {
            Some(Picture::Startup) => true,
            Some(_) => false,
            // The splash of an opening project is the splash of the start.
            None => !state.scripted && state.opening.is_none(),
        };
        let bottom = match cli.bottom {
            Bottom::Content => Panel::Content,
            Bottom::Console => Panel::Console,
            Bottom::Timeline => Panel::Timeline,
        };
        let state_opening = state.opening.is_some();
        Ok(Studio {
            look,
            state,
            // A picture of the window is always of the default layout.
            dock: match cli.screenshot {
                None => panels::saved_layout().unwrap_or_else(|| panels::default_layout(bottom)),
                Some(_) => panels::default_layout(bottom),
            },
            bottom,
            screenshot: cli.screenshot.clone(),
            clicks: cli.click.clone(),
            type_text: cli.type_text.clone(),
            type_after: cli.type_after,
            hover: cli.hover,
            drag: cli.drag,
            wheel: cli.wheel,
            keys: cli
                .key
                .iter()
                .map(|k| keymap::Chord::parse(k).with_context(|| format!("{k} is not a key")))
                .collect::<Result<_>>()?,
            frames: 0,
            title: String::new(),
            closing: false,
            temporary,
            editor_size: editor_size(cli)?,
            small: (selecting || state_opening) && cli.size.is_none(),
            entering: None,
            splash: splash.then_some(f64::NAN),
            hold_splash: cli.loading == Some(Picture::Startup),
            report: cli.frame_report.then(FrameReport::new),
            quit_after: cli.quit_after,
            bench_add: cli.bench_add.clone(),
        })
    }
}

/// Open Sans for the interface, in two weights, Noto Sans Mono for the
/// console and Phosphor for icons. The interface fonts come from the
/// system for now; bundled ones follow once the choice is made.
fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    let fallback = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut add = |name: &str, bytes: Option<Vec<u8>>, family: egui::FontFamily| {
        let list = fonts.families.entry(family).or_insert_with(|| fallback.clone());
        if let Some(bytes) = bytes {
            fonts
                .font_data
                .insert(name.into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            list.insert(0, name.into());
        }
    };
    // The interface font ships inside the program (Open Sans, under the SIL
    // Open Font License in assets/fonts), so it looks the same everywhere.
    add(
        "interface",
        Some(include_bytes!("../assets/fonts/OpenSans-Regular.ttf").to_vec()),
        egui::FontFamily::Proportional,
    );
    add(
        "interface-semibold",
        Some(include_bytes!("../assets/fonts/OpenSans-SemiBold.ttf").to_vec()),
        egui::FontFamily::Name(theme::SEMIBOLD.into()),
    );
    add(
        "console",
        [
            "/usr/share/fonts/noto/NotoSansMono-Regular.ttf",
            "C:\\Windows\\Fonts\\consola.ttf",
        ]
        .iter()
        .find_map(|path| std::fs::read(path).ok()),
        egui::FontFamily::Monospace,
    );
    // Card, item and mod names are often Japanese, which the interface font
    // does not cover: the first system font found that does comes last in
    // every family, so it only draws what the others lack.
    const JAPANESE: [&str; 6] = [
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "C:\\Windows\\Fonts\\YuGothM.ttc",
        "C:\\Windows\\Fonts\\meiryo.ttc",
        "C:\\Windows\\Fonts\\msgothic.ttc",
    ];
    if let Some(bytes) = JAPANESE.iter().find_map(|path| std::fs::read(path).ok()) {
        fonts.font_data.insert(
            "japanese".into(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        for list in fonts.families.values_mut() {
            list.push("japanese".into());
        }
    }
    fonts
}

impl eframe::App for Studio {
    fn on_exit(&mut self) {
        if self.screenshot.is_none() {
            if let Err(e) = panels::save_layout(&self.dock) {
                eprintln!("The layout could not be kept: {e:#}");
            }
        }
        // Restarting into an update, on the project that was open.
        if let Some(program) = self.state.restart_into.take() {
            let project = (!self.state.project.is_none()).then(|| self.state.project.root.clone());
            if let Err(e) = self_update::start(&program, project.as_deref()) {
                eprintln!("The updated Renkatsu did not start: {e}");
            }
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if self.screenshot.is_some() {
            self.scripted_pointer(raw_input);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        let k = self.look;
        if let Some(report) = &mut self.report {
            report.frame();
            ctx.request_repaint();
        }
        let seconds = ctx.input(|i| i.time) as f32;
        if self.quit_after.is_some_and(|after| seconds > after) {
            self.state.quit = true;
        }
        if seconds > 9.0 && self.state.selector.is_none() {
            if let Some(path) = self.bench_add.take() {
                self.state.imports.push_back(dialogs::Import {
                    path,
                    place: true,
                    ask: false,
                    name: None,
                    first: false,
                });
            }
        }
        // A drag that changes a value is one step of the history; it ends
        // when the pointer is let go.
        if !ctx.input(|i| i.pointer.any_down()) && !self.state.sun_turning {
            self.state.doc.end_gesture();
        }
        self.state.poll_picker();
        self.state.next_import();
        // What the jobs finished is taken in, and what a project that is
        // opening still waits for is moved on.
        self.state.tick(ctx.input(|i| i.time));
        let selecting = self.state.selector.is_some();
        let opening = self.state.opening.is_some();
        // The project selector has a smaller window of its own, which a
        // project that opens keeps for its splash. The desktop resizes it
        // in one step, so the editor's content fades in instead.
        let compact = selecting || opening;
        if compact != self.small {
            self.small = compact;
            // A project opens in a maximized window; scripted pictures keep
            // their asked size. The selector goes back to its own size.
            let maximize = !compact && self.screenshot.is_none();
            if !maximize {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            }
            let size = if compact { selector::WINDOW } else { self.editor_size };
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            if maximize {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
            self.entering = (!compact).then_some(0.0);
        }
        if !compact {
            keymap::handle(ctx, &mut self.state);
        }
        let t = k.t;

        // The title bar is a strip of its own, a shade darker than the menu
        // row under it.
        egui::Panel::top("title")
            .exact_size(self.state.title_bar.height())
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(t.input))
            .show(ui, |ui| chrome::title_bar(ui, &k, &self.state));
        if opening {
            // The splash of the project that opens takes the window.
            chrome::window_edges(ui.ctx(), &k);
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(t.input))
                .show(ui, |ui| opening_page(ui, &k, &self.state));
        } else if selecting {
            // The selector takes the window; the editor is not drawn.
            chrome::window_edges(ui.ctx(), &k);
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(t.panel))
                .show(ui, |ui| selector::page(ui, &k, &mut self.state));
        } else {
            self.editor(ui, &k);
        }

        preferences::show(ctx, &k, &mut self.state);
        shortcuts::show(ctx, &k, &mut self.state);
        about::show(ctx, &k, &mut self.state);
        packages::show(ctx, &k, &mut self.state);
        card_import::show(ctx, &k, &mut self.state);
        map_library::show(ctx, &k, &mut self.state);
        anim_import::show(ctx, &k, &mut self.state);
        self.enter(ctx, &k);
        dialogs::show(ctx, &k, &mut self.state);
        if !compact {
            jobs::notices(ctx, &k, &mut self.state.jobs);
            jobs::wait_window(ctx, &k, &mut self.state.jobs);
        }
        self.startup_splash(ctx, &k);
        if self.state.jobs.busy() || opening {
            // The bars move also where frames come only on input.
            ctx.request_repaint();
        }
        self.window(ctx);

        // Something asked for the cards: bring the Content panel forward.
        if std::mem::take(&mut self.state.show_content) {
            self.state.maximised = false;
            self.bottom = Panel::Content;
            panels::open(&mut self.dock, Panel::Content);
            if let Some(tab) = self.dock.find_tab(&Panel::Content) {
                let _ = self.dock.set_active_tab(tab);
            }
        }

        // A mirror asked for at the start, once the character is built.
        if self.state.mirror_pending {
            let id = self.state.doc.selected;
            // After any IK asked for has reached the live character.
            let ik_ready = self.state.ik_pending.is_empty()
                && (!self.state.ik_wanted
                    || id.is_some_and(|id| self.state.live.as_ref().is_some_and(|l| l.ik_handles(id).is_some())));
            let ready = ik_ready
                && id.is_some_and(|id| {
                    self.state
                        .live
                        .as_ref()
                        .is_some_and(|l| l.mirror_edits(id, None).is_some_and(|m| !m.is_empty()))
                });
            if ready {
                viewport::mirror(&mut self.state, false);
                self.state.mirror_pending = false;
            }
        }

        // An update at work: with no project open it restarts into the new
        // Renkatsu at once, with one it waits to be restarted.
        let no_project = self.state.project.is_none();
        if let Some(updating) = &mut self.state.updating {
            updating.poll(ctx, no_project);
        }

        // A bake asked for at the start, once the animation poses her.
        if self.state.bake_pending {
            let id = self.state.doc.selected;
            let ready = id.is_some_and(|id| {
                self.state
                    .live
                    .as_mut()
                    .and_then(|l| l.baked_edits(id))
                    .is_some_and(|(_, parts)| !parts.is_empty())
            });
            if ready {
                viewport::bake_animation(&mut self.state);
                self.state.bake_pending = false;
            }
        }

        // IK asked for at the start, once the character is built and any
        // bake is done: the targets on her bones, then moved.
        if !self.state.ik_pending.is_empty() && !self.state.bake_pending {
            let on_bones = self
                .state
                .doc
                .selected
                .and_then(|id| self.state.live.as_ref()?.ik_targets_on_bones(id));
            if let (Some(id), Some(mut targets)) = (self.state.doc.selected, on_bones) {
                for (target, by) in std::mem::take(&mut self.state.ik_pending) {
                    targets[target].pos += by;
                }
                if let Some(mut object) = self.state.doc.scene().get(id).cloned() {
                    if let scene::Content::Character { pose_layer, .. } = &mut object.content {
                        let layer = pose_layer.get_or_insert_with(Default::default);
                        layer.ik_on = true;
                        layer.ik = targets;
                    }
                    self.state.doc.edit(object, false);
                }
            }
        }

        // A save asked for at the start, once the character is built.
        if self.state.save_pose_pending && !self.state.bake_pending && !self.state.mirror_pending {
            let built = self
                .state
                .doc
                .selected
                .is_some_and(|id| self.state.live.as_ref().is_some_and(|l| l.joints(id).is_some()));
            if built {
                self.state.run(Command::SavePose);
                self.state.save_pose_pending = false;
            }
        }

        // Posing by hand was switched on: the Animator panel comes forward.
        if std::mem::take(&mut self.state.show_animator) {
            panels::open(&mut self.dock, Panel::Animator);
            if let Some(tab) = self.dock.find_tab(&Panel::Animator) {
                let _ = self.dock.set_active_tab(tab);
            }
        }

        // Frames come as often as the display shows them unless the user
        // chose to save power, when egui draws only on input.
        // Changed settings are written once the pointer lets go, so that a
        // value being dragged is written once.
        if self.state.settings_changed && !ctx.input(|i| i.pointer.any_down()) {
            self.state.settings_changed = false;
            if !self.state.scripted {
                if let Err(e) = self.state.settings.save() {
                    eprintln!("could not save the settings: {e:#}");
                }
            }
        }
        if self.state.settings.frame_rate.continuous() || self.state.live.as_ref().is_some_and(live::Live::settling) {
            ctx.request_repaint();
        } else if let Some(wait) = self.state.live.as_ref().and_then(live::Live::next_blink) {
            // Woken for the next blink.
            ctx.request_repaint_after(std::time::Duration::from_secs_f32(wait.max(0.001)));
        }
        let dt = ctx.input(|i| i.unstable_dt);
        let ft = &mut self.state.frame_time;
        *ft = if *ft == 0.0 { dt } else { *ft + (dt - *ft) * 0.05 };

        self.frames += 1;
        if let Some(path) = self.screenshot.clone() {
            if self.frames == 5 + (self.clicks.len() as u32 + 1) * CLICK_FRAMES + self.keys.len() as u32 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            let shot = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = shot {
                if let Err(e) = save_png(&path, &image) {
                    eprintln!("could not save {}: {e:#}", path.display());
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint();
        }
    }
}

impl Studio {
    /// The editor below the title bar: the menu row, the status bar and
    /// the docked panels.
    fn editor(&mut self, ui: &mut egui::Ui, k: &Look) {
        let k = *k;
        let t = k.t;
        let bar = |id: &'static str, height: f32| {
            let panel = if id == "status" {
                egui::Panel::bottom(id)
            } else {
                egui::Panel::top(id)
            };
            panel
                .exact_size(height)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(t.well))
        };
        let mut reset = false;
        bar("menu", k.d.menu_height).show(ui, |ui| {
            chrome::menu_row(ui, &k, &mut self.state, &mut self.dock, &mut reset)
        });
        if reset {
            self.dock = panels::default_layout(self.bottom);
            self.state.maximised = false;
        }
        bar("status", k.d.status_height).show(ui, |ui| chrome::status_bar(ui, &k, &mut self.state));
        chrome::window_edges(ui.ctx(), &k);
        if k.d.overlays == Overlays::Header && !self.state.overlays_hidden {
            egui::Panel::left("rail")
                .exact_size(chrome::window_rail_width(&k, &self.state))
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(t.well))
                .show(ui, |ui| chrome::window_rail(ui, &k, &mut self.state));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(t.well))
            .show(ui, |ui| {
                if self.state.maximised {
                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                    viewport::show(ui, &k, &mut self.state, None);
                    return;
                }
                let style = panels::dock_style(ui, &k);
                let active = self
                    .dock
                    .iter_leaves()
                    .filter_map(|(_, leaf)| leaf.tabs.get(leaf.active.0).copied())
                    .collect();
                let focused = self.dock.find_active_focused().map_or(Panel::Viewport, |(_, tab)| *tab);
                let mut viewer = panels::Viewer {
                    k: &k,
                    st: &mut self.state,
                    active,
                    focused,
                    tab_rects: Vec::new(),
                };
                DockArea::new(&mut self.dock)
                    .style(style)
                    // Panels close from their tab or their floating window,
                    // and come back from the Window menu.
                    .show_close_buttons(true)
                    .show_leaf_close_all_buttons(false)
                    .show_leaf_collapse_buttons(false)
                    .show_inside(ui, &mut viewer);
            });
    }

    /// The splash of the start: the art and the name over the whole
    /// window for a moment, then it fades. A click sends it away.
    fn startup_splash(&mut self, ctx: &egui::Context, k: &Look) {
        const SHOWN: f64 = 1.1;
        const FADE: f64 = 0.3;
        let Some(since) = self.splash else { return };
        let now = ctx.input(|i| i.time);
        if since.is_nan() {
            self.splash = Some(now);
        }
        let age = if since.is_nan() { 0.0 } else { now - since };
        let opacity = if self.hold_splash {
            1.0
        } else {
            (1.0 - (age - SHOWN) / FADE).clamp(0.0, 1.0) as f32
        };
        if opacity <= 0.0 {
            self.splash = None;
            return;
        }
        let full = ctx.content_rect();
        let sub = format!("Version {}", env!("CARGO_PKG_VERSION"));
        let clicked = egui::Area::new(egui::Id::new("startup splash"))
            .order(egui::Order::Foreground)
            .fixed_pos(full.min)
            .show(ctx, |ui| {
                let resp = ui.allocate_rect(full, egui::Sense::click());
                let what = jobs::Splash {
                    title: "Renkatsu Studio",
                    sub: &sub,
                    step: "",
                    count: "",
                    fraction: None,
                    items: &[],
                    cancel: false,
                    opacity,
                };
                jobs::splash(ui, k, full, &what);
                resp.clicked()
            })
            .inner;
        if clicked && !self.hold_splash {
            self.splash = None;
        }
        ctx.request_repaint();
    }

    /// Fades the editor in after a project opened from the selector: a
    /// veil of the selector's own colour lifts off it.
    fn enter(&mut self, ctx: &egui::Context, k: &Look) {
        let Some(elapsed) = self.entering else { return };
        let time = 0.24 * ctx.global_style().animation_time * 12.0;
        if time <= 0.0 || elapsed >= time {
            self.entering = None;
            return;
        }
        let left = (1.0 - elapsed / time).clamp(0.0, 1.0).powi(3);
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("entering"));
        ctx.layer_painter(layer)
            .rect_filled(ctx.content_rect(), 0.0, k.t.panel.gamma_multiply(left));
        // The frame that builds the scene's characters is a long one; it
        // must not use the fade up.
        self.entering = Some(elapsed + ctx.input(|i| i.stable_dt).min(1.0 / 30.0));
        ctx.request_repaint();
    }

    /// Keeps the desktop's window title in step with the project, and
    /// asks about unsaved changes before the window closes.
    fn window(&mut self, ctx: &egui::Context) {
        let title = chrome::window_title(&self.state);
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        let asked = ctx.input(|i| i.viewport().close_requested());
        if asked && !self.state.quit && self.screenshot.is_none() && self.state.dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.state.ask(Command::Quit, ctx);
        }
        if self.state.quit && !self.closing {
            self.closing = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Feeds the scripted clicks and the resting pointer of `--click` and
    /// `--hover` to egui as if they came from the mouse.
    fn scripted_pointer(&self, input: &mut egui::RawInput) {
        // The real mouse may rest on the window; it must not show in pictures.
        input.events.retain(|e| {
            !matches!(
                e,
                egui::Event::PointerMoved(_)
                    | egui::Event::PointerButton { .. }
                    | egui::Event::PointerGone
                    | egui::Event::MouseMoved(_)
                    | egui::Event::MouseWheel { .. }
            )
        });
        let step = self.frames.saturating_sub(2);
        let (index, phase) = ((step / CLICK_FRAMES) as usize, step % CLICK_FRAMES);
        if self.frames >= 2 {
            let type_at = self.type_after.unwrap_or(self.clicks.len());
            if index == type_at && phase == 0 {
                if let Some(text) = &self.type_text {
                    // Over what the field holds, as typing over a selection.
                    input.events.push(egui::Event::Key {
                        key: egui::Key::A,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::COMMAND,
                    });
                    input.events.push(egui::Event::Text(text.clone()));
                }
            }
            if let Some(click) = self.clicks.get(index).copied() {
                let pos = click.at;
                input.events.push(egui::Event::PointerMoved(pos));
                let button = if click.right {
                    egui::PointerButton::Secondary
                } else {
                    egui::PointerButton::Primary
                };
                let modifiers = if click.ctrl {
                    egui::Modifiers::COMMAND | egui::Modifiers::CTRL
                } else {
                    egui::Modifiers::NONE
                };
                input.events.push(egui::Event::ModifiersChanged(modifiers));
                let mut press = |pressed: bool| {
                    input.events.push(egui::Event::PointerButton {
                        pos,
                        button,
                        pressed,
                        modifiers,
                    });
                };
                if phase == 1 || phase == 2 {
                    press(phase == 1);
                    // The second click of a double click, in the same frames.
                    if click.double && phase == 2 {
                        press(true);
                        press(false);
                    }
                }
                return;
            }
        }
        // The scripted wheel, once the clicks are done: the pointer rests
        // there, and the wheel turns on the first of those frames.
        if let (Some((at, dy)), true) = (self.wheel, index >= self.clicks.len() && self.frames >= 2) {
            input.events.push(egui::Event::PointerMoved(at));
            if step == self.clicks.len() as u32 * CLICK_FRAMES {
                input.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, dy),
                    modifiers: egui::Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                });
            }
        }
        // The scripted keys, once the clicks are done: one a frame.
        if index >= self.clicks.len() && self.frames >= 2 {
            let at = (step - self.clicks.len() as u32 * CLICK_FRAMES) as usize;
            if let Some(chord) = at.checked_sub(1).and_then(|n| self.keys.get(n)) {
                input.events.push(egui::Event::Key {
                    key: chord.key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: chord.modifiers(),
                });
            }
        }
        // The scripted drag, once the clicks are done: a press, a first
        // stretch of the way, then the rest, and the button stays down.
        if let (Some((from, to)), true) = (self.drag, index >= self.clicks.len() && self.frames >= 2) {
            let step = step - self.clicks.len() as u32 * CLICK_FRAMES;
            let pos = match step {
                0 | 1 => from,
                2 => from + (to - from) * 0.3,
                _ => to,
            };
            // A Ctrl click before the drag does not reach into it.
            input.events.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
            input.events.push(egui::Event::PointerMoved(pos));
            if step == 1 {
                input.events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            return;
        }
        if let Some(pos) = self.hover.or(self.clicks.last().map(|c| c.at)) {
            input.events.push(egui::Event::PointerMoved(pos));
        }
    }
}

/// The splash of a project that opens: its name, what loads and how far
/// it is.
fn opening_page(ui: &mut egui::Ui, k: &Look, st: &State) {
    let Some(opening) = &st.opening else { return };
    let progress = &opening.progress;
    let (items, step, count) = (progress.items(), progress.step_text(), progress.count());
    let sub = format!("Renkatsu Studio {}", env!("CARGO_PKG_VERSION"));
    let what = jobs::Splash {
        title: &opening.name,
        sub: &sub,
        step: &step,
        count: &count,
        fraction: Some(progress.fraction()),
        items: &items,
        cancel: true,
        opacity: 1.0,
    };
    let full = ui.max_rect();
    if jobs::splash(ui, k, full, &what) {
        progress.cancel();
    }
}

/// Loads a PNG as a texture and returns it with the colour of its top
/// left corner.
fn load_texture(ctx: &egui::Context, path: &std::path::Path) -> Result<(egui::TextureHandle, egui::Color32)> {
    let decoder = png::Decoder::new(std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()]
            .chunks(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        other => anyhow::bail!("{}: unsupported PNG colour type {other:?}", path.display()),
    };
    let corner = egui::Color32::from_rgb(rgba[0], rgba[1], rgba[2]);
    let image = egui::ColorImage::from_rgba_unmultiplied([info.width as usize, info.height as usize], &rgba);
    Ok((
        ctx.load_texture("viewport", image, egui::TextureOptions::LINEAR),
        corner,
    ))
}

fn save_png(path: &std::path::Path, image: &egui::ColorImage) -> Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, image.size[0] as u32, image.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header()?;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    w.write_image_data(&bytes)?;
    Ok(())
}

fn main() -> Result<()> {
    let result = run();
    // Without a console nobody reads the error, so it is shown in a box.
    if let Err(e) = &result {
        if cfg!(windows) {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("Renkatsu Studio")
                .set_description(format!("{e:#}"))
                .show();
        }
    }
    result
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let settings = settings::Settings::load();
    let mut size = editor_size(&cli)?;
    let selecting = starts_on_selector(&cli, &settings);
    // The selector has a window of its own size, and so has the splash
    // of the project a start goes straight into. The editor's window
    // comes, maximized, when the project is open. A scripted picture
    // starts in the editor's size.
    if cli.size.is_none() && (selecting || cli.screenshot.is_none()) {
        size = selector::WINDOW;
    }
    let maximized = false;
    let mut options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("Renkatsu Studio")
            // Matches renkatsu.desktop, so the desktop shows the launcher's icon.
            .with_app_id("renkatsu")
            .with_inner_size(size)
            .with_maximized(maximized)
            .with_min_inner_size([960.0, 600.0])
            // The window draws its own title bar and border, as Unreal does.
            .with_decorations(false),
        ..Default::default()
    };
    if let [x, y] = cli.at[..] {
        options.viewport = options.viewport.with_position([x, y]).with_active(false);
    }
    options.wgpu_options.surface.present_mode = if settings.frame_rate.vsync() {
        wgpu::PresentMode::AutoVsync
    } else {
        wgpu::PresentMode::AutoNoVsync
    };
    let mut error = None;
    eframe::run_native(
        "Renkatsu Studio",
        options,
        Box::new(|cc| match Studio::new(cc, &cli, settings) {
            Ok(s) => Ok(Box::new(s)),
            Err(e) => {
                error = Some(e);
                Err("startup failed".into())
            }
        }),
    )
    .map_err(|e| match error.take() {
        Some(inner) => inner,
        None => anyhow::anyhow!("{e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_shows_the_selector_unless_told_otherwise() {
        let on = settings::Settings::default();
        let off = settings::Settings {
            show_project_selector: false,
            ..Default::default()
        };
        let cli = |args: &[&str]| Cli::parse_from([&["renkatsu"], args].concat());
        assert!(starts_on_selector(&cli(&[]), &on));
        // With the setting off the last project opens, as before.
        assert!(!starts_on_selector(&cli(&[]), &off));
        // A project or a card named on the command line goes straight in.
        assert!(!starts_on_selector(&cli(&["--project", "/somewhere"]), &on));
        assert!(!starts_on_selector(&cli(&["--card", "a.png"]), &on));
        assert!(!starts_on_selector(
            &cli(&["--project-selector", "--project", "/p"]),
            &on
        ));
        // A scripted picture shows the selector only when it asks for it.
        assert!(!starts_on_selector(&cli(&["--screenshot", "/tmp/a.png"]), &on));
        assert!(starts_on_selector(
            &cli(&["--screenshot", "/tmp/a.png", "--project-selector"]),
            &off
        ));
        assert!(starts_on_selector(&cli(&["--project-selector"]), &off));
    }
}
