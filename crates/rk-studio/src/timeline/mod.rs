// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Timeline: the scene's sequence on one clock, a track an object. A
//! character's track opens into her clip, the clip baked to keys and the
//! keys by hand, then the parts of her body and their bones; the Graph
//! switch shows the selected keys' curves in the sheet's place. A thin
//! slider of the same frames lies under the viewport, and the Animator
//! lists the same rows without their keys. What they share is here: the
//! rows, and what the panels remember.

pub mod graph;
pub mod sheet;
pub mod slider;
pub mod time;
pub mod tracks;

use crate::scene::{Content, ObjectId};
use crate::sequence::BAKED;
use crate::state::{Look, State};
use crate::widgets::{self, glyph, hair_h};
use egui::{Color32, Id, Pos2, Rect, Sense, Ui, Vec2};
use egui_phosphor::regular as icon;
use rk_chara::keys::Key;
use std::collections::{BTreeMap, BTreeSet};

/// A key: whose, of which bone or value (`baked/BONE` for a key of the
/// baked clip), and on which frame.
pub type KeyRef = (ObjectId, String, i32);

/// What a drag on the keys does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragKind {
    /// Moves the selected keys, or copies of them.
    Move { copy: bool },
    /// Scales the selected keys' times around the playhead.
    Scale,
    /// Selects the keys in a box, from this corner.
    Select(Pos2),
    /// Moves the selected keys of a curve along the frames and, on the
    /// axis its point is of, in value.
    Point { axis: usize },
    /// Moves a handle of a key's curve into the next key: the one that
    /// leaves the key, or the one that comes into the next.
    Handle { leaves: bool, axis: usize },
}

/// A drag on the keys: the frame it began on and the one it is over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    pub kind: DragKind,
    pub from: f32,
    pub to: f32,
    /// The playhead's frame when the drag began.
    pub pivot: f32,
}

impl Drag {
    /// Where a selected key of `frame` shows while the drag lasts.
    pub fn moved(&self, frame: i32, snap: bool) -> f32 {
        let at = match self.kind {
            DragKind::Move { .. } => frame as f32 + (self.to - self.from),
            DragKind::Scale => {
                let reach = self.from - self.pivot;
                if reach.abs() < 0.5 {
                    frame as f32
                } else {
                    self.pivot + (frame as f32 - self.pivot) * (self.to - self.pivot) / reach
                }
            }
            // The graph's drags change the keys as they go.
            _ => return frame as f32,
        };
        if snap {
            at.round()
        } else {
            at
        }
    }
}

/// The sequence, the selection and the pointer's place a drag of the
/// graph began with: each frame the drag is made again from them.
pub type DragFrom = (crate::sequence::Sequence, BTreeSet<KeyRef>, Pos2);

/// What the Timeline, its slider and the Animator remember.
pub struct Anim {
    /// The time shown: fitted to the panel, or `px` points a frame from
    /// the frame `first`.
    pub fit: bool,
    pub px: f32,
    pub first: f32,
    /// The rows opened or shut against how they start.
    pub toggled: BTreeSet<String>,
    /// Only the bones and values with keys are listed.
    pub keyed_only: bool,
    pub selected: BTreeSet<KeyRef>,
    pub drag: Option<Drag>,
    pub drag_from: Option<DragFrom>,
    /// The key whose handle is dragged.
    pub handle: Option<KeyRef>,
    /// The values the graph shows from and to, held while its points are
    /// dragged.
    pub frozen: Option<(f32, f32)>,
    /// An edit of a bone, an IK target or a value sets its key at the
    /// playhead.
    pub auto_key: bool,
    /// Dragged keys show on whole frames as they go.
    pub snap: bool,
    /// Ghosts of the poses this many frames either side.
    pub onion: bool,
    pub onion_frames: f32,
    /// Curves in place of the sheet.
    pub graph: bool,
    /// Drags scale the keys around the playhead.
    pub scale: bool,
    /// The keys copied: each bone's, by its frames after the first.
    pub clipboard: Vec<(String, i32, Key)>,
    /// The row chosen in the list when it is not the posed bone's own: a
    /// value's or a baked bone's, with the bone chosen when it was.
    pub value: Option<(String, Option<String>)>,
}

