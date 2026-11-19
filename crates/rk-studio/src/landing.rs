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
        "Welcome to Renkatsu!",
        k.bold(34.0),
        t.text_bright,
    );
    hair_h(p, rect.left(), rect.right(), rect.bottom() - 1.0, t.divider);
}

/// What a click on a post asks for.
enum Click {
    /// Show the whole post.
    Open,
    /// Open its page.
    Page,
}

/// The size of a post's text.
const BODY: f32 = 14.0;
/// How much of a post's text the list shows before it fades out.
const PREVIEW: f32 = 92.0;

/// A piece of news as a post: its date, its title and its text. In the
/// list the text is cut and fades, and a click anywhere opens the post;
/// shown whole, its title opens its page when it has one.
fn post_ui(ui: &mut Ui, k: &Look, post: &Post, today: &str, n: usize, whole: bool) -> Option<Click> {
    let t = &k.t;
    let language = Language::default();
    let width = ui.available_width().min(MEASURE);
    let top = ui.cursor().top();
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
    let title = ui
        .painter()
        .layout(post.title_in(language).to_owned(), k.bold(24.0), t.text_bright, width);
    let (title_rect, _) = ui.allocate_exact_size(Vec2::new(width, title.size().y), Sense::hover());
    ui.add_space(12.0);
    // The text, a paragraph at a time; in the list only its start.
    let font = egui::FontId::proportional(BODY);
    let galleys: Vec<_> = post
        .body_in(language)
        .split("\n\n")
        .map(|paragraph| ui.painter().layout(paragraph.to_owned(), font.clone(), t.text, width))
        .collect();
    let full: f32 = galleys.iter().map(|g| g.size().y).sum::<f32>() + 10.0 * galleys.len().saturating_sub(1) as f32;
    let cut = !whole && full > PREVIEW;
    let height = if cut { PREVIEW } else { full };
    let (text, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let p = ui.painter().with_clip_rect(text.intersect(ui.clip_rect()));
    let mut y = text.top();
    for galley in galleys {
        let h = galley.size().y;
        p.galley(Pos2::new(text.left(), y), galley, t.text);
        y += h + 10.0;
        if y > text.bottom() {
            break;
        }
    }
    if cut {
        fade(
            ui,
            Rect::from_min_max(Pos2::new(text.left(), text.bottom() - 48.0), text.max),
            t.panel,
        );
    }
    // What answers the pointer: the whole post in the list, the title of a
    // post shown whole that has a page.
    let (hit, sense) = if whole {
        let sense = if post.url.is_some() {
            Sense::click()
        } else {
            Sense::hover()
        };
        (title_rect, sense)
    } else {
        (
            Rect::from_min_max(Pos2::new(text.left(), top), text.max),
            Sense::click(),
        )
    };
    let resp = ui.interact(hit, ui.id().with(("news post", n)), sense);
    let live = resp.hovered() && (!whole || post.url.is_some());
    if live {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let color = if live { t.select_bright } else { t.text_bright };
    ui.painter().galley(title_rect.min, title, color);
    match (resp.clicked(), whole) {
        (false, _) => None,
        (true, false) => Some(Click::Open),
        (true, true) => Some(Click::Page),
    }
}

/// A band that goes from clear at its top to `color` at its bottom.
fn fade(ui: &Ui, rect: Rect, color: egui::Color32) {
    let mut mesh = egui::Mesh::default();
    let clear = egui::Color32::TRANSPARENT;
    for (pos, c) in [
        (rect.left_top(), clear),
        (rect.right_top(), clear),
        (rect.right_bottom(), color),
        (rect.left_bottom(), color),
    ] {
        mesh.colored_vertex(pos, c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// The "Back" button over a post shown whole. Returns whether it was
/// clicked.
fn back(ui: &mut Ui, k: &Look) -> bool {
    let t = &k.t;
    let text = "Back";
    let w = 22.0 + crate::widgets::text_width(ui, text, k.font()).ceil() + 10.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, k.radius(), t.row_hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let color = if resp.hovered() { t.text_bright } else { t.text };
    glyph(
        p,
        k,
        Pos2::new(rect.left() + 11.0, rect.center().y),
        icon::CARET_LEFT,
        13.0,
        color,
    );
    p.text(
        Pos2::new(rect.left() + 22.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        k.font(),
        color,
    );
    resp.clicked()
}

/// The home page: the banner with the welcome, and the news under it as
/// posts, newest first, each cut short; a click shows one whole under the
/// banner, with a way back. Projects have pages of their own. Without
/// news only the banner shows.
pub fn home(ui: &mut Ui, k: &Look, main: Rect, posts: &[Post], today: &str) -> Option<Act> {
    let mut act = None;
    let top = Rect::from_min_size(main.min, Vec2::new(main.width(), BANNER));
    banner(ui, k, top);
    if posts.is_empty() {
        return None;
    }
    // The post shown whole, by its place in the list.
    let open_id = egui::Id::new("news post shown whole");
    let mut open = ui.data(|d| d.get_temp::<usize>(open_id)).filter(|&n| n < posts.len());
    let area = Rect::from_min_max(Pos2::new(main.left(), top.bottom()), main.max);
    let mut column = ui.new_child(egui::UiBuilder::new().max_rect(area));
    let scroll = egui::ScrollArea::vertical().auto_shrink(false).id_salt(("news", open));
    scroll.show(&mut column, |ui| {
        ui.add_space(if open.is_some() { 16.0 } else { 28.0 });
        ui.horizontal(|ui| {
            ui.add_space(PAD);
            ui.vertical(|ui| {
                ui.set_max_width((area.width() - 2.0 * PAD).min(MEASURE));
                if let Some(n) = open {
                    if back(ui, k) {
                        open = None;
                    }
                    ui.add_space(14.0);
                    if let Some(Click::Page) = post_ui(ui, k, &posts[n], today, n, true) {
                        act = posts[n].url.clone().map(Act::OpenUrl);
                    }
                } else {
                    for (n, post) in posts.iter().enumerate() {
                        if n > 0 {
                            ui.add_space(22.0);
                            let (line, _) =
                                ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
                            hair_h(ui.painter(), line.left(), line.right(), line.center().y, k.t.line);
                            ui.add_space(22.0);
                        }
                        if post_ui(ui, k, post, today, n, false).is_some() {
                            open = Some(n);
                        }
                    }
                }
                ui.add_space(32.0);
            });
        });
    });
    ui.data_mut(|d| {
        if let Some(n) = open {
            d.insert_temp(open_id, n);
        } else {
            d.remove::<usize>(open_id);
        }
    });
    act
}
