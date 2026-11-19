// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Keys and mouse buttons drawn as what they are: a key as a key cap with
//! its symbol, a mouse button as a mouse with that button lit.

use crate::keymap::Chord;
use crate::state::Look;
use egui::{Align2, Color32, CornerRadius, Painter, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};
use egui_phosphor::regular as icon;

/// A mouse button, or the wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mouse {
    Left,
    Right,
    Middle,
    Wheel,
}

/// One thing pressed together with the others.
#[derive(Debug, Clone, PartialEq)]
pub enum Cap {
    /// A key with a name.
    Key(String),
    /// A key shown by its symbol, with its name beside it or not.
    Symbol(&'static str, &'static str),
    Mouse(Mouse),
    /// The mouse moves while the button is held.
    Drag,
}

/// The caps of a chord: its modifiers, then its key.
pub fn of_chord(chord: Chord) -> Vec<Cap> {
    let mut caps = Vec::new();
    if chord.ctrl {
        caps.push(Cap::Key("Ctrl".into()));
    }
    if chord.shift {
        caps.push(Cap::Symbol(icon::ARROW_FAT_UP, "Shift"));
    }
    if chord.alt {
        caps.push(Cap::Key("Alt".into()));
    }
    caps.push(of_key(chord.key));
    caps
}

/// The cap of one key.
pub fn of_key(key: egui::Key) -> Cap {
    use egui::Key as K;
    match key {
        K::Enter => Cap::Symbol(icon::KEY_RETURN, "Enter"),
        K::Backspace => Cap::Symbol(icon::BACKSPACE, "Backspace"),
        K::Tab => Cap::Symbol(icon::ARROW_LINE_RIGHT, "Tab"),
        K::ArrowUp => Cap::Symbol(icon::ARROW_UP, ""),
        K::ArrowDown => Cap::Symbol(icon::ARROW_DOWN, ""),
        K::ArrowLeft => Cap::Symbol(icon::ARROW_LEFT, ""),
        K::ArrowRight => Cap::Symbol(icon::ARROW_RIGHT, ""),
        K::Escape => Cap::Key("Esc".into()),
        K::Delete => Cap::Key("Del".into()),
        K::Backtick => Cap::Key("`".into()),
        K::Backslash => Cap::Key("\\".into()),
        K::Comma => Cap::Key(",".into()),
        other => Cap::Key(other.name().into()),
    }
}

/// The height of a cap.
fn height(k: &Look) -> f32 {
    (k.field() + 2.0).max(22.0)
}

const GAP: f32 = 4.0;
const SYMBOL: f32 = 15.0;

/// The width of one cap.
fn cap_width(ui: &Ui, k: &Look, cap: &Cap) -> f32 {
    let h = height(k);
    let text = |t: &str| crate::widgets::text_width(ui, t, k.font());
    match cap {
        Cap::Key(name) => (text(name) + 12.0).max(h),
        Cap::Symbol(_, "") => h,
        Cap::Symbol(_, name) => SYMBOL + 4.0 + text(name) + 12.0,
        Cap::Mouse(_) => 19.0,
        Cap::Drag => 16.0,
    }
}

/// The width of caps drawn side by side.
pub fn width(ui: &Ui, k: &Look, caps: &[Cap]) -> f32 {
    let sum: f32 = caps.iter().map(|c| cap_width(ui, k, c)).sum();
    sum + GAP * caps.len().saturating_sub(1) as f32
}