impl Default for Anim {
    fn default() -> Self {
        Anim {
            fit: true,
            px: 4.0,
            first: 0.0,
            toggled: BTreeSet::new(),
            keyed_only: false,
            selected: BTreeSet::new(),
            drag: None,
            drag_from: None,
            handle: None,
            frozen: None,
            auto_key: false,
            snap: true,
            onion: false,
            onion_frames: 3.0,
            graph: false,
            scale: false,
            clipboard: Vec::new(),
            value: None,
        }
    }
}

/// The frame a key at `time` lies on.
pub fn frame_of(time: f32, fps: f32) -> i32 {
    (time * fps).round() as i32
}

/// The playhead goes to `time`, on the frames, and holds the scene there.
pub fn go_to(st: &mut State, time: f32) {
    st.clock = st.doc.scene().sequence.snap(time);
    st.playback = crate::state::Playback::Paused;
}

/// The playhead's frame, of the character playing alone while one does.
pub fn now(st: &State) -> f32 {
    let sequence = &st.doc.scene().sequence;
    let time = if st.solo.is_some() { st.solo_time } else { st.clock };
    time.clamp(0.0, sequence.length) * sequence.fps.max(1.0)
}

/// What a row of the lists is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An object of the scene: all its keys.
    Object,
    /// The premade clip a character plays.
    Clip,
    /// The clip baked to keys of its own, as a layer in its place.
    Baked,
    /// The keys made by hand, as a layer over the clip.
    Layer,
    /// A part of the body: its bones' keys.
    Group,
    /// A bone, an IK target or a value.
    Channel,
}

/// A row: what it is, and its keys by frame, each with the bones and
/// values that have one there.
pub struct Row {
    pub kind: Kind,
    pub object: ObjectId,
    pub name: String,
    pub label: String,
    pub glyph: &'static str,
    pub tint: Option<Color32>,
    pub depth: usize,
    /// Whether it is open, for a row that folds.
    pub open: Option<bool>,
    pub marks: Vec<(i32, Vec<String>)>,
}

impl Row {
    /// What its fold is remembered by.
    pub fn fold(&self) -> String {
        match self.kind {
            Kind::Object => self.object.0.to_string(),
            _ => format!("{}/{}", self.object.0, self.name),
        }
    }

    /// Whether it is of the baked clip.
    pub fn baked(&self) -> bool {
        self.kind == Kind::Baked || self.name.starts_with(BAKED)
    }
}

/// The parts of the body the bones and values are listed under.
pub const GROUPS: [&str; 9] = ["Face", "Head", "Torso", "Arms", "Hands", "Legs", "IK", "Skin", "Other"];

/// The part of the body a bone belongs to.
pub fn part(bone: &str) -> &'static str {
    let Some(rest) = bone
        .get(5..)
        .filter(|_| bone.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("cf_j_")))
    else {
        return "Other";
    };
    // Only the joints with a name of their own have a part; the helper
    // bones between them, and the skirt's, are listed apart.
    if crate::bones::pose_label(bone) == bone {
        return "Other";
    }
    let name = rest.trim_end_matches("_L").trim_end_matches("_R");
    let is = |list: &[&str]| list.iter().any(|p| name.starts_with(p));
    if is(&["neck", "head"]) {
        "Head"
    } else if is(&["thumb", "index", "middle", "ring", "little"]) {
        "Hands"
    } else if is(&["shoulder", "arm", "forearm", "hand"]) {
        "Arms"
    } else if is(&["thigh", "leg", "foot", "toes"]) {
        "Legs"
    } else {
        "Torso"
    }
}

/// One of a character's rows to be: the part it is listed under, its name
/// among the keys, the name shown and its icon.
type Listed = (&'static str, String, String, &'static str);

