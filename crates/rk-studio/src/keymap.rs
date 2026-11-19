// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The keys of the editor. Every function a key can reach is an action
//! with default keys; the user's own keys are kept in the studio settings
//! and override them. One place reads the keyboard each frame, finds the
//! action a key stands for and carries it out.
//!
//! It also keeps the inputs shown in the viewport's corner while "Show
//! inputs" is on: the keys and mouse buttons pressed, and what they did.

use crate::state::{Command, Look, Playback, State};
use crate::theme::Theme;
use egui::{Align2, Key, Modifiers, PointerButton, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};
use egui_phosphor::regular as icon;
use std::collections::BTreeMap;

/// The groups the Keyboard shortcuts window lists the actions in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    File,
    Edit,
    Select,
    Tools,
    Transform,
    View,
    Modes,
    Scene,
    Window,
}

impl Group {
    pub const ALL: [Group; 9] = [
        Group::File,
        Group::Edit,
        Group::Select,
        Group::Tools,
        Group::Transform,
        Group::View,
        Group::Modes,
        Group::Scene,
        Group::Window,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Group::File => "File",
            Group::Edit => "Edit",
            Group::Select => "Select",
            Group::Tools => "Tools",
            Group::Transform => "Transform",
            Group::View => "View",
            Group::Modes => "Modes",
            Group::Scene => "Scene",
            Group::Window => "Window",
        }
    }
}

/// Something a key can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    NewProject,
    OpenProject,
    Save,
    SaveAs,
    ImportCard,
    Quit,
    Undo,
    Redo,
    Copy,
    Cut,
    Paste,
    Duplicate,
    Rename,
    Delete,
    ToggleHidden,
    Attach,
    Unparent,
    NewFolder,
    Preferences,
    SelectAll,
    SelectNone,
    SelectChildren,
    /// A transform tool, by its place in the strip.
    Tool(u8),
    ToggleSpace,
    TogglePivot,
    ToggleSnap,
    ToggleGrid,
    DropToSurface,
    Focus,
    ResetView,
    Maximise,
    ViewportTools,
    ModeTools,
    ShowInputs,
    /// A mode of the rail, by its place.
    Mode(u8),
    PlayPause,
    Stop,
    Shortcuts,
    Packages,
}

/// An action with its name in the settings file, its name on screen, its
/// group and its default keys.
pub struct Def {
    pub action: Action,
    pub id: &'static str,
    pub name: &'static str,
    pub group: Group,
    pub keys: &'static [&'static str],
}

const fn def(action: Action, id: &'static str, name: &'static str, group: Group, keys: &'static [&'static str]) -> Def {
    Def {
        action,
        id,
        name,
        group,
        keys,
    }
}

