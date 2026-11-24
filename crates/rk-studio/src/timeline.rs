// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Timeline panel: the scene's sequence on one clock. Its bar plays,
//! goes to the start and loops, and says the frame; the ruler moves the
//! playhead on the frames; each object with keys has a row, its keys as
//! diamonds. A bone's keys are edited in the Animator.

use crate::kinds::Kind;
use crate::panels::toolbar;
use crate::state::{Look, Playback, State};
use crate::widgets::{self, bar_divider, glyph, hair_h, hair_v, icon_button, label};
use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use egui_phosphor::regular as icon;

/// Where the rows' names end and the time begins.
const NAMES: f32 = 168.0;

/// A diamond, the mark of a key.
pub fn diamond(p: &egui::Painter, centre: Pos2, r: f32, fill: Color32) {
    p.add(egui::Shape::convex_polygon(
        vec![
            centre + Vec2::new(0.0, -r),
            centre + Vec2::new(r, 0.0),
            centre + Vec2::new(0.0, r),
            centre + Vec2::new(-r, 0.0),
        ],
        fill,
        Stroke::new(1.0, Color32::from_black_alpha(140)),
    ));
}

/// The playhead goes to `time`, on the frames, and holds the scene there.
pub fn go_to(st: &mut State, time: f32) {
    st.clock = st.doc.scene().sequence.snap(time);
    st.playback = Playback::Paused;
}

