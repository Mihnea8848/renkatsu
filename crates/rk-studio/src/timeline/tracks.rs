// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Timeline panel: one bar with the transport, the frame, auto-key,
//! snapping, the onion skin, the loop range, the keys' tools and the
//! filter; under it the tracks beside their keys, or beside the selected
//! keys' curves while the Graph switch is on.

use super::time::{self, RULER};
use super::{graph, sheet, Cells, Row};
use crate::panels::toolbar;
use crate::state::{Look, State};
use crate::widgets::{hair_h, hair_v, icon_button};
use egui::{Align2, Pos2, Rect, Ui, UiBuilder, Vec2};
use egui_phosphor::regular as icon;

/// The list's width.
const NAMES: f32 = 270.0;
/// A key's half size.
const KEY: f32 = 6.5;

/// Draws the panel.
pub fn show(ui: &mut Ui, k: &Look, st: &mut State) {
    toolbar(ui, k, |ui| {
        time::transport(ui, k, st);
        time::frame_field(ui, k, st);
        time::gap(ui, k);
        time::switches(ui, k, st);
        time::gap(ui, k);
        time::range_and_marker(ui, k, st);
        time::gap(ui, k);
        time::scale_switch(ui, k, st);
        if icon_button(ui, k, icon::CHART_LINE, st.anim.graph)
            .on_hover_text("Graph")
            .clicked()
        {
            st.anim.graph = !st.anim.graph;
        }
        ui.add_space((ui.available_width() - 420.0).max(4.0));
        time::length_fields(ui, k, st);
        time::gap(ui, k);
        let room = (ui.available_width() - 2.0 * k.field() - 8.0).max(60.0);
        time::filter(ui, k, st, room);
        super::more_menu(ui, k, st);
    });
    let t = &k.t;
    let cells = Cells { row: k.d.row + 6.0 };
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, egui::Sense::hover());
    let p = ui.painter().with_clip_rect(area);
    hair_h(&p, area.left(), area.right(), area.top(), t.line);
    let split = area.left() + NAMES.min(area.width() * 0.45);
    let ruler = Rect::from_min_max(
        Pos2::new(split + 1.0, area.top() + 1.0),
        Pos2::new(area.right(), area.top() + 1.0 + RULER),
    );
    let corner = Rect::from_min_max(Pos2::new(area.left(), ruler.top()), Pos2::new(split, ruler.bottom()));
    p.rect_filled(corner, 0.0, t.header);
    hair_h(&p, corner.left(), corner.right(), corner.bottom() - 1.0, t.divider);
    let timed = Rect::from_min_max(ruler.min, area.max);
    let span = time::span(ui, st, timed);
    let id = ui.id().with("timeline");
    time::ruler(ui, k, st, &span, ruler, id.with("ruler"));
    let rows = super::rows(st);
    let below = Rect::from_min_max(Pos2::new(area.left(), ruler.bottom()), area.max);
    if rows.is_empty() {
        p.text(below.center(), Align2::CENTER_CENTER, "No keys", k.font(), t.text_dim);
        hair_v(&p, split, area.top(), area.bottom(), t.divider);
        time::overlay(ui, k, st, &span, ruler, area.bottom());
        return;
    }
    // The object whose curves show: the selected one, or the first.
    let object = st
        .doc
        .selected
        .filter(|id| rows.iter().any(|r| r.object == *id))
        .unwrap_or(rows[0].object);
    let graphed = st.anim.graph;
    let list = if graphed {
        Rect::from_min_max(below.min, Pos2::new(split, below.bottom()))
    } else {
        below
    };
    ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
        ui.set_clip_rect(list.intersect(ui.clip_rect()));
        egui::ScrollArea::vertical()
            .id_salt(id.with("rows"))
            .auto_shrink(false)
            .show(ui, |ui| {
                let mut lanes: Vec<(Rect, &Row)> = Vec::new();
                for row in &rows {
                    let size = Vec2::new(ui.available_width(), cells.height(row));
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    let name = Rect::from_min_max(rect.min, Pos2::new(split, rect.bottom()));
                    super::name_cell(ui, k, st, row, name);
                    if !graphed {
                        lanes.push((Rect::from_min_max(Pos2::new(split + 1.0, rect.top()), rect.max), row));
                    }
                }
                if !graphed {
                    let marks = sheet::lanes(ui, k, st, &span, &lanes, KEY);
                    let over = Rect::from_min_max(Pos2::new(split + 1.0, below.top()), below.max);
                    sheet::interact(ui, k, st, &span, over, id.with("keys"), &marks, KEY, None);
                }
            });
    });
    if graphed {
        let curves = Rect::from_min_max(Pos2::new(split + 1.0, below.top()), below.max);
        graph::show(ui, k, st, &span, curves, object, id.with("graph"));
    }
    hair_v(&p, split, area.top(), area.bottom(), t.divider);
    time::overlay(ui, k, st, &span, ruler, area.bottom());
}
