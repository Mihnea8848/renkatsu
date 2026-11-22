// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Keyboard shortcuts window: every action with its keys, by group.
//! A click on a key waits for new keys; an action that shares a key with
//! another says so.

use crate::keycaps::{self, Cap, Mouse};
use crate::keymap::{Action, Group, Keymap, ACTIONS, SLOTS};
use crate::state::{Look, State};
use crate::widgets::{self, field_box, flat_button, glyph, label, text_width};
use egui::{Align2, Pos2, Rect, Sense, Vec2};
use egui_phosphor::regular as icon;

const WIDTH: f32 = 640.0;
const HEIGHT: f32 = 600.0;

/// What the mouse does in the viewport. These are not remapped.
fn mouse_rows() -> Vec<(&'static str, Vec<Vec<Cap>>)> {
    use Cap::{Drag, Mouse as M};
    let key = |t: &str| Cap::Key(t.into());
    vec![
        ("Select", vec![vec![M(Mouse::Left)]]),
        ("Add or remove from selection", vec![vec![key("Ctrl"), M(Mouse::Left)]]),
        ("Box select", vec![vec![M(Mouse::Left), Drag]]),
        (
            "Orbit",
            vec![vec![M(Mouse::Right), Drag], vec![key("Alt"), M(Mouse::Left), Drag]],
        ),
        ("Pan", vec![vec![M(Mouse::Middle), Drag]]),
        (
            "Zoom",
            vec![vec![M(Mouse::Wheel)], vec![key("Alt"), M(Mouse::Right), Drag]],
        ),
        (
            "Fly faster",
            vec![vec![M(Mouse::Right), Cap::Symbol(icon::ARROW_FAT_UP, "Shift")]],
        ),
        ("Fly speed", vec![vec![M(Mouse::Right), M(Mouse::Wheel)]]),
        ("Field of view", vec![vec![key("Alt"), M(Mouse::Wheel)]]),
    ]
}

/// What a frame of the window asks for.
enum Ask {
    Capture(Action, usize),
    Clear(Action, usize),
    Reset(Action),
    ResetAll,
}

pub fn show(ctx: &egui::Context, k: &Look, st: &mut State) {
    if !st.shortcuts_open {
        st.key_capture = None;
        return;
    }
    let mut open = true;
    let mut ask = None;
    // Escape while a key is being taken cancels that, not the window.
    let capturing = st.key_capture.is_some();
    widgets::window(ctx, k, "Keyboard shortcuts", WIDTH, &mut open, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let reset = widgets::flat_width(ui, k, Some(icon::ARROW_COUNTER_CLOCKWISE), "Reset all", false);
            widgets::search(ui, k, &mut st.shortcuts_search, "Search", WIDTH - reset - 6.0);
            let changed = st.keys != Keymap::default();
            let button = ui.add_enabled_ui(changed, |ui| {
                flat_button(ui, k, Some(icon::ARROW_COUNTER_CLOCKWISE), "Reset all", false, false)
            });
            if button.inner.clicked() {
                ask = Some(Ask::ResetAll);
            }
        });
        ui.add_space(4.0);
        let search = st.shortcuts_search.trim().to_lowercase();
        // As tall as the window allows, up to `HEIGHT`.
        let height = HEIGHT.min(ui.ctx().content_rect().height() - 200.0).max(200.0);
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .min_scrolled_height(height)
            .max_height(height)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for group in Group::ALL {
                    let shown: Vec<_> = ACTIONS
                        .iter()
                        .filter(|def| def.group == group)
                        .filter(|def| {
                            search.is_empty()
                                || def.name.to_lowercase().contains(&search)
                                || group.name().to_lowercase().contains(&search)
                                || st
                                    .keys
                                    .chords(def.action)
                                    .iter()
                                    .any(|c| c.text().to_lowercase().contains(&search))
                        })
                        .collect();
                    if shown.is_empty() {
                        continue;
                    }
                    group_head(ui, k, group.name());
                    for def in shown {
                        if let Some(asked) = row(ui, k, st, def.action, def.name) {
                            ask = Some(asked);
                        }
                    }
                }
                let mice: Vec<_> = mouse_rows()
                    .into_iter()
                    .filter(|(name, _)| {
                        search.is_empty() || name.to_lowercase().contains(&search) || "mouse".contains(&search)
                    })
                    .collect();
                if !mice.is_empty() {
                    group_head(ui, k, "Mouse");
                    for (name, ways) in mice {
                        mouse_row(ui, k, name, &ways);
                    }
                }
            });
    });
    // A click anywhere else gives the waiting up.
    if ask.is_none() && st.key_capture.is_some() && ctx.input(|i| i.pointer.any_click()) {
        st.key_capture = None;
    }
    match ask {
        Some(Ask::Capture(action, slot)) => st.key_capture = Some((action, slot)),
        Some(Ask::Clear(action, slot)) => {
            st.keys.clear(action, slot);
            st.key_capture = None;
        }
        Some(Ask::Reset(action)) => st.keys.reset(action),
        Some(Ask::ResetAll) => st.keys = Keymap::default(),
        None => {}
    }
    if matches!(ask, Some(Ask::Clear(..) | Ask::Reset(_) | Ask::ResetAll)) {
        st.settings.keys = st.keys.changes();
        st.settings_changed = true;
    }
    if capturing && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = true;
    }
    if !open {
        st.shortcuts_open = false;
        st.key_capture = None;
    }
}