/// A character's bones, IK targets and values that take keys.
fn channels(st: &State, id: ObjectId) -> Vec<Listed> {
    use rk_chara::channels::Channel;
    let keys = st.doc.scene().sequence.track(id).map(|t| &t.keys);
    let keyed = |name: &str| keys.is_some_and(|k| k.contains_key(name));
    let mut out = Vec::new();
    for channel in Channel::all() {
        let (group, label) = match channel {
            Channel::Hand(0) => ("Hands", "Left hand pattern".to_owned()),
            Channel::Hand(_) => ("Hands", "Right hand pattern".to_owned()),
            Channel::Skin(_) => ("Skin", channel.label()),
            _ => ("Face", channel.label()),
        };
        out.push((group, channel.key(), label, icon::SLIDERS_HORIZONTAL));
    }
    let layer = match st.doc.scene().get(id).map(|o| &o.content) {
        Some(Content::Character {
            pose_layer: Some(l), ..
        }) => Some(l),
        _ => None,
    };
    let ik_ready = layer.is_some_and(|l| l.ik_on && l.ik.len() == rk_chara::pose::IK_TARGETS);
    for target in 0..rk_chara::pose::IK_TARGETS {
        let name = rk_chara::pose::ik_key(target);
        if keyed(&name) || ik_ready && layer.is_some_and(|l| l.ik_group_on(target)) {
            let label = rk_chara::pose::ik_label(target).trim_end_matches(" (IK)").to_owned();
            out.push(("IK", name, label, icon::TARGET));
        }
    }
    let fk_on = layer.is_some_and(|l| l.fk_on);
    let joints = st.live.as_ref().and_then(|l| l.joints(id)).unwrap_or_default();
    let mut listed = BTreeSet::new();
    for (_, name) in &joints {
        if fk_on || keyed(name) {
            listed.insert(name.clone());
            out.push((part(name), name.clone(), crate::bones::pose_label(name), icon::BONE));
        }
    }
    // Keys of bones the built character does not list still show.
    for name in keys.into_iter().flat_map(|k| k.keys()) {
        let bone = Channel::of_key(name).is_none() && rk_chara::pose::ik_key_target(name).is_none();
        if bone && !listed.contains(name) {
            out.push((part(name), name.clone(), crate::bones::pose_label(name), icon::BONE));
        }
    }
    out
}

/// Whether a folding row is open: as it starts, unless it was toggled.
fn is_open(st: &State, fold: &str, starts: bool) -> bool {
    starts != st.anim.toggled.contains(fold)
}

