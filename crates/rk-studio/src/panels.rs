// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The docked panels and Unity's default layout: the viewport large at the
//! top left, Outliner over Properties on the right, Content, Console and
//! Timeline along the bottom. The Outliner shows the scene model's tree,
//! the Content panel the cards on disk and the Console the editor's log.

use crate::design::{Tabs, Tree};
use crate::kinds::Kind;
use crate::properties;
use crate::scene::{ObjectId, Place};
use crate::state::{Command, Look, State};
use crate::viewport;
use crate::widgets::{self, bar_divider, flat_button, glyph, hair_h, hair_v, icon_button, label};
use egui::{Align, Align2, Color32, Layout, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use egui_dock::{DockState, NodeIndex, TabViewer};
use egui_phosphor::regular as icon;
use std::path::PathBuf;

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

    pub fn glyph(self) -> &'static str {
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

/// Unity's arrangement, with the viewport given as much room as the side
/// and bottom panels can spare.
pub fn default_layout(bottom: Panel) -> DockState<Panel> {
    let mut state = DockState::new(vec![Panel::Viewport]);
    let surface = state.main_surface_mut();
    let [left, right] = surface.split_right(NodeIndex::root(), 0.795, vec![Panel::Outliner]);
    surface.split_below(right, 0.44, vec![Panel::Properties]);
    let tabs = vec![Panel::Content, Panel::Console, Panel::Timeline];
    let active = tabs.iter().position(|p| *p == bottom).unwrap_or(0);
    let [top, below] = surface.split_below(left, 0.75, tabs);
    let path = |node| egui_dock::NodePath {
        surface: egui_dock::SurfaceIndex::main(),
        node,
    };
    if let Ok(leaf) = state.leaf_mut(path(below)) {
        let _ = leaf.set_active_tab(active);
    }
    state.set_focused_node_and_surface(path(top));
    state
}

/// Opens a panel that is not shown, as Window in the menu row does.
pub fn open(state: &mut DockState<Panel>, panel: Panel) {
    if state.find_tab(&panel).is_none() {
        state.push_to_focused_leaf(panel);
    }
}

/// The docking style of a design: square tabs, thin dividers.
pub fn dock_style(ui: &Ui, k: &Look) -> egui_dock::Style {
    let t = &k.t;
    let mut style = egui_dock::Style::from_egui(ui.style().as_ref());
    let underline = k.d.tabs == Tabs::Underline;
    let bar = if underline { t.panel } else { t.well };
    style.tab_bar.bg_fill = bar;
    style.tab_bar.height = k.d.tab_height;
    style.tab_bar.corner_radius = egui::CornerRadius::ZERO;
    style.tab_bar.hline_color = if underline { t.line } else { bar };
    style.tab.hline_below_active_tab_name = underline;
    style.tab.spacing = 0.0;
    style.tab.tab_body.corner_radius = egui::CornerRadius::ZERO;
    style.tab.tab_body.stroke = Stroke::NONE;
    style.tab.tab_body.inner_margin = egui::Margin::ZERO;
    style.tab.tab_body.bg_fill = t.panel;
    let lit = if underline { t.panel } else { t.row_hover };
    for (state, fill, text) in [
        (&mut style.tab.active, t.panel, t.text_bright),
        (&mut style.tab.focused, t.panel, t.text_bright),
        (&mut style.tab.inactive, bar, t.text_dim),
        (&mut style.tab.hovered, lit, t.text),
        (&mut style.tab.active_with_kb_focus, t.panel, t.text_bright),
        (&mut style.tab.focused_with_kb_focus, t.panel, t.text_bright),
        (&mut style.tab.inactive_with_kb_focus, bar, t.text_dim),
    ] {
        state.corner_radius = egui::CornerRadius::ZERO;
        state.bg_fill = fill;
        state.outline_color = Color32::TRANSPARENT;
        state.text_color = text;
    }
    style.separator.width = k.d.separator;
    style.separator.extra_interact_width = 4.0;
    style.separator.color_idle = t.divider;
    style.separator.color_hovered = t.select;
    style.separator.color_dragged = t.select;
    style.main_surface_border_stroke = Stroke::NONE;
    style.main_surface_border_rounding = egui::CornerRadius::ZERO;
    style.overlay.selection_color = crate::theme::Theme::glass(t.select, 90);
    style
}

pub struct Viewer<'a> {
    pub k: &'a Look,
    pub st: &'a mut State,
    /// The tab shown in each group of tabs.
    pub active: Vec<Panel>,
    /// The panel that has the focus.
    pub focused: Panel,
    /// Where each tab's title was drawn this frame.
    pub tab_rects: Vec<(Panel, Rect)>,
}

