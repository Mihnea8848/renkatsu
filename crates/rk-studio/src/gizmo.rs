// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Working in the viewport with the pointer: selecting objects by a click
//! or a box, and the transform gizmos that move, rotate and scale what is
//! selected, in the manner of Unreal 5's.
//!
//! A gizmo is built in points of the screen around its pivot, so it keeps
//! one size at any distance. The same measures give the solids that
//! `gizmo_pass` draws and the shapes the pointer is tested against. A drag
//! works from the transforms the objects had when it started, so that it
//! can be snapped, shown as one number and called off.
//!
//! The grid on the ground and the drop onto the surface below are here
//! too: they belong to the same block of the viewport's bar.

use crate::gizmo_pass::{Mesh, Vertex};
use crate::keymap::Action;
use crate::kinds::Kind;
use crate::scene::{clean, Object, ObjectId, Transform};
use crate::state::{Look, State};
use crate::theme::Theme;
use crate::widgets::{self, bar_divider, flat_button, glyph, icon_button};
use clap::ValueEnum;
use egui::{Align2, Color32, PointerButton, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};
use egui_phosphor::regular as icon;
use glam::{EulerRot, Mat3, Mat4, Quat, Vec3};
use std::f32::consts::{FRAC_PI_2, TAU};

/// The axes the handles follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Space {
    #[default]
    Global,
    Local,
}

/// The point the handles sit at and things turn and scale around.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Pivot {
    /// Each object's own origin; the handles sit at the first one's.
    #[default]
    Origin,
    /// The centre of what is selected.
    Centre,
}

/// A part of a gizmo that can be dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handle {
    /// Along an axis, or around it.
    Axis(usize),
    /// Along the two other axes at once.
    Plane(usize),
    /// Move in the plane of the screen, or scale all three axes.
    Centre,
}

/// The transform tools, as `State::tool` numbers them.
const MOVE: usize = 2;
const ROTATE: usize = 3;
const SCALE: usize = 4;

/// A row of the Snap menu.
struct Step {
    glyph: &'static str,
    name: &'static str,
    /// The steps offered as a quick choice.
    presets: &'static [f32],
    unit: &'static str,
    /// The smallest and the largest step.
    range: (f32, f32),
    /// How far a point of dragging changes the step.
    speed: f32,
    decimals: usize,
}

/// The rows of the Snap menu, in the order of the settings' steps.
const STEPS: [Step; 4] = [
    Step {
        glyph: icon::ARROWS_OUT_CARDINAL,
        name: "Move",
        presets: &[0.01, 0.05, 0.1, 0.25, 0.5, 1.0],
        unit: "m",
        range: (0.01, 100.0),
        speed: 0.005,
        decimals: 2,
    },
    Step {
        glyph: icon::ARROWS_CLOCKWISE,
        name: "Rotate",
        presets: &[1.0, 5.0, 15.0, 30.0, 45.0, 90.0],
        unit: "°",
        range: (0.1, 180.0),
        speed: 0.25,
        decimals: 1,
    },
    Step {
        glyph: icon::ARROWS_OUT,
        name: "Scale",
        presets: &[0.01, 0.05, 0.1, 0.25, 0.5],
        unit: "",
        range: (0.01, 10.0),
        speed: 0.005,
        decimals: 2,
    },
    Step {
        glyph: icon::GRID_FOUR,
        name: "Grid size",
        presets: &[0.1, 0.25, 0.5, 1.0],
        unit: "m",
        range: (0.01, 100.0),
        speed: 0.005,
        decimals: 2,
    },
];

/// What the viewport's pointer work keeps between frames.
#[derive(Default)]
pub struct Gizmos {
    pub space: Space,
    pub pivot: Pivot,
    /// Whether the grid on the ground shows.
    pub grid: bool,
    hover: Option<Handle>,
    drag: Option<Drag>,
    /// Where a selection box was started.
    band: Option<Pos2>,
    /// The icons drawn last frame, which clicks pick from.
    shown: Vec<Shown>,
    /// Whether each icon was behind something, for the camera's eye and
    /// the icon's place it was worked out for.
    behind: std::collections::HashMap<ObjectId, (Vec3, Vec3, bool)>,
}

/// Length of an axis handle in points, and the colours.
const L: f32 = 108.0;
const AXES: [[f32; 3]; 3] = [[0.84, 0.17, 0.08], [0.41, 0.70, 0.05], [0.16, 0.47, 0.94]];
const HOT: [f32; 3] = [1.0, 0.92, 0.0];
const PLAIN: [f32; 3] = [0.94, 0.94, 0.94];

/// The viewport's camera, for going between the world and the screen.
struct View {
    rect: Rect,
    eye: Vec3,
    forward: Vec3,
    fov: f32,
    view_proj: Mat4,
    inverse: Mat4,
}

impl View {
    fn of(camera: &rk_render::Camera, rect: Rect) -> View {
        let distance = (camera.eye - camera.target).length();
        let fov = camera.fov_y_deg.to_radians();
        // The renderer's own view: left handed, Y up.
        let view_proj = Mat4::perspective_lh(
            fov,
            rect.width() / rect.height(),
            (distance * 0.05).max(0.01),
            distance * 4.0 + 10.0,
        ) * Mat4::look_at_lh(camera.eye, camera.target, Vec3::Y);
        View {
            rect,
            eye: camera.eye,
            forward: (camera.target - camera.eye).normalize_or_zero(),
            fov,
            view_proj,
            inverse: view_proj.inverse(),
        }
    }

    /// Where a point of the world is on the screen, unless it is behind
    /// the camera.
    fn screen(&self, world: Vec3) -> Option<Pos2> {
        let clip = self.view_proj * world.extend(1.0);
        (clip.w > 1e-4).then(|| {
            Pos2::new(
                self.rect.left() + (clip.x / clip.w * 0.5 + 0.5) * self.rect.width(),
                self.rect.top() + (0.5 - clip.y / clip.w * 0.5) * self.rect.height(),
            )
        })
    }

    /// The line through `origin` along `axis` as it crosses the screen:
    /// the part of it that lies before the camera.
    fn line(&self, origin: Vec3, axis: Vec3) -> Option<[Pos2; 2]> {
        const FAR: f32 = 1000.0;
        const NEAR: f32 = 0.05;
        let depth = |p: Vec3| (self.view_proj * p.extend(1.0)).w;
        let (a, b) = (origin - axis * FAR, origin + axis * FAR);
        let (da, db) = (depth(a), depth(b));
        if da < NEAR && db < NEAR {
            return None;
        }
        // Where the line passes behind the camera it ends just before it.
        let cut = a + (b - a) * ((NEAR - da) / (db - da));
        let a = if da < NEAR { cut } else { a };
        let b = if db < NEAR { cut } else { b };
        Some([self.screen(a)?, self.screen(b)?])
    }

    /// The ray into the world under a point of the screen: where it
    /// starts and which way it goes.
    fn ray(&self, pos: Pos2) -> (Vec3, Vec3) {
        let x = (pos.x - self.rect.left()) / self.rect.width() * 2.0 - 1.0;
        let y = 1.0 - (pos.y - self.rect.top()) / self.rect.height() * 2.0;
        let far = self.inverse.project_point3(Vec3::new(x, y, 1.0));
        (self.eye, (far - self.eye).normalize_or_zero())
    }

    /// World units per point of the screen at a place in the world.
    fn unit_at(&self, world: Vec3) -> f32 {
        let depth = (world - self.eye).dot(self.forward).max(0.01);
        2.0 * depth * (self.fov * 0.5).tan() / self.rect.height()
    }
}

/// Where a gizmo sits: its pivot, its axes and its size.
#[derive(Clone, Copy)]
struct Frame {
    origin: Vec3,
    axes: [Vec3; 3],
    /// World units per point of the screen at the pivot.
    unit: f32,
}

impl Frame {
    /// A place given in points around the pivot, in the world.
    fn at(&self, v: Vec3) -> Vec3 {
        self.origin + v * self.unit
    }