/// Every action. Where two actions share a key, the first one listed
/// gets it. The defaults follow Unity and Unreal, then Blender.
pub const ACTIONS: [Def; 49] = [
    def(
        Action::NewProject,
        "new-project",
        "New project",
        Group::File,
        &["Ctrl+N"],
    ),
    def(
        Action::OpenProject,
        "open-project",
        "Open project",
        Group::File,
        &["Ctrl+O"],
    ),
    def(Action::Save, "save", "Save", Group::File, &["Ctrl+S"]),
    def(Action::SaveAs, "save-as", "Save as", Group::File, &["Ctrl+Shift+S"]),
    def(
        Action::ImportCard,
        "import-card",
        "Import card",
        Group::File,
        &["Ctrl+I"],
    ),
    def(Action::Quit, "quit", "Quit", Group::File, &["Ctrl+Q"]),
    def(Action::Undo, "undo", "Undo", Group::Edit, &["Ctrl+Z"]),
    def(Action::Redo, "redo", "Redo", Group::Edit, &["Ctrl+Y", "Ctrl+Shift+Z"]),
    def(Action::Copy, "copy", "Copy", Group::Edit, &["Ctrl+C"]),
    def(Action::Cut, "cut", "Cut", Group::Edit, &["Ctrl+X"]),
    def(Action::Paste, "paste", "Paste", Group::Edit, &["Ctrl+V"]),
    def(Action::Duplicate, "duplicate", "Duplicate", Group::Edit, &["Ctrl+D"]),
    def(Action::Rename, "rename", "Rename", Group::Edit, &["F2"]),
    def(Action::Delete, "delete", "Delete", Group::Edit, &["Delete"]),
    def(Action::ToggleHidden, "hide", "Hide or show", Group::Edit, &["H"]),
    def(Action::Attach, "attach", "Attach", Group::Edit, &["P"]),
    def(Action::Unparent, "unparent", "Detach", Group::Edit, &["Alt+P"]),
    def(
        Action::NewFolder,
        "new-folder",
        "New folder",
        Group::Edit,
        &["Ctrl+Shift+N"],
    ),
    def(
        Action::Preferences,
        "preferences",
        "Preferences",
        Group::Edit,
        &["Ctrl+Comma"],
    ),
    def(
        Action::SelectAll,
        "select-all",
        "Select all",
        Group::Select,
        &["Ctrl+A"],
    ),
    def(
        Action::SelectNone,
        "select-none",
        "Select none",
        Group::Select,
        &["Alt+A"],
    ),
    def(
        Action::SelectChildren,
        "select-children",
        "Select children",
        Group::Select,
        &["Shift+G"],
    ),
    def(Action::Tool(0), "tool-view", "View", Group::Tools, &[]),
    def(Action::Tool(1), "tool-select", "Select", Group::Tools, &["Q"]),
    def(Action::Tool(2), "tool-move", "Move", Group::Tools, &["W"]),
    def(Action::Tool(3), "tool-rotate", "Rotate", Group::Tools, &["E"]),
    def(Action::Tool(4), "tool-scale", "Scale", Group::Tools, &["R"]),
    def(Action::Tool(5), "tool-pose", "Pose", Group::Tools, &["T"]),
    def(
        Action::ToggleSpace,
        "space",
        "Global or local",
        Group::Transform,
        &["X"],
    ),
    def(Action::TogglePivot, "pivot", "Pivot", Group::Transform, &["Z"]),
    def(Action::ToggleSnap, "snap", "Snap", Group::Transform, &["Backslash"]),
    def(Action::ToggleGrid, "grid", "Grid", Group::Transform, &["G"]),
    def(
        Action::DropToSurface,
        "drop",
        "Snap to floor",
        Group::Transform,
        &["End"],
    ),
    def(Action::Focus, "focus", "Focus", Group::View, &["F"]),
    def(Action::ResetView, "reset-view", "Reset view", Group::View, &["Home"]),
    def(
        Action::Maximise,
        "maximise",
        "Maximise viewport",
        Group::View,
        &["Shift+Space"],
    ),
    def(
        Action::ViewportTools,
        "viewport-tools",
        "Viewport tools",
        Group::View,
        &["Backtick"],
    ),
    def(Action::ModeTools, "mode-tools", "Mode tools", Group::View, &["N"]),
    def(Action::ShowInputs, "show-inputs", "Show inputs", Group::View, &["F8"]),
    def(Action::Mode(0), "mode-select", "Select", Group::Modes, &["1"]),
    def(Action::Mode(1), "mode-character", "Character", Group::Modes, &["2"]),
    def(Action::Mode(2), "mode-pose", "Pose", Group::Modes, &["3"]),
    def(Action::Mode(3), "mode-light", "Light", Group::Modes, &["4"]),
    def(Action::Mode(4), "mode-animate", "Animate", Group::Modes, &["5"]),
    def(Action::Mode(5), "mode-bake", "Bake", Group::Modes, &["6"]),
    def(Action::PlayPause, "play", "Play or pause", Group::Scene, &["Space"]),
    def(Action::Stop, "stop", "Stop", Group::Scene, &["Ctrl+Space"]),
    def(
        Action::Shortcuts,
        "shortcuts",
        "Keyboard shortcuts",
        Group::Window,
        &["F1"],
    ),
    def(Action::Packages, "packages", "Packages", Group::Window, &[]),
];

/// How many keys one action can have.
pub const SLOTS: usize = 2;

/// A key with the modifiers held with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

impl Chord {
    fn of(modifiers: Modifiers, key: Key) -> Chord {
        Chord {
            ctrl: modifiers.command || modifiers.ctrl,
            shift: modifiers.shift,
            alt: modifiers.alt,
            key,
        }
    }

    /// Reads a chord as the settings file writes it: `Ctrl+Shift+S`.
    pub fn parse(text: &str) -> Option<Chord> {
        let mut chord = Chord {
            ctrl: false,
            shift: false,
            alt: false,
            key: Key::A,
        };
        let mut key = None;
        for part in text.split('+').map(str::trim) {
            match part.to_lowercase().as_str() {
                "ctrl" | "cmd" => chord.ctrl = true,
                "shift" => chord.shift = true,
                "alt" => chord.alt = true,
                _ => key = Some(Key::from_name(part)?),
            }
        }
        chord.key = key?;
        Some(chord)
    }

