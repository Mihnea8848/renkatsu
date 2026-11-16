// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The viewport and what lies over it: the mode rail, the active mode's
//! tool palette, the transform tool strip, the selection bar and the view
//! options. The picture is a still until the live viewport exists, so the
//! viewport stays a plain texture area.
//!
//! Everything over the picture can get out of the way. Choosing the active
//! mode again, or the palette's close button, folds the palette; unpinned,
//! it also folds as soon as work starts in the viewport. The rail's chevron
//! shrinks it to icons. The backtick key or the eye button hides every
//! overlay, and Shift with Space gives the viewport the whole window.

use crate::camera::RightDrag;
use crate::design::Overlays;
use crate::modes::{actions, pipeline_missing, Pipeline, MODES, PIPELINES, TOOLS};
use crate::panels::{dragged_over, dropped, DragCard};
use crate::state::{Command, Look, Rail, State};
use crate::theme::Theme;
use crate::widgets::{self, bar_divider, flat_button, glyph, hair_h, hair_v, icon_button, icon_cell};
use egui::{Align, Align2, Color32, Key, Layout, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use egui_phosphor::regular as icon;

/// How the rail lays out a mode: the name under the icon or beside it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RailShape {
    Stacked,
    Rows,
}

/// Draws the viewport into the space left in `ui`. `header` is the free
/// part of the viewport's tab row, which some designs use for the selection
/// bar; without it that bar is drawn over the picture's top edge.
pub fn show(ui: &mut Ui, k: &Look, st: &mut State, header: Option<Rect>) {
    let full = ui.available_rect_before_wrap();
    ui.painter().rect_filled(full, 0.0, st.backdrop);
    // A still picture stands in only until the scene has a character.
    let still = st.viewport_image.is_some() && !st.live.as_ref().is_some_and(|live| live.has_characters());
    if st.live.is_some() && !still {
        live_picture(ui, st, full);
    } else if let Some(tex) = &st.viewport_image {
        let size = tex.size_vec2();
        let scale = (full.width() / size.x).min(full.height() / size.y);
        let fit = Rect::from_center_size(full.center(), (size * scale).round());
        ui.painter().image(
            tex.id(),
            fit,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    let scene = ui.interact(full, ui.id().with("scene"), Sense::click_and_drag());
    navigate(ui, st, &scene, full);
    // A card dragged from the Content panel and let go here joins the scene.
    if dragged_over::<DragCard>(ui, full) {
        ui.painter().rect_stroke(
            full,
            0.0,
            egui::Stroke::new(2.0, k.t.select_bright),
            egui::StrokeKind::Inside,
        );
    }
    if let Some(card) = dropped::<DragCard>(ui, full) {
        st.run(Command::AddCard(card.0.clone()));
    }
    if (scene.clicked() || scene.drag_started()) && !st.palette_pinned {
        st.palette_open = false;
    }
    keys(ui, st);

    if st.overlays_hidden {
        match header {
            // The header row keeps its eye button, which brings them back.
            Some(header) if k.d.overlays == Overlays::Header => {
                in_header(ui, header, |ui| bar_contents(ui, k, st, header));
            }
            // Otherwise one small handle stays, so the tools can be found.
            // A selection bar in the header row is not over the picture
            // and stays as well.
            _ => {
                if let Some(header) = header {
                    in_header(ui, header, |ui| context_row(ui, k, st, header));
                }
                let handle = Rect::from_min_size(
                    Pos2::new(full.right() - 10.0 - k.field(), full.top() + 10.0),
                    Vec2::splat(k.field()),
                );
                let resp = ui
                    .interact(handle, ui.id().with("show-tools"), Sense::click())
                    .on_hover_text("Show the viewport tools (`)");
                block(ui, k, handle.expand(2.0));
                icon_cell(ui, k, handle, &resp, icon::EYE, false);
                if resp.clicked() {
                    st.overlays_hidden = false;
                }
            }
        }
    } else {
        match k.d.overlays {
            Overlays::Docked => docked(ui, k, st, full),
            Overlays::Floating => floating(ui, k, st, full),
            Overlays::Header => clean(ui, k, st, full, header),
            Overlays::Blocks => blocks(ui, k, st, full, header),
        }
        // The camera's corner is the bottom left one, clear of a rail that
        // runs down the whole left edge.
        let mut corner = full.left_bottom();
        if k.d.overlays == Overlays::Docked {
            corner.x += rail_width(k, st, RailShape::Stacked);
        }
        camera_corner(ui, k, st, corner);
    }
    speed_meter(ui, k, st, full);
    ui.allocate_rect(full, Sense::hover());
}

/// The keys of the viewport: the backtick hides the overlays, the number
/// keys pick a mode, and Unity's letters pick a transform tool.
fn keys(ui: &Ui, st: &mut State) {
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    let pressed = |key: Key| ui.input(|i| i.key_pressed(key) && i.modifiers.is_none());
    if pressed(Key::Backtick) {
        st.overlays_hidden = !st.overlays_hidden;
    }
    for (i, key) in [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6]
        .into_iter()
        .enumerate()
    {
        if pressed(key) {
            st.pick_mode(i);
        }
    }
    // F focuses the camera on the selection and frames it; Home goes back
    // to the starting view.
    if pressed(Key::F) {
        focus_selection(st);
    }
    if pressed(Key::Home) {
        if let Some(live) = st.live.as_mut() {
            live.cam.reset();
        }
    }
    // While the right button is held these keys fly the camera instead.
    if ui.input(|i| i.pointer.secondary_down()) {
        return;
    }
    for (i, key) in [Key::Q, Key::W, Key::E, Key::R, Key::T].into_iter().enumerate() {
        if pressed(key) {
            st.tool = i + 1;
        }
    }
}

/// The dark see-through surface of bars lying on the picture.
fn glass(k: &Look) -> Color32 {
    Theme::glass(k.t.well, 242)
}

/// The box of a floating overlay.
fn block(ui: &Ui, k: &Look, rect: Rect) {
    let p = ui.painter();
    p.rect_filled(rect, k.radius(), glass(k));
    p.rect_stroke(
        rect,
        k.radius(),
        Stroke::new(1.0, Theme::glass(k.t.hover, 110)),
        StrokeKind::Inside,
    );
}

/// The surface of a palette that hangs from a bar: the panel colour, with
/// hairlines along the two edges that meet the picture.
fn drawer(ui: &Ui, k: &Look, rect: Rect) {
    let p = ui.painter();
    p.rect_filled(rect, 0.0, Theme::glass(k.t.panel, 244));
    let line = Theme::glass(k.t.divider, 200);
    hair_v(p, rect.right() - 1.0, rect.top(), rect.bottom(), line);
    hair_h(p, rect.left(), rect.right(), rect.bottom() - 1.0, line);
}

/// Bars on the viewport's edges: the selection bar along the top, the rail
/// down the left side, the palette beside the rail.
fn docked(ui: &mut Ui, k: &Look, st: &mut State, full: Rect) {
    let bar = Rect::from_min_size(full.min, Vec2::new(full.width(), k.d.row + 8.0));
    ui.painter().rect_filled(bar, 0.0, glass(k));
    bar_contents(ui, k, st, bar);

    let rail_rect = Rect::from_min_max(
        Pos2::new(full.left(), bar.bottom()),
        Pos2::new(full.left() + rail_width(k, st, RailShape::Stacked), full.bottom()),
    );
    ui.painter().rect_filled(rail_rect, 0.0, glass(k));
    rail(ui, k, st, rail_rect, RailShape::Stacked);

    let mut left = rail_rect.right();
    if st.palette_open {
        let pal = Rect::from_min_size(
            Pos2::new(left, bar.bottom()),
            Vec2::new(184.0, palette_height(k, st.mode).min(rail_rect.height())),
        );
        drawer(ui, k, pal);
        palette(ui, k, st, pal);
        left = pal.right();
    }
    tool_strip(ui, k, st, Pos2::new(left + 8.0, bar.bottom() + 8.0), true);
}

/// Separate blocks floating over the picture, as Unity 6's overlays: the
/// tool strip, the selection bar and the view options along the top, the
/// rail below the strip and the palette beside the rail.
fn floating(ui: &mut Ui, k: &Look, st: &mut State, full: Rect) {
    floating_blocks(ui, k, st, full, true);
}

/// The floating blocks with the selection bar built into the top: in the
/// viewport's header row, or without one in a bar along the top edge.
fn blocks(ui: &mut Ui, k: &Look, st: &mut State, full: Rect, header: Option<Rect>) {
    let mut area = full;
    match header {
        Some(header) => in_header(ui, header, |ui| context_row(ui, k, st, header)),
        None => {
            let bar = Rect::from_min_size(full.min, Vec2::new(full.width(), k.d.row + 8.0));
            ui.painter().rect_filled(bar, 0.0, glass(k));
            context_row(ui, k, st, bar);
            area.min.y = bar.bottom();
        }
    }
    floating_blocks(ui, k, st, area, false);
}

/// The blocks over `full`. `selection` adds the selection bar as a block
/// of its own.
fn floating_blocks(ui: &mut Ui, k: &Look, st: &mut State, full: Rect, selection: bool) {
    let m = 8.0;
    let h = k.d.row + 6.0;
    let top = full.top() + m;
    let strip = tool_strip(ui, k, st, Pos2::new(full.left() + m, top), false);

    // View options, sized by what they hold, against the right edge.
    let slot = ui.painter().add(egui::Shape::Noop);
    let area = Rect::from_min_max(
        Pos2::new(strip.right() + m, top),
        Pos2::new(full.right() - m - 4.0, top + h),
    );
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(area)
            .layout(Layout::right_to_left(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 2.0;
    view_items(&mut row, k, st);
    let view = Rect::from_min_max(
        Pos2::new(row.min_rect().left() - 4.0, top),
        Pos2::new(full.right() - m, top + h),
    );
    set_block(ui, k, slot, view);

    // The selection bar, centred when there is room for it.
    if selection && st.selection().is_some() {
        let id = ui.id().with("context-width");
        let known: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(420.0);
        let w = known + 16.0;
        let lo = strip.right() + m;
        let hi = (view.left() - m - w).max(lo);
        let x = (full.center().x - w / 2.0).clamp(lo, hi).round();
        let rect = Rect::from_min_size(Pos2::new(x, top), Vec2::new(w, h));
        block(ui, k, rect);
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(8.0, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        row.spacing_mut().item_spacing.x = 2.0;
        context_items(&mut row, k, st);
        let used = row.min_rect().width();
        ui.ctx().data_mut(|d| d.insert_temp(id, used));
    }

    let shape = RailShape::Rows;
    let rail_rect = Rect::from_min_size(
        Pos2::new(full.left() + m, strip.bottom() + m),
        Vec2::new(
            rail_width(k, st, shape),
            MODES.len() as f32 * rail_cell(k, st, shape) + 22.0,
        ),
    );
    block(ui, k, rail_rect);
    rail(ui, k, st, rail_rect, shape);
    if st.palette_open {
        let pal = Rect::from_min_size(
            Pos2::new(rail_rect.right() + 4.0, rail_rect.top()),
            Vec2::new(188.0, palette_height(k, st.mode)),
        );
        block(ui, k, pal);
        palette(ui, k, st, pal);
    }
}

/// The picture with as little on it as possible: the rail belongs to the
/// window and the selection bar to the header row, so only the palette and
/// the tool strip lie over the picture.
fn clean(ui: &mut Ui, k: &Look, st: &mut State, full: Rect, header: Option<Rect>) {
    let mut top = full.top();
    match header {
        Some(header) => in_header(ui, header, |ui| bar_contents(ui, k, st, header)),
        None => {
            let bar = Rect::from_min_size(full.min, Vec2::new(full.width(), k.d.row + 8.0));
            ui.painter().rect_filled(bar, 0.0, Theme::glass(k.t.panel, 244));
            bar_contents(ui, k, st, bar);
            top = bar.bottom();
        }
    }
    let mut left = full.left();
    if st.palette_open {
        let pal = Rect::from_min_size(
            Pos2::new(left, top),
            Vec2::new(196.0, palette_height(k, st.mode).min(full.bottom() - top)),
        );
        drawer(ui, k, pal);
        palette(ui, k, st, pal);
        left = pal.right();
    }
    tool_strip(ui, k, st, Pos2::new(left + 8.0, top + 8.0), true);
}

/// Runs `add` in a child whose clip reaches the header row above the
/// viewport's own area.
fn in_header(ui: &mut Ui, header: Rect, add: impl FnOnce(&mut Ui)) {
    let mut child = ui.new_child(UiBuilder::new().max_rect(header));
    child.set_clip_rect(header);
    add(&mut child);
}

/// The selection and its actions from the left, the view options from the
/// right, inside a bar.
fn bar_contents(ui: &mut Ui, k: &Look, st: &mut State, bar: Rect) {
    let inner = bar.shrink2(Vec2::new(8.0, 0.0));
    let mut right = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::right_to_left(Align::Center)),
    );
    right.spacing_mut().item_spacing.x = 2.0;
    view_items(&mut right, k, st);
    let limit = right.min_rect().left() - 8.0;

    let mut left = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(inner.min, Pos2::new(limit, inner.bottom())))
            .layout(Layout::left_to_right(Align::Center)),
    );
    left.set_clip_rect(left.clip_rect().intersect(left.max_rect()));
    left.spacing_mut().item_spacing.x = 2.0;
    left.add_space(4.0);
    context_items(&mut left, k, st);
}

/// The selection and its actions alone, from the left of `bar`.
fn context_row(ui: &mut Ui, k: &Look, st: &mut State, bar: Rect) {
    let inner = bar.shrink2(Vec2::new(8.0, 0.0));
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.set_clip_rect(row.clip_rect().intersect(inner));
    row.spacing_mut().item_spacing.x = 2.0;
    row.add_space(4.0);
    context_items(&mut row, k, st);
}

/// A piece of text in a bar, centred on the row.
fn bar_text(ui: &mut Ui, text: &str, font: egui::FontId, color: Color32) {
    let w = widgets::text_width(ui, text, font.clone()).ceil();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 20.0), Sense::hover());
    ui.painter()
        .text(rect.left_center(), Align2::LEFT_CENTER, text, font, color);
}