    /// The two other axes of an axis, turned to the side of the camera:
    /// a rotate arc sits in the corner that faces the viewer.
    fn facing(&self, axis: usize, view: &View) -> (Vec3, Vec3) {
        let towards = view.eye - self.origin;
        let turned = |a: Vec3| if a.dot(towards) < 0.0 { -a } else { a };
        (turned(self.axes[(axis + 1) % 3]), turned(self.axes[(axis + 2) % 3]))
    }
}

/// An object as a drag found it.
struct Start {
    object: Object,
    /// What carries the object's own transform into the scene.
    parent: Mat4,
    parent_turn: Quat,
    /// Where the object is in the scene.
    place: Vec3,
}

/// How far a drag has gone.
#[derive(Clone, Copy)]
enum Amount {
    /// World units.
    Move(Vec3),
    /// Radians around the dragged axis.
    Turn(f32),
    /// Times along each of the gizmo's axes.
    Scale(Vec3),
}

/// A handle being dragged.
struct Drag {
    tool: usize,
    handle: Handle,
    /// The gizmo as the drag found it.
    frame: Frame,
    /// A move along the scene's own axes, which snaps to world positions.
    on_grid: bool,
    pivot: Pivot,
    starts: Vec<Start>,
    press: Pos2,
    /// Where on the dragged axis the pointer took hold, in world units.
    along: f32,
    /// Where on the dragged plane the pointer took hold.
    held: Vec3,
    /// From the pivot to where a rotate arc was taken hold of.
    spoke: Vec3,
    /// The way along the arc from there, on the screen.
    tangent: Vec2,
    amount: Amount,
    /// The amount in words, shown beside the gizmo.
    readout: String,
}

/// Where a ray comes nearest to a line through `origin` along `axis`, as
/// a distance along the line.
fn along_axis(ray: (Vec3, Vec3), origin: Vec3, axis: Vec3) -> Option<f32> {
    let (eye, dir) = ray;
    let r = origin - eye;
    let ad = axis.dot(dir);
    let denom = 1.0 - ad * ad;
    (denom > 1e-4).then(|| (ad * dir.dot(r) - axis.dot(r)) / denom)
}

/// Where a ray meets the plane through `origin` that faces `normal`.
fn on_plane(ray: (Vec3, Vec3), origin: Vec3, normal: Vec3) -> Option<Vec3> {
    let (eye, dir) = ray;
    let facing = dir.dot(normal);
    if facing.abs() < 1e-3 {
        return None;
    }
    let t = (origin - eye).dot(normal) / facing;
    (t > 0.0).then(|| eye + dir * t)
}

fn snapped(value: f32, step: Option<f32>) -> f32 {
    match step {
        Some(step) if step > 0.0 => (value / step).round() * step,
        _ => value,
    }
}

/// How to snap a drag: by a step from where it started, and, for a move
/// along the scene's axes, to a grid of the scene instead.
#[derive(Clone, Copy, Default)]
struct Snap {
    step: Option<f32>,
    grid: Option<f32>,
}

impl Snap {
    /// A move of `by` from `from` along one axis, snapped: to world
    /// positions in steps of `grid` when it is set, else by the step.
    fn moved(&self, from: f32, by: f32) -> f32 {
        match self.grid {
            Some(cell) => snapped(from + by, Some(cell)) - from,
            None => snapped(by, self.step),
        }
    }
}

impl Drag {
    /// Takes hold of a handle where the pointer pressed.
    fn begin(tool: usize, handle: Handle, frame: Frame, pivot: Pivot, view: &View, press: Pos2, st: &State) -> Drag {
        let scene = st.doc.scene();
        let starts = st
            .doc
            .selected_all()
            .into_iter()
            .filter_map(|id| {
                let object = scene.get(id)?.clone();
                let parent = scene.parent_world(id);
                Some(Start {
                    parent,
                    parent_turn: parent.to_scale_rotation_translation().1,
                    place: parent.transform_point3(object.transform.position.into()),
                    object,
                })
            })
            .collect();
        let ray = view.ray(press);
        let mut drag = Drag {
            tool,
            handle,
            frame,
            on_grid: tool == MOVE && frame.axes == [Vec3::X, Vec3::Y, Vec3::Z],
            pivot,
            starts,
            press,
            along: 0.0,
            held: frame.origin,
            spoke: Vec3::X,
            tangent: Vec2::X,
            amount: Amount::Move(Vec3::ZERO),
            readout: String::new(),
        };
        match handle {
            Handle::Axis(i) if tool == ROTATE => {
                // The point of the arc nearest to the pointer.
                let (a, b) = frame.facing(i, view);
                let nearest = (0..=32)
                    .map(|step| {
                        let (s, c) = (step as f32 / 32.0 * FRAC_PI_2).sin_cos();
                        a * c + b * s
                    })
                    .min_by(|p, q| {
                        let far = |v: &Vec3| {
                            view.screen(frame.at(*v * L))
                                .map_or(f32::MAX, |at| at.distance_sq(press))
                        };
                        far(p).total_cmp(&far(q))
                    });
                drag.spoke = nearest.unwrap_or(a);
                let side = frame.axes[i].cross(drag.spoke);
                let (from, to) = (
                    view.screen(frame.at(drag.spoke * L)),
                    view.screen(frame.at(drag.spoke * L + side * 20.0)),
                );
                if let (Some(from), Some(to)) = (from, to) {
                    drag.tangent = (to - from).normalized();
                }
            }
            Handle::Axis(i) => drag.along = along_axis(ray, frame.origin, frame.axes[i]).unwrap_or(0.0),
            Handle::Plane(n) => drag.held = on_plane(ray, frame.origin, frame.axes[n]).unwrap_or(frame.origin),
            Handle::Centre => drag.held = on_plane(ray, frame.origin, view.forward).unwrap_or(frame.origin),
        }
        drag
    }

    /// Works out how far the drag has gone with the pointer at `pos`,
    /// snapped as `snap` says.
    fn follow(&mut self, view: &View, pos: Pos2, snap: Snap) {
        let frame = &self.frame;
        let step = snap.step;
        let snap = Snap {
            grid: snap.grid.filter(|_| self.on_grid),
            ..snap
        };
        let ray = view.ray(pos);
        let pulled = pos - self.press;
        match (self.tool, self.handle) {
            (MOVE, Handle::Axis(i)) => {
                let Some(t) = along_axis(ray, frame.origin, frame.axes[i]) else {
                    return;
                };
                let t = snap.moved(frame.origin.dot(frame.axes[i]), t - self.along);
                self.amount = Amount::Move(frame.axes[i] * t);
                self.readout = format!("{t:.2} m");
            }
            (MOVE, handle) => {
                let normal = match handle {
                    Handle::Plane(n) => frame.axes[n],
                    _ => view.forward,
                };
                let Some(hit) = on_plane(ray, frame.origin, normal) else {
                    return;
                };
                let mut delta = hit - self.held;
                if let Handle::Plane(n) = handle {
                    // Each of the plane's two axes snaps by itself.
                    let (a, b) = (frame.axes[(n + 1) % 3], frame.axes[(n + 2) % 3]);
                    let u = snap.moved(frame.origin.dot(a), delta.dot(a));
                    let v = snap.moved(frame.origin.dot(b), delta.dot(b));
                    delta = a * u + b * v;
                    self.readout = format!("{u:.2}, {v:.2} m");
                } else {
                    if snap.grid.is_some() {
                        // Across the screen it goes from one crossing of
                        // the grid to another.
                        let to = |i: usize| snap.moved(frame.origin[i], delta[i]);
                        delta = Vec3::new(to(0), to(1), to(2));
                    }
                    self.readout = format!("{:.2} m", delta.length());
                }
                self.amount = Amount::Move(delta);
            }
            (ROTATE, _) => {
                // The pointer's way along the arc, as if the arc were
                // rolled out flat: one radius of travel is one radian.
                let angle = snapped((pulled.dot(self.tangent) / L).to_degrees(), step);
                self.amount = Amount::Turn(angle.to_radians());
                self.readout = format!("{angle:.1}°");
            }
            (_, Handle::Axis(i)) => {
                let Some(t) = along_axis(ray, frame.origin, frame.axes[i]) else {
                    return;
                };
                // One length of the handle doubles the size.
                let factor = snapped(1.0 + (t - self.along) / (L * frame.unit), step).max(0.01);
                let mut scale = Vec3::ONE;
                scale[i] = factor;
                self.amount = Amount::Scale(scale);
                self.readout = format!("{factor:.2}");
            }
            (_, Handle::Plane(n)) => {
                let Some(hit) = on_plane(ray, frame.origin, frame.axes[n]) else {
                    return;
                };
                let (a, b) = (frame.axes[(n + 1) % 3], frame.axes[(n + 2) % 3]);
                let out = (hit - self.held).dot((a + b).normalize_or_zero());
                let factor = snapped(1.0 + out / (L * frame.unit), step).max(0.01);
                let mut scale = Vec3::splat(factor);
                scale[n] = 1.0;
                self.amount = Amount::Scale(scale);
                self.readout = format!("{factor:.2}");
            }
            (_, Handle::Centre) => {
                // To the right or up grows, to the left or down shrinks.
                let factor = snapped(1.0 + (pulled.x - pulled.y) / L, step).max(0.01);
                self.amount = Amount::Scale(Vec3::splat(factor));
                self.readout = format!("{factor:.2}");
            }
        }
    }