    /// The chord as the settings file keeps it.
    fn stored(self) -> String {
        self.joined(self.key.name(), "+")
    }

    /// The chord as menus show it: `Ctrl+Z`.
    pub fn text(self) -> String {
        self.joined(key_name(self.key), "+")
    }

    /// The chord as the shown inputs write it: `Ctrl Z`.
    fn spaced(self) -> String {
        self.joined(key_name(self.key), " ")
    }

    fn joined(self, key: &str, with: &str) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        parts.push(key);
        parts.join(with)
    }

    pub fn modifiers(self) -> Modifiers {
        Modifiers {
            alt: self.alt,
            ctrl: self.ctrl,
            shift: self.shift,
            mac_cmd: false,
            command: self.ctrl,
        }
    }
}

/// A key's name as people write it.
fn key_name(key: Key) -> &'static str {
    match key {
        Key::Backtick => "`",
        Key::Backslash => "\\",
        Key::Comma => ",",
        Key::Delete => "Del",
        other => other.name(),
    }
}

/// The keys of every action: the defaults with the user's changes.
#[derive(Debug, Clone, PartialEq)]
pub struct Keymap {
    /// The chords of each action, in the order of `ACTIONS`.
    bound: Vec<Vec<Chord>>,
}

impl Default for Keymap {
    fn default() -> Keymap {
        Keymap {
            bound: ACTIONS.iter().map(defaults).collect(),
        }
    }
}

fn defaults(def: &Def) -> Vec<Chord> {
    def.keys.iter().filter_map(|k| Chord::parse(k)).collect()
}

fn index(action: Action) -> usize {
    ACTIONS.iter().position(|d| d.action == action).unwrap_or(0)
}

impl Keymap {
    /// The defaults with what the settings file changes.
    pub fn with(changes: &BTreeMap<String, Vec<String>>) -> Keymap {
        let mut map = Keymap::default();
        for (i, def) in ACTIONS.iter().enumerate() {
            if let Some(keys) = changes.get(def.id) {
                map.bound[i] = keys.iter().filter_map(|k| Chord::parse(k)).take(SLOTS).collect();
            }
        }
        map
    }

    /// What the settings file keeps: only the actions whose keys differ
    /// from the defaults.
    pub fn changes(&self) -> BTreeMap<String, Vec<String>> {
        let mut changes = BTreeMap::new();
        for (def, chords) in ACTIONS.iter().zip(&self.bound) {
            if *chords != defaults(def) {
                changes.insert(def.id.to_owned(), chords.iter().map(|c| c.stored()).collect());
            }
        }
        changes
    }

    pub fn chords(&self, action: Action) -> &[Chord] {
        &self.bound[index(action)]
    }

    /// The first key of an action as menus show it, or nothing.
    pub fn text(&self, action: Action) -> String {
        self.chords(action).first().map(|c| c.text()).unwrap_or_default()
    }

    /// A tip with the action's key after it in brackets, when it has one.
    pub fn tip(&self, tip: &str, action: Action) -> String {
        match self.chords(action).first() {
            Some(chord) => format!("{tip} ({})", chord.text()),
            None => tip.to_owned(),
        }
    }

    /// The action a chord stands for: the first listed that has it.
    fn find(&self, chord: Chord) -> Option<&'static Def> {
        ACTIONS
            .iter()
            .zip(&self.bound)
            .find(|(_, chords)| chords.contains(&chord))
            .map(|(def, _)| def)
    }

    /// The names of the other actions that share a key with this one.
    pub fn clashes(&self, action: Action) -> Vec<&'static str> {
        let mine = self.chords(action);
        ACTIONS
            .iter()
            .zip(&self.bound)
            .filter(|(def, chords)| def.action != action && chords.iter().any(|c| mine.contains(c)))
            .map(|(def, _)| def.name)
            .collect()
    }

    /// Puts a chord in one of an action's places, or adds it after them.
    pub fn set(&mut self, action: Action, slot: usize, chord: Chord) {
        let chords = &mut self.bound[index(action)];
        if chords.iter().enumerate().any(|(i, c)| *c == chord && i != slot) {
            return;
        }
        match chords.get_mut(slot) {
            Some(place) => *place = chord,
            None => chords.push(chord),
        }
    }

    /// Takes one of an action's keys away.
    pub fn clear(&mut self, action: Action, slot: usize) {
        let chords = &mut self.bound[index(action)];
        if slot < chords.len() {
            chords.remove(slot);
        }
    }

    pub fn reset(&mut self, action: Action) {
        let i = index(action);
        self.bound[i] = defaults(&ACTIONS[i]);
    }

    pub fn is_default(&self, action: Action) -> bool {
        let i = index(action);
        self.bound[i] == defaults(&ACTIONS[i])
    }
}

