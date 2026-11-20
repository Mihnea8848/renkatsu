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

/// A small window drawn like the editor's other windows: a header with the
/// title at the left and a close cell at the right, a line under it, and
/// `add` drawing the body, `width` wide. The close cell, Escape or a click
/// on nothing else set `open` to false. Returns what `add` returns.
pub fn window<R>(
    ctx: &egui::Context,
    k: &Look,
    title: &str,
    width: f32,
    open: &mut bool,
    add: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    const HEAD: f32 = 46.0;
    const SIDE: i8 = 18;
    let t = &k.t;
    let frame = egui::Frame::new()
        .fill(t.panel)
        .stroke(Stroke::new(1.0, t.border))
        .inner_margin(egui::Margin::ZERO);
    let mut close = false;
    let shown = egui::Window::new(title)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            let full = width + 2.0 * f32::from(SIDE);
            ui.set_width(full);
            ui.spacing_mut().item_spacing.y = 0.0;
            let (head, _) = ui.allocate_exact_size(Vec2::new(full, HEAD), Sense::hover());
            let p = ui.painter();
            p.text(
                Pos2::new(head.left() + f32::from(SIDE), head.center().y),
                Align2::LEFT_CENTER,
                title,
                k.bold(15.0),
                t.text_bright,
            );
            hair_h(p, head.left(), head.right(), head.bottom() - 1.0, t.line);
            let cell = Rect::from_center_size(Pos2::new(head.right() - 24.0, head.center().y), Vec2::splat(28.0));
            let x = ui.interact(cell, ui.id().with((title, "close")), Sense::click());
            if x.hovered() {
                ui.painter().rect_filled(cell, k.radius(), t.control);
            }
            let tint = if x.hovered() { t.text_bright } else { t.text_dim };
            glyph(ui.painter(), k, cell.center(), icon::X, 14.0, tint);
            close |= x.clicked();
            ui.spacing_mut().item_spacing.y = 4.0;
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(SIDE, 14))
                .show(ui, |ui| {
                    ui.set_width(width);
                    add(ui)
                })
                .inner
        });
    if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.egui_wants_keyboard_input() {
        *open = false;
    }
    shown.and_then(|r| r.inner)
}

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
/// point of the drag, or typed after a click, as in Unity and Unreal:
/// Enter or a click elsewhere takes the typed value, Escape keeps the old.
#[allow(clippy::too_many_arguments)]
pub fn number(ui: &mut Ui, k: &Look, rect: Rect, id: Id, value: &mut f32, marker: Marker, framed: bool, speed: f32) {
    number_to(ui, k, rect, id, value, marker, framed, speed, 2);
}

/// A number as [`number`], shown with `decimals` places; with none, what is
/// dragged or typed is rounded to whole numbers.
#[allow(clippy::too_many_arguments)]
pub fn number_to(
    ui: &mut Ui,
    k: &Look,
    rect: Rect,
    id: Id,
    value: &mut f32,
    marker: Marker,
    framed: bool,
    speed: f32,
    decimals: usize,
) {
    let typing = id.with("typing");
    if let Some(mut text) = ui.data(|d| d.get_temp::<String>(typing)) {
        let edit = ui.put(
            rect,
            egui::TextEdit::singleline(&mut text)
                .id(typing.with("edit"))
                .font(k.font())
                .margin(egui::Margin::symmetric(6, 2))
                .horizontal_align(egui::Align::LEFT),
        );
        if !edit.has_focus() && !edit.lost_focus() {
            edit.request_focus();
        }
        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if edit.lost_focus() || cancel {
            if !cancel {
                if let Some(typed) = parse_number(&text) {
                    *value = typed;
                }
            }
            ui.data_mut(|d| d.remove::<String>(typing));
        } else {
            ui.data_mut(|d| d.insert_temp(typing, text));
        }
        return;
    }
    let resp = ui
        .interact(rect, id, Sense::click_and_drag())
        .on_hover_cursor(CursorIcon::ResizeHorizontal);
    if resp.dragged() {
        *value += resp.drag_delta().x * speed;
    }
    if resp.clicked() {
        ui.data_mut(|d| d.insert_temp(typing, format!("{value:.decimals$}")));
        ui.ctx().request_repaint();
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
        format!("{value:.decimals$}"),
        k.font(),
        color,
    );
}

/// A text field in a property's box. What is typed is kept aside while
/// the field has the keyboard and returned once, when Enter or a click
/// elsewhere ends the typing; Escape gives it up.
pub fn text_field(ui: &mut Ui, k: &Look, rect: Rect, id: Id, value: &str, hint: &str) -> Option<String> {
    let typing = id.with("typing");
    let mut text = ui
        .data(|d| d.get_temp::<String>(typing))
        .unwrap_or_else(|| value.to_owned());
    let focused = ui.memory(|m| m.has_focus(id));
    field_box(ui.painter(), k, rect, ui.rect_contains_pointer(rect), focused);
    // One line of text, in the middle of the box.
    let line = ui.painter().layout_no_wrap("Ag".into(), k.font(), k.t.text).size().y;
    let inner = Rect::from_center_size(rect.center(), Vec2::new(rect.width() - 14.0, line));
    // In a child, so the field does not move where the panel lays out
    // what comes next.
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    let edit = child.add_sized(
        inner.size(),
        egui::TextEdit::singleline(&mut text)
            .id(id)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .font(k.font())
            .hint_text(egui::RichText::new(hint).color(k.t.text_dim))
            .text_color(if focused { k.t.text_bright } else { k.t.text }),
    );
    if edit.has_focus() {
        ui.data_mut(|d| d.insert_temp(typing, text));
        return None;
    }
    ui.data_mut(|d| d.remove::<String>(typing));
    let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
    (edit.lost_focus() && !cancel && text != value).then_some(text)
}

