// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the shell remembers between frames: the look in use, the selection,
//! the state of the viewport's overlays and the placeholder values the
//! panels edit. A real scene model replaces the placeholder parts later.

use crate::design::Spec;
use crate::kinds::Kind;
use crate::theme::{Theme, ICONS, SEMIBOLD};
use egui::{Color32, FontFamily, FontId, TextureHandle};

/// The theme and the chosen design, passed to everything that draws.
#[derive(Debug, Clone, Copy)]
pub struct Look {
    pub t: Theme,
    pub d: Spec,
}

impl Look {
    /// Body text.
    pub fn font(&self) -> FontId {
        FontId::proportional(self.d.text)
    }

    /// Captions and secondary text.
    pub fn small(&self) -> FontId {
        FontId::proportional(self.d.small)
    }

    pub fn bold(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
    }

    pub fn icons(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(ICONS.into()))
    }

    pub fn mono(&self) -> FontId {
        FontId::monospace(self.d.text - 1.0)
    }

    pub fn radius(&self) -> f32 {
        f32::from(self.t.radius)
    }

    /// Height of fields and buttons inside a row.
    pub fn field(&self) -> f32 {
        self.d.row - 2.0
    }
}

/// How much of the mode rail shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rail {
    /// Icons with the mode names.
    Named,
    /// Icons only, the names in tooltips.
    Icons,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Physics {
    Stopped,
    Playing,
    Paused,
}

/// The placeholder scene: depth in the tree, kind and name of each object.
pub const SCENE: [(usize, Kind, &str); 11] = [
    (0, Kind::Scene, "Untitled scene"),
    (1, Kind::Folder, "Cast"),
    (2, Kind::Character, "Simple"),
    (3, Kind::Outfit, "Outfit 1"),
    (3, Kind::Accessory, "Accessories"),
    (3, Kind::Pose, "Standing pose"),
    (1, Kind::Item, "Chair"),
    (1, Kind::Light, "Key light"),
    (1, Kind::Camera, "Camera 1"),
    (1, Kind::Map, "Rooftop"),
    (1, Kind::Script, "Scene script"),
];

/// Whether a scene row has rows below it in the tree.
pub fn has_children(row: usize) -> bool {
    SCENE.get(row + 1).is_some_and(|next| next.0 > SCENE[row].0)
}

pub struct State {
    /// Index into `SCENE` of the selected object.
    pub selected: Option<usize>,
    /// Scene rows whose children are folded away.
    pub folded: [bool; SCENE.len()],
    /// Scene rows hidden in the viewport.
    pub hidden: [bool; SCENE.len()],

    /// Index into `modes::MODES`.
    pub mode: usize,
    /// The active tool of each mode, counted through its sections.
    pub mode_tool: [usize; 6],
    /// The active mode's tool palette is open.
    pub palette_open: bool,
    /// The palette stays open while working in the viewport.
    pub palette_pinned: bool,
    pub rail: Rail,
    /// Every viewport overlay is hidden.
    pub overlays_hidden: bool,
    /// The viewport takes the place of all docked panels.
    pub maximised: bool,
    /// Index into `modes::TOOLS`.
    pub tool: usize,
    pub snap: bool,
    pub physics: Physics,

    pub transform: [[f32; 3]; 3],
    pub outfit: usize,
    pub shoes: usize,
    pub simulate: bool,
    pub eyes: usize,
    pub mouth: usize,
    pub modules: [bool; 4],
    pub light_type: usize,
    pub intensity: f32,
    pub shadows: bool,
    pub lens: [f32; 3],
    /// Properties sections that are open.
    pub sections: [bool; 4],

    pub scene_search: String,
    pub property_search: String,
    pub content_search: String,
    pub console_search: String,
    pub command_search: String,
    /// The folder open in the Content panel.
    pub folder: usize,
    /// The asset selected in the Content panel.
    pub asset: usize,
    /// Console levels shown: information, warnings, errors.
    pub levels: [bool; 3],

    pub viewport_image: Option<TextureHandle>,
    /// What surrounds the still picture, taken from its corner so that it
    /// reads as a viewport filled edge to edge.
    pub backdrop: Color32,
}

impl State {
    pub fn new() -> Self {
        State {
            selected: Some(2),
            folded: [false; SCENE.len()],
            hidden: [false; SCENE.len()],
            mode: 2,
            mode_tool: [0, 0, 1, 0, 0, 0],
            palette_open: true,
            palette_pinned: true,
            rail: Rail::Named,
            overlays_hidden: false,
            maximised: false,
            tool: 2,
            snap: true,
            physics: Physics::Stopped,
            transform: [[0.0; 3], [0.0; 3], [1.0; 3]],
            outfit: 0,
            shoes: 0,
            simulate: true,
            eyes: 0,
            mouth: 0,
            modules: [true, true, true, false],
            light_type: 0,
            intensity: 1.0,
            shadows: true,
            lens: [35.0, 0.05, 100.0],
            sections: [true, true, true, false],
            scene_search: String::new(),
            property_search: String::new(),
            content_search: String::new(),
            console_search: String::new(),
            command_search: String::new(),
            folder: 0,
            asset: 0,
            levels: [true; 3],
            viewport_image: None,
            backdrop: Color32::from_rgb(0x0f, 0x0f, 0x0f),
        }
    }

    /// The kind and name of the selected object.
    pub fn selection(&self) -> Option<(Kind, &'static str)> {
        let (_, kind, name) = *SCENE.get(self.selected?)?;
        Some((kind, name))
    }

    /// Switches to a mode; choosing the active mode again folds its tools.
    pub fn pick_mode(&mut self, mode: usize) {
        if self.mode == mode {
            self.palette_open = !self.palette_open;
        } else {
            self.mode = mode;
            self.palette_open = true;
        }
    }
}
