// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The home page of the project selector: the banner with the welcome,
//! and the news under it. The news come from `rk_packages::news`.

use crate::brand::{self, Art};
use crate::news_pictures::{Pictures, State};
use crate::selector::Act;
use crate::state::Look;
use crate::widgets::{glyph, hair_h};
use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};
use egui_phosphor::regular as icon;
use rk_packages::news::{Kind, Language, Picture, Post};

const BANNER: f32 = 280.0;
/// The key of a scroll a screenshot asks for, taken once a post is open.
pub const SCROLL_START: &str = "news scroll start";
const PAD: f32 = 32.0;
/// The widest a post's text runs, for lines that read well.
const MEASURE: f32 = 760.0;
/// Seconds a post takes to rise over the banner, and to go back down.
const OPENING: f32 = 0.32;

/// Where the column of news starts in a page `width` wide: in its middle,
/// or `PAD` from its left where it is narrow.
fn column_left(width: f32) -> f32 {
    ((width - MEASURE) / 2.0).max(PAD).round()
}

/// The banner: the art, darkened towards the bottom, with the welcome on
/// it.
fn banner(ui: &Ui, k: &Look, rect: Rect) {
    let left = rect.left() + column_left(rect.width());
    let t = &k.t;
    brand::art(ui, k, rect, Art::Banner);
    let p = ui.painter();
    let shade = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 120.0), rect.max);
    brand::shade(p, shade, 200);
    p.text(
        Pos2::new(left, rect.bottom() - 40.0),
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
/// The space between paragraphs, and around a picture.
const GAP: f32 = 10.0;
const PICTURE_GAP: f32 = 22.0;
/// The tallest a picture shows, so that a tall one leaves room for the
/// text around it; less where the page shows less, so that a picture is
/// always seen whole with its caption.
const TALLEST: f32 = 540.0;
const TALLEST_OF_VIEW: f32 = 0.85;

/// A paragraph of a post as the page lays it out.
enum Block {
    Text(std::sync::Arc<egui::Galley>),
    Picture(Picture, Option<std::sync::Arc<egui::Galley>>),
}

/// The size a picture shows at in a column `width` wide: its own size,
/// smaller to fit the column and `TALLEST`, never larger; a frame of the
/// column's width while it loads; nothing when it could not be had.
fn picture_size(state: &State, width: f32, tallest: f32, points_per_pixel: f32) -> Option<Vec2> {
    match state {
        State::Loading => Some(Vec2::new(width, (width * 9.0 / 16.0).min(tallest).round())),
        State::Failed => None,
        State::Ready(shown) => {
            let natural = Vec2::new(shown.size[0] as f32, shown.size[1] as f32) * points_per_pixel;
            let scale = (width / natural.x).min(tallest / natural.y).min(1.0);
            Some((natural * scale).round())
        }
    }
}

/// A piece of news as a post: its date, its title and its text. In the
/// list the text is cut and fades, and a click anywhere opens the post;
/// shown whole, its title opens its page when it has one.
fn post_ui(
    ui: &mut Ui,
    k: &Look,
    post: &Post,
    today: &str,
    n: usize,
    whole: bool,
    pictures: &mut Pictures,
) -> Option<Click> {
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
    let date = p.text(
        Pos2::new(line.left() + 24.0, line.center().y),
        Align2::LEFT_CENTER,
        post.day(today),
        k.small(),
        t.text_dim,
    );
    if let Some(author) = &post.author {
        let dot = Pos2::new(date.right() + 8.0, line.center().y);
        p.circle_filled(dot, 1.5, t.text_dim);
        p.text(
            Pos2::new(dot.x + 8.0, line.center().y),
            Align2::LEFT_CENTER,
            author,
            k.small(),
            t.text,
        );
    }
    ui.add_space(6.0);
    let title = ui
        .painter()
        .layout(post.title_in(language).to_owned(), k.bold(24.0), t.text_bright, width);
    let (title_rect, _) = ui.allocate_exact_size(Vec2::new(width, title.size().y), Sense::hover());
    ui.add_space(12.0);
    // The text, a paragraph at a time; in the list only its start, and no
    // pictures: they show once the post is opened.
    let font = egui::FontId::proportional(BODY);
    let blocks: Vec<Block> = post
        .body_in(language)
        .split("\n\n")
        .filter_map(|paragraph| {
            if let Some(picture) = rk_packages::news::picture(paragraph) {
                let caption = (!picture.caption.is_empty()).then(|| {
                    ui.painter()
                        .layout(picture.caption.clone(), k.small(), t.text_dim, width)
                });
                return whole.then_some(Block::Picture(picture, caption));
            }
            Some(Block::Text(match paragraph.strip_prefix(rk_packages::news::HEADING) {
                Some(heading) => ui
                    .painter()
                    .layout(heading.to_owned(), k.bold(BODY + 3.0), t.text_bright, width),
                None => ui.painter().layout(paragraph.to_owned(), font.clone(), t.text, width),
            }))
        })
        .collect();
    // Where each block goes, from the top of the text: a picture has more
    // room around it than a paragraph, and its caption under it.
    let per_pixel = 1.0 / ui.ctx().pixels_per_point();
    let tallest = TALLEST
        .min((ui.clip_rect().height() * TALLEST_OF_VIEW).max(160.0))
        .round();
    let mut sizes: Vec<Option<Vec2>> = Vec::with_capacity(blocks.len());
    let mut full: f32 = 0.0;
    let mut after: f32 = 0.0;
    for block in &blocks {
        let (size, gap) = match block {
            Block::Text(g) => (Some(g.size()), GAP),
            Block::Picture(picture, caption) => {
                let state = pictures.get(ui.ctx(), &picture.url);
                let size = picture_size(&state, width, tallest, per_pixel);
                let tall = size.map(|s| s.y + caption.as_ref().map_or(0.0, |c| 8.0 + c.size().y));
                (tall.map(|h| Vec2::new(width, h)), PICTURE_GAP)
            }
        };
        if let Some(size) = size {
            if full > 0.0 {
                full += after.max(gap);
            }
            full += size.y;
            after = gap;
        }
        sizes.push(size);
    }
    let cut = !whole && full > PREVIEW;
    let height = if cut { PREVIEW } else { full };
    let (text, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let p = ui.painter().with_clip_rect(text.intersect(ui.clip_rect()));
    let mut y = text.top();
    let mut after = 0.0;
    let mut first = true;
    for (block, size) in blocks.into_iter().zip(sizes) {
        let Some(size) = size else { continue };
        let gap = if matches!(block, Block::Text(_)) {
            GAP
        } else {
            PICTURE_GAP
        };
        if !first {
            y += f32::max(after, gap);
        }
        first = false;
        after = gap;
        match block {
            Block::Text(galley) => p.galley(Pos2::new(text.left(), y), galley, t.text),
            Block::Picture(picture, caption) => {
                let state = pictures.get(ui.ctx(), &picture.url);
                let shown = picture_size(&state, width, tallest, per_pixel).unwrap_or(size);
                // A picture narrower than the column sits in its middle.
                let at = Rect::from_min_size(Pos2::new((text.left() + (width - shown.x) / 2.0).round(), y), shown);
                picture_ui(ui, &p, k, &state, at);
                if let Some(caption) = caption {
                    let x = (text.left() + (width - caption.size().x) / 2.0).round();
                    p.galley(Pos2::new(x, at.bottom() + 8.0), caption, t.text_dim);
                }
            }
        }
        y += size.y;
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

/// A picture in its place: rounded like the panels, with a hairline round
/// it; a quiet frame while it loads. A moving one asks for its next frame.
fn picture_ui(ui: &Ui, p: &egui::Painter, k: &Look, state: &State, at: Rect) {
    let t = &k.t;
    let round = k.radius() + 2.0;
    match state {
        State::Loading => {
            p.rect_filled(at, round, t.input);
            glyph(p, k, at.center(), icon::IMAGE, 28.0, t.text_dim.gamma_multiply(0.5));
        }
        State::Ready(shown) => {
            let (texture, next) = shown.frame(ui.input(|i| i.time));
            p.add(egui::Shape::Rect(
                egui::epaint::RectShape::filled(at, round, egui::Color32::WHITE)
                    .with_texture(texture.id(), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))),
            ));
            if let Some(next) = next {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f32(next));
            }
        }
        State::Failed => return,
    }
    p.rect_stroke(at, round, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
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
pub fn home(ui: &mut Ui, k: &Look, main: Rect, posts: &[Post], today: &str, pictures: &mut Pictures) -> Option<Act> {
    let mut act = None;
    let top = Rect::from_min_size(main.min, Vec2::new(main.width(), BANNER));
    banner(ui, k, top);
    if posts.is_empty() {
        return None;
    }
    // The post shown whole, by its place in the list, and whether it is
    // going back down to the list: it shows until it is down.
    let open_id = egui::Id::new("news post shown whole");
    let leaving_id = egui::Id::new("news post going back");
    let mut open = ui.data(|d| d.get_temp::<usize>(open_id)).filter(|&n| n < posts.len());
    let mut leaving = open.is_some() && ui.data(|d| d.get_temp::<bool>(leaving_id)).unwrap_or(false);
    // An open post rises over the banner until it has the whole page.
    let risen = ui.ctx().animate_bool_with_time_and_easing(
        egui::Id::new("news post over the banner"),
        open.is_some() && !leaving,
        OPENING,
        egui::emath::easing::cubic_in_out,
    );
    if leaving && risen <= 0.0 {
        (open, leaving) = (None, false);
    }
    let area = Rect::from_min_max(
        Pos2::new(main.left(), (top.bottom() - BANNER * risen).round()),
        main.max,
    );
    if risen > 0.0 {
        let p = ui.painter();
        p.rect_filled(area, 0.0, k.t.panel);
        hair_h(p, area.left(), area.right(), area.top(), k.t.divider);
    }
    let mut column = ui.new_child(egui::UiBuilder::new().max_rect(area));
    let mut scroll = egui::ScrollArea::vertical().auto_shrink(false).id_salt(("news", open));
    if open.is_some() {
        if let Some(y) = ui.data_mut(|d| d.remove_temp::<f32>(egui::Id::new(SCROLL_START))) {
            scroll = scroll.vertical_scroll_offset(y);
        }
    }
    scroll.show(&mut column, |ui| {
        ui.add_space(if open.is_some() { 16.0 } else { 28.0 });
        ui.horizontal(|ui| {
            ui.add_space(column_left(area.width()));
            ui.vertical(|ui| {
                ui.set_max_width((area.width() - 2.0 * PAD).min(MEASURE));
                if let Some(n) = open {
                    if back(ui, k) && !leaving {
                        leaving = true;
                    }
                    ui.add_space(14.0);
                    if let Some(Click::Page) = post_ui(ui, k, &posts[n], today, n, true, pictures) {
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
                        if post_ui(ui, k, post, today, n, false, pictures).is_some() {
                            (open, leaving) = (Some(n), false);
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
        d.insert_temp(leaving_id, leaving);
    });
    act
}