/// Draws caps side by side ending at `right`, centred on `y`, and gives
/// back where they are.
pub fn draw(ui: &Ui, k: &Look, caps: &[Cap], right: f32, y: f32, color: Color32) -> Rect {
    let h = height(k);
    let total = width(ui, k, caps);
    let mut x = right - total;
    let p = ui.painter();
    for cap in caps {
        let w = cap_width(ui, k, cap);
        let rect = Rect::from_min_size(Pos2::new(x, (y - h / 2.0).round()), Vec2::new(w, h));
        // The label sits above the key's lip.
        let c = rect.center() - Vec2::new(0.0, 1.0);
        match cap {
            Cap::Key(name) => {
                key_body(p, k, rect);
                p.text(c, Align2::CENTER_CENTER, name, k.font(), color);
            }
            Cap::Symbol(glyph, name) => {
                key_body(p, k, rect);
                let symbol = egui::FontId::proportional(SYMBOL);
                if name.is_empty() {
                    p.text(c, Align2::CENTER_CENTER, *glyph, symbol, color);
                } else {
                    let left = Pos2::new(rect.left() + 6.0, c.y);
                    p.text(left, Align2::LEFT_CENTER, *glyph, symbol, color);
                    let at = Pos2::new(left.x + SYMBOL + 4.0, c.y);
                    p.text(at, Align2::LEFT_CENTER, *name, k.font(), color);
                }
            }
            Cap::Mouse(button) => mouse(p, k, rect.center(), *button, color),
            Cap::Drag => {
                let font = egui::FontId::proportional(15.0);
                p.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    icon::ARROWS_OUT_CARDINAL,
                    font,
                    k.t.text_dim,
                );
            }
        }
        x += w + GAP;
    }
    Rect::from_min_size(Pos2::new(right - total, (y - h / 2.0).round()), Vec2::new(total, h))
}

/// The body of a key, raised by a darker lip at the bottom.
fn key_body(p: &Painter, k: &Look, rect: Rect) {
    let r = CornerRadius::same(4);
    p.rect_filled(rect, r, k.t.well);
    let top = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.bottom() - 2.0));
    p.rect(top, r, k.t.control, Stroke::new(1.0, k.t.border), StrokeKind::Inside);
}

/// A mouse seen from above with one button, or the wheel, lit.
fn mouse(p: &Painter, k: &Look, c: Pos2, lit: Mouse, color: Color32) {
    let (w, h) = (17.0, 25.0);
    let body = Rect::from_center_size(c, Vec2::new(w, h));
    let split = body.top() + h * 0.42;
    let on = k.t.select_bright;
    // The lit button first, under the outline.
    let half = |left: bool| {
        let (x0, x1) = if left { (body.left(), c.x) } else { (c.x, body.right()) };
        Rect::from_min_max(Pos2::new(x0, body.top()), Pos2::new(x1, split))
    };
    let corner = |left: bool| {
        if left {
            CornerRadius {
                nw: 8,
                ne: 0,
                sw: 0,
                se: 0,
            }
        } else {
            CornerRadius {
                nw: 0,
                ne: 8,
                sw: 0,
                se: 0,
            }
        }
    };
    match lit {
        Mouse::Left => {
            p.rect_filled(half(true), corner(true), on);
        }
        Mouse::Right => {
            p.rect_filled(half(false), corner(false), on);
        }
        _ => {}
    }
    p.rect(
        body,
        CornerRadius::same(8),
        Color32::TRANSPARENT,
        Stroke::new(1.2, color),
        StrokeKind::Inside,
    );
    let line = Stroke::new(1.0, color);
    p.line_segment([Pos2::new(body.left(), split), Pos2::new(body.right(), split)], line);
    p.line_segment([Pos2::new(c.x, body.top()), Pos2::new(c.x, split)], line);
    // The wheel, in the middle of the split.
    let wheel = Rect::from_center_size(Pos2::new(c.x, body.top() + h * 0.24), Vec2::new(4.0, 7.0));
    let wheel_lit = matches!(lit, Mouse::Middle | Mouse::Wheel);
    p.rect_filled(wheel, CornerRadius::same(2), if wheel_lit { on } else { k.t.panel });
    p.rect(
        wheel,
        CornerRadius::same(2),
        Color32::TRANSPARENT,
        Stroke::new(1.0, color),
        StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chord_lists_its_modifiers_then_its_key() {
        let chord = Chord {
            ctrl: true,
            shift: true,
            alt: false,
            key: egui::Key::S,
        };
        assert_eq!(
            of_chord(chord),
            [
                Cap::Key("Ctrl".into()),
                Cap::Symbol(icon::ARROW_FAT_UP, "Shift"),
                Cap::Key("S".into())
            ]
        );
        assert_eq!(of_key(egui::Key::Enter), Cap::Symbol(icon::KEY_RETURN, "Enter"));
    }
}
