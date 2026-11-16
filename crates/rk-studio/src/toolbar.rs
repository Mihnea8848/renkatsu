// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The top bar options other than the full ribbon: an Unreal 5 style
//! toolbar with category dropdowns, a Unity 6 style tool row, and a slim
//! ribbon of small icon buttons. They share the categories in `ribbon::TABS`.

use crate::ribbon::{Ribbon, TABS};
use crate::theme::Theme;
use egui::{Align2, Color32, FontId, Sense, Ui, Vec2};
use egui_phosphor::regular as icon;

/// A square icon button the size of a toolbar row.
fn icon_button(ui: &mut Ui, theme: &Theme, glyph: &str, tip: &str, on: bool) -> egui::Response {
    let h = theme.row + 4.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(h + 2.0, h), Sense::click());
    let fill = if on {
        theme.field
    } else if resp.hovered() {
        theme.control
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, theme.control_radius as f32, fill);
    let color = if on { theme.select } else { theme.text };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(16.0),
        color,
    );
    resp.on_hover_text(tip)
}

/// A labelled dropdown button whose menu lists a category's actions.
pub fn category_menu(ui: &mut Ui, glyph: &str, name: &str, tab: usize) {
    ui.menu_button(format!("{glyph}  {name}  {}", icon::CARET_DOWN), |ui| {
        for (gi, group) in TABS[tab].groups.iter().enumerate() {
            if gi > 0 {
                ui.separator();
            }
            ui.label(egui::RichText::new(group.name).small().weak());
            for &(g, label) in group.buttons {
                if ui.button(format!("{g}  {label}")).clicked() {
                    ui.close();
                }
            }
        }
    });
}

fn divider(ui: &mut Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(9.0, theme.row), Sense::hover());
    ui.painter()
        .vline(rect.center().x, rect.y_range(), egui::Stroke::new(1.0, theme.divider));
}

/// Play, pause and reset for the physics, centred in the remaining space.
pub fn play_controls(ui: &mut Ui, theme: &Theme) {
    for (g, tip, on) in [
        (icon::PLAY, "Play physics", true),
        (icon::PAUSE, "Pause", false),
        (icon::ARROW_U_UP_LEFT, "Reset", false),
    ] {
        let resp = icon_button(ui, theme, g, tip, false);
        if on {
            ui.painter().text(
                resp.rect.center(),
                Align2::CENTER_CENTER,
                g,
                FontId::proportional(16.0),
                theme.active,
            );
        }
    }
}

/// Unreal 5: save, the selection mode, a dropdown per category, play
/// controls, and the layout menu on the right.
pub fn unreal(ui: &mut Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let _ = icon_button(ui, theme, icon::FLOPPY_DISK, "Save", false);
        divider(ui, theme);
        ui.menu_button(format!("{}  Select  {}", icon::CURSOR, icon::CARET_DOWN), |ui| {
            for m in ["Select", "Move", "Rotate", "Scale"] {
                let _ = ui.button(m);
            }
        });
        divider(ui, theme);
        for (i, g) in [
            (0, icon::PLUS_CIRCLE),
            (2, icon::USER),
            (3, icon::PERSON_ARMS_SPREAD),
            (5, icon::LIGHTBULB),
            (6, icon::FILM_SLATE),
        ] {
            let name = if i == 0 { "Add" } else { TABS[i].name };
            category_menu(ui, g, name, i);
        }
        divider(ui, theme);
        play_controls(ui, theme);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(format!("{}  Layout  {}", icon::LAYOUT, icon::CARET_DOWN), |ui| {
                let _ = ui.button("Reset layout");
            });
        });
    });
}

