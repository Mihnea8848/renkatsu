// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The time slider: a thin strip under the viewport that shows whatever
//! the panels below it do, with the transport, the frames with the chosen
//! bone's keys (or the selected object's), the loop range, the markers
//! and auto-key.

use super::time::{self, RULER};
use super::{sheet, Kind};
use crate::state::{Look, State};
use crate::widgets::{self, hair_h};
use egui::{Pos2, Rect, Sense, Ui, UiBuilder, Vec2};

/// The strip's height.
pub const HEIGHT: f32 = 52.0;

/// A row of controls in a part of the strip. Returns where it ends.
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

/// Draws the slider in `rect`.
pub fn show(ui: &mut Ui, k: &Look, st: &mut State, rect: Rect) {
    let t = &k.t;
    ui.painter().rect_filled(rect, 0.0, t.panel);
    hair_h(ui.painter(), rect.left(), rect.right(), rect.top(), t.divider);
    let inner = rect.shrink2(Vec2::new(8.0, 0.0));
    let left = bar(ui, inner, |ui| {
        time::transport(ui, k, st);
        time::frame_field(ui, k, st);
    });
    let tools = 180.0 + if time::range(st).is_some() { 88.0 } else { 0.0 } + if st.anim.onion { 38.0 } else { 0.0 };
    let right = Rect::from_min_max(Pos2::new(inner.right() - tools, inner.top()), inner.max);
    let frames = Rect::from_min_max(
        Pos2::new(left + 8.0, rect.top() + 5.0),
        Pos2::new(right.left() - 8.0, rect.bottom() - 5.0),
    );
    if frames.width() < 120.0 {
        return;
    }
    bar(ui, right, |ui| {
        time::switches(ui, k, st);
        time::gap(ui, k);
        time::range_and_marker(ui, k, st);
    });
    widgets::boxed(ui.painter(), frames.expand(1.0), k.radius(), t.input, t.border);
    let ruler = Rect::from_min_max(frames.min, Pos2::new(frames.right(), frames.top() + RULER - 4.0));
    let lane = Rect::from_min_max(Pos2::new(frames.left(), ruler.bottom()), frames.max);
    let span = time::span(ui, st, frames);
    let id = ui.id().with("time slider");
    time::ruler(ui, k, st, &span, ruler, id.with("ruler"));
    let rows = super::rows(st);
    let chosen = super::chosen(st);
    let object = st.doc.selected;
    let row = rows
        .iter()
        .find(|r| Some(r.object) == object && r.kind == Kind::Channel && Some(&r.name) == chosen.as_ref())
        .filter(|r| !r.marks.is_empty())
        .or_else(|| rows.iter().find(|r| Some(r.object) == object && r.kind == Kind::Object))
        .or_else(|| rows.first());
    match row {
        Some(row) => {
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
            sheet::interact(ui, k, st, &span, lane, id.with("keys"), &marks, 5.0, None);
        }
        None => {
            ui.interact(lane, id.with("keys"), Sense::hover());
        }
    }
    time::overlay(ui, k, st, &span, ruler, frames.bottom());
}
