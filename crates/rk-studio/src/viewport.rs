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

use crate::camera::{Focus, RightDrag, View};
use crate::design::Overlays;
use crate::keymap::Action;
use crate::kinds::Kind;
use crate::modes::{actions, pipeline_missing, Pipeline, LATER, MODES, PIPELINES, TOOLS};
use crate::panels::{dragged_over, dropped, DragCard};
use crate::scene::ObjectId;
use crate::state::{Command, Look, Rail, State};
use crate::theme::Theme;
use crate::widgets::{self, bar_divider, flat_button, glyph, hair_h, hair_v, icon_button, icon_cell};
use egui::{Align, Align2, Color32, Layout, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use egui_phosphor::regular as icon;
use glam::Vec3;

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
    let selected = st.doc.selected_all();
    let ppp = ui.ctx().pixels_per_point();
    if let Some(live) = st.live.as_mut() {
        live.smooth_edges = st.settings.smooth_edges;
        // The selected characters get an outline in the selection colour.
        live.outline = (!selected.is_empty()).then(|| crate::live::Outline {
            objects: selected,
            color: egui::Rgba::from(k.t.select_bright).to_array(),
            width: 2.0 * ppp,
        });
    }
    let full = ui.available_rect_before_wrap();
    ui.painter().rect_filled(full, 0.0, st.backdrop);
    // A still picture stands in only until the scene has a character.
    let still = st.viewport_image.is_some() && !st.live.as_ref().is_some_and(|live| live.has_characters());
    let scene = ui.interact(full, ui.id().with("scene"), Sense::click_and_drag());
    // The pointer's work on the scene comes before the picture, so that
    // the picture shows where this frame's drag put things.
    let busy = crate::gizmo::interact(ui, st, &scene, full);
    if st.live.is_some() && !still {
        live_picture(ui, st, full);
        crate::gizmo::draw(ui, k, st, full);
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
    if !busy {
        navigate(ui, st, &scene, full);
        turn_sun(ui, st, &scene);
    }
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
                    .on_hover_text(st.keys.tip("Show the viewport tools", Action::ViewportTools));
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
    // The shown inputs stack above the camera's corner.
    let lift = if st.overlays_hidden || st.live.is_none() {
        8.0
    } else {
        8.0 + k.d.row + 6.0 + 6.0
    };
    let mut at = full.left_bottom() + Vec2::new(8.0, -lift);
    if k.d.overlays == Overlays::Docked && !st.overlays_hidden {
        at.x += rail_width(k, st, RailShape::Stacked);
    }
    crate::keymap::inputs(ui, k, st, at);
    speed_meter(ui, k, st, full);
    lens_meter(ui, k, st, full);
    ui.allocate_rect(full, Sense::hover());
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
            Vec2::new(
                palette_width(st.mode, 184.0),
                palette_height(k, st).min(full.bottom() - chip_room(k) - bar.bottom()),
            ),
        );
        drawer(ui, k, pal);
        palette(ui, k, st, pal);
        left = pal.right();
    }
    let strip = tool_strip(ui, k, st, Pos2::new(left + 8.0, bar.bottom() + 8.0), true);
    let _ = options_block(ui, k, st, strip, full.right() - 8.0);
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

/// The transform options, in a block right of the tool `strip` and up to
/// `limit`, while a transform tool is the active one. Gives back the
/// block when it shows.
fn options_block(ui: &mut Ui, k: &Look, st: &mut State, strip: Rect, limit: f32) -> Option<Rect> {
    if !crate::gizmo::has_options(st) {
        return None;
    }
    let m = 8.0;
    let (top, h) = (strip.top(), k.d.row + 6.0);
    let slot = ui.painter().add(egui::Shape::Noop);
    let area = Rect::from_min_max(Pos2::new(strip.right() + m + 4.0, top), Pos2::new(limit, top + h));
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(area)
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 2.0;
    crate::gizmo::options(&mut row, k, st);
    let options = Rect::from_min_max(
        Pos2::new(strip.right() + m, top),
        Pos2::new(row.min_rect().right() + 4.0, top + h),
    );
    set_block(ui, k, slot, options);
    Some(options)
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

    let options = options_block(ui, k, st, strip, view.left() - m);

    // The selection bar, centred when there is room for it.
    if selection && st.selection().is_some() {
        let id = ui.id().with("context-width");
        let known: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(420.0);
        let w = known + 16.0;
        let lo = options.map_or(strip.right(), |o| o.right()) + m;
        let hi = view.left() - m - w;
        // Without room in the top row, it takes a row of its own below.
        let rect = if hi >= lo {
            let x = (full.center().x - w / 2.0).clamp(lo, hi).round();
            Rect::from_min_size(Pos2::new(x, top), Vec2::new(w, h))
        } else {
            Rect::from_min_size(Pos2::new(strip.right() + m, top + h + m), Vec2::new(w, h))
        };
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
            (MODES.len() + LATER.len()) as f32 * rail_cell(k, st, shape) + LATER_GAP + 22.0,
        ),
    );
    block(ui, k, rail_rect);
    rail(ui, k, st, rail_rect, shape);
    if st.palette_open {
        let pal = Rect::from_min_size(
            Pos2::new(rail_rect.right() + 4.0, rail_rect.top()),
            Vec2::new(
                palette_width(st.mode, 188.0),
                palette_height(k, st).min(full.bottom() - chip_room(k) - rail_rect.top()),
            ),
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
            Vec2::new(
                palette_width(st.mode, 196.0),
                palette_height(k, st).min(full.bottom() - chip_room(k) - top),
            ),
        );
        drawer(ui, k, pal);
        palette(ui, k, st, pal);
        left = pal.right();
    }
    let strip = tool_strip(ui, k, st, Pos2::new(left + 8.0, top + 8.0), true);
    let _ = options_block(ui, k, st, strip, full.right() - 8.0);
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
    let color = st
        .doc
        .selection()
        .map_or(kind.color(), |o| st.library.color_of(&o.content));
    // A light or an effect says which it is, as in Properties.
    let (icon_of, label) = st
        .doc
        .selection()
        .map_or((kind.glyph(), kind.label()), |o| (o.content.glyph(), o.content.label()));
    let (rect, _) = ui.allocate_exact_size(Vec2::new(18.0, 20.0), Sense::hover());
    glyph(ui.painter(), k, rect.center(), icon_of, 16.0, color);
    ui.add_space(4.0);
    bar_text(ui, &name, k.bold(k.d.text), k.t.text_bright);
    if label != name {
        ui.add_space(5.0);
        bar_text(ui, label, k.font(), k.t.text_dim);
    }
    // A character's actions are in its Properties, in the Character
    // section.
    if kind == crate::kinds::Kind::Character {
        return;
    }
    ui.add_space(4.0);
    bar_divider(ui, k);
    ui.add_space(2.0);
    if kind == crate::kinds::Kind::Map {
        map_items(ui, k, st);
        return;
    }
    for (g, text, menu) in actions(kind) {
        use crate::kinds::Kind;
        // What nothing stands behind yet is shown greyed.
        let works = matches!(
            (kind, *text),
            (_, "Load card" | "Add")
                | (Kind::Item, "Attach to")
                | (Kind::Folder, "Hide all")
                | (Kind::Light, "Shadows" | "Type" | "Colour")
        );
        if !works {
            ui.add_enabled_ui(false, |ui| flat_button(ui, k, Some(g), text, *menu, false))
                .response
                .on_disabled_hover_text("Not available yet");
            continue;
        }
        let mut resp = flat_button(ui, k, Some(g), text, *menu, false);
        if *text == "Load card" {
            resp = resp.on_hover_text("Import card");
        }
        match (kind, *text) {
            (_, "Load card") if resp.clicked() => st.ask(Command::ImportCard, ui.ctx()),
            (Kind::Item, "Attach to") => {
                egui::Popup::menu(&resp).show(|ui| {
                    let chosen = st
                        .doc
                        .selected
                        .and_then(|id| crate::panels::attach_items(ui, k, st, id));
                    if let Some((to, bone)) = chosen {
                        st.run(Command::Attach(to, bone));
                    }
                });
            }
            // The same kinds the Outliner's own Add button offers.
            (_, "Add") => {
                egui::Popup::menu(&resp).show(|ui| crate::panels::add_items(ui, k, st));
            }
            (Kind::Folder, "Hide all") if resp.clicked() => hide_folder(st),
            (Kind::Light, "Shadows") => {
                egui::Popup::menu(&resp).show(|ui| shadows_menu(ui, k, st));
            }
            (Kind::Light, "Type") => {
                egui::Popup::menu(&resp).show(|ui| light_type_menu(ui, k, st));
            }
            (Kind::Light, "Colour") => {
                egui::Popup::menu(&resp)
                    .close_behavior(widgets::picker_closing(ui.ctx(), LIGHT_COLOUR.into()))
                    .show(|ui| light_colour(ui, k, st));
            }
            _ if *menu => {
                egui::Popup::menu(&resp).show(|ui| {
                    ui.label(egui::RichText::new("Not available yet").color(k.t.text_dim));
                });
            }
            _ => {}
        }
    }
}

