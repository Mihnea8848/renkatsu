// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The graph: the chosen bone's or value's keys as curves over the
//! frames, a curve each axis, with the keys as points and the handles of
//! their eases. A point is dragged along the frames and in value, a
//! handle to shape the curve into the next key; each drag is one step of
//! the history.

use super::sheet::{self, Curves, Mark, Point};
use super::time::Span;
use super::{frame_of, DragKind, KeyRef};
use crate::scene::ObjectId;
use crate::sequence::BAKED;
use crate::state::{Look, State};
use crate::theme::Theme;
use crate::widgets::hair_h;
use egui::{Align2, Color32, Id, Pos2, Rect, Stroke, Ui};
use rk_chara::keys::{Ease, Key};
use rk_chara::pose::BoneEdit;
use std::collections::BTreeSet;

/// What a bone's or value's curves are of.
#[derive(Clone, Copy, PartialEq)]
enum Of {
    Value,
    Place,
    Turn,
}

fn of(name: &str) -> Of {
    if rk_chara::channels::Channel::of_key(name).is_some() {
        Of::Value
    } else if rk_chara::pose::ik_key_target(name).is_some() {
        Of::Place
    } else {
        Of::Turn
    }
}

/// An edit as the numbers its curves show: a value, a place in
/// centimetres or a turn in degrees.
fn numbers(what: Of, edit: &BoneEdit) -> Vec<f32> {
    match what {
        Of::Value => vec![rk_chara::channels::of_edit(*edit)],
        Of::Place => (edit.pos * 100.0).to_array().to_vec(),
        Of::Turn => {
            let (y, x, z) = edit.rot.to_euler(glam::EulerRot::YXZ);
            vec![x.to_degrees(), y.to_degrees(), z.to_degrees()]
        }
    }
}

/// An edit with one of its numbers changed by `by`.
fn nudged(what: Of, edit: BoneEdit, axis: usize, by: f32) -> BoneEdit {
    let mut now = numbers(what, &edit);
    let Some(number) = now.get_mut(axis) else { return edit };
    *number += by;
    match what {
        Of::Value => BoneEdit {
            pos: glam::Vec3::new(now[0], edit.pos.y, edit.pos.z),
            ..edit
        },
        Of::Place => BoneEdit {
            pos: glam::Vec3::new(now[0], now[1], now[2]) / 100.0,
            ..edit
        },
        Of::Turn => BoneEdit {
            rot: rk_chara::scene::euler_deg(now[0], now[1], now[2]),
            ..edit
        },
    }
}

/// The bones and values whose curves show: the chosen one when it has
/// keys, else those of the selected keys.
pub fn plotted(st: &State, object: ObjectId) -> Vec<String> {
    let Some(track) = st.doc.scene().sequence.track(object) else {
        return Vec::new();
    };
    let keyed = |name: &str| track.list(name).is_some_and(|l| !l.is_empty());
    let mut names: Vec<String> = st
        .anim
        .selected
        .iter()
        .filter(|key| key.0 == object && keyed(&key.1))
        .map(|key| key.1.clone())
        .collect();
    names.dedup();
    names.truncate(4);
    if names.is_empty() {
        names.extend(super::chosen(st).filter(|n| keyed(n)));
    }
    names
}

/// A step between the graph's lines that reads well: 1, 2 or 5 times a
/// power of ten.
fn nice(span: f32) -> f32 {
    let raw = (span / 5.0).max(1e-4);
    let power = 10f32.powf(raw.log10().floor());
    let lead = raw / power;
    power
        * if lead < 1.5 {
            1.0
        } else if lead < 3.5 {
            2.0
        } else if lead < 7.5 {
            5.0
        } else {
            10.0
        }
}

/// A name's keys with what its curves are of: each key, its frame and
/// whether it is selected.
type Curve = (String, Of, Vec<(Key, i32, bool)>);

/// The name a row of `name` is shown under.
fn label(name: &str) -> String {
    use rk_chara::channels::Channel;
    if let Some(channel) = Channel::of_key(name) {
        return channel.label();
    }
    if let Some(target) = rk_chara::pose::ik_key_target(name) {
        return rk_chara::pose::ik_label(target).to_owned();
    }
    crate::bones::pose_label(name.strip_prefix(BAKED).unwrap_or(name))
}

