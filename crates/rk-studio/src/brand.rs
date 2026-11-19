// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The pictures of the brand: the banner of the home page and the art of
//! the splash. Until they exist their places are drawn as marked slots.

use crate::state::Look;
use crate::widgets::glyph;
use egui::{Color32, Pos2, Rect, Stroke, TextureHandle, Ui};
use egui_phosphor::regular as icon;

/// The banner of the home page, as a PNG of twice the slot's size
/// (1664x352). To put the art in, name its file here:
/// `Some(include_bytes!("../assets/brand/banner.png"))`.
const BANNER: Option<&[u8]> = None;
/// The art of the splash, as a PNG of twice the selector's window
/// (2080x1360): `Some(include_bytes!("../assets/brand/splash.png"))`.
const SPLASH: Option<&[u8]> = None;

/// A place art goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Art {
    Banner,
    Splash,
}

impl Art {
    fn png(self) -> Option<&'static [u8]> {
        match self {
            Art::Banner => BANNER,
            Art::Splash => SPLASH,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Art::Banner => "Banner",
            Art::Splash => "Splash art",
        }
    }
}

/// Reads a PNG into a picture egui can show.
fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let mut reader = png::Decoder::new(bytes).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let size = [info.width as usize, info.height as usize];
    let data = &buf[..info.buffer_size()];
    match info.color_type {
        png::ColorType::Rgba => Some(egui::ColorImage::from_rgba_unmultiplied(size, data)),
        png::ColorType::Rgb => Some(egui::ColorImage::from_rgb(size, data)),
        _ => None,
    }
}

/// The texture of a piece of art, decoded once; `None` while there is no
/// art, or when its file does not read.
fn texture(ui: &Ui, which: Art) -> Option<TextureHandle> {
    let bytes = which.png()?;
    let id = egui::Id::new(("brand art", which));
    if let Some(known) = ui.data(|d| d.get_temp::<Option<TextureHandle>>(id)) {
        return known;
    }
    let made = decode(bytes).map(|image| ui.ctx().load_texture(which.name(), image, egui::TextureOptions::LINEAR));
    ui.data_mut(|d| d.insert_temp(id, made.clone()));
    made
}

/// Darkens a rectangle towards its bottom, so that text on it reads on
/// any picture.
pub fn shade(p: &egui::Painter, rect: Rect, alpha: u8) {
    let mut mesh = egui::Mesh::default();
    let dark = Color32::from_black_alpha(alpha);
    mesh.colored_vertex(rect.left_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(rect.right_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(rect.right_bottom(), dark);
    mesh.colored_vertex(rect.left_bottom(), dark);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(mesh);
}

/// Draws a piece of art over `rect`, covering it and keeping its middle.
/// While the art does not exist the place is marked: stripes, and its size.
pub fn art(ui: &Ui, k: &Look, rect: Rect, which: Art) {
    let t = &k.t;
    let p = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    p.rect_filled(rect, 0.0, t.input);
    if let Some(texture) = texture(ui, which) {
        let uv = crate::selector::cover_uv(texture.size_vec2(), rect.size());
        p.image(texture.id(), rect, uv, Color32::WHITE);
        return;
    }
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        p.line_segment(
            [Pos2::new(x, rect.bottom()), Pos2::new(x + rect.height(), rect.top())],
            Stroke::new(1.0, t.well),
        );
        x += 22.0;
    }
    let text = format!("{}  {:.0} × {:.0}", which.name(), rect.width(), rect.height());
    let galley = p.layout_no_wrap(text, k.small(), t.text_dim);
    let width = 14.0 + 7.0 + galley.size().x;
    let at = Pos2::new(rect.right() - 16.0 - width, rect.top() + 18.0);
    glyph(&p, k, Pos2::new(at.x + 7.0, at.y), icon::IMAGE, 14.0, t.text_dim);
    p.galley(Pos2::new(at.x + 21.0, at.y - galley.size().y / 2.0), galley, t.text_dim);
}
