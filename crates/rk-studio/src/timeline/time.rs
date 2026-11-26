// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The one clock of the animation panels: the frames shown, zoomed and
//! scrolled the same in every panel, their ruler with the loop range and
//! the markers, the playhead, and the bar's transport and switches.

use super::{frame_of, now};
use crate::state::{Look, Playback, State};
use crate::theme::Theme;
use crate::widgets::{self, bar_divider, glyph, hair_h, hair_v, icon_button, label};
use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Ui, Vec2};
use egui_phosphor::regular as icon;
use rk_chara::keys::Ease;

/// Room left and right of the frames, so the first and last key show whole.
pub const PAD: f32 = 14.0;
/// The ruler's height.
pub const RULER: f32 = 24.0;

/// The frames a panel shows across `rect`.
#[derive(Debug, Clone, Copy)]
pub struct Span {
    pub rect: Rect,
    /// Points a frame.
    pub px: f32,
    /// The frame at the left end.
    pub first: f32,
    pub fps: f32,
    /// The sequence's last frame.
    pub frames: i32,
}

impl Span {
    pub fn x(&self, frame: f32) -> f32 {
        (self.rect.left() + PAD + (frame - self.first) * self.px).round()
    }

    pub fn frame(&self, x: f32) -> f32 {
        (x - self.rect.left() - PAD) / self.px + self.first
    }

    /// The frames between two numbers of the ruler, and between ticks.
    pub fn steps(&self) -> (i32, i32) {
        let major = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 5000]
            .into_iter()
            .find(|s| *s as f32 * self.px >= 56.0)
            .unwrap_or(10000);
        let minor = match major {
            1 | 2 => 1,
            5 => 1,
            s => s / 5,
        };
        (major, minor)
    }
}

/// The frames shown across `rect`, after this frame's zooming (Ctrl and
/// the wheel), panning (the middle button, or the wheel sideways) and
/// fitting (Home).
pub fn span(ui: &Ui, st: &mut State, rect: Rect) -> Span {
    let sequence = &st.doc.scene().sequence;
    let fps = sequence.fps.max(1.0);
    let frames = frame_of(sequence.length, fps).max(1);
    let fit = ((rect.width() - 2.0 * PAD) / frames as f32).max(0.02);
    let (mut px, mut first) = if st.anim.fit {
        (fit, 0.0)
    } else {
        (st.anim.px, st.anim.first)
    };
    if ui.rect_contains_pointer(rect) {
        let (zoom, wheel, middle, moved, at) = ui.input(|i| {
            (
                i.zoom_delta(),
                i.smooth_scroll_delta.x,
                i.pointer.middle_down(),
                i.pointer.delta().x,
                i.pointer.hover_pos(),
            )
        });
        let mut changed = false;
        if let Some(at) = at.filter(|_| (zoom - 1.0).abs() > 1e-4) {
            let under = (at.x - rect.left() - PAD) / px + first;
            px = (px * zoom).clamp(fit * 0.5, 80.0);
            first = under - (at.x - rect.left() - PAD) / px;
            changed = true;
        }
        let pan = wheel + if middle { moved } else { 0.0 };
        if pan != 0.0 {
            first -= pan / px;
            changed = true;
        }
        if changed {
            st.anim.fit = false;
            st.anim.px = px;
            st.anim.first = first;
        }
    }
    Span {
        rect,
        px,
        first,
        fps,
        frames,
    }
}

/// Moves the playhead to a frame, within the sequence.
pub fn go_to(st: &mut State, frame: f32) {
    let sequence = &st.doc.scene().sequence;
    let time = frame.max(0.0) / sequence.fps.max(1.0);
    st.solo = None;
    super::go_to(st, time);
}

