// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The keys as marks to work on directly: drawn in the rows' lanes,
//! selected by a click, with Shift or in a box, dragged along the frames
//! (with Alt as copies, or scaled around the playhead), copied, pasted
//! and deleted. Each change is one step of the scene's history.

use super::time::Span;
use super::{frame_of, now, Drag, DragKind, KeyRef, Kind, Row};
use crate::scene::{Content, ObjectId};
use crate::state::{Look, State};
use crate::theme::Theme;
use crate::widgets::{self, glyph, hair_h};
use egui::{Align2, Color32, Id, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use egui_phosphor::regular as icon;
use rk_chara::keys::{Ease, Key};
use std::collections::BTreeSet;

/// A diamond, the mark of a key.
pub fn diamond(p: &Painter, centre: Pos2, r: f32, fill: Color32, line: Color32) {
    p.add(egui::Shape::convex_polygon(
        vec![
            centre + Vec2::new(0.0, -r),
            centre + Vec2::new(r, 0.0),
            centre + Vec2::new(0.0, r),
            centre + Vec2::new(-r, 0.0),
        ],
        fill,
        Stroke::new(1.0, line),
    ));
}

/// A mark on the screen and the keys it stands for.
pub struct Mark {
    pub at: Pos2,
    pub keys: Vec<KeyRef>,
}

/// A key of the scene's sequence.
pub fn key_of(st: &State, (object, name, frame): &KeyRef) -> Option<Key> {
    let sequence = &st.doc.scene().sequence;
    let fps = sequence.fps.max(1.0);
    sequence
        .track(*object)?
        .keys
        .get(name)?
        .iter()
        .find(|k| frame_of(k.time, fps) == *frame)
        .copied()
}

/// Where a mark of `frame` shows: moved with the drag while one of its
/// keys is selected and dragged.
fn shown(st: &State, frame: i32, selected: bool) -> f32 {
    match &st.anim.drag {
        Some(drag) if selected => drag.moved(frame, st.anim.snap),
        _ => frame as f32,
    }
}

/// Draws the rows' keys in their lanes, each lane the part of a row over
/// the time, and gives the marks drawn. `r` is a key's half size.
pub fn lanes(ui: &Ui, k: &Look, st: &State, span: &Span, lanes: &[(Rect, &Row)], r: f32) -> Vec<Mark> {
    let t = &k.t;
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let copying = matches!(
        st.anim.drag,
        Some(Drag {
            kind: DragKind::Move { copy: true },
            ..
        })
    );
    let mut marks = Vec::new();
    for (i, (rect, row)) in lanes.iter().enumerate() {
        let p = ui.painter().with_clip_rect(rect.intersect(span.rect));
        match row.kind {
            Kind::Object => {
                p.rect_filled(*rect, 0.0, Theme::glass(t.header, 150));
            }
            Kind::Group | Kind::Layer => {
                p.rect_filled(*rect, 0.0, Color32::from_white_alpha(5));
            }
            _ if i % 2 == 1 => {
                p.rect_filled(*rect, 0.0, Color32::from_black_alpha(22));
            }
            _ => {}
        }
        hair_h(&p, rect.left(), rect.right(), rect.bottom() - 1.0, t.line);
        let y = rect.center().y;
        match row.kind {
            Kind::Clip => {
                clip(ui, k, st, span, *rect, row.object);
                continue;
            }
            Kind::Baked => {
                baked(ui, k, st, span, *rect, row.object);
                continue;
            }
            _ => {}
        }
        let summary = row.kind != Kind::Channel;
        // A bone's keys hang on a line from its first to its last.
        if let (false, Some((first, _)), Some((last, _))) = (summary, row.marks.first(), row.marks.last()) {
            if last > first {
                p.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(span.x(*first as f32), y - 1.0),
                        Pos2::new(span.x(*last as f32), y + 1.0),
                    ),
                    0.0,
                    Color32::from_white_alpha(26),
                );
            }
        }
        for (frame, names) in &row.marks {
            let keys: Vec<KeyRef> = names.iter().map(|n| (row.object, n.clone(), *frame)).collect();
            let chosen = keys.iter().filter(|key| st.anim.selected.contains(*key)).count();
            let centre = Pos2::new(span.x(shown(st, *frame, chosen > 0)) + 0.5, y);
            let under = pointer
                .is_some_and(|at| rect.contains(at) && (at.x - centre.x).abs() <= r + 2.0 && st.anim.drag.is_none());
            let size = if summary { r - 0.5 } else { r } + if under { 1.0 } else { 0.0 };
            let (fill, line) = if chosen == keys.len() {
                (t.select_bright, Color32::WHITE)
            } else if chosen > 0 {
                (t.select_soft, t.select_bright)
            } else if summary {
                (t.text_dim, Color32::from_black_alpha(160))
            } else {
                (t.text, Color32::from_black_alpha(160))
            };
            if copying && chosen > 0 {
                // The key stays where it was while its copy is dragged.
                diamond(&p, Pos2::new(span.x(*frame as f32) + 0.5, y), size, t.text, line);
            }
            let held = !summary && keys.first().and_then(|key| key_of(st, key)).map(|key| key.ease) == Some(Ease::Step);
            if held {
                // A key that holds is a square.
                let square = Rect::from_center_size(centre, Vec2::splat(size * 1.5));
                p.rect_filled(square, 1.0, fill);
                p.rect_stroke(square, 1.0, Stroke::new(1.0, line), egui::StrokeKind::Outside);
            } else {
                diamond(&p, centre, size, fill, line);
            }
            marks.push(Mark { at: centre, keys });
        }
    }
    marks
}