/// Hides everything in the selected folder, or shows it all again once
/// none of it is left showing, as one step of the history.
fn hide_folder(st: &mut State) {
    let Some(folder) = st.doc.selected else { return };
    let inside = st.doc.scene().children(Some(folder));
    let showing = inside
        .iter()
        .any(|id| st.doc.scene().get(*id).is_some_and(|o| o.visible));
    st.doc.set_visible_many(&inside, !showing);
}

/// Whether the selected light casts shadows.
fn shadows_menu(ui: &mut Ui, k: &Look, st: &mut State) {
    use crate::scene::Content;
    let Some(mut object) = st.doc.selection().cloned() else {
        return;
    };
    let before = object.clone();
    let Content::Light { shadows, .. } = &mut object.content else {
        return;
    };
    for (text, cast) in [("Cast", true), ("None", false)] {
        if widgets::check_item(ui, k, *shadows == cast, text, "").clicked() {
            *shadows = cast;
            ui.close();
        }
    }
    if object != before {
        st.doc.edit(object, false);
    }
}

/// The kinds of light, for the selected light.
fn light_type_menu(ui: &mut Ui, k: &Look, st: &mut State) {
    use crate::scene::{Content, LightKind};
    let Some(mut object) = st.doc.selection().cloned() else {
        return;
    };
    let Content::Light { light, .. } = &mut object.content else {
        return;
    };
    let now = *light;
    for kind in LightKind::ALL {
        if widgets::check_item(ui, k, now == kind, kind.label(), "").clicked() {
            *light = kind;
            ui.close();
        }
    }
    if *light != now {
        st.doc.edit(object, false);
    }
}

const LIGHT_COLOUR: &str = "light colour";

/// The selected light's colour picker.
fn light_colour(ui: &mut Ui, k: &Look, st: &mut State) {
    use crate::scene::Content;
    let Some(mut object) = st.doc.selection().cloned() else {
        return;
    };
    let before = object.clone();
    let Content::Light { color, .. } = &mut object.content else {
        return;
    };
    widgets::color_picker(ui, k, LIGHT_COLOUR.into(), color);
    if object != before {
        let held = ui.input(|i| i.pointer.any_down());
        st.doc.edit(object, held);
    }
}

/// What the selection bar offers for the selected map: another map, and
/// its time of day.
fn map_items(ui: &mut Ui, k: &Look, st: &mut State) {
    use crate::scene::{Content, MapTime};
    let Some(mut object) = st.doc.selection().cloned() else {
        return;
    };
    let before = object.clone();
    let Content::Map { map, time, .. } = &mut object.content else {
        return;
    };
    let file = st.map_pictures.sidecar(&st.project.resolve(map));
    let times = file.as_ref().map_or_else(Vec::new, |f| f.times.clone());
    let change = flat_button(ui, k, Some(icon::SWAP), "Change map", false, false).clicked();
    let resp = flat_button(ui, k, Some(icon::SUN_HORIZON), "Time of day", true, false);
    egui::Popup::menu(&resp).show(|ui| {
        for when in MapTime::ALL {
            let has = times.contains(&when);
            let clicked = ui
                .add_enabled_ui(has, |ui| widgets::check_item(ui, k, *time == when, when.label(), ""))
                .inner
                .clicked();
            if clicked {
                *time = when;
                ui.close();
            }
        }
    });
    if object != before {
        st.doc.edit(object, false);
    }
    if change {
        st.open_map_library();
    }
}

/// The Clothing menu of the selected character: what it has on, and in
/// which state; a click on a piece cycles it, and the menu stays open.
fn clothing_menu(ui: &mut Ui, k: &Look, st: &mut State) {
    use crate::scene::Content;
    use rk_chara::clothes_state as cs;
    {
        ui.set_min_width(220.0);
        let Some(mut object) = st.doc.selection().cloned() else {
            return;
        };
        let worn = st
            .live
            .as_ref()
            .and_then(|live| live.built(object.id))
            .map(|built| built.worn);
        let Content::Character {
            clothes, outdoor_shoes, ..
        } = &mut object.content
        else {
            return;
        };
        let Some(worn) = worn else {
            ui.label(egui::RichText::new("The character is not built").color(k.t.text_dim));
            return;
        };
        let before = *clothes;
        for (text, state) in [("All on", cs::ON), ("All half off", cs::HALF), ("All off", cs::OFF)] {
            if widgets::menu_step(ui, k, "", text, "", true) {
                cs::set_all(clothes, &worn, state);
            }
        }
        ui.separator();
        // The pair of shoes that is worn stands for both.
        let shoes = if *outdoor_shoes {
            (cs::SHOES_OUTDOOR, "Outdoor shoes")
        } else {
            (cs::SHOES_INDOOR, "Indoor shoes")
        };
        let rows = [
            (cs::TOP, "Top"),
            (cs::BOTTOM, "Bottom"),
            (cs::BRA, "Bra"),
            (cs::SHORTS, "Underwear"),
            (cs::GLOVES, "Gloves"),
            (cs::PANTYHOSE, "Pantyhose"),
            (cs::SOCKS, "Socks"),
            shoes,
        ];
        let kinds = worn.kinds();
        let shown = cs::fitted(clothes, &worn);
        for (kind, name) in rows {
            let has = kinds.is_kind(kind);
            let state = if has {
                clothes_state_name(shown[kind])
            } else {
                "None worn"
            };
            // A click moves on to the next state the piece has, as
            // CharaStudio's button does; the menu stays open.
            if widgets::menu_step(ui, k, "", name, state, has) {
                *clothes = shown;
                cs::next_state(clothes, &worn, kind);
            }
        }
        if *clothes != before {
            st.doc.edit(object, false);
        }
    }
}

/// A menu of the tool palette, which opens beside the row rather than
/// under it so that it does not cover the rest of the tools.
fn beside(resp: &egui::Response) -> egui::Popup<'_> {
    egui::Popup::menu(resp)
        .align(egui::RectAlign::RIGHT_START)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
}