/// One of the inputs shown in the viewport's corner.
pub struct Input {
    /// The modifiers and the key, or the modifiers held with a button.
    keys: String,
    /// The mouse button's icon, for a button.
    button: Option<&'static str>,
    /// What it did.
    action: &'static str,
    /// How often in a row.
    times: u32,
    /// When it was last pressed, in the window's seconds.
    at: f64,
}

/// How long an input stays, then how long it takes to fade, in seconds.
const INPUT_HOLD: f64 = 2.5;
const INPUT_FADE: f64 = 0.6;
/// How many inputs show at once.
const INPUT_LINES: usize = 5;

fn show_input(st: &mut State, keys: String, button: Option<&'static str>, action: &'static str, at: f64) {
    if !st.settings.show_inputs {
        return;
    }
    match st.inputs.last_mut() {
        Some(last) if last.keys == keys && last.button == button && last.action == action => {
            last.times += 1;
            last.at = at;
        }
        _ => st.inputs.push(Input {
            keys,
            button,
            action,
            times: 1,
            at,
        }),
    }
    let extra = st.inputs.len().saturating_sub(INPUT_LINES);
    st.inputs.drain(..extra);
}

/// Reads this frame's keys and carries out what they stand for. Runs
/// before anything is drawn.
pub fn handle(ctx: &egui::Context, st: &mut State) {
    let now = ctx.input(|i| i.time);
    let typing = ctx.egui_wants_keyboard_input();
    // The keys pressed this frame. Copy, cut and paste reach the window as
    // events of their own on some systems and as keys on others.
    let mut presses: Vec<(Chord, bool)> = Vec::new();
    let mut buttons: Vec<(Modifiers, PointerButton)> = Vec::new();
    ctx.input(|i| {
        let command = Modifiers::COMMAND;
        for event in &i.events {
            let press = match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat,
                    modifiers,
                    ..
                } => Some((Chord::of(*modifiers, *key), *repeat)),
                egui::Event::Copy => Some((Chord::of(command, Key::C), false)),
                egui::Event::Cut => Some((Chord::of(command, Key::X), false)),
                egui::Event::Paste(_) => Some((Chord::of(command, Key::V), false)),
                egui::Event::PointerButton {
                    button,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    buttons.push((*modifiers, *button));
                    None
                }
                _ => None,
            };
            if let Some(press) = press {
                if !presses.iter().any(|(chord, _)| *chord == press.0) {
                    presses.push(press);
                }
            }
        }
    });

    // The Keyboard shortcuts window waits for the keys of an action.
    if let Some((action, slot)) = st.key_capture {
        if let Some((chord, _)) = presses.first() {
            if chord.key != Key::Escape {
                st.keys.set(action, slot, *chord);
                st.settings.keys = st.keys.changes();
                st.settings_changed = true;
            }
            st.key_capture = None;
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { .. })));
        }
        return;
    }

    for (modifiers, button) in buttons {
        let glyph = match button {
            PointerButton::Primary => icon::MOUSE_LEFT_CLICK,
            PointerButton::Secondary => icon::MOUSE_RIGHT_CLICK,
            PointerButton::Middle => icon::MOUSE_MIDDLE_CLICK,
            _ => continue,
        };
        let mut held = Vec::new();
        for (on, name) in [
            (modifiers.command || modifiers.ctrl, "Ctrl"),
            (modifiers.shift, "Shift"),
            (modifiers.alt, "Alt"),
        ] {
            if on {
                held.push(name);
            }
        }
        show_input(st, held.join(" "), Some(glyph), "", now);
    }

    // The keys belong to a dialog while one is open.
    if st.modal.is_some() {
        return;
    }
    let flying = ctx.input(|i| i.pointer.secondary_down());
    for (chord, repeat) in presses {
        let def = st.keys.find(chord).filter(|def| {
            // Typing keeps every key but those of the File menu; a held
            // key repeats only undo and redo; and while the right button
            // flies the camera, plain keys steer it.
            let plain = !chord.ctrl && !chord.alt;
            (!typing || def.group == Group::File)
                && (!repeat || matches!(def.action, Action::Undo | Action::Redo))
                && !(flying && plain)
        });
        match def {
            Some(def) => {
                ctx.input_mut(|i| i.consume_key(chord.modifiers(), chord.key));
                st.act(def.action, ctx);
                show_input(st, chord.spaced(), None, def.name, now);
            }
            // What is typed into a field is nobody's business.
            None if !typing && !repeat => show_input(st, chord.spaced(), None, "", now),
            None => {}
        }
    }
}

