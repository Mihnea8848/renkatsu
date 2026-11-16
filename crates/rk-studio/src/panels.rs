// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The docked panels and Unity's default layout. Content is placeholder: the
//! shell is for judging layout and look before the scene model exists.

use crate::appearance::{Kind, TopBar, TypeIcons, TypePalette};
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
    Tools,
}

impl Panel {
    pub const ALL: [Panel; 7] = [
        Panel::Viewport,
        Panel::Outliner,
        Panel::Properties,
        Panel::Content,
        Panel::Console,
        Panel::Timeline,
        Panel::Tools,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Panel::Viewport => "Viewport",
            Panel::Outliner => "Outliner",
            Panel::Properties => "Properties",
            Panel::Content => "Content",
            Panel::Console => "Console",
            Panel::Timeline => "Timeline",
            Panel::Tools => "Tools",
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
            Panel::Tools => icon::TOOLBOX,
        }
    }
}

/// Unity's arrangement: the viewport large at the top left, Outliner over
/// Properties on the right, Content, Console and Timeline along the bottom.
pub fn default_layout(top_bar: TopBar) -> DockState<Panel> {
    let mut state = DockState::new(vec![Panel::Viewport]);
    let surface = state.main_surface_mut();
    let [left, right] = surface.split_right(NodeIndex::root(), 0.8, vec![Panel::Outliner]);
    surface.split_below(right, 0.42, vec![Panel::Properties]);
    let [top, _] = surface.split_below(left, 0.76, vec![Panel::Content, Panel::Console, Panel::Timeline]);
    // Unreal's modes keep the active mode's tools in a panel on the left.
    if top_bar.has_tools_panel() {
        surface.split_left(top, 0.2, vec![Panel::Tools]);
    }
    state
}

/// Opens a panel that is not shown, as Window in the menu bar does.
pub fn open(state: &mut DockState<Panel>, panel: Panel) {
    if state.find_tab(&panel).is_none() {
        state.push_to_focused_leaf(panel);
    }
    let _ = SurfaceIndex::main();
}

/// What the viewport's overlays show, kept between frames.
pub struct Overlays {
    /// Index into `toolbar::MODES`.
    pub mode: usize,
    /// The active mode's tool palette is open.
    pub palette: bool,
    /// The mode rail is shown; folded it is a thin edge.
    pub rail: bool,
    /// Every overlay is hidden.
    pub hidden: bool,
}

pub struct Viewer<'a> {
    pub theme: &'a Theme,
    pub viewport_image: Option<&'a TextureHandle>,
    pub selected: &'a mut usize,
    pub type_icons: TypeIcons,
    pub palette: TypePalette,
    pub top_bar: TopBar,
    pub overlays: &'a mut Overlays,
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
            Panel::Tools => self.tools(ui),
            Panel::Timeline => {
                ui.label(RichText::new("The timeline arrives with animation.").color(self.theme.text_dim));
            }
        }
    }
}