impl TabViewer for Viewer<'_> {
    type Tab = Panel;

    fn id(&mut self, tab: &mut Panel) -> egui::Id {
        egui::Id::new(tab.title())
    }

    fn title(&mut self, tab: &mut Panel) -> egui::WidgetText {
        let k = self.k;
        let format = |font: egui::FontId| egui::TextFormat {
            font_id: font,
            color: Color32::PLACEHOLDER,
            valign: Align::Center,
            ..Default::default()
        };
        let mut job = egui::text::LayoutJob::default();
        if k.d.tab_icons {
            job.append(tab.glyph(), 2.0, format(k.icons(15.0)));
            job.append(tab.title(), 6.0, format(k.font()));
        } else {
            job.append(tab.title(), 4.0, format(k.font()));
        }
        job.append(" ", 0.0, format(k.font()));
        job.into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Panel) {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let header = self.viewport_header(ui);
        let (k, st) = (self.k, &mut *self.st);
        match tab {
            Panel::Viewport => viewport::show(ui, k, st, header),
            Panel::Outliner => outliner(ui, k, st),
            Panel::Properties => properties::show(ui, k, st),
            Panel::Content => content(ui, k, st),
            Panel::Console => console(ui, k, st),
            Panel::Timeline => timeline(ui, k),
        }
    }

    fn on_tab_button(&mut self, tab: &mut Panel, response: &egui::Response) {
        self.tab_rects.push((*tab, response.rect));
        if !self.active.contains(tab) {
            return;
        }
        let t = &self.k.t;
        let color = if self.focused == *tab { t.select } else { t.hover };
        let r = response.rect;
        let line = match self.k.d.tabs {
            Tabs::Filled => return,
            Tabs::TopLine => Rect::from_min_max(r.min, Pos2::new(r.right(), r.top() + 2.0)),
            Tabs::Underline => Rect::from_min_max(
                Pos2::new(r.left() + 10.0, r.bottom() - 3.0),
                Pos2::new(r.right() - 10.0, r.bottom() - 1.0),
            ),
        };
        response
            .ctx
            .layer_painter(response.layer_id)
            .rect_filled(line, 0.0, color);
    }

    fn scroll_bars(&self, _tab: &Panel) -> [bool; 2] {
        [false, false]
    }
}

impl Viewer<'_> {
    /// The free part of the tab row above the viewport, for the designs
    /// that put the selection bar there.
    fn viewport_header(&self, ui: &Ui) -> Option<Rect> {
        if !self.k.d.overlays.bar_in_header() {
            return None;
        }
        let body = ui.available_rect_before_wrap();
        let row = Rect::from_min_max(
            Pos2::new(body.left(), body.top() - self.k.d.tab_height),
            Pos2::new(body.right(), body.top() - 1.0),
        );
        let tabs_end = self
            .tab_rects
            .iter()
            .filter(|(_, r)| row.contains(r.center()))
            .map(|(_, r)| r.right())
            .fold(row.left(), f32::max);
        let free = Rect::from_min_max(Pos2::new(tabs_end + 4.0, row.top()), row.max);
        (free.width() > 240.0).then_some(free)
    }
}

/// A row of controls across the top of a panel. Returns its rectangle.
fn toolbar(ui: &mut Ui, k: &Look, add: impl FnOnce(&mut Ui)) -> Rect {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row + 10.0), Sense::hover());
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(8.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 4.0;
    add(&mut row);
    rect
}

/// A type colour lightened so it stays readable on the selection colour.
fn on_selection(color: Color32) -> Color32 {
    let mix = |c: u8| (u16::from(c) + (255 - u16::from(c)) * 7 / 10) as u8;
    Color32::from_rgb(mix(color.r()), mix(color.g()), mix(color.b()))
}

/// An object dragged in the Outliner, to move it in the tree.
#[derive(Debug, Clone, Copy)]
struct DragObject(ObjectId);

/// A card dragged out of the Content panel, to add its character.
#[derive(Debug, Clone)]
pub struct DragCard(pub PathBuf);

/// What was dropped on `rect` this frame, when it is a `T`.
pub fn dropped<T: std::any::Any + Send + Sync>(ui: &Ui, rect: Rect) -> Option<std::sync::Arc<T>> {
    if ui.rect_contains_pointer(rect) && ui.input(|i| i.pointer.any_released()) {
        egui::DragAndDrop::take_payload::<T>(ui.ctx())
    } else {
        None
    }
}

/// Whether a `T` is being dragged over `rect`.
pub fn dragged_over<T: std::any::Any + Send + Sync>(ui: &Ui, rect: Rect) -> bool {
    ui.rect_contains_pointer(rect) && egui::DragAndDrop::has_payload_of_type::<T>(ui.ctx())
}

/// One row of the Outliner. The scene itself is the first row and has no
/// object.
struct Row {
    depth: usize,
    id: Option<ObjectId>,
    kind: Kind,
    name: String,
    /// The object's own visibility switch.
    visible: bool,
    /// Whether it shows, which also takes the folders around it.
    shown: bool,
    parent: bool,
    folded: bool,
}

/// What a frame of the Outliner asks for, carried out once it is drawn.
enum Act {
    Select(Option<ObjectId>),
    Fold(Option<ObjectId>),
    Eye(ObjectId),
    Rename(ObjectId),
    Renamed(ObjectId, String),
    Move(ObjectId, Place),
    Run(Command),
}