/// The selected object and what can be done with it.
fn context_items(ui: &mut Ui, k: &Look, st: &mut State) {
    let Some((kind, name)) = st.selection() else {
        bar_text(ui, "Nothing selected", k.font(), k.t.text_dim);
        return;
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(18.0, 20.0), Sense::hover());
    glyph(ui.painter(), k, rect.center(), kind.glyph(), 16.0, kind.color());
    ui.add_space(4.0);
    bar_text(ui, &name, k.bold(k.d.text), k.t.text_bright);
    ui.add_space(5.0);
    bar_text(ui, kind.label(), k.font(), k.t.text_dim);
    ui.add_space(4.0);
    bar_divider(ui, k);
    ui.add_space(2.0);
    for (g, text, menu) in actions(kind) {
        let resp = flat_button(ui, k, Some(g), text, *menu, false);
        if *text == "Load card" {
            if resp.on_hover_text("Show the cards in the Content panel").clicked() {
                st.show_content = true;
            }
        } else if *menu {
            egui::Popup::menu(&resp).show(|ui| {
                ui.label(egui::RichText::new(format!("{text} arrives with its feature")).color(k.t.text_dim));
            });
        }
    }
}

/// Camera, shading and snapping, then the two buttons that clear the view.
/// Laid out from the right.
fn view_items(ui: &mut Ui, k: &Look, st: &mut State) {
    let glyph = if st.maximised {
        icon::CORNERS_IN
    } else {
        icon::CORNERS_OUT
    };
    if icon_button(ui, k, glyph, st.maximised)
        .on_hover_text("Give the viewport the whole window (Shift+Space)")
        .clicked()
    {
        st.maximised = !st.maximised;
    }
    if icon_button(ui, k, icon::EYE_SLASH, st.overlays_hidden)
        .on_hover_text("Hide or show the viewport tools (`)")
        .clicked()
    {
        st.overlays_hidden = !st.overlays_hidden;
    }
    bar_divider(ui, k);
    if flat_button(ui, k, Some(icon::MAGNET), "Snap", false, st.snap)
        .on_hover_text("Snap while moving, rotating and scaling")
        .clicked()
    {
        st.snap = !st.snap;
    }
    for (g, text, options) in [
        (icon::SUN, "Lit", ["Lit", "Unlit", "Wireframe"]),
        (icon::CUBE_FOCUS, "Perspective", ["Perspective", "Front", "Side"]),
    ] {
        let resp = flat_button(ui, k, Some(g), text, true, false);
        egui::Popup::menu(&resp).show(|ui| {
            for option in options {
                if widgets::check_item(ui, k, option == text, option, "").clicked() {
                    ui.close();
                }
            }
        });
    }
    // The rendering pipeline, by its abbreviation: the bar has little room.
    let active = &PIPELINES[0];
    let resp = flat_button(ui, k, Some(icon::STACK), active.short, true, false)
        .on_hover_text(format!("Rendering pipeline: {}", active.name));
    egui::Popup::menu(&resp).show(|ui| {
        for pipeline in &PIPELINES {
            let text = format!("{} ({})", pipeline.name, pipeline.short);
            let on = pipeline.short == active.short;
            let item = ui
                .add_enabled_ui(pipeline.available, |ui| widgets::check_item(ui, k, on, &text, ""))
                .inner
                .on_disabled_hover_text(pipeline_missing(pipeline));
            if item.clicked() {
                ui.close();
            }
        }
    });
}

