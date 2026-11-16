// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The editor's own controls. They are painted by hand on whole pixels so
//! that every design keeps exact sizes, aligned baselines and the same
//! hover and selected states everywhere.

use crate::state::Look;
use egui::{
    Align2, Color32, CursorIcon, FontId, Id, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use egui_phosphor::regular as icon;

/// An icon centred on a point.
pub fn glyph(p: &Painter, k: &Look, center: Pos2, glyph: &str, size: f32, color: Color32) {
    p.text(center, Align2::CENTER_CENTER, glyph, k.icons(size), color);
}

/// Body text starting at a point, centred on it vertically. Returns the
/// right edge of the text.
pub fn label(p: &Painter, k: &Look, pos: Pos2, text: &str, color: Color32) -> f32 {
    p.text(pos, Align2::LEFT_CENTER, text, k.font(), color).right()
}

pub fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
        .size()
        .x
}

/// A horizontal line one pixel thick.
pub fn hair_h(p: &Painter, left: f32, right: f32, y: f32, color: Color32) {
    p.rect_filled(
        Rect::from_min_max(Pos2::new(left, y), Pos2::new(right, y + 1.0)),
        0.0,
        color,
    );
}

/// A vertical line one pixel thick.
pub fn hair_v(p: &Painter, x: f32, top: f32, bottom: f32, color: Color32) {
    p.rect_filled(
        Rect::from_min_max(Pos2::new(x, top), Pos2::new(x + 1.0, bottom)),
        0.0,
        color,
    );
}

/// A filled box with an outline inside its edge.
pub fn boxed(p: &Painter, rect: Rect, radius: f32, fill: Color32, outline: Color32) {
    p.rect_filled(rect, radius, fill);
    p.rect_stroke(rect, radius, Stroke::new(1.0, outline), StrokeKind::Inside);
}

/// The recessed box of a field, lit when hovered or in use.
pub fn field_box(p: &Painter, k: &Look, rect: Rect, hovered: bool, active: bool) {
    let outline = if active {
        k.t.select
    } else if hovered {
        k.t.hover
    } else {
        k.t.border
    };
    boxed(p, rect, k.radius(), k.t.input, outline);
}

/// Width of a `flat_button` with these parts.
pub fn flat_width(ui: &Ui, k: &Look, icon: Option<&str>, text: &str, caret: bool) -> f32 {
    let mut w = 16.0;
    if icon.is_some() {
        w += 16.0;
    }
    if !text.is_empty() {
        if icon.is_some() {
            w += 6.0;
        }
        w += text_width(ui, text, k.font());
    }
    if caret {
        w += if text.is_empty() { 10.0 } else { 14.0 };
    }
    w.ceil()
}

/// A button without a box until hovered: an icon, a name and optionally
/// the caret of a menu. `on` marks a toggle that is switched on.
pub fn flat_button(ui: &mut Ui, k: &Look, icon_glyph: Option<&str>, text: &str, caret: bool, on: bool) -> Response {
    let size = Vec2::new(flat_width(ui, k, icon_glyph, text, caret), k.field());
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    if on {
        boxed(p, rect, k.radius(), k.t.input, k.t.border);
    } else if resp.hovered() {
        p.rect_filled(rect, k.radius(), k.t.control);
    }
    let color = if on || resp.hovered() {
        k.t.text_bright
    } else {
        k.t.text
    };
    let mut x = rect.left() + 8.0;
    let y = rect.center().y;
    if let Some(g) = icon_glyph {
        let tint = if on { k.t.select_bright } else { color };
        glyph(p, k, Pos2::new(x + 8.0, y), g, 16.0, tint);
        x += 16.0 + 6.0;
    }
    if !text.is_empty() {
        x = label(p, k, Pos2::new(x, y), text, color);
    }
    if caret {
        let gap = if text.is_empty() { 3.0 } else { 9.0 };
        glyph(p, k, Pos2::new(x + gap, y + 1.0), icon::CARET_DOWN, 10.0, k.t.text_dim);
    }
    resp
}

/// A square button holding one icon.
pub fn icon_button(ui: &mut Ui, k: &Look, icon_glyph: &str, on: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(k.field()), Sense::click());
    icon_cell(ui, k, rect, &resp, icon_glyph, on);
    resp
}

