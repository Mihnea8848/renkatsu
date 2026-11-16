// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Preferences window under Edit: the user's own settings, saved as
//! soon as they change.

use crate::settings::FrameRate;
use crate::state::{Look, State};
use egui::RichText;

pub fn show(ctx: &egui::Context, k: &Look, st: &mut State) {
    if !st.preferences_open {
        return;
    }
    let t = &k.t;
    let mut open = true;
    egui::Window::new("Preferences")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .default_width(360.0)
        .show(ctx, |ui| {
            ui.label(RichText::new("Viewport").font(k.bold(k.d.text)).color(t.text_bright));
            ui.add_space(4.0);
            ui.label("Frame rate");
            let before = st.settings;
            for rate in FrameRate::ALL {
                ui.radio_value(&mut st.settings.frame_rate, rate, rate.label());
                ui.indent(rate.label(), |ui| {
                    ui.label(RichText::new(rate.hint()).font(k.small()).color(t.text_dim));
                });
            }
            if st.settings.frame_rate.vsync() != st.vsync_at_start {
                ui.add_space(4.0);
                ui.label(RichText::new("Waiting for the display changes after a restart.").color(t.accent));
            }
            if st.settings != before {
                if let Err(e) = st.settings.save() {
                    eprintln!("could not save the settings: {e:#}");
                }
            }
        });
    st.preferences_open = open;
}