/// Width of the rail for a shape and the rail's state.
pub fn rail_width(k: &Look, st: &State, shape: RailShape) -> f32 {
    match (shape, st.rail) {
        (RailShape::Stacked, Rail::Named) => 62.0,
        (RailShape::Stacked, Rail::Icons) => 40.0,
        (RailShape::Rows, Rail::Named) => 112.0,
        (RailShape::Rows, Rail::Icons) => k.d.row + 6.0,
    }
}

/// Height of one mode in the rail.
fn rail_cell(k: &Look, st: &State, shape: RailShape) -> f32 {
    match (shape, st.rail) {
        (RailShape::Stacked, Rail::Named) => 52.0,
        (RailShape::Stacked, Rail::Icons) => 40.0,
        (RailShape::Rows, _) => k.d.row + 6.0,
    }
}

/// The named modes in `rect`, with the chevron that shrinks the rail to
/// icons at its bottom. The caller paints what lies behind.
pub fn rail(ui: &mut Ui, k: &Look, st: &mut State, rect: Rect, shape: RailShape) {
    let cell_h = rail_cell(k, st, shape);
    let named = st.rail == Rail::Named;
    for (i, mode) in MODES.iter().enumerate() {
        let cell = Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + i as f32 * cell_h),
            Vec2::new(rect.width(), cell_h),
        );
        let mut resp = ui.interact(cell, ui.id().with(("mode", i)), Sense::click());
        if !named {
            resp = resp.on_hover_text(format!("{} mode ({})", mode.name, i + 1));
        }
        if resp.clicked() {
            st.pick_mode(i);
        }
        let on = st.mode == i;
        let p = ui.painter();
        let lit = match shape {
            RailShape::Stacked => cell,
            RailShape::Rows => cell.shrink(2.0),
        };
        if on {
            let fill = match shape {
                RailShape::Stacked => k.t.panel,
                RailShape::Rows => k.t.control,
            };
            p.rect_filled(lit, if shape == RailShape::Rows { 2.0 } else { 0.0 }, fill);
            if shape == RailShape::Stacked {
                p.rect_filled(
                    Rect::from_min_size(cell.min, Vec2::new(2.0, cell.height())),
                    0.0,
                    k.t.select,
                );
            }
        } else if resp.hovered() {
            p.rect_filled(
                lit,
                if shape == RailShape::Rows { 2.0 } else { 0.0 },
                Theme::glass(k.t.control, 150),
            );
        }
        let color = if on {
            k.t.text_bright
        } else if resp.hovered() {
            k.t.text
        } else {
            k.t.text_dim
        };
        let tint = if on && shape == RailShape::Rows {
            k.t.select_bright
        } else {
            color
        };
        match (shape, named) {
            (RailShape::Stacked, true) => {
                glyph(
                    p,
                    k,
                    Pos2::new(cell.center().x, cell.top() + 20.0),
                    mode.glyph,
                    20.0,
                    tint,
                );
                p.text(
                    Pos2::new(cell.center().x, cell.top() + 39.0),
                    Align2::CENTER_CENTER,
                    mode.name,
                    k.small(),
                    color,
                );
            }
            (RailShape::Stacked, false) => glyph(p, k, cell.center(), mode.glyph, 20.0, tint),
            (RailShape::Rows, true) => {
                glyph(
                    p,
                    k,
                    Pos2::new(cell.left() + 15.0, cell.center().y),
                    mode.glyph,
                    16.0,
                    tint,
                );
                widgets::label(p, k, Pos2::new(cell.left() + 30.0, cell.center().y), mode.name, color);
            }
            (RailShape::Rows, false) => glyph(p, k, cell.center(), mode.glyph, 16.0, tint),
        }
    }

    let fold_h = if shape == RailShape::Stacked { 28.0 } else { 22.0 };
    let fold = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - fold_h), rect.max);
    let resp = ui
        .interact(fold, ui.id().with("rail-fold"), Sense::click())
        .on_hover_text(if named {
            "Shrink the modes to icons"
        } else {
            "Show the mode names"
        });
    if resp.clicked() {
        st.rail = if named { Rail::Icons } else { Rail::Named };
    }
    let p = ui.painter();
    hair_h(
        p,
        fold.left() + 6.0,
        fold.right() - 6.0,
        fold.top(),
        Theme::glass(k.t.hover, 90),
    );
    let arrow = if named {
        icon::CARET_DOUBLE_LEFT
    } else {
        icon::CARET_DOUBLE_RIGHT
    };
    let color = if resp.hovered() { k.t.text_bright } else { k.t.text_dim };
    glyph(p, k, fold.center(), arrow, 12.0, color);
}

