// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The graph editor: the chosen bone's or value's keys as curves over
//! the frames, a curve each axis, with the keys as points and their
//! handles. The points are selected and dragged along the frames as the
//! sheet's marks are.

use super::sheet::{self, Mark};
use super::time::Span;
use super::{frame_of, KeyRef};
use crate::scene::ObjectId;
use crate::state::{Look, State};
use crate::theme::Theme;
use crate::widgets::hair_h;
use egui::{Align2, Color32, Id, Pos2, Rect, Stroke, Ui};
use rk_chara::keys::{Ease, Key};
use rk_chara::pose::BoneEdit;

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

/// A name's keys with what its curves are of: each key, its frame and
/// whether it is selected.
type Curve = (String, Of, Vec<(Key, i32, bool)>);

/// The bones and values whose curves show: the chosen one when it has
/// keys, else those of the selected keys.
pub fn plotted(st: &State, object: ObjectId) -> Vec<String> {
    let Some(track) = st.doc.scene().sequence.track(object) else {
        return Vec::new();
    };
    if let Some(name) = super::chosen(st).filter(|n| track.keys.contains_key(n)) {
        return vec![name];
    }
    let mut names: Vec<String> = st
        .anim
        .selected
        .iter()
        .filter(|key| key.0 == object && track.keys.contains_key(&key.1))
        .map(|key| key.1.clone())
        .collect();
    names.dedup();
    names.truncate(4);
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

/// Draws the curves of a character's chosen keys in `rect` and takes the
/// pointer's work on their points.
pub fn show(ui: &mut Ui, k: &Look, st: &mut State, span: &Span, rect: Rect, object: ObjectId, id: Id) {
    let t = &k.t;
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 0.0, t.well);
    let names = plotted(st, object);
    let fps = span.fps;
    // Each name's keys, the selected ones where the drag has them.
    let curves: Vec<Curve> = names
        .iter()
        .filter_map(|name| {
            let list = st.doc.scene().sequence.track(object)?.keys.get(name)?;
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
        sheet::interact(ui, k, st, span, rect, id, &[], 6.0);
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
            if let Some(edit) = rk_chara::keys::sample_named(name, &list, time) {
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
    let (low, high) = (low - room, high + room);
    let y_of = |value: f32| rect.bottom() - (value - low) / (high - low) * rect.height();
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
    // The keys: a point each curve, with the handles its ease gives.
    let mut marks = Vec::new();
    let mut line = 0;
    for (name, what, keys) in &curves {
        let count = numbers(*what, &BoneEdit::default()).len();
        for axis in 0..count {
            let color = lines[line + axis].0;
            let at = |key: &Key| Pos2::new(span.x(key.time * fps) + 0.5, y_of(numbers(*what, &key.edit)[axis]));
            for (i, (key, frame, chosen)) in keys.iter().enumerate() {
                let here = at(key);
                let tint = if *chosen {
                    Color32::WHITE
                } else {
                    Theme::glass(color, 150)
                };
                let handle = |to: Pos2| {
                    p.line_segment([here, to], Stroke::new(1.0, tint));
                    p.circle_filled(to, 2.5, tint);
                };
                // Out of the key, by its own ease; into it, by the ease of
                // the key before.
                if let Some((next, ..)) = keys.get(i + 1) {
                    let reach = (at(next).x - here.x) / 3.0;
                    match key.ease {
                        Ease::Smooth | Ease::EaseIn | Ease::Bezier => handle(Pos2::new(here.x + reach, here.y)),
                        _ => {}
                    }
                }
                if let Some((before, ..)) = i.checked_sub(1).and_then(|b| keys.get(b)) {
                    let reach = (here.x - at(before).x) / 3.0;
                    if matches!(before.ease, Ease::Smooth | Ease::EaseOut | Ease::Bezier) {
                        handle(Pos2::new(here.x - reach, here.y));
                    }
                }
                let fill = if *chosen { Color32::WHITE } else { color };
                let line = if *chosen {
                    t.select_bright
                } else {
                    Color32::from_black_alpha(200)
                };
                sheet::diamond(&p, here, if *chosen { 5.0 } else { 4.0 }, fill, line);
                let key: KeyRef = (object, name.clone(), *frame);
                marks.push(Mark {
                    at: here,
                    keys: vec![key],
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
    let shown = super::rows(st, false)
        .into_iter()
        .find(|row| row.object == object && row.name == first.0)
        .map_or_else(|| first.0.clone(), |row| row.label);
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
    sheet::interact(ui, k, st, span, rect, id, &marks, 6.0);
}
