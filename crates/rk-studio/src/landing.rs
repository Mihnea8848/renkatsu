// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The home page of the project selector: the banner with the welcome,
//! and the news under it. The news come from `rk_packages::news`.

use crate::brand::{self, Art};
use crate::selector::{self, Act};
use crate::state::Look;
use crate::widgets::{glyph, hair_h};
use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};
use egui_phosphor::regular as icon;
use rk_packages::news::{Kind, Language, Post};

const BANNER: f32 = 176.0;
const PAD: f32 = 20.0;
const ROW: f32 = 54.0;

/// The banner: the art, darkened towards the bottom, with the welcome on
/// it.
fn banner(ui: &Ui, k: &Look, rect: Rect) {
    let t = &k.t;
    brand::art(ui, k, rect, Art::Banner);
    let p = ui.painter();
    let shade = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 84.0), rect.max);
    brand::shade(p, shade, 190);
    p.text(
        Pos2::new(rect.left() + PAD, rect.bottom() - 26.0),
        Align2::LEFT_CENTER,
        "Welcome to Renkatsu",
        k.bold(20.0),
        t.text_bright,
    );
    hair_h(p, rect.left(), rect.right(), rect.bottom() - 1.0, t.divider);
}

/// A piece of news as a row. Returns whether it was clicked; a post
/// without a page answers no click.
fn row(ui: &mut Ui, k: &Look, rect: Rect, post: &Post, today: &str, n: usize) -> bool {
    let t = &k.t;
    let language = Language::default();
    let sense = if post.url.is_some() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(rect, ui.id().with(("news row", n)), sense);
    if resp.hovered() && post.url.is_some() {
        ui.painter().rect_filled(rect, 0.0, t.row_hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let (left, right) = (rect.left() + PAD, rect.right() - PAD);
    let (mark, tint) = match post.kind {
        Kind::Release => (icon::TAG, t.select_bright),
        Kind::Post => (icon::NEWSPAPER, t.text),
    };
    let y = rect.center().y;
    glyph(ui.painter(), k, Pos2::new(left + 8.0, y), mark, 16.0, tint);
    let x = left + 30.0;
    let width = right - 90.0 - x;
    let summary = post.summary_in(language);
    let title_y = if summary.is_empty() { y } else { y - 9.0 };
    selector::line(
        ui,
        Pos2::new(x, title_y),
        width,
        post.title_in(language),
        k.bold(k.d.text),
        t.text_bright,
    );
    if !summary.is_empty() {
        selector::line(ui, Pos2::new(x, y + 9.0), width, summary, k.small(), t.text_dim);
    }
    ui.painter().text(
        Pos2::new(right, y),
        Align2::RIGHT_CENTER,
        post.day(today),
        k.small(),
        t.text_dim,
    );
    hair_h(ui.painter(), left, right, rect.bottom() - 0.5, t.line);
    resp.clicked()
}

/// The home page: the banner with the welcome, and the news. Projects
/// have pages of their own. Without news only the banner shows.
pub fn home(ui: &mut Ui, k: &Look, main: Rect, posts: &[Post], today: &str) -> Option<Act> {
    let mut act = None;
    let top = Rect::from_min_size(main.min, Vec2::new(main.width(), BANNER));
    banner(ui, k, top);
    if posts.is_empty() {
        return None;
    }
    let mut y = top.bottom() + 28.0;
    ui.painter().text(
        Pos2::new(main.left() + PAD, y),
        Align2::LEFT_CENTER,
        "News",
        k.bold(15.0),
        k.t.text_bright,
    );
    y += 22.0;
    for (n, post) in posts.iter().enumerate() {
        if y + ROW > main.bottom() - 6.0 {
            break;
        }
        let at = Rect::from_min_size(Pos2::new(main.left(), y), Vec2::new(main.width(), ROW));
        if row(ui, k, at, post, today, n) {
            act = post.url.clone().map(Act::OpenUrl);
        }
        y += ROW;
    }
    act
}