impl Viewer<'_> {
    fn viewport(&mut self, ui: &mut Ui) {
        if self.top_bar.tools_in_viewport() {
            self.studio_viewport(ui);
            return;
        }
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
        if self.top_bar.has_context_bar() {
            self.context_bar(ui);
        }
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
        if self.top_bar == TopBar::UnityOverlay || self.top_bar.has_context_bar() {
            tool_strip(ui, t, rect);
        }
        ui.allocate_rect(rect, egui::Sense::click_and_drag());
    }

    /// The converged design: the picture fills the panel, and everything
    /// else floats over it. A slim row along the top carries the
    /// selection's actions and the view options; down the left side sit
    /// the named mode rail, the active mode's tools and the tool strip.
    fn studio_viewport(&mut self, ui: &mut Ui) {
        let t = self.theme;
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0.0, t.field);
        if let Some(tex) = self.viewport_image {
            let size = tex.size_vec2();
            let scale = (full.width() / size.x).min(full.height() / size.y);
            let fit = egui::Rect::from_center_size(full.center(), size * scale);
            ui.painter().image(
                tex.id(),
                fit,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let overlay = |c: Color32| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 235);
        // The backtick key hides every overlay, for a clean viewport.
        if ui.input(|i| i.key_pressed(egui::Key::Backtick)) {
            self.overlays.hidden = !self.overlays.hidden;
        }
        if self.overlays.hidden {
            ui.painter().text(
                full.right_top() + Vec2::new(-10.0, 10.0),
                egui::Align2::RIGHT_TOP,
                "Press ` to show the tools",
                egui::FontId::proportional(11.0),
                t.text_dim,
            );
            ui.allocate_rect(full, egui::Sense::click_and_drag());
            return;
        }

        // Top row: the selection and what can be done with it, then the view.
        let bar_h = t.row + 8.0;
        let bar = egui::Rect::from_min_size(full.min, Vec2::new(full.width(), bar_h));
        ui.painter().rect_filled(bar, 0.0, overlay(t.header));
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(Vec2::new(8.0, 4.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        row.spacing_mut().item_spacing.x = 2.0;
        row.label(
            RichText::new(Kind::Character.glyph())
                .color(Kind::Character.color(self.palette))
                .size(15.0),
        );
        row.label(RichText::new("Simple").color(Color32::WHITE));
        row.label(RichText::new("Character").color(t.text_dim).size(12.0));
        row.add_space(10.0);
        for (g, label) in [
            (icon::IDENTIFICATION_CARD, "Load card"),
            (icon::T_SHIRT, "Outfit"),
            (icon::SMILEY, "Expression"),
            (icon::PUZZLE_PIECE, "Modules"),
        ] {
            let _ = row.add(egui::Button::new(format!("{g}  {label}  {}", icon::CARET_DOWN)).frame(false));
        }
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (g, label) in [
                (icon::MAGNET, "Snap"),
                (icon::SUN, "Lit"),
                (icon::CAMERA, "Perspective"),
            ] {
                let _ = ui.add(egui::Button::new(format!("{g}  {label}")).frame(false));
            }
        });

        // Left: the named mode rail, which can fold to a thin edge.
        let rail_w = if self.overlays.rail { 58.0 } else { 14.0 };
        let rail = egui::Rect::from_min_max(
            egui::pos2(full.left(), bar.bottom()),
            egui::pos2(full.left() + rail_w, full.bottom()),
        );
        ui.painter().rect_filled(rail, 0.0, overlay(t.header));
        if self.overlays.rail {
            for (i, (g, name)) in crate::toolbar::MODES.iter().enumerate() {
                let cell = egui::Rect::from_min_size(
                    rail.min + Vec2::new(0.0, 4.0 + i as f32 * 54.0),
                    Vec2::new(rail_w, 54.0),
                );
                let resp = ui.interact(cell, ui.id().with(("mode", i)), egui::Sense::click());
                if resp.clicked() {
                    // The active mode again folds its tools away.
                    if self.overlays.mode == i {
                        self.overlays.palette = !self.overlays.palette;
                    } else {
                        self.overlays.mode = i;
                        self.overlays.palette = true;
                    }
                }
                let on = i == self.overlays.mode;
                let p = ui.painter();
                if on {
                    p.rect_filled(cell.shrink2(Vec2::new(6.0, 3.0)), t.control_radius as f32, t.control);
                } else if resp.hovered() {
                    p.rect_filled(cell.shrink2(Vec2::new(6.0, 3.0)), t.control_radius as f32, t.panel);
                }
                let color = if on { Color32::WHITE } else { t.text_dim };
                p.text(
                    cell.center() - Vec2::new(0.0, 8.0),
                    egui::Align2::CENTER_CENTER,
                    *g,
                    egui::FontId::proportional(19.0),
                    color,
                );
                p.text(
                    cell.center() + Vec2::new(0.0, 13.0),
                    egui::Align2::CENTER_CENTER,
                    *name,
                    egui::FontId::proportional(10.5),
                    color,
                );
            }
        }
        let chevron = egui::Rect::from_min_size(egui::pos2(rail.left(), rail.bottom() - 26.0), Vec2::new(rail_w, 26.0));
        let resp = ui
            .interact(chevron, ui.id().with("rail"), egui::Sense::click())
            .on_hover_text(if self.overlays.rail {
                "Fold the modes away"
            } else {
                "Show the modes"
            });
        if resp.clicked() {
            self.overlays.rail = !self.overlays.rail;
        }
        let glyph = if self.overlays.rail {
            icon::CARET_DOUBLE_LEFT
        } else {
            icon::CARET_DOUBLE_RIGHT
        };
        ui.painter().text(
            chevron.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::proportional(13.0),
            if resp.hovered() { Color32::WHITE } else { t.text_dim },
        );

        // The active mode's tools, beside the rail, while they are open.
        let mut strip_left = rail.right();
        let tools_of = mode_tools(self.overlays.mode);
        if self.overlays.rail && self.overlays.palette && !tools_of.1.is_empty() {
            let pal = egui::Rect::from_min_max(
                egui::pos2(rail.right() + 1.0, rail.top()),
                egui::pos2(rail.right() + 1.0 + 210.0, full.bottom()),
            );
            ui.painter().rect_filled(pal, 0.0, overlay(t.panel));
            let mut tools = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(pal.shrink(10.0))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            tools.label(RichText::new(tools_of.0).size(15.0).color(Color32::WHITE));
            tools.add_space(6.0);
            for (section, items) in tools_of.1 {
                tools.label(RichText::new(*section).size(11.5).color(t.text_dim));
                for (g, label) in *items {
                    let _ = tool_row(&mut tools, t, g, label);
                }
                tools.add_space(6.0);
            }
            strip_left = pal.right();
        }

        // The transform tools, floating just right of whatever is open.
        tool_strip(
            ui,
            t,
            egui::Rect::from_min_max(egui::pos2(strip_left, rail.top()), full.max),
        );
        ui.allocate_rect(full, egui::Sense::click_and_drag());
    }

    /// What fits the selection, above the viewport: here a character.
    fn context_bar(&mut self, ui: &mut Ui) {
        let t = self.theme;
        egui::Frame::new()
            .fill(t.header)
            .inner_margin(egui::Margin::symmetric(6, 3))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(Kind::Character.glyph())
                            .color(Kind::Character.color(self.palette))
                            .size(15.0),
                    );
                    ui.label(RichText::new("Simple").color(t.text).strong());
                    ui.label(RichText::new("Character").color(t.text_dim));
                    ui.separator();
                    for (g, label) in [
                        (icon::IDENTIFICATION_CARD, "Load card"),
                        (icon::T_SHIRT, "Outfit"),
                        (icon::PERSON_ARMS_SPREAD, "Pose"),
                        (icon::SMILEY, "Expression"),
                        (icon::PUZZLE_PIECE, "Modules"),
                    ] {
                        let _ = ui.button(format!("{g}  {label}  {}", icon::CARET_DOWN));
                    }
                });
            });
    }

    fn outliner(&mut self, ui: &mut Ui) {
        let rows: [(usize, Kind, &str); 11] = [
            (0, Kind::Scene, "Untitled scene"),
            (1, Kind::Folder, "Cast"),
            (2, Kind::Character, "Simple"),
            (3, Kind::Outfit, "Outfit 1"),
            (3, Kind::Accessory, "Accessories"),
            (2, Kind::Pose, "Standing pose"),
            (1, Kind::Item, "Chair"),
            (1, Kind::Light, "Key light"),
            (1, Kind::Camera, "Camera 1"),
            (1, Kind::Map, "Rooftop"),
            (1, Kind::Script, "Scene script"),
        ];
        search(ui, "Search the scene");
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, (depth, kind, name)) in rows.iter().enumerate() {
                let selected = *self.selected == i;
                if type_row(
                    ui,
                    self.theme,
                    self.type_icons,
                    self.palette,
                    *depth,
                    *kind,
                    name,
                    selected,
                )
                .clicked()
                {
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

    /// Unreal's mode toolkit: the tools of the active mode, here Pose.
    fn tools(&mut self, ui: &mut Ui) {
        if self.top_bar == TopBar::CombinedRail {
            ui.horizontal_top(|ui| {
                mode_rail(ui, self.theme, 2);
                ui.vertical(|ui| self.tool_list(ui));
            });
        } else {
            self.tool_list(ui);
        }
    }

    fn tool_list(&mut self, ui: &mut Ui) {
        let t = self.theme;
        ui.label(
            RichText::new(format!("{}  Pose mode", icon::PERSON_ARMS_SPREAD))
                .size(14.0)
                .color(t.text),
        );
        ui.add_space(4.0);
        for (section, items) in [
            (
                "Edit",
                [
                    (icon::BONE, "Forward kinematics"),
                    (icon::TARGET, "Inverse kinematics"),
                    (icon::FLIP_HORIZONTAL, "Mirror pose"),
                ],
            ),
            (
                "Library",
                [
                    (icon::BOOKMARKS_SIMPLE, "Load pose"),
                    (icon::BOOKMARK_SIMPLE, "Save pose"),
                    (icon::ARROW_U_UP_LEFT, "Reset pose"),
                ],
            ),
            (
                "Body",
                [
                    (icon::HAND, "Hands"),
                    (icon::SMILEY, "Expression"),
                    (icon::EYE, "Look at camera"),
                ],
            ),
        ] {
            egui::CollapsingHeader::new(section).default_open(true).show(ui, |ui| {
                for (g, label) in items {
                    let _ = ui.add_sized(
                        [ui.available_width(), t.row + 4.0],
                        egui::Button::new(format!("{g}  {label}")),
                    );
                }
            });
        }
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

/// One full-width list row with the object's type shown in the chosen
/// style, highlighted edge to edge as in Unity and Unreal.
#[allow(clippy::too_many_arguments)]
pub fn type_row(
    ui: &mut Ui,
    t: &Theme,
    style: TypeIcons,
    palette: TypePalette,
    depth: usize,
    kind: Kind,
    name: &str,
    selected: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), t.row), egui::Sense::click());
    let fill = if selected {
        t.select
    } else if resp.hovered() {
        t.control
    } else {
        Color32::TRANSPARENT
    };
    let p = ui.painter();
    p.rect_filled(rect, 0.0, fill);
    let x = rect.left() + 8.0 + depth as f32 * 14.0;
    let y = rect.center().y;
    let text = if selected { Color32::WHITE } else { t.text };
    let glyph_font = egui::FontId::proportional(14.0);
    match style {
        TypeIcons::Glyph => {
            p.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_CENTER,
                kind.glyph(),
                glyph_font,
                kind.color(palette),
            );
        }
        TypeIcons::Chip => {
            let chip = egui::Rect::from_center_size(egui::pos2(x + 7.0, y), Vec2::splat(t.row - 6.0));
            p.rect_filled(chip, 2.0, kind.color(palette));
            p.text(
                chip.center(),
                egui::Align2::CENTER_CENTER,
                kind.glyph(),
                egui::FontId::proportional(12.0),
                t.field,
            );
        }
        TypeIcons::Strip => {
            let bar = egui::Rect::from_min_size(egui::pos2(x - 5.0, rect.top() + 3.0), Vec2::new(3.0, t.row - 6.0));
            p.rect_filled(bar, 0.0, kind.color(palette));
            p.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_CENTER,
                kind.glyph(),
                glyph_font,
                text,
            );
        }
        TypeIcons::Plain => {
            p.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_CENTER,
                kind.glyph(),
                glyph_font,
                t.text_dim,
            );
        }
    }
    p.text(
        egui::pos2(x + 22.0, y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(13.0),
        text,
    );
    resp
}

