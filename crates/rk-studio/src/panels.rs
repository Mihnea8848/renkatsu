// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The docked panels and Unity's default layout. Content is placeholder: the
//! shell is for judging layout and look before the scene model exists.

use crate::theme::Theme;
use egui::{Color32, RichText, TextureHandle, Ui, Vec2};
use egui_dock::{DockState, NodeIndex, SurfaceIndex, TabViewer};
use egui_phosphor::regular as icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Viewport,
    Outliner,
    Properties,
    Content,
    Console,
    Timeline,
}

impl Panel {
    pub const ALL: [Panel; 6] = [
        Panel::Viewport,
        Panel::Outliner,
        Panel::Properties,
        Panel::Content,
        Panel::Console,
        Panel::Timeline,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Panel::Viewport => "Viewport",
            Panel::Outliner => "Outliner",
            Panel::Properties => "Properties",
            Panel::Content => "Content",
            Panel::Console => "Console",
            Panel::Timeline => "Timeline",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Panel::Viewport => icon::CUBE_FOCUS,
            Panel::Outliner => icon::TREE_STRUCTURE,
            Panel::Properties => icon::SLIDERS_HORIZONTAL,
            Panel::Content => icon::FOLDER_SIMPLE,
            Panel::Console => icon::TERMINAL_WINDOW,
            Panel::Timeline => icon::FILM_STRIP,
        }
    }
}

/// Unity's arrangement: the viewport large at the top left, Outliner over
/// Properties on the right, Content, Console and Timeline along the bottom.
pub fn default_layout() -> DockState<Panel> {
    let mut state = DockState::new(vec![Panel::Viewport]);
    let surface = state.main_surface_mut();
    let [left, right] = surface.split_right(NodeIndex::root(), 0.76, vec![Panel::Outliner]);
    surface.split_below(right, 0.42, vec![Panel::Properties]);
    surface.split_below(left, 0.70, vec![Panel::Content, Panel::Console, Panel::Timeline]);
    state
}

/// Opens a panel that is not shown, as Window in the menu bar does.
pub fn open(state: &mut DockState<Panel>, panel: Panel) {
    if state.find_tab(&panel).is_none() {
        state.push_to_focused_leaf(panel);
    }
    let _ = SurfaceIndex::main();
}

pub struct Viewer<'a> {
    pub theme: &'a Theme,
    pub viewport_image: Option<&'a TextureHandle>,
    pub selected: &'a mut usize,
}

impl TabViewer for Viewer<'_> {
    type Tab = Panel;

    fn id(&mut self, tab: &mut Panel) -> egui::Id {
        egui::Id::new(tab.title())
    }

    fn title(&mut self, tab: &mut Panel) -> egui::WidgetText {
        format!("{}  {}", tab.glyph(), tab.title()).into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Panel) {
        match tab {
            Panel::Viewport => self.viewport(ui),
            Panel::Outliner => self.outliner(ui),
            Panel::Properties => self.properties(ui),
            Panel::Content => self.content(ui),
            Panel::Console => self.console(ui),
            Panel::Timeline => {
                ui.label(RichText::new("The timeline arrives with animation.").color(self.theme.text_dim));
            }
        }
    }
}

