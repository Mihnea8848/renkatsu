// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The landing page of the project selector: a picture, a welcome and the
//! news. A prototype of three layouts, shown with `--landing`; the news
//! are samples until the feed is read from GitHub.

use crate::selector::{self, Act};
use crate::state::Look;
use crate::widgets::{glyph, hair_h, hair_v};
use clap::ValueEnum;
use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use egui_phosphor::regular as icon;

/// Where the landing page sits in the selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Layout {
    /// A banner across the top, the news beside the recent projects.
    Banner,
    /// A page of its own, first in the side column.
    Home,
    /// A column at the right: the picture, the welcome, the news.
    Rail,
}

/// What a piece of news is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A release on GitHub.
    Release,
    /// A post of the news file.
    Post,
}

/// One piece of news.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    pub kind: Kind,
    pub title: String,
    /// The day, as shown.
    pub date: String,
    pub summary: String,
    pub url: String,
}

/// News to picture the page with.
fn samples() -> Vec<Post> {
    let post = |kind, title: &str, date: &str, summary: &str| Post {
        kind,
        title: title.to_owned(),
        date: date.to_owned(),
        summary: summary.to_owned(),
        url: "https://github.com/Mihnea8848/renkatsu/releases".to_owned(),
    };
    vec![
        post(
            Kind::Release,
            "Renkatsu 0.1.8",
            "17 Nov",
            "Gizmos, viewport selection and keyboard shortcuts.",
        ),
        post(
            Kind::Post,
            "Posing comes next",
            "16 Nov",
            "FK and IK posing are in the works, with baked characters after them.",
        ),
        post(
            Kind::Release,
            "Renkatsu 0.1.7",
            "15 Nov",
            "Character profiles: likes, traits and the rest of the card.",
        ),
        post(
            Kind::Post,
            "Starter Content",
            "12 Nov",
            "Characters and clothing for those without Koikatsu, as a package.",
        ),
        post(
            Kind::Release,
            "Renkatsu 0.1.6",
            "10 Nov",
            "More animations, listed by group and category.",
        ),
    ]
}

const BANNER: f32 = 148.0;
const RAIL: f32 = 292.0;
const PAD: f32 = 20.0;

/// The parts a layout takes from the main area: what is left of it, the
/// banner and the column of news.
pub fn carve(layout: Option<Layout>, main: Rect) -> (Rect, Option<Rect>, Option<Rect>) {
    match layout {
        Some(Layout::Banner) => {
            let banner = Rect::from_min_size(main.min, Vec2::new(main.width(), BANNER));
            let rail = Rect::from_min_max(Pos2::new(main.right() - RAIL, banner.bottom()), main.max);
            let rest = Rect::from_min_max(banner.left_bottom(), Pos2::new(rail.left(), main.bottom()));
            (rest, Some(banner), Some(rail))
        }
        Some(Layout::Rail) => {
            let rail = Rect::from_min_max(Pos2::new(main.right() - RAIL, main.top()), main.max);
            let rest = Rect::from_min_max(main.min, Pos2::new(rail.left(), main.bottom()));
            (rest, None, Some(rail))
        }
        _ => (main, None, None),
    }
}

/// The place the art goes, marked as empty: stripes, and its size.
fn art_slot(ui: &Ui, k: &Look, rect: Rect, what: &str) {
    let t = &k.t;
    let p = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    p.rect_filled(rect, 0.0, t.input);
    let step = 22.0;
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        p.line_segment(
            [Pos2::new(x, rect.bottom()), Pos2::new(x + rect.height(), rect.top())],
            Stroke::new(1.0, t.well),
        );
        x += step;
    }
    let text = format!("{what}  {:.0} × {:.0}", rect.width(), rect.height());
    let galley = p.layout_no_wrap(text, k.small(), t.text_dim);
    let width = 14.0 + 7.0 + galley.size().x;
    let at = Pos2::new(rect.right() - 16.0 - width, rect.top() + 18.0);
    glyph(&p, k, Pos2::new(at.x + 7.0, at.y), icon::IMAGE, 14.0, t.text_dim);
    p.galley(Pos2::new(at.x + 21.0, at.y - galley.size().y / 2.0), galley, t.text_dim);
}