/// The ruler: the frames' numbers, the loop range with its two ends to
/// drag, and the markers. A click or a drag moves the playhead.
pub fn ruler(ui: &mut Ui, k: &Look, st: &mut State, span: &Span, rect: Rect, id: Id) {
    let t = &k.t;
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 0.0, t.header);
    hair_h(&p, rect.left(), rect.right(), rect.bottom() - 1.0, t.divider);
    if let Some((from, to)) = range(st) {
        let band = Rect::from_min_max(
            Pos2::new(span.x(from as f32), rect.top()),
            Pos2::new(span.x(to as f32) + 1.0, rect.bottom() - 1.0),
        );
        p.rect_filled(band, 0.0, Theme::glass(t.select, 70));
    }
    let (major, minor) = span.steps();
    let from = (span.frame(rect.left()).floor() as i32).max(0) / minor * minor;
    let to = (span.frame(rect.right()).ceil() as i32).min(span.frames);
    let mut f = from;
    while f <= to {
        let x = span.x(f as f32);
        let whole = f % major == 0;
        hair_v(
            &p,
            x,
            rect.bottom() - if whole { 9.0 } else { 4.0 },
            rect.bottom() - 1.0,
            if whole { t.text_dim } else { t.hover },
        );
        if whole {
            p.text(
                Pos2::new(x + 4.0, rect.top() + 9.0),
                Align2::LEFT_CENTER,
                f.to_string(),
                k.small(),
                t.text_dim,
            );
        }
        f += minor;
    }
    let scrub = ui.interact(rect, id, Sense::click_and_drag());
    if scrub.clicked() || scrub.dragged() {
        if let Some(at) = scrub.interact_pointer_pos() {
            go_to(st, span.frame(at.x).clamp(0.0, span.frames as f32));
        }
    }
    // The loop range's ends, each a bracket to drag.
    if let Some((from, to)) = range(st) {
        for end in 0..2 {
            let frame = if end == 0 { from } else { to };
            let x = span.x(frame as f32);
            let grip = Rect::from_min_max(Pos2::new(x - 5.0, rect.top()), Pos2::new(x + 6.0, rect.bottom()));
            let resp = ui
                .interact(grip, id.with(("range", end)), Sense::drag())
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            if let Some(at) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
                let at = (span.frame(at.x).round() as i32).clamp(0, span.frames);
                let next = if end == 0 {
                    [at.min(to - 1), to]
                } else {
                    [from, at.max(from + 1)]
                };
                edit(st, true, |sequence| sequence.loop_range = Some(next));
            }
            let color = if resp.hovered() || resp.dragged() {
                Color32::WHITE
            } else {
                t.select_bright
            };
            p.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x - 1.0, rect.top() + 2.0),
                    Pos2::new(x + 1.0, rect.bottom() - 1.0),
                ),
                0.0,
                color,
            );
            let lip = if end == 0 { 5.0 } else { -5.0 };
            for y in [rect.top() + 2.0, rect.bottom() - 3.0] {
                p.rect_filled(
                    Rect::from_two_pos(Pos2::new(x, y), Pos2::new(x + lip, y + 2.0)),
                    0.0,
                    color,
                );
            }
        }
    }
    // The markers: a flag and its name, to drag along the frames; a right
    // click names it or takes it away.
    let markers = st.doc.scene().sequence.markers.clone();
    let mut next = markers.clone();
    let mut jump = None;
    for (i, marker) in markers.iter().enumerate() {
        let x = span.x(marker.frame as f32);
        let end = p
            .text(
                Pos2::new(x + 19.0, rect.top() + 9.0),
                Align2::LEFT_CENTER,
                &marker.name,
                k.small(),
                t.accent,
            )
            .right();
        glyph(
            &p,
            k,
            Pos2::new(x + 8.0, rect.top() + 9.0),
            icon::FLAG_PENNANT,
            12.0,
            t.accent,
        );
        let flag = Rect::from_min_max(Pos2::new(x - 2.0, rect.top()), Pos2::new(end + 2.0, rect.top() + 16.0));
        let resp = ui.interact(flag, id.with(("marker", i)), Sense::click_and_drag());
        if resp.clicked() {
            jump = Some(marker.frame);
        }
        if let Some(at) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
            next[i].frame = (span.frame(at.x).round() as i32).clamp(0, span.frames);
        }
        resp.context_menu(|ui| {
            ui.set_min_width(160.0);
            ui.add(egui::TextEdit::singleline(&mut next[i].name).desired_width(150.0));
            ui.separator();
            if widgets::menu_item(ui, k, icon::TRASH, "Delete marker", "", true) {
                next[i].frame = -1;
                ui.close();
            }
        });
    }
    if next != markers {
        next.retain(|m| m.frame >= 0);
        edit(st, true, |sequence| sequence.markers = next);
    }
    if let Some(frame) = jump {
        go_to(st, frame as f32);
    }
}