/// Paints an icon button into a rectangle that was already claimed.
pub fn icon_cell(ui: &Ui, k: &Look, rect: Rect, resp: &Response, icon_glyph: &str, on: bool) {
    let p = ui.painter();
    if on {
        boxed(p, rect, k.radius(), k.t.input, k.t.border);
    } else if resp.hovered() {
        p.rect_filled(rect, k.radius(), k.t.control);
    }
    let color = if on {
        k.t.select_bright
    } else if resp.hovered() {
        k.t.text_bright
    } else {
        k.t.text
    };
    glyph(p, k, rect.center(), icon_glyph, 16.0, color);
}

/// A short vertical divider between groups in a bar.
pub fn bar_divider(ui: &mut Ui, k: &Look) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(9.0, k.field()), Sense::hover());
    hair_v(
        ui.painter(),
        rect.center().x.floor(),
        rect.top() + 3.0,
        rect.bottom() - 3.0,
        k.t.border,
    );
}

/// A search field with its magnifier, `width` wide.
pub fn search(ui: &mut Ui, k: &Look, text: &mut String, hint: &str, width: f32) -> Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, k.field()), Sense::hover());
    field_box(ui.painter(), k, rect, false, false);
    glyph(
        ui.painter(),
        k,
        Pos2::new(rect.left() + 13.0, rect.center().y),
        icon::MAGNIFYING_GLASS,
        13.0,
        k.t.text_dim,
    );
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + 24.0, rect.top()),
        Pos2::new(rect.right() - 6.0, rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let resp = child.add(
        egui::TextEdit::singleline(text)
            .hint_text(egui::RichText::new(hint).color(k.t.text_dim))
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .desired_width(inner.width()),
    );
    if resp.has_focus() || resp.hovered() {
        let color = if resp.has_focus() { k.t.select } else { k.t.hover };
        ui.painter()
            .rect_stroke(rect, k.radius(), Stroke::new(1.0, color), StrokeKind::Inside);
    }
    resp
}