    /// The transform an object has at this point of the drag.
    fn moved(&self, start: &Start) -> Transform {
        let frame = &self.frame;
        let mut t = start.object.transform;
        let around = match self.pivot {
            Pivot::Origin => start.place,
            Pivot::Centre => frame.origin,
        };
        let back = start.parent.inverse();
        let own = t.quat();
        let place = match self.amount {
            Amount::Move(delta) => start.place + delta,
            Amount::Turn(angle) => {
                let Handle::Axis(i) = self.handle else { return t };
                let turn = Quat::from_axis_angle(frame.axes[i], angle);
                let now = (start.parent_turn.inverse() * turn * start.parent_turn * own).normalize();
                let (y, x, z) = now.to_euler(EulerRot::YXZ);
                t.rotation = clean(Vec3::new(x, y, z) * (180.0 / std::f32::consts::PI));
                around + turn * (start.place - around)
            }
            Amount::Scale(scale) => {
                // The stretch along the gizmo's axes, as one matrix; each
                // of the object's own axes grows by how much it is
                // stretched.
                let axes = Mat3::from_cols(frame.axes[0], frame.axes[1], frame.axes[2]);
                let stretch = axes * Mat3::from_diagonal(scale) * axes.transpose();
                let turn = start.parent_turn * own;
                let grown = [Vec3::X, Vec3::Y, Vec3::Z].map(|axis| (stretch * (turn * axis)).length());
                t.scale = clean(Vec3::from(t.scale) * Vec3::from(grown));
                around + stretch * (start.place - around)
            }
        };
        t.position = clean(back.transform_point3(place));
        t
    }

    /// The gizmo as it is drawn during the drag: it goes with a move.
    fn shown(&self) -> Frame {
        let mut frame = self.frame;
        if let Amount::Move(delta) = self.amount {
            frame.origin += delta;
        }
        frame
    }
}

/// The objects that are picked by an icon, as they have no picture of
/// their own, and where they are in the world.
fn icons(st: &State) -> Vec<(ObjectId, Kind, Vec3)> {
    if st.icons_hidden {
        return Vec::new();
    }
    let scene = st.doc.scene();
    scene
        .objects()
        .iter()
        .filter(|o| matches!(o.kind(), Kind::Light | Kind::Camera | Kind::Item | Kind::Map) && scene.shown(o.id))
        // A map's pieces and doors are seen as themselves.
        .filter(|o| {
            !matches!(
                o.content,
                crate::scene::Content::MapPiece { .. } | crate::scene::Content::MapDoor { .. }
            )
        })
        .map(|o| (o.id, o.kind(), scene.world(o.id).w_axis.truncate()))
        .collect()
}

/// The middle of an object: of a character its body, of anything else
/// its origin.
fn centre(st: &State, id: ObjectId) -> Vec3 {
    match st.live.as_ref().and_then(|live| live.subject(id)) {
        Some(subject) => subject.center,
        None => st.doc.scene().world(id).w_axis.truncate(),
    }
}

/// Where the gizmo of a tool sits for what is selected.
fn frame(st: &State, view: &View, tool: usize) -> Option<Frame> {
    let all = st.doc.selected_all();
    let first = *all.first()?;
    let scene = st.doc.scene();
    let origin = match st.gizmo.pivot {
        Pivot::Origin => scene.world(first).w_axis.truncate(),
        Pivot::Centre => all.iter().map(|id| centre(st, *id)).sum::<Vec3>() / all.len() as f32,
    };
    if (origin - view.eye).dot(view.forward) < 0.02 {
        return None;
    }
    // A scale goes along the object's own axes whatever the choice: a
    // transform cannot hold a stretch along any other.
    let turn = if st.gizmo.space == Space::Local || tool == SCALE {
        scene.world(first).to_scale_rotation_translation().1
    } else {
        Quat::IDENTITY
    };
    Some(Frame {
        origin,
        axes: [turn * Vec3::X, turn * Vec3::Y, turn * Vec3::Z],
        unit: view.unit_at(origin),
    })
}

/// The measures of the handles, in parts of the handle's length: shared
/// by what is drawn and what the pointer is tested against.
mod measure {
    /// Where a move axis starts, where its cone starts, the cone's radius.
    pub const MOVE_AXIS: (f32, f32, f32) = (0.12, 0.76, 0.075);
    /// From where to where a move plane's corner reaches along its axes.
    pub const MOVE_PLANE: (f32, f32) = (0.1, 0.3);
    pub const BALL: f32 = 0.05;
    /// Half the side of a scale cube.
    pub const CUBE: f32 = 0.07;
    /// Where on its axes a scale plane's line ends.
    pub const SCALE_PLANE: f32 = 0.5;
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 {
        ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

fn inside(p: Pos2, corners: [Pos2; 4]) -> bool {
    let side = |a: Pos2, b: Pos2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let sides = [0, 1, 2, 3].map(|i| side(corners[i], corners[(i + 1) % 4]));
    sides.iter().all(|s| *s >= 0.0) || sides.iter().all(|s| *s <= 0.0)
}

/// The handle of a tool's gizmo under a point of the screen.
fn hit(tool: usize, frame: &Frame, view: &View, pos: Pos2) -> Option<Handle> {
    use measure::*;
    let at = |v: Vec3| view.screen(frame.at(v * L));
    let origin = at(Vec3::ZERO)?;
    let line = |a: Vec3, b: Vec3| match (at(a), at(b)) {
        (Some(a), Some(b)) => distance_to_segment(pos, a, b),
        _ => f32::MAX,
    };
    let nearest = |distances: [f32; 3], reach: f32| {
        (0..3)
            .filter(|i| distances[*i] < reach)
            .min_by(|a, b| distances[*a].total_cmp(&distances[*b]))
    };
    let axes = frame.axes;
    if tool == ROTATE {
        let arcs = [0, 1, 2].map(|i| {
            let (a, b) = frame.facing(i, view);
            (0..16)
                .map(|step| {
                    let point = |step: i32| {
                        let (s, c) = (step as f32 / 16.0 * FRAC_PI_2).sin_cos();
                        a * c + b * s
                    };
                    line(point(step), point(step + 1))
                })
                .fold(f32::MAX, f32::min)
        });
        return nearest(arcs, 8.0).map(Handle::Axis);
    }
    if origin.distance(pos) < 10.0 {
        return Some(Handle::Centre);
    }
    let from = if tool == MOVE { MOVE_AXIS.0 } else { 2.0 * CUBE };
    if let Some(i) = nearest([0, 1, 2].map(|i| line(axes[i] * from, axes[i])), 7.0) {
        return Some(Handle::Axis(i));
    }
    (0..3)
        .find(|n| {
            let (a, b) = (axes[(n + 1) % 3], axes[(n + 2) % 3]);
            if tool == MOVE {
                let (lo, hi) = MOVE_PLANE;
                let corners = [a * lo + b * lo, a * hi + b * lo, a * hi + b * hi, a * lo + b * hi].map(at);
                match corners {
                    [Some(c0), Some(c1), Some(c2), Some(c3)] => inside(pos, [c0, c1, c2, c3]),
                    _ => false,
                }
            } else {
                line(a * SCALE_PLANE, b * SCALE_PLANE) < 6.0
            }
        })
        .map(Handle::Plane)
}

/// Builds the solids of a gizmo out of tubes, cones, cubes and balls.
struct Solids<'a> {
    frame: &'a Frame,
    mesh: Mesh,
}

impl Solids<'_> {
    fn corner(&self, at: Vec3, normal: Vec3, color: [f32; 4]) -> Vertex {
        let p = self.frame.at(at);
        let [r, g, b, a] = color;
        [p.x, p.y, p.z, normal.x, normal.y, normal.z, r, g, b, a]
    }