/// The rows of the scene's keys: each object with keys and the selected
/// character, opened into her clip, its baked keys and the keys by hand,
/// each layer into the parts of her body and their bones.
pub fn rows(st: &State) -> Vec<Row> {
    let scene = st.doc.scene();
    let fps = scene.sequence.fps.max(1.0);
    let mut ids: Vec<ObjectId> = scene
        .sequence
        .tracks
        .iter()
        .map(|t| t.object)
        .filter(|id| scene.get(*id).is_some())
        .collect();
    if let Some(chosen) = st
        .doc
        .selection()
        .filter(|o| matches!(o.content, Content::Character { .. }))
    {
        if !ids.contains(&chosen.id) {
            ids.push(chosen.id);
        }
    }
    let needle = st.animator_search.trim().to_lowercase();
    let narrowed = !needle.is_empty() || st.anim.keyed_only;
    let alone = ids.len() == 1;
    let mut out = Vec::new();
    for id in ids {
        let Some(object) = scene.get(id) else { continue };
        let track = scene.sequence.track(id);
        let frames = |name: &str| -> Vec<i32> {
            track
                .and_then(|t| t.list(name))
                .map(|l| l.iter().map(|k| frame_of(k.time, fps)).collect())
                .unwrap_or_default()
        };
        let found = |name: &str, label: &str| {
            needle.is_empty() || label.to_lowercase().contains(&needle) || name.to_lowercase().contains(&needle)
        };
        let list: Vec<Listed> = channels(st, id)
            .into_iter()
            .filter(|(_, name, label, _)| {
                found(name, label) && (!st.anim.keyed_only || track.is_some_and(|t| t.keys.contains_key(name)))
            })
            .collect();
        let merged = |names: &mut dyn Iterator<Item = &String>| -> Vec<(i32, Vec<String>)> {
            let mut by: BTreeMap<i32, Vec<String>> = BTreeMap::new();
            for name in names {
                for frame in frames(name) {
                    by.entry(frame).or_default().push(name.clone());
                }
            }
            by.into_iter().collect()
        };
        let all = merged(&mut list.iter().map(|c| &c.1));
        let kind = object.kind();
        let mut row = Row {
            kind: Kind::Object,
            object: id,
            name: String::new(),
            label: object.name.clone(),
            glyph: kind.glyph(),
            tint: Some(kind.color()),
            depth: 0,
            open: None,
            marks: all.clone(),
        };
        let open = is_open(st, &row.fold(), alone || st.doc.selected == Some(id));
        row.open = Some(open);
        out.push(row);
        if !open {
            continue;
        }
        if let Content::Character {
            animation: Some(clip), ..
        } = &object.content
        {
            out.push(Row {
                kind: Kind::Clip,
                object: id,
                name: "clip".to_owned(),
                label: clip.name.clone(),
                glyph: icon::FILM_REEL,
                tint: None,
                depth: 1,
                open: None,
                marks: Vec::new(),
            });
        }
        // A layer's rows: the parts of the body with any of `list`, each
        // open into its bones.
        let groups = |out: &mut Vec<Row>, list: &[Listed], prefix: &str, starts_open: bool| {
            for group in GROUPS {
                let members: Vec<_> = list.iter().filter(|c| c.0 == group).collect();
                if members.is_empty() {
                    continue;
                }
                let marks = merged(&mut members.iter().map(|c| &c.1));
                let mut head = Row {
                    kind: Kind::Group,
                    object: id,
                    name: format!("{prefix}{group}"),
                    label: group.to_owned(),
                    glyph: "",
                    tint: None,
                    depth: 2,
                    open: None,
                    marks,
                };
                let open = narrowed || is_open(st, &head.fold(), starts_open && !head.marks.is_empty());
                head.open = Some(open);
                out.push(head);
                if !open {
                    continue;
                }
                for (_, name, label, mark) in members {
                    out.push(Row {
                        kind: Kind::Channel,
                        object: id,
                        name: name.clone(),
                        label: label.clone(),
                        glyph: mark,
                        tint: None,
                        depth: 3,
                        open: None,
                        marks: frames(name).into_iter().map(|f| (f, vec![name.clone()])).collect(),
                    });
                }
            }
        };
        if let Some(baked) = track.and_then(|t| t.baked.as_ref()) {
            let list: Vec<Listed> = baked
                .keys
                .keys()
                .map(|bone| {
                    let label = crate::bones::pose_label(bone);
                    (part(bone), format!("{BAKED}{bone}"), label, icon::BONE)
                })
                .filter(|(_, name, label, _)| found(name, label))
                .collect();
            let mut layer = Row {
                kind: Kind::Baked,
                object: id,
                name: "baked".to_owned(),
                label: "Baked clip".to_owned(),
                glyph: icon::DIAMONDS_FOUR,
                tint: None,
                depth: 1,
                open: None,
                marks: merged(&mut list.iter().map(|c| &c.1)),
            };
            let open = is_open(st, &layer.fold(), false);
            layer.open = Some(open);
            out.push(layer);
            if open {
                groups(&mut out, &list, BAKED, false);
            }
        }
        let mut layer = Row {
            kind: Kind::Layer,
            object: id,
            name: "keys".to_owned(),
            label: "Keys".to_owned(),
            glyph: icon::DIAMOND,
            tint: None,
            depth: 1,
            open: None,
            marks: all,
        };
        let open = is_open(st, &layer.fold(), true);
        layer.open = Some(open);
        out.push(layer);
        if open {
            groups(&mut out, &list, "", true);
        }
    }
    out
}

/// The bone, IK target or value chosen in the lists, among the selected
/// character's keys.
pub fn chosen(st: &State) -> Option<String> {
    if let Some(target) = st.pose_ik {
        return Some(rk_chara::pose::ik_key(target));
    }
    match &st.anim.value {
        Some((name, with)) if *with == st.pose_bone => Some(name.clone()),
        _ => st.pose_bone.clone(),
    }
}