fn palette_row(k: &Look) -> f32 {
    k.d.row + 2.0
}

/// Height the palette needs for a mode's tools.
fn palette_height(k: &Look, mode: usize) -> f32 {
    let sections = MODES[mode].sections;
    let rows: usize = sections.iter().map(|(_, tools)| tools.len()).sum();
    34.0 + sections.len() as f32 * 24.0 + rows as f32 * palette_row(k) + 8.0
}

/// The active mode's tools in named sections, under a header that pins or
/// closes the palette. The caller paints what lies behind.
fn palette(ui: &mut Ui, k: &Look, st: &mut State, rect: Rect) {
    // Claim the area so that clicks do not reach the scene below.
    ui.interact(rect, ui.id().with("palette"), Sense::click());
    let mode = &MODES[st.mode];
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 34.0));
    ui.painter().text(
        Pos2::new(head.left() + 12.0, head.center().y),
        Align2::LEFT_CENTER,
        mode.name,
        k.bold(k.d.text + 1.0),
        k.t.text_bright,
    );
    let side = k.field();
    let close = Rect::from_center_size(
        Pos2::new(head.right() - 8.0 - side / 2.0, head.center().y),
        Vec2::splat(side),
    );
    let resp = ui
        .interact(close, ui.id().with("palette-close"), Sense::click())
        .on_hover_text("Close the tools; choosing the mode again does the same");
    icon_cell(ui, k, close, &resp, icon::X, false);
    if resp.clicked() {
        st.palette_open = false;
    }
    let pin = close.translate(Vec2::new(-side - 2.0, 0.0));
    let resp = ui
        .interact(pin, ui.id().with("palette-pin"), Sense::click())
        .on_hover_text(if st.palette_pinned {
            "Pinned: the tools stay open. Unpin to fold them when work starts in the viewport"
        } else {
            "Pin the tools open"
        });
    icon_cell(ui, k, pin, &resp, icon::PUSH_PIN, st.palette_pinned);
    if resp.clicked() {
        st.palette_pinned = !st.palette_pinned;
    }
    hair_h(
        ui.painter(),
        rect.left(),
        rect.right(),
        head.bottom() - 1.0,
        Theme::glass(k.t.hover, 70),
    );

    let mut y = head.bottom();
    let mut index = 0;
    for (section, tools) in mode.sections {
        ui.painter().text(
            Pos2::new(rect.left() + 12.0, y + 14.0),
            Align2::LEFT_CENTER,
            *section,
            k.small(),
            k.t.text_dim,
        );
        y += 24.0;
        for (g, name) in *tools {
            let row = Rect::from_min_size(
                Pos2::new(rect.left() + 6.0, y + 1.0),
                Vec2::new(rect.width() - 12.0, palette_row(k) - 2.0),
            );
            let id = ui.id().with(("tool", st.mode, index));
            if let Some(pipeline) = PIPELINES.iter().find(|p| p.tool == *name) {
                pipeline_row(ui, k, row, id, g, pipeline);
            } else {
                let on = st.mode_tool[st.mode] == index;
                if widgets::tool_row(ui, k, row, id, g, name, on).clicked() {
                    st.mode_tool[st.mode] = index;
                }
            }
            y += palette_row(k);
            index += 1;
        }
    }
}

