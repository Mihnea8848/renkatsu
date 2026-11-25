// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Design A, the dope sheet: Animator and Timeline as one panel. One bar
//! holds the transport, the frame, auto-key, snapping, the onion skin,
//! the loop range, the keys' tools and the filter; under it the list of
//! objects, body parts and bones beside their keys, the clip as a strip
//! under the character's own row. The Graph switch shows curves in the
//! sheet's place.

use super::time::{self, RULER};
use super::{graph, sheet, Cells, Kind, Row};
use crate::panels::toolbar;
use crate::scene::Content;
use crate::state::{Look, State};
use crate::widgets::{hair_h, hair_v, icon_button};
use egui::{Align2, Pos2, Rect, Ui, UiBuilder, Vec2};
use egui_phosphor::regular as icon;

/// What shows beside the list.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Beside {
    /// Each row's keys.
    Sheet,
    /// The chosen keys' curves.
    Graph,
    /// A row of the chosen keys over their curves.
    RowAndGraph,
}

/// How a design draws the list and the keys.
#[derive(Clone, Copy)]
pub struct Body {
    pub cells: Cells,
    /// The list's width.
    pub names: f32,
    /// The keys by hand as a layer beside the clip.
    pub layers: bool,
    pub beside: Beside,
    /// A key's half size.
    pub key: f32,
}

/// Draws the panel. Without `whole` the bar leaves out the transport and
/// the switches, for a design that has them elsewhere.
pub fn show(ui: &mut Ui, k: &Look, st: &mut State, whole: bool) {
    toolbar(ui, k, |ui| {
        if whole {
            time::transport(ui, k, st);
            time::frame_field(ui, k, st);
            time::gap(ui, k);
            time::switches(ui, k, st);
            time::gap(ui, k);
            time::range_and_marker(ui, k, st);
            time::gap(ui, k);
        }
        time::scale_switch(ui, k, st);
        time::ease_field(ui, k, st);
        time::gap(ui, k);
        if icon_button(ui, k, icon::CHART_LINE, st.anim.graph)
            .on_hover_text("Graph")
            .clicked()
        {
            st.anim.graph = !st.anim.graph;
        }
        let right = if whole { 420.0 } else { 250.0 };
        ui.add_space((ui.available_width() - right).max(4.0));
        if whole {
            time::length_fields(ui, k, st);
            time::gap(ui, k);
        }
        let room = (ui.available_width() - 2.0 * k.field() - 8.0).max(60.0);
        time::filter(ui, k, st, room);
        super::more_menu(ui, k, st);
    });
    let beside = if st.anim.graph { Beside::Graph } else { Beside::Sheet };
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
            beside,
            key: 5.0,
        },
    );
}

/// The list of rows with, beside it, the ruler over the keys or the
/// curves, and the playhead over all of it.
pub fn body(ui: &mut Ui, k: &Look, st: &mut State, body: Body) {
    let t = &k.t;
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, egui::Sense::hover());
    let p = ui.painter().with_clip_rect(area);
    hair_h(&p, area.left(), area.right(), area.top(), t.line);
    let split = area.left() + body.names.min(area.width() * 0.45);
    let ruler = Rect::from_min_max(
        Pos2::new(split + 1.0, area.top() + 1.0),
        Pos2::new(area.right(), area.top() + 1.0 + RULER),
    );
    let corner = Rect::from_min_max(Pos2::new(area.left(), ruler.top()), Pos2::new(split, ruler.bottom()));
    p.rect_filled(corner, 0.0, t.header);
    hair_h(&p, corner.left(), corner.right(), corner.bottom() - 1.0, t.divider);
    let timed = Rect::from_min_max(ruler.min, area.max);
    let span = time::span(ui, st, timed);
    let id = ui.id().with("anim body");
    time::ruler(ui, k, st, &span, ruler, id.with("ruler"));
    let rows = super::rows(st, body.layers);
    let below = Rect::from_min_max(Pos2::new(area.left(), ruler.bottom()), area.max);
    if rows.is_empty() {
        p.text(
            below.center(),
            Align2::CENTER_CENTER,
            "Select a character",
            k.font(),
            t.text_dim,
        );
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
    let sheet_on = body.beside == Beside::Sheet;
    let list = if sheet_on {
        below
    } else {
        Rect::from_min_max(below.min, Pos2::new(split, below.bottom()))
    };
    ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
        ui.set_clip_rect(list.intersect(ui.clip_rect()));
        egui::ScrollArea::vertical()
            .id_salt(id.with("rows"))
            .auto_shrink(false)
            .show(ui, |ui| {
                let mut lanes: Vec<(Rect, &Row)> = Vec::new();
                for row in &rows {
                    let size = Vec2::new(ui.available_width(), body.cells.height(row));
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    let name = Rect::from_min_max(rect.min, Pos2::new(split, rect.bottom()));
                    super::name_cell(ui, k, st, row, name, body.cells);
                    if sheet_on {
                        lanes.push((Rect::from_min_max(Pos2::new(split + 1.0, rect.top()), rect.max), row));
                    }
                }
                if sheet_on {
                    let marks = sheet::lanes(ui, k, st, &span, &lanes, body.key);
                    let over = Rect::from_min_max(Pos2::new(split + 1.0, below.top()), below.max);
                    sheet::interact(ui, k, st, &span, over, id.with("keys"), &marks, body.key);
                }
            });
    });
    if !sheet_on {
        let mut curves = Rect::from_min_max(Pos2::new(split + 1.0, below.top()), below.max);
        if body.beside == Beside::RowAndGraph {
            // The chosen bone's keys in a row over its curves, or all of
            // the character's.
            let chosen = super::chosen(st);
            let row = rows
                .iter()
                .find(|r| r.object == object && r.kind == Kind::Channel && Some(&r.name) == chosen.as_ref())
                .filter(|r| !r.marks.is_empty())
                .or_else(|| rows.iter().find(|r| r.object == object && r.kind == Kind::Object));
            let lane = Rect::from_min_max(curves.min, Pos2::new(curves.right(), curves.top() + k.d.row + 2.0));
            ui.painter().rect_filled(lane, 0.0, t.panel);
            if let Some(row) = row {
                let marks = sheet::lanes(ui, k, st, &span, &[(lane, row)], body.key + 0.5);
                sheet::interact(ui, k, st, &span, lane, id.with("row keys"), &marks, body.key + 0.5);
            }
            hair_h(ui.painter(), lane.left(), lane.right(), lane.bottom() - 1.0, t.divider);
            curves = curves.with_min_y(lane.bottom());
        }
        let has_clip = matches!(
            st.doc.scene().get(object).map(|o| &o.content),
            Some(Content::Character { .. })
        );
        if has_clip {
            graph::show(ui, k, st, &span, curves, object, id.with("graph"));
        }
    }
    hair_v(&p, split, area.top(), area.bottom(), t.divider);
    time::overlay(ui, k, st, &span, ruler, area.bottom());
}