    /// One triangle; `glass` puts it with the see-through parts.
    fn triangle(&mut self, corners: [(Vec3, Vec3); 3], color: [f32; 4], glass: bool) {
        let made = corners.map(|(at, normal)| self.corner(at, normal, color));
        if glass {
            self.mesh.glass.extend(made);
        } else {
            self.mesh.solid.extend(made);
        }
    }

    /// A band between two rings of points that have the same count.
    fn band(&mut self, a: &[(Vec3, Vec3)], b: &[(Vec3, Vec3)], color: [f32; 4]) {
        for i in 0..a.len() {
            let j = (i + 1) % a.len();
            self.triangle([a[i], a[j], b[i]], color, false);
            self.triangle([a[j], b[j], b[i]], color, false);
        }
    }

    /// A ring of points around `centre`, across `axis`, with the way each
    /// faces.
    fn ring(centre: Vec3, axis: Vec3, radius: f32, sides: usize) -> Vec<(Vec3, Vec3)> {
        let (a, b) = axis.normalize_or_zero().any_orthonormal_pair();
        (0..sides)
            .map(|i| {
                let (s, c) = (i as f32 / sides as f32 * TAU).sin_cos();
                let out = a * c + b * s;
                (centre + out * radius, out)
            })
            .collect()
    }

    fn tube(&mut self, from: Vec3, to: Vec3, radius: f32, color: [f32; 4]) {
        let (a, b) = (
            Self::ring(from, to - from, radius, 8),
            Self::ring(to, to - from, radius, 8),
        );
        self.band(&a, &b, color);
    }

    fn cone(&mut self, base: Vec3, tip: Vec3, radius: f32, color: [f32; 4]) {
        let axis = (tip - base).normalize_or_zero();
        let ring = Self::ring(base, axis, radius, 20);
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            self.triangle([a, b, (tip, (a.1 + b.1) * 0.5)], color, false);
            self.triangle([(a.0, -axis), (b.0, -axis), (base, -axis)], color, false);
        }
    }

    fn cube(&mut self, centre: Vec3, half: f32, color: [f32; 4]) {
        let axes = self.frame.axes;
        for (i, sign) in [0, 1, 2].into_iter().flat_map(|i| [(i, -1.0), (i, 1.0)]) {
            let normal = axes[i] * sign;
            let (u, v) = (axes[(i + 1) % 3] * half, axes[(i + 2) % 3] * half);
            let face = centre + normal * half;
            let corners = [face - u - v, face + u - v, face + u + v, face - u + v].map(|c| (c, normal));
            self.triangle([corners[0], corners[1], corners[2]], color, false);
            self.triangle([corners[0], corners[2], corners[3]], color, false);
        }
    }

    fn ball(&mut self, centre: Vec3, radius: f32, color: [f32; 4]) {
        let ring = |step: usize| {
            let (s, c) = (step as f32 / 8.0 * std::f32::consts::PI).sin_cos();
            Self::ring(centre + Vec3::Y * c * radius, Vec3::Y, s * radius, 14)
                .into_iter()
                .map(|(p, _)| (p, (p - centre).normalize_or_zero()))
                .collect::<Vec<_>>()
        };
        for step in 0..8 {
            self.band(&ring(step), &ring(step + 1), color);
        }
    }

    /// A tube bent along a circle around the pivot, in the plane of `a`
    /// and `b`, from one angle to another.
    fn arc(&mut self, a: Vec3, b: Vec3, radius: f32, angles: (f32, f32), thick: f32, color: [f32; 4]) {
        let (from, to) = angles;
        let steps = ((to - from).abs() / TAU * 96.0).ceil().max(2.0) as usize;
        let across = a.cross(b).normalize_or_zero();
        let ring = |step: usize| {
            let (s, c) = (from + (to - from) * step as f32 / steps as f32).sin_cos();
            let out = a * c + b * s;
            (0..8)
                .map(|i| {
                    let (s, c) = (i as f32 / 8.0 * TAU).sin_cos();
                    let normal = out * c + across * s;
                    (out * radius + normal * thick, normal)
                })
                .collect::<Vec<_>>()
        };
        for step in 0..steps {
            self.band(&ring(step), &ring(step + 1), color);
        }
    }

    /// A flat, see-through slice of the disc in the plane of `a` and `b`.
    fn slice(&mut self, a: Vec3, b: Vec3, radius: f32, angle: f32, color: [f32; 4]) {
        let steps = (angle.abs() / TAU * 96.0).ceil().max(2.0) as usize;
        let point = |step: usize| {
            let (s, c) = (angle * step as f32 / steps as f32).sin_cos();
            ((a * c + b * s) * radius, Vec3::ZERO)
        };
        for step in 0..steps {
            self.triangle([(Vec3::ZERO, Vec3::ZERO), point(step), point(step + 1)], color, true);
        }
    }
}

fn solid(rgb: [f32; 3]) -> [f32; 4] {
    [rgb[0], rgb[1], rgb[2], 1.0]
}

fn sheer(rgb: [f32; 3], alpha: f32) -> [f32; 4] {
    [rgb[0], rgb[1], rgb[2], alpha]
}