/// The height of a row.
fn row_height(k: &Look) -> f32 {
    k.d.row + 8.0
}

/// The name of a group above its rows.
fn group_head(ui: &mut egui::Ui, k: &Look, name: &str) {
    let (head, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
    ui.painter().text(
        Pos2::new(head.left() + 6.0, head.center().y + 3.0),
        Align2::LEFT_CENTER,
        name,
        k.small(),
        k.t.text_dim,
    );
}

/// A mouse control: its name and each way of doing it.
fn mouse_row(ui: &mut egui::Ui, k: &Look, name: &str, ways: &[Vec<Cap>]) {
    let t = &k.t;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_height(k)), Sense::hover());
    if ui.rect_contains_pointer(rect) {
        ui.painter().rect_filled(rect, k.radius(), t.row_hover);
    }
    let y = rect.center().y;
    label(ui.painter(), k, Pos2::new(rect.left() + 14.0, y), name, t.text);
    // Lined up with the keys above, which leave room for the reset button.
    let mut right = rect.right() - 6.0 - k.field() - 4.0 - 6.0;
    for (i, caps) in ways.iter().enumerate().rev() {
        let drawn = keycaps::draw(ui, k, caps, right, y, t.text_bright);
        right = drawn.left() - 6.0;
        if i > 0 {
            ui.painter().text(
                Pos2::new(right - 4.0, y),
                Align2::CENTER_CENTER,
                "or",
                k.small(),
                t.text_dim,
            );
            right -= 8.0 + text_width(ui, "or", k.small());
        }
    }
}

/// One action: its name, a warning when it shares a key, its keys, and at
/// the right end the button that puts its default keys back.
fn row(ui: &mut egui::Ui, k: &Look, st: &State, action: Action, name: &str) -> Option<Ask> {
    let t = &k.t;
    let mut ask = None;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_height(k)), Sense::hover());
    if ui.rect_contains_pointer(rect) {
        ui.painter().rect_filled(rect, k.radius(), t.row_hover);
    }
    let y = rect.center().y;
    let x = label(ui.painter(), k, Pos2::new(rect.left() + 14.0, y), name, t.text);
    let clashes = st.keys.clashes(action);
    if !clashes.is_empty() {
        glyph(ui.painter(), k, Pos2::new(x + 16.0, y), icon::WARNING, 13.0, t.accent);
        let text = format!("Also {}", clashes.join(", "));
        ui.painter()
            .text(Pos2::new(x + 26.0, y), Align2::LEFT_CENTER, text, k.small(), t.accent);
    }

    // From the right: the reset or remove button, then the keys.
    let side = k.field();
    let cell =
        |right: f32, w: f32| Rect::from_min_size(Pos2::new(right - w, (y - side / 2.0).round()), Vec2::new(w, side));
    let mut right = rect.right() - 6.0;
    let end = cell(right, side);
    right = end.left() - 4.0;
    let waiting = st.key_capture.filter(|(a, _)| *a == action).map(|(_, slot)| slot);
    let chords = st.keys.chords(action);
    if let Some(slot) = waiting.filter(|slot| *slot < chords.len()) {
        let resp = ui
            .interact(end, ui.id().with(("remove", name)), Sense::click())
            .on_hover_text("Remove");
        widgets::icon_cell(ui, k, end, &resp, icon::X, false);
        if resp.clicked() {
            ask = Some(Ask::Clear(action, slot));
        }
    } else if !st.keys.is_default(action) {
        let resp = ui
            .interact(end, ui.id().with(("reset", name)), Sense::click())
            .on_hover_text("Reset");
        widgets::icon_cell(ui, k, end, &resp, icon::ARROW_COUNTER_CLOCKWISE, false);
        if resp.clicked() {
            ask = Some(Ask::Reset(action));
        }
    }
    // One place more than the action has keys, to add one.
    let places = (chords.len() + 1).min(SLOTS);
    for slot in (0..places).rev() {
        let listening = waiting == Some(slot);
        let caps = chords.get(slot).map(|c| keycaps::of_chord(*c));
        let w = match (listening, &caps) {
            (true, _) => (text_width(ui, "Press keys", k.font()) + 16.0).ceil(),
            (false, Some(caps)) => keycaps::width(ui, k, caps) + 8.0,
            (false, None) => side,
        };
        let chip = cell(right, w);
        right = chip.left() - 6.0;
        let resp = ui.interact(chip, ui.id().with(("key", name, slot)), Sense::click());
        if resp.clicked() {
            ask = Some(Ask::Capture(action, slot));
        }
        let p = ui.painter();
        match (listening, caps) {
            (true, _) => {
                field_box(p, k, chip, resp.hovered(), true);
                p.text(chip.center(), Align2::CENTER_CENTER, "Press keys", k.font(), t.text_dim);
            }
            (false, Some(caps)) => {
                if resp.hovered() {
                    p.rect_filled(chip.expand(2.0), k.radius(), t.control);
                }
                keycaps::draw(ui, k, &caps, chip.right() - 4.0, chip.center().y, t.text_bright);
            }
            (false, None) => {
                // The place for another key.
                let color = if resp.hovered() { t.text_bright } else { t.text_dim };
                if resp.hovered() {
                    p.rect_filled(chip, k.radius(), t.control);
                }
                glyph(p, k, chip.center(), icon::PLUS, 12.0, color);
            }
        }
    }
    ask
}
