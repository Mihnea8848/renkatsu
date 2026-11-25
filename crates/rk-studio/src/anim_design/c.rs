// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Design C, the time slider and the graph: a thin strip under the
//! viewport that always shows, with the transport, the frames with the
//! chosen keys, the loop range, the markers and auto-key; and the
//! Animator as the list of bones beside a large graph editor, a row of
//! the chosen keys over the curves.

use super::a::{body, Beside, Body};
use super::time::{self, RULER};
use super::{sheet, Cells, Kind};
use crate::panels::toolbar;
use crate::scene::Content;
use crate::state::{Look, State};
use crate::widgets::{self, glyph, hair_h};
use egui::{Pos2, Rect, Sense, Ui, UiBuilder, Vec2};

/// The strip's height.
pub const SLIDER: f32 = 52.0;

/// A row of controls in a part of the strip.
fn bar(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) -> f32 {
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 4.0;
    add(&mut row);
    row.min_rect().right()
}

/// Draws the time slider in `rect`, under the viewport.
pub fn slider(ui: &mut Ui, k: &Look, st: &mut State, rect: Rect) {
    let t = &k.t;
    ui.painter().rect_filled(rect, 0.0, t.panel);
    hair_h(ui.painter(), rect.left(), rect.right(), rect.top(), t.divider);
    let inner = rect.shrink2(Vec2::new(8.0, 0.0));
    let left = bar(ui, inner, |ui| {
        time::transport(ui, k, st);
        time::frame_field(ui, k, st);
    });
    let tools = if st.anim.range_on { 268.0 } else { 180.0 } + if st.anim.onion { 38.0 } else { 0.0 };
    let right = Rect::from_min_max(Pos2::new(inner.right() - tools, inner.top()), inner.max);
    bar(ui, right, |ui| {
        time::switches(ui, k, st);
        time::gap(ui, k);
        time::range_and_marker(ui, k, st);
    });
    // The frames: the ruler over the chosen bone's keys, or the selected
    // character's.
    let frames = Rect::from_min_max(
        Pos2::new(left + 8.0, rect.top() + 5.0),
        Pos2::new(right.left() - 8.0, rect.bottom() - 5.0),
    );
    if frames.width() < 80.0 {
        return;
    }
    widgets::boxed(ui.painter(), frames.expand(1.0), k.radius(), t.input, t.border);
    let ruler = Rect::from_min_max(frames.min, Pos2::new(frames.right(), frames.top() + RULER - 4.0));
    let lane = Rect::from_min_max(Pos2::new(frames.left(), ruler.bottom()), frames.max);
    let span = time::span(ui, st, frames);
    let id = ui.id().with("time slider");
    time::ruler(ui, k, st, &span, ruler, id.with("ruler"));
    let rows = super::rows(st, false);
    let chosen = super::chosen(st);
    let object = st.doc.selected;
    let row = rows
        .iter()
        .find(|r| Some(r.object) == object && r.kind == Kind::Channel && Some(&r.name) == chosen.as_ref())
        .filter(|r| !r.marks.is_empty())
        .or_else(|| rows.iter().find(|r| Some(r.object) == object && r.kind == Kind::Object))
        .or_else(|| rows.first());
    if let Some(row) = row {
        // The lane is drawn as a bone's, without an object's tint.
        let plain = super::Row {
            kind: Kind::Channel,
            object: row.object,
            name: row.name.clone(),
            label: String::new(),
            glyph: "",
            tint: None,
            depth: 0,
            open: None,
            marks: row.marks.clone(),
        };
        let marks = sheet::lanes(ui, k, st, &span, &[(lane, &plain)], 5.0);
        sheet::interact(ui, k, st, &span, lane, id.with("keys"), &marks, 5.0);
    } else {
        ui.interact(lane, id.with("keys"), Sense::hover());
    }
    time::overlay(ui, k, st, &span, ruler, frames.bottom());
}

/// Draws the Animator panel: the list beside the graph editor.
pub fn animator(ui: &mut Ui, k: &Look, st: &mut State) {
    let t = &k.t;
    let character = st
        .doc
        .selection()
        .filter(|o| matches!(o.content, Content::Character { .. }))
        .map(|o| o.name.clone());
    toolbar(ui, k, |ui| {
        if let Some(name) = &character {
            let w = widgets::text_width(ui, name, k.font()) + 30.0;
            let (rect, _) = ui.allocate_exact_size(Vec2::new(w, k.field()), Sense::hover());
            let kind = crate::kinds::Kind::Character;
            glyph(
                ui.painter(),
                k,
                Pos2::new(rect.left() + 8.0, rect.center().y),
                kind.glyph(),
                14.0,
                kind.color(),
            );
            widgets::label(
                ui.painter(),
                k,
                Pos2::new(rect.left() + 22.0, rect.center().y),
                name,
                t.text_bright,
            );
            time::gap(ui, k);
        }
        time::scale_switch(ui, k, st);
        time::ease_field(ui, k, st);
        ui.add_space((ui.available_width() - 250.0).max(4.0));
        let room = (ui.available_width() - 2.0 * k.field() - 8.0).max(60.0);
        time::filter(ui, k, st, room);
        super::more_menu(ui, k, st);
    });
    body(
        ui,
        k,
        st,
        Body {
            cells: Cells {
                row: k.d.row,
                tracks: false,
            },
            names: 250.0,
            layers: false,
            beside: Beside::RowAndGraph,
            key: 4.5,
        },
    );
}

/// Draws the Timeline panel: the scene's tracks, with the transport left
/// to the slider.
pub fn timeline(ui: &mut Ui, k: &Look, st: &mut State) {
    super::a::show(ui, k, st, false);
}