/// Unity 6's tool overlay: the transform tools as a vertical strip floating
/// at the viewport's left edge.
fn tool_strip(ui: &mut Ui, t: &Theme, viewport: egui::Rect) {
    let tools = [
        (icon::HAND, "View"),
        (icon::CURSOR, "Select"),
        (icon::ARROWS_OUT_CARDINAL, "Move"),
        (icon::ARROWS_CLOCKWISE, "Rotate"),
        (icon::ARROWS_OUT, "Scale"),
        (icon::BONE, "Pose"),
    ];
    let size = 30.0;
    let strip = egui::Rect::from_min_size(
        viewport.min + Vec2::new(8.0, 8.0),
        Vec2::new(size, size * tools.len() as f32),
    );
    let p = ui.painter();
    p.rect_filled(strip, 0.0, t.header);
    p.rect_stroke(strip, 0.0, egui::Stroke::new(1.0, t.divider), egui::StrokeKind::Outside);
    for (i, (g, _)) in tools.iter().enumerate() {
        let cell = egui::Rect::from_min_size(strip.min + Vec2::new(0.0, i as f32 * size), Vec2::splat(size));
        if i == 2 {
            p.rect_filled(cell, 0.0, t.select);
        }
        p.text(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            *g,
            egui::FontId::proportional(16.0),
            t.text,
        );
    }
}