/// The clip a character plays, as a strip under her keys: a block each
/// pass, named, dimmed while its baked keys play in its place.
fn clip(ui: &Ui, k: &Look, st: &State, span: &Span, rect: Rect, id: ObjectId) {
    let t = &k.t;
    let Some(Content::Character {
        animation: Some(clip), ..
    }) = st.doc.scene().get(id).map(|o| &o.content)
    else {
        return;
    };
    let read = st.live.as_ref().and_then(|l| l.built(id)).and_then(|b| b.animation);
    let (fixed, loops, seconds) = read.unwrap_or((false, true, 0.0));
    let end = span.frames as f32;
    let pass = if fixed || seconds <= 0.0 {
        end
    } else {
        (seconds / clip.speed.abs().max(0.01) * span.fps).max(1.0)
    };
    let off = st.anim.baked.get(&id) == Some(&true);
    let tint = crate::kinds::Kind::Animation.color();
    let (fill, line, text) = if off {
        (Theme::glass(tint, 22), Theme::glass(tint, 70), t.text_dim)
    } else {
        (Theme::glass(tint, 70), Theme::glass(tint, 200), t.text_bright)
    };
    let p = ui.painter().with_clip_rect(rect.intersect(span.rect));
    let mut from = -(clip.start.rem_euclid(1.0)) * pass;
    let mut n = 0;
    while from < end && n < 2000 {
        let to = if loops || clip.force_loop {
            from + pass
        } else {
            end.max(from + pass)
        };
        let block = Rect::from_min_max(
            Pos2::new(span.x(from.max(0.0)), rect.top() + 3.0),
            Pos2::new(span.x(to.min(end)), rect.bottom() - 4.0),
        );
        if block.width() > 2.0 && block.right() > rect.left() && block.left() < rect.right() {
            let block = block.shrink2(Vec2::new(0.5, 0.0));
            p.rect_filled(block, 3.0, fill);
            p.rect_stroke(block, 3.0, Stroke::new(1.0, line), egui::StrokeKind::Inside);
            let inner = p.with_clip_rect(block.shrink2(Vec2::new(4.0, 0.0)).intersect(span.rect));
            let mark = if n == 0 { icon::FILM_REEL } else { icon::REPEAT };
            glyph(
                &inner,
                k,
                Pos2::new(block.left() + 12.0, block.center().y),
                mark,
                12.0,
                text,
            );
            if n == 0 {
                inner.text(
                    Pos2::new(block.left() + 22.0, block.center().y),
                    Align2::LEFT_CENTER,
                    &clip.name,
                    k.small(),
                    text,
                );
            }
            if !(loops || clip.force_loop) && to > from + pass {
                // Past its end the clip holds its last frame.
                let hold = span.x(from + pass);
                p.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(hold, block.top() + 1.0),
                        Pos2::new(hold + 1.0, block.bottom() - 1.0),
                    ),
                    0.0,
                    line,
                );
            }
        }
        from = to;
        n += 1;
    }
}

