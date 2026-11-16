// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Look options that can be switched while the editor runs, so each can be
//! compared on screenshots before one is chosen.

use clap::ValueEnum;
use egui::Color32;

/// The area above the docked panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TopBar {
    /// Menu bar, then one toolbar row with category dropdowns, as Unreal 5.
    Unreal,
    /// Category menus in the menu bar and a slim tool row, as Unity 6.
    Unity,
    /// Category tabs over one row of small icon buttons.
    SlimRibbon,
    /// Category tabs over labelled button groups.
    Ribbon,
    /// One taller row of labelled buttons and labelled category dropdowns.
    Labelled,
    /// Office and 3ds Max style: a large button per group, the rest stacked.
    ProRibbon,
    /// Labelled category modes, the active one's tools in a row below.
    ModeBar,
    /// Unity 6: a slim toolbar, tools as overlays inside the viewport,
    /// categories in the menu bar.
    UnityOverlay,
    /// Unreal 5: a slim toolbar with a mode selector; the mode's tools in a
    /// docked Tools panel on the left.
    UnrealModes,
    /// No toolbar: a bar above the viewport offers what fits the selection.
    ContextBar,
    /// Unreal's modes and Tools panel with the context bar; the mode
    /// selector, Add and play controls sit in the menu bar row.
    Combined,
    /// As Combined, with the modes as a named vertical rail beside the tools.
    CombinedRail,
}

impl TopBar {
    /// Whether the active mode's tools live in a docked Tools panel.
    pub fn has_tools_panel(self) -> bool {
        matches!(self, TopBar::UnrealModes | TopBar::Combined | TopBar::CombinedRail)
    }

    /// Whether the viewport carries the selection's context bar and the
    /// floating tool strip.
    pub fn has_context_bar(self) -> bool {
        matches!(self, TopBar::ContextBar | TopBar::Combined | TopBar::CombinedRail)
    }

    /// Whether there is no toolbar row under the menu bar.
    pub fn no_toolbar(self) -> bool {
        matches!(self, TopBar::ContextBar | TopBar::Combined | TopBar::CombinedRail)
    }
}

/// How docked panel tabs are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Tabs {
    /// The active tab matches the panel, with an accent line under its name.
    Unreal,
    /// Flat: the active tab matches the panel, the others are darker.
    Unity,
    /// No tab shapes, the active title underlined.
    Underline,
}

/// How an object's type shows in lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TypeIcons {
    /// The glyph takes the type's colour.
    Glyph,
    /// A light glyph on a small square in the type's colour.
    Chip,
    /// A neutral glyph and a coloured bar at the row's left edge.
    Strip,
    /// Monochrome glyphs.
    Plain,
}

/// Colour sets for object types, in `Kind::ALL` order: scene, character,
/// outfit, accessory, item, light, camera, map, folder, pose, script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TypePalette {
    /// Light, soft colours.
    Pastel,
    /// Muted colours close to Unreal's class icons.
    Muted,
    /// Saturated colours that stand out on dark panels.
    Vivid,
    /// Warm colours for people and things, cool ones for the scene.
    WarmCool,
}

impl TypePalette {
    pub fn colors(self) -> [[u8; 3]; 11] {
        match self {
            TypePalette::Pastel => [
                [0x9a, 0xa4, 0xb1],
                [0xe8, 0x7a, 0xa8],
                [0xc0, 0x8b, 0xe0],
                [0xb0, 0x9c, 0xf0],
                [0xe8, 0x9a, 0x4c],
                [0xf0, 0xc8, 0x40],
                [0x4c, 0xb8, 0xe8],
                [0x6c, 0xc4, 0x6c],
                [0xc8, 0xa8, 0x70],
                [0x48, 0xc8, 0xb0],
                [0x90, 0x8c, 0xf0],
            ],
            TypePalette::Muted => [
                [0x8c, 0x93, 0x9c],
                [0xc4, 0x7a, 0x93],
                [0x9e, 0x86, 0xb8],
                [0x8e, 0x88, 0xb8],
                [0xb8, 0x8c, 0x5c],
                [0xc8, 0xb0, 0x5a],
                [0x6a, 0x9c, 0xc0],
                [0x78, 0xa8, 0x6c],
                [0xa8, 0x96, 0x74],
                [0x5c, 0xa8, 0x98],
                [0x84, 0x84, 0xc0],
            ],
            TypePalette::Vivid => [
                [0xb4, 0xbc, 0xc8],
                [0xff, 0x5c, 0x9a],
                [0xc8, 0x6c, 0xff],
                [0x9a, 0x7c, 0xff],
                [0xff, 0x8c, 0x1a],
                [0xff, 0xd0, 0x1a],
                [0x1a, 0xb4, 0xff],
                [0x3c, 0xd6, 0x5c],
                [0xe0, 0xb0, 0x50],
                [0x1a, 0xd8, 0xb4],
                [0x7c, 0x84, 0xff],
            ],
            TypePalette::WarmCool => [
                [0x8c, 0xa0, 0xb8],
                [0xf0, 0x80, 0x70],
                [0xf0, 0xa0, 0x70],
                [0xe8, 0xb8, 0x70],
                [0xd8, 0xa0, 0x58],
                [0xf0, 0xd0, 0x60],
                [0x60, 0xa8, 0xe0],
                [0x58, 0xb8, 0xa8],
                [0x90, 0xa8, 0xc8],
                [0xf0, 0x90, 0x98],
                [0x88, 0x98, 0xe8],
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Appearance {
    pub top_bar: TopBar,
    pub tabs: Tabs,
    pub type_icons: TypeIcons,
    pub palette: TypePalette,
}

/// The kinds of object the scene and the project hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Scene,
    Character,
    Outfit,
    Accessory,
    Item,
    Light,
    Camera,
    Map,
    Folder,
    Pose,
    Script,
}

impl Kind {
    /// The type colour in a palette, the same in every list and menu.
    pub fn color(self, palette: TypePalette) -> Color32 {
        let i = Kind::ALL.iter().position(|k| *k == self).unwrap_or(0);
        let [r, g, b] = palette.colors()[i];
        Color32::from_rgb(r, g, b)
    }

    pub const ALL: [Kind; 11] = [
        Kind::Scene,
        Kind::Character,
        Kind::Outfit,
        Kind::Accessory,
        Kind::Item,
        Kind::Light,
        Kind::Camera,
        Kind::Map,
        Kind::Folder,
        Kind::Pose,
        Kind::Script,
    ];

    pub fn glyph(self) -> &'static str {
        use egui_phosphor::regular as i;
        match self {
            Kind::Scene => i::GLOBE_HEMISPHERE_WEST,
            Kind::Character => i::USER,
            Kind::Outfit => i::T_SHIRT,
            Kind::Accessory => i::DIAMOND,
            Kind::Item => i::CUBE,
            Kind::Light => i::LIGHTBULB,
            Kind::Camera => i::VIDEO_CAMERA,
            Kind::Map => i::MAP_TRIFOLD,
            Kind::Folder => i::FOLDER_SIMPLE,
            Kind::Pose => i::PERSON_ARMS_SPREAD,
            Kind::Script => i::CODE,
        }
    }
}