/// Which of the card's outfits the selected character wears. The names
/// are More Outfits' where the card carries them, and how many there are
/// is known only once the character is built.
fn outfit_menu(ui: &mut Ui, k: &Look, st: &mut State) {
    use crate::scene::Content;
    ui.set_min_width(180.0);
    let Some(mut object) = st.doc.selection().cloned() else {
        return;
    };
    let before = object.clone();
    let built = st.live.as_ref().and_then(|live| live.built(object.id));
    let (count, names) = built.map_or((1, Default::default()), |b| (b.outfits.max(1), b.outfit_names.clone()));
    let Content::Character { outfit, .. } = &mut object.content else {
        return;
    };
    for i in 0..count {
        let name = rk_chara::more_outfits::outfit_name(i, &names);
        if widgets::check_item(ui, k, *outfit == i, &name, "").clicked() {
            *outfit = i;
            ui.close();
        }
    }
    if object != before {
        st.doc.edit(object, false);
    }
}

/// Which pair of shoes the selected character wears. The game keeps a
/// state for each pair and shows the one the scene asks for.
fn shoes_menu(ui: &mut Ui, k: &Look, st: &mut State) {
    use crate::scene::Content;
    let Some(mut object) = st.doc.selection().cloned() else {
        return;
    };
    let before = object.clone();
    let Content::Character { outdoor_shoes, .. } = &mut object.content else {
        return;
    };
    for (text, outdoor) in [("Indoor shoes", false), ("Outdoor shoes", true)] {
        if widgets::check_item(ui, k, *outdoor_shoes == outdoor, text, "").clicked() {
            *outdoor_shoes = outdoor;
            ui.close();
        }
    }
    if object != before {
        st.doc.edit(object, false);
    }
}

/// What a state of a piece of clothing is called.
fn clothes_state_name(state: u8) -> &'static str {
    use rk_chara::clothes_state as cs;
    match state {
        cs::ON => "On",
        cs::HALF => "Half off",
        cs::HALF_2 => "Half off 2",
        _ => "Off",
    }
}