/// Unity 6: transform tools on the left, play controls in the middle,
/// search and layout on the right. Categories are in the menu bar.
pub fn unity(ui: &mut Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (i, (g, tip)) in [
            (icon::HAND, "View (Q)"),
            (icon::ARROWS_OUT_CARDINAL, "Move (W)"),
            (icon::ARROWS_CLOCKWISE, "Rotate (E)"),
            (icon::ARROWS_OUT, "Scale (R)"),
        ]
        .into_iter()
        .enumerate()
        {
            let _ = icon_button(ui, theme, g, tip, i == 1);
        }
        divider(ui, theme);
        let _ = ui.button(format!("{}  Pivot", icon::CROSSHAIR_SIMPLE));
        let _ = ui.button(format!("{}  Global", icon::GLOBE_SIMPLE));
        let left = ui.cursor().left();
        let centre = ui.max_rect().center().x;
        ui.add_space((centre - left - 60.0).max(8.0));
        play_controls(ui, theme);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(format!("{}  Layout  {}", icon::LAYOUT, icon::CARET_DOWN), |ui| {
                let _ = ui.button("Reset layout");
            });
            let mut text = String::new();
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .hint_text(format!("{}  Search", icon::MAGNIFYING_GLASS))
                    .desired_width(180.0),
            );
        });
    });
}

/// The menu bar items for the Unity option: the categories become menus.
pub fn unity_category_menus(ui: &mut Ui) {
    for (i, tab) in TABS.iter().enumerate().take(7).skip(1) {
        ui.menu_button(tab.name, |ui| {
            for group in TABS[i].groups {
                for &(g, label) in group.buttons {
                    if ui.button(format!("{g}  {label}")).clicked() {
                        ui.close();
                    }
                }
                ui.separator();
            }
        });
    }
}

/// Category tabs over a single row of small icon buttons, grouped.
pub fn slim_ribbon(ui: &mut Ui, theme: &Theme, ribbon: &mut Ribbon) {
    ribbon.tab_row(ui, theme);
    if ribbon.collapsed {
        return;
    }
    egui::Frame::new()
        .fill(theme.panel)
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                for (gi, group) in TABS[ribbon.tab].groups.iter().enumerate() {
                    if gi > 0 {
                        divider(ui, theme);
                    }
                    for &(g, label) in group.buttons {
                        let _ = icon_button(ui, theme, g, label, false);
                    }
                }
            });
        });
}

/// A button with the icon left of its label, a toolbar row tall.
fn labelled(ui: &mut Ui, theme: &Theme, glyph: &str, label: &str, on: bool) -> egui::Response {
    let font = FontId::proportional(13.0);
    let text_w = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), theme.text)
        .size()
        .x;
    let h = theme.row + 10.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(text_w + 40.0, h), Sense::click());
    let fill = if on {
        theme.field
    } else if resp.hovered() {
        theme.control
    } else {
        Color32::TRANSPARENT
    };
    let p = ui.painter();
    p.rect_filled(rect, theme.control_radius as f32, fill);
    if on {
        p.hline(
            rect.x_range(),
            rect.bottom() - 1.0,
            egui::Stroke::new(2.0, theme.select),
        );
    }
    p.text(
        rect.left_center() + Vec2::new(16.0, 0.0),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(18.0),
        theme.text,
    );
    p.text(
        rect.left_center() + Vec2::new(30.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        font,
        theme.text,
    );
    resp
}

/// One taller row of labelled buttons: the common tools are buttons, the
/// categories are labelled dropdowns.
pub fn labelled_toolbar(ui: &mut Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let _ = labelled(ui, theme, icon::FLOPPY_DISK, "Save", false);
        divider(ui, theme);
        for (i, (g, l)) in [
            (icon::CURSOR, "Select"),
            (icon::ARROWS_OUT_CARDINAL, "Move"),
            (icon::ARROWS_CLOCKWISE, "Rotate"),
            (icon::ARROWS_OUT, "Scale"),
        ]
        .into_iter()
        .enumerate()
        {
            let _ = labelled(ui, theme, g, l, i == 1);
        }
        divider(ui, theme);
        ui.spacing_mut().button_padding = Vec2::new(10.0, 7.0);
        for (i, g) in [
            (0, icon::PLUS_CIRCLE),
            (2, icon::USER),
            (3, icon::PERSON_ARMS_SPREAD),
            (5, icon::LIGHTBULB),
            (6, icon::FILM_SLATE),
        ] {
            let name = if i == 0 { "Add" } else { TABS[i].name };
            category_menu(ui, g, name, i);
        }
        divider(ui, theme);
        let _ = labelled(ui, theme, icon::PLAY, "Simulate", false);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(format!("{}  Layout  {}", icon::LAYOUT, icon::CARET_DOWN), |ui| {
                let _ = ui.button("Reset layout");
            });
        });
    });
}

