// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The parts of the window around the docked panels: the menu row with the
//! project, the physics play controls and the command search; the status
//! bar; and, in the design that keeps the viewport clean, the mode rail at
//! the window's left edge.

use crate::design::Overlays;
use crate::kinds::Kind;
use crate::modes::{tool_hint, MODES, TOOLS};
use crate::panels::{self, Panel};
use crate::state::{Look, Physics, Rail, State};
use crate::viewport::{self, RailShape};
use crate::widgets::{self, flat_button, glyph, hair_h, hair_v, label};
use egui::{Align, Align2, Color32, Layout, Pos2, Rect, Sense, Stroke, Ui, UiBuilder, Vec2};
use egui_dock::DockState;
use egui_phosphor::regular as icon;

/// The menu row. Sets `reset` when the layout should return to default.
pub fn menu_row(ui: &mut Ui, k: &Look, st: &mut State, dock: &mut DockState<Panel>, reset: &mut bool) {
    let t = &k.t;
    let rect = ui.max_rect();
    hair_h(ui.painter(), rect.left(), rect.right(), rect.bottom() - 1.0, t.divider);

    // The mark, then the menus and the project. In the design with the
    // rail in the window, the mark sits over the rail's icons.
    let mark_x = if k.d.overlays == Overlays::Header {
        (window_rail_width(k, st) / 2.0).floor()
    } else {
        21.0
    };
    let mark = Rect::from_center_size(Pos2::new(rect.left() + mark_x, rect.center().y), Vec2::splat(18.0));
    ui.painter().rect_filled(mark, 2.0, t.accent);
    ui.painter().text(
        mark.center() + Vec2::new(0.5, 0.0),
        Align2::CENTER_CENTER,
        "R",
        k.bold(13.0),
        t.input,
    );
    let limit = rect.center().x - 90.0;
    let mut left = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(mark.right() + mark_x - 9.0 - 4.0, rect.top() - 2.0),
                Pos2::new(limit, rect.bottom() - 3.0),
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    left.spacing_mut().item_spacing.x = 2.0;
    left.spacing_mut().button_padding = Vec2::new(8.0, 4.0);
    let mut end = left.cursor().left();
    egui::MenuBar::new().ui(&mut left, |ui| {
        menus(ui, k, st, dock, reset);
        end = ui.cursor().left();
    });
    let scene = "Untitled scene";
    let project = "Untitled project";
    let need = widgets::text_width(ui, scene, k.font()) + widgets::text_width(ui, project, k.font()) + 60.0;
    if end + need < limit {
        let y = rect.center().y;
        let p = ui.painter();
        hair_v(p, end + 8.0, y - 8.0, y + 8.0, t.border);
        let x = label(p, k, Pos2::new(end + 22.0, y), project, t.text_dim);
        glyph(p, k, Pos2::new(x + 9.0, y + 0.5), icon::CARET_RIGHT, 9.0, t.text_dim);
        let x = label(p, k, Pos2::new(x + 18.0, y), scene, t.text);
        // Unsaved changes: one of the few places for the orange.
        p.circle_filled(Pos2::new(x + 9.0, y + 0.5), 3.0, t.accent);
    }

    play_group(ui, k, st, rect.center());

    let mut right = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(rect.center().x + 80.0, rect.top()),
                Pos2::new(rect.right() - 8.0, rect.bottom() - 1.0),
            ))
            .layout(Layout::right_to_left(Align::Center)),
    );
    right.spacing_mut().item_spacing.x = 6.0;
    let resp = flat_button(&mut right, k, Some(icon::LAYOUT), "Layout", true, false);
    egui::Popup::menu(&resp).show(|ui| {
        if ui.button("Reset layout").clicked() {
            *reset = true;
            ui.close();
        }
        let _ = ui.button("Save layout");
    });
    let w = (right.available_width() - 8.0).clamp(120.0, 260.0);
    widgets::search(&mut right, k, &mut st.command_search, "Search commands", w);
}