/// Camera, shading and the grid, then the two buttons that clear the view.
/// Laid out from the right.
fn view_items(ui: &mut Ui, k: &Look, st: &mut State) {
    let glyph = if st.maximised {
        icon::CORNERS_IN
    } else {
        icon::CORNERS_OUT
    };
    if icon_button(ui, k, glyph, st.maximised)
        .on_hover_text(st.keys.tip("Maximise viewport", Action::Maximise))
        .clicked()
    {
        st.maximised = !st.maximised;
    }
    if icon_button(ui, k, icon::EYE_SLASH, st.overlays_hidden)
        .on_hover_text(st.keys.tip("Viewport tools", Action::ViewportTools))
        .clicked()
    {
        st.overlays_hidden = !st.overlays_hidden;
    }
    if icon_button(ui, k, icon::KEYBOARD, st.settings.show_inputs)
        .on_hover_text(st.keys.tip("Show inputs", Action::ShowInputs))
        .clicked()
    {
        st.act(Action::ShowInputs, ui.ctx());
    }
    bar_divider(ui, k);
    crate::gizmo::grid_button(ui, k, st);
    let resp = flat_button(ui, k, Some(icon::SUN), "Lit", true, false);
    egui::Popup::menu(&resp).show(|ui| {
        if widgets::check_item(ui, k, true, "Lit", "").clicked() {
            ui.close();
        }
        // The other ways to draw wait for their own passes.
        for option in ["Unlit", "Wireframe"] {
            ui.add_enabled_ui(false, |ui| widgets::check_item(ui, k, false, option, ""))
                .response
                .on_disabled_hover_text("Not available yet");
        }
    });
    // The lens is perspective and only that; the other two turn the
    // camera to look in down an axis, which is what they are for.
    let resp = flat_button(ui, k, Some(icon::CUBE_FOCUS), "Perspective", true, false);
    egui::Popup::menu(&resp).show(|ui| {
        if widgets::check_item(ui, k, true, "Perspective", "").clicked() {
            ui.close();
        }
        for view in [View::Front, View::Side] {
            if widgets::check_item(ui, k, false, view.label(), "").clicked() {
                if let Some(live) = st.live.as_mut() {
                    live.cam.look_from(view);
                }
                ui.close();
            }
        }
    });
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

/// The room between the modes and the ones to come.
const LATER_GAP: f32 = 5.0;

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
            let tip = format!("{} mode", mode.name);
            resp = resp.on_hover_text(st.keys.tip(&tip, Action::Mode(i as u8)));
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

    // The modes to come, greyed, below a hairline.
    let top = rect.top() + MODES.len() as f32 * cell_h;
    hair_h(
        ui.painter(),
        rect.left() + 6.0,
        rect.right() - 6.0,
        top + (LATER_GAP / 2.0).floor(),
        Theme::glass(k.t.hover, 90),
    );
    for (i, (g, name)) in LATER.iter().enumerate() {
        let cell = Rect::from_min_size(
            Pos2::new(rect.left(), top + LATER_GAP + i as f32 * cell_h),
            Vec2::new(rect.width(), cell_h),
        );
        let tip = if named {
            "Not available yet".to_owned()
        } else {
            format!("{name} mode, not available yet")
        };
        ui.interact(cell, ui.id().with(("later", i)), Sense::hover())
            .on_hover_text(tip);
        let faint = Theme::glass(k.t.text_dim, 130);
        let p = ui.painter();
        match (shape, named) {
            (RailShape::Stacked, true) => {
                glyph(p, k, Pos2::new(cell.center().x, cell.top() + 20.0), g, 20.0, faint);
                p.text(
                    Pos2::new(cell.center().x, cell.top() + 39.0),
                    Align2::CENTER_CENTER,
                    *name,
                    k.small(),
                    faint,
                );
            }
            (RailShape::Stacked, false) => glyph(p, k, cell.center(), g, 20.0, faint),
            (RailShape::Rows, true) => {
                glyph(p, k, Pos2::new(cell.left() + 15.0, cell.center().y), g, 16.0, faint);
                widgets::label(p, k, Pos2::new(cell.left() + 30.0, cell.center().y), name, faint);
            }
            (RailShape::Rows, false) => glyph(p, k, cell.center(), g, 16.0, faint),
        }
    }

    let fold_h = if shape == RailShape::Stacked { 28.0 } else { 22.0 };
    let fold = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - fold_h), rect.max);
    let resp = ui
        .interact(fold, ui.id().with("rail-fold"), Sense::click())
        .on_hover_text(if named {
            "Hide the mode names"
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

/// Whether a mode's palette holds the map's controls rather than tools.
fn map_mode(mode: usize) -> bool {
    MODES.get(mode).is_some_and(|m| m.name == "Map")
}

/// The palette's width: the Map mode's rows need more room than tools,
/// and the Pose mode's categories a box at their end.
fn palette_width(mode: usize, tools: f32) -> f32 {
    if map_mode(mode) {
        crate::map_menu::WIDTH
    } else if MODES.get(mode).is_some_and(|m| m.name == "Pose") {
        POSE_PALETTE_WIDTH
    } else {
        tools
    }
}

/// The Pose palette's width.
const POSE_PALETTE_WIDTH: f32 = 220.0;

/// Height the palette needs for a mode's tools, with the FK parts under
/// Forward kinematics while the selected character's FK is on.
fn palette_height(k: &Look, st: &State) -> f32 {
    let mode = st.mode;
    if map_mode(mode) {
        return 34.0 + crate::map_menu::height(k);
    }
    let sections = MODES[mode].sections;
    let mut rows: usize = sections.iter().map(|(_, tools)| tools.len()).sum();
    if MODES[mode].name == "Pose" && st.fk_open && fk_layer(st).is_some() {
        rows += FK_PART_ROWS;
    }
    if MODES[mode].name == "Pose" && st.ik_open && fk_layer(st).is_some() {
        rows += IK_ROWS;
    }

    34.0 + sections.len() as f32 * 24.0 + rows as f32 * palette_row(k) + 8.0
}

/// The rows the FK parts take in the Pose palette, one each.
const FK_PART_ROWS: usize = rk_chara::bone_info::Part::ALL.len();
/// The rows of the IK category: its groups, and Optimize IK.
const IK_ROWS: usize = rk_chara::pose::IkGroup::ALL.len() + 1;

/// The pose layer of the selected character, when a character is selected.
fn fk_layer(st: &State) -> Option<rk_chara::pose::Layer> {
    match st.doc.selection().map(|o| &o.content) {
        Some(crate::scene::Content::Character { pose_layer, .. }) => {
            Some(pose_layer.as_deref().cloned().unwrap_or_default())
        }
        _ => None,
    }
}

/// Changes the selected character's pose layer as one step of the history.
pub fn edit_fk_layer(st: &mut State, change: impl FnOnce(&mut rk_chara::pose::Layer)) {
    edit_layer_held(st, false, change);
}

/// Edits the layer, as one step of the history for all of a drag while
/// `held`.
fn edit_layer_held(st: &mut State, held: bool, change: impl FnOnce(&mut rk_chara::pose::Layer)) {
    let Some(mut object) = st.doc.selection().cloned() else {
        return;
    };
    let crate::scene::Content::Character { pose_layer, .. } = &mut object.content else {
        return;
    };
    let layer = pose_layer.get_or_insert_with(Default::default);
    change(layer);
    if layer.is_empty() {
        *pose_layer = None;
    }
    st.doc.edit(object, held);
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
        .on_hover_text("Close");
    icon_cell(ui, k, close, &resp, icon::X, false);
    if resp.clicked() {
        st.palette_open = false;
    }
    let pin = close.translate(Vec2::new(-side - 2.0, 0.0));
    let resp = ui
        .interact(pin, ui.id().with("palette-pin"), Sense::click())
        .on_hover_text(if st.palette_pinned {
            "Unpin the tools"
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

    if map_mode(st.mode) {
        let body = Rect::from_min_max(Pos2::new(rect.left(), head.bottom()), rect.max);
        crate::map_menu::panel(ui, k, st, body);
        return;
    }
    // Rows that do not fit scroll under the header with the wheel; the
    // ones out of sight take no clicks.
    let body = Rect::from_min_max(Pos2::new(rect.left(), head.bottom()), rect.max);
    let overflow = (palette_height(k, st) - rect.height()).max(0.0);
    let scroll_id = ui.id().with(("palette scroll", st.mode));
    let mut scroll: f32 = ui.data(|d| d.get_temp(scroll_id)).unwrap_or(0.0);
    if ui.rect_contains_pointer(body) {
        scroll -= ui.input(|i| i.smooth_scroll_delta.y);
    }
    scroll = scroll.clamp(0.0, overflow);
    ui.data_mut(|d| d.insert_temp(scroll_id, scroll));
    let mut inner = ui.new_child(UiBuilder::new().max_rect(body));
    inner.set_clip_rect(body.intersect(ui.clip_rect()));
    let ui = &mut inner;
    let mut y = head.bottom() - scroll;
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
                if LATER_TOOLS.contains(name) {
                    // Shown, greyed, until they do something.
                    ui.interact(row, id, Sense::hover()).on_hover_text("Not available yet");
                    let faint = Theme::glass(k.t.text_dim, 150);
                    let p = ui.painter();
                    glyph(p, k, Pos2::new(row.left() + 16.0, row.center().y), g, 16.0, faint);
                    widgets::label(p, k, Pos2::new(row.left() + 32.0, row.center().y), name, faint);
                    y += palette_row(k);
                    index += 1;
                    continue;
                }
                if *name == "Forward kinematics" {
                    kinematics_category(ui, k, st, row, id, g, name, false);
                    y += palette_row(k);
                    index += 1;
                    if st.fk_open {
                        if let Some(layer) = fk_layer(st) {
                            fk_parts(
                                ui,
                                k,
                                st,
                                Rect::from_min_max(Pos2::new(rect.left(), y), rect.max),
                                &layer,
                            );
                            y += FK_PART_ROWS as f32 * palette_row(k);
                        }
                    }
                    continue;
                }
                if *name == "Inverse kinematics" {
                    kinematics_category(ui, k, st, row, id, g, name, true);
                    y += palette_row(k);
                    index += 1;
                    if st.ik_open {
                        if let Some(layer) = fk_layer(st) {
                            ik_rows(
                                ui,
                                k,
                                st,
                                Rect::from_min_max(Pos2::new(rect.left(), y), rect.max),
                                &layer,
                            );
                            y += IK_ROWS as f32 * palette_row(k);
                        }
                    }
                    continue;
                }
                if *name == "Expression" || *name == "Hands" {
                    // The values are the character's: they live in the
                    // Animation tab of Properties, which this opens.
                    let resp = widgets::tool_row(ui, k, row, id, g, name, false);
                    if resp.clicked() {
                        st.property_tab = 1;
                        let section = if *name == "Hands" {
                            crate::properties::HANDS_SECTION
                        } else {
                            crate::properties::FACE_SECTION
                        };
                        st.sections[section] = true;
                    }
                    y += palette_row(k);
                    index += 1;
                    continue;
                }
                if *name == "Mirror pose" || *name == "Copy to other side" {
                    // A copy takes the bone chosen and what hangs from it.
                    let copy = *name == "Copy to other side";
                    let ready = fk_layer(st).is_some_and(|l| l.fk_on) && (!copy || st.pose_bone.is_some());
                    let resp = widgets::tool_row(ui, k, row, id, g, name, false);
                    if resp.clicked() && ready {
                        mirror(st, copy);
                    }
                    y += palette_row(k);
                    index += 1;
                    continue;
                }
                if *name == "Set key" || *name == "Delete key" {
                    // The chosen bone, or every bone the pose has an edit
                    // or a key for, at the playhead.
                    let who = fk_layer(st).filter(|l| l.fk_on).and(st.doc.selected);
                    let bones = who.map(|w| crate::keying::bones_to_key(st, w)).unwrap_or_default();
                    let resp = widgets::tool_row(ui, k, row, id, g, name, false);
                    if let (true, Some(who), false) = (resp.clicked(), who, bones.is_empty()) {
                        if *name == "Set key" {
                            crate::keying::set_keys(st, who, &bones);
                        } else {
                            crate::keying::delete_keys(st, who, &bones);
                        }
                    }
                    y += palette_row(k);
                    index += 1;
                    continue;
                }
                if *name == "Bake animation" || *name == "Reset pose" {
                    let bake = *name == "Bake animation";
                    let ready = if bake {
                        animated(st)
                    } else {
                        fk_layer(st).is_some_and(|l| !l.fk.is_empty())
                    };
                    let resp = widgets::tool_row(ui, k, row, id, g, name, false);
                    if resp.clicked() && ready {
                        if bake {
                            bake_animation(st);
                        } else {
                            reset_bones(st, None);
                        }
                    }
                    y += palette_row(k);
                    index += 1;
                    continue;
                }
                // Only the Select mode's rows are states; the others act.
                let on = st.mode == 0 && st.mode_tool[st.mode] == index;
                let resp = widgets::tool_row(ui, k, row, id, g, name, on);
                if *name == "Load card" {
                    // The same as the selection bar's Load card.
                    if resp.on_hover_text("Import card").clicked() {
                        st.ask(Command::ImportCard, ui.ctx());
                    }
                } else if *name == "Clothing" {
                    // Opens the clothing of the selected character beside
                    // the palette, rather than becoming the mode's tool.
                    egui::Popup::menu(&resp)
                        .align(egui::RectAlign::RIGHT_START)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| clothing_menu(ui, k, st));
                } else if *name == "Outfit" {
                    beside(&resp).show(|ui| outfit_menu(ui, k, st));
                } else if *name == "Shoes" {
                    beside(&resp).show(|ui| shoes_menu(ui, k, st));
                } else if *name == "Save pose" {
                    if resp.clicked() && st.doc.selection().is_some_and(|o| o.kind() == Kind::Character) {
                        st.run(Command::SavePose);
                    }
                } else if *name == "Load pose" {
                    beside(&resp).show(|ui| pose_menu(ui, k, st));
                } else if *name == "Load clip" {
                    beside(&resp).show(|ui| {
                        if let Some(chosen) = crate::panels::animation_items(ui, k, st, st.doc.selected) {
                            st.run(Command::Animate(chosen));
                        }
                    });
                } else if *name == "Add light" {
                    if resp.clicked() {
                        st.run(Command::Add(crate::kinds::Kind::Light));
                    }
                } else if resp.clicked() {
                    palette_tool(st, name, index);
                }
            }
            y += palette_row(k);
            index += 1;
        }
    }
}

