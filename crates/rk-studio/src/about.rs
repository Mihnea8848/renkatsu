// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Help, About Renkatsu: the name, the version, who made it and what it is
//! built on.

use crate::state::{Look, State};
use crate::widgets::{self, flat_button};
use egui::{Align2, Pos2, Rect, Sense, Vec2};

const WIDTH: f32 = 360.0;
const REPOSITORY: &str = "https://github.com/Mihnea8848/renkatsu";
const LICENCE: &str = "https://github.com/Mihnea8848/renkatsu/blob/main/LICENSE";

pub fn show(ctx: &egui::Context, k: &Look, st: &mut State) {
    if !st.about_open {
        return;
    }
    let mut open = true;
    widgets::window(ctx, k, "About Renkatsu", WIDTH, &mut open, |ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 56.0), Sense::hover());
        let mark = Rect::from_min_size(rect.min + Vec2::new(0.0, 4.0), Vec2::splat(48.0));
        let p = ui.painter();
        p.rect_filled(mark, 6.0, k.t.accent);
        p.text(mark.center(), Align2::CENTER_CENTER, "R", k.bold(32.0), k.t.input);
        let x = mark.right() + 14.0;
        p.text(
            Pos2::new(x, mark.center().y - 9.0),
            Align2::LEFT_CENTER,
            "Renkatsu Studio",
            k.bold(k.d.text + 5.0),
            k.t.text_bright,
        );
        p.text(
            Pos2::new(x, mark.center().y + 12.0),
            Align2::LEFT_CENTER,
            format!("Version {}", env!("CARGO_PKG_VERSION")),
            k.font(),
            k.t.text_dim,
        );
        ui.add_space(8.0);
        let line = |ui: &mut egui::Ui, text: &str| ui.label(egui::RichText::new(text).color(k.t.text));
        line(ui, "Created by Mihnea8848");
        line(ui, "© 2026 Mihnea Gîrbăcică (Mihnea8848). All rights reserved.");
        line(ui, "PolyForm Shield 1.0.0");
        ui.add_space(10.0);
        ui.label(egui::RichText::new("Credits").font(k.small()).color(k.t.text_dim));
        for text in [
            "Koikatsu and its content are ILLUSION's.",
            "Renkatsu is not made by ILLUSION.",
            "Built with egui, wgpu and egui_dock.",
            "Icons: Phosphor. Font: Open Sans.",
        ] {
            line(ui, text);
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if flat_button(ui, k, None, "GitHub", false, false).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(REPOSITORY));
            }
            if flat_button(ui, k, None, "Licence", false, false).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(LICENCE));
            }
        });
    });
    if !open {
        st.about_open = false;
    }
}
