// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The home page of the project selector: the banner with the welcome,
//! and the news under it. The news come from `rk_packages::news`.

use crate::brand::{self, Art};
use crate::selector::Act;
use crate::state::Look;
use crate::widgets::{glyph, hair_h};
use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};
use egui_phosphor::regular as icon;
use rk_packages::news::{Kind, Language, Post};

const BANNER: f32 = 280.0;
const PAD: f32 = 32.0;
/// The widest a post's text runs, for lines that read well.
const MEASURE: f32 = 760.0;

/// The banner: the art, darkened towards the bottom, with the welcome on
/// it.
fn banner(ui: &Ui, k: &Look, rect: Rect) {
    let t = &k.t;
    brand::art(ui, k, rect, Art::Banner);
    let p = ui.painter();
    let shade = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 120.0), rect.max);
    brand::shade(p, shade, 200);
    p.text(
        Pos2::new(rect.left() + PAD, rect.bottom() - 40.0),
        Align2::LEFT_CENTER,
        "Welcome to Renkatsu",
        k.bold(34.0),
        t.text_bright,
    );
    hair_h(p, rect.left(), rect.right(), rect.bottom() - 1.0, t.divider);
}

/// A piece of news as a post: its date, its title and its whole text.
/// Returns whether its title was clicked; a post without a page answers no
/// click.
fn post_ui(ui: &mut Ui, k: &Look, post: &Post, today: &str, n: usize) -> bool {
    let t = &k.t;
    let language = Language::default();
    let width = ui.available_width().min(MEASURE);
    let mut clicked = false;
    // The date, with the mark of a release or a post.
    let (mark, tint) = match post.kind {
        Kind::Release => (icon::TAG, t.select_bright),
        Kind::Post => (icon::NEWSPAPER, t.text_dim),
    };
    let (line, _) = ui.allocate_exact_size(Vec2::new(width, 20.0), Sense::hover());
    let p = ui.painter();
    glyph(p, k, Pos2::new(line.left() + 8.0, line.center().y), mark, 14.0, tint);
    p.text(
        Pos2::new(line.left() + 24.0, line.center().y),
        Align2::LEFT_CENTER,
        post.day(today),
        k.small(),
        t.text_dim,
    );
    ui.add_space(6.0);
    // The title, which opens the post's page when it has one.
    let title = ui
        .painter()
        .layout(post.title_in(language).to_owned(), k.bold(24.0), t.text_bright, width);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, title.size().y), Sense::hover());
    let resp = ui.interact(
        rect,
        ui.id().with(("news title", n)),
        if post.url.is_some() {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let color = if resp.hovered() && post.url.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        t.select_bright
    } else {
        t.text_bright
    };
    ui.painter().galley(rect.min, title, color);
    clicked |= resp.clicked();
    ui.add_space(12.0);
    // The text, a paragraph at a time.
    for (i, paragraph) in post.body_in(language).split("\n\n").enumerate() {
        if i > 0 {
            ui.add_space(10.0);
        }
        let galley = ui
            .painter()
            .layout(paragraph.to_owned(), egui::FontId::proportional(15.0), t.text, width);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, galley.size().y), Sense::hover());
        ui.painter().galley(rect.min, galley, t.text);
    }
    clicked
}

/// The home page: the banner with the welcome, and the news under it as
/// posts, newest first. Projects have pages of their own. Without news
/// only the banner shows.
pub fn home(ui: &mut Ui, k: &Look, main: Rect, posts: &[Post], today: &str) -> Option<Act> {
    let mut act = None;
    let top = Rect::from_min_size(main.min, Vec2::new(main.width(), BANNER));
    banner(ui, k, top);
    if posts.is_empty() {
        return None;
    }
    let area = Rect::from_min_max(Pos2::new(main.left(), top.bottom()), main.max);
    let mut column = ui.new_child(egui::UiBuilder::new().max_rect(area));
    egui::ScrollArea::vertical().auto_shrink(false).show(&mut column, |ui| {
        ui.add_space(28.0);
        ui.horizontal(|ui| {
            ui.add_space(PAD);
            ui.vertical(|ui| {
                ui.set_max_width((area.width() - 2.0 * PAD).min(MEASURE));
                for (n, post) in posts.iter().enumerate() {
                    if n > 0 {
                        ui.add_space(26.0);
                        let (line, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
                        hair_h(ui.painter(), line.left(), line.right(), line.center().y, k.t.line);
                        ui.add_space(26.0);
                    }
                    if post_ui(ui, k, post, today, n) {
                        act = post.url.clone().map(Act::OpenUrl);
                    }
                }
                ui.add_space(32.0);
            });
        });
    });
    act
}
