// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The kinds of object a scene and a project hold. Each has an icon and a
//! colour, the same in every list, bar and menu: the icon takes the colour
//! and the text beside it stays neutral.

use egui::Color32;
use egui_phosphor::regular as icon;

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
    /// The type colour: muted, close to Unreal's class icons.
    pub fn color(self) -> Color32 {
        let [r, g, b] = match self {
            Kind::Scene => [0x8c, 0x93, 0x9c],
            Kind::Character => [0xc4, 0x7a, 0x93],
            Kind::Outfit => [0x9e, 0x86, 0xb8],
            Kind::Accessory => [0x8e, 0x88, 0xb8],
            Kind::Item => [0xb8, 0x8c, 0x5c],
            Kind::Light => [0xc8, 0xb0, 0x5a],
            Kind::Camera => [0x6a, 0x9c, 0xc0],
            Kind::Map => [0x78, 0xa8, 0x6c],
            Kind::Folder => [0xa8, 0x96, 0x74],
            Kind::Pose => [0x5c, 0xa8, 0x98],
            Kind::Script => [0x84, 0x84, 0xc0],
        };
        Color32::from_rgb(r, g, b)
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Kind::Scene => icon::GLOBE_HEMISPHERE_WEST,
            Kind::Character => icon::USER,
            Kind::Outfit => icon::T_SHIRT,
            Kind::Accessory => icon::DIAMOND,
            Kind::Item => icon::CUBE,
            Kind::Light => icon::LIGHTBULB,
            Kind::Camera => icon::VIDEO_CAMERA,
            Kind::Map => icon::MAP_TRIFOLD,
            Kind::Folder => icon::FOLDER_SIMPLE,
            Kind::Pose => icon::PERSON_ARMS_SPREAD,
            Kind::Script => icon::CODE,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Scene => "Scene",
            Kind::Character => "Character",
            Kind::Outfit => "Outfit",
            Kind::Accessory => "Accessories",
            Kind::Item => "Item",
            Kind::Light => "Light",
            Kind::Camera => "Camera",
            Kind::Map => "Map",
            Kind::Folder => "Folder",
            Kind::Pose => "Pose",
            Kind::Script => "Script",
        }
    }
}
