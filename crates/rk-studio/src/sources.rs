// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! What the import windows share at their left: the dropdown that says
//! where to import from (Koikatsu, a package, the mods), and the rows of
//! the tree below it.

use crate::state::Look;
use crate::widgets::{check_item, glyph, hair_h, label};
use egui::{Align2, Id, Pos2, Rect, Sense, Ui, Vec2};
use egui_phosphor::regular as icon;

/// How wide the left column of an import window is.
pub const COLUMN: f32 = 264.0;

/// A source in the dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    /// How far in it stands, as a mod under Mods.
    pub depth: usize,
    /// How many things it holds, when that is known.
    pub count: Option<usize>,
}

/// The dropdown at the top of the column `column`. Returns the source
/// that was picked, if one was, and the rest of the column.
pub fn dropdown(ui: &mut Ui, k: &Look, column: Rect, id: Id, items: &[Item], chosen: usize) -> (Option<usize>, Rect) {
    let t = &k.t;
    let bar = Rect::from_min_size(column.min, Vec2::new(column.width(), k.d.row + 12.0));
    let rest = Rect::from_min_max(bar.left_bottom(), column.max);
    hair_h(ui.painter(), bar.left(), bar.right(), bar.bottom() - 1.0, t.line);
    let rect = Rect::from_min_max(
        Pos2::new(bar.left() + 12.0, bar.center().y - k.field() / 2.0),
        Pos2::new(bar.right() - 12.0, bar.center().y + k.field() / 2.0),
    );
    let resp = ui.interact(rect, id, Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, k.radius(), if resp.hovered() { t.hover } else { t.control });
    let color = if resp.hovered() { t.text_bright } else { t.text };
    crate::selector::line(
        ui,
        Pos2::new(rect.left() + 8.0, rect.center().y),
        rect.width() - 30.0,
        items.get(chosen).map_or("", |i| i.label.as_str()),
        k.font(),
        color,
    );
    glyph(
        ui.painter(),
        k,
        Pos2::new(rect.right() - 12.0, rect.center().y + 1.0),
        icon::CARET_DOWN,
        10.0,
        color,
    );
    let mut picked = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(rect.width() - 10.0);
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (i, item) in items.iter().enumerate() {
                let text = format!("{}{}", "      ".repeat(item.depth), item.label);
                let count = item.count.map_or_else(String::new, |n| n.to_string());
                if check_item(ui, k, i == chosen, &text, &count).clicked() {
                    picked = Some(i);
                    ui.close();
                }
            }
        });
    });
    (picked, rest)
}

/// A caption between the rows of a tree.
pub fn caption(ui: &mut Ui, k: &Look, text: &str) {
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row + 4.0), Sense::hover());
    ui.painter().text(
        Pos2::new(row.left() + 14.0, row.bottom() - 9.0),
        Align2::LEFT_CENTER,
        text.to_uppercase(),
        k.small(),
        k.t.text_dim,
    );
}

/// A row of a tree.
pub struct Row<'a> {
    pub label: &'a str,
    pub depth: usize,
    /// How many things it holds, when that is known.
    pub count: Option<usize>,
    /// It is the one that shows.
    pub on: bool,
    /// Whether it is unfolded, when it has rows below it.
    pub open: Option<bool>,
}

/// What was clicked of a row: the row itself, or its caret.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Clicked {
    pub row: bool,
    pub fold: bool,
}

/// Draws a row of a tree, with its caret, name and count.
pub fn row(ui: &mut Ui, k: &Look, key: &str, of: Row) -> Clicked {
    let t = &k.t;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row), Sense::click());
    let p = ui.painter();
    if of.on {
        p.rect_filled(rect, 0.0, t.control);
        p.rect_filled(
            Rect::from_min_size(rect.left_top(), Vec2::new(2.0, rect.height())),
            0.0,
            t.select,
        );
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, t.row_hover);
    }
    let x = rect.left() + 14.0 + of.depth as f32 * 16.0;
    let y = rect.center().y;
    let mut fold = false;
    if let Some(open) = of.open {
        let g = if open { icon::CARET_DOWN } else { icon::CARET_RIGHT };
        let cell = Rect::from_center_size(Pos2::new(x + 4.0, y), Vec2::new(18.0, rect.height()));
        let caret = ui.interact(cell, ui.id().with(("fold", key)), Sense::click());
        fold = caret.clicked();
        let color = if caret.hovered() { t.text_bright } else { t.text_dim };
        glyph(ui.painter(), k, Pos2::new(x + 4.0, y), g, 10.0, color);
    }
    let p = ui.painter();
    let color = if of.on { t.text_bright } else { t.text };
    let clip = p.with_clip_rect(rect.with_max_x(rect.right() - 44.0));
    label(&clip, k, Pos2::new(x + 16.0, y), of.label, color);
    p.text(
        Pos2::new(rect.right() - 12.0, y),
        Align2::RIGHT_CENTER,
        of.count.map_or_else(String::new, |n| n.to_string()),
        k.small(),
        t.text_dim,
    );
    Clicked {
        row: !fold && resp.clicked(),
        fold,
    }
}