/// A typed number: a decimal point or comma, a sign, spaces around it.
pub fn parse_number(text: &str) -> Option<f32> {
    let n: f32 = text.trim().replace(',', ".").parse().ok()?;
    n.is_finite().then_some(n)
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
    let resp = dropdown_field(ui, k, rect, id, options.get(*chosen).copied().unwrap_or(""));
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(rect.width() - 10.0);
        // A long list, as of personalities, scrolls.
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (i, option) in options.iter().enumerate() {
                if check_item(ui, k, i == *chosen, option, "").clicked() {
                    *chosen = i;
                    ui.close();
                }
            }
        });
    });
}

/// A field that opens a list, showing `text`; the caller opens the list
/// from the response.
pub fn dropdown_field(ui: &mut Ui, k: &Look, rect: Rect, id: Id, text: &str) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    let p = ui.painter();
    let fill = if resp.hovered() { k.t.hover } else { k.t.control };
    p.rect_filled(rect, k.radius(), fill);
    let color = if resp.hovered() { k.t.text_bright } else { k.t.text };
    p.with_clip_rect(rect.with_max_x(rect.right() - 22.0)).text(
        Pos2::new(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
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
    resp
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

/// The text of a menu item: an icon, a name and, to the right, the keys
/// that do the same or the state the item is in.
fn menu_job(k: &Look, icon_glyph: &str, text: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let format = |font, color| egui::TextFormat {
        font_id: font,
        color,
        valign: egui::Align::Center,
        ..Default::default()
    };
    let icon_text = if icon_glyph.is_empty() { icon::CHECK } else { icon_glyph };
    let tint = if icon_glyph.is_empty() {
        Color32::TRANSPARENT
    } else {
        k.t.text_dim
    };
    job.append(icon_text, 0.0, format(k.icons(14.0), tint));
    job.append(text, 8.0, format(k.font(), Color32::PLACEHOLDER));
    job
}

/// An item of a context menu. Returns whether it was chosen, and closes
/// the menu when it was.
pub fn menu_item(ui: &mut Ui, k: &Look, icon_glyph: &str, text: &str, keys: &str, enabled: bool) -> bool {
    let mut button = egui::Button::new(menu_job(k, icon_glyph, text));
    if !keys.is_empty() {
        button = button.shortcut_text(keys);
    }
    let clicked = ui.add_enabled(enabled, button).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// An item of a menu that stays open when it is chosen, with what it is
/// set to at its right: for values stepped through by clicking.
pub fn menu_step(ui: &mut Ui, k: &Look, icon_glyph: &str, text: &str, value: &str, enabled: bool) -> bool {
    let button = egui::Button::new(menu_job(k, icon_glyph, text)).shortcut_text(value);
    ui.add_enabled(enabled, button).clicked()
}

/// A menu inside a menu, drawn like `menu_item`.
pub fn menu_sub(ui: &mut Ui, k: &Look, icon_glyph: &str, text: &str, add: impl FnOnce(&mut Ui)) {
    ui.menu_button(menu_job(k, icon_glyph, text), add);
}

/// A text field over `rect` for renaming something in a list. Returns
/// the name when Enter or a click elsewhere took it, an empty name when
/// Escape gave it up, and `None` while the user types.
pub fn rename_field(ui: &mut Ui, k: &Look, rect: Rect, id: Id, text: &mut String) -> Option<String> {
    let p = ui.painter();
    p.rect_filled(rect, k.radius(), k.t.input);
    p.rect_stroke(rect, k.radius(), Stroke::new(1.0, k.t.select), StrokeKind::Inside);
    // In a child, so the field does not move where the panel lays out
    // what comes next.
    let inner = rect.shrink2(Vec2::new(4.0, 0.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    let edit = child.add_sized(
        inner.size(),
        egui::TextEdit::singleline(text)
            .id(id)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .font(k.font())
            .text_color(k.t.text_bright),
    );
    // The menu or the double click that started the renaming may take the
    // keyboard away in the first frames; only a field that kept it a while
    // ends when it loses it.
    let frames_id = id.with("frames");
    let frames = ui.data(|d| d.get_temp::<u32>(frames_id)).unwrap_or(0);
    let done = |ui: &Ui| ui.data_mut(|d| d.remove::<u32>(frames_id));
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        done(ui);
        Some(String::new())
    } else if edit.lost_focus() && frames > 3 {
        done(ui);
        Some(text.clone())
    } else {
        if !edit.has_focus() {
            edit.request_focus();
        }
        ui.data_mut(|d| d.insert_temp(frames_id, frames + 1));
        None
    }
}

#[cfg(test)]
mod tests {
    use super::parse_number;

    #[test]
    fn typed_numbers_take_a_point_or_a_comma_and_refuse_the_rest() {
        assert_eq!(parse_number(" 1.5 "), Some(1.5));
        assert_eq!(parse_number("-0,25"), Some(-0.25));
        assert_eq!(parse_number("3"), Some(3.0));
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number("inf"), None);
        assert_eq!(parse_number(""), None);
    }
}