/// Draws the curves of a character's chosen keys in `rect` and takes the
/// pointer's work on their points and handles.
pub fn show(ui: &mut Ui, k: &Look, st: &mut State, span: &Span, rect: Rect, object: ObjectId, id: Id) {
    let t = &k.t;
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 0.0, t.well);
    let names = plotted(st, object);
    let fps = span.fps;
    // Each name's keys, the selected ones where a drag of the sheet's
    // kind has them.
    let curves: Vec<Curve> = names
        .iter()
        .filter_map(|name| {
            let list = st.doc.scene().sequence.track(object)?.list(name)?;
            let mut keys: Vec<(Key, i32, bool)> = list
                .iter()
                .map(|key| {
                    let frame = frame_of(key.time, fps);
                    let chosen = st.anim.selected.contains(&(object, name.clone(), frame));
                    let mut key = *key;
                    if let (true, Some(drag)) = (chosen, &st.anim.drag) {
                        key.time = drag.moved(frame, st.anim.snap) / fps;
                    }
                    (key, frame, chosen)
                })
                .collect();
            keys.sort_by(|a, b| a.0.time.total_cmp(&b.0.time));
            Some((name.clone(), of(name), keys))
        })
        .collect();
    if curves.is_empty() {
        p.text(rect.center(), Align2::CENTER_CENTER, "No keys", k.font(), t.text_dim);
        sheet::interact(ui, k, st, span, rect, id, &[], 6.0, None);
        return;
    }
    // The curves, a point each few pixels, and the reach of their values.
    let (left, right) = (
        span.x(0.0).max(rect.left()),
        span.x(span.frames as f32).min(rect.right()),
    );
    let mut lines: Vec<(Color32, Vec<(f32, f32)>)> = Vec::new();
    let (mut low, mut high) = (f32::MAX, f32::MIN);
    for (name, what, keys) in &curves {
        let list: Vec<Key> = keys.iter().map(|(key, ..)| *key).collect();
        let count = numbers(*what, &BoneEdit::default()).len();
        let mut points = vec![Vec::new(); count];
        let mut x = left;
        while x <= right {
            let time = span.frame(x) / fps;
            let plain = name.strip_prefix(BAKED).unwrap_or(name);
            if let Some(edit) = rk_chara::keys::sample_named(plain, &list, time) {
                for (axis, value) in numbers(*what, &edit).into_iter().enumerate() {
                    points[axis].push((x, value));
                    low = low.min(value);
                    high = high.max(value);
                }
            }
            x += 2.0;
        }
        for (axis, points) in points.into_iter().enumerate() {
            let color = if count == 1 { t.text_bright } else { t.axis[axis] };
            lines.push((color, points));
        }
    }
    let least = if curves.iter().all(|c| c.1 == Of::Value) {
        1.0
    } else {
        10.0
    };
    if high - low < least {
        let middle = (high + low) / 2.0;
        (low, high) = (middle - least / 2.0, middle + least / 2.0);
    }
    let room = (high - low) * 0.18;
    // While its points are dragged the graph keeps its reach, so that
    // the pointer and the values stay together.
    let dragging = matches!(
        st.anim.drag.map(|d| d.kind),
        Some(DragKind::Point { .. } | DragKind::Handle { .. })
    );
    let (low, high) = match st.anim.frozen {
        Some(reach) if dragging => reach,
        _ => (low - room, high + room),
    };
    st.anim.frozen = dragging.then_some((low, high));
    let y_of = |value: f32| rect.bottom() - (value - low) / (high - low) * rect.height();
    let value_at = |y: f32| low + (rect.bottom() - y) / rect.height() * (high - low);
    // The grid: a line each step of the values, the frames' as the ruler.
    let step = nice(high - low);
    let mut value = (low / step).ceil() * step;
    while value <= high {
        let y = y_of(value).round();
        let zero = value.abs() < step * 0.01;
        hair_h(&p, rect.left(), rect.right(), y, if zero { t.border } else { t.line });
        let text = if step < 1.0 {
            format!("{value:.1}")
        } else {
            format!("{value:.0}")
        };
        p.text(
            egui::pos2(rect.left() + 6.0, y - 1.0),
            Align2::LEFT_BOTTOM,
            text,
            k.small(),
            t.text_dim,
        );
        value += step;
    }
    let (major, _) = span.steps();
    let mut frame = (span.frame(rect.left()).floor() as i32).max(0) / major * major;
    while frame <= span.frames {
        crate::widgets::hair_v(&p, span.x(frame as f32), rect.top(), rect.bottom(), t.line);
        frame += major;
    }
    for (color, points) in &lines {
        let points: Vec<Pos2> = points.iter().map(|(x, v)| Pos2::new(*x, y_of(*v))).collect();
        p.add(egui::Shape::line(points, Stroke::new(1.5, *color)));
    }
    // The keys: a point each curve, and the two handles of the curve
    // into the next key.
    let mut marks = Vec::new();
    let mut line = 0;
    for (name, what, keys) in &curves {
        let count = numbers(*what, &BoneEdit::default()).len();
        for axis in 0..count {
            let color = lines[line + axis].0;
            let number = |key: &Key| numbers(*what, &key.edit)[axis];
            let at = |key: &Key| Pos2::new(span.x(key.time * fps) + 0.5, y_of(number(key)));
            for (i, (key, frame, chosen)) in keys.iter().enumerate() {
                let here = at(key);
                let owner: KeyRef = (object, name.clone(), *frame);
                if let (Some((next, _, next_chosen)), Some(handles)) =
                    (keys.get(i + 1), key.curve().filter(|_| key.ease != Ease::Linear))
                {
                    let there = at(next);
                    let (from, to) = (number(key), number(next));
                    let place =
                        |x: f32, y: f32| Pos2::new(here.x + (there.x - here.x) * x, y_of(from + (to - from) * y));
                    for (leaves, end, lit) in [
                        (true, place(handles[0], handles[1]), *chosen),
                        (false, place(handles[2], handles[3]), *next_chosen),
                    ] {
                        let tint = if lit { Color32::WHITE } else { Theme::glass(color, 150) };
                        let root = if leaves { here } else { there };
                        p.line_segment([root, end], Stroke::new(1.0, tint));
                        p.circle_filled(end, 3.0, tint);
                        marks.push(Mark {
                            at: end,
                            keys: vec![owner.clone()],
                            point: Some(Point::Handle { leaves, axis }),
                        });
                    }
                }
                let fill = if *chosen { Color32::WHITE } else { color };
                let edge = if *chosen {
                    t.select_bright
                } else {
                    Color32::from_black_alpha(200)
                };
                sheet::diamond(&p, here, if *chosen { 5.0 } else { 4.0 }, fill, edge);
                marks.push(Mark {
                    at: here,
                    keys: vec![owner],
                    point: Some(Point::Curve { axis }),
                });
            }
        }
        line += count;
    }
    // What the curves are of, and each axis in its colour.
    let first = &curves[0];
    let what = match first.1 {
        Of::Value => "",
        Of::Place => "Position",
        Of::Turn => "Rotation",
    };
    let shown = label(&first.0);
    let mut x = p
        .text(
            egui::pos2(rect.left() + 44.0, rect.top() + 12.0),
            Align2::LEFT_CENTER,
            if what.is_empty() {
                shown
            } else {
                format!("{shown}  {what}")
            },
            k.bold(k.d.text),
            t.text_bright,
        )
        .right();
    if first.1 != Of::Value {
        for (axis, letter) in ["X", "Y", "Z"].into_iter().enumerate() {
            x = p
                .text(
                    egui::pos2(x + 12.0, rect.top() + 12.0),
                    Align2::LEFT_CENTER,
                    letter,
                    k.bold(k.d.text),
                    t.axis[axis],
                )
                .right();
        }
    }
    let curves = Curves { value_at: &value_at };
    // The points come first, so that a key is picked before a handle
    // that lies on it.
    marks.sort_by_key(|m| matches!(m.point, Some(Point::Handle { .. })));
    sheet::interact(ui, k, st, span, rect, id, &marks, 6.0, Some(&curves));
}