/// The Outliner's rows: the scene, then its objects in tree order, without
/// what is folded away. A search lists every object that matches instead.
fn rows(st: &State) -> Vec<Row> {
    let scene = st.doc.scene();
    let search = st.scene_search.trim().to_lowercase();
    let mut rows = vec![Row {
        depth: 0,
        id: None,
        kind: Kind::Scene,
        name: scene.name.clone(),
        visible: true,
        shown: true,
        parent: !scene.objects().is_empty(),
        folded: st.scene_folded,
    }];
    let mut hide_below: Option<usize> = st.scene_folded.then_some(0);
    for (depth, id) in scene.rows() {
        let Some(object) = scene.get(id) else { continue };
        let depth = depth + 1;
        if search.is_empty() {
            if hide_below.is_some_and(|d| depth > d) {
                continue;
            }
            hide_below = st.folded.contains(&id).then_some(depth);
        } else if !object.name.to_lowercase().contains(&search) {
            continue;
        }
        rows.push(Row {
            depth,
            id: Some(id),
            kind: object.kind(),
            name: object.name.clone(),
            visible: object.visible,
            shown: scene.shown(id),
            parent: scene.has_children(id),
            folded: st.folded.contains(&id),
        });
    }
    rows
}

fn outliner(ui: &mut Ui, k: &Look, st: &mut State) {
    let t = &k.t;
    toolbar(ui, k, |ui| {
        if k.d.tree == Tree::Plain {
            let resp = flat_button(ui, k, Some(icon::PLUS), "", true, false).on_hover_text("Add to the scene");
            egui::Popup::menu(&resp).show(|ui| add_items(ui, k, st));
        }
        let w = ui.available_width() - k.field() - 4.0;
        widgets::search(ui, k, &mut st.scene_search, "Search the scene", w);
        let resp = icon_button(ui, k, icon::FOLDER_SIMPLE_PLUS, false).on_hover_text("Add a folder");
        if resp.clicked() {
            st.run(Command::Add(Kind::Folder));
        }
    });

    let eye_w = 28.0;
    let type_w = 92.0;
    if k.d.tree == Tree::Columns {
        let (head, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(head, 0.0, t.header);
        glyph(
            p,
            k,
            Pos2::new(head.left() + eye_w / 2.0, head.center().y),
            icon::EYE,
            13.0,
            t.text_dim,
        );
        hair_v(p, head.left() + eye_w, head.top() + 4.0, head.bottom() - 4.0, t.hover);
        let caption = |x: f32, text: &str| {
            p.text(
                Pos2::new(x, head.center().y),
                Align2::LEFT_CENTER,
                text,
                k.small(),
                t.text,
            );
        };
        caption(head.left() + eye_w + 10.0, "Name");
        hair_v(p, head.right() - type_w, head.top() + 4.0, head.bottom() - 4.0, t.hover);
        caption(head.right() - type_w + 10.0, "Type");
    }

    let footer = if k.d.tree == Tree::Columns { 24.0 } else { 0.0 };
    let indent = if k.d.tree == Tree::Guides { 16.0 } else { 14.0 };
    let rows = rows(st);
    let mut acts: Vec<Act> = Vec::new();
    let area = ui.available_rect_before_wrap();
    let area = area.with_max_y(area.bottom() - footer);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .max_height(area.height())
        .show(ui, |ui| {
            for row in &rows {
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row), Sense::click_and_drag());
                if resp.clicked() || resp.drag_started() {
                    acts.push(Act::Select(row.id));
                }
                let selected = row.id.is_some() && st.doc.selected == row.id;
                let hovered = ui.rect_contains_pointer(rect);
                let p = ui.painter().clone();
                if selected {
                    p.rect_filled(rect, 0.0, t.select);
                } else if hovered {
                    p.rect_filled(rect, 0.0, t.row_hover);
                }
                let y = rect.center().y;
                let text = if selected {
                    Color32::WHITE
                } else if !row.shown {
                    t.text_dim
                } else {
                    t.text
                };
                let faint = if selected { on_selection(t.select) } else { t.text_dim };

                if let Some(id) = row.id {
                    resp.dnd_set_drag_payload(DragObject(id));
                    if resp.double_clicked() {
                        acts.push(Act::Rename(id));
                    }
                    resp.context_menu(|ui| {
                        let item = |ui: &mut Ui, text: &str, keys: &str| {
                            let clicked = ui.add(egui::Button::new(text).shortcut_text(keys)).clicked();
                            if clicked {
                                ui.close();
                            }
                            clicked
                        };
                        let mut command = None;
                        if item(ui, "Rename", "F2") {
                            command = Some(Command::Rename);
                        }
                        if item(ui, "Duplicate", "Ctrl+D") {
                            command = Some(Command::Duplicate);
                        }
                        if item(ui, if row.visible { "Hide" } else { "Show" }, "H") {
                            command = Some(Command::ToggleHidden);
                        }
                        ui.separator();
                        if item(ui, "Delete", "Del") {
                            command = Some(Command::Delete);
                        }
                        if let Some(command) = command {
                            acts.push(Act::Select(Some(id)));
                            acts.push(Act::Run(command));
                        }
                    });

                    // Visibility: a column, or a button at the right edge.
                    let eye = match k.d.tree {
                        Tree::Columns => Rect::from_min_size(rect.min, Vec2::new(eye_w, rect.height())),
                        _ => Rect::from_min_max(Pos2::new(rect.right() - 28.0, rect.top()), rect.max),
                    };
                    let eye_resp = ui
                        .interact(eye, ui.id().with(("eye", id)), Sense::click())
                        .on_hover_text(if row.visible { "Hide" } else { "Show" });
                    if eye_resp.clicked() {
                        acts.push(Act::Eye(id));
                    }
                    let show_eye = match k.d.tree {
                        Tree::Plain => hovered || !row.visible,
                        _ => true,
                    };
                    if show_eye {
                        let g = if row.visible { icon::EYE } else { icon::EYE_CLOSED };
                        let color = if eye_resp.hovered() { text } else { faint };
                        glyph(&p, k, eye.center(), g, 13.0, color);
                    }
                }

                let x0 = rect.left() + if k.d.tree == Tree::Columns { eye_w + 4.0 } else { 6.0 };
                if k.d.tree == Tree::Guides && !selected {
                    for level in 1..row.depth {
                        hair_v(
                            &p,
                            x0 + level as f32 * indent - 9.0,
                            rect.top(),
                            rect.bottom(),
                            t.header,
                        );
                    }
                }
                let x = x0 + row.depth as f32 * indent;
                if row.parent {
                    let fold = Rect::from_center_size(Pos2::new(x + 7.0, y), Vec2::new(16.0, rect.height()));
                    let fold_resp = ui.interact(fold, ui.id().with(("fold", row.id)), Sense::click());
                    if fold_resp.clicked() {
                        acts.push(Act::Fold(row.id));
                    }
                    let g = if row.folded {
                        icon::CARET_RIGHT
                    } else {
                        icon::CARET_DOWN
                    };
                    let color = if fold_resp.hovered() { text } else { faint };
                    glyph(&p, k, Pos2::new(x + 7.0, y), g, 10.0, color);
                }
                let tint = if selected {
                    on_selection(row.kind.color())
                } else {
                    row.kind.color()
                };
                glyph(&p, k, Pos2::new(x + 24.0, y), row.kind.glyph(), 15.0, tint);
                let name_end = match k.d.tree {
                    Tree::Columns => rect.right() - type_w,
                    _ => rect.right() - 28.0,
                };
                match &mut st.renaming {
                    Some((id, name)) if row.id == Some(*id) => {
                        let field = Rect::from_min_max(
                            Pos2::new(x + 32.0, rect.top() + 1.0),
                            Pos2::new(name_end - 2.0, rect.bottom() - 1.0),
                        );
                        p.rect_filled(field, k.radius(), t.input);
                        p.rect_stroke(field, k.radius(), Stroke::new(1.0, t.select), StrokeKind::Inside);
                        let edit = ui.put(
                            field.shrink2(Vec2::new(4.0, 0.0)),
                            egui::TextEdit::singleline(name)
                                .frame(egui::Frame::NONE)
                                .margin(egui::Margin::ZERO)
                                .font(k.font())
                                .text_color(t.text_bright),
                        );
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            acts.push(Act::Renamed(*id, String::new()));
                        } else if edit.lost_focus() {
                            acts.push(Act::Renamed(*id, name.clone()));
                        } else if !edit.has_focus() {
                            edit.request_focus();
                        }
                    }
                    _ => {
                        let clip = Rect::from_min_max(rect.min, Pos2::new(name_end - 4.0, rect.bottom()));
                        label(&p.with_clip_rect(clip), k, Pos2::new(x + 36.0, y), &row.name, text);
                    }
                }
                if k.d.tree == Tree::Columns {
                    p.text(
                        Pos2::new(rect.right() - type_w + 10.0, y),
                        Align2::LEFT_CENTER,
                        row.kind.label(),
                        k.font(),
                        faint,
                    );
                }

                // An object dragged over this row: where letting go puts it.
                if let Some(dragged) = resp.dnd_hover_payload::<DragObject>() {
                    let at = ui.input(|i| i.pointer.interact_pos()).map_or(y, |pos| pos.y);
                    let part = (at - rect.top()) / rect.height();
                    let place = match (row.id, row.kind) {
                        (None, _) => Place::Into(None),
                        (Some(id), Kind::Folder) if (0.25..=0.75).contains(&part) => Place::Into(Some(id)),
                        (Some(id), _) if part < 0.5 => Place::Before(id),
                        (Some(id), _) => Place::After(id),
                    };
                    if st.doc.scene().can_place(dragged.0, place) {
                        let top = ui
                            .painter()
                            .clone()
                            .with_layer_id(egui::LayerId::new(egui::Order::Foreground, ui.id().with("drop")));
                        match place {
                            Place::Into(_) => {
                                top.rect_stroke(rect, 0.0, Stroke::new(1.0, t.select_bright), StrokeKind::Inside);
                            }
                            Place::Before(_) => hair_h(&top, x + 16.0, rect.right(), rect.top(), t.select_bright),
                            Place::After(_) => {
                                hair_h(&top, x + 16.0, rect.right(), rect.bottom() - 1.0, t.select_bright)
                            }
                        }
                        if ui.input(|i| i.pointer.any_released()) {
                            acts.push(Act::Move(dragged.0, place));
                        }
                    }
                }
            }
        });
    if dragged_over::<DragCard>(ui, area) {
        ui.painter()
            .rect_stroke(area, 0.0, Stroke::new(1.0, t.select_bright), StrokeKind::Inside);
    }
    if let Some(card) = dropped::<DragCard>(ui, area) {
        acts.push(Act::Run(Command::AddCard(card.0.clone())));
    }

    for act in acts {
        match act {
            Act::Select(id) => st.doc.selected = id,
            Act::Fold(None) => st.scene_folded = !st.scene_folded,
            Act::Fold(Some(id)) => {
                if !st.folded.remove(&id) {
                    st.folded.insert(id);
                }
            }
            Act::Eye(id) => {
                let visible = st.doc.scene().get(id).is_some_and(|o| o.visible);
                st.doc.set_visible(id, !visible);
            }
            Act::Rename(id) => {
                st.doc.selected = Some(id);
                st.run(Command::Rename);
            }
            Act::Renamed(id, name) => {
                st.doc.rename(id, &name);
                st.renaming = None;
            }
            Act::Move(id, place) => {
                if st.doc.place(id, place) {
                    if let Place::Into(Some(folder)) = place {
                        st.folded.remove(&folder);
                    }
                }
            }
            Act::Run(command) => st.run(command),
        }
    }

    if footer > 0.0 {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), footer), Sense::hover());
        let p = ui.painter();
        hair_h(p, rect.left(), rect.right(), rect.top(), t.line);
        let chosen = if st.doc.selected.is_some() { ", 1 selected" } else { "" };
        p.text(
            Pos2::new(rect.left() + 10.0, rect.center().y),
            Align2::LEFT_CENTER,
            format!("{} objects{chosen}", st.doc.scene().objects().len()),
            k.small(),
            t.text_dim,
        );
    }
}