/// A category of the Pose palette that is switched on and opened: an
/// arrow that opens or closes it, its name, and a box that switches it for
/// the selected character. Switching it on opens it, off closes it.
#[allow(clippy::too_many_arguments)]
fn kinematics_category(ui: &mut Ui, k: &Look, st: &mut State, row: Rect, id: egui::Id, g: &str, name: &str, ik: bool) {
    let layer = fk_layer(st);
    let on = layer.as_ref().is_some_and(|l| if ik { l.ik_on } else { l.fk_on });
    let open = if ik { st.ik_open } else { st.fk_open };
    let resp = ui.interact(row, id, Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(row, 2.0, k.t.control);
    }
    let y = row.center().y;
    let arrow = if open { icon::CARET_DOWN } else { icon::CARET_RIGHT };
    glyph(p, k, Pos2::new(row.left() + 9.0, y), arrow, 10.0, k.t.text_dim);
    let tint = if on { k.t.text_bright } else { k.t.text };
    glyph(p, k, Pos2::new(row.left() + 24.0, y), g, 16.0, tint);
    widgets::label(p, k, Pos2::new(row.left() + 38.0, y), name, tint);
    let side = 14.0;
    let box_rect = Rect::from_min_size(
        Pos2::new(row.right() - side - 8.0, row.top()),
        Vec2::new(side + 8.0, row.height()),
    );
    let mut ticked = on;
    if layer.is_some() {
        widgets::checkbox(ui, k, box_rect, id.with("on"), &mut ticked, "");
    }
    if ticked != on && ik {
        // The targets start on the bones as the character stands now, as
        // Studio's copy puts them, every time IK goes on: Studio keeps the
        // targets it had, which snaps the limbs back to an old pose, often
        // the T-pose the character was added in.
        let on_bones = st.doc.selected.and_then(|id| st.live.as_ref()?.ik_targets_on_bones(id));
        edit_fk_layer(st, |l| {
            l.ik_on = ticked;
            if let (true, Some(targets)) = (ticked, on_bones) {
                l.ik = targets;
            }
        });
        st.ik_open = ticked;
    } else if ticked != on {
        edit_fk_layer(st, |l| l.fk_on = ticked);
        st.fk_open = ticked;
        st.show_animator |= ticked;
    } else if resp.clicked() && ik {
        st.ik_open = !st.ik_open;
    } else if resp.clicked() {
        st.fk_open = !st.fk_open;
    }
}

/// The IK groups as boxes to tick, one to a row, and Optimize IK, under
/// the category's name.
fn ik_rows(ui: &mut Ui, k: &Look, st: &mut State, area: Rect, layer: &rk_chara::pose::Layer) {
    let cell = |i: usize| {
        Rect::from_min_size(
            Pos2::new(area.left() + 38.0, area.top() + i as f32 * palette_row(k)),
            Vec2::new(area.width() - 50.0, palette_row(k)),
        )
    };
    for (i, group) in rk_chara::pose::IkGroup::ALL.into_iter().enumerate() {
        let mut on = layer.ik_groups.contains(&group);
        let was = on;
        widgets::checkbox(
            ui,
            k,
            cell(i),
            ui.id().with(("ik group", group.label())),
            &mut on,
            group.label(),
        );
        if on != was {
            edit_fk_layer(st, |l| {
                if on {
                    l.ik_groups.push(group);
                } else {
                    l.ik_groups.retain(|g| *g != group);
                }
            });
        }
    }
    let mut optimize = layer.optimize_ik;
    widgets::checkbox(
        ui,
        k,
        cell(rk_chara::pose::IkGroup::ALL.len()),
        ui.id().with("optimize ik"),
        &mut optimize,
        "Optimize IK",
    );
    if optimize != layer.optimize_ik {
        edit_fk_layer(st, |l| l.optimize_ik = optimize);
    }
}

/// Mirrors the selected character's pose by hand, or with `copy` copies
/// the chosen bone and the bones below it to the other side, as one step
/// of the history.
pub fn mirror(st: &mut State, copy: bool) {
    let Some(id) = st.doc.selected else { return };
    let mut edits: Vec<(String, rk_chara::pose::BoneEdit)> = Vec::new();
    // A chosen IK target copies onto its twin; the whole pose mirrors its
    // targets with its bones.
    let targets: Vec<usize> = match (copy, st.pose_ik) {
        (true, Some(target)) => vec![target],
        (true, None) => Vec::new(),
        (false, _) => (0..rk_chara::pose::IK_TARGETS).collect(),
    };
    if let Some(found) = st.live.as_ref().and_then(|l| l.ik_mirror(id, &targets)) {
        edits.extend(found.into_iter().map(|(i, t)| (rk_chara::pose::ik_key(i), t.as_edit())));
    }
    if !(copy && st.pose_ik.is_some()) {
        let bone = st.pose_bone.clone().filter(|_| copy);
        if let Some(found) = st.live.as_ref().and_then(|l| l.mirror_edits(id, bone.as_deref())) {
            for (name, rot) in found {
                let edit = rk_chara::pose::BoneEdit {
                    rot,
                    ..crate::keying::shown(st, id, &name)
                };
                edits.push((name, edit));
            }
        }
    }
    // A mirrored pose swaps the hands' patterns too.
    let hands = fk_layer(st).map_or([0, 0], |l| l.hands);
    let swap = !copy && hands[0] != hands[1];
    if !edits.is_empty() || swap {
        crate::keying::write_many(st, id, edits, |content| {
            if let crate::scene::Content::Character {
                pose_layer: Some(layer),
                ..
            } = content
            {
                if swap {
                    layer.hands = [hands[1], hands[0]];
                }
            }
        });
    }
}