impl State {
    /// Carries out an action of the keymap.
    pub fn act(&mut self, action: Action, ctx: &egui::Context) {
        use Action as A;
        match action {
            A::NewProject => self.ask(Command::NewProject, ctx),
            A::OpenProject => self.ask(Command::OpenProject(None), ctx),
            A::Save => self.ask(Command::Save, ctx),
            A::SaveAs => self.ask(Command::SaveAs, ctx),
            A::ImportCard => self.ask(Command::ImportCard, ctx),
            A::Quit => self.ask(Command::Quit, ctx),
            A::Undo => self.run(Command::Undo),
            A::Redo => self.run(Command::Redo),
            A::Copy | A::Cut => {
                // The system's clipboard gets the name, so that a paste
                // has something to bring back.
                if let Some(object) = self.doc.selection() {
                    ctx.copy_text(object.name.clone());
                }
                self.run(if action == A::Copy { Command::Copy } else { Command::Cut });
            }
            A::Paste => self.run(Command::Paste),
            A::Duplicate => self.run(Command::Duplicate),
            A::Rename => self.run(Command::Rename),
            A::Delete => self.run(Command::Delete),
            A::ToggleHidden => self.run(Command::ToggleHidden),
            A::Attach => self.run(Command::AttachToLast),
            A::Unparent => self.run(Command::Unparent),
            A::NewFolder => self.run(Command::AddChildFolder),
            A::Preferences => self.preferences_open = !self.preferences_open,
            A::SelectAll => {
                let all: Vec<_> = self.doc.scene().objects().iter().map(|o| o.id).collect();
                self.doc.select_many(&all);
            }
            A::SelectNone => self.doc.select_many(&[]),
            A::SelectChildren => self.run(Command::SelectChildren),
            A::Tool(tool) => self.tool = tool as usize,
            A::ToggleSpace => {
                self.gizmo.space = match self.gizmo.space {
                    crate::gizmo::Space::Global => crate::gizmo::Space::Local,
                    crate::gizmo::Space::Local => crate::gizmo::Space::Global,
                };
            }
            A::TogglePivot => {
                self.gizmo.pivot = match self.gizmo.pivot {
                    crate::gizmo::Pivot::Origin => crate::gizmo::Pivot::Centre,
                    crate::gizmo::Pivot::Centre => crate::gizmo::Pivot::Origin,
                };
            }
            A::ToggleSnap => self.snap = !self.snap,
            A::ToggleGrid => self.gizmo.grid = !self.gizmo.grid,
            A::DropToSurface => self.run(Command::SnapToFloor),
            A::Focus => crate::viewport::focus_selection(self),
            A::ResetView => {
                if let Some(live) = self.live.as_mut() {
                    live.cam.reset();
                }
            }
            A::Maximise => self.maximised = !self.maximised,
            A::ViewportTools => self.overlays_hidden = !self.overlays_hidden,
            A::ModeTools => self.palette_open = !self.palette_open,
            A::ShowInputs => {
                self.settings.show_inputs = !self.settings.show_inputs;
                self.settings_changed = true;
                self.inputs.clear();
            }
            A::Mode(mode) => self.pick_mode(mode as usize),
            A::PlayPause => {
                self.playback = match self.playback {
                    Playback::Playing => Playback::Paused,
                    _ => Playback::Playing,
                };
            }
            A::Stop => self.playback = Playback::Stopped,
            A::Shortcuts => self.shortcuts_open = !self.shortcuts_open,
            A::Packages => self.ask(Command::Packages, ctx),
        }
    }
}