/// The kinds the Add menu offers, as menu items with the kind's icon.
pub fn add_items(ui: &mut Ui, k: &Look, st: &mut State) {
    for kind in [
        Kind::Character,
        Kind::Item,
        Kind::Light,
        Kind::Camera,
        Kind::Map,
        Kind::Folder,
    ] {
        let mut job = egui::text::LayoutJob::default();
        let format = |font, color| egui::TextFormat {
            font_id: font,
            color,
            valign: Align::Center,
            ..Default::default()
        };
        job.append(kind.glyph(), 0.0, format(k.icons(15.0), kind.color()));
        let text = if kind == Kind::Character {
            "Character from a card"
        } else {
            kind.label()
        };
        job.append(text, 8.0, format(k.font(), k.t.text));
        if ui.button(job).clicked() {
            st.run(Command::Add(kind));
            ui.close();
        }
    }
}

/// The project's folders besides Characters, as they are on disk: each
/// with its kind, what its import button says (none when the folder fills
/// another way) and what the folder shows until its feature arrives.
const LATER: [(&str, Kind, Option<&str>, &str); 6] = [
    ("Poses", Kind::Pose, Some("Import pose"), "Poses arrive with posing"),
    ("Maps", Kind::Map, Some("Import map"), "Maps arrive with their feature"),
    (
        "Items",
        Kind::Item,
        Some("Import item"),
        "Items arrive with their feature",
    ),
    (
        "Scenes",
        Kind::Scene,
        Some("Import scene"),
        "A project holds one scene for now",
    ),
    ("Renders", Kind::Camera, None, "Renders arrive with baking"),
    (
        "Mods",
        Kind::Folder,
        None,
        "Imported cards copy the mods they need here",
    ),
];

