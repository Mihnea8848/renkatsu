// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Renkatsu Studio, the editor, after the editor design: Unity's layout,
//! Unreal's modes drawn over the viewport, the menu row and docked panels
//! in the designs of `design.rs`, on the scene model of `scene.rs`, which
//! the viewport draws live.

// A window program on Windows: no console opens beside it.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod camera;
mod chrome;
mod design;
mod dialogs;
mod kinds;
mod library;
mod live;
mod modes;
mod mods;
mod panels;
mod preferences;
mod project;
mod properties;
mod scene;
mod selector;
mod settings;
mod state;
mod theme;
mod viewport;
mod widgets;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use design::{Design, Overlays, TitleBar};
use eframe::egui_wgpu::wgpu;
use egui_dock::{DockArea, DockState};
use panels::Panel;
use project::Project;
use state::{Command, Look, Physics, Rail, State};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Bottom {
    Content,
    Console,
    Timeline,
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
    /// Start on the project selector even where the settings or a
    /// screenshot would not, to picture it.
    #[arg(long, hide = true)]
    project_selector: bool,
    /// The mode to start in, by name.
    #[arg(long, default_value = "select")]
    mode: String,
    /// The object selected at the start, by its name or its card's file
    /// name, or "none". Without it the first `--card` character is.
    #[arg(long)]
    select: Option<String>,
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
    /// May be given several times; the clicks happen in order.
    #[arg(long, value_parser = parse_point)]
    click: Vec<egui::Pos2>,
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
    /// Start with the camera focused on nothing.
    #[arg(long)]
    free_camera: bool,
    /// Start with this fly speed and its meter showing.
    #[arg(long)]
    fly_speed: Option<f32>,
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
    clicks: Vec<egui::Pos2>,
    hover: Option<egui::Pos2>,
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
}

