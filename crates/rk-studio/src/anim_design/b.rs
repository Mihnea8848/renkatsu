// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Design B, the tracks: the Timeline stays the scene's view, a track an
//! object. A character's track opens in place into her clip and her keys
//! as two layers, then the parts of her body and their bones, with keys
//! large enough to grab. The selected keys' frame, interpolation and
//! values are a group of Properties.

use super::a::{body, Beside, Body};
use super::time;
use super::Cells;
use crate::panels::toolbar;
use crate::state::{Look, State};
use egui::Ui;

/// Draws the Timeline panel.
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
        ui.add_space((ui.available_width() - 420.0).max(4.0));
        time::length_fields(ui, k, st);
        time::gap(ui, k);
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
                row: k.d.row + 6.0,
                tracks: true,
            },
            names: 270.0,
            layers: true,
            beside: Beside::Sheet,
            key: 6.5,
        },
    );
}