/// The clip baked to keys: a key each frame where they fit, dimmed while
/// the layer is off.
fn baked(ui: &Ui, k: &Look, st: &State, span: &Span, rect: Rect, id: ObjectId) {
    let on = st.anim.baked.get(&id) == Some(&true);
    let p = ui.painter().with_clip_rect(rect.intersect(span.rect));
    let step = [1, 2, 5, 10, 30]
        .into_iter()
        .find(|s| *s as f32 * span.px >= 7.0)
        .unwrap_or(60);
    let fill = if on { k.t.text_dim } else { k.t.border };
    let mut frame = 0;
    while frame <= span.frames {
        let centre = Pos2::new(span.x(frame as f32) + 0.5, rect.center().y);
        diamond(&p, centre, 3.0, fill, Color32::from_black_alpha(120));
        frame += step;
    }
}

/// The ease the selected keys share.
pub fn ease_of(st: &State) -> Option<Ease> {
    let mut eases = st
        .anim
        .selected
        .iter()
        .filter_map(|key| key_of(st, key))
        .map(|k| k.ease);
    let first = eases.next()?;
    eases.all(|e| e == first).then_some(first)
}

/// Changes the scene's sequence as one step of the history; `change`
/// gets the sequence and its frames a second.
fn edit(st: &mut State, held: bool, change: impl FnOnce(&mut crate::sequence::Sequence, f32)) {
    let mut sequence = st.doc.scene().sequence.clone();
    let fps = sequence.fps.max(1.0);
    change(&mut sequence, fps);
    sequence.tidy();
    st.doc.edit_sequence(sequence, held);
}

/// Puts a key on a frame of a bone's keys, over any key there.
fn put(list: &mut Vec<Key>, mut key: Key, frame: i32, fps: f32) {
    key.time = frame as f32 / fps;
    list.retain(|k| frame_of(k.time, fps) != frame);
    let at = list.partition_point(|k| k.time < key.time);
    list.insert(at, key);
}

/// Gives the selected keys this ease.
pub fn set_ease(st: &mut State, ease: Ease) {
    let selected = st.anim.selected.clone();
    edit(st, false, |sequence, fps| {
        for (object, name, frame) in &selected {
            let list = sequence
                .tracks
                .iter_mut()
                .find(|t| t.object == *object)
                .and_then(|t| t.keys.get_mut(name));
            if let Some(key) = list.and_then(|l| l.iter_mut().find(|k| frame_of(k.time, fps) == *frame)) {
                key.ease = ease;
            }
        }
    });
}

/// Moves the selected keys, each to the frame `to` gives for its own, or
/// puts copies of them there. The moved keys stay selected.
pub fn retime(st: &mut State, to: impl Fn(i32) -> i32, copy: bool) {
    retime_held(st, to, copy, false);
}

/// As `retime`; with `held` the change joins the step of a drag.
pub fn retime_held(st: &mut State, to: impl Fn(i32) -> i32, copy: bool, held: bool) {
    let selected = st.anim.selected.clone();
    let mut next = BTreeSet::new();
    edit(st, held, |sequence, fps| {
        let last = frame_of(sequence.length, fps);
        for track in &mut sequence.tracks {
            let mut lifted = Vec::new();
            for (_, name, frame) in selected.iter().filter(|key| key.0 == track.object) {
                let Some(list) = track.keys.get_mut(name) else { continue };
                let Some(at) = list.iter().position(|k| frame_of(k.time, fps) == *frame) else {
                    continue;
                };
                let key = if copy { list[at] } else { list.remove(at) };
                lifted.push((name.clone(), to(*frame).clamp(0, last), key));
            }
            for (name, frame, key) in lifted {
                put(track.keys.entry(name.clone()).or_default(), key, frame, fps);
                next.insert((track.object, name, frame));
            }
        }
    });
    st.anim.selected = next;
}

/// Gives a key another edit, as part of a drag's step.
pub fn set_edit(st: &mut State, (object, name, frame): &KeyRef, to: rk_chara::pose::BoneEdit) {
    edit(st, true, |sequence, fps| {
        let list = sequence
            .tracks
            .iter_mut()
            .find(|t| t.object == *object)
            .and_then(|t| t.keys.get_mut(name));
        if let Some(key) = list.and_then(|l| l.iter_mut().find(|k| frame_of(k.time, fps) == *frame)) {
            key.edit = to;
        }
    });
}

/// Deletes the selected keys.
pub fn delete(st: &mut State) {
    let selected = std::mem::take(&mut st.anim.selected);
    edit(st, false, |sequence, fps| {
        for (object, name, frame) in &selected {
            if let Some(track) = sequence.tracks.iter_mut().find(|t| t.object == *object) {
                if let Some(list) = track.keys.get_mut(name) {
                    list.retain(|k| frame_of(k.time, fps) != *frame);
                    if list.is_empty() {
                        track.keys.remove(name);
                    }
                }
            }
        }
    });
}

