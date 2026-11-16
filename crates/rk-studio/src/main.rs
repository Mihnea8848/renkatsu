// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Renkatsu Studio, the editor. This is the shell prototype from
//! docs/editor.md: Unity's layout, the ribbon, the menu bar and docked panels
//! with placeholder content, for judging layout and look.

mod appearance;
mod panels;
mod ribbon;
mod theme;
mod toolbar;

use anyhow::{Context, Result};
use appearance::{Appearance, Tabs, TopBar, TypeIcons};
use clap::Parser;
use egui_dock::{DockArea, DockState};
use panels::Panel;
use std::path::PathBuf;
use theme::{Theme, THEMES};

#[derive(Parser)]
#[command(name = "renkatsu", version, about = "Renkatsu Studio")]
struct Cli {
    /// Theme variant to start with.
    #[arg(long, default_value = "Unreal dark")]
    theme: String,
    /// Picture to show in the viewport until the live viewport exists.
    #[arg(long)]
    viewport_image: Option<PathBuf>,
    /// Save a picture of the window here after a few frames, then quit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Window size, as WIDTHxHEIGHT.
    #[arg(long, default_value = "1600x900")]
    size: String,
    /// The area above the panels.
    #[arg(long, value_enum, default_value = "unreal")]
    top_bar: TopBar,
    /// How docked panel tabs are drawn.
    #[arg(long, value_enum, default_value = "unreal")]
    tabs: Tabs,
    /// How object types show in lists.
    #[arg(long, value_enum, default_value = "glyph")]
    type_icons: TypeIcons,
}

struct Studio {
    theme: Theme,
    dock: DockState<Panel>,
    ribbon: ribbon::Ribbon,
    viewport_image: Option<egui::TextureHandle>,
    selected: usize,
    screenshot: Option<PathBuf>,
    frames: u32,
    look: Appearance,
}

impl Studio {
    fn new(cc: &eframe::CreationContext, cli: &Cli) -> Result<Self> {
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        cc.egui_ctx.set_fonts(fonts);
        let theme = Theme::by_name(&cli.theme).with_context(|| {
            let names: Vec<_> = THEMES.iter().map(|t| t.name).collect();
            format!("unknown theme {:?}; there are {}", cli.theme, names.join(", "))
        })?;
        let viewport_image = match &cli.viewport_image {
            Some(path) => Some(load_texture(&cc.egui_ctx, path)?),
            None => None,
        };
        let studio = Studio {
            theme,
            dock: panels::default_layout(),
            ribbon: ribbon::Ribbon {
                tab: 0,
                collapsed: false,
            },
            viewport_image,
            selected: 1,
            screenshot: cli.screenshot.clone(),
            frames: 0,
            look: Appearance {
                top_bar: cli.top_bar,
                tabs: cli.tabs,
                type_icons: cli.type_icons,
            },
        };
        studio.apply_theme(&cc.egui_ctx);
        Ok(studio)
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        let theme = self.theme;
        ctx.all_styles_mut(|s| theme.apply(s));
    }

    fn next_theme(&mut self, ctx: &egui::Context) {
        let i = THEMES.iter().position(|t| t.name == self.theme.name).unwrap_or(0);
        self.theme = THEMES[(i + 1) % THEMES.len()];
        self.apply_theme(ctx);
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                for item in ["New project", "Open project", "Save", "Save as", "Quit"] {
                    let _ = ui.button(item);
                }
            });
            ui.menu_button("Edit", |ui| {
                for item in ["Undo", "Redo", "Preferences"] {
                    let _ = ui.button(item);
                }
            });
            if self.look.top_bar == TopBar::Unity {
                toolbar::unity_category_menus(ui);
            }
            ui.menu_button("Window", |ui| {
                for panel in Panel::ALL {
                    if ui.button(panel.title()).clicked() {
                        panels::open(&mut self.dock, panel);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Reset layout").clicked() {
                    self.dock = panels::default_layout();
                    ui.close();
                }
            });
            ui.menu_button("Help", |ui| {
                let _ = ui.button("About Renkatsu");
            });
        });
    }
}