/// The project's poses, to give the selected character one.
fn pose_menu(ui: &mut Ui, k: &Look, st: &mut State) {
    let character = st.doc.selection().is_some_and(|o| o.kind() == Kind::Character);
    let poses = crate::poses::scan(&st.project);
    if poses.is_empty() {
        widgets::menu_item(ui, k, Kind::Pose.glyph(), "No poses", "", false);
    }
    for rel in poses {
        let name = crate::content::stem(&rel).to_owned();
        if widgets::menu_item(ui, k, Kind::Pose.glyph(), &name, "", character) {
            st.run(Command::LoadPose(rel));
            ui.close();
        }
    }
}

/// Whether the selected character plays an animation.
fn animated(st: &State) -> bool {
    matches!(
        st.doc.selection().map(|o| &o.content),
        Some(crate::scene::Content::Character { animation: Some(_), .. })
    )
}

/// Bakes the selected character's animation into its pose by hand: the
/// pose of the frame is held over the bones as loaded, the animation is
/// taken off and FK switched on with the parts the pose turns, as one step
/// of the history.
pub fn bake_animation(st: &mut State) {
    let Some(id) = st.doc.selected else { return };
    let Some((found, parts)) = st.live.as_mut().and_then(|l| l.baked_edits(id)) else {
        return;
    };
    // The animation goes and FK comes on with the parts the pose turns; the
    // edits go where each bone's go, its keys or the pose layer.
    crate::keying::write_many(st, id, found, |content| {
        if let crate::scene::Content::Character {
            animation, pose_layer, ..
        } = content
        {
            *animation = None;
            let layer = pose_layer.get_or_insert_with(Default::default);
            layer.fk_on = true;
            for part in parts {
                if !layer.fk_parts.contains(&part) {
                    layer.fk_parts.push(part);
                }
            }
        }
    });
}

/// Puts the selected character's bones back as one step: those of
/// `bones`, or all of them. A keyed bone takes a key of no turn at the
/// playhead; the others lose their edit.
fn reset_bones(st: &mut State, bones: Option<&[String]>) {
    let Some(id) = st.doc.selected else { return };
    let fits = |name: &String| bones.is_none_or(|b| b.contains(name));
    let mut names: Vec<String> = fk_layer(st).map(|l| l.fk.keys().cloned().collect()).unwrap_or_default();
    names.extend(crate::keying::keyed_bones(st, id));
    names.retain(fits);
    names.sort();
    names.dedup();
    let edits = names
        .into_iter()
        .map(|n| (n, rk_chara::pose::BoneEdit::default()))
        .collect();
    crate::keying::write_many(st, id, edits, |_| {});
}

/// The FK parts as boxes to tick, one to a row, under the category's
/// name.
fn fk_parts(ui: &mut Ui, k: &Look, st: &mut State, area: Rect, layer: &rk_chara::pose::Layer) {
    for (i, part) in rk_chara::bone_info::Part::ALL.into_iter().enumerate() {
        let cell = Rect::from_min_size(
            Pos2::new(area.left() + 38.0, area.top() + i as f32 * palette_row(k)),
            Vec2::new(area.width() - 50.0, palette_row(k)),
        );
        let mut on = layer.fk_parts.contains(&part);
        let was = on;
        widgets::checkbox(
            ui,
            k,
            cell,
            ui.id().with(("fk part", part.label())),
            &mut on,
            part.label(),
        );
        if on != was {
            edit_fk_layer(st, |l| {
                if on {
                    l.fk_parts.push(part);
                } else {
                    l.fk_parts.retain(|p| *p != part);
                }
            });
        }
        // A part with bones turned can be put back.
        let Some(id) = st.doc.selected else { continue };
        let bones = st.live.as_ref().map(|l| l.part_bones(id, part)).unwrap_or_default();
        if !bones.iter().any(|b| layer.fk.contains_key(b)) {
            continue;
        }
        let side = palette_row(k) - 6.0;
        let button = Rect::from_center_size(
            Pos2::new(area.right() - 8.0 - side / 2.0, cell.center().y),
            Vec2::splat(side),
        );
        let resp = ui
            .interact(button, ui.id().with(("fk part reset", part.label())), Sense::click())
            .on_hover_text("Reset");
        if resp.hovered() {
            ui.painter().rect_filled(button, k.radius(), k.t.hover);
        }
        let color = if resp.hovered() { k.t.text_bright } else { k.t.text_dim };
        glyph(
            ui.painter(),
            k,
            button.center(),
            icon::ARROW_COUNTER_CLOCKWISE,
            14.0,
            color,
        );
        if resp.clicked() {
            reset_bones(st, Some(&bones));
        }
    }
}

/// Tools of the modes that do nothing yet.
const LATER_TOOLS: [&str; 15] = [
    "Replace",
    "Reload",
    "Accessories",
    "Gaze",
    "Look at camera",
    "Depth of field",
    "Auto key",
    "Bake physics",
    "New shot from view",
    "Shot list",
    "Still",
    "Video",
    "Queue",
    "Export",
    "Sync",
];

/// The strip's Pose tool, which waits for posing by hand.
pub const POSE_TOOL: usize = 5;

