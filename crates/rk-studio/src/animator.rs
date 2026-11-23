// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Animator panel: the selected character's skeleton while she is
//! posed by hand (FK), each joint by its readable name, to choose a bone
//! as a click on its ball does.
//!
//! It edits the character's pose layer, plain values that keys will drive:
//! with the scene's timeline the character's FK and IK animation plays as
//! her track beside the map's objects, the effects, lights and cameras,
//! and this panel shows each bone's keys at the timeline's time.

use crate::panels::toolbar;
use crate::scene::Content;
use crate::state::{Look, State};
use crate::widgets::{self, glyph};
use egui::{Align2, Color32, Pos2, Sense, Ui, Vec2};
use egui_phosphor::regular as icon;

/// Draws the panel.
pub fn show(ui: &mut Ui, k: &Look, st: &mut State) {
    let t = &k.t;
    let character = st
        .doc
        .selection()
        .filter(|o| matches!(o.content, Content::Character { .. }))
        .map(|o| (o.id, o.name.clone()));
    let fk_on = matches!(
        st.doc.selection().map(|o| &o.content),
        Some(Content::Character { pose_layer: Some(l), .. }) if l.fk_on
    );
    toolbar(ui, k, |ui| {
        if let Some((_, name)) = &character {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(140.0, k.field()), Sense::hover());
            glyph(
                ui.painter(),
                k,
                Pos2::new(rect.left() + 8.0, rect.center().y),
                crate::kinds::Kind::Character.glyph(),
                14.0,
                t.text,
            );
            widgets::label(
                ui.painter(),
                k,
                Pos2::new(rect.left() + 22.0, rect.center().y),
                name,
                t.text_bright,
            );
        }
        let w = ui.available_width();
        widgets::search(ui, k, &mut st.animator_search, "Search", w);
    });
    let area = ui.available_rect_before_wrap();
    let say = |ui: &Ui, text: &str| {
        ui.painter()
            .text(area.center(), Align2::CENTER_CENTER, text, k.font(), t.text_dim);
    };
    let Some((id, _)) = character else {
        say(ui, "Select a character");
        return;
    };
    if !fk_on {
        say(ui, "Forward kinematics is off");
        return;
    }
    let Some(joints) = st.live.as_ref().and_then(|l| l.joints(id)) else {
        say(ui, "The character is not built");
        return;
    };
    let needle = st.animator_search.trim().to_lowercase();
    let mut chosen: Option<String> = None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (depth, name) in &joints {
            let shown = crate::bones::pose_label(name);
            if !needle.is_empty() && !shown.to_lowercase().contains(&needle) && !name.to_lowercase().contains(&needle) {
                continue;
            }
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row), Sense::click());
            let on = st.pose_bone.as_deref() == Some(name.as_str());
            let p = ui.painter();
            if on {
                p.rect_filled(rect, 0.0, t.select);
            } else if resp.hovered() {
                p.rect_filled(rect, 0.0, t.row_hover);
            }
            let indent = if needle.is_empty() { *depth as f32 * 14.0 } else { 0.0 };
            let x = rect.left() + 10.0 + indent;
            let y = rect.center().y;
            let (text, faint) = if on {
                (Color32::WHITE, Color32::from_white_alpha(170))
            } else {
                (t.text, t.text_dim)
            };
            glyph(p, k, Pos2::new(x + 6.0, y), icon::BONE, 13.0, faint);
            widgets::label(p, k, Pos2::new(x + 18.0, y), &shown, text);
            let resp = if shown != *name { resp.on_hover_text(name) } else { resp };
            if resp.clicked() {
                chosen = Some(name.clone());
            }
        }
    });
    if let Some(name) = chosen {
        st.pose_bone = Some(name);
        st.property_tab = 2;
    }
}