/// Over the rows, from the ruler down: the time outside the sequence and
/// the loop range dimmed, the markers' lines, and the playhead with its
/// frame.
pub fn overlay(ui: &Ui, k: &Look, st: &State, span: &Span, ruler: Rect, bottom: f32) {
    let t = &k.t;
    let rows = Rect::from_min_max(
        Pos2::new(ruler.left(), ruler.bottom()),
        Pos2::new(ruler.right(), bottom),
    );
    let p = ui
        .painter()
        .with_clip_rect(Rect::from_min_max(ruler.min, Pos2::new(ruler.right(), bottom)));
    let shade = |from: f32, to: f32, alpha: u8| {
        let (from, to) = (from.max(rows.left()), to.min(rows.right()));
        if to > from {
            p.rect_filled(
                Rect::from_min_max(Pos2::new(from, rows.top()), Pos2::new(to, rows.bottom())),
                0.0,
                Color32::from_black_alpha(alpha),
            );
        }
    };
    shade(rows.left(), span.x(0.0), 70);
    shade(span.x(span.frames as f32) + 1.0, rows.right(), 70);
    if let Some((from, to)) = range(st) {
        shade(span.x(0.0), span.x(from as f32), 50);
        shade(span.x(to as f32) + 1.0, span.x(span.frames as f32) + 1.0, 50);
        for frame in [from, to] {
            hair_v(
                &p,
                span.x(frame as f32),
                rows.top(),
                rows.bottom(),
                Theme::glass(t.select_bright, 120),
            );
        }
    }
    for marker in &st.doc.scene().sequence.markers {
        hair_v(
            &p,
            span.x(marker.frame as f32),
            ruler.top() + 16.0,
            rows.bottom(),
            Theme::glass(t.accent, 110),
        );
    }
    // The onion skin's reach, either side of the playhead.
    let at = now(st);
    let x = span.x(at);
    if st.anim.onion {
        let reach = st.anim.onion_frames.round();
        let band = Rect::from_min_max(
            Pos2::new(span.x(at - reach), ruler.bottom() - 4.0),
            Pos2::new(span.x(at + reach) + 1.0, ruler.bottom() - 1.0),
        );
        p.rect_filled(band, 1.0, Theme::glass(t.play, 200));
    }
    hair_v(&p, x, ruler.top() + 4.0, rows.bottom(), t.select);
    let text = format!("{}", at.round() as i32);
    let galley = p.layout_no_wrap(text, k.small(), Color32::WHITE);
    let head = Rect::from_center_size(
        Pos2::new(x + 0.5, ruler.top() + 9.0),
        Vec2::new(galley.size().x + 10.0, 15.0),
    );
    p.rect_filled(head, 3.0, t.select);
    p.galley(head.center() - galley.size() / 2.0, galley, Color32::WHITE);
}

/// The frames of the selected object's keys, or of every key.
fn key_frames(st: &State) -> Vec<i32> {
    let sequence = &st.doc.scene().sequence;
    let fps = sequence.fps.max(1.0);
    let own = st.doc.selected.and_then(|id| sequence.track(id));
    let mut frames: Vec<i32> = sequence
        .tracks
        .iter()
        .filter(|t| own.is_none_or(|o| o.object == t.object))
        .flat_map(|t| rk_chara::keys::times(&t.keys))
        .map(|time| frame_of(time, fps))
        .collect();
    frames.sort_unstable();
    frames.dedup();
    frames
}

/// The transport: to the start, from key to key, play and loop.
pub fn transport(ui: &mut Ui, k: &Look, st: &mut State) {
    if icon_button(ui, k, icon::SKIP_BACK, false)
        .on_hover_text("Go to start")
        .clicked()
    {
        let start = range(st).map_or(0, |(from, _)| from);
        go_to(st, start as f32);
    }
    let here = now(st).round() as i32;
    if icon_button(ui, k, icon::CARET_LINE_LEFT, false)
        .on_hover_text("Previous key")
        .clicked()
    {
        if let Some(frame) = key_frames(st).into_iter().rev().find(|f| *f < here) {
            go_to(st, frame as f32);
        }
    }
    let playing = st.playback == Playback::Playing || st.solo.is_some();
    let (g, tip) = if playing {
        (icon::PAUSE, "Pause")
    } else {
        (icon::PLAY, "Play")
    };
    if icon_button(ui, k, g, playing).on_hover_text(tip).clicked() {
        if let Some(id) = st.solo {
            super::toggle_solo(st, id);
        } else {
            st.playback = if playing { Playback::Paused } else { Playback::Playing };
        }
    }
    if icon_button(ui, k, icon::CARET_LINE_RIGHT, false)
        .on_hover_text("Next key")
        .clicked()
    {
        if let Some(frame) = key_frames(st).into_iter().find(|f| *f > here) {
            go_to(st, frame as f32);
        }
    }
    if icon_button(ui, k, icon::REPEAT, st.loop_sequence)
        .on_hover_text("Loop")
        .clicked()
    {
        st.loop_sequence = !st.loop_sequence;
    }
}

