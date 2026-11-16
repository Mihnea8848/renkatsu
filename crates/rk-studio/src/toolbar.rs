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
fn category_menu(ui: &mut Ui, glyph: &str, name: &str, tab: usize) {
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
fn play_controls(ui: &mut Ui, theme: &Theme) {
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
