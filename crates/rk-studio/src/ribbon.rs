// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The ribbon: one tab per category from docs/editor.md, each with labelled
//! groups of buttons. Clicking the active tab again collapses the ribbon to
//! its tab row.

use crate::theme::Theme;
use egui::{Color32, RichText, Sense, Ui, Vec2};
use egui_phosphor::regular as icon;

pub struct Group {
    pub name: &'static str,
    pub buttons: &'static [(&'static str, &'static str)],
}

pub struct Tab {
    pub name: &'static str,
    pub groups: &'static [Group],
}

pub const TABS: &[Tab] = &[
    Tab {
        name: "Home",
        groups: &[
            Group {
                name: "Project",
                buttons: &[
                    (icon::FILE_PLUS, "New"),
                    (icon::FOLDER_OPEN, "Open"),
                    (icon::FLOPPY_DISK, "Save"),
                ],
            },
            Group {
                name: "Edit",
                buttons: &[
                    (icon::ARROW_COUNTER_CLOCKWISE, "Undo"),
                    (icon::ARROW_CLOCKWISE, "Redo"),
                    (icon::COPY, "Duplicate"),
                    (icon::TRASH, "Delete"),
                ],
            },
            Group {
                name: "Tools",
                buttons: &[
                    (icon::CURSOR, "Select"),
                    (icon::ARROWS_OUT_CARDINAL, "Move"),
                    (icon::ARROWS_CLOCKWISE, "Rotate"),
                    (icon::ARROWS_OUT, "Scale"),
                    (icon::GLOBE_SIMPLE, "World"),
                    (icon::MAGNET, "Snap"),
                ],
            },
            Group {
                name: "Add",
                buttons: &[
                    (icon::USER_PLUS, "Character"),
                    (icon::CUBE, "Item"),
                    (icon::LIGHTBULB, "Light"),
                    (icon::VIDEO_CAMERA, "Camera"),
                    (icon::MAP_TRIFOLD, "Map"),
                    (icon::FOLDER_SIMPLE, "Folder"),
                ],
            },
            Group {
                name: "Physics",
                buttons: &[
                    (icon::PLAY, "Play"),
                    (icon::PAUSE, "Pause"),
                    (icon::ARROW_U_UP_LEFT, "Reset"),
                ],
            },
        ],
    },
    Tab {
        name: "Scene",
        groups: &[
            Group {
                name: "Organise",
                buttons: &[
                    (icon::FOLDERS, "Group"),
                    (icon::TREE_STRUCTURE, "Parent"),
                    (icon::EYE_SLASH, "Hide"),
                    (icon::LOCK_SIMPLE, "Lock"),
                ],
            },
            Group {
                name: "Map",
                buttons: &[(icon::MAP_TRIFOLD, "Load map"), (icon::SUN_HORIZON, "Environment")],
            },
        ],
    },
    Tab {
        name: "Character",
        groups: &[
            Group {
                name: "Card",
                buttons: &[
                    (icon::IDENTIFICATION_CARD, "Load"),
                    (icon::SWAP, "Replace"),
                    (icon::ARROWS_COUNTER_CLOCKWISE, "Reload"),
                ],
            },
            Group {
                name: "Outfit",
                buttons: &[
                    (icon::T_SHIRT, "Outfit"),
                    (icon::SNEAKER, "Shoes"),
                    (icon::STACK, "States"),
                ],
            },
            Group {
                name: "Expression",
                buttons: &[(icon::EYE, "Eyes"), (icon::SMILEY, "Mouth"), (icon::CROSSHAIR, "Gaze")],
            },
            Group {
                name: "Modules",
                buttons: &[(icon::PUZZLE_PIECE, "Plugins")],
            },
        ],
    },
    Tab {
        name: "Pose",
        groups: &[
            Group {
                name: "Library",
                buttons: &[
                    (icon::BOOKMARKS_SIMPLE, "Load pose"),
                    (icon::BOOKMARK_SIMPLE, "Save pose"),
                ],
            },
            Group {
                name: "Mode",
                buttons: &[(icon::BONE, "FK"), (icon::TARGET, "IK")],
            },
            Group {
                name: "Edit",
                buttons: &[(icon::FLIP_HORIZONTAL, "Mirror"), (icon::ARROW_U_UP_LEFT, "Reset")],
            },
            Group {
                name: "Body",
                buttons: &[(icon::HAND, "Hands"), (icon::EYE, "Look at camera")],
            },
        ],
    },
    Tab {
        name: "Animate",
        groups: &[Group {
            name: "Timeline",
            buttons: &[
                (icon::FILM_STRIP, "Timeline"),
                (icon::DIAMOND, "Key"),
                (icon::FILM_REEL, "Clips"),
            ],
        }],
    },
    Tab {
        name: "Light",
        groups: &[
            Group {
                name: "Lights",
                buttons: &[(icon::LIGHTBULB, "Add light"), (icon::SUN, "Key light")],
            },
            Group {
                name: "Effects",
                buttons: &[
                    (icon::CIRCLE_HALF, "Occlusion"),
                    (icon::SPARKLE, "Bloom"),
                    (icon::APERTURE, "Depth of field"),
                    (icon::VIGNETTE, "Vignette"),
                    (icon::PALETTE, "Colour"),
                ],
            },
            Group {
                name: "Look",
                buttons: &[(icon::PAINT_BRUSH, "Koikatsu"), (icon::SLIDERS_HORIZONTAL, "Quality")],
            },
        ],
    },
    Tab {
        name: "Bake",
        groups: &[
            Group {
                name: "Shots",
                buttons: &[(icon::CAMERA_PLUS, "New shot")],
            },
            Group {
                name: "Render",
                buttons: &[
                    (icon::IMAGE, "Still"),
                    (icon::FILM_SLATE, "Video"),
                    (icon::QUEUE, "Queue"),
                ],
            },
            Group {
                name: "Ren'Py",
                buttons: &[(icon::EXPORT, "Export"), (icon::ARROWS_CLOCKWISE, "Sync")],
            },
        ],
    },
    Tab {
        name: "View",
        groups: &[
            Group {
                name: "Layout",
                buttons: &[
                    (icon::LAYOUT, "Reset"),
                    (icon::FLOPPY_DISK, "Save"),
                    (icon::FOLDER_OPEN, "Load"),
                ],
            },
            Group {
                name: "Viewport",
                buttons: &[
                    (icon::GRID_FOUR, "Grid"),
                    (icon::ARROWS_OUT_CARDINAL, "Gizmos"),
                    (icon::GAUGE, "Stats"),
                ],
            },
            Group {
                name: "Theme",
                buttons: &[(icon::PALETTE, "Next theme")],
            },
        ],
    },
];

