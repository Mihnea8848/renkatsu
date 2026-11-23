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
use crate::widgets::{self, glyph, hair_h, hair_v};
use egui::{Align2, Color32, Pos2, Rect, Sense, Ui, Vec2};
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
    let rows: Vec<(usize, &String, String)> = joints
        .iter()
        .map(|(depth, name)| (*depth, name, crate::bones::pose_label(name)))
        .filter(|(_, name, shown)| {
            needle.is_empty() || shown.to_lowercase().contains(&needle) || name.to_lowercase().contains(&needle)
        })
        .collect();

    // The time the keys lie in: one pass of the character's animation, or
    // ten seconds while she holds a pose; the playhead at the scene's time.
    let pass = st
        .live
        .as_ref()
        .and_then(|l| l.built(id))
        .and_then(|b| b.animation)
        .filter(|(still, _, seconds)| !still && *seconds > 0.1)
        .map_or(10.0, |(_, _, seconds)| seconds);
    let now = st.clock.rem_euclid(pass);

    let area = ui.available_rect_before_wrap();
    let tree_w = (area.width() * 0.3).clamp(200.0, 320.0);
    let (ruler, _) = ui.allocate_exact_size(Vec2::new(area.width(), 22.0), Sense::hover());
    let sheet_x = |r: Rect| (r.left() + tree_w + 8.0, r.right() - 12.0);
    let (x0, x1) = sheet_x(ruler);
    let at = |time: f32| x0 + (x1 - x0) * (time / pass);
    // A tick each second, a finer one each quarter while there is room.
    let per_second = (x1 - x0) / pass;
    let step = if per_second > 160.0 {
        0.25
    } else if per_second > 60.0 {
        0.5
    } else {
        1.0
    };
    {
        let p = ui.painter();
        p.rect_filled(ruler, 0.0, t.header);
        hair_h(p, ruler.left(), ruler.right(), ruler.bottom() - 0.5, t.divider);
        hair_v(p, ruler.left() + tree_w, ruler.top(), ruler.bottom(), t.divider);
        let ticks = (pass / step).floor() as usize;
        for i in 0..=ticks {
            let time = i as f32 * step;
            let x = at(time).round() + 0.5;
            let whole = (time - time.round()).abs() < 1e-4;
            let top = if whole { ruler.top() + 6.0 } else { ruler.bottom() - 6.0 };
            hair_v(p, x, top, ruler.bottom(), if whole { t.text_dim } else { t.border });
            if whole {
                p.text(
                    Pos2::new(x + 3.0, ruler.top() + 2.0),
                    Align2::LEFT_TOP,
                    format!("{time:.0}"),
                    k.small(),
                    t.text_dim,
                );
            }
        }
    }

    let mut chosen: Option<String> = None;
    let listed = egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (i, (depth, name, shown)) in rows.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row), Sense::click());
            let on = st.pose_bone.as_deref() == Some(name.as_str());
            let p = ui.painter();
            let tree = Rect::from_min_max(rect.min, Pos2::new(rect.left() + tree_w, rect.bottom()));
            // The bone's row of the sheet, every other one a shade darker.
            let (sx0, sx1) = sheet_x(rect);
            let sheet = Rect::from_min_max(Pos2::new(sx0, rect.top()), Pos2::new(sx1, rect.bottom()));
            if i % 2 == 1 {
                p.rect_filled(sheet, 0.0, Color32::from_black_alpha(28));
            }
            if on {
                p.rect_filled(tree, 0.0, t.select);
                p.rect_filled(sheet, 0.0, Color32::from_white_alpha(10));
            } else if resp.hovered() {
                p.rect_filled(tree, 0.0, t.row_hover);
            }
            hair_v(p, rect.left() + tree_w, rect.top(), rect.bottom(), t.divider);
            let indent = if needle.is_empty() { *depth as f32 * 14.0 } else { 0.0 };
            let x = rect.left() + 10.0 + indent;
            let y = rect.center().y;
            let (text, faint) = if on {
                (Color32::WHITE, Color32::from_white_alpha(170))
            } else {
                (t.text, t.text_dim)
            };
            glyph(p, k, Pos2::new(x + 6.0, y), icon::BONE, 13.0, faint);
            widgets::label(p, k, Pos2::new(x + 18.0, y), shown, text);
            let resp = if shown != *name {
                resp.on_hover_text(name.as_str())
            } else {
                resp
            };
            if resp.clicked() {
                chosen = Some((*name).clone());
            }
        }
    });
    // The playhead, over the ruler and the rows.
    let x = at(now).round() + 0.5;
    let p = ui.painter();
    p.vline(
        x,
        ruler.top()..=listed.inner_rect.bottom(),
        egui::Stroke::new(1.5, t.accent),
    );
    if let Some(name) = chosen {
        st.pose_bone = Some(name);
        st.property_tab = 1;
    }
}
