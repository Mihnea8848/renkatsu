// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! What the editor offers and where: the modes with their tools, the
//! transform tools, and the actions that fit each kind of selection. All
//! of it is placeholder data until the features exist.

use crate::kinds::Kind;
use egui_phosphor::regular as icon;

/// An icon and a name.
pub type Entry = (&'static str, &'static str);

pub struct Mode {
    pub glyph: &'static str,
    pub name: &'static str,
    /// The mode's tools in named sections.
    pub sections: &'static [(&'static str, &'static [Entry])],
}

/// A rendering pipeline: what turns the scene into the picture.
pub struct Pipeline {
    /// The abbreviation, for places with little room.
    pub short: &'static str,
    pub name: &'static str,
    /// Its name as a tool of the Light mode.
    pub tool: &'static str,
    /// Whether it exists yet.
    pub available: bool,
}

/// The pipelines on offer. KRP reproduces Koikatsu's picture; RRP will be
/// the project's own look and does not exist yet.
pub const PIPELINES: [Pipeline; 2] = [
    Pipeline {
        short: "KRP",
        name: "Koikatsu Rendering Pipeline",
        tool: "Koikatsu (KRP)",
        available: true,
    },
    Pipeline {
        short: "RRP",
        name: "Renkatsu Rendering Pipeline",
        tool: "Renkatsu (RRP)",
        available: false,
    },
];

/// The tip of a pipeline that cannot be picked yet.
pub fn pipeline_missing(pipeline: &Pipeline) -> String {
    format!("The {} ({}) is not available yet", pipeline.name, pipeline.short)
}

pub const MODES: [Mode; 7] = [
    Mode {
        glyph: icon::CURSOR,
        name: "Select",
        sections: &[
            (
                "Pick",
                &[
                    (icon::CURSOR_CLICK, "Objects"),
                    (icon::SELECTION, "Box select"),
                    (icon::SELECTION_ALL, "All of this type"),
                ],
            ),
            (
                "Filter",
                &[
                    (icon::USER, "Characters"),
                    (icon::CUBE, "Items"),
                    (icon::LIGHTBULB, "Lights and cameras"),
                ],
            ),
        ],
    },
    Mode {
        glyph: icon::USER,
        name: "Character",
        sections: &[
            (
                "Card",
                &[
                    (icon::IDENTIFICATION_CARD, "Load card"),
                    (icon::SWAP, "Replace"),
                    (icon::ARROWS_COUNTER_CLOCKWISE, "Reload"),
                ],
            ),
            (
                "Wear",
                &[
                    (icon::COAT_HANGER, "Clothing"),
                    (icon::T_SHIRT, "Outfit"),
                    (icon::SNEAKER, "Shoes"),
                    (icon::DIAMOND, "Accessories"),
                ],
            ),
            ("Face", &[(icon::SMILEY, "Expression"), (icon::EYE, "Gaze")]),
        ],
    },
    Mode {
        glyph: icon::PERSON_ARMS_SPREAD,
        name: "Pose",
        sections: &[
            (
                "Edit",
                &[
                    (icon::BONE, "Forward kinematics"),
                    (icon::TARGET, "Inverse kinematics"),
                    (icon::FLIP_HORIZONTAL, "Mirror pose"),
                    (icon::COPY_SIMPLE, "Copy to other side"),
                    (icon::FILM_STRIP, "Bake animation"),
                ],
            ),
            ("Keys", &[(icon::DIAMOND, "Set key"), (icon::TRASH, "Delete key")]),
            (
                "Library",
                &[
                    (icon::BOOKMARKS_SIMPLE, "Load pose"),
                    (icon::BOOKMARK_SIMPLE, "Save pose"),
                    (icon::ARROW_U_UP_LEFT, "Reset pose"),
                ],
            ),
            (
                "Body",
                &[
                    (icon::HAND, "Hands"),
                    (icon::SMILEY, "Expression"),
                    (icon::EYE, "Look at camera"),
                ],
            ),
        ],
    },
    Mode {
        glyph: icon::LIGHTBULB,
        name: "Light",
        sections: &[
            ("Lights", &[(icon::LIGHTBULB, "Add light"), (icon::SUN, "Key light")]),
            (
                "Effects",
                &[
                    (icon::CIRCLE_HALF, "Ambient occlusion"),
                    (icon::SPARKLE, "Bloom"),
                    (icon::APERTURE, "Depth of field"),
                ],
            ),
            (
                "Rendering pipeline",
                &[
                    (icon::PAINT_BRUSH, PIPELINES[0].tool),
                    (icon::SPHERE, PIPELINES[1].tool),
                ],
            ),
        ],
    },
    // The map's own controls: its palette is drawn by map_menu.rs.
    Mode {
        glyph: icon::MAP_TRIFOLD,
        name: "Map",
        sections: &[],
    },
    Mode {
        glyph: icon::FILM_STRIP,
        name: "Animate",
        sections: &[
            (
                "Keys",
                &[
                    (icon::DIAMOND, "Set key"),
                    (icon::RECORD, "Auto key"),
                    (icon::TRASH, "Delete key"),
                ],
            ),
            (
                "Clips",
                &[(icon::FILM_REEL, "Load clip"), (icon::WAVEFORM, "Bake physics")],
            ),
            ("Show", &[(icon::GHOST, "Onion skin"), (icon::PATH, "Motion trail")]),
        ],
    },
    Mode {
        glyph: icon::FILM_SLATE,
        name: "Bake",
        sections: &[
            (
                "Shots",
                &[
                    (icon::CAMERA_PLUS, "New shot from view"),
                    (icon::LIST_BULLETS, "Shot list"),
                ],
            ),
            (
                "Render",
                &[(icon::IMAGE, "Still"), (icon::VIDEO, "Video"), (icon::QUEUE, "Queue")],
            ),
            ("Ren'Py", &[(icon::EXPORT, "Export"), (icon::ARROWS_CLOCKWISE, "Sync")]),
        ],
    },
];