/// A rendering pipeline among a mode's tools: the one in use carries a
/// check mark, and one that does not exist yet is greyed and says so.
fn pipeline_row(ui: &mut Ui, k: &Look, row: Rect, id: egui::Id, g: &str, pipeline: &Pipeline) {
    let at = Pos2::new(row.right() - 14.0, row.center().y);
    if pipeline.available {
        widgets::tool_row(ui, k, row, id, g, pipeline.tool, false).on_hover_text(pipeline.name);
        glyph(ui.painter(), k, at, icon::CHECK, 12.0, k.t.select_bright);
    } else {
        ui.interact(row, id, Sense::hover())
            .on_hover_text(pipeline_missing(pipeline));
        let faint = Theme::glass(k.t.text_dim, 150);
        let p = ui.painter();
        glyph(p, k, Pos2::new(row.left() + 16.0, row.center().y), g, 16.0, faint);
        widgets::label(p, k, Pos2::new(row.left() + 32.0, row.center().y), pipeline.tool, faint);
    }
}

/// Focuses the camera on the selected object and frames it, when the
/// object is something the viewport draws.
fn focus_selection(st: &mut State) {
    let Some(live) = st.live.as_mut() else { return };
    if let Some(focus) = st.doc.selected.and_then(|id| live.subject(id)) {
        live.cam.focus_on(focus);
    }
}

