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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// Where the panels' layout is kept between runs: beside the settings.
fn layout_path() -> Option<PathBuf> {
    Some(crate::settings::Settings::path()?.with_file_name("layout.toml"))
}

/// The layout the panels were left in, when it was kept and still shows
/// the viewport.
pub fn saved_layout() -> Option<DockState<Panel>> {
    let text = std::fs::read_to_string(layout_path()?).ok()?;
    let dock: DockState<Panel> = toml::from_str(&text).ok()?;
    dock.find_tab(&Panel::Viewport).is_some().then_some(dock)
}

/// Keeps the panels' layout for the next run.
pub fn save_layout(dock: &DockState<Panel>) -> anyhow::Result<()> {
    let path = layout_path().ok_or_else(|| anyhow::anyhow!("no configuration folder"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // TOML, as it keeps the sizes not set yet (NaN), which JSON cannot.
    std::fs::write(&path, toml::to_string(dock)?)?;
    Ok(())
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
        // The panel last clicked in is the one keys such as F2 act in.
        if ui.rect_contains_pointer(ui.max_rect()) && ui.input(|i| i.pointer.any_pressed()) {
            self.st.focus = *tab;
        }
        let header = self.viewport_header(ui);
        let (k, st) = (self.k, &mut *self.st);
        match tab {
            Panel::Viewport => viewport::show(ui, k, st, header),
            Panel::Outliner => outliner(ui, k, st),
            Panel::Properties => properties::show(ui, k, st),
            Panel::Content => crate::content_panel::show(ui, k, st),
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
pub fn toolbar(ui: &mut Ui, k: &Look, add: impl FnOnce(&mut Ui)) -> Rect {
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
    /// The icon and the type: the kind's, or the light's or effect's.
    glyph: &'static str,
    label: &'static str,
    /// The icon's colour: the kind's, blue for a man.
    color: Color32,
    name: String,
    /// The object's own visibility switch.
    visible: bool,
    /// Whether it shows, which also takes the folders around it.
    shown: bool,
    parent: bool,
    folded: bool,
    /// The bone of its parent the object is attached to, as it is shown.
    bone: Option<String>,
    /// How many map pieces a folder of them holds.
    count: Option<usize>,
}

/// What a frame of the Outliner asks for, carried out once it is drawn.
enum Act {
    Select(Option<ObjectId>),
    /// Add an object to the selection, or take it out.
    Toggle(ObjectId),
    /// Select an object unless it is one of several selected, before a
    /// command that acts on the selection.
    Keep(ObjectId),
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
        glyph: Kind::Scene.glyph(),
        label: Kind::Scene.label(),
        color: Kind::Scene.color(),
        name: scene.name.clone(),
        visible: true,
        shown: true,
        parent: !scene.objects().is_empty(),
        folded: st.scene_folded,
        bone: None,
        count: None,
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
            glyph: object.content.glyph(),
            label: object.content.label(),
            color: st.library.color_of(&object.content),
            name: object.name.clone(),
            visible: object.visible,
            shown: scene.shown(id),
            parent: scene.has_children(id),
            folded: st.folded.contains(&id),
            bone: object.bone.as_deref().map(|bone| crate::bones::label(bone).to_owned()),
            count: pieces_in(scene, id),
        });
    }
    rows
}

/// How many map pieces a folder holds, when it holds map pieces only.
fn pieces_in(scene: &crate::scene::Scene, id: crate::scene::ObjectId) -> Option<usize> {
    let object = scene.get(id)?;
    if object.kind() != Kind::Folder {
        return None;
    }
    let children = scene.children(Some(id));
    let all = !children.is_empty()
        && children.iter().all(|c| {
            matches!(
                scene.get(*c).map(|o| &o.content),
                Some(crate::scene::Content::MapPiece { .. } | crate::scene::Content::MapDoor { .. })
            )
        });
    all.then_some(children.len())
}

fn outliner(ui: &mut Ui, k: &Look, st: &mut State) {
    let t = &k.t;
    toolbar(ui, k, |ui| {
        if k.d.tree == Tree::Plain {
            let resp = flat_button(ui, k, Some(icon::PLUS), "", true, false).on_hover_text("Add to the scene");
            egui::Popup::menu(&resp).show(|ui| add_items(ui, k, st));
        }
        let w = ui.available_width() - k.field() - 4.0;
        widgets::search(ui, k, &mut st.scene_search, "Search", w);
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
                let adding = ui.input(|i| i.modifiers.command);
                match row.id {
                    Some(id) if resp.clicked() && adding => acts.push(Act::Toggle(id)),
                    _ if resp.clicked() || resp.drag_started() => acts.push(Act::Select(row.id)),
                    _ => {}
                }
                let selected = row.id.is_some_and(|id| st.doc.is_selected(id));
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

                if row.id.is_none() {
                    resp.context_menu(|ui| {
                        if let Some(command) = scene_menu(ui, k, st) {
                            acts.push(Act::Select(None));
                            acts.push(Act::Run(command));
                        }
                    });
                }
                if let Some(id) = row.id {
                    resp.dnd_set_drag_payload(DragObject(id));
                    if resp.double_clicked() {
                        acts.push(Act::Rename(id));
                    }
                    resp.context_menu(|ui| {
                        if let Some(command) = object_menu(ui, k, st, row) {
                            acts.push(Act::Keep(id));
                            acts.push(Act::Run(command));
                        }
                    });
                    // An animation of the project dropped on a character.
                    if row.kind == Kind::Character {
                        if let Some(file) = resp.dnd_release_payload::<crate::content_panel::DragFile>() {
                            if let Some(item) = st.animations.find(&file.0) {
                                acts.push(Act::Select(Some(id)));
                                acts.push(Act::Run(Command::Animate(Some(item.reference()))));
                            }
                        }
                    }

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
                if k.d.tree == Tree::Guides {
                    // A line down from each object this one lies in.
                    let line = if selected {
                        Color32::from_white_alpha(50)
                    } else {
                        t.border
                    };
                    for level in 1..=row.depth {
                        hair_v(
                            &p,
                            (x0 + level as f32 * indent - 9.0).floor(),
                            rect.top(),
                            rect.bottom(),
                            line,
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
                let tint = if selected { on_selection(row.color) } else { row.color };
                glyph(&p, k, Pos2::new(x + 24.0, y), row.glyph, 15.0, tint);
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
                        let p = p.with_clip_rect(clip);
                        let end = label(&p, k, Pos2::new(x + 36.0, y), &row.name, text);
                        if let Some(bone) = &row.bone {
                            p.text(Pos2::new(end + 8.0, y), Align2::LEFT_CENTER, bone, k.small(), faint);
                        } else if let Some(n) = row.count {
                            p.text(
                                Pos2::new(end + 8.0, y),
                                Align2::LEFT_CENTER,
                                n.to_string(),
                                k.small(),
                                faint,
                            );
                        }
                    }
                }
                if k.d.tree == Tree::Columns {
                    p.text(
                        Pos2::new(rect.right() - type_w + 10.0, y),
                        Align2::LEFT_CENTER,
                        row.label,
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
                        // On a folder it goes into it, on anything else it
                        // is attached to it.
                        (Some(id), _) if (0.25..=0.75).contains(&part) => Place::Into(Some(id)),
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
            Act::Toggle(id) => st.doc.toggle_selected(id),
            Act::Keep(id) => {
                // The object that was clicked leads what is selected.
                let mut all = vec![id];
                if st.doc.is_selected(id) {
                    all.extend(st.doc.selected_all().into_iter().filter(|other| *other != id));
                }
                st.doc.select_many(&all);
            }
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
                // What is dragged takes the rest of the selection along.
                let all = if st.doc.is_selected(id) {
                    st.doc.selected_all()
                } else {
                    vec![id]
                };
                if st.doc.place_many(&all, place) > 0 {
                    if let Place::Into(Some(parent)) = place {
                        st.folded.remove(&parent);
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
        let chosen = match st.doc.selected_all().len() {
            0 => String::new(),
            n => format!(", {n} selected"),
        };
        p.text(
            Pos2::new(rect.left() + 10.0, rect.center().y),
            Align2::LEFT_CENTER,
            format!("{} objects{chosen}", st.doc.scene().objects().len()),
            k.small(),
            t.text_dim,
        );
    }
}

/// The context menu of an object in the Outliner. Returns what was chosen.
fn object_menu(ui: &mut Ui, k: &Look, st: &State, row: &Row) -> Option<Command> {
    use crate::keymap::Action;
    use widgets::menu_item;
    let key = |action: Action| st.keys.text(action);
    let mut command = None;
    let scene = st.doc.scene();
    let folder = row.kind == Kind::Folder;
    let character = row.kind == Kind::Character;
    let inside = row.id.and_then(|id| scene.get(id)).is_some_and(|o| o.parent.is_some());
    if menu_item(ui, k, icon::CROSSHAIR, "Focus", &key(Action::Focus), !folder) {
        command = Some(Command::Focus);
    }
    if character {
        widgets::menu_sub(ui, k, Kind::Animation.glyph(), "Apply animation", |ui| {
            if let Some(chosen) = animation_items(ui, k, st, row.id) {
                command = Some(Command::Animate(chosen));
            }
        });
    }
    ui.separator();
    if menu_item(ui, k, icon::PENCIL_SIMPLE, "Rename", &key(Action::Rename), true) {
        command = Some(Command::Rename);
    }
    if menu_item(ui, k, icon::COPY, "Duplicate", &key(Action::Duplicate), true) {
        command = Some(Command::Duplicate);
    }
    if menu_item(ui, k, icon::COPY, "Copy", &key(Action::Copy), true) {
        command = Some(Command::Copy);
    }
    if menu_item(ui, k, icon::SCISSORS, "Cut", &key(Action::Cut), true) {
        command = Some(Command::Cut);
    }
    if menu_item(
        ui,
        k,
        icon::CLIPBOARD,
        "Paste",
        &key(Action::Paste),
        !st.scene_clipboard.is_empty(),
    ) {
        command = Some(Command::Paste);
    }
    ui.separator();
    let eye = if row.visible { icon::EYE_SLASH } else { icon::EYE };
    if menu_item(
        ui,
        k,
        eye,
        if row.visible { "Hide" } else { "Show" },
        &key(Action::ToggleHidden),
        true,
    ) {
        command = Some(Command::ToggleHidden);
    }
    if menu_item(
        ui,
        k,
        icon::SELECTION_ALL,
        "Select children",
        &key(Action::SelectChildren),
        row.parent,
    ) {
        command = Some(Command::SelectChildren);
    }
    if menu_item(
        ui,
        k,
        icon::FOLDER_SIMPLE_PLUS,
        "Add child folder",
        &key(Action::NewFolder),
        folder,
    ) {
        command = Some(Command::AddChildFolder);
    }
    widgets::menu_sub(ui, k, icon::LINK, "Attach to", |ui| {
        if let Some(id) = row.id {
            if let Some((to, bone)) = attach_items(ui, k, st, id) {
                command = Some(Command::Attach(to, bone));
            }
        }
    });
    if menu_item(
        ui,
        k,
        icon::ARROW_BEND_LEFT_UP,
        "Detach",
        &key(Action::Unparent),
        inside,
    ) {
        command = Some(Command::Unparent);
    }
    ui.separator();
    if menu_item(ui, k, icon::TRASH, "Delete", &key(Action::Delete), true) {
        command = Some(Command::Delete);
    }
    command
}

/// What an object can be attached to, as menu items: the other objects
/// of the scene, a character opening its main bones. The object takes
/// the rest of the selection along when it is part of it. Returns what
/// was chosen, with the bone when it was one.
pub fn attach_items(ui: &mut Ui, k: &Look, st: &State, id: ObjectId) -> Option<(ObjectId, Option<String>)> {
    let scene = st.doc.scene();
    let moving = if st.doc.is_selected(id) {
        st.doc.selected_all()
    } else {
        vec![id]
    };
    let fits = |to: ObjectId| {
        let place = Place::Into(Some(to));
        !moving.contains(&to) && moving.iter().all(|m| scene.can_place(*m, place))
    };
    // What the object is attached to now carries a mark.
    let now = scene.get(id).map(|o| (o.parent, o.bone.as_deref()));
    let mut chosen = None;
    let mut any = false;
    for (_, to) in scene.rows() {
        let Some(target) = scene.get(to).filter(|o| o.kind() != Kind::Folder && fits(to)) else {
            continue;
        };
        any = true;
        let on = now.is_some_and(|(parent, _)| parent == Some(to));
        if target.kind() == Kind::Character {
            widgets::menu_sub(ui, k, target.kind().glyph(), &target.name, |ui| {
                let bone = now.filter(|_| on).and_then(|(_, bone)| bone);
                if widgets::check_item(ui, k, on && bone.is_none(), "Origin", "").clicked() {
                    chosen = Some((to, None));
                }
                ui.separator();
                if let Some(name) = crate::bones::main_items(ui, k, bone) {
                    chosen = Some((to, Some(name.to_owned())));
                }
                if chosen.is_some() {
                    ui.close();
                }
            });
        } else if widgets::menu_item(ui, k, target.kind().glyph(), &target.name, "", true) {
            chosen = Some((to, None));
        }
    }
    if !any {
        widgets::menu_item(ui, k, "", "Nothing to attach to", "", false);
    }
    chosen
}

/// The context menu of the scene's own row.
fn scene_menu(ui: &mut Ui, k: &Look, st: &State) -> Option<Command> {
    use crate::keymap::Action;
    use widgets::menu_item;
    let key = |action: Action| st.keys.text(action);
    let mut command = None;
    if menu_item(ui, k, icon::FOLDER_SIMPLE_PLUS, "Add folder", "", true) {
        command = Some(Command::AddChildFolder);
    }
    for (kind, text) in [(Kind::Light, "Add light"), (Kind::Camera, "Add camera")] {
        if menu_item(ui, k, kind.glyph(), text, "", true) {
            command = Some(Command::Add(kind));
        }
    }
    if menu_item(ui, k, Kind::Character.glyph(), "Add character from a card", "", true) {
        command = Some(Command::Add(Kind::Character));
    }
    ui.separator();
    if menu_item(
        ui,
        k,
        icon::CLIPBOARD,
        "Paste",
        &key(Action::Paste),
        !st.scene_clipboard.is_empty(),
    ) {
        command = Some(Command::Paste);
    }
    command
}

/// The project's animations as menu items, by the folders the user keeps
/// them in, with none first. Returns what was chosen: an animation, or
/// `Some(None)` for none. `of` is the character whose animation is marked.
pub fn animation_items(
    ui: &mut Ui,
    k: &Look,
    st: &State,
    of: Option<ObjectId>,
) -> Option<Option<crate::scene::Animation>> {
    let worn = of.and_then(|id| st.doc.scene().get(id)).and_then(|o| match &o.content {
        crate::scene::Content::Character { animation, .. } => animation.as_ref(),
        _ => None,
    });
    let worn_rel = worn.and_then(|a| st.animations.of(a)).map(|i| i.rel.as_str());
    let mut chosen = None;
    // A character is built in the T pose; the maker's pose is always on
    // offer too, without importing it.
    if widgets::check_item(ui, k, worn.is_none(), "T-pose", "").clicked() {
        chosen = Some(None);
        ui.close();
    }
    let maker = worn.is_some_and(|a| a.file.is_none() && crate::scene::Animation::is_maker_pose(a));
    if widgets::check_item(ui, k, maker, "Default pose", "Moving").clicked() {
        chosen = Some(Some(crate::scene::Animation::maker_pose()));
        ui.close();
    }
    // An animation the scene has that the project does not own.
    if let (Some(worn), None, false) = (worn, worn_rel, maker) {
        let text = format!("{} (not in the project)", worn.name);
        let _ = widgets::check_item(ui, k, true, &text, "");
    }
    if st.animations.items.is_empty() {
        ui.separator();
        ui.label(egui::RichText::new("This project has no animations yet").color(k.t.text_dim));
        return chosen;
    }
    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        let mut folder: Option<&str> = None;
        for item in &st.animations.items {
            if folder != Some(item.folder()) {
                folder = Some(item.folder());
                ui.separator();
                let name = match item.folder().strip_prefix(crate::project::ANIMATIONS) {
                    Some("") | None => crate::project::ANIMATIONS.to_owned(),
                    Some(rest) => rest.trim_start_matches('/').replace('/', " / "),
                };
                ui.label(egui::RichText::new(name).font(k.small()).color(k.t.text_dim));
            }
            let kind = match item.is_static() {
                Some(true) => "Static",
                Some(false) => "Moving",
                None => "Linked",
            };
            if widgets::check_item(ui, k, worn_rel == Some(item.rel.as_str()), &item.name, kind).clicked() {
                chosen = Some(Some(item.reference()));
                ui.close();
            }
        }
    });
    chosen
}

/// The kinds the Add menu offers, as menu items with the kind's icon,
/// then the kinds of light and the effects.
pub fn add_items(ui: &mut Ui, k: &Look, st: &mut State) {
    use crate::scene::{Content, Effect, LightKind};
    let item = |ui: &mut Ui, glyph: &str, color: Color32, text: &str, on: bool| {
        let mut job = egui::text::LayoutJob::default();
        let format = |font, color| egui::TextFormat {
            font_id: font,
            color,
            valign: Align::Center,
            ..Default::default()
        };
        job.append(glyph, 0.0, format(k.icons(15.0), color));
        job.append(text, 8.0, format(k.font(), k.t.text));
        let resp = ui.add_enabled(on, egui::Button::new(job));
        let clicked = resp.on_disabled_hover_text("Not available yet").clicked();
        if clicked {
            ui.close();
        }
        clicked
    };
    for kind in [Kind::Character, Kind::Item, Kind::Camera, Kind::Map, Kind::Folder] {
        let text = if kind == Kind::Character {
            "Character from a card"
        } else {
            kind.label()
        };
        // An item has no model to add yet.
        if item(ui, kind.glyph(), kind.color(), text, kind != Kind::Item) {
            st.run(Command::Add(kind));
        }
    }
    let heading = |ui: &mut Ui, name: &str| {
        ui.separator();
        ui.label(egui::RichText::new(name).font(k.small()).color(k.t.text_dim));
    };
    heading(ui, "Lights");
    for light in LightKind::ALL {
        if item(ui, light.glyph(), Kind::Light.color(), light.label(), true) {
            st.run(Command::AddContent(Content::light_of(light)));
        }
    }
    heading(ui, "Effects");
    for effect in Effect::all() {
        if item(ui, effect.glyph(), Kind::Effect.color(), effect.label(), effect.drawn()) {
            st.run(Command::AddContent(Content::Effect { effect }));
        }
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
        if flat_button(ui, k, None, "Clear", false, false).clicked() {
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
            widgets::search(ui, k, &mut st.console_search, "Search", 240.0);
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
            ui.add_enabled_ui(false, |ui| icon_button(ui, k, g, false));
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
            let text = "Not available yet";
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
        (1, Kind::Animation, "Animation", &[0.0, 24.0, 60.0]),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_layout_comes_back_as_it_was() {
        let mut dock = default_layout(Panel::Content);
        let console = dock.find_tab(&Panel::Console).unwrap();
        dock.set_active_tab(console).unwrap();
        let text = toml::to_string(&dock).unwrap();
        let back: DockState<Panel> = toml::from_str(&text).unwrap();
        assert_eq!(toml::to_string(&back).unwrap(), text);
        assert!(back.find_tab(&Panel::Viewport).is_some());
    }
}