/// A whole number in a field `width` wide. Returns the number when it was
/// changed.
fn whole(ui: &mut Ui, k: &Look, id: Id, value: i32, width: f32) -> Option<i32> {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, k.field()), Sense::hover());
    let mut shown = value as f32;
    widgets::number_to(ui, k, rect, id, &mut shown, widgets::Marker::None, true, 0.25, 0);
    let next = shown.round() as i32;
    (next != value).then_some(next)
}

/// Dim words in a bar.
fn caption(ui: &mut Ui, k: &Look, text: &str) {
    let w = widgets::text_width(ui, text, k.font());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w + 4.0, k.field()), Sense::hover());
    label(
        ui.painter(),
        k,
        Pos2::new(rect.left() + 2.0, rect.center().y),
        text,
        k.t.text_dim,
    );
}

/// The playhead's frame, to type or drag, and the sequence's last.
pub fn frame_field(ui: &mut Ui, k: &Look, st: &mut State) {
    let here = now(st).round() as i32;
    if let Some(frame) = whole(ui, k, ui.id().with("anim frame"), here, 52.0) {
        let last = frame_of(st.doc.scene().sequence.length, st.doc.scene().sequence.fps.max(1.0));
        go_to(st, frame.clamp(0, last) as f32);
    }
    let sequence = &st.doc.scene().sequence;
    let last = frame_of(sequence.length, sequence.fps.max(1.0));
    caption(ui, k, &format!("/ {last}"));
}

/// The sequence's length in frames and its frames a second.
pub fn length_fields(ui: &mut Ui, k: &Look, st: &mut State) {
    let sequence = st.doc.scene().sequence.clone();
    let fps = sequence.fps.max(1.0);
    let frames = frame_of(sequence.length, fps);
    caption(ui, k, "End");
    let next_frames = whole(ui, k, ui.id().with("anim end"), frames, 52.0);
    let next_fps = whole(ui, k, ui.id().with("anim fps"), fps.round() as i32, 40.0);
    caption(ui, k, "fps");
    if next_frames.is_some() || next_fps.is_some() {
        let rate = next_fps.unwrap_or(fps.round() as i32).clamp(1, 240) as f32;
        let mut changed = sequence;
        changed.length = (next_frames.unwrap_or(frames).max(1) as f32 / fps).clamp(0.1, 3600.0);
        changed.fps = rate;
        st.doc.edit_sequence(changed, true);
    }
}

/// Auto-key, snapping, the onion skin with its reach and the motion
/// trail.
pub fn switches(ui: &mut Ui, k: &Look, st: &mut State) {
    let record = icon_button(ui, k, icon::RECORD, st.anim.auto_key).on_hover_text("Auto-key");
    if st.anim.auto_key {
        glyph(ui.painter(), k, record.rect.center(), icon::RECORD, 16.0, k.t.stop);
    }
    if record.clicked() {
        st.anim.auto_key = !st.anim.auto_key;
    }
    if icon_button(ui, k, icon::MAGNET, st.anim.snap)
        .on_hover_text("Snap to frames")
        .clicked()
    {
        st.anim.snap = !st.anim.snap;
    }
    if icon_button(ui, k, icon::GHOST, st.anim.onion)
        .on_hover_text("Onion skin")
        .clicked()
    {
        st.anim.onion = !st.anim.onion;
    }
    if st.anim.onion {
        let reach = st.anim.onion_frames.round() as i32;
        if let Some(next) = whole(ui, k, ui.id().with("anim onion"), reach, 34.0) {
            st.anim.onion_frames = next.clamp(1, 30) as f32;
        }
    }
    if icon_button(ui, k, icon::PATH, st.anim.trail)
        .on_hover_text("Motion trail")
        .clicked()
    {
        st.anim.trail = !st.anim.trail;
    }
}