/// The modes as a named vertical rail, the active one marked.
fn mode_rail(ui: &mut Ui, t: &Theme, active: usize) {
    let w = 64.0;
    let (rail, _) = ui.allocate_exact_size(Vec2::new(w, 6.0 * 50.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rail, 0.0, t.header);
    for (i, (g, name)) in crate::toolbar::MODES.iter().enumerate() {
        let cell = egui::Rect::from_min_size(rail.min + Vec2::new(0.0, i as f32 * 50.0), Vec2::new(w, 50.0));
        if i == active {
            p.rect_filled(cell, 0.0, t.panel);
            p.vline(cell.left() + 1.0, cell.y_range(), egui::Stroke::new(2.0, t.select));
        }
        let color = if i == active { t.text } else { t.text_dim };
        p.text(
            cell.center() - Vec2::new(0.0, 8.0),
            egui::Align2::CENTER_CENTER,
            *g,
            egui::FontId::proportional(18.0),
            color,
        );
        p.text(
            cell.center() + Vec2::new(0.0, 13.0),
            egui::Align2::CENTER_CENTER,
            *name,
            egui::FontId::proportional(11.0),
            color,
        );
    }
}

/// A left-aligned tool row with its icon, highlighted on hover.
fn tool_row(ui: &mut Ui, t: &Theme, glyph: &str, label: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), t.row), egui::Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, t.control_radius as f32, t.control);
    }
    let color = if resp.hovered() { Color32::WHITE } else { t.text };
    p.text(
        rect.left_center() + Vec2::new(10.0, 0.0),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(15.0),
        color,
    );
    p.text(
        rect.left_center() + Vec2::new(24.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        color,
    );
    resp
}