/// A small button with its label to the right, for stacked columns.
fn small(ui: &mut Ui, theme: &Theme, glyph: &str, label: &str) -> egui::Response {
    let text_w = ui
        .painter()
        .layout_no_wrap(label.to_string(), FontId::proportional(12.5), theme.text)
        .size()
        .x;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(text_w + 30.0, 20.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, theme.control_radius as f32, theme.control);
    }
    let p = ui.painter();
    p.text(
        rect.left_center() + Vec2::new(10.0, 0.0),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(14.0),
        theme.text,
    );
    p.text(
        rect.left_center() + Vec2::new(22.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.5),
        theme.text,
    );
    resp
}

/// A large button: icon above a label, slimmer than the full ribbon's.
fn large(ui: &mut Ui, theme: &Theme, glyph: &str, label: &str) -> egui::Response {
    let font = FontId::proportional(12.0);
    let w = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), theme.text)
        .size()
        .x
        .max(30.0)
        + 14.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 62.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, theme.control_radius as f32, theme.control);
    }
    let p = ui.painter();
    p.text(
        rect.center_top() + Vec2::new(0.0, 20.0),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(24.0),
        theme.text,
    );
    p.text(
        rect.center_bottom() - Vec2::new(0.0, 14.0),
        Align2::CENTER_CENTER,
        label,
        font,
        theme.text,
    );
    resp
}

/// Office and 3ds Max style: per group, the first action as a large button
/// and the rest stacked three high with their labels beside them.
pub fn pro_ribbon(ui: &mut Ui, theme: &Theme, ribbon: &mut Ribbon) {
    ribbon.tab_row(ui, theme);
    if ribbon.collapsed {
        return;
    }
    egui::Frame::new()
        .fill(theme.panel)
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                for (gi, group) in TABS[ribbon.tab].groups.iter().enumerate() {
                    if gi > 0 {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(9.0, 78.0), Sense::hover());
                        ui.painter()
                            .vline(rect.center().x, rect.y_range(), egui::Stroke::new(1.0, theme.divider));
                    }
                    ui.vertical(|ui| {
                        let row = ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let (g, l) = group.buttons[0];
                            let _ = large(ui, theme, g, l);
                            for column in group.buttons[1..].chunks(3) {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 1.0;
                                    for &(g, l) in column {
                                        let _ = small(ui, theme, g, l);
                                    }
                                });
                            }
                        });
                        let w = row.response.rect.width();
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 14.0), Sense::hover());
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            group.name,
                            FontId::proportional(11.0),
                            theme.text_dim,
                        );
                    });
                }
            });
        });
}

/// Labelled category modes on the left, as Unreal's mode selector, and the
/// active mode's tools as labelled buttons in the row below.
pub fn mode_bar(ui: &mut Ui, theme: &Theme, ribbon: &mut Ribbon) {
    let modes = [
        icon::HOUSE,
        icon::GLOBE_HEMISPHERE_WEST,
        icon::USER,
        icon::PERSON_ARMS_SPREAD,
        icon::FILM_STRIP,
        icon::LIGHTBULB,
        icon::FILM_SLATE,
        icon::EYE,
    ];
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, tab) in TABS.iter().enumerate() {
                    if labelled(ui, theme, modes[i], tab.name, i == ribbon.tab).clicked() {
                        ribbon.tab = i;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let _ = labelled(ui, theme, icon::PLAY, "Simulate", false);
                });
            });
        });
    egui::Frame::new()
        .fill(theme.panel)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                for (gi, group) in TABS[ribbon.tab].groups.iter().enumerate() {
                    if gi > 0 {
                        divider(ui, theme);
                    }
                    for &(g, l) in group.buttons {
                        let _ = small(ui, theme, g, l);
                    }
                }
            });
        });
}