/// What marks the axis of a number field.
#[derive(Clone, Copy)]
pub enum Marker {
    None,
    /// A coloured bar along the field's left edge, as in Unreal.
    Bar(Color32),
    /// A coloured letter inside the field.
    Letter(Color32, &'static str),
}

/// A number that is changed by dragging it sideways, by `speed` for each
/// point of the drag.
#[allow(clippy::too_many_arguments)]
pub fn number(ui: &mut Ui, k: &Look, rect: Rect, id: Id, value: &mut f32, marker: Marker, framed: bool, speed: f32) {
    let resp = ui
        .interact(rect, id, Sense::click_and_drag())
        .on_hover_cursor(CursorIcon::ResizeHorizontal);
    if resp.dragged() {
        *value += resp.drag_delta().x * speed;
    }
    let p = ui.painter();
    if framed {
        field_box(p, k, rect, resp.hovered(), resp.dragged());
    } else if resp.hovered() || resp.dragged() {
        p.rect_filled(rect.shrink(1.0), 0.0, k.t.well);
    }
    let mut x = rect.left() + 7.0;
    match marker {
        Marker::None => {}
        Marker::Bar(color) => {
            let bar = Rect::from_min_max(
                Pos2::new(rect.left() + 1.0, rect.top() + 1.0),
                Pos2::new(rect.left() + 4.0, rect.bottom() - 1.0),
            );
            let r = (k.radius() - 1.0).max(0.0) as u8;
            p.rect_filled(
                bar,
                egui::CornerRadius {
                    nw: r,
                    sw: r,
                    ne: 0,
                    se: 0,
                },
                color,
            );
            x += 4.0;
        }
        Marker::Letter(color, letter) => {
            p.text(
                Pos2::new(x + 4.0, rect.center().y),
                Align2::CENTER_CENTER,
                letter,
                k.bold(k.d.small),
                color,
            );
            x += 14.0;
        }
    }
    let color = if resp.hovered() { k.t.text_bright } else { k.t.text };
    p.with_clip_rect(rect.shrink(1.0)).text(
        Pos2::new(x, rect.center().y),
        Align2::LEFT_CENTER,
        format!("{value:.2}"),
        k.font(),
        color,
    );
}

/// A read-only text value in a field's box.
pub fn value_box(ui: &mut Ui, k: &Look, rect: Rect, text: &str) {
    let p = ui.painter();
    field_box(p, k, rect, false, false);
    p.with_clip_rect(rect.shrink(1.0)).text(
        Pos2::new(rect.left() + 7.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        k.font(),
        k.t.text,
    );
}

/// A dropdown: the chosen option and a caret, opening a list.
pub fn combo(ui: &mut Ui, k: &Look, rect: Rect, id: Id, chosen: &mut usize, options: &[&str]) {
    let resp = ui.interact(rect, id, Sense::click());
    let p = ui.painter();
    let fill = if resp.hovered() { k.t.hover } else { k.t.control };
    p.rect_filled(rect, k.radius(), fill);
    let color = if resp.hovered() { k.t.text_bright } else { k.t.text };
    p.with_clip_rect(rect.with_max_x(rect.right() - 22.0)).text(
        Pos2::new(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        options.get(*chosen).copied().unwrap_or(""),
        k.font(),
        color,
    );
    glyph(
        p,
        k,
        Pos2::new(rect.right() - 12.0, rect.center().y + 1.0),
        icon::CARET_DOWN,
        10.0,
        color,
    );
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(rect.width() - 10.0);
        for (i, option) in options.iter().enumerate() {
            if check_item(ui, k, i == *chosen, option, "").clicked() {
                *chosen = i;
                ui.close();
            }
        }
    });
}

/// A check box with its name to the right.
pub fn checkbox(ui: &mut Ui, k: &Look, rect: Rect, id: Id, on: &mut bool, text: &str) {
    let resp = ui.interact(rect, id, Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    let p = ui.painter();
    let side = 14.0;
    let mark = Rect::from_min_size(
        Pos2::new(rect.left(), (rect.center().y - side / 2.0).round()),
        Vec2::splat(side),
    );
    if *on {
        p.rect_filled(mark, 2.0, k.t.select);
        glyph(p, k, mark.center(), icon::CHECK, 11.0, Color32::WHITE);
    } else {
        let outline = if resp.hovered() { k.t.text_dim } else { k.t.hover };
        boxed(p, mark, 2.0, k.t.input, outline);
    }
    if !text.is_empty() {
        let color = if resp.hovered() { k.t.text_bright } else { k.t.text };
        label(p, k, Pos2::new(mark.right() + 7.0, rect.center().y), text, color);
    }
}

/// A choice between a few options shown side by side in one box.
pub fn segmented(ui: &mut Ui, k: &Look, rect: Rect, id: Id, chosen: &mut usize, options: &[&str]) {
    boxed(ui.painter(), rect, k.radius(), k.t.input, k.t.border);
    let w = (rect.width() - 2.0) / options.len() as f32;
    for (i, option) in options.iter().enumerate() {
        let cell = Rect::from_min_size(
            Pos2::new((rect.left() + 1.0 + i as f32 * w).round(), rect.top() + 1.0),
            Vec2::new(w.round(), rect.height() - 2.0),
        );
        let resp = ui.interact(cell, id.with(i), Sense::click());
        if resp.clicked() {
            *chosen = i;
        }
        let p = ui.painter();
        let on = *chosen == i;
        if on {
            p.rect_filled(cell.shrink(1.0), (k.radius() - 1.0).max(0.0), k.t.control);
        }
        let color = if on {
            k.t.text_bright
        } else if resp.hovered() {
            k.t.text
        } else {
            k.t.text_dim
        };
        p.text(cell.center(), Align2::CENTER_CENTER, *option, k.font(), color);
    }
}

/// A full-width row with an icon and a name that lights when hovered and
/// holds a soft selection when `on`.
pub fn tool_row(ui: &mut Ui, k: &Look, rect: Rect, id: Id, icon_glyph: &str, text: &str, on: bool) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, k.radius(), k.t.select_soft);
    } else if resp.hovered() {
        p.rect_filled(rect, k.radius(), k.t.control);
    }
    let color = if on || resp.hovered() {
        k.t.text_bright
    } else {
        k.t.text
    };
    let tint = if on { k.t.select_bright } else { color };
    glyph(
        p,
        k,
        Pos2::new(rect.left() + 16.0, rect.center().y),
        icon_glyph,
        16.0,
        tint,
    );
    label(p, k, Pos2::new(rect.left() + 32.0, rect.center().y), text, color);
    resp
}

/// A menu item that carries a check mark while it is switched on, and
/// optionally the keys that do the same.
pub fn check_item(ui: &mut Ui, k: &Look, on: bool, text: &str, keys: &str) -> Response {
    let mut job = egui::text::LayoutJob::default();
    let format = |font, color| egui::TextFormat {
        font_id: font,
        color,
        valign: egui::Align::Center,
        ..Default::default()
    };
    let mark = if on { k.t.text_bright } else { Color32::TRANSPARENT };
    job.append(icon::CHECK, 0.0, format(k.icons(12.0), mark));
    job.append(text, 7.0, format(k.font(), Color32::PLACEHOLDER));
    let mut button = egui::Button::new(job);
    if !keys.is_empty() {
        button = button.shortcut_text(keys);
    }
    ui.add(button)
}