type ToolSection = (&'static str, &'static [(&'static str, &'static str)]);

/// The title and tool sections of a mode; empty for modes without tools.
fn mode_tools(mode: usize) -> (&'static str, &'static [ToolSection]) {
    match mode {
        1 => (
            "Character",
            &[
                (
                    "Card",
                    &[
                        (icon::IDENTIFICATION_CARD, "Load card"),
                        (icon::SWAP, "Replace"),
                        (icon::ARROWS_COUNTER_CLOCKWISE, "Reload"),
                    ],
                ),
                (
                    "Wear",
                    &[
                        (icon::T_SHIRT, "Outfit"),
                        (icon::SNEAKER, "Shoes"),
                        (icon::DIAMOND, "Accessories"),
                    ],
                ),
                ("Face", &[(icon::SMILEY, "Expression"), (icon::EYE, "Gaze")]),
            ],
        ),
        2 => (
            "Pose",
            &[
                (
                    "Edit",
                    &[
                        (icon::BONE, "Forward kinematics"),
                        (icon::TARGET, "Inverse kinematics"),
                        (icon::FLIP_HORIZONTAL, "Mirror pose"),
                    ],
                ),
                (
                    "Library",
                    &[
                        (icon::BOOKMARKS_SIMPLE, "Load pose"),
                        (icon::BOOKMARK_SIMPLE, "Save pose"),
                        (icon::ARROW_U_UP_LEFT, "Reset pose"),
                    ],
                ),
                (
                    "Body",
                    &[
                        (icon::HAND, "Hands"),
                        (icon::SMILEY, "Expression"),
                        (icon::EYE, "Look at camera"),
                    ],
                ),
            ],
        ),
        3 => (
            "Light",
            &[
                ("Lights", &[(icon::LIGHTBULB, "Add light"), (icon::SUN, "Key light")]),
                (
                    "Effects",
                    &[
                        (icon::CIRCLE_HALF, "Ambient occlusion"),
                        (icon::SPARKLE, "Bloom"),
                        (icon::APERTURE, "Depth of field"),
                    ],
                ),
            ],
        ),
        _ => ("", &[]),
    }
}