/// Chooses a row's bone, IK target or value, and its character.
pub fn choose(st: &mut State, object: ObjectId, name: &str) {
    st.doc.selected = Some(object);
    st.anim.value = None;
    if let Some(target) = rk_chara::pose::ik_key_target(name) {
        st.pose_ik = Some(target);
        st.pose_bone = None;
    } else if rk_chara::channels::Channel::of_key(name).is_some() {
        st.pose_ik = None;
        st.anim.value = Some((name.to_owned(), st.pose_bone.clone()));
    } else {
        // A baked bone's row chooses the bone, and stays the row chosen.
        let bone = name.strip_prefix(BAKED).unwrap_or(name);
        st.pose_bone = Some(bone.to_owned());
        st.pose_ik = None;
        st.property_tab = 1;
        if bone != name {
            st.anim.value = Some((name.to_owned(), st.pose_bone.clone()));
        }
    }
}

/// Plays a character's keys alone, or no longer: paused where she is, so
/// that frame can be edited.
pub fn toggle_solo(st: &mut State, id: ObjectId) {
    if st.solo == Some(id) {
        let at = st.solo_time;
        st.solo = None;
        go_to(st, at);
    } else {
        st.solo = Some(id);
        st.solo_time = st.clock.clamp(0.0, st.doc.scene().sequence.length);
        if st.playback == crate::state::Playback::Playing {
            st.playback = crate::state::Playback::Paused;
        }
    }
}

/// Whether a character plays a clip that is not baked to keys yet.
fn can_bake(st: &State, id: ObjectId) -> bool {
    let clip = matches!(
        st.doc.scene().get(id).map(|o| &o.content),
        Some(Content::Character { animation: Some(_), .. })
    );
    clip && st.doc.scene().sequence.track(id).is_none_or(|t| t.baked.is_none())
}

/// Bakes a character's clip to keys of its own, a key each frame of the
/// sequence where the clip does not go straight: a layer that poses her
/// in the clip's place while it is on. The clip's file is not touched.
/// One step of the history. Returns whether she was built to bake.
pub fn bake(st: &mut State, id: ObjectId) -> bool {
    let Some(Content::Character {
        animation: Some(clip), ..
    }) = st.doc.scene().get(id).map(|o| o.content.clone())
    else {
        return true;
    };
    let mut sequence = st.doc.scene().sequence.clone();
    let fps = sequence.fps.max(1.0);
    let times: Vec<f32> = (0..=frame_of(sequence.length, fps)).map(|f| f as f32 / fps).collect();
    let Some(keys) = st.live.as_mut().and_then(|l| l.bake_clip(id, &clip, &times)) else {
        return false;
    };
    let name = st.doc.scene().get(id).map(|o| o.name.clone()).unwrap_or_default();
    sequence.track_mut(id, &name).baked = Some(crate::sequence::BakedClip { on: true, keys });
    st.doc.edit_sequence(sequence, false);
    true
}

/// Changes a character's baked clip as one step of the history; `false`
/// from `change` takes the layer away.
fn edit_baked(st: &mut State, id: ObjectId, change: impl FnOnce(&mut crate::sequence::BakedClip) -> bool) {
    let mut sequence = st.doc.scene().sequence.clone();
    if let Some(track) = sequence.tracks.iter_mut().find(|t| t.object == id) {
        if let Some(baked) = track.baked.as_mut() {
            if !change(baked) {
                track.baked = None;
            }
        }
    }
    sequence.tidy();
    st.anim
        .selected
        .retain(|key| !(key.0 == id && key.1.starts_with(BAKED)));
    st.doc.edit_sequence(sequence, false);
}

/// How the lists are drawn: the rows' height.
#[derive(Clone, Copy)]
pub struct Cells {
    pub row: f32,
}

impl Cells {
    /// A row's height: taller for an object's, the head of its track.
    pub fn height(&self, row: &Row) -> f32 {
        if row.kind == Kind::Object {
            self.row + 6.0
        } else {
            self.row
        }
    }
}