/// The solids of a tool's gizmo. `hot` is the handle under the pointer
/// or in the hand; a rotate being dragged is drawn as a dial instead.
fn build(tool: usize, frame: &Frame, view: &View, hot: Option<Handle>, drag: Option<&Drag>) -> Mesh {
    use measure::*;
    let mut s = Solids {
        frame,
        mesh: Mesh::default(),
    };
    let axes = frame.axes;
    let tone = |handle: Handle, rgb: [f32; 3]| solid(if hot == Some(handle) { HOT } else { rgb });
    match (tool, drag) {
        (ROTATE, Some(drag)) => {
            let (Handle::Axis(n), Amount::Turn(angle)) = (drag.handle, drag.amount) else {
                return s.mesh;
            };
            // Only the turned axis stays: a whole dial with a tick every
            // fifteen degrees, the angle filled in, and a spoke at the
            // start and at the angle.
            let (start, side) = (drag.spoke, axes[n].cross(drag.spoke));
            s.arc(start, side, L, (0.0, TAU), 1.4, solid(HOT));
            for tick in 0..24 {
                let (sin, cos) = (tick as f32 / 24.0 * TAU).sin_cos();
                let out = start * cos + side * sin;
                let inner = if tick % 6 == 0 { 0.86 } else { 0.92 };
                s.tube(out * L * inner, out * L, 0.8, solid(HOT));
            }
            let now = start * angle.cos() + side * angle.sin();
            s.tube(Vec3::ZERO, start * L, 0.9, solid(HOT));
            s.tube(Vec3::ZERO, now * L, 0.9, solid(HOT));
            s.slice(start, side, L, angle, sheer(AXES[n], 0.42));
        }
        (ROTATE, None) => {
            // A quarter of each ring as a thick band, in the corner that
            // faces the camera.
            for (i, rgb) in AXES.into_iter().enumerate() {
                let (a, b) = frame.facing(i, view);
                let color = tone(Handle::Axis(i), rgb);
                s.arc(a, b, L, (0.0, FRAC_PI_2), 2.5, color);
                s.slice(a, b, L, FRAC_PI_2, sheer([color[0], color[1], color[2]], 0.14));
            }
        }
        (MOVE, _) => {
            let (from, neck, radius) = MOVE_AXIS;
            for (i, rgb) in AXES.into_iter().enumerate() {
                let color = tone(Handle::Axis(i), rgb);
                s.tube(axes[i] * L * from, axes[i] * L * neck, 1.75, color);
                s.cone(axes[i] * L * neck, axes[i] * L, L * radius, color);
            }
            // Two lines that join two axes in a corner, each in the
            // colour of its axis.
            for n in 0..3 {
                let (i, j) = ((n + 1) % 3, (n + 2) % 3);
                let (a, b) = (axes[i] * L * MOVE_PLANE.1, axes[j] * L * MOVE_PLANE.1);
                s.tube(a, a + b, 1.5, tone(Handle::Plane(n), AXES[i]));
                s.tube(b, a + b, 1.5, tone(Handle::Plane(n), AXES[j]));
                if hot == Some(Handle::Plane(n)) {
                    s.triangle([Vec3::ZERO, a, a + b].map(|p| (p, Vec3::ZERO)), sheer(HOT, 0.25), true);
                    s.triangle([Vec3::ZERO, a + b, b].map(|p| (p, Vec3::ZERO)), sheer(HOT, 0.25), true);
                }
            }
            s.ball(Vec3::ZERO, L * BALL, tone(Handle::Centre, PLAIN));
        }
        _ => {
            for (i, rgb) in AXES.into_iter().enumerate() {
                let color = tone(Handle::Axis(i), rgb);
                s.tube(Vec3::ZERO, axes[i] * L * (1.0 - CUBE), 1.75, color);
                s.cube(axes[i] * L * (1.0 - CUBE), L * CUBE, color);
            }
            // A line from one axis to the other, half in each colour.
            for n in 0..3 {
                let (i, j) = ((n + 1) % 3, (n + 2) % 3);
                let (a, b) = (axes[i] * L * SCALE_PLANE, axes[j] * L * SCALE_PLANE);
                s.tube(a, (a + b) * 0.5, 1.5, tone(Handle::Plane(n), AXES[i]));
                s.tube(b, (a + b) * 0.5, 1.5, tone(Handle::Plane(n), AXES[j]));
            }
            s.cube(Vec3::ZERO, L * CUBE, tone(Handle::Centre, PLAIN));
        }
    }
    s.mesh
}

/// The pointer's work in the viewport for this frame: dragging a handle,
/// drawing a selection box, or selecting by a click. Returns whether the
/// pointer is busy with it, when the camera leaves it alone. Runs before
/// the scene is drawn, so that the picture shows this frame's drag.
pub fn interact(ui: &Ui, st: &mut State, scene: &egui::Response, full: Rect) -> bool {
    let Some(live) = st.live.as_ref() else { return false };
    let view = View::of(&live.cam.orbit.camera(), full);
    let tool = st.tool;
    let (pos, press, primary, secondary, alt, ctrl, escape) = ui.input(|i| {
        (
            i.pointer.latest_pos(),
            i.pointer.press_origin(),
            i.pointer.primary_down(),
            i.pointer.button_pressed(PointerButton::Secondary),
            i.modifiers.alt,
            i.modifiers.command,
            st.keys
                .chords(crate::keymap::Action::CancelDrag)
                .iter()
                .any(|c| i.key_pressed(c.key)),
        )
    });

    if let Some(mut drag) = st.gizmo.drag.take() {
        if escape || secondary || drag.tool != tool {
            st.doc.cancel_gesture();
        } else if !primary {
            st.doc.end_gesture();
        } else {
            // Ctrl turns snapping on for this drag, or off when it is on.
            let row = drag.tool - MOVE;
            let steps = st.settings.snap_steps;
            let snap = Snap {
                step: (st.snap != ctrl).then(|| steps[row]),
                // Along the scene's axes a move lands on world positions in
                // steps of the move step; the grid size is for drawing.
                grid: (st.snap != ctrl).then(|| steps[0]),
            };
            if let Some(pos) = pos {
                drag.follow(&view, pos, snap);
            }
            for start in &drag.starts {
                let mut object = start.object.clone();
                object.transform = drag.moved(start);
                st.doc.edit(object, true);
            }
            st.gizmo.drag = Some(drag);
        }
        if let Some(live) = st.live.as_mut() {
            live.hold_camera = st.gizmo.drag.is_some();
        }
        return true;
    }

    let selecting = (1..=SCALE).contains(&tool);
    let at = if (MOVE..=SCALE).contains(&tool) {
        frame(st, &view, tool)
    } else {
        None
    };
    let over = |pos: Option<Pos2>| at.as_ref().zip(pos).and_then(|(f, pos)| hit(tool, f, &view, pos));
    st.gizmo.hover = if scene.hovered() && st.gizmo.band.is_none() {
        over(pos)
    } else {
        None
    };

    if scene.drag_started_by(PointerButton::Primary) && !alt && selecting {
        let press = press.or(pos).unwrap_or(full.center());
        match (over(Some(press)), at) {
            (Some(handle), Some(at)) => {
                let mut drag = Drag::begin(tool, handle, at, st.gizmo.pivot, &view, press, st);
                drag.follow(&view, press, Snap::default());
                st.gizmo.drag = Some(drag);
                st.gizmo.hover = None;
                if let Some(live) = st.live.as_mut() {
                    live.hold_camera = true;
                }
                return true;
            }
            _ => st.gizmo.band = Some(press),
        }
    }
    if let Some(from) = st.gizmo.band {
        if escape || secondary {
            st.gizmo.band = None;
        } else if !primary {
            st.gizmo.band = None;
            if let Some(pos) = pos {
                let found = boxed(st, &view, Rect::from_two_pos(from, pos));
                let mut all = if ctrl { st.doc.selected_all() } else { Vec::new() };
                for id in found {
                    if !all.contains(&id) {
                        all.push(id);
                    }
                }
                st.doc.select_many(&all);
            }
        }
        return true;
    }
    if scene.clicked_by(PointerButton::Primary) && !alt && selecting && st.gizmo.hover.is_none() {
        match pos.and_then(|pos| pick(st, &view, pos)) {
            Some(id) if ctrl => st.doc.toggle_selected(id),
            Some(id) => st.doc.selected = Some(id),
            None if ctrl => {}
            None => st.doc.selected = None,
        }
    }
    false
}

/// The object under a point of the screen: an icon first, as icons lie
/// over the picture, then the nearest character.
fn pick(st: &State, view: &View, pos: Pos2) -> Option<ObjectId> {
    // The icons drawn: one of a group stands for all of it.
    let icon = st
        .gizmo
        .shown
        .iter()
        .filter(|s| pickable(st, s.id))
        .map(|s| (s.id, s.at.distance(pos), s.size))
        .filter(|(_, distance, size)| *distance < size + 2.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, distance, _)| (id, distance));
    if let Some((id, _)) = icon {
        return Some(id);
    }
    let (origin, dir) = view.ray(pos);
    let live = st.live.as_ref()?;
    let scene = st.doc.scene();
    let character = live.pick(scene, origin, dir);
    // The map in front: its piece or door, or nothing for a wall.
    if let Some((owner, t)) = live.pick_map(scene, origin, dir) {
        if character.is_none_or(|(_, far)| t < far) {
            return owner.filter(|id| pickable(st, *id));
        }
    }
    character.map(|(id, _)| id).filter(|id| pickable(st, *id))
}

/// Whether the Select mode's filter lets an object be picked: lights and
/// cameras go together.
fn pickable(st: &State, id: ObjectId) -> bool {
    let Some(filter) = st.pick_filter else { return true };
    let Some(kind) = st.doc.scene().get(id).map(|o| o.kind()) else {
        return false;
    };
    match filter {
        crate::kinds::Kind::Light => matches!(kind, crate::kinds::Kind::Light | crate::kinds::Kind::Camera),
        other => kind == other,
    }
}