/// Copies the selected keys, each by its frames after the first of them.
pub fn copy(st: &mut State) {
    let Some(first) = st.anim.selected.iter().map(|key| key.2).min() else {
        return;
    };
    st.anim.clipboard = st
        .anim
        .selected
        .iter()
        .filter_map(|key| Some((key.1.clone(), key.2 - first, key_of(st, key)?)))
        .collect();
}

/// Pastes the copied keys on a character from the playhead on; keys
/// copied from one bone go to `onto` when it is given.
pub fn paste(st: &mut State, object: ObjectId, onto: Option<&str>) {
    let copied = st.anim.clipboard.clone();
    if copied.is_empty() {
        return;
    }
    let here = now(st).round() as i32;
    let one = copied.iter().all(|(name, ..)| *name == copied[0].0);
    let owner = st.doc.scene().get(object).map(|o| o.name.clone()).unwrap_or_default();
    let mut next = BTreeSet::new();
    edit(st, false, |sequence, fps| {
        let last = frame_of(sequence.length, fps);
        let track = sequence.track_mut(object, &owner);
        for (name, after, key) in copied {
            let name = match onto {
                Some(onto) if one => onto.to_owned(),
                _ => name,
            };
            let frame = (here + after).clamp(0, last);
            put(track.keys.entry(name.clone()).or_default(), key, frame, fps);
            next.insert((object, name, frame));
        }
    });
    st.anim.selected = next;
}