/// A small icon button at a row's right end. Returns whether it was
/// clicked.
fn row_button(ui: &mut Ui, k: &Look, at: Rect, id: Id, mark: &str, on: bool, tip: &str) -> bool {
    let resp = ui.interact(at, id, Sense::click()).on_hover_text(tip);
    let color = if on {
        k.t.select_bright
    } else if resp.hovered() {
        k.t.text_bright
    } else {
        k.t.text_dim
    };
    if on {
        widgets::boxed(ui.painter(), at, k.radius(), k.t.input, k.t.border);
    } else if resp.hovered() {
        ui.painter().rect_filled(at, k.radius(), k.t.control);
    }
    glyph(ui.painter(), k, at.center(), mark, 13.0, color);
    resp.clicked()
}

/// A row's name in the list: its fold, its icon and its name, and at its
/// right end what the row offers. A click chooses or folds it.
pub fn name_cell(ui: &mut Ui, k: &Look, st: &mut State, row: &Row, rect: Rect) {
    let t = &k.t;
    let id = ui
        .id()
        .with(("anim row", row.object.0, row.kind as u8, row.name.as_str()));
    let resp = ui.interact(rect, id, Sense::click());
    let on = match row.kind {
        Kind::Channel => st.doc.selected == Some(row.object) && chosen(st).as_deref() == Some(row.name.as_str()),
        _ => false,
    };
    let p = ui.painter().with_clip_rect(rect);
    let fill = match row.kind {
        _ if on => Some(t.select),
        Kind::Object => Some(t.header),
        _ if resp.hovered() => Some(t.row_hover),
        Kind::Group | Kind::Layer | Kind::Baked => Some(Color32::from_white_alpha(5)),
        _ => None,
    };
    if let Some(fill) = fill {
        p.rect_filled(rect, 0.0, fill);
    }
    if row.kind == Kind::Object {
        // A track's head: the object's colour down its left edge.
        p.rect_filled(
            Rect::from_min_max(rect.min, Pos2::new(rect.left() + 3.0, rect.bottom())),
            0.0,
            row.tint.unwrap_or(t.select),
        );
    }
    hair_h(&p, rect.left(), rect.right(), rect.bottom() - 1.0, t.line);
    let y = rect.center().y;
    let mut x = rect.left() + 10.0 + row.depth as f32 * 14.0;
    if let Some(open) = row.open {
        let arrow = if open { icon::CARET_DOWN } else { icon::CARET_RIGHT };
        glyph(&p, k, Pos2::new(x + 4.0, y), arrow, 10.0, t.text_dim);
    }
    x += 14.0;
    let (text, faint) = if on {
        (Color32::WHITE, Color32::from_white_alpha(190))
    } else if matches!(row.kind, Kind::Object) {
        (t.text_bright, t.text)
    } else {
        (t.text, t.text_dim)
    };
    if !row.glyph.is_empty() {
        let size = if row.kind == Kind::Object { 15.0 } else { 13.0 };
        glyph(&p, k, Pos2::new(x + 6.0, y), row.glyph, size, row.tint.unwrap_or(faint));
        x += 18.0;
    }
    let font = match row.kind {
        Kind::Object | Kind::Group | Kind::Layer | Kind::Baked => k.bold(k.d.text),
        _ => k.font(),
    };
    p.text(Pos2::new(x, y), egui::Align2::LEFT_CENTER, &row.label, font, text);
    // What the row offers, at its right end.
    let side = 18.0;
    let slot = |n: usize| {
        Rect::from_center_size(
            Pos2::new(rect.right() - 14.0 - n as f32 * (side + 2.0), y),
            Vec2::splat(side),
        )
    };
    let mut handled = false;
    match row.kind {
        Kind::Object => {
            let alone = st.solo == Some(row.object);
            if matches!(
                st.doc.scene().get(row.object).map(|o| &o.content),
                Some(Content::Character { .. })
            ) && row_button(ui, k, slot(0), id.with("solo"), icon::USER_FOCUS, alone, "Play alone")
            {
                toggle_solo(st, row.object);
                handled = true;
            }
        }
        Kind::Clip => {
            if can_bake(st, row.object)
                && row_button(
                    ui,
                    k,
                    slot(0),
                    id.with("bake"),
                    icon::DIAMONDS_FOUR,
                    false,
                    "Bake to keys",
                )
            {
                bake(st, row.object);
                handled = true;
            }
        }
        Kind::Baked => {
            let shown = st
                .doc
                .scene()
                .sequence
                .track(row.object)
                .is_some_and(|t| t.baked_on().is_some());
            let eye = if shown { icon::EYE } else { icon::EYE_SLASH };
            let tip = if shown { "Play the clip" } else { "Play the baked keys" };
            if row_button(ui, k, slot(0), id.with("baked on"), eye, false, tip) {
                edit_baked(st, row.object, |baked| {
                    baked.on = !baked.on;
                    true
                });
                handled = true;
            }
            if row_button(
                ui,
                k,
                slot(1),
                id.with("unbake"),
                icon::TRASH,
                false,
                "Delete baked keys",
            ) {
                edit_baked(st, row.object, |_| false);
                handled = true;
            }
        }
        Kind::Channel if !row.baked() => {
            // The key at the playhead: full with one, hollow without.
            let here = now(st).round() as i32;
            let has = row.marks.iter().any(|(f, _)| *f == here);
            let at = slot(0);
            let mark = ui.interact(at, id.with("key"), Sense::click()).on_hover_text(if has {
                "Delete key"
            } else {
                "Set key"
            });
            let color = if has {
                t.accent
            } else if mark.hovered() {
                t.text_bright
            } else {
                t.border
            };
            let fill = if has { color } else { Color32::TRANSPARENT };
            sheet::diamond(ui.painter(), at.center(), 4.5, fill, color);
            if mark.clicked() {
                if has {
                    let time = crate::keying::now(st);
                    crate::keying::delete_key_at(st, row.object, &row.name, time);
                } else {
                    crate::keying::set_keys(st, row.object, std::slice::from_ref(&row.name));
                }
                handled = true;
            }
        }
        Kind::Channel => {}
        Kind::Layer | Kind::Group => {
            let count: usize = row.marks.iter().map(|(_, names)| names.len()).sum();
            if count > 0 {
                p.text(
                    Pos2::new(rect.right() - 10.0, y),
                    egui::Align2::RIGHT_CENTER,
                    count.to_string(),
                    k.small(),
                    t.text_dim,
                );
            }
        }
    }
    if resp.clicked() && !handled {
        let folds = resp
            .interact_pointer_pos()
            .is_some_and(|at| at.x < rect.left() + 26.0 + row.depth as f32 * 14.0);
        match row.kind {
            Kind::Object if !folds => st.doc.selected = Some(row.object),
            Kind::Channel => choose(st, row.object, &row.name),
            Kind::Clip => st.doc.selected = Some(row.object),
            _ => {
                let fold = row.fold();
                if !st.anim.toggled.remove(&fold) {
                    st.anim.toggled.insert(fold);
                }
            }
        }
    }
    let (object, name) = (row.object, row.name.clone());
    match row.kind {
        Kind::Channel if !row.baked() => resp.context_menu(|ui| {
            if widgets::menu_item(ui, k, icon::DIAMOND, "Set key", "", true) {
                crate::keying::set_keys(st, object, std::slice::from_ref(&name));
                ui.close();
            }
            let pasted = !st.anim.clipboard.is_empty();
            if widgets::menu_item(ui, k, icon::CLIPBOARD, "Paste keys", "", pasted) {
                sheet::paste(st, object, Some(&name));
                ui.close();
            }
            ui.separator();
            if widgets::menu_item(ui, k, icon::TRASH, "Delete keys", "", true) {
                crate::keying::delete_bone_keys(st, object, &name);
                ui.close();
            }
        }),
        Kind::Object | Kind::Clip => resp.context_menu(|ui| {
            if widgets::menu_item(ui, k, icon::DIAMONDS_FOUR, "Bake to keys", "", can_bake(st, object)) {
                bake(st, object);
                ui.close();
            }
            ui.separator();
            if widgets::menu_item(ui, k, icon::TRASH, "Clear keys", "", true) {
                crate::keying::clear_keys(st, object);
                ui.close();
            }
        }),
        _ => None,
    };
}

