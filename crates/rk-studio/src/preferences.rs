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
    crate::widgets::window(ctx, k, "Preferences", 480.0, &mut open, |ui| {
        let before = st.settings.clone();
        ui.label(RichText::new("Koikatsu").font(k.bold(k.d.text)).color(t.text_bright));
        ui.add_space(4.0);
        let game = rk_core::Config::load().ok().map(|c| c.game_dir);
        ui.horizontal(|ui| {
            match &game {
                Some(dir) => ui.label(dir.display().to_string()),
                None => ui.label(RichText::new("No folder chosen").color(t.accent)),
            };
            if ui.button("Browse").clicked() {
                st.pick(crate::dialogs::Pick::GameFolder, game.clone(), ui.ctx());
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new("Start").font(k.bold(k.d.text)).color(t.text_bright));
        ui.add_space(4.0);
        ui.checkbox(
            &mut st.settings.show_project_selector,
            "Show the project selector at start",
        );
        ui.checkbox(&mut st.settings.check_updates, "Check for a newer Renkatsu at start");
        ui.checkbox(&mut st.settings.show_news, "Show news on the home page");
        ui.add_space(8.0);
        ui.label(RichText::new("Viewport").font(k.bold(k.d.text)).color(t.text_bright));
        ui.add_space(4.0);
        ui.checkbox(&mut st.settings.smooth_edges, "Smooth edges");
        ui.add_space(4.0);
        ui.label("Frame rate");
        for rate in FrameRate::ALL {
            ui.radio_value(&mut st.settings.frame_rate, rate, rate.label());
        }
        if st.settings.frame_rate.vsync() != st.vsync_at_start {
            ui.add_space(4.0);
            ui.label(RichText::new("Applies after a restart").color(t.accent));
        }
        let asks = |s: &crate::settings::Settings| (s.check_updates, s.show_news);
        if asks(&st.settings) != asks(&before) {
            st.fetch_feed();
        }
        if st.settings != before && !st.scripted {
            if let Err(e) = st.settings.save() {
                eprintln!("could not save the settings: {e:#}");
            }
        }
    });
    st.preferences_open = open;
}