impl eframe::App for Studio {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        let t = self.theme;
        egui::Panel::top("menu")
            .frame(
                egui::Frame::new()
                    .fill(t.header)
                    .inner_margin(egui::Margin::symmetric(6, 2)),
            )
            .show(ui, |ui| self.menu_bar(ui));
        let top_margin = match self.look.top_bar {
            TopBar::Unreal | TopBar::Unity => egui::Margin::symmetric(6, 3),
            TopBar::SlimRibbon | TopBar::Ribbon => egui::Margin::ZERO,
        };
        egui::Panel::top("ribbon")
            .frame(egui::Frame::new().fill(t.header).inner_margin(top_margin))
            .show(ui, |ui| match self.look.top_bar {
                TopBar::Unreal => toolbar::unreal(ui, &t),
                TopBar::Unity => toolbar::unity(ui, &t),
                TopBar::SlimRibbon => toolbar::slim_ribbon(ui, &t, &mut self.ribbon),
                TopBar::Ribbon => {
                    if let ribbon::Action::Pressed(label) = self.ribbon.show(ui, &t) {
                        match label {
                            "Next theme" => self.next_theme(ctx),
                            "Reset" if self.ribbon.tab == ribbon::TABS.len() - 1 => {
                                self.dock = panels::default_layout()
                            }
                            _ => {}
                        }
                    }
                }
            });
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(t.header)
                    .inner_margin(egui::Margin::symmetric(8, 3)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Untitled project").color(t.text_dim));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(format!("Theme: {}", t.name)).color(t.text_dim));
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(t.base))
            .show(ui, |ui| {
                let mut style = egui_dock::Style::from_egui(ui.style().as_ref());
                style.tab_bar.bg_fill = t.header;
                style.tab_bar.corner_radius = egui::CornerRadius::ZERO;
                style.tab.tab_body.corner_radius = egui::CornerRadius::ZERO;
                style.tab.tab_body.stroke = egui::Stroke::NONE;
                style.tab.active.corner_radius = egui::CornerRadius::ZERO;
                style.tab.inactive.corner_radius = egui::CornerRadius::ZERO;
                style.tab.focused.corner_radius = egui::CornerRadius::ZERO;
                style.tab.hovered.corner_radius = egui::CornerRadius::ZERO;
                style.tab.active.bg_fill = t.panel;
                style.tab.focused.bg_fill = t.panel;
                style.tab.inactive.bg_fill = t.header;
                style.tab.hovered.bg_fill = t.control;
                match self.look.tabs {
                    Tabs::Unreal => {
                        style.tab.hline_below_active_tab_name = true;
                        style.tab_bar.hline_color = t.select;
                    }
                    Tabs::Unity => {
                        style.tab.hline_below_active_tab_name = false;
                        style.tab.inactive.text_color = t.text_dim;
                    }
                    Tabs::Underline => {
                        style.tab.hline_below_active_tab_name = true;
                        style.tab_bar.hline_color = t.select;
                        style.tab.active.bg_fill = t.header;
                        style.tab.focused.bg_fill = t.header;
                        style.tab.inactive.text_color = t.text_dim;
                    }
                }
                style.separator.width = 2.0;
                style.separator.color_idle = t.divider;
                style.separator.color_hovered = t.select;
                style.separator.color_dragged = t.select;
                style.main_surface_border_stroke = egui::Stroke::NONE;
                style.main_surface_border_rounding = egui::CornerRadius::ZERO;
                let mut viewer = panels::Viewer {
                    theme: &t,
                    viewport_image: self.viewport_image.as_ref(),
                    selected: &mut self.selected,
                    type_icons: self.look.type_icons,
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
            if self.frames == 5 {
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

fn load_texture(ctx: &egui::Context, path: &std::path::Path) -> Result<egui::TextureHandle> {
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
    let image = egui::ColorImage::from_rgba_unmultiplied([info.width as usize, info.height as usize], &rgba);
    Ok(ctx.load_texture("viewport", image, egui::TextureOptions::LINEAR))
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