/// Modes that do not exist yet, shown greyed at the end of the rail:
/// playing the scene as a game, in first or third person, and playing
/// Ren'Py scripts.
pub const LATER: [Entry; 2] = [(icon::GAME_CONTROLLER, "Game"), (icon::BOOK_OPEN_TEXT, "Story")];

/// The transform tools of the floating strip: icon, name, and a key held
/// for it that the keymap does not list.
pub const TOOLS: [(&str, &str, &str); 6] = [
    (icon::HAND, "View", "Alt"),
    (icon::CURSOR, "Select", ""),
    (icon::ARROWS_OUT_CARDINAL, "Move", ""),
    (icon::ARROWS_CLOCKWISE, "Rotate", ""),
    (icon::ARROWS_OUT, "Scale", ""),
    (icon::BONE, "Pose", ""),
];

/// An action of the selection bar: icon, name, whether it opens a menu.
pub type Action = (&'static str, &'static str, bool);

/// What the selection bar offers for a kind of object.
pub fn actions(kind: Kind) -> &'static [Action] {
    match kind {
        Kind::Scene => &[
            (icon::PLUS, "Add", true),
            (icon::MOUNTAINS, "Environment", true),
            (icon::FLOPPY_DISK, "Save scene", false),
        ],
        Kind::Character => &[
            // Outfit, clothing and expressions live in the Character
            // mode's palette; the bar keeps what matters most.
            (icon::IDENTIFICATION_CARD, "Load card", false),
            (icon::PUZZLE_PIECE, "Modules", true),
        ],
        Kind::Outfit => &[
            (icon::T_SHIRT, "Change outfit", true),
            (icon::STACK, "Clothing states", true),
            (icon::PALETTE, "Colours", false),
        ],
        Kind::Accessory => &[
            (icon::PLUS, "Add accessory", false),
            (icon::LINK, "Attach to", true),
            (icon::PALETTE, "Colours", false),
        ],
        Kind::Item => &[
            (icon::SWAP, "Replace", false),
            (icon::LINK, "Attach to", true),
            (icon::PALETTE, "Material", true),
        ],
        Kind::Light => &[
            (icon::LIGHTBULB, "Type", true),
            (icon::PALETTE, "Colour", true),
            (icon::CIRCLE_HALF, "Shadows", true),
        ],
        Kind::Camera => &[
            (icon::EYE, "Look through", false),
            (icon::APERTURE, "Lens", true),
            (icon::CAMERA_PLUS, "New shot", false),
        ],
        Kind::Effect => &[],
        Kind::Map => &[
            (icon::SWAP, "Change map", false),
            (icon::SUN_HORIZON, "Time of day", true),
        ],
        Kind::Folder => &[(icon::PLUS, "Add", true), (icon::EYE_SLASH, "Hide all", false)],
        Kind::Animation => &[(icon::PLAY, "Apply", false)],
        Kind::Pose => &[
            (icon::BOOKMARKS_SIMPLE, "Load pose", false),
            (icon::BOOKMARK_SIMPLE, "Save pose", false),
            (icon::FLIP_HORIZONTAL, "Mirror", false),
        ],
        Kind::Script => &[(icon::PENCIL_SIMPLE, "Open", false), (icon::PLAY, "Run", false)],
    }
}
