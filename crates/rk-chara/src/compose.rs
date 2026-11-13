// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builds the textures the game creates at load time: skin and face are
//! not stored as finished images but composed from a base texture, masks,
//! colours from the card and optional layers (paint, sunburn, cheeks...).
//!
//! The formulas follow the game's own texture-creation shaders. Images are
//! bottom-up, and so are the UV coordinates used here.

use crate::scene::Image;
use rayon::prelude::*;

pub type Rgba = [f32; 4];

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn mul3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

fn rgb(c: Rgba) -> [f32; 3] {
    [c[0], c[1], c[2]]
}

/// Bilinear sample at a UV coordinate, wrapping or clamping as the image asks.
pub fn sample(img: &Image, u: f32, v: f32) -> Rgba {
    let (w, h) = (img.width as i64, img.height as i64);
    if w == 0 || h == 0 {
        return [0.0; 4];
    }
    let x = u * w as f32 - 0.5;
    let y = v * h as f32 - 0.5;
    let (x0, y0) = (x.floor(), y.floor());
    let (tx, ty) = (x - x0, y - y0);
    let fetch = |xi: i64, yi: i64| -> Rgba {
        let (xi, yi) = if img.repeat {
            (xi.rem_euclid(w), yi.rem_euclid(h))
        } else {
            (xi.clamp(0, w - 1), yi.clamp(0, h - 1))
        };
        let o = ((yi * w + xi) * 4) as usize;
        let p = &img.rgba[o..o + 4];
        [
            p[0] as f32 / 255.0,
            p[1] as f32 / 255.0,
            p[2] as f32 / 255.0,
            p[3] as f32 / 255.0,
        ]
    };
    let (a, b, c, d) = (
        fetch(x0 as i64, y0 as i64),
        fetch(x0 as i64 + 1, y0 as i64),
        fetch(x0 as i64, y0 as i64 + 1),
        fetch(x0 as i64 + 1, y0 as i64 + 1),
    );
    let mut out = [0.0; 4];
    for k in 0..4 {
        let top = a[k] + (b[k] - a[k]) * tx;
        let bot = c[k] + (d[k] - c[k]) * tx;
        out[k] = top + (bot - top) * ty;
    }
    out
}

/// Placement of a paint layer: xy offset, z rotation in turns, w size.
fn paint_uv(uv: [f32; 2], layout: Rgba, rotate: bool) -> [f32; 2] {
    let s = 2.0 - 2.0 * layout[3];
    let mut u = (uv[0] + layout[0]) * (2.0 * s) + (0.5 - s);
    let mut v = (uv[1] + layout[1]) * (2.0 * s) + (0.5 - s);
    if rotate {
        u -= 0.5;
        v -= 0.5;
        let (sin, cos) = (layout[2] * std::f32::consts::TAU).sin_cos();
        (u, v) = (u * cos + v * sin + 0.5, -u * sin + v * cos + 0.5);
    }
    [u, v]
}

/// Placement used by cheek and lip line: xy offset, zw size.
fn offset_uv(uv: [f32; 2], t: Rgba) -> [f32; 2] {
    [
        (uv[0] + t[0] - 1.0) * (1.0 - t[2]) + 0.5,
        (uv[1] + t[1] - 1.0) * (1.0 - t[3]) + 0.5,
    ]
}

/// A tinted texture blended over the base by its alpha.
#[derive(Clone, Copy)]
pub struct Decal<'a> {
    pub image: Option<&'a Image>,
    pub color: Rgba,
    pub layout: Rgba,
}

impl<'a> Decal<'a> {
    pub fn none() -> Self {
        Self {
            image: None,
            color: [1.0; 4],
            layout: [0.0; 4],
        }
    }

    fn apply(&self, base: [f32; 3], uv: [f32; 2], strength: f32) -> [f32; 3] {
        let Some(img) = self.image else { return base };
        let p = sample(img, uv[0], uv[1]);
        mix3(base, mul3(rgb(p), rgb(self.color)), p[3] * self.color[3] * strength)
    }
}