fn content(ui: &mut Ui, k: &Look, st: &mut State) {
    let t = &k.t;
    st.library.begin_frame();
    st.folder = st.folder.min(LATER.len());
    // The characters come first, then the folders kept for later.
    // The folder this frame draws; a click in the tree below changes
    // `st.folder` for the next frame.
    let folder = st.folder;
    let characters = folder == 0;
    let trail = [
        st.project.file.name.clone(),
        if characters {
            "Characters".to_owned()
        } else {
            LATER[folder - 1].0.to_owned()
        },
    ];

    let mut add: Option<PathBuf> = None;
    let bar = toolbar(ui, k, |ui| {
        // Each folder imports its own kind of file.
        if characters {
            if flat_button(ui, k, Some(icon::DOWNLOAD_SIMPLE), "Import card", false, false)
                .on_hover_text("Pick a card and copy it into the project, with the mods it needs")
                .clicked()
            {
                st.ask(Command::ImportCard, ui.ctx());
            }
            let resp = flat_button(ui, k, Some(icon::PLUS), "Add to scene", false, false).on_hover_text(
                if st.asset.is_some() {
                    "Add the selected character to the scene"
                } else {
                    "Select a character first, or double click one"
                },
            );
            if resp.clicked() {
                add = st.asset.clone();
            }
        } else if let (Some(import), why) = (LATER[folder - 1].2, LATER[folder - 1].3) {
            ui.add_enabled_ui(false, |ui| {
                flat_button(ui, k, Some(icon::DOWNLOAD_SIMPLE), import, false, false).on_disabled_hover_text(why);
            });
        }
        if icon_button(ui, k, icon::ARROWS_CLOCKWISE, false)
            .on_hover_text("Read the project's folders again")
            .clicked()
        {
            st.library.refresh(&st.project);
        }
        bar_divider(ui, k);
        ui.add_space(2.0);
        for (i, part) in trail.iter().enumerate() {
            let last = i + 1 == trail.len();
            let w = widgets::text_width(ui, part, k.font()).ceil();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(w, k.field()), Sense::hover());
            let color = if last { t.text_bright } else { t.text_dim };
            label(ui.painter(), k, rect.left_center(), part, color);
            if !last {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(14.0, k.field()), Sense::hover());
                glyph(ui.painter(), k, rect.center(), icon::CARET_RIGHT, 10.0, t.text_dim);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            widgets::search(ui, k, &mut st.content_search, "Search this folder", 240.0);
        });
    });
    hair_h(ui.painter(), bar.left(), bar.right(), bar.bottom() - 1.0, t.line);

    let body = ui.available_rect_before_wrap();
    let tree_w = 188.0;
    let tree = Rect::from_min_max(body.min, Pos2::new(body.left() + tree_w, body.bottom()));
    hair_v(ui.painter(), tree.right(), tree.top(), tree.bottom(), t.line);
    let mut side = ui.new_child(UiBuilder::new().max_rect(tree));
    side.set_clip_rect(tree.intersect(ui.clip_rect()));
    egui::ScrollArea::vertical()
        .id_salt("folders")
        .auto_shrink(false)
        .show(&mut side, |ui| {
            ui.add_space(4.0);
            // The project folder holds the others, as on disk.
            let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row), Sense::hover());
            let p = ui.painter();
            let x = row.left() + 10.0;
            glyph(p, k, Pos2::new(x, row.center().y), icon::CARET_DOWN, 10.0, t.text_dim);
            glyph(
                p,
                k,
                Pos2::new(x + 14.0, row.center().y),
                icon::FOLDER_OPEN,
                15.0,
                t.text_dim,
            );
            let clip = p.with_clip_rect(row.with_max_x(row.right() - 4.0));
            label(
                &clip,
                k,
                Pos2::new(x + 28.0, row.center().y),
                &st.project.file.name,
                t.text_bright,
            );
            let folders = std::iter::once(("Characters", Kind::Character))
                .chain(LATER.iter().map(|(name, kind, _, _)| (*name, *kind)));
            let mut chosen = None;
            for (i, (name, kind)) in folders.enumerate() {
                let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row), Sense::click());
                if resp.clicked() {
                    chosen = Some(i);
                }
                let p = ui.painter();
                let on = st.folder == i;
                if on {
                    p.rect_filled(row, 0.0, t.control);
                } else if resp.hovered() {
                    p.rect_filled(row, 0.0, t.row_hover);
                }
                let g = if on { icon::FOLDER_OPEN } else { icon::FOLDER_SIMPLE };
                let x = row.left() + 34.0;
                glyph(p, k, Pos2::new(x, row.center().y), g, 15.0, kind.color());
                let color = if on { t.text_bright } else { t.text };
                let clip = p.with_clip_rect(row.with_max_x(row.right() - 4.0));
                label(&clip, k, Pos2::new(x + 14.0, row.center().y), name, color);
            }
            if let Some(i) = chosen {
                st.folder = i;
                st.asset = None;
            }
        });

    let grid = Rect::from_min_max(Pos2::new(tree.right() + 1.0, body.top()), body.max);
    let mut area = ui.new_child(UiBuilder::new().max_rect(grid));
    area.set_clip_rect(grid.intersect(ui.clip_rect()));
    let note = |area: &Ui, text: &str| {
        area.painter().text(
            grid.min + Vec2::new(14.0, 20.0),
            Align2::LEFT_CENTER,
            text,
            k.font(),
            t.text_dim,
        );
    };
    let search = st.content_search.trim().to_lowercase();
    let cards: Vec<crate::library::Entry> = if characters {
        st.library
            .characters
            .iter()
            .filter(|c| {
                search.is_empty()
                    || c.name.to_lowercase().contains(&search)
                    || c.file.to_string_lossy().to_lowercase().contains(&search)
            })
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    if !characters {
        let (name, _, _, why) = LATER[folder - 1];
        if name == "Mods" {
            let count = std::fs::read_dir(st.project.folder(name))
                .map(|d| {
                    d.flatten()
                        .filter(|e| e.path().extension().is_some_and(|x| x == "zipmod"))
                        .count()
                })
                .unwrap_or(0);
            note(&area, &format!("{count} mods. {why}"));
        } else {
            note(&area, why);
        }
    } else if cards.is_empty() && !search.is_empty() {
        note(&area, "No character matches the search");
    } else if cards.is_empty() {
        note(&area, "The project has no characters yet. Import a card to add one.");
    }

    // Cards are portrait pictures, 252 by 352.
    let thumb = Vec2::new((k.d.row * 2.5).round(), (k.d.row * 2.5 * 352.0 / 252.0).round());
    let caption = k.d.tree != Tree::Plain;
    let tile = Vec2::new(thumb.x + 12.0, thumb.y + if caption { 40.0 } else { 26.0 });
    let kind = Kind::Character;
    egui::ScrollArea::vertical()
        .id_salt("cards")
        .auto_shrink(false)
        .show(&mut area, |ui| {
            let columns = ((ui.available_width() - 12.0) / tile.x).floor().max(1.0) as usize;
            let rows = cards.len().div_ceil(columns);
            let (space, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), rows as f32 * tile.y + 12.0),
                Sense::hover(),
            );
            for (i, card) in cards.iter().enumerate() {
                let cell = Rect::from_min_size(
                    space.min
                        + Vec2::new(
                            12.0 + (i % columns) as f32 * tile.x,
                            10.0 + (i / columns) as f32 * tile.y,
                        ),
                    tile - Vec2::new(12.0, 0.0),
                );
                if !ui.is_rect_visible(cell) {
                    continue;
                }
                let name = card.name.as_str();
                let resp = ui
                    .interact(cell, ui.id().with(("card", i)), Sense::click_and_drag())
                    .on_hover_text(format!(
                        "{name}\n{}\nDouble click or drag into the scene to add",
                        card.file.display()
                    ));
                resp.dnd_set_drag_payload(DragCard(card.file.clone()));
                if resp.clicked() || resp.drag_started() {
                    st.asset = Some(card.file.clone());
                }
                if resp.double_clicked() {
                    add = Some(card.file.clone());
                }
                let on = st.asset.as_ref() == Some(&card.file);
                let picture = Rect::from_min_size(cell.min, thumb);
                let texture = st.library.thumb(ui.ctx(), &card.path).map(|tex| tex.id());
                let p = ui.painter();
                p.rect_filled(picture, 0.0, t.input);
                match texture {
                    Some(id) => {
                        p.image(
                            id,
                            picture,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    None => glyph(p, k, picture.center(), kind.glyph(), thumb.x * 0.42, t.hover),
                }
                // The type colour along the picture's foot, as Unreal's tiles.
                p.rect_filled(
                    Rect::from_min_max(Pos2::new(picture.left(), picture.bottom() - 2.0), picture.max),
                    0.0,
                    kind.color(),
                );
                if on {
                    p.rect_stroke(picture, 0.0, Stroke::new(1.0, t.select), StrokeKind::Outside);
                    p.rect_stroke(picture, 0.0, Stroke::new(1.0, t.select), StrokeKind::Inside);
                } else if resp.hovered() {
                    p.rect_stroke(picture, 0.0, Stroke::new(1.0, t.hover), StrokeKind::Inside);
                }
                let clip = p.with_clip_rect(Rect::from_min_max(
                    Pos2::new(cell.left(), picture.bottom()),
                    Pos2::new(cell.right(), cell.bottom()),
                ));
                let color = if on { t.text_bright } else { t.text };
                label(
                    &clip,
                    k,
                    Pos2::new(cell.left() + 1.0, picture.bottom() + 12.0),
                    name,
                    color,
                );
                if caption {
                    clip.text(
                        Pos2::new(cell.left() + 1.0, picture.bottom() + 27.0),
                        Align2::LEFT_CENTER,
                        kind.label(),
                        k.small(),
                        t.text_dim,
                    );
                }
            }
        });
    ui.allocate_rect(body, Sense::hover());
    if let Some(card) = add {
        st.run(Command::AddCard(card));
    }
}