/// The objects a box on the screen touches: a character by any of its
/// bones, anything else by its icon. In the order of the scene.
fn boxed(st: &State, view: &View, area: Rect) -> Vec<ObjectId> {
    let within = |world: Vec3| view.screen(world).is_some_and(|at| area.contains(at));
    let mut found: Vec<ObjectId> = icons(st)
        .into_iter()
        .filter(|(_, _, world)| within(*world))
        .map(|(id, ..)| id)
        .collect();
    if let Some(live) = st.live.as_ref() {
        for (id, bones) in live.bones(st.doc.scene()) {
            if bones.into_iter().any(within) {
                found.push(id);
            }
        }
    }
    found.retain(|id| pickable(st, *id));
    let order = |id: &ObjectId| st.doc.scene().objects().iter().position(|o| o.id == *id);
    found.sort_by_key(order);
    found
}

/// The grid's lines among the design's colours, as the screen shows them.
fn grid_look(k: &Look, cell: f32, camera: &rk_render::Camera) -> crate::grid_pass::Grid {
    let tone = |color: Color32, alpha: f32| {
        let [r, g, b, _] = color.to_array().map(|c| f32::from(c) / 255.0);
        [r, g, b, alpha]
    };
    crate::grid_pass::Grid {
        cell,
        // Far enough to lie under the whole of what the camera frames.
        reach: ((camera.eye - camera.target).length() * 8.0).clamp(12.0, 600.0),
        line: tone(k.t.text_dim, 0.34),
        axis_x: tone(k.t.axis[0], 0.6),
        axis_z: tone(k.t.axis[2], 0.6),
    }
}

/// One icon, `size` its radius and `fade` how much of it shows.
fn icon_at(p: &egui::Painter, k: &Look, at: Pos2, kind: Kind, selected: bool, size: f32, fade: f32) {
    let alpha = |a: u8| (f32::from(a) * fade) as u8;
    p.circle_filled(at, size, Theme::glass(k.t.well, alpha(215)));
    let edge = if selected {
        Stroke::new(1.5, k.t.select_bright)
    } else {
        Stroke::new(1.0, Theme::glass(k.t.hover, alpha(150)))
    };
    p.circle_stroke(at, size, edge);
    glyph(
        p,
        k,
        at,
        kind.glyph(),
        size * 14.0 / 11.0,
        Theme::glass(kind.color(), alpha(255)),
    );
}

/// An icon as the viewport shows it this frame: for its object and the
/// others grouped under it.
#[derive(Debug, Clone, Copy)]
struct Shown {
    id: ObjectId,
    kind: Kind,
    at: Pos2,
    /// Its radius in points, smaller the further it is.
    size: f32,
    /// Something stands between the camera and it.
    behind: bool,
}

/// An icon's radius at full size, and at its smallest.
const ICON_SIZE: f32 = 11.0;
const ICON_SMALLEST: f32 = 6.0;
/// Up to how far an icon keeps its full size, in metres.
const ICON_NEAR: f32 = 4.0;
/// The least distance between two icons' middles, in points: two closer
/// than this are one. At most one icon for each square this wide, about
/// twelve in a hundred points square, whatever their size.
const ICON_ROOM: f32 = 28.0;

/// The icons to draw and pick: sized by distance, faded behind what
/// stands in front of them, and never crowded. Of icons that would
/// stand closer than `ICON_ROOM` one is drawn for all, the selected
/// first, then the hovered, then the nearest; the others join it, and
/// come apart as the camera comes closer. Whether an icon is behind
/// something is worked out
/// again only when it or the camera moves.
fn shown_icons(st: &mut State, view: &View, pointer: Option<Pos2>) -> Vec<Shown> {
    let mut list: Vec<(Shown, f32)> = Vec::new();
    for (id, kind, world) in icons(st) {
        let Some(at) = view.screen(world) else { continue };
        let way = world - view.eye;
        let far = way.length().max(0.01);
        let cached = st
            .gizmo
            .behind
            .get(&id)
            .filter(|(eye, w, _)| *eye == view.eye && *w == world);
        let behind = match cached {
            Some((_, _, behind)) => *behind,
            None => {
                let dir = way / far;
                let scene = st.doc.scene();
                let live = st.live.as_ref();
                let map = live.and_then(|l| l.pick_map(scene, view.eye, dir)).map(|(_, t)| t);
                let chara = live.and_then(|l| l.pick(scene, view.eye, dir)).map(|(_, t)| t);
                let behind = map.into_iter().chain(chara).any(|t| t < far - 0.05);
                st.gizmo.behind.insert(id, (view.eye, world, behind));
                behind
            }
        };
        let size = (ICON_SIZE * ICON_NEAR / far).clamp(ICON_SMALLEST, ICON_SIZE);
        list.push((
            Shown {
                id,
                kind,
                at,
                size,
                behind,
            },
            far,
        ));
    }
    // Who stands for a group: the selected, the hovered, then the nearest.
    let rank = |s: &Shown, far: f32| {
        let hovered = pointer.is_some_and(|p| p.distance(s.at) < s.size + 2.0);
        (!st.doc.is_selected(s.id), !hovered, far)
    };
    list.sort_by(|a, b| {
        let (x, y) = (rank(&a.0, a.1), rank(&b.0, b.1));
        x.0.cmp(&y.0).then(x.1.cmp(&y.1)).then(x.2.total_cmp(&y.2))
    });
    let mut shown: Vec<Shown> = Vec::new();
    for (icon, _) in list {
        let crowded = shown
            .iter()
            .any(|other| other.at.distance(icon.at) < ICON_ROOM.max((other.size + icon.size) * 1.15));
        if !crowded {
            shown.push(icon);
        }
    }
    shown
}

/// Draws the shown icons, the far under the near.
fn draw_icons(p: &egui::Painter, k: &Look, st: &State, shown: &[Shown]) {
    let mut order: Vec<&Shown> = shown.iter().collect();
    order.sort_by(|a, b| a.size.total_cmp(&b.size));
    for icon in order {
        let fade = if icon.behind { 0.3 } else { 1.0 };
        icon_at(p, k, icon.at, icon.kind, st.doc.is_selected(icon.id), icon.size, fade);
    }
}