impl Viewer<'_> {
    fn viewport(&mut self, ui: &mut Ui) {
        let t = self.theme;
        // Toolbar along the top edge, as in Unreal.
        ui.horizontal(|ui| {
            for (g, label) in [
                (icon::CAMERA, "Perspective"),
                (icon::SUN, "Lit"),
                (icon::GLOBE_SIMPLE, "World"),
                (icon::MAGNET, "Snap"),
            ] {
                let _ = ui.button(format!("{g}  {label}"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("Koikatsu look").color(t.text_dim));
            });
        });
        let rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(rect, 0.0, t.field);
        if let Some(tex) = self.viewport_image {
            let size = tex.size_vec2();
            let scale = (rect.width() / size.x).min(rect.height() / size.y);
            let fit = egui::Rect::from_center_size(rect.center(), size * scale);
            ui.painter().image(
                tex.id(),
                fit,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        ui.painter().text(
            rect.left_bottom() + Vec2::new(8.0, -8.0),
            egui::Align2::LEFT_BOTTOM,
            "Still image: the live viewport comes next",
            egui::FontId::proportional(11.0),
            t.text_dim,
        );
        ui.allocate_rect(rect, egui::Sense::click_and_drag());
    }

    fn outliner(&mut self, ui: &mut Ui) {
        let rows: [(usize, &str, &str); 7] = [
            (0, icon::GLOBE_HEMISPHERE_WEST, "Untitled scene"),
            (1, icon::USER, "Simple"),
            (2, icon::T_SHIRT, "Outfit 1"),
            (2, icon::DIAMOND, "Accessories"),
            (1, icon::SUN, "Key light"),
            (1, icon::VIDEO_CAMERA, "Camera 1"),
            (1, icon::MAP_TRIFOLD, "No map"),
        ];
        search(ui, "Search the scene");
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, (depth, g, name)) in rows.iter().enumerate() {
                // Full-width rows, highlighted edge to edge as in Unity and Unreal.
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), self.theme.row), egui::Sense::click());
                let fill = if *self.selected == i {
                    self.theme.select
                } else if resp.hovered() {
                    self.theme.control
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(rect, 0.0, fill);
                let x = rect.left() + 6.0 + *depth as f32 * ui.spacing().indent;
                ui.painter().text(
                    egui::pos2(x, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!("{g}  {name}"),
                    egui::FontId::proportional(13.0),
                    if *self.selected == i {
                        Color32::WHITE
                    } else {
                        self.theme.text
                    },
                );
                if resp.clicked() {
                    *self.selected = i;
                }
            }
        });
    }

    fn properties(&mut self, ui: &mut Ui) {
        let t = self.theme;
        ui.label(
            RichText::new(format!("{}  Simple", icon::USER))
                .size(14.0)
                .color(t.text),
        );
        ui.add_space(4.0);
        egui::CollapsingHeader::new("Transform")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("transform")
                    .num_columns(4)
                    .spacing([4.0, 4.0])
                    .show(ui, |ui| {
                        for name in ["Position", "Rotation", "Scale"] {
                            ui.label(RichText::new(name).color(t.text_dim));
                            for _ in 0..3 {
                                let mut v = if name == "Scale" { 1.0_f32 } else { 0.0 };
                                ui.add_sized(
                                    [58.0, t.row],
                                    egui::DragValue::new(&mut v).speed(0.01).fixed_decimals(2),
                                );
                            }
                            ui.end_row();
                        }
                    });
            });
        egui::CollapsingHeader::new("Character")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("character")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Card").color(t.text_dim));
                        ui.label("Simple.png");
                        ui.end_row();
                        ui.label(RichText::new("Outfit").color(t.text_dim));
                        egui::ComboBox::from_id_salt("outfit")
                            .selected_text("Outfit 1")
                            .show_ui(ui, |ui| {
                                for o in ["Outfit 1", "Outfit 2", "Outfit 3"] {
                                    let _ = ui.selectable_label(o == "Outfit 1", o);
                                }
                            });
                        ui.end_row();
                        ui.label(RichText::new("Shoes").color(t.text_dim));
                        ui.horizontal(|ui| {
                            let _ = ui.radio(true, "Indoor");
                            let _ = ui.radio(false, "Outdoor");
                        });
                        ui.end_row();
                        ui.label(RichText::new("Physics").color(t.text_dim));
                        let mut on = true;
                        ui.checkbox(&mut on, "Simulate");
                        ui.end_row();
                    });
            });
        egui::CollapsingHeader::new("Modules")
            .default_open(false)
            .show(ui, |ui| {
                for m in ["MaterialEditor", "ABMX", "Stiletto", "Clothing Unlocker"] {
                    let mut on = true;
                    ui.checkbox(&mut on, m);
                }
            });
    }

    fn content(&mut self, ui: &mut Ui) {
        let t = self.theme;
        search(ui, "Search the project");
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(150.0);
                for (g, f) in [
                    (icon::FOLDER_SIMPLE, "Characters"),
                    (icon::FOLDER_SIMPLE, "Poses"),
                    (icon::FOLDER_SIMPLE, "Maps"),
                    (icon::FOLDER_SIMPLE, "Items"),
                    (icon::FOLDER_SIMPLE, "Scenes"),
                    (icon::FOLDER_SIMPLE, "Renders"),
                ] {
                    let _ = ui.selectable_label(f == "Characters", format!("{g}  {f}"));
                }
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                for name in ["Simple", "Card 2", "Card 3", "Card 4", "Card 5", "Card 6"] {
                    ui.vertical(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(72.0, 72.0), egui::Sense::click());
                        ui.painter().rect_filled(rect, 0.0, t.field);
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            icon::USER,
                            egui::FontId::proportional(28.0),
                            t.text_dim,
                        );
                        ui.label(RichText::new(name).size(11.5));
                    });
                }
            });
        });
    }

    fn console(&mut self, ui: &mut Ui) {
        let t = self.theme;
        for (c, line) in [
            (t.text_dim, "Indexed 2,214 zipmods and the game folder"),
            (t.text, "Loaded Simple.png in 1.8 s"),
            (t.active, "Pattern texture missing: no item"),
        ] {
            ui.label(RichText::new(line).monospace().color(c));
        }
    }
}

fn search(ui: &mut Ui, hint: &str) {
    let mut text = String::new();
    ui.add(
        egui::TextEdit::singleline(&mut text)
            .hint_text(format!("{}  {hint}", icon::MAGNIFYING_GLASS))
            .desired_width(f32::INFINITY),
    );
}
