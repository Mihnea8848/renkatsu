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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Appearance {
    pub top_bar: TopBar,
    pub tabs: Tabs,
    pub type_icons: TypeIcons,
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
    /// The type colour, the same in every list and menu.
    pub fn color(self) -> Color32 {
        match self {
            Kind::Scene => Color32::from_rgb(0x9a, 0xa4, 0xb1),
            Kind::Character => Color32::from_rgb(0xe8, 0x7a, 0xa8),
            Kind::Outfit => Color32::from_rgb(0xc0, 0x8b, 0xe0),
            Kind::Accessory => Color32::from_rgb(0xb0, 0x9c, 0xf0),
            Kind::Item => Color32::from_rgb(0xe8, 0x9a, 0x4c),
            Kind::Light => Color32::from_rgb(0xf0, 0xc8, 0x40),
            Kind::Camera => Color32::from_rgb(0x4c, 0xb8, 0xe8),
            Kind::Map => Color32::from_rgb(0x6c, 0xc4, 0x6c),
            Kind::Folder => Color32::from_rgb(0xc8, 0xa8, 0x70),
            Kind::Pose => Color32::from_rgb(0x48, 0xc8, 0xb0),
            Kind::Script => Color32::from_rgb(0x90, 0x8c, 0xf0),
        }
    }

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