/// The banner: the art, darkened towards the bottom, with the welcome on
/// it.
pub fn banner(ui: &Ui, k: &Look, rect: Rect) {
    let t = &k.t;
    art_slot(ui, k, rect, "Banner");
    let p = ui.painter();
    // The welcome stays readable on any picture.
    let shade = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 84.0), rect.max);
    let mut mesh = egui::Mesh::default();
    let dark = Color32::from_black_alpha(190);
    mesh.colored_vertex(shade.left_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(shade.right_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(shade.right_bottom(), dark);
    mesh.colored_vertex(shade.left_bottom(), dark);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(mesh);
    p.text(
        Pos2::new(rect.left() + PAD, rect.bottom() - 26.0),
        Align2::LEFT_CENTER,
        "Welcome to Renkatsu",
        k.bold(20.0),
        t.text_bright,
    );
    hair_h(p, rect.left(), rect.right(), rect.bottom() - 1.0, t.divider);
}

/// A piece of news as a block of a narrow column. Returns whether it was
/// clicked.
fn block(ui: &mut Ui, k: &Look, rect: Rect, post: &Post, n: usize) -> bool {
    let t = &k.t;
    let resp = ui.interact(rect, ui.id().with(("news", n)), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, t.row_hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let (left, right) = (rect.left() + 16.0, rect.right() - 16.0);
    let (mark, tint) = match post.kind {
        Kind::Release => (icon::TAG, t.select_bright),
        Kind::Post => (icon::NEWSPAPER, t.text),
    };
    let y = rect.top() + 20.0;
    glyph(ui.painter(), k, Pos2::new(left + 7.0, y), mark, 14.0, tint);
    selector::line(
        ui,
        Pos2::new(left + 22.0, y),
        right - left - 22.0 - 50.0,
        &post.title,
        k.bold(k.d.text),
        t.text_bright,
    );
    ui.painter().text(
        Pos2::new(right, y),
        Align2::RIGHT_CENTER,
        &post.date,
        k.small(),
        t.text_dim,
    );
    let mut job = egui::text::LayoutJob::simple(post.summary.clone(), k.small(), t.text_dim, right - left);
    job.wrap.max_rows = 2;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(Pos2::new(left, y + 13.0), galley, t.text_dim);
    hair_h(ui.painter(), left, right, rect.bottom() - 0.5, t.line);
    resp.clicked()
}

/// A piece of news as a row of a wide list.
fn row(ui: &mut Ui, k: &Look, rect: Rect, post: &Post, n: usize) -> bool {
    let t = &k.t;
    let resp = ui.interact(rect, ui.id().with(("news row", n)), Sense::click());
    if resp.hovered() {
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
    let width = right - 60.0 - x;
    selector::line(
        ui,
        Pos2::new(x, y - 9.0),
        width,
        &post.title,
        k.bold(k.d.text),
        t.text_bright,
    );
    selector::line(ui, Pos2::new(x, y + 9.0), width, &post.summary, k.small(), t.text_dim);
    ui.painter().text(
        Pos2::new(right, y),
        Align2::RIGHT_CENTER,
        &post.date,
        k.small(),
        t.text_dim,
    );
    hair_h(ui.painter(), left, right, rect.bottom() - 0.5, t.line);
    resp.clicked()
}

/// The title of a part of the page.
fn title(ui: &Ui, k: &Look, at: Pos2, text: &str) {
    ui.painter()
        .text(at, Align2::LEFT_CENTER, text, k.bold(15.0), k.t.text_bright);
}

/// The column of news; with `art`, the picture and the welcome above it.
pub fn rail(ui: &mut Ui, k: &Look, rect: Rect, art: bool) -> Option<Act> {
    let t = &k.t;
    ui.painter().rect_filled(rect, 0.0, t.well);
    hair_v(ui.painter(), rect.left(), rect.top(), rect.bottom(), t.divider);
    let mut top = rect.top();
    if art {
        let slot = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 176.0));
        art_slot(ui, k, slot, "Art");
        ui.painter().text(
            Pos2::new(rect.left() + 16.0, slot.bottom() + 28.0),
            Align2::LEFT_CENTER,
            "Welcome to Renkatsu",
            k.bold(16.0),
            t.text_bright,
        );
        top = slot.bottom() + 56.0;
        hair_h(ui.painter(), rect.left() + 16.0, rect.right() - 16.0, top, t.border);
    }
    title(ui, k, Pos2::new(rect.left() + 16.0, top + 26.0), "News");
    let mut act = None;
    let height = 78.0;
    let mut y = top + 52.0;
    for (n, post) in samples().iter().enumerate() {
        if y + height > rect.bottom() {
            break;
        }
        let at = Rect::from_min_size(Pos2::new(rect.left() + 1.0, y), Vec2::new(rect.width() - 1.0, height));
        if block(ui, k, at, post, n) {
            act = Some(Act::OpenUrl(post.url.clone()));
        }
        y += height;
    }
    act
}

/// The home page: the banner with the welcome, and the news. Projects
/// have pages of their own.
pub fn home(ui: &mut Ui, k: &Look, main: Rect) -> Option<Act> {
    let mut act = None;
    let top = Rect::from_min_size(main.min, Vec2::new(main.width(), 176.0));
    banner(ui, k, top);

    let mut y = top.bottom() + 28.0;
    title(ui, k, Pos2::new(main.left() + PAD, y), "News");
    y += 22.0;
    let height = 54.0;
    for (n, post) in samples().iter().enumerate() {
        if y + height > main.bottom() - 6.0 {
            break;
        }
        let at = Rect::from_min_size(Pos2::new(main.left(), y), Vec2::new(main.width(), height));
        if row(ui, k, at, post, n) {
            act = Some(Act::OpenUrl(post.url.clone()));
        }
        y += height;
    }
    act
}