/// The camera's corner of the viewport, at the bottom left: what the
/// camera is focused on, and, once the view has left its starting framing,
/// the button that goes back to it. `corner` is the bottom left point.
fn camera_corner(ui: &mut Ui, k: &Look, st: &mut State, corner: Pos2) {
    let Some(live) = st.live.as_ref() else { return };
    let m = 8.0;
    let h = k.d.row + 6.0;
    let focus = live
        .cam
        .focus
        .and_then(|f| st.doc.scene().get(f.object))
        .map(|o| (o.kind(), o.name.clone()));
    let moved = live.cam.moved();
    let name = match &focus {
        Some((_, name)) => name.as_str(),
        None => "Free camera",
    };
    // The target, then the object's kind and name; the reset button joins
    // at the right end so that nothing shifts when it appears.
    let name_x = if focus.is_some() { 54.0 } else { 32.0 };
    let w = (name_x + widgets::text_width(ui, name, k.font()) + 10.0).ceil();
    let cell = Rect::from_min_size(Pos2::new(corner.x + m, corner.y - m - h), Vec2::new(w, h));
    let reset = Rect::from_min_size(cell.right_top(), Vec2::new(h, h));
    let whole = if moved { cell.union(reset) } else { cell };
    block(ui, k, whole);

    let can_focus = st.doc.selected.and_then(|id| live.subject(id));
    let tip = match (&focus, can_focus) {
        (Some((kind, name)), _) => format!(
            "The camera is focused on the {} {name}: it orbits around it. Click to let go",
            kind.label().to_lowercase()
        ),
        (None, Some(_)) => "The camera is free. Click or press F to focus on the selection".to_owned(),
        (None, None) => "The camera is free. Select a character and press F to focus on it".to_owned(),
    };
    let resp = ui
        .interact(cell, ui.id().with("camera-focus"), Sense::click())
        .on_hover_text(tip);
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(cell.shrink(3.0), 2.0, k.t.control);
    }
    let y = cell.center().y;
    match &focus {
        Some((kind, name)) => {
            glyph(
                p,
                k,
                Pos2::new(cell.left() + 16.0, y),
                icon::GPS_FIX,
                16.0,
                k.t.select_bright,
            );
            glyph(p, k, Pos2::new(cell.left() + 39.0, y), kind.glyph(), 15.0, kind.color());
            widgets::label(p, k, Pos2::new(cell.left() + name_x, y), name, k.t.text_bright);
        }
        None => {
            glyph(
                p,
                k,
                Pos2::new(cell.left() + 16.0, y),
                icon::GPS_SLASH,
                16.0,
                k.t.text_dim,
            );
            widgets::label(p, k, Pos2::new(cell.left() + name_x, y), name, k.t.text_dim);
        }
    }
    let toggled = resp.clicked();

    let mut go_home = false;
    if moved {
        let resp = ui
            .interact(reset, ui.id().with("camera-reset"), Sense::click())
            .on_hover_text("Reset the view to the starting framing (Home)");
        let p = ui.painter();
        hair_v(
            p,
            reset.left(),
            reset.top() + 6.0,
            reset.bottom() - 6.0,
            Theme::glass(k.t.hover, 110),
        );
        if resp.hovered() {
            p.rect_filled(reset.shrink(3.0), 2.0, k.t.control);
        }
        let color = if resp.hovered() { Color32::WHITE } else { k.t.text };
        glyph(p, k, reset.center(), icon::ARROWS_COUNTER_CLOCKWISE, 16.0, color);
        go_home = resp.clicked();
    }

    if go_home {
        if let Some(live) = st.live.as_mut() {
            live.cam.reset();
        }
    } else if toggled {
        if focus.is_some() {
            if let Some(live) = st.live.as_mut() {
                live.cam.release();
            }
        } else {
            focus_selection(st);
        }
    }
}

