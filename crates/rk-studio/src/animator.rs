// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Animator panel: the selected character's skeleton, to choose a
//! bone as a click on its ball does. It lists the rows of her track in
//! the Timeline, by the parts of her body, without their keys: each
//! bone, IK target and value with its key at the playhead.

use crate::panels::toolbar;
use crate::scene::Content;
use crate::state::{Look, State};
use crate::timeline::{self, time, Cells, Kind};
use crate::widgets::{self, glyph};
use egui::{Align2, Pos2, Sense, Ui, Vec2};

/// Draws the panel.
pub fn show(ui: &mut Ui, k: &Look, st: &mut State) {
    let t = &k.t;
    let character = st
        .doc
        .selection()
        .filter(|o| matches!(o.content, Content::Character { .. }))
        .map(|o| (o.id, o.name.clone()));
    toolbar(ui, k, |ui| {
        if let Some((_, name)) = &character {
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
        let room = (ui.available_width() - 2.0 * k.field() - 8.0).max(60.0);
        time::filter(ui, k, st, room);
        timeline::more_menu(ui, k, st);
    });
    let area = ui.available_rect_before_wrap();
    let Some((id, _)) = character else {
        ui.painter().text(
            area.center(),
            Align2::CENTER_CENTER,
            "Select a character",
            k.font(),
            t.text_dim,
        );
        return;
    };
    // Her rows, without the head of her track.
    let rows: Vec<_> = timeline::rows(st)
        .into_iter()
        .filter(|r| r.object == id && r.kind != Kind::Object)
        .collect();
    let cells = Cells { row: k.d.row };
    egui::ScrollArea::vertical()
        .id_salt("animator rows")
        .auto_shrink(false)
        .show(ui, |ui| {
            for row in &rows {
                let size = Vec2::new(ui.available_width(), cells.height(row));
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                if ui.is_rect_visible(rect) {
                    timeline::name_cell(ui, k, st, row, rect);
                }
            }
        });
}