fn console(ui: &mut Ui, k: &Look, st: &mut State) {
    let t = &k.t;
    let levels = [
        (icon::INFO, "Information", t.text_dim),
        (icon::WARNING, "Warnings", t.accent),
        (icon::X_CIRCLE, "Errors", t.stop),
    ];
    toolbar(ui, k, |ui| {
        if flat_button(ui, k, Some(icon::BROOM), "Clear", false, false).clicked() {
            st.log.lines.clear();
        }
        bar_divider(ui, k);
        for (i, (g, name, _)) in levels.iter().enumerate() {
            let count = st.log.lines.iter().filter(|l| l.0 == i).count();
            if flat_button(ui, k, Some(g), &format!("{name}  {count}"), false, st.levels[i]).clicked() {
                st.levels[i] = !st.levels[i];
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            widgets::search(ui, k, &mut st.console_search, "Search the log", 240.0);
        });
    });
    let body = ui.available_rect_before_wrap();
    ui.painter().rect_filled(body, 0.0, t.input);
    let mut area = ui.new_child(UiBuilder::new().max_rect(body));
    area.set_clip_rect(body.intersect(ui.clip_rect()));
    let search = st.console_search.trim().to_lowercase();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show(&mut area, |ui| {
            ui.add_space(4.0);
            for (level, time, text) in &st.log.lines {
                if !st.levels[*level] || !(search.is_empty() || text.to_lowercase().contains(&search)) {
                    continue;
                }
                let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row - 2.0), Sense::hover());
                let p = ui.painter();
                if ui.rect_contains_pointer(rect) {
                    p.rect_filled(rect, 0.0, t.well);
                }
                let y = rect.center().y;
                glyph(
                    p,
                    k,
                    Pos2::new(rect.left() + 18.0, y),
                    levels[*level].0,
                    14.0,
                    levels[*level].2,
                );
                let x = p
                    .text(
                        Pos2::new(rect.left() + 32.0, y),
                        Align2::LEFT_CENTER,
                        time,
                        k.mono(),
                        t.text_dim,
                    )
                    .right();
                let color = if *level == 0 { t.text } else { levels[*level].2 };
                p.text(Pos2::new(x + 12.0, y), Align2::LEFT_CENTER, text, k.mono(), color);
            }
        });
    ui.allocate_rect(body, Sense::hover());
}