/// What a row of the palette does that is not a menu: the Select mode's
/// ways of picking and its filters, and the Light mode's lights and
/// effects.
fn palette_tool(st: &mut State, name: &str, index: usize) {
    use crate::kinds::Kind;
    use crate::scene::{Content, Effect, LightKind};
    let select_tool = 1;
    match name {
        "Objects" | "Box select" => {
            st.tool = select_tool;
            st.pick_filter = None;
            st.mode_tool[st.mode] = index;
        }
        "All of this type" => {
            if let Some(kind) = st.doc.selection().map(|o| o.kind()) {
                let all: Vec<_> = st
                    .doc
                    .scene()
                    .objects()
                    .iter()
                    .filter(|o| o.kind() == kind)
                    .map(|o| o.id)
                    .collect();
                st.doc.select_many(&all);
            }
        }
        "Characters" | "Items" | "Lights and cameras" => {
            let kind = match name {
                "Characters" => Kind::Character,
                "Items" => Kind::Item,
                _ => Kind::Light,
            };
            st.tool = select_tool;
            if st.pick_filter == Some(kind) {
                st.pick_filter = None;
                st.mode_tool[st.mode] = 0;
            } else {
                st.pick_filter = Some(kind);
                st.mode_tool[st.mode] = index;
            }
        }
        "Key light" => {
            let found = st.doc.scene().key_light().map(|o| o.id);
            match found {
                Some(id) => st.doc.select_many(&[id]),
                None => st.run(Command::AddContent(Content::light_of(LightKind::Character))),
            }
        }
        "Ambient occlusion" | "Bloom" => {
            let found = st
                .doc
                .scene()
                .objects()
                .iter()
                .find(|o| matches!(&o.content, Content::Effect { effect } if effect.label() == name))
                .map(|o| o.id);
            match found {
                Some(id) => st.doc.select_many(&[id]),
                None => {
                    let effect = Effect::all().into_iter().find(|e| e.label() == name);
                    if let Some(effect) = effect {
                        st.run(Command::AddContent(Content::Effect { effect }));
                    }
                }
            }
        }
        _ => st.mode_tool[st.mode] = index,
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
pub fn focus_selection(st: &mut State) {
    let Some(focus) = st.doc.selected.and_then(|id| subject(st, id)) else {
        return;
    };
    if let Some(live) = st.live.as_mut() {
        live.cam.focus_on(focus);
    }
}

/// Frames everything in the scene but maps, focusing on nothing.
pub fn frame_all(st: &mut State) {
    let scene = st.doc.scene();
    let all: Vec<Focus> = scene
        .objects()
        .iter()
        .filter(|o| scene.shown(o.id))
        .filter(|o| {
            !matches!(
                o.kind(),
                Kind::Map | Kind::Folder | Kind::Scene | Kind::Animation | Kind::Pose
            )
        })
        .filter_map(|o| subject(st, o.id))
        .collect();
    let Some(live) = st.live.as_mut() else { return };
    if let Some(group) = enclose(&all, crate::camera::FRAMING_LENS) {
        live.cam.frame(group.center, group.distance);
    }
}

/// How far from an object with no picture of its own (an item, a light,
/// a camera) the camera stands to frame it, at scale 1.
const PLAIN_DISTANCE: f32 = 1.5;

/// An object as something to focus on. A character is framed whole, a
/// folder with everything in it, an animation or a pose by its character,
/// anything else around its origin. Maps are not focused on: they are
/// what the scene stands in.
pub fn subject(st: &State, id: ObjectId) -> Option<Focus> {
    let live = st.live.as_ref()?;
    let scene = st.doc.scene();
    let object = scene.get(id)?;
    match object.kind() {
        Kind::Map => None,
        Kind::Character => live.subject(id),
        Kind::Animation | Kind::Pose => object.parent.and_then(|parent| subject(st, parent)),
        Kind::Folder | Kind::Scene => {
            let inside: Vec<Focus> = scene
                .children(Some(id))
                .into_iter()
                .filter_map(|c| subject(st, c))
                .collect();
            let mut group = enclose(&inside, crate::camera::FRAMING_LENS)?;
            group.object = id;
            Some(group)
        }
        _ => {
            let world = scene.world(id);
            let (scale, ..) = world.to_scale_rotation_translation();
            Some(Focus {
                object: id,
                center: world.w_axis.truncate(),
                distance: PLAIN_DISTANCE * scale.abs().max_element().max(1e-3),
            })
        }
    }
}

/// One framing that holds several, seen as balls that fit a view of
/// `fov_y_deg`.
fn enclose(all: &[Focus], fov_y_deg: f32) -> Option<Focus> {
    let first = all.first()?;
    let half = (fov_y_deg.to_radians() * 0.5).sin().max(1e-3);
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for f in all {
        let r = f.distance * half;
        min = min.min(f.center - r);
        max = max.max(f.center + r);
    }
    let center = (min + max) * 0.5;
    let radius = all
        .iter()
        .map(|f| f.center.distance(center) + f.distance * half)
        .fold(0.0, f32::max);
    Some(Focus {
        object: first.object,
        center,
        distance: radius / half,
    })
}

/// The camera's corner of the viewport, at the bottom left: what the
/// camera is focused on, and, once the view has left its starting framing,
/// the button that goes back to it. `corner` is the bottom left point.
/// The height the camera's corner takes at the viewport's bottom, which
/// the palette stays above; it scrolls when its rows do not fit.
fn chip_room(k: &Look) -> f32 {
    k.d.row + 6.0 + 16.0
}

fn camera_corner(ui: &mut Ui, k: &Look, st: &mut State, corner: Pos2) {
    let Some(live) = st.live.as_ref() else { return };
    let m = 8.0;
    let h = k.d.row + 6.0;
    let focus = live
        .cam
        .focus
        .and_then(|f| st.doc.scene().get(f.object))
        .map(|o| (o.kind(), st.library.color_of(&o.content), o.name.clone()));
    let moved = live.cam.moved();
    let name = match &focus {
        Some((_, _, name)) => name.as_str(),
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

    let can_focus = st.doc.selected.and_then(|id| subject(st, id));
    let tip = match (&focus, can_focus) {
        (Some((kind, _, name)), _) => format!("Focused on the {} {name}", kind.label().to_lowercase()),
        (None, Some(_)) => st.keys.tip("Focus on the selection", Action::Focus),
        (None, None) => "Free camera".to_owned(),
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
        Some((kind, color, name)) => {
            glyph(
                p,
                k,
                Pos2::new(cell.left() + 16.0, y),
                icon::GPS_FIX,
                16.0,
                k.t.select_bright,
            );
            glyph(p, k, Pos2::new(cell.left() + 39.0, y), kind.glyph(), 15.0, *color);
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
            .on_hover_text(st.keys.tip("Reset view", Action::ResetView));
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
/// The lens in use, as it was just widened or narrowed with Alt and the
/// wheel: its field of view on a track from the narrowest to the widest,
/// fading away as the speed's does.
fn lens_meter(ui: &Ui, k: &Look, st: &mut State, full: Rect) {
    let (Some(shown), Some(live)) = (st.lens_shown, st.live.as_ref()) else {
        return;
    };
    let age = ui.input(|i| i.time) - shown;
    if age >= METER_HOLD + METER_FADE {
        st.lens_shown = None;
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
        icon::APERTURE,
        16.0,
        fade(k.t.text),
    );
    let track = Rect::from_min_max(
        Pos2::new(rect.left() + 32.0, y - 1.5),
        Pos2::new(rect.right() - 52.0, y + 1.5),
    );
    p.rect_filled(track, 1.0, fade(k.t.control));
    let (lo, hi) = crate::camera::LENS_RANGE;
    let lens = live.cam.lenses[live.cam.lens_in_use()];
    let at = |deg: f32| track.left() + track.width() * ((deg - lo) / (hi - lo)).clamp(0.0, 1.0);
    p.rect_filled(
        Rect::from_min_max(track.min, Pos2::new(at(lens), track.bottom())),
        1.0,
        fade(k.t.select_bright),
    );
    // A notch at the lens it started as.
    let first = crate::camera::LENSES[live.cam.lens_in_use()];
    hair_v(p, at(first).round(), y - 4.0, y + 4.0, fade(k.t.text_dim));
    p.text(
        Pos2::new(rect.right() - 10.0, y),
        Align2::RIGHT_CENTER,
        format!("{lens:.0}°"),
        k.bold(k.d.text),
        fade(k.t.text_bright),
    );
}

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
        // The tool's key; the View tool is also held with Alt.
        let tip = match st.keys.text(Action::Tool(i as u8)) {
            keys if keys.is_empty() && key.is_empty() => (*name).to_owned(),
            keys if keys.is_empty() => format!("{name} ({key})"),
            keys => format!("{name} ({keys})"),
        };
        let resp = ui
            .interact(r, ui.id().with(("tool-strip", i)), Sense::click())
            .on_hover_text(tip);
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
    st.settle_on_map();
    st.clear_start_view();
    st.list_map_pieces();
    let still = st.physics_still();
    let Some(live) = st.live.as_mut() else { return };
    live.hold_physics_still(&still);
    let dt = ui.input(|i| i.stable_dt).min(1.0 / 30.0);
    // The scene's time runs while it plays and starts over on a stop.
    match st.playback {
        crate::state::Playback::Playing => st.clock += dt,
        crate::state::Playback::Paused => {}
        crate::state::Playback::Stopped => st.clock = 0.0,
    }
    // A character playing alone loops her keys on a time of her own.
    if st.solo.is_some() {
        let length = st.doc.scene().sequence.length.max(0.01);
        st.solo_time = (st.solo_time + dt).rem_euclid(length);
    }
    // A sequence with keys plays to its end, then starts over or holds.
    let sequence = &st.doc.scene().sequence;
    if st.playback == crate::state::Playback::Playing && !sequence.tracks.is_empty() && st.clock > sequence.length {
        if st.loop_sequence {
            st.clock = st.clock.rem_euclid(sequence.length.max(0.01));
        } else {
            st.clock = sequence.length;
            st.playback = crate::state::Playback::Paused;
        }
    }
    live.follow(
        st.doc.scene(),
        &st.project,
        &mut st.games,
        &mut st.jobs,
        crate::live::Clock {
            was: st.last_playback,
            now: st.playback,
            dt,
            time: st.clock,
            solo: st.solo.map(|id| (id, st.solo_time)),
        },
        &mut st.log,
    );
    st.doc.set_poses(live.poses().clone());
    if st.playback == crate::state::Playback::Playing {
        ui.ctx().request_repaint();
    }
    // What was just added gets the camera, once there is something of it
    // to look at: a character when it is built, anything else where it is.
    if let Some(id) = st.focus_pending {
        match (st.doc.scene().get(id), live.subject(id)) {
            (None, _) => st.focus_pending = None,
            (Some(_), Some(subject)) => {
                live.cam.focus_on(subject);
                st.focus_pending = None;
            }
            (Some(object), None) => {
                if object.kind() != crate::kinds::Kind::Character {
                    if object.kind() != crate::kinds::Kind::Folder {
                        live.cam.look_at(object.transform.position.into());
                    }
                    st.focus_pending = None;
                } else if live.failure(id).is_some() {
                    st.focus_pending = None;
                }
            }
        }
    }
    // A view focused on anything but a character (which follows its own
    // moves) goes with it as it moves.
    let hold = live.hold_camera;
    let focused = live.cam.focus.map(|f| f.object);
    let now = focused
        .filter(|id| st.doc.scene().get(*id).is_some_and(|o| o.kind() != Kind::Character))
        .and_then(|id| subject(st, id));
    let Some(live) = st.live.as_mut() else { return };
    if let Some(now) = now {
        live.cam.follow(now, hold);
    }
    if live.cam.advance(dt) {
        ui.ctx().request_repaint();
    }
    // The lens of the moment, eased to: wide while moving, the framing
    // lens on what is focused.
    live.cam.lenses = st.settings.lenses;
    let before = live.cam.orbit.fov_y_deg;
    live.cam.ease_lens(dt);
    if live.cam.orbit.fov_y_deg != before {
        ui.ctx().request_repaint();
    }
    st.last_playback = st.playback;
    let ppp = ui.ctx().pixels_per_point();
    let size = [
        (full.width() * ppp).round() as u32,
        (full.height() * ppp).round() as u32,
    ];
    // The grid is drawn behind what this picture shows.
    live.keep_depth = st.gizmo.grid;
    live.map_motion = st.settings.map_motion;
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
    let keys = st.keys.clone();
    // The View tool moves the view with the left button alone, as Unity's
    // hand does; with Alt it turns around the scene.
    let view_tool = st.tool == 0;
    let Some(live) = st.live.as_mut() else { return };
    let cam = &mut live.cam;
    let (alt, shift, right_down) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift, i.pointer.secondary_down()));
    let delta = scene.drag_delta();
    let right = right_down && (scene.dragged_by(Secondary) || scene.is_pointer_button_down_on());
    let nav = if right && alt {
        cam.cancel_glide();
        cam.orbit.zoom((delta.x - delta.y) * 2.0);
        Some(Nav::Zoom)
    } else if right {
        let typing = ui.ctx().egui_wants_keyboard_input();
        let held = |fly: u8| {
            if !typing && ui.input(|i| crate::keymap::held(&keys, Action::Fly(fly), i)) {
                1.0
            } else {
                0.0
            }
        };
        let along = glam::Vec3::new(held(3) - held(2), held(5) - held(4), held(0) - held(1));
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
    } else if scene.dragged_by(Middle) || (view_tool && scene.dragged_by(Primary)) {
        cam.cancel_glide();
        cam.orbit.pan(delta, full.height());
        Some(Nav::Pan)
    } else {
        None
    };
    if !right_down {
        cam.right_released();
    }
    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
    if scroll != 0.0 && alt && scene.hovered() {
        // Alt and the wheel widen or narrow the lens in use.
        cam.lenses = st.settings.lenses;
        cam.scroll_lens(scroll);
        st.settings.lenses = cam.lenses;
        st.settings_changed = true;
        st.lens_shown = Some(ui.input(|i| i.time));
        st.speed_shown = None;
    } else if scroll != 0.0 && right {
        cam.scroll_speed(scroll);
        st.speed_shown = Some(ui.input(|i| i.time));
    } else if scroll != 0.0 && scene.hovered() {
        cam.cancel_glide();
        cam.orbit.zoom(scroll);
    }
    if let Some(nav) = nav {
        nav_cursor(ui, nav);
    }
}

/// The map's sun turned as in Unreal: with its shortcut (Ctrl+L) held,
/// moving the mouse sideways turns its heading, up and down raises and
/// lowers it. One hold is one step of the history.
fn turn_sun(ui: &Ui, st: &mut State, scene: &egui::Response) {
    let typing = ui.ctx().egui_wants_keyboard_input();
    let held = !typing && ui.input(|i| crate::keymap::held_chord(&st.keys, Action::TurnSun, i));
    if !held || (!st.sun_turning && !scene.hovered()) {
        st.sun_turning = false;
        return;
    }
    let map = st
        .doc
        .scene()
        .objects()
        .iter()
        .find(|o| matches!(o.content, crate::scene::Content::Map { .. }))
        .map(|o| o.id);
    let Some(mut object) = map.and_then(|id| st.doc.scene().get(id).cloned()) else {
        return;
    };
    st.sun_turning = true;
    let delta = ui.input(|i| i.pointer.delta());
    if delta == Vec2::ZERO {
        return;
    }
    let own = crate::map_menu::own_sun(st);
    if let crate::scene::Content::Map { sun, .. } = &mut object.content {
        let [heading, elevation] = sun.unwrap_or(own);
        *sun = Some([
            (heading + delta.x * 0.4).rem_euclid(360.0),
            (elevation - delta.y * 0.4).clamp(-90.0, 90.0),
        ]);
    }
    st.doc.edit(object, true);
    ui.ctx().request_repaint();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_selects_by_kind_and_finds_its_effects() {
        use crate::scene::{Content, Effect};
        let mut st = State::new(crate::project::Project::none(), crate::scene::Scene::new());
        let first = st.doc.add("Fill", Content::light());
        st.doc.add("Rim", Content::light());
        // Every light, from one.
        st.doc.select_many(&[first]);
        palette_tool(&mut st, "All of this type", 2);
        assert_eq!(st.doc.selected_all().len(), 3);
        // A filter picks the Select tool; again, it is off.
        palette_tool(&mut st, "Characters", 3);
        assert_eq!((st.tool, st.pick_filter), (1, Some(crate::kinds::Kind::Character)));
        palette_tool(&mut st, "Characters", 3);
        assert_eq!(st.pick_filter, None);
        // Bloom is found where the new scene keeps it.
        palette_tool(&mut st, "Bloom", 1);
        let chosen = st.doc.selection().map(|o| o.content.clone());
        assert!(matches!(
            chosen,
            Some(Content::Effect {
                effect: Effect::Bloom { .. }
            })
        ));
    }

    #[test]
    fn a_group_frames_everything_in_it() {
        let ball = |x: f32, distance: f32| Focus {
            object: ObjectId(1),
            center: Vec3::new(x, 1.0, 0.0),
            distance,
        };
        let group = enclose(&[ball(-2.0, 1.5), ball(2.0, 1.5)], 30.0).expect("two balls");
        assert!((group.center - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-4);
        // Farther back than either one alone, so both fit.
        assert!(group.distance > 1.5 + 2.0 / 15f32.to_radians().sin() - 1e-3);
        assert!(enclose(&[], 30.0).is_none());
    }
}