fn render(size: u32, name: &str, pixel: impl Fn([f32; 2]) -> [f32; 3] + Sync) -> Image {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    rgba.par_chunks_mut((size * 4) as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let v = (y as f32 + 0.5) / size as f32;
            for (x, px) in row.chunks_exact_mut(4).enumerate() {
                let c = pixel([(x as f32 + 0.5) / size as f32, v]);
                for k in 0..3 {
                    px[k] = (c[k].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
                px[3] = 255;
            }
        });
    Image {
        name: name.to_string(),
        width: size,
        height: size,
        rgba,
        srgb: true,
        repeat: false,
    }
}

pub struct BodyInputs<'a> {
    pub size: u32,
    pub main: Option<&'a Image>,
    pub color_mask: Option<&'a Image>,
    pub skin_main: Rgba,
    pub skin_sub: Rgba,
    pub nail: Rgba,
    pub sunburn: Option<&'a Image>,
    pub sunburn_color: Rgba,
    pub paint_mask: Option<&'a Image>,
    pub paints: [Decal<'a>; 2],
}

/// Body skin texture.
pub fn body(i: &BodyInputs) -> Image {
    render(i.size, "body_composed", |uv| {
        // Sunburn replaces the skin colour where its texture is dark.
        let sun = i.sunburn.map(|t| sample(t, uv[0], uv[1])[0]).unwrap_or(1.0);
        let skin = mix3(
            rgb(i.sunburn_color),
            rgb(i.skin_main),
            sun.max(1.0 - i.sunburn_color[3]),
        );
        let main = i.main.map(|t| sample(t, uv[0], uv[1])).unwrap_or([1.0; 4]);
        let mask = i.color_mask.map(|t| sample(t, uv[0], uv[1])).unwrap_or([0.0; 4]);
        // Green marks the areas tinted by the secondary skin colour.
        let tint = mix3([1.0; 3], rgb(i.skin_sub), mask[1] * i.skin_sub[3]);
        let mut base = mul3(skin, mul3(rgb(main), tint));
        let paint_mask = i.paint_mask.map(|t| sample(t, uv[0], uv[1])[0]).unwrap_or(1.0);
        for p in &i.paints {
            base = p.apply(base, paint_uv(uv, p.layout, true), paint_mask);
        }
        // Blue marks the nails.
        mix3(base, mul3(rgb(i.nail), base), mask[2])
    })
}

pub struct FaceInputs<'a> {
    pub size: u32,
    pub main: Option<&'a Image>,
    pub color_mask: Option<&'a Image>,
    pub skin_main: Rgba,
    pub skin_sub: Rgba,
    /// Cheek and lip line, each placed by its own offset and size.
    pub cheek: Decal<'a>,
    pub lip_line: Decal<'a>,
    pub paint_mask: Option<&'a Image>,
    pub paints: [Decal<'a>; 2],
    pub mole: Decal<'a>,
}

/// Face skin texture.
pub fn face(i: &FaceInputs) -> Image {
    render(i.size, "face_composed", |uv| {
        let mask = i.color_mask.map(|t| sample(t, uv[0], uv[1])).unwrap_or([0.0; 4]);
        // Red takes the skin colour, green the secondary colour, and blue
        // areas stay untinted.
        let mut tint = mul3(
            mix3([1.0; 3], rgb(i.skin_main), mask[0]),
            mix3([1.0; 3], rgb(i.skin_sub), mask[1]),
        );
        for t in tint.iter_mut() {
            *t = t.max(mask[2]);
        }
        let main = i.main.map(|t| sample(t, uv[0], uv[1])).unwrap_or([1.0; 4]);
        let mut base = mul3(tint, rgb(main));
        base = i.cheek.apply(base, offset_uv(uv, i.cheek.layout), 1.0);
        base = i.lip_line.apply(base, offset_uv(uv, i.lip_line.layout), 1.0);
        let paint_mask = i.paint_mask.map(|t| sample(t, uv[0], uv[1])[0]).unwrap_or(1.0);
        for p in &i.paints {
            base = p.apply(base, paint_uv(uv, p.layout, true), paint_mask);
        }
        i.mole.apply(base, paint_uv(uv, i.mole.layout, false), 1.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(c: [u8; 4]) -> Image {
        Image {
            name: "t".into(),
            width: 2,
            height: 2,
            rgba: c.repeat(4),
            srgb: true,
            repeat: true,
        }
    }

    fn px(img: &Image, x: u32, y: u32) -> [u8; 4] {
        let o = ((y * img.width + x) * 4) as usize;
        img.rgba[o..o + 4].try_into().unwrap()
    }

    fn body_inputs<'a>() -> BodyInputs<'a> {
        BodyInputs {
            size: 4,
            main: None,
            color_mask: None,
            skin_main: [1.0, 0.8, 0.6, 1.0],
            skin_sub: [1.0, 0.5, 0.5, 1.0],
            nail: [0.5, 0.5, 1.0, 1.0],
            sunburn: None,
            sunburn_color: [0.4, 0.2, 0.1, 1.0],
            paint_mask: None,
            paints: [Decal::none(), Decal::none()],
        }
    }

    #[test]
    fn sampling_wraps_or_clamps() {
        let mut img = Image {
            name: "t".into(),
            width: 2,
            height: 1,
            rgba: vec![0, 0, 0, 255, 255, 255, 255, 255],
            srgb: true,
            repeat: false,
        };
        assert_eq!(sample(&img, 0.25, 0.5)[0], 0.0);
        assert_eq!(sample(&img, 0.75, 0.5)[0], 1.0);
        assert!((sample(&img, 0.5, 0.5)[0] - 0.5).abs() < 1e-6);
        // Outside the image: clamped stays white, wrapped blends with black.
        assert_eq!(sample(&img, 1.2, 0.5)[0], 1.0);
        img.repeat = true;
        assert!((sample(&img, 1.0, 0.5)[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn body_is_skin_colour_times_base() {
        let main = solid([128, 128, 128, 255]);
        let mut i = body_inputs();
        i.main = Some(&main);
        let out = body(&i);
        let p = px(&out, 1, 1);
        assert_eq!((p[0], p[1], p[2], p[3]), (128, 102, 77, 255));
    }

    #[test]
    fn body_masks_pick_secondary_and_nail_colours() {
        let green = solid([0, 255, 0, 255]);
        let blue = solid([0, 0, 255, 255]);
        let mut i = body_inputs();
        i.color_mask = Some(&green);
        assert_eq!(px(&body(&i), 0, 0)[..3], [255, 102, 77]);
        i.color_mask = Some(&blue);
        assert_eq!(px(&body(&i), 0, 0)[..3], [128, 102, 153]);
    }

    #[test]
    fn sunburn_and_paint_layers() {
        let dark = solid([0, 0, 0, 255]);
        let mut i = body_inputs();
        i.sunburn = Some(&dark);
        assert_eq!(
            px(&body(&i), 0, 0)[..3],
            [102, 51, 26],
            "dark sunburn texture gives the sunburn colour"
        );
        i.sunburn_color[3] = 0.0;
        assert_eq!(
            px(&body(&i), 0, 0)[..3],
            [255, 204, 153],
            "zero strength leaves the skin colour"
        );

        let red = solid([255, 0, 0, 128]);
        let mut i = body_inputs();
        i.paints[0] = Decal {
            image: Some(&red),
            color: [1.0, 1.0, 1.0, 1.0],
            layout: [0.0, 0.0, 0.0, 0.5],
        };
        let p = px(&body(&i), 2, 2);
        // Half-transparent red over the skin colour.
        assert!(
            (p[0] as i32 - 255).abs() <= 1 && (p[1] as i32 - 102).abs() <= 1 && (p[2] as i32 - 76).abs() <= 2,
            "{p:?}"
        );
        let none = solid([255, 255, 255, 255]);
        let hidden = solid([0, 0, 0, 255]);
        i.paint_mask = Some(&hidden);
        assert_eq!(px(&body(&i), 2, 2)[..3], [255, 204, 153], "paint mask hides the paint");
        i.paint_mask = Some(&none);
        assert_ne!(px(&body(&i), 2, 2)[..3], [255, 204, 153]);
    }

    #[test]
    fn face_tint_and_untinted_areas() {
        let red = solid([255, 0, 0, 255]);
        let blue = solid([255, 0, 255, 255]);
        let mut i = FaceInputs {
            size: 2,
            main: None,
            color_mask: Some(&red),
            skin_main: [1.0, 0.8, 0.6, 1.0],
            skin_sub: [1.0, 1.0, 1.0, 1.0],
            cheek: Decal::none(),
            lip_line: Decal::none(),
            paint_mask: None,
            paints: [Decal::none(), Decal::none()],
            mole: Decal::none(),
        };
        assert_eq!(px(&face(&i), 0, 0)[..3], [255, 204, 153]);
        i.color_mask = Some(&blue);
        assert_eq!(px(&face(&i), 0, 0)[..3], [255, 255, 255], "blue keeps the area white");
    }

    #[test]
    fn layout_centre_and_size() {
        // Size 0.75 with no offset leaves the UVs unchanged.
        let uv = paint_uv([0.3, 0.7], [0.0, 0.0, 0.0, 0.75], true);
        assert!((uv[0] - 0.3).abs() < 1e-6 && (uv[1] - 0.7).abs() < 1e-6);
        // A quarter turn rotates around the centre.
        let uv = paint_uv([1.0, 0.5], [0.0, 0.0, 0.25, 0.75], true);
        assert!((uv[0] - 0.5).abs() < 1e-5 && (uv[1] - 0.0).abs() < 1e-5, "{uv:?}");
    }
}