/// What the user clicked in the ribbon this frame.
pub enum Action {
    None,
    Pressed(&'static str),
}

pub struct Ribbon {
    pub tab: usize,
    pub collapsed: bool,
}

impl Ribbon {
    /// The row of category tabs. Clicking the active tab again collapses
    /// the ribbon to this row.
    pub fn tab_row(&mut self, ui: &mut Ui, theme: &Theme) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, tab) in TABS.iter().enumerate() {
                let on = i == self.tab;
                let text = RichText::new(tab.name).color(if on { theme.text } else { theme.text_dim });
                let size = Vec2::new(tab.name.len() as f32 * 7.5 + 24.0, 24.0);
                let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                if on {
                    ui.painter().rect_filled(rect, 0.0, theme.panel);
                    ui.painter()
                        .hline(rect.x_range(), rect.top() + 0.5, egui::Stroke::new(2.0, theme.select));
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 0.0, theme.control);
                }
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    text.text(),
                    egui::FontId::proportional(13.0),
                    if on { theme.text } else { theme.text_dim },
                );
                if resp.clicked() {
                    if on {
                        self.collapsed = !self.collapsed;
                    } else {
                        self.tab = i;
                        self.collapsed = false;
                    }
                }
            }
        });
    }

    pub fn show(&mut self, ui: &mut Ui, theme: &Theme) -> Action {
        let mut action = Action::None;
        self.tab_row(ui, theme);
        if self.collapsed {
            return action;
        }
        // Groups of the active tab.
        egui::Frame::new()
            .fill(theme.panel)
            .inner_margin(egui::Margin::symmetric(6, 4))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    for (gi, group) in TABS[self.tab].groups.iter().enumerate() {
                        if gi > 0 {
                            let x = ui.cursor().left() - 3.0;
                            let y = ui.min_rect().y_range();
                            ui.painter().vline(
                                x,
                                (y.min + 2.0)..=(y.min + 62.0),
                                egui::Stroke::new(1.0, theme.divider),
                            );
                        }
                        ui.vertical(|ui| {
                            let row = ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                for &(glyph, label) in group.buttons {
                                    if big_button(ui, theme, glyph, label).clicked() {
                                        action = Action::Pressed(label);
                                    }
                                }
                            });
                            // The group's name, centred under its own buttons.
                            let width = row.response.rect.width();
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 14.0), Sense::hover());
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                group.name,
                                egui::FontId::proportional(11.0),
                                theme.text_dim,
                            );
                        });
                        ui.add_space(6.0);
                    }
                });
            });
        action
    }
}

/// An icon above a label, as in a ribbon.
fn big_button(ui: &mut Ui, theme: &Theme, glyph: &str, label: &str) -> egui::Response {
    let width = (label.len() as f32 * 6.5 + 12.0).max(46.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 44.0), Sense::click());
    let fill = if resp.is_pointer_button_down_on() {
        theme.select
    } else if resp.hovered() {
        theme.control
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, theme.control_radius as f32, fill);
    let p = ui.painter();
    p.text(
        rect.center_top() + Vec2::new(0.0, 13.0),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(18.0),
        theme.text,
    );
    p.text(
        rect.center_bottom() - Vec2::new(0.0, 10.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(11.5),
        theme.text,
    );
    resp.on_hover_text(label)
}
