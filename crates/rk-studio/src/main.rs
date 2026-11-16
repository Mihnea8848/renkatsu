// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Renkatsu Studio, the editor. This is the shell prototype from
//! docs/editor.md: Unity's layout, Unreal's modes drawn over the viewport,
//! the menu row and docked panels with placeholder content, in the designs
//! of `design.rs`, for judging layout and look on screenshots.

mod chrome;
mod design;
mod kinds;
mod modes;
mod panels;
mod properties;
mod state;
mod theme;
mod viewport;
mod widgets;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use design::{Design, Overlays};
use egui_dock::{DockArea, DockState};
use panels::Panel;
use state::{Look, Rail, State, SCENE};
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
    /// Picture to show in the viewport until the live viewport exists.
    #[arg(long)]
    viewport_image: Option<PathBuf>,
    /// Save a picture of the window here after a few frames, then quit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Window size, as WIDTHxHEIGHT.
    #[arg(long, default_value = "1600x900")]
    size: String,
    /// The mode to start in, by name.
    #[arg(long, default_value = "pose")]
    mode: String,
    /// The object selected at the start, by name, or "none".
    #[arg(long, default_value = "Simple")]
    select: String,
    /// Start with the mode's tool palette folded away.
    #[arg(long)]
    palette_closed: bool,
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
}

impl Studio {
    fn new(cc: &eframe::CreationContext, cli: &Cli) -> Result<Self> {
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

        let mut state = State::new();
        state.mode = modes::MODES
            .iter()
            .position(|m| m.name.eq_ignore_ascii_case(&cli.mode))
            .with_context(|| format!("unknown mode {:?}", cli.mode))?;
        state.selected = if cli.select.eq_ignore_ascii_case("none") {
            None
        } else {
            let row = SCENE
                .iter()
                .position(|(_, _, name)| name.eq_ignore_ascii_case(&cli.select))
                .with_context(|| format!("no object named {:?} in the scene", cli.select))?;
            Some(row)
        };
        state.palette_open = !cli.palette_closed;
        state.overlays_hidden = cli.overlays_hidden;
        state.maximised = cli.maximised;
        if cli.rail_icons {
            state.rail = Rail::Icons;
        }
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
    let mut add = |name: &str, path: &str, family: egui::FontFamily| {
        let list = fonts.families.entry(family).or_insert_with(|| fallback.clone());
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert(name.into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            list.insert(0, name.into());
        }
    };
    add(
        "interface",
        "/usr/share/fonts/TTF/OpenSans-Regular.ttf",
        egui::FontFamily::Proportional,
    );
    add(
        "interface-semibold",
        "/usr/share/fonts/TTF/OpenSans-SemiBold.ttf",
        egui::FontFamily::Name(theme::SEMIBOLD.into()),
    );
    add(
        "console",
        "/usr/share/fonts/noto/NotoSansMono-Regular.ttf",
        egui::FontFamily::Monospace,
    );
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
    let cli = Cli::parse();
    let (w, h) = cli
        .size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?)))
        .context("--size must look like 1600x900")?;
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("Renkatsu Studio")
            .with_inner_size([w, h]),
        ..Default::default()
    };
    let mut error = None;
    eframe::run_native(
        "Renkatsu Studio",
        options,
        Box::new(|cc| match Studio::new(cc, &cli) {
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