/// What the keys take from the keyboard while the Timeline or the
/// Animator has the focus: Delete, copy, paste and duplicate act on the
/// selected keys, and Home fits the time. Returns whether the action was
/// taken.
pub fn key_action(st: &mut State, action: crate::keymap::Action) -> bool {
    use crate::keymap::Action as A;
    use crate::panels::Panel;
    if !matches!(st.focus, Panel::Timeline | Panel::Animator) {
        return false;
    }
    match action {
        A::Delete if !st.anim.selected.is_empty() => sheet::delete(st),
        A::Copy if !st.anim.selected.is_empty() => sheet::copy(st),
        A::Paste if !st.anim.clipboard.is_empty() => {
            if let Some(id) = st.doc.selected {
                sheet::paste(st, id, None);
            }
        }
        A::Duplicate if !st.anim.selected.is_empty() => {
            sheet::copy(st);
            if let Some(id) = st.doc.selected {
                sheet::paste(st, id, None);
            }
        }
        A::ResetView if st.focus == Panel::Timeline => st.anim.fit = true,
        _ => return false,
    }
    true
}

/// The ··· menu of a character's keys: baked, saved, loaded and cleared.
pub fn more_menu(ui: &mut Ui, k: &Look, st: &mut State) {
    let character = st
        .doc
        .selection()
        .filter(|o| matches!(o.content, Content::Character { .. }))
        .map(|o| o.id);
    let has_keys = character
        .and_then(|id| st.doc.scene().sequence.track(id))
        .is_some_and(|t| !t.keys.is_empty());
    let saved: Vec<String> = {
        let mut files = Vec::new();
        crate::project::files_below(&st.project.folder(crate::project::ANIMATIONS), &mut files);
        let mut rels: Vec<String> = files
            .iter()
            .filter_map(|f| crate::project::relative_to(&st.project.root, f))
            .filter(|r| crate::sequence::is_keys(r))
            .collect();
        rels.sort_by_key(|r| r.to_lowercase());
        rels
    };
    let more = ui
        .add_enabled_ui(character.is_some(), |ui| {
            widgets::icon_button(ui, k, icon::DOTS_THREE, false)
        })
        .inner;
    let Some(id) = character else { return };
    egui::Popup::menu(&more)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            if widgets::menu_item(ui, k, icon::DIAMONDS_FOUR, "Bake to keys", "", can_bake(st, id)) {
                bake(st, id);
                ui.close();
            }
            ui.separator();
            if widgets::menu_item(ui, k, icon::FLOPPY_DISK, "Save as animation", "", has_keys) {
                st.run(crate::state::Command::SaveKeys);
                ui.close();
            }
            widgets::menu_sub(ui, k, icon::FILM_REEL, "Load animation", |ui| {
                if saved.is_empty() {
                    widgets::menu_item(ui, k, icon::FILM_REEL, "No saved animations", "", false);
                }
                for rel in &saved {
                    if widgets::menu_item(ui, k, icon::FILM_REEL, crate::content::stem(rel), "", true) {
                        st.run(crate::state::Command::LoadKeys(rel.clone()));
                        ui.close();
                    }
                }
            });
            ui.separator();
            if widgets::menu_item(ui, k, icon::TRASH, "Clear keys", "", has_keys) {
                crate::keying::clear_keys(st, id);
                ui.close();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bones_go_to_their_parts() {
        assert_eq!(part("cf_j_neck"), "Head");
        assert_eq!(part("cf_j_arm00_L"), "Arms");
        assert_eq!(part("cf_j_hand_R"), "Arms");
        assert_eq!(part("cf_j_index02_L"), "Hands");
        assert_eq!(part("cf_j_leg01_R"), "Legs");
        assert_eq!(part("cf_j_spine02"), "Torso");
        assert_eq!(part("cf_j_spinesk_00"), "Other");
        assert_eq!(part("cf_d_bust01_L"), "Other");
    }

    #[test]
    fn a_drag_moves_and_scales() {
        let mut drag = Drag {
            kind: DragKind::Move { copy: false },
            from: 10.0,
            to: 14.4,
            pivot: 0.0,
        };
        assert_eq!(drag.moved(30, true), 34.0);
        drag.kind = DragKind::Scale;
        drag.to = 20.0;
        assert_eq!(drag.moved(30, true), 60.0);
    }
}