impl Drop for Studio {
    fn drop(&mut self) {
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
        state.scripted = cli.screenshot.is_some();
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
        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            let mut live = live::Live::new(render_state)?;
            if state.scripted {
                // A picture must not catch the camera on its way.
                live.cam.glide_time = 0.0;
            }
            state.live = Some(live);
        }
        if fresh {
            state.start_fresh(cli.card.is_empty());
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
            });
        }
        for card in &cli.import {
            state.imports.push_back(dialogs::Import {
                path: card.clone(),
                place: false,
                ask: true,
                name: None,
            });
        }
        while state.modal.is_none() && !state.imports.is_empty() {
            state.next_import();
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
        // What the command line put in the scene is where the history starts.
        state.doc.clear_history();
        if !cli.card.is_empty() {
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
                Physics::Stopped,
                Physics::Stopped,
                0.0,
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
        if let Some(path) = &cli.viewport_image {
            let (texture, corner) = load_texture(&cc.egui_ctx, path)?;
            state.viewport_image = Some(texture);
            state.backdrop = corner;
        }
        let bottom = match cli.bottom {
            Bottom::Content => Panel::Content,
            Bottom::Console => Panel::Console,
            Bottom::Timeline => Panel::Timeline,
        };
        Ok(Studio {
            look,
            state,
            dock: panels::default_layout(bottom),
            bottom,
            screenshot: cli.screenshot.clone(),
            clicks: cli.click.clone(),
            hover: cli.hover,
            frames: 0,
            title: String::new(),
            closing: false,
            temporary,
            editor_size: editor_size(cli)?,
            small: selecting && cli.size.is_none(),
            entering: None,
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
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if self.screenshot.is_some() {
            self.scripted_pointer(raw_input);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        let k = self.look;
        // A drag that changes a value is one step of the history; it ends
        // when the pointer is let go.
        if !ctx.input(|i| i.pointer.any_down()) {
            self.state.doc.end_gesture();
        }
        self.state.poll_picker();
        self.state.next_import();
        let selecting = self.state.selector.is_some();
        // The project selector has a smaller window of its own. The desktop
        // resizes it in one step, so the editor's content fades in instead.
        if selecting != self.small {
            self.small = selecting;
            let size = if selecting { selector::WINDOW } else { self.editor_size };
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            self.entering = (!selecting).then_some(0.0);
        }
        if !selecting {
            self.file_keys(ctx);
            self.scene_keys(ctx);
        }
        let t = k.t;

        // The title bar is a strip of its own, a shade darker than the menu
        // row under it.
        egui::Panel::top("title")
            .exact_size(self.state.title_bar.height())
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(t.input))
            .show(ui, |ui| chrome::title_bar(ui, &k, &self.state));
        if selecting {
            // The selector takes the window; the editor is not drawn.
            chrome::window_edges(ui.ctx(), &k);
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(t.panel))
                .show(ui, |ui| selector::page(ui, &k, &mut self.state));
        } else {
            self.editor(ui, &k);
        }

        preferences::show(ctx, &k, &mut self.state);
        self.enter(ctx, &k);
        dialogs::show(ctx, &k, &mut self.state);
        self.window(ctx);

        // Something asked for the cards: bring the Content panel forward.
        if std::mem::take(&mut self.state.show_content) {
            self.state.maximised = false;
            self.state.folder = 0;
            self.bottom = Panel::Content;
            panels::open(&mut self.dock, Panel::Content);
            if let Some(tab) = self.dock.find_tab(&Panel::Content) {
                let _ = self.dock.set_active_tab(tab);
            }
        }

        // Frames come as often as the display shows them unless the user
        // chose to save power, when egui draws only on input.
        if self.state.settings.frame_rate.continuous() {
            ctx.request_repaint();
        }
        let dt = ctx.input(|i| i.unstable_dt);
        let ft = &mut self.state.frame_time;
        *ft = if *ft == 0.0 { dt } else { *ft + (dt - *ft) * 0.05 };

        self.frames += 1;
        if let Some(path) = self.screenshot.clone() {
            if self.frames == 5 + (self.clicks.len() as u32 + 1) * CLICK_FRAMES {
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
        let ctx = &ui.ctx().clone();
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
        bar("status", k.d.status_height).show(ui, |ui| chrome::status_bar(ui, &k, &self.state));
        chrome::window_edges(ui.ctx(), &k);
        if k.d.overlays == Overlays::Header && !self.state.overlays_hidden {
            egui::Panel::left("rail")
                .exact_size(chrome::window_rail_width(&k, &self.state))
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(t.well))
                .show(ui, |ui| chrome::window_rail(ui, &k, &mut self.state));
        }

        // Shift with Space gives the viewport the whole window, as in Unity.
        if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::Space)) {
            self.state.maximised = !self.state.maximised;
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
                    .show_close_buttons(false)
                    .show_leaf_close_all_buttons(false)
                    .show_leaf_collapse_buttons(false)
                    .show_inside(ui, &mut viewer);
            });
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

    /// The keys of the File menu. They work while a text field has the
    /// keyboard too, but not while a dialog is open.
    fn file_keys(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        if self.state.modal.is_some() {
            return;
        }
        let keys = [
            (Modifiers::COMMAND | Modifiers::SHIFT, Key::S, Command::SaveAs),
            (Modifiers::COMMAND, Key::S, Command::Save),
            (Modifiers::COMMAND, Key::N, Command::NewProject),
            (Modifiers::COMMAND, Key::O, Command::OpenProject(None)),
            (Modifiers::COMMAND, Key::I, Command::ImportCard),
            (Modifiers::COMMAND, Key::Q, Command::Quit),
        ];
        for (modifiers, key, command) in keys {
            if ctx.input_mut(|i| i.consume_key(modifiers, key)) {
                self.state.ask(command, ctx);
            }
        }
    }

    /// The keys that act on the scene: Unity's, where it has them.
    fn scene_keys(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let redo = Modifiers::COMMAND | Modifiers::SHIFT;
        let keys = [
            (redo, Key::Z, Command::Redo),
            (Modifiers::COMMAND, Key::Y, Command::Redo),
            (Modifiers::COMMAND, Key::Z, Command::Undo),
            (Modifiers::COMMAND, Key::D, Command::Duplicate),
            (Modifiers::NONE, Key::Delete, Command::Delete),
            (Modifiers::NONE, Key::H, Command::ToggleHidden),
            (Modifiers::NONE, Key::F2, Command::Rename),
        ];
        for (modifiers, key, command) in keys {
            // Plain letters must not fire while Ctrl or Alt is held.
            let plain = modifiers != Modifiers::NONE || ctx.input(|i| i.modifiers.is_none());
            if plain && ctx.input_mut(|i| i.consume_key(modifiers, key)) {
                self.state.run(command);
            }
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
            if let Some(pos) = self.clicks.get(index).copied() {
                input.events.push(egui::Event::PointerMoved(pos));
                if phase == 1 || phase == 2 {
                    input.events.push(egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: phase == 1,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                return;
            }
        }
        if let Some(pos) = self.hover.or(self.clicks.last().copied()) {
            input.events.push(egui::Event::PointerMoved(pos));
        }
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
    if starts_on_selector(&cli, &settings) && cli.size.is_none() {
        size = selector::WINDOW;
    }
    let mut options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("Renkatsu Studio")
            // Matches renkatsu.desktop, so the desktop shows the launcher's icon.
            .with_app_id("renkatsu")
            .with_inner_size(size)
            .with_min_inner_size([960.0, 600.0])
            // The window draws its own title bar and border, as Unreal does.
            .with_decorations(false),
        ..Default::default()
    };
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