/// How long the speed meter stays after the speed changed, then how long
/// it takes to fade, in seconds.
const METER_HOLD: f64 = 1.0;
const METER_FADE: f64 = 0.5;

/// The fly speed as a small meter at the bottom of the viewport, shown
/// when the speed changes and fading away a moment later.
fn speed_meter(ui: &Ui, k: &Look, st: &mut State, full: Rect) {
    let (Some(shown), Some(live)) = (st.speed_shown, st.live.as_ref()) else {
        return;
    };
    let age = ui.input(|i| i.time) - shown;
    if age >= METER_HOLD + METER_FADE {
        st.speed_shown = None;
        return;
    }
    let alpha = (1.0 - (age - METER_HOLD) / METER_FADE).clamp(0.0, 1.0) as f32;
    ui.ctx().request_repaint();

    let h = k.d.row + 6.0;
    let rect = Rect::from_center_size(
        Pos2::new(full.center().x.round(), full.bottom() - 8.0 - h / 2.0),
        Vec2::new(176.0, h),
    );
    let fade = |c: Color32| c.gamma_multiply(alpha);
    let p = ui.painter();
    p.rect_filled(rect, k.radius(), fade(glass(k)));
    p.rect_stroke(
        rect,
        k.radius(),
        Stroke::new(1.0, fade(Theme::glass(k.t.hover, 110))),
        StrokeKind::Inside,
    );
    let y = rect.center().y;
    glyph(
        p,
        k,
        Pos2::new(rect.left() + 16.0, y),
        icon::GAUGE,
        16.0,
        fade(k.t.text),
    );
    // A track from the slowest to the fastest speed, with a notch at the
    // usual one in its middle.
    let track = Rect::from_min_max(
        Pos2::new(rect.left() + 32.0, y - 1.5),
        Pos2::new(rect.right() - 52.0, y + 1.5),
    );
    p.rect_filled(track, 1.0, fade(k.t.control));
    let reach = track.left() + track.width() * live.cam.speed_fraction();
    p.rect_filled(
        Rect::from_min_max(track.min, Pos2::new(reach, track.bottom())),
        1.0,
        fade(k.t.select_bright),
    );
    hair_v(p, track.center().x.round(), y - 4.0, y + 4.0, fade(k.t.text_dim));
    p.text(
        Pos2::new(rect.right() - 10.0, y),
        Align2::RIGHT_CENTER,
        speed_text(live.cam.speed),
        k.bold(k.d.text),
        fade(k.t.text_bright),
    );
}

/// The fly speed as it is written on the meter: a multiple of the usual
/// speed, with more digits below it.
fn speed_text(speed: f32) -> String {
    if speed < 1.0 {
        format!("{speed:.2}x")
    } else {
        format!("{speed:.1}x")
    }
}

/// Fills a shape slot reserved earlier with the box of a floating overlay.
fn set_block(ui: &Ui, k: &Look, slot: egui::layers::ShapeIdx, rect: Rect) {
    ui.painter().set(
        slot,
        egui::epaint::RectShape::new(
            rect,
            k.radius(),
            glass(k),
            Stroke::new(1.0, Theme::glass(k.t.hover, 110)),
            StrokeKind::Inside,
        ),
    );
}

/// The transform tools as a strip floating on the picture. Returns the
/// strip's rectangle.
fn tool_strip(ui: &mut Ui, k: &Look, st: &mut State, origin: Pos2, vertical: bool) -> Rect {
    let cell = k.d.row + 6.0;
    let step = if vertical {
        Vec2::new(0.0, cell)
    } else {
        Vec2::new(cell, 0.0)
    };
    let rect = Rect::from_min_size(origin, Vec2::splat(cell) + step * (TOOLS.len() as f32 - 1.0));
    block(ui, k, rect);
    for (i, (g, name, key)) in TOOLS.iter().enumerate() {
        let r = Rect::from_min_size(origin + step * i as f32, Vec2::splat(cell));
        let resp = ui
            .interact(r, ui.id().with(("tool-strip", i)), Sense::click())
            .on_hover_text(format!("{name} ({key})"));
        if resp.clicked() {
            st.tool = i;
        }
        let p = ui.painter();
        let on = st.tool == i;
        if on {
            p.rect_filled(r.shrink(3.0), 2.0, k.t.select);
        } else if resp.hovered() {
            p.rect_filled(r.shrink(3.0), 2.0, k.t.control);
        }
        let color = if on || resp.hovered() { Color32::WHITE } else { k.t.text };
        glyph(p, k, r.center(), g, 16.0, color);
        // A hairline sets the navigation tools and the pose tool apart.
        if i == 2 || i == 5 {
            let line = Theme::glass(k.t.hover, 110);
            if vertical {
                hair_h(p, r.left() + 6.0, r.right() - 6.0, r.top(), line);
            } else {
                hair_v(p, r.left(), r.top() + 6.0, r.bottom() - 6.0, line);
            }
        }
    }
    rect
}