/// The marks' pointer work over `area`: a click selects a mark's keys
/// (Shift adds), a drag from a mark moves the selected keys along the
/// frames (Alt copies, and the scale switch scales around the playhead),
/// a drag from empty time selects in a box, and the right button opens
/// the keys' menu. `reach` is how far from a mark's centre counts.
#[allow(clippy::too_many_arguments)]
pub fn interact(ui: &mut Ui, k: &Look, st: &mut State, span: &Span, area: Rect, id: Id, marks: &[Mark], reach: f32) {
    let resp = ui.interact(area, id, Sense::click_and_drag());
    let hit = |at: Pos2| {
        marks
            .iter()
            .filter(|m| (m.at.x - at.x).abs() <= reach + 2.0 && (m.at.y - at.y).abs() <= reach + 4.0)
            .min_by(|a, b| {
                a.at.distance_sq(at)
                    .partial_cmp(&b.at.distance_sq(at))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    };
    let (shift, alt, origin, pointer) = ui.input(|i| {
        (
            i.modifiers.shift,
            i.modifiers.alt,
            i.pointer.press_origin(),
            i.pointer.latest_pos(),
        )
    });
    // A mark picked: its bone becomes the chosen one.
    let pick = |st: &mut State, mark: &Mark| {
        if let [(object, name, _)] = mark.keys.as_slice() {
            super::choose(st, *object, name);
        } else if let Some((object, ..)) = mark.keys.first() {
            st.doc.selected = Some(*object);
        }
        st.property_tab = 1;
    };
    if resp.drag_started_by(egui::PointerButton::Primary) {
        let from = origin.or(pointer).unwrap_or(area.center());
        match hit(from) {
            Some(mark) => {
                if !mark.keys.iter().all(|key| st.anim.selected.contains(key)) {
                    if !shift {
                        st.anim.selected.clear();
                    }
                    st.anim.selected.extend(mark.keys.iter().cloned());
                }
                pick(st, mark);
                let kind = if st.anim.scale {
                    DragKind::Scale
                } else {
                    DragKind::Move { copy: alt }
                };
                let frame = span.frame(from.x);
                st.anim.drag = Some(Drag {
                    kind,
                    from: frame,
                    to: frame,
                    pivot: now(st).round(),
                });
            }
            None => {
                if !shift {
                    st.anim.selected.clear();
                }
                st.anim.drag = Some(Drag {
                    kind: DragKind::Select(from),
                    from: 0.0,
                    to: 0.0,
                    pivot: 0.0,
                });
            }
        }
    }
    if let (Some(drag), Some(at)) = (st.anim.drag.as_mut(), pointer) {
        drag.to = span.frame(at.x);
        if let DragKind::Select(from) = drag.kind {
            let reach = Rect::from_two_pos(from, at).intersect(area);
            let p = ui.painter().with_clip_rect(area);
            p.rect_filled(reach, 0.0, Theme::glass(k.t.select, 40));
            p.rect_stroke(
                reach,
                0.0,
                Stroke::new(1.0, k.t.select_bright),
                egui::StrokeKind::Inside,
            );
        }
    }
    let down = ui.input(|i| i.pointer.primary_down());
    if st.anim.drag.is_some() && (resp.drag_stopped() || !down && !resp.dragged()) {
        let drag = st.anim.drag.take().unwrap_or(Drag {
            kind: DragKind::Scale,
            from: 0.0,
            to: 0.0,
            pivot: 0.0,
        });
        match drag.kind {
            DragKind::Select(from) => {
                let reach = Rect::from_two_pos(from, pointer.unwrap_or(from)).expand(2.0);
                for mark in marks.iter().filter(|m| reach.contains(m.at)) {
                    st.anim.selected.extend(mark.keys.iter().cloned());
                }
            }
            DragKind::Move { copy } => {
                let moved = |frame: i32| drag.moved(frame, true) as i32;
                if copy || moved(0) != 0 {
                    retime(st, moved, copy);
                }
            }
            DragKind::Scale => retime(st, |frame| drag.moved(frame, true) as i32, false),
        }
    }
    if resp.clicked() {
        match resp.interact_pointer_pos().and_then(hit) {
            Some(mark) => {
                let all = mark.keys.iter().all(|key| st.anim.selected.contains(key));
                if shift && all {
                    for key in &mark.keys {
                        st.anim.selected.remove(key);
                    }
                } else {
                    if !shift {
                        st.anim.selected.clear();
                    }
                    st.anim.selected.extend(mark.keys.iter().cloned());
                    pick(st, mark);
                }
            }
            None if !shift => st.anim.selected.clear(),
            None => {}
        }
    }
    if resp.double_clicked() {
        if let Some(frame) = resp
            .interact_pointer_pos()
            .and_then(hit)
            .and_then(|m| m.keys.first())
            .map(|key| key.2)
        {
            super::time::go_to(st, frame as f32);
        }
    }
    if resp.secondary_clicked() {
        if let Some(mark) = resp.interact_pointer_pos().and_then(hit) {
            if !mark.keys.iter().all(|key| st.anim.selected.contains(key)) {
                st.anim.selected = mark.keys.iter().cloned().collect();
            }
        }
    }
    resp.context_menu(|ui| menu(ui, k, st));
}

/// The menu of the selected keys.
fn menu(ui: &mut Ui, k: &Look, st: &mut State) {
    let any = !st.anim.selected.is_empty();
    widgets::menu_sub(ui, k, icon::BEZIER_CURVE, "Interpolation", |ui| {
        let current = ease_of(st);
        for (ease, name, _) in super::time::EASES {
            if widgets::check_item(ui, k, current == Some(ease), name, "").clicked() {
                set_ease(st, ease);
                ui.close();
            }
        }
    });
    ui.separator();
    if widgets::menu_item(ui, k, icon::COPY, "Copy", "Ctrl+C", any) {
        copy(st);
        ui.close();
    }
    let target = st.doc.selected.filter(|_| !st.anim.clipboard.is_empty());
    if widgets::menu_item(ui, k, icon::CLIPBOARD, "Paste", "Ctrl+V", target.is_some()) {
        if let Some(id) = target {
            paste(st, id, None);
        }
        ui.close();
    }
    if widgets::menu_item(ui, k, icon::ARROWS_HORIZONTAL, "Move to playhead", "", any) {
        let first = st.anim.selected.iter().map(|key| key.2).min().unwrap_or(0);
        let by = now(st).round() as i32 - first;
        retime(st, |frame| frame + by, false);
        ui.close();
    }
    ui.separator();
    if widgets::menu_item(ui, k, icon::TRASH, "Delete", "Delete", any) {
        delete(st);
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_chara::pose::BoneEdit;

    fn key(time: f32) -> Key {
        Key {
            time,
            edit: BoneEdit::default(),
            ease: Ease::Linear,
        }
    }

    #[test]
    fn a_key_put_on_a_frame_replaces_the_one_there() {
        let mut list = vec![key(0.0), key(1.0), key(2.0)];
        put(&mut list, key(0.0), 30, 30.0);
        let times: Vec<f32> = list.iter().map(|k| k.time).collect();
        assert_eq!(times, [0.0, 1.0, 2.0]);
        put(&mut list, key(0.0), 45, 30.0);
        assert_eq!(list.len(), 4);
        assert_eq!(list[2].time, 1.5);
    }
}