/// A drag of the graph, made again from how the keys were when it began:
/// a point takes the selected keys along the frames and, on its axis, in
/// value; a handle shapes its key's curve into the next key, which
/// becomes a bezier.
pub fn drag(st: &mut State, span: &Span, kind: DragKind, at: Pos2, curves: &Curves) {
    let Some((origin, selected, from)) = st.anim.drag_from.clone() else {
        return;
    };
    let mut sequence = origin;
    let fps = sequence.fps.max(1.0);
    let last = frame_of(sequence.length, fps);
    match kind {
        DragKind::Point { axis } => {
            let by = (span.frame(at.x) - span.frame(from.x)).round() as i32;
            let lift = (curves.value_at)(at.y) - (curves.value_at)(from.y);
            let mut next = BTreeSet::new();
            for track in &mut sequence.tracks {
                let mut lifted = Vec::new();
                let owner = track.object;
                for (_, name, frame) in selected.iter().filter(|key| key.0 == owner) {
                    let Some(list) = track.list_mut(name) else { continue };
                    let Some(i) = list.iter().position(|k| frame_of(k.time, fps) == *frame) else {
                        continue;
                    };
                    let mut key = list.remove(i);
                    key.edit = nudged(of(name), key.edit, axis, lift);
                    lifted.push((name.clone(), (*frame + by).clamp(0, last), key));
                }
                for (name, frame, key) in lifted {
                    if let Some(list) = track.list_mut(&name) {
                        sheet::put(list, key, frame, fps);
                        next.insert((track.object, name, frame));
                    }
                }
            }
            sequence.tidy();
            st.anim.selected = next;
        }
        DragKind::Handle { leaves, axis } => {
            let Some(owner) = st.anim.handle.clone() else { return };
            let what = of(&owner.1);
            let Some(list) = sequence
                .tracks
                .iter_mut()
                .find(|t| t.object == owner.0)
                .and_then(|t| t.list_mut(&owner.1))
            else {
                return;
            };
            let Some(i) = list.iter().position(|k| frame_of(k.time, fps) == owner.2) else {
                return;
            };
            let Some(next) = list.get(i + 1).copied() else { return };
            let key = &mut list[i];
            let (start, end) = (key.time * fps, next.time * fps);
            let (low, high) = (numbers(what, &key.edit)[axis], numbers(what, &next.edit)[axis]);
            let mut handles = key.curve().unwrap_or(rk_chara::keys::HANDLES);
            let (hx, hy) = if leaves { (0, 1) } else { (2, 3) };
            handles[hx] = ((span.frame(at.x) - start) / (end - start).max(1e-3)).clamp(0.0, 1.0);
            // An axis that does not change between the two keys has no
            // height to read the share from.
            if (high - low).abs() > 1e-3 {
                handles[hy] = (((curves.value_at)(at.y) - low) / (high - low)).clamp(-1.0, 2.0);
            }
            key.ease = Ease::Bezier;
            key.handles = Some(handles);
        }
        _ => return,
    }
    st.doc.edit_sequence(sequence, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_lifted_changes_one_number() {
        let turn = BoneEdit {
            rot: rk_chara::scene::euler_deg(10.0, 20.0, 30.0),
            ..BoneEdit::default()
        };
        let lifted = numbers(Of::Turn, &nudged(Of::Turn, turn, 1, 5.0));
        for (got, want) in lifted.iter().zip([10.0, 25.0, 30.0]) {
            assert!((got - want).abs() < 1e-3, "{lifted:?}");
        }
        let value = rk_chara::channels::as_edit(0.25);
        assert_eq!(numbers(Of::Value, &nudged(Of::Value, value, 0, 0.5)), [0.75]);
        let place = nudged(Of::Place, BoneEdit::default(), 2, 12.0);
        assert!((place.pos.z - 0.12).abs() < 1e-6);
    }
}