/// The shown inputs, stacked upwards from `corner`, the newest lowest.
/// Each fades away a moment after it was pressed.
pub fn inputs(ui: &Ui, k: &Look, st: &mut State, corner: Pos2) {
    if !st.settings.show_inputs {
        return;
    }
    let now = ui.input(|i| i.time);
    st.inputs.retain(|input| now - input.at < INPUT_HOLD + INPUT_FADE);
    if st.inputs.is_empty() {
        return;
    }
    ui.ctx().request_repaint();
    let h = k.d.row + 6.0;
    let p = ui.painter();
    let mut bottom = corner.y;
    for input in st.inputs.iter().rev() {
        let alpha = (1.0 - (now - input.at - INPUT_HOLD) / INPUT_FADE).clamp(0.0, 1.0) as f32;
        let bold = k.bold(k.d.text);
        let keys = p.layout_no_wrap(input.keys.clone(), bold, k.t.text_bright.gamma_multiply(alpha));
        let mut said = input.action.to_owned();
        if input.times > 1 {
            said = format!("{said} ×{}", input.times).trim().to_owned();
        }
        let action = p.layout_no_wrap(said, k.font(), k.t.text_dim.gamma_multiply(alpha));
        let glyph_w = if input.button.is_some() { 18.0 } else { 0.0 };
        let gap = |w: f32, next: f32| if w > 0.0 && next > 0.0 { 6.0 } else { 0.0 };
        let (kw, aw) = (keys.size().x, action.size().x);
        let w = 10.0
            + kw
            + gap(kw, glyph_w)
            + glyph_w
            + gap(kw + glyph_w, aw)
            + if aw > 0.0 { 4.0 } else { 0.0 }
            + aw
            + 10.0;
        let rect = Rect::from_min_size(Pos2::new(corner.x, bottom - h), Vec2::new(w.ceil(), h));
        p.rect_filled(rect, k.radius(), Theme::glass(k.t.well, 242).gamma_multiply(alpha));
        p.rect_stroke(
            rect,
            k.radius(),
            Stroke::new(1.0, Theme::glass(k.t.hover, 110).gamma_multiply(alpha)),
            StrokeKind::Inside,
        );
        let y = rect.center().y;
        let mut x = rect.left() + 10.0;
        p.galley(Pos2::new(x, y - keys.size().y / 2.0), keys, k.t.text_bright);
        x += kw + gap(kw, glyph_w);
        if let Some(g) = input.button {
            p.text(
                Pos2::new(x + 8.0, y),
                Align2::CENTER_CENTER,
                g,
                k.icons(16.0),
                k.t.text_bright.gamma_multiply(alpha),
            );
            x += glyph_w;
        }
        x += gap(kw + glyph_w, aw) + if aw > 0.0 { 4.0 } else { 0.0 };
        p.galley(Pos2::new(x, y - action.size().y / 2.0), action, k.t.text_dim);
        bottom -= h + 4.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_key_reads_and_no_two_actions_share_one() {
        let map = Keymap::default();
        for def in &ACTIONS {
            assert_eq!(map.chords(def.action).len(), def.keys.len(), "{}", def.id);
            assert!(map.clashes(def.action).is_empty(), "{} shares a key", def.id);
        }
        let ids: std::collections::HashSet<_> = ACTIONS.iter().map(|d| d.id).collect();
        assert_eq!(ids.len(), ACTIONS.len());
    }

    #[test]
    fn a_chord_reads_back_as_it_is_written() {
        for text in ["Ctrl+Shift+S", "F2", "Alt+P", "Backtick", "1", "Shift+Space"] {
            let chord = Chord::parse(text).unwrap();
            assert_eq!(chord.stored(), text);
        }
        assert_eq!(Chord::parse("Ctrl+Z").unwrap().text(), "Ctrl+Z");
        assert_eq!(Chord::parse("Backtick").unwrap().text(), "`");
        assert_eq!(Chord::parse("Ctrl+Nothing"), None);
        assert_eq!(Chord::parse("Ctrl"), None);
    }

    #[test]
    fn only_changed_keys_are_kept_and_they_come_back() {
        let mut map = Keymap::default();
        assert!(map.changes().is_empty());
        let chord = Chord::parse("Ctrl+Z").unwrap();
        map.set(Action::Focus, 0, chord);
        map.clear(Action::Rename, 0);
        let changes = map.changes();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes["focus"], ["Ctrl+Z"]);
        assert!(changes["rename"].is_empty());
        assert_eq!(Keymap::with(&changes), map);
        // The clash is told on both sides, and the first listed wins.
        assert_eq!(map.clashes(Action::Focus), ["Undo"]);
        assert_eq!(map.clashes(Action::Undo), ["Focus"]);
        assert_eq!(map.find(chord).map(|d| d.action), Some(Action::Undo));
        map.reset(Action::Focus);
        assert!(map.is_default(Action::Focus));
        assert!(!map.is_default(Action::Rename));
    }
}