/// The loop range and a marker at the playhead.
pub fn range_and_marker(ui: &mut Ui, k: &Look, st: &mut State) {
    let on = range(st);
    if icon_button(ui, k, icon::BRACKETS_SQUARE, on.is_some())
        .on_hover_text("Loop range")
        .clicked()
    {
        let sequence = &st.doc.scene().sequence;
        let last = frame_of(sequence.length, sequence.fps.max(1.0));
        // The selected keys' reach, or the whole sequence.
        let frames: Vec<i32> = st.anim.selected.iter().map(|r| r.2).collect();
        let low = frames.iter().copied().min().unwrap_or(0);
        let high = frames.iter().copied().max().unwrap_or(last);
        let next = match on {
            Some(_) => None,
            None if high > low => Some([low, high]),
            None => Some([0, last]),
        };
        edit(st, false, |sequence| sequence.loop_range = next);
    }
    if let Some((from, to)) = range(st) {
        if let Some(next) = whole(ui, k, ui.id().with("anim in"), from, 40.0) {
            edit(st, true, |sequence| {
                sequence.loop_range = Some([next.clamp(0, to - 1), to])
            });
        }
        if let Some(next) = whole(ui, k, ui.id().with("anim out"), to, 40.0) {
            edit(st, true, |sequence| {
                sequence.loop_range = Some([from, next.max(from + 1)])
            });
        }
    }
    if icon_button(ui, k, icon::FLAG_PENNANT, false)
        .on_hover_text("Add marker")
        .clicked()
    {
        let here = now(st).round() as i32;
        edit(st, false, |sequence| {
            if !sequence.markers.iter().any(|m| m.frame == here) {
                let name = format!("Marker {}", sequence.markers.len() + 1);
                sequence.markers.push(crate::sequence::Marker { frame: here, name });
                sequence.markers.sort_by_key(|m| m.frame);
            }
        });
    }
}

/// The loop range, while playing stays in one.
pub fn range(st: &State) -> Option<(i32, i32)> {
    st.doc.scene().sequence.loop_range.map(|[from, to]| (from, to))
}

/// Changes the sequence's markers or loop range as one step of the
/// history, or with `held` as part of a drag's.
fn edit(st: &mut State, held: bool, change: impl FnOnce(&mut crate::sequence::Sequence)) {
    let mut sequence = st.doc.scene().sequence.clone();
    change(&mut sequence);
    st.doc.edit_sequence(sequence, held);
}

/// The eases a key may have, as the lists name them.
pub const EASES: [(Ease, &str, &str); 6] = [
    (Ease::Step, "Constant", icon::STAIRS),
    (Ease::Linear, "Linear", icon::LINE_SEGMENT),
    (Ease::EaseIn, "Ease in", icon::WAVE_SINE),
    (Ease::EaseOut, "Ease out", icon::WAVE_SINE),
    (Ease::Smooth, "Ease in-out", icon::WAVE_SINE),
    (Ease::Bezier, "Bezier", icon::BEZIER_CURVE),
];

/// The switch that makes drags scale the keys around the playhead.
pub fn scale_switch(ui: &mut Ui, k: &Look, st: &mut State) {
    if icon_button(ui, k, icon::ARROWS_OUT_LINE_HORIZONTAL, st.anim.scale)
        .on_hover_text("Scale around playhead")
        .clicked()
    {
        st.anim.scale = !st.anim.scale;
    }
}

/// Only the keyed rows, and the search, in what is left of the bar.
pub fn filter(ui: &mut Ui, k: &Look, st: &mut State, width: f32) {
    if icon_button(ui, k, icon::FUNNEL, st.anim.keyed_only)
        .on_hover_text("Keyed only")
        .clicked()
    {
        st.anim.keyed_only = !st.anim.keyed_only;
    }
    widgets::search(ui, k, &mut st.animator_search, "Search", width);
}

/// A divider between a bar's groups.
pub fn gap(ui: &mut Ui, k: &Look) {
    bar_divider(ui, k);
}