/// Draws what the pointer works with over the scene's picture: the grid,
/// the icons of the objects that have no picture, the gizmo of the active
/// tool, the amount of a drag beside it, and the selection box.
pub fn draw(ui: &Ui, k: &Look, st: &mut State, full: Rect) {
    let Some(live) = st.live.as_mut() else { return };
    let camera = live.cam.orbit.camera();
    let view = View::of(&camera, full);
    let p = ui.painter().with_clip_rect(full);
    let tool = st.tool;
    let ppp = ui.ctx().pixels_per_point();
    let pixels = [
        (full.width() * ppp).round() as u32,
        (full.height() * ppp).round() as u32,
    ];
    let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    if st.gizmo.grid {
        let grid = grid_look(k, st.settings.snap_steps[3], &camera);
        if let Some(id) = live.paint_grid(pixels, view.view_proj, &grid) {
            p.image(id, full, whole, Color32::WHITE);
        }
    }

    let pointer = ui.ctx().pointer_hover_pos();
    let shown = shown_icons(st, &view, pointer);
    draw_icons(&p, k, st, &shown);
    st.gizmo.shown = shown;

    let shown = match &st.gizmo.drag {
        Some(drag) => Some(drag.shown()),
        None if (MOVE..=SCALE).contains(&tool) => frame(st, &view, tool),
        None => None,
    };
    if let Some(at) = shown {
        let drag = st.gizmo.drag.as_ref();
        let hot = drag.map(|d| d.handle).or(st.gizmo.hover);
        let mesh = build(drag.map_or(tool, |d| d.tool), &at, &view, hot, drag);
        let readout = drag.and_then(|d| {
            // Beside the spoke of a dial, or below the pivot.
            let place = match (d.tool, d.amount, d.handle) {
                (ROTATE, Amount::Turn(angle), Handle::Axis(n)) => {
                    let side = at.axes[n].cross(d.spoke);
                    view.screen(at.at((d.spoke * angle.cos() + side * angle.sin()) * L * 1.2))
                }
                _ => view.screen(at.origin).map(|o| o + Vec2::new(0.0, 30.0)),
            };
            Some((place?, d.readout.clone()))
        });
        // The axis a move or a scale goes along, as a thin line across the
        // whole view, as in Blender.
        if let Some((Handle::Axis(n), MOVE | SCALE)) = drag.map(|d| (d.handle, d.tool)) {
            if let Some(ends) = view.line(at.origin, at.axes[n]) {
                p.line_segment(ends, Stroke::new(1.0, k.t.axis[n]));
            }
        }
        if let Some(live) = st.live.as_mut() {
            let id = live.paint_gizmo(pixels, view.view_proj, &mesh);
            p.image(id, full, whole, Color32::WHITE);
        }
        if let Some((place, text)) = readout {
            let font = k.bold(k.d.text + 1.0);
            let size = widgets::text_width(ui, &text, font.clone());
            let chip = Rect::from_center_size(place, Vec2::new(size + 14.0, k.d.text + 11.0));
            p.rect_filled(chip, k.radius(), Theme::glass(k.t.well, 230));
            p.text(chip.center(), Align2::CENTER_CENTER, text, font, k.t.text_bright);
        }
    }

    if let (Some(from), Some(pos)) = (st.gizmo.band, ui.input(|i| i.pointer.latest_pos())) {
        let area = Rect::from_two_pos(from, pos).intersect(full);
        p.rect_filled(area, 0.0, Theme::glass(k.t.select, 36));
        p.rect_stroke(area, 0.0, Stroke::new(1.0, k.t.select_bright), StrokeKind::Inside);
    }
}

/// Whether the active tool has transform options to show.
pub fn has_options(st: &State) -> bool {
    (MOVE..=SCALE).contains(&st.tool)
}

/// The names and icons of the two axis spaces and the two pivots.
const SPACES: [(&str, &str, Space); 2] = [
    (icon::GLOBE, "Global", Space::Global),
    (icon::CUBE, "Local", Space::Local),
];
const PIVOTS: [(&str, &str, Pivot); 2] = [
    (icon::CROSSHAIR_SIMPLE, "Origin", Pivot::Origin),
    (icon::BOUNDING_BOX, "Centre", Pivot::Centre),
];

/// The transform options from the left, for the block beside the tool
/// strip: the axes, the pivot, snapping with the menu of its steps, and
/// the drop to the surface below.
pub fn options(ui: &mut Ui, k: &Look, st: &mut State) {
    let (g, name, ..) = SPACES[st.gizmo.space as usize];
    let resp = flat_button(ui, k, Some(g), name, true, false).on_hover_text("Axes");
    egui::Popup::menu(&resp).show(|ui| {
        for (_, name, space) in SPACES {
            if widgets::check_item(ui, k, st.gizmo.space == space, name, "").clicked() {
                st.gizmo.space = space;
                ui.close();
            }
        }
    });
    let (g, name, ..) = PIVOTS[st.gizmo.pivot as usize];
    let resp = flat_button(ui, k, Some(g), name, true, false).on_hover_text("Pivot");
    egui::Popup::menu(&resp).show(|ui| {
        for (_, name, pivot) in PIVOTS {
            if widgets::check_item(ui, k, st.gizmo.pivot == pivot, name, "").clicked() {
                st.gizmo.pivot = pivot;
                ui.close();
            }
        }
    });
    bar_divider(ui, k);
    if flat_button(ui, k, Some(icon::MAGNET), "Snap", false, st.snap)
        .on_hover_text(st.keys.tip("Snap", Action::ToggleSnap))
        .clicked()
    {
        st.snap = !st.snap;
    }
    let resp = flat_button(ui, k, None, "", true, false).on_hover_text("Snap steps");
    egui::Popup::menu(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            if widgets::check_item(ui, k, st.snap, "Snap", &st.keys.text(Action::ToggleSnap)).clicked() {
                st.snap = !st.snap;
            }
            ui.separator();
            for (row, step) in STEPS.iter().enumerate() {
                if row == 3 {
                    ui.separator();
                }
                step_row(ui, k, st, row, step);
            }
        });
    bar_divider(ui, k);
    let tip = st.keys.tip("Snap to floor", Action::DropToSurface);
    let any = st.doc.selected.is_some();
    let drop = ui
        .add_enabled_ui(any, |ui| {
            flat_button(ui, k, Some(icon::ARROW_LINE_DOWN), "Snap to floor", false, false)
        })
        .inner;
    if drop.on_hover_text(&tip).on_disabled_hover_text(&tip).clicked() {
        st.run(crate::state::Command::SnapToFloor);
    }
}

/// A row of the Snap menu: the step's name, and its value as a number
/// that is dragged or typed, with the usual steps behind the caret.
fn step_row(ui: &mut Ui, k: &Look, st: &mut State, row: usize, step: &Step) {
    let h = k.field();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(214.0, h + 4.0), egui::Sense::hover());
    let y = rect.center().y;
    let p = ui.painter();
    glyph(p, k, Pos2::new(rect.left() + 11.0, y), step.glyph, 14.0, k.t.text_dim);
    widgets::label(p, k, Pos2::new(rect.left() + 26.0, y), step.name, k.t.text);

    let caret = Rect::from_min_size(Pos2::new(rect.right() - 20.0, y - h / 2.0), Vec2::new(18.0, h));
    let field = Rect::from_min_max(Pos2::new(caret.left() - 74.0, caret.top()), caret.left_bottom());
    let id = ui.id().with(("snap step", row));
    let value = &mut st.settings.snap_steps[row];
    let before = *value;
    widgets::number_to(
        ui,
        k,
        field,
        id,
        value,
        widgets::Marker::None,
        true,
        step.speed,
        step.decimals,
    );
    // The unit follows the number, unless it is being typed.
    let typing = ui.data(|d| d.get_temp::<String>(id.with("typing")).is_some());
    if !typing && !step.unit.is_empty() {
        let shown = format!("{:.*}", step.decimals, *value);
        let x =
            field.left() + 7.0 + widgets::text_width(ui, &shown, k.font()) + if step.unit == "°" { 1.0 } else { 3.0 };
        widgets::label(ui.painter(), k, Pos2::new(x, y), step.unit, k.t.text_dim);
    }
    let resp = ui.interact(caret, id.with("presets"), egui::Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(caret, k.radius(), k.t.control);
    }
    let color = if resp.hovered() { k.t.text_bright } else { k.t.text_dim };
    glyph(p, k, caret.center(), icon::CARET_RIGHT, 10.0, color);
    egui::containers::menu::SubMenu::new().show(ui, &resp, |ui| {
        for preset in step.presets {
            let gap = if step.unit == "°" { "" } else { " " };
            let text = format!("{preset}{gap}{}", step.unit);
            if widgets::check_item(ui, k, (*value - preset).abs() < 1e-4, text.trim(), "").clicked() {
                *value = *preset;
            }
        }
    });
    *value = value.clamp(step.range.0, step.range.1);
    if *value != before {
        st.settings_changed = true;
    }
}

