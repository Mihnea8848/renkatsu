// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The complete designs offered for comparison. They share the theme, the
//! layout and every feature; a design decides sizes and how the parts are
//! drawn and arranged. D is the chosen mix of the first three, which stay
//! for comparison until they are removed.

use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Design {
    /// Docked bars: the mode rail and the selection bar sit on the
    /// viewport's edges, Unreal 5 style tabs and Details rows.
    A,
    /// Floating overlays: compact blocks float over the viewport as in
    /// Unity 6, with denser rows and an Inspector style Properties panel.
    B,
    /// Clean viewport: the mode rail is part of the window, the selection
    /// bar lives in the viewport's header row, panel titles are underlined.
    C,
    /// The three combined: the tabs and Details rows of A, the floating
    /// blocks of B, the Outliner of C, and the selection bar in the
    /// viewport's header row.
    D,
}

/// The window's title bar, the strip above the menu row that moves the
/// window and carries its buttons. Three are offered for comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TitleBar {
    /// The mark at the left, the project and the application's name in
    /// the centre.
    A,
    /// The mark and the application's name at the left, then the project
    /// and the scene as breadcrumbs.
    B,
    /// A slim strip with the title only; the mark stays in the menu row.
    C,
}

impl TitleBar {
    pub fn height(self) -> f32 {
        match self {
            TitleBar::A | TitleBar::B => 30.0,
            TitleBar::C => 22.0,
        }
    }
}

/// How the tabs of docked panels are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tabs {
    /// The active tab takes the panel's colour.
    Filled,
    /// As `Filled`, with a line in the selection colour along its top.
    TopLine,
    /// No tab shapes: titles on the panel's colour, the active underlined.
    Underline,
}

/// Where the modes, their tools and the selection bar are placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlays {
    /// Bars attached to the viewport's top and left edges.
    Docked,
    /// Separate blocks floating over the viewport.
    Floating,
    /// The rail at the window's left edge, the selection bar in the
    /// viewport's header row.
    Header,
    /// Separate blocks floating over the viewport, except the selection
    /// bar, which lives in the viewport's header row.
    Blocks,
}

impl Overlays {
    /// The selection bar belongs to the viewport's header row.
    pub fn bar_in_header(self) -> bool {
        matches!(self, Overlays::Header | Overlays::Blocks)
    }
}

/// How the Properties panel lays out its sections and rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Props {
    /// Unreal's Details: header bars, a name and a value column, row lines.
    Details,
    /// Unity's Inspector: component headers, axis letters before fields.
    Inspector,
    /// Right aligned names and joined fields with the axis letter inside.
    Joined,
}

/// How the Outliner draws the scene tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tree {
    /// A visibility column, the name and a type column under a header row.
    Columns,
    /// A plain tree.
    Plain,
    /// A tree with indent guides and visibility at the right edge.
    Guides,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spec {
    pub name: &'static str,
    /// Size of body text.
    pub text: f32,
    /// Size of captions and secondary text.
    pub small: f32,
    /// Height of list rows, fields and buttons.
    pub row: f32,
    pub menu_height: f32,
    pub tab_height: f32,
    pub status_height: f32,
    /// Width of the divider between docked panels.
    pub separator: f32,
    pub tabs: Tabs,
    /// Panel tabs carry an icon before the title.
    pub tab_icons: bool,
    pub overlays: Overlays,
    pub props: Props,
    pub tree: Tree,
}

impl Design {
    #[cfg(test)]
    pub const ALL: [Design; 4] = [Design::A, Design::B, Design::C, Design::D];

    pub fn spec(self) -> Spec {
        match self {
            Design::A => Spec {
                name: "A, docked bars",
                text: 13.0,
                small: 11.0,
                row: 24.0,
                menu_height: 34.0,
                tab_height: 28.0,
                status_height: 24.0,
                separator: 2.0,
                tabs: Tabs::Filled,
                tab_icons: true,
                overlays: Overlays::Docked,
                props: Props::Details,
                tree: Tree::Columns,
            },
            Design::B => Spec {
                name: "B, floating overlays",
                text: 12.0,
                small: 10.5,
                row: 22.0,
                menu_height: 30.0,
                tab_height: 25.0,
                status_height: 22.0,
                separator: 1.0,
                tabs: Tabs::TopLine,
                tab_icons: false,
                overlays: Overlays::Floating,
                props: Props::Inspector,
                tree: Tree::Plain,
            },
            Design::C => Spec {
                name: "C, clean viewport",
                text: 13.0,
                small: 11.0,
                row: 24.0,
                menu_height: 36.0,
                tab_height: 32.0,
                status_height: 24.0,
                separator: 2.0,
                tabs: Tabs::Underline,
                tab_icons: false,
                overlays: Overlays::Header,
                props: Props::Joined,
                tree: Tree::Guides,
            },
            Design::D => Spec {
                name: "D, combined",
                text: 13.0,
                small: 11.0,
                row: 24.0,
                menu_height: 34.0,
                tab_height: 28.0,
                status_height: 24.0,
                separator: 2.0,
                tabs: Tabs::Filled,
                tab_icons: true,
                overlays: Overlays::Blocks,
                props: Props::Details,
                tree: Tree::Guides,
            },
        }
    }
}