/// Draws the panel.
pub fn show(ui: &mut Ui, k: &Look, st: &mut State) {
    let t = &k.t;
    let sequence = st.doc.scene().sequence.clone();
    let fps = sequence.fps.max(1.0);
    let frames = (sequence.length * fps).round().max(1.0);
    let now = st.clock.clamp(0.0, sequence.length);
    let mut length = sequence.length;
    let mut rate = sequence.fps;
    let bar = toolbar(ui, k, |ui| {
        if icon_button(ui, k, icon::SKIP_BACK, false)
            .on_hover_text("Go to start")
            .clicked()
        {
            go_to(st, 0.0);
        }
        let playing = st.playback == Playback::Playing;
        let (g, tip) = if playing {
            (icon::PAUSE, "Pause")
        } else {
            (icon::PLAY, "Play")
        };
        if icon_button(ui, k, g, false).on_hover_text(tip).clicked() {
            st.playback = if playing { Playback::Paused } else { Playback::Playing };
        }
        if icon_button(ui, k, icon::REPEAT, st.loop_sequence)
            .on_hover_text("Loop")
            .clicked()
        {
            st.loop_sequence = !st.loop_sequence;
        }
        bar_divider(ui, k);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(150.0, k.field()), Sense::hover());
        let frame = (now * fps).round() as i64;
        let x = label(
            ui.painter(),
            k,
            rect.left_center(),
            &format!("Frame {frame}"),
            t.text_bright,
        );
        label(
            ui.painter(),
            k,
            Pos2::new(x + 6.0, rect.center().y),
            &format!("of {frames}"),
            t.text_dim,
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let field = |ui: &mut Ui, value: &mut f32, id: &str, unit: &str| {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(64.0, k.field()), Sense::hover());
                widgets::number(ui, k, rect, ui.id().with(id), value, widgets::Marker::None, true, 0.05);
                let (r, _) = ui.allocate_exact_size(
                    Vec2::new(widgets::text_width(ui, unit, k.font()) + 6.0, k.field()),
                    Sense::hover(),
                );
                label(ui.painter(), k, r.left_center(), unit, t.text_dim);
            };
            field(ui, &mut rate, "fps", "fps");
            ui.add_space(8.0);
            field(ui, &mut length, "length", "Length");
        });
    });
    // A new length or frame rate, within reason.
    let length = length.clamp(0.1, 3600.0);
    let rate = rate.round().clamp(1.0, 240.0);
    if (length - sequence.length).abs() > 1e-4 || (rate - sequence.fps).abs() > 1e-4 {
        let mut changed = sequence.clone();
        changed.length = length;
        changed.fps = rate;
        st.doc.edit_sequence(changed, false);
    }

    let body = ui.available_rect_before_wrap();
    let p = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
    hair_h(&p, bar.left(), bar.right(), body.top(), t.line);
    let ruler = Rect::from_min_max(
        Pos2::new(body.left() + NAMES, body.top() + 1.0),
        Pos2::new(body.right(), body.top() + 23.0),
    );
    p.rect_filled(ruler, 0.0, t.header);
    hair_v(&p, ruler.left() - 1.0, body.top(), body.bottom(), t.line);
    let (x0, x1) = (ruler.left() + 12.0, ruler.right() - 12.0);
    let x_of = |time: f32| (x0 + (x1 - x0) * time / sequence.length.max(0.01)).round();
    let time_of = |x: f32| (x - x0) / (x1 - x0).max(1.0) * sequence.length;
    // Frame numbers where they fit: every 5, 10, 30 or 60 frames.
    let per_frame = (x1 - x0) / frames;
    let step = [1.0, 5.0, 10.0, 30.0, 60.0, 300.0]
        .into_iter()
        .find(|s| s * per_frame >= 48.0)
        .unwrap_or(600.0);
    let minor = (step / 5.0).max(1.0);
    let mut f = 0.0;
    while f <= frames + 0.5 {
        let x = x_of(f / fps);
        let major = (f % step).abs() < 0.5;
        hair_v(
            &p,
            x,
            ruler.bottom() - if major { 8.0 } else { 4.0 },
            ruler.bottom(),
            t.hover,
        );
        if major {
            p.text(
                Pos2::new(x + 3.0, ruler.top() + 8.0),
                Align2::LEFT_CENTER,
                format!("{f:.0}"),
                k.small(),
                t.text_dim,
            );
        }
        f += minor;
    }
    let scrub = ui.interact(ruler, ui.id().with("timeline ruler"), Sense::click_and_drag());
    if scrub.clicked() || scrub.dragged() {
        if let Some(pos) = scrub.interact_pointer_pos() {
            go_to(st, time_of(pos.x));
        }
    }
    if scrub.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    // A row for each object with keys.
    let mut select = None;
    let mut clear = None;
    let rows: Vec<_> = sequence
        .tracks
        .iter()
        .filter_map(|track| {
            let object = st.doc.scene().get(track.object)?;
            Some((
                track.object,
                object.name.clone(),
                object.kind(),
                rk_chara::keys::times(&track.keys),
            ))
        })
        .collect();
    if rows.is_empty() {
        p.text(
            Pos2::new(body.center().x, ruler.bottom() + 40.0),
            Align2::CENTER_CENTER,
            "No keys",
            k.font(),
            t.text_dim,
        );
    }
    for (i, (id, name, kind, times)) in rows.iter().enumerate() {
        let top = ruler.bottom() + i as f32 * k.d.row;
        let row = Rect::from_min_size(Pos2::new(body.left(), top), Vec2::new(body.width(), k.d.row));
        let resp = ui.interact(row, ui.id().with(("timeline row", id.0)), Sense::click());
        if st.doc.selected == Some(*id) {
            p.rect_filled(row, 0.0, Color32::from_white_alpha(8));
        } else if resp.hovered() {
            p.rect_filled(row, 0.0, t.row_hover);
        }
        let y = row.center().y;
        let kind: Kind = *kind;
        glyph(
            &p,
            k,
            Pos2::new(body.left() + 20.0, y),
            kind.glyph(),
            15.0,
            kind.color(),
        );
        label(&p, k, Pos2::new(body.left() + 34.0, y), name, t.text);
        hair_h(&p, body.left(), body.right(), row.bottom() - 1.0, t.line);
        for time in times {
            diamond(&p, Pos2::new(x_of(*time) + 0.5, y), 4.5, t.text);
        }
        if resp.clicked() {
            select = Some(*id);
        }
        resp.context_menu(|ui| {
            if widgets::menu_item(ui, k, icon::CURSOR_CLICK, "Select", "", true) {
                select = Some(*id);
                ui.close();
            }
            if widgets::menu_item(ui, k, icon::TRASH, "Clear keys", "", true) {
                clear = Some(*id);
                ui.close();
            }
        });
    }
    // The playhead, its head on the ruler.
    let x = x_of(now);
    p.rect_filled(
        Rect::from_min_max(Pos2::new(x - 4.0, ruler.top()), Pos2::new(x + 5.0, ruler.top() + 9.0)),
        1.0,
        t.select,
    );
    hair_v(&p, x, ruler.top(), body.bottom(), t.select);
    ui.allocate_rect(body, Sense::hover());
    if let Some(id) = select {
        st.doc.selected = Some(id);
    }
    if let Some(id) = clear {
        crate::keying::clear_keys(st, id);
    }
}