fn menus(ui: &mut Ui, k: &Look, st: &mut State, dock: &mut DockState<Panel>, reset: &mut bool) {
    // The menu bar resets the spacing; give the titles room again.
    ui.spacing_mut().button_padding = Vec2::new(7.0, 4.0);
    ui.spacing_mut().item_spacing.x = 0.0;
    let item = |ui: &mut Ui, text: &str, keys: &str| ui.add(egui::Button::new(text).shortcut_text(keys));
    ui.menu_button("File", |ui| {
        let _ = item(ui, "New project", "Ctrl+N");
        let _ = item(ui, "Open project", "Ctrl+O");
        ui.separator();
        let _ = item(ui, "Save", "Ctrl+S");
        let _ = item(ui, "Save as", "Ctrl+Shift+S");
        ui.separator();
        let _ = item(ui, "Project settings", "");
        let _ = item(ui, "Quit", "Ctrl+Q");
    });
    ui.menu_button("Edit", |ui| {
        let _ = item(ui, "Undo", "Ctrl+Z");
        let _ = item(ui, "Redo", "Ctrl+Y");
        ui.separator();
        let _ = item(ui, "Duplicate", "Ctrl+D");
        let _ = item(ui, "Delete", "Del");
        ui.separator();
        let _ = item(ui, "Preferences", "");
    });
    ui.menu_button("Add", |ui| {
        for kind in [
            Kind::Character,
            Kind::Item,
            Kind::Light,
            Kind::Camera,
            Kind::Map,
            Kind::Folder,
        ] {
            let mut job = egui::text::LayoutJob::default();
            let format = |font, color| egui::TextFormat {
                font_id: font,
                color,
                valign: Align::Center,
                ..Default::default()
            };
            job.append(kind.glyph(), 0.0, format(k.icons(15.0), kind.color()));
            job.append(kind.label(), 8.0, format(k.font(), k.t.text));
            let _ = ui.button(job);
        }
    });
    ui.menu_button("View", |ui| {
        if widgets::check_item(ui, k, !st.overlays_hidden, "Viewport tools", "`").clicked() {
            st.overlays_hidden = !st.overlays_hidden;
        }
        if widgets::check_item(ui, k, st.maximised, "Maximise viewport", "Shift+Space").clicked() {
            st.maximised = !st.maximised;
        }
        ui.separator();
        if widgets::check_item(ui, k, st.palette_open, "Mode tools", "").clicked() {
            st.palette_open = !st.palette_open;
        }
        if widgets::check_item(ui, k, st.rail == Rail::Named, "Mode names", "").clicked() {
            st.rail = if st.rail == Rail::Named {
                Rail::Icons
            } else {
                Rail::Named
            };
        }
        ui.separator();
        for (i, mode) in MODES.iter().enumerate() {
            let text = format!("{} mode", mode.name);
            if widgets::check_item(ui, k, st.mode == i, &text, &(i + 1).to_string()).clicked() {
                st.mode = i;
                st.palette_open = true;
            }
        }
    });
    ui.menu_button("Window", |ui| {
        for panel in Panel::ALL {
            if ui.button(panel.title()).clicked() {
                panels::open(dock, panel);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Reset layout").clicked() {
            *reset = true;
            ui.close();
        }
    });
    ui.menu_button("Help", |ui| {
        let _ = ui.button("Keyboard map");
        let _ = ui.button("About Renkatsu");
    });
}

/// Unreal 5's play controls, centred in the menu row: flat icon buttons
/// with no box around them, a soft background only on hover or while
/// held, thin dividers either side of the group, and the simulation's
/// options behind a vertical three-dot button. Play is green; pause and
/// stop stay grey while nothing runs, and stop turns red once it does.
fn play_group(ui: &mut Ui, k: &Look, st: &mut State, center: Pos2) {
    let t = &k.t;
    let cell = Vec2::new(30.0, k.d.row + 2.0);
    let dots_w = 20.0;
    let gap = 2.0;
    let size = Vec2::new(cell.x * 3.0 + gap * 3.0 + dots_w, cell.y);
    let group = Rect::from_min_size((center - size / 2.0).round(), size);
    let running = st.physics != Physics::Stopped;
    {
        let p = ui.painter();
        for x in [group.left() - 8.0, group.right() + 8.0] {
            hair_v(p, x, group.top() + 4.0, group.bottom() - 4.0, t.border);
        }
    }
    let tips = [
        "Play the physics",
        "Pause the physics",
        "Stop and reset the physics",
        "Simulation options",
    ];
    let mut x = group.left();
    for (i, tip) in tips.into_iter().enumerate() {
        let w = if i == 3 { dots_w } else { cell.x };
        let rect = Rect::from_min_size(Pos2::new(x, group.top()), Vec2::new(w, cell.y));
        x += w + gap;
        let resp = ui
            .interact(rect, ui.id().with(("play", i)), Sense::click())
            .on_hover_text(tip);
        if resp.clicked() {
            st.physics = match (i, st.physics) {
                (0, _) => Physics::Playing,
                (1, Physics::Playing) => Physics::Paused,
                (1, Physics::Paused) => Physics::Playing,
                (2, _) => Physics::Stopped,
                (_, now) => now,
            };
        }
        let p = ui.painter();
        let held = matches!((i, st.physics), (0, Physics::Playing) | (1, Physics::Paused));
        if held {
            p.rect_filled(rect, k.radius(), t.control);
        } else if resp.hovered() {
            p.rect_filled(rect, k.radius(), t.hover);
        }
        let c = rect.center().round();
        let idle = t.text_dim;
        match i {
            0 => {
                p.add(egui::Shape::convex_polygon(
                    vec![
                        c + Vec2::new(-5.0, -7.0),
                        c + Vec2::new(7.0, 0.0),
                        c + Vec2::new(-5.0, 7.0),
                    ],
                    t.play,
                    Stroke::NONE,
                ));
            }
            1 => {
                let color = if running { t.text_bright } else { idle };
                for dx in [-5.0, 1.5] {
                    p.rect_filled(
                        Rect::from_min_size(c + Vec2::new(dx, -6.0), Vec2::new(3.5, 12.0)),
                        0.5,
                        color,
                    );
                }
            }
            2 => {
                let color = if running { t.stop } else { idle };
                p.rect_filled(Rect::from_center_size(c, Vec2::splat(11.0)), 1.5, color);
            }
            _ => {
                let color = if resp.hovered() { t.text_bright } else { idle };
                for dy in [-5.0, 0.0, 5.0] {
                    p.circle_filled(c + Vec2::new(0.0, dy), 1.6, color);
                }
                egui::Popup::menu(&resp).show(|ui| {
                    let _ = ui.button("Simulate hair");
                    let _ = ui.button("Simulate clothes");
                    let _ = ui.button("Reset when stopped");
                });
            }
        }
    }
}

pub fn status_bar(ui: &mut Ui, k: &Look, st: &State) {
    let t = &k.t;
    let rect = ui.max_rect();
    let p = ui.painter();
    hair_h(p, rect.left(), rect.right(), rect.top(), t.divider);
    let y = rect.center().y + 0.5;
    let small =
        |x: f32, align: Align2, text: &str, color: Color32| p.text(Pos2::new(x, y), align, text, k.small(), color);

    let (tool_glyph, tool, _) = TOOLS[st.tool];
    glyph(p, k, Pos2::new(rect.left() + 17.0, y), tool_glyph, 13.0, t.text);
    let x = small(rect.left() + 29.0, Align2::LEFT_CENTER, tool, t.text).right();
    let x = small(x + 10.0, Align2::LEFT_CENTER, tool_hint(st.tool), t.text_dim).right();
    if st.physics != Physics::Stopped {
        let text = if st.physics == Physics::Playing {
            "Physics running"
        } else {
            "Physics paused"
        };
        p.circle_filled(Pos2::new(x + 22.0, y), 3.0, t.accent);
        small(x + 31.0, Align2::LEFT_CENTER, text, t.text);
    }

    let mut x = rect.right() - 12.0;
    for text in [
        format!("Design {}", k.d.name),
        "Koikatsu look".to_owned(),
        match &st.live {
            Some(live) => format!("Live: {}", live.name),
            None => "Still picture".to_owned(),
        },
    ] {
        x = small(x, Align2::RIGHT_CENTER, &text, t.text_dim).left() - 12.0;
        hair_v(p, x, rect.top() + 6.0, rect.bottom() - 5.0, t.border);
        x -= 12.0;
    }
}

/// The mode rail as part of the window, down its left edge.
pub fn window_rail(ui: &mut Ui, k: &Look, st: &mut State) {
    let rect = ui.max_rect();
    hair_v(ui.painter(), rect.right() - 1.0, rect.top(), rect.bottom(), k.t.divider);
    let inner = Rect::from_min_max(rect.min, Pos2::new(rect.right() - 1.0, rect.bottom()));
    viewport::rail(ui, k, st, inner, RailShape::Stacked);
}

/// Width of the window rail, with its dividing line.
pub fn window_rail_width(k: &Look, st: &State) -> f32 {
    viewport::rail_width(k, st, RailShape::Stacked) + 1.0
}