/// The grid's button among the view options.
pub fn grid_button(ui: &mut Ui, k: &Look, st: &mut State) {
    if icon_button(ui, k, icon::GRID_FOUR, st.gizmo.grid)
        .on_hover_text(st.keys.tip("Show the grid", Action::ToggleGrid))
        .clicked()
    {
        st.gizmo.grid = !st.gizmo.grid;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Content, Scene};

    fn view() -> View {
        let camera = rk_render::Camera {
            eye: Vec3::new(0.0, 1.0, -5.0),
            target: Vec3::new(0.0, 1.0, 0.0),
            fov_y_deg: 23.0,
        };
        View::of(&camera, Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)))
    }

    fn frame_at(view: &View, origin: Vec3) -> Frame {
        Frame {
            origin,
            axes: [Vec3::X, Vec3::Y, Vec3::Z],
            unit: view.unit_at(origin),
        }
    }

    fn drag(tool: usize, handle: Handle, pivot: Pivot, object: Object, parent: Mat4, amount: Amount) -> Transform {
        let view = view();
        let frame = frame_at(&view, Vec3::new(0.0, 1.0, 0.0));
        let start = Start {
            parent,
            parent_turn: parent.to_scale_rotation_translation().1,
            place: parent.transform_point3(object.transform.position.into()),
            object,
        };
        let drag = Drag {
            tool,
            handle,
            frame,
            on_grid: false,
            pivot,
            starts: Vec::new(),
            press: Pos2::ZERO,
            along: 0.0,
            held: Vec3::ZERO,
            spoke: Vec3::X,
            tangent: Vec2::X,
            amount,
            readout: String::new(),
        };
        drag.moved(&start)
    }

    fn light() -> Object {
        let mut scene = Scene::empty();
        let id = scene.add("Light", Content::light(), None);
        let mut object = scene.get(id).unwrap().clone();
        object.transform = Transform::default();
        object
    }

    #[test]
    fn the_screen_and_the_world_agree() {
        let view = view();
        let point = Vec3::new(0.4, 1.3, 0.2);
        let at = view.screen(point).unwrap();
        let (eye, dir) = view.ray(at);
        // The ray through a point's place on the screen passes the point.
        let along = (point - eye).dot(dir);
        assert!((eye + dir * along).distance(point) < 1e-3);
        // Behind the camera there is no place on the screen.
        assert!(view.screen(Vec3::new(0.0, 1.0, -9.0)).is_none());
    }

    #[test]
    fn the_pointer_finds_the_handle_it_is_over() {
        let view = view();
        let frame = frame_at(&view, Vec3::new(0.0, 1.0, 0.0));
        let at = |v: Vec3| view.screen(frame.at(v * L)).unwrap();
        assert_eq!(hit(MOVE, &frame, &view, at(Vec3::X * 0.6)), Some(Handle::Axis(0)));
        assert_eq!(hit(MOVE, &frame, &view, at(Vec3::Y * 0.9)), Some(Handle::Axis(1)));
        assert_eq!(hit(MOVE, &frame, &view, at(Vec3::ZERO)), Some(Handle::Centre));
        assert_eq!(
            hit(MOVE, &frame, &view, at(Vec3::new(0.2, 0.2, 0.0))),
            Some(Handle::Plane(2))
        );
        assert_eq!(hit(MOVE, &frame, &view, at(Vec3::new(0.7, 0.7, 0.0))), None);
        assert_eq!(hit(SCALE, &frame, &view, at(Vec3::Y * 0.6)), Some(Handle::Axis(1)));
        assert_eq!(
            hit(SCALE, &frame, &view, at(Vec3::new(0.25, 0.25, 0.0))),
            Some(Handle::Plane(2))
        );
        // The arc around Z joins X and Y in the corner facing the camera.
        let (a, b) = frame.facing(2, &view);
        let on_arc = (a + b).normalize();
        assert_eq!(hit(ROTATE, &frame, &view, at(on_arc)), Some(Handle::Axis(2)));
        assert_eq!(hit(ROTATE, &frame, &view, at(on_arc * 0.5)), None);
    }

    #[test]
    fn a_drag_along_an_axis_moves_by_what_the_pointer_travelled() {
        let view = view();
        let frame = frame_at(&view, Vec3::new(0.0, 1.0, 0.0));
        let at = |x: f32| view.screen(frame.origin + Vec3::X * x).unwrap();
        let st = State::new(crate::project::Project::none(), Scene::new());
        let mut drag = Drag::begin(MOVE, Handle::Axis(0), frame, Pivot::Origin, &view, at(0.2), &st);
        drag.follow(&view, at(0.53), Snap::default());
        let Amount::Move(delta) = drag.amount else { panic!() };
        assert!((delta - Vec3::X * 0.33).length() < 1e-3);
        // Snapped, it goes to the nearest step.
        let by_step = Snap {
            step: Some(0.1),
            grid: None,
        };
        drag.follow(&view, at(0.53), by_step);
        let Amount::Move(delta) = drag.amount else { panic!() };
        assert!((delta - Vec3::X * 0.3).length() < 1e-4);
        assert_eq!(drag.readout, "0.30 m");
    }

    #[test]
    fn a_move_along_the_scenes_axes_lands_on_the_grid() {
        let view = view();
        // The pivot stands off the grid's lines.
        let frame = frame_at(&view, Vec3::new(0.13, 1.0, 0.0));
        let at = |x: f32| view.screen(frame.origin + Vec3::X * x).unwrap();
        let st = State::new(crate::project::Project::none(), Scene::new());
        let to_grid = Snap {
            step: Some(0.1),
            grid: Some(0.25),
        };
        let mut drag = Drag::begin(MOVE, Handle::Axis(0), frame, Pivot::Origin, &view, at(0.0), &st);
        drag.follow(&view, at(0.33), to_grid);
        let Amount::Move(delta) = drag.amount else { panic!() };
        // 0.13 + 0.33 is nearest to the line at 0.5.
        assert!((frame.origin.x + delta.x - 0.5).abs() < 1e-4);
        // Along the object's own axes there is no grid: the step counts.
        let turned = Frame {
            axes: [Vec3::NEG_X, Vec3::Y, Vec3::NEG_Z],
            ..frame
        };
        let mut drag = Drag::begin(MOVE, Handle::Axis(0), turned, Pivot::Origin, &view, at(0.0), &st);
        drag.follow(&view, at(-0.33), to_grid);
        let Amount::Move(delta) = drag.amount else { panic!() };
        assert!((delta + Vec3::X * 0.3).length() < 1e-4);
    }

    #[test]
    fn an_object_in_a_turned_folder_moves_along_the_scenes_axes() {
        // The folder is turned a quarter around Y and stands at x = 2.
        let folder = Transform {
            position: [2.0, 0.0, 0.0],
            rotation: [0.0, 90.0, 0.0],
            scale: [1.0; 3],
        };
        let mut object = light();
        object.transform.position = [1.0, 0.0, 0.0];
        let before = folder.matrix().transform_point3(Vec3::X);
        let t = drag(
            MOVE,
            Handle::Axis(0),
            Pivot::Origin,
            object,
            folder.matrix(),
            Amount::Move(Vec3::X * 0.5),
        );
        let after = folder.matrix().transform_point3(t.position.into());
        assert!((after - before - Vec3::X * 0.5).length() < 1e-3);
        assert_eq!(t.rotation, [0.0; 3]);
    }

    #[test]
    fn a_turn_goes_around_the_chosen_pivot() {
        let mut object = light();
        object.transform.position = [1.0, 1.0, 0.0];
        let quarter = Amount::Turn(FRAC_PI_2);
        // Around its own origin the object turns where it stands.
        let own = drag(
            ROTATE,
            Handle::Axis(1),
            Pivot::Origin,
            object.clone(),
            Mat4::IDENTITY,
            quarter,
        );
        assert_eq!(own.position, [1.0, 1.0, 0.0]);
        assert_eq!(own.rotation, [0.0, 90.0, 0.0]);
        // Around the centre, at (0, 1, 0), it also goes around it.
        let around = drag(ROTATE, Handle::Axis(1), Pivot::Centre, object, Mat4::IDENTITY, quarter);
        assert_eq!(around.rotation, [0.0, 90.0, 0.0]);
        assert_eq!(around.position, [0.0, 1.0, -1.0]);
    }

    #[test]
    fn a_scale_along_one_axis_leaves_the_others() {
        let mut object = light();
        object.transform.scale = [1.0, 2.0, 1.0];
        let t = drag(
            SCALE,
            Handle::Axis(0),
            Pivot::Origin,
            object,
            Mat4::IDENTITY,
            Amount::Scale(Vec3::new(1.5, 1.0, 1.0)),
        );
        assert_eq!(t.scale, [1.5, 2.0, 1.0]);
    }
}