/// The slim toolbar of the Unity and Unreal directions: the project and
/// save on the left, play controls in the middle, search and layout on the
/// right. With `modes`, an Unreal style mode selector and Add menu follow
/// the project.
pub fn slim(ui: &mut Ui, theme: &Theme, modes: bool) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.menu_button(
            format!("{}  Untitled project  {}", icon::CUBE, icon::CARET_DOWN),
            |ui| {
                for item in ["Project settings", "Character library", "Open project folder"] {
                    let _ = ui.button(item);
                }
            },
        );
        let _ = icon_button(ui, theme, icon::FLOPPY_DISK, "Save", false);
        if modes {
            divider(ui, theme);
            ui.menu_button(format!("{}  Select mode  {}", icon::CURSOR, icon::CARET_DOWN), |ui| {
                for (g, m) in [
                    (icon::CURSOR, "Select"),
                    (icon::PERSON_ARMS_SPREAD, "Pose"),
                    (icon::LIGHTBULB, "Light"),
                    (icon::FILM_STRIP, "Animate"),
                    (icon::FILM_SLATE, "Bake"),
                ] {
                    let _ = ui.button(format!("{g}  {m}"));
                }
            });
            category_menu(ui, icon::PLUS_CIRCLE, "Add", 0);
        }
        let left = ui.cursor().left();
        let centre = ui.max_rect().center().x;
        ui.add_space((centre - left - 50.0).max(8.0));
        play_controls(ui, theme);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(format!("{}  Layout  {}", icon::LAYOUT, icon::CARET_DOWN), |ui| {
                let _ = ui.button("Reset layout");
            });
            let mut text = String::new();
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .hint_text(format!("{}  Search commands", icon::MAGNIFYING_GLASS))
                    .desired_width(200.0),
            );
        });
    });
}

/// The modes of the editor, each with its icon.
pub const MODES: [(&str, &str); 6] = [
    (icon::CURSOR, "Select"),
    (icon::USER, "Character"),
    (icon::PERSON_ARMS_SPREAD, "Pose"),
    (icon::LIGHTBULB, "Light"),
    (icon::FILM_STRIP, "Animate"),
    (icon::FILM_SLATE, "Bake"),
];

/// The right side of the menu bar row in the combined directions: with
/// `modes`, the mode selector and Add, then play controls, command search
/// and the layout menu.
pub fn menu_bar_tools(ui: &mut Ui, theme: &Theme, modes: bool) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.menu_button(format!("{}  Layout  {}", icon::LAYOUT, icon::CARET_DOWN), |ui| {
            let _ = ui.button("Reset layout");
        });
        let mut text = String::new();
        ui.add(
            egui::TextEdit::singleline(&mut text)
                .hint_text(format!("{}  Search commands", icon::MAGNIFYING_GLASS))
                .desired_width(180.0),
        );
        divider(ui, theme);
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            if modes {
                ui.menu_button(
                    format!("{}  Pose mode  {}", icon::PERSON_ARMS_SPREAD, icon::CARET_DOWN),
                    |ui| {
                        for (g, m) in MODES {
                            let _ = ui.button(format!("{g}  {m}"));
                        }
                    },
                );
                category_menu(ui, icon::PLUS_CIRCLE, "Add", 0);
                divider(ui, theme);
            }
            play_controls(ui, theme);
        });
    });
}