/// Draws the scene's characters into the viewport, after bringing them
/// in line with the scene: stepping the physics while it plays and putting
/// it back to rest when it is stopped.
fn live_picture(ui: &Ui, st: &mut State, full: Rect) {
    let Some(live) = st.live.as_mut() else { return };
    let dt = ui.input(|i| i.stable_dt).min(1.0 / 30.0);
    live.follow(st.doc.scene(), st.last_physics, st.physics, dt, &mut st.log);
    if st.physics == crate::state::Physics::Playing {
        ui.ctx().request_repaint();
    }
    st.last_physics = st.physics;
    let ppp = ui.ctx().pixels_per_point();
    let size = [
        (full.width() * ppp).round() as u32,
        (full.height() * ppp).round() as u32,
    ];
    match live.paint(st.doc.scene(), size, [0.22, 0.22, 0.23, 1.0]) {
        Ok(id) => {
            ui.painter().image(
                id,
                full,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        Err(e) => {
            ui.painter().text(
                full.center(),
                Align2::CENTER_CENTER,
                format!("The viewport could not draw: {e:#}"),
                egui::FontId::proportional(13.0),
                Color32::LIGHT_RED,
            );
        }
    }
}

/// What a drag in the viewport is doing, as in Unity.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Nav {
    /// Right button with a movement key, or with nothing focused: turn
    /// where the camera stands, and fly with W A S D.
    Explore,
    /// Right button, or Alt and the left button: go around the focused
    /// object.
    Orbit,
    /// Middle button: slide the view.
    Pan,
    /// Alt and the right button: move closer or farther.
    Zoom,
}

impl Nav {
    /// The symbol the pointer turns into, after Unity's: an eye while
    /// exploring, a turning arrow while orbiting, a hand while panning and
    /// a magnifier while zooming. Drawn with our own icon font.
    fn glyph(self) -> &'static str {
        match self {
            Nav::Explore => icon::EYE,
            Nav::Orbit => icon::ARROWS_CLOCKWISE,
            Nav::Pan => icon::HAND_GRABBING,
            Nav::Zoom => icon::MAGNIFYING_GLASS,
        }
    }
}

/// The viewport's navigation, Unity's with a focused object. Holding the
/// right button goes around the focused object; a movement key pressed
/// during the hold (W A S D, Q and E for down and up) switches to
/// exploring, which looks around where the camera stands and flies, faster
/// with Shift, until the button is let go. With nothing focused the right
/// button explores from the start. The wheel zooms, and while the right
/// button is held it changes the fly speed instead. Alt with the left
/// button orbits, the middle button pans, Alt with the right button zooms.
fn navigate(ui: &Ui, st: &mut State, scene: &egui::Response, full: Rect) {
    use egui::PointerButton::{Middle, Primary, Secondary};
    let Some(live) = st.live.as_mut() else { return };
    let cam = &mut live.cam;
    let (alt, shift, right_down) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift, i.pointer.secondary_down()));
    let delta = scene.drag_delta();
    let right = right_down && (scene.dragged_by(Secondary) || scene.is_pointer_button_down_on());
    let nav = if right && alt {
        cam.orbit.zoom((delta.x - delta.y) * 2.0);
        Some(Nav::Zoom)
    } else if right {
        let typing = ui.ctx().egui_wants_keyboard_input();
        let held = |key: Key| {
            if !typing && ui.input(|i| i.key_down(key)) {
                1.0
            } else {
                0.0
            }
        };
        let along = glam::Vec3::new(
            held(Key::D) - held(Key::A),
            held(Key::E) - held(Key::Q),
            held(Key::W) - held(Key::S),
        );
        let dt = ui.input(|i| i.stable_dt).min(1.0 / 20.0);
        // Keep drawing while the button is held, so flying is smooth.
        ui.ctx().request_repaint();
        Some(match cam.right_held(delta, along, dt, shift) {
            RightDrag::Orbit => Nav::Orbit,
            RightDrag::Explore => Nav::Explore,
        })
    } else if alt && scene.dragged_by(Primary) {
        cam.orbit_drag(delta);
        Some(Nav::Orbit)
    } else if scene.dragged_by(Middle) {
        cam.orbit.pan(delta, full.height());
        Some(Nav::Pan)
    } else {
        None
    };
    if !right_down {
        cam.right_released();
    }
    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
    if scroll != 0.0 && right {
        cam.scroll_speed(scroll);
        st.speed_shown = Some(ui.input(|i| i.time));
    } else if scroll != 0.0 && scene.hovered() {
        cam.orbit.zoom(scroll);
    }
    if let Some(nav) = nav {
        nav_cursor(ui, nav);
    }
}

/// Hides the system pointer and draws the action's symbol in its place,
/// with a dark edge so it reads on any picture.
fn nav_cursor(ui: &Ui, nav: Nav) {
    let Some(at) = ui.input(|i| i.pointer.latest_pos()) else {
        return;
    };
    ui.ctx().set_cursor_icon(egui::CursorIcon::None);
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("nav cursor")));
    let font = egui::FontId::proportional(20.0);
    for offset in [
        Vec2::new(-1.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(0.0, -1.0),
        Vec2::new(0.0, 1.0),
    ] {
        painter.text(
            at + offset,
            Align2::CENTER_CENTER,
            nav.glyph(),
            font.clone(),
            Color32::from_black_alpha(220),
        );
    }
    painter.text(at, Align2::CENTER_CENTER, nav.glyph(), font, Color32::WHITE);
}