/// A sketch of the timeline: tracks, a ruler and a playhead. Animation is
/// a later phase, so nothing here is live.
fn timeline(ui: &mut Ui, k: &Look) {
    let t = &k.t;
    let bar = toolbar(ui, k, |ui| {
        for g in [icon::SKIP_BACK, icon::PLAY, icon::SKIP_FORWARD] {
            let _ = icon_button(ui, k, g, false);
        }
        bar_divider(ui, k);
        ui.add_space(2.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(120.0, k.field()), Sense::hover());
        let x = label(ui.painter(), k, rect.left_center(), "Frame 24", t.text_bright);
        label(
            ui.painter(),
            k,
            Pos2::new(x + 6.0, rect.center().y),
            "of 120",
            t.text_dim,
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let text = "The timeline arrives with animation";
            let w = widgets::text_width(ui, text, k.font()).ceil();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(w, k.field()), Sense::hover());
            label(ui.painter(), k, rect.left_center(), text, t.text_dim);
        });
    });
    let body = ui.available_rect_before_wrap();
    let p = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
    hair_h(&p, bar.left(), bar.right(), body.top(), t.line);
    let names_w = 168.0;
    let ruler = Rect::from_min_max(
        Pos2::new(body.left() + names_w, body.top() + 1.0),
        Pos2::new(body.right(), body.top() + 23.0),
    );
    p.rect_filled(ruler, 0.0, t.header);
    hair_v(&p, ruler.left() - 1.0, body.top(), body.bottom(), t.line);
    let per_frame = (ruler.width() - 24.0) / 120.0;
    let frame_x = |f: f32| (ruler.left() + 12.0 + f * per_frame).round();
    for f in (0..=120).step_by(5) {
        let x = frame_x(f as f32);
        let major = f % 10 == 0;
        hair_v(
            &p,
            x,
            ruler.bottom() - if major { 8.0 } else { 4.0 },
            ruler.bottom(),
            t.hover,
        );
        if major {
            p.text(
                Pos2::new(x + 3.0, ruler.top() + 8.0),
                Align2::LEFT_CENTER,
                f.to_string(),
                k.small(),
                t.text_dim,
            );
        }
    }
    let tracks: [(usize, Kind, &str, &[f32]); 4] = [
        (0, Kind::Character, "Simple", &[]),
        (1, Kind::Pose, "Pose", &[0.0, 24.0, 60.0]),
        (1, Kind::Character, "Expression", &[12.0, 48.0]),
        (0, Kind::Camera, "Camera 1", &[0.0, 90.0]),
    ];
    for (i, (depth, kind, name, keys)) in tracks.iter().enumerate() {
        let top = ruler.bottom() + i as f32 * k.d.row;
        let y = top + k.d.row / 2.0;
        let x = body.left() + 12.0 + *depth as f32 * 14.0;
        glyph(&p, k, Pos2::new(x + 8.0, y), kind.glyph(), 15.0, kind.color());
        label(&p, k, Pos2::new(x + 22.0, y), name, t.text);
        hair_h(&p, body.left(), body.right(), top + k.d.row - 1.0, t.line);
        for key in *keys {
            let c = Pos2::new(frame_x(*key) + 0.5, y);
            let d = 4.5;
            p.add(egui::Shape::convex_polygon(
                vec![
                    c + Vec2::new(0.0, -d),
                    c + Vec2::new(d, 0.0),
                    c + Vec2::new(0.0, d),
                    c + Vec2::new(-d, 0.0),
                ],
                t.text,
                Stroke::NONE,
            ));
        }
    }
    let x = frame_x(24.0);
    p.rect_filled(
        Rect::from_min_max(Pos2::new(x - 4.0, ruler.top()), Pos2::new(x + 5.0, ruler.top() + 9.0)),
        1.0,
        t.select,
    );
    hair_v(&p, x, ruler.top(), body.bottom(), t.select);
    ui.allocate_rect(body, Sense::hover());
}
