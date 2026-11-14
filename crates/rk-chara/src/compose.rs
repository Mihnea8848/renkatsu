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

/// sRGB to linear for one channel. The game renders in linear colour space,
/// so Unity hands colours and sRGB textures to shaders this way.
fn to_linear(x: f32) -> f32 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear to sRGB for one channel: how an sRGB render target stores a value.
fn to_srgb(x: f32) -> f32 {
    if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

/// The colour channels of a sample as the shader sees them: decoded when
/// the texture is marked sRGB, and never above one.
fn linear_rgb(img: &Image, c: Rgba) -> [f32; 3] {
    let f = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        if img.srgb {
            to_linear(x)
        } else {
            x
        }
    };
    [f(c[0]), f(c[1]), f(c[2])]
}

/// A sample as the shader sees it: `sample` with the colour channels decoded.
fn sample_linear(img: &Image, u: f32, v: f32) -> Rgba {
    let t = sample(img, u, v);
    let c = linear_rgb(img, t);
    [c[0], c[1], c[2], t[3]]
}

/// A card colour as the shader sees it. The create shaders declare their
/// colours as Color properties, which Unity decodes in linear colour space.
fn linear_color(c: Rgba) -> [f32; 3] {
    rgb(c).map(to_linear)
}

/// What an sRGB render target stores for a linear colour.
fn stored(c: [f32; 3]) -> Rgba {
    [
        to_srgb(c[0].clamp(0.0, 1.0)),
        to_srgb(c[1].clamp(0.0, 1.0)),
        to_srgb(c[2].clamp(0.0, 1.0)),
        1.0,
    ]
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

    /// `base` and the result are linear values.
    fn apply(&self, base: [f32; 3], uv: [f32; 2], strength: f32) -> [f32; 3] {
        let Some(img) = self.image else { return base };
        let p = sample_linear(img, uv[0], uv[1]);
        mix3(
            base,
            mul3(rgb(p), linear_color(self.color)),
            p[3] * self.color[3] * strength,
        )
    }
}

fn render(width: u32, height: u32, name: &str, pixel: impl Fn([f32; 2]) -> Rgba + Sync) -> Image {
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    rgba.par_chunks_mut((width * 4) as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let v = (y as f32 + 0.5) / height as f32;
            for (x, px) in row.chunks_exact_mut(4).enumerate() {
                let c = pixel([(x as f32 + 0.5) / width as f32, v]);
                for k in 0..4 {
                    px[k] = (c[k].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
        });
    Image {
        name: name.to_string(),
        width,
        height,
        rgba,
        srgb: true,
        repeat: false,
    }
}

/// Paints `over` onto `base` by its alpha. The result is as large as the
/// larger of the two, so a detailed overlay keeps its detail.
pub fn blend(base: &Image, over: &Image) -> Image {
    let mut out = render(
        base.width.max(over.width),
        base.height.max(over.height),
        &base.name,
        |uv| {
            let b = sample(base, uv[0], uv[1]);
            let o = sample(over, uv[0], uv[1]);
            let c = mix3(rgb(b), rgb(o), o[3]);
            [c[0], c[1], c[2], o[3] + b[3] * (1.0 - o[3])]
        },
    );
    out.repeat = base.repeat;
    out
}

/// A fully transparent image, the starting point when an overlay replaces
/// a texture.
pub fn clear(width: u32, height: u32, name: &str) -> Image {
    Image {
        name: name.to_string(),
        width,
        height,
        rgba: vec![0; (width * height * 4) as usize],
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
///
/// The game draws this (shader create_body) on linear values into an sRGB
/// texture: `CustomTextureCreate` turns `GL.sRGBWrite` on for the blit, the
/// colours are Color properties and the textures are decoded when sampled.
pub fn body(i: &BodyInputs) -> Image {
    render(i.size, i.size, "body_composed", |uv| {
        // Sunburn replaces the skin colour where its texture is dark.
        let sun = i.sunburn.map(|t| sample_linear(t, uv[0], uv[1])[0]).unwrap_or(1.0);
        let skin = mix3(
            linear_color(i.sunburn_color),
            linear_color(i.skin_main),
            sun.max(1.0 - i.sunburn_color[3]),
        );
        let main = i.main.map(|t| sample_linear(t, uv[0], uv[1])).unwrap_or([1.0; 4]);
        let mask = i.color_mask.map(|t| sample_linear(t, uv[0], uv[1])).unwrap_or([0.0; 4]);
        // Green marks the areas tinted by the secondary skin colour.
        let tint = mix3([1.0; 3], linear_color(i.skin_sub), mask[1] * i.skin_sub[3]);
        let mut base = mul3(skin, mul3(rgb(main), tint));
        let paint_mask = i.paint_mask.map(|t| sample_linear(t, uv[0], uv[1])[0]).unwrap_or(1.0);
        for p in &i.paints {
            base = p.apply(base, paint_uv(uv, p.layout, true), paint_mask);
        }
        // Blue marks the nails.
        stored(mix3(base, mul3(linear_color(i.nail), base), mask[2]))
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

/// Face skin texture (shader create_head), on linear values like `body`.
pub fn face(i: &FaceInputs) -> Image {
    render(i.size, i.size, "face_composed", |uv| {
        let mask = i.color_mask.map(|t| sample_linear(t, uv[0], uv[1])).unwrap_or([0.0; 4]);
        // Red takes the skin colour, green the secondary colour, and blue
        // areas stay untinted.
        let mut tint = mul3(
            mix3([1.0; 3], linear_color(i.skin_main), mask[0]),
            mix3([1.0; 3], linear_color(i.skin_sub), mask[1]),
        );
        for t in tint.iter_mut() {
            *t = t.max(mask[2]);
        }
        let main = i.main.map(|t| sample_linear(t, uv[0], uv[1])).unwrap_or([1.0; 4]);
        let mut base = mul3(tint, rgb(main));
        base = i.cheek.apply(base, offset_uv(uv, i.cheek.layout), 1.0);
        base = i.lip_line.apply(base, offset_uv(uv, i.lip_line.layout), 1.0);
        let paint_mask = i.paint_mask.map(|t| sample_linear(t, uv[0], uv[1])[0]).unwrap_or(1.0);
        for p in &i.paints {
            base = p.apply(base, paint_uv(uv, p.layout, true), paint_mask);
        }
        stored(i.mole.apply(base, paint_uv(uv, i.mole.layout, false), 1.0))
    })
}

/// One of the three colour areas of a clothing piece.
#[derive(Clone, Copy)]
pub struct ClothColor<'a> {
    pub base: Rgba,
    pub pattern: Option<&'a Image>,
    pub pattern_color: Rgba,
    /// 0 to 1 per axis: how often the pattern repeats.
    pub tiling: [f32; 2],
}

impl ClothColor<'_> {
    /// The area's colour at a point, in linear values.
    fn at(&self, uv: [f32; 2]) -> [f32; 3] {
        // The pattern colour shows where the pattern texture is dark.
        let pat = match self.pattern {
            Some(p) => {
                let t = sample(
                    p,
                    uv[0] * (20.0 - 19.0 * self.tiling[0]),
                    uv[1] * (20.0 - 19.0 * self.tiling[1]),
                );
                linear_rgb(p, t)[0]
            }
            None => 1.0,
        };
        mix3(
            rgb(self.pattern_color).map(to_linear),
            rgb(self.base).map(to_linear),
            pat.max(1.0 - self.pattern_color[3]),
        )
    }
}

/// Clothing texture: the base texture tinted by up to three colours, each
/// with an optional pattern, placed by the colour mask's channels.
///
/// The game draws this (shader create_topN) on linear values into a cleared
/// sRGB texture, blending with the base texture's alpha. So the colour comes
/// out multiplied by that alpha and the stored alpha is its square.
pub fn clothes(main: &Image, mask: &Image, colors: &[ClothColor; 3]) -> Image {
    render(main.width, main.height, "clothes_composed", |uv| {
        let m = sample(mask, uv[0], uv[1]);
        let mut tint = [1.0; 3];
        for (k, c) in colors.iter().enumerate() {
            tint = mix3(tint, c.at(uv), m[k]);
        }
        let t = sample(main, uv[0], uv[1]);
        let a = t[3].clamp(0.0, 1.0);
        let c = mul3(tint, linear_rgb(main, t)).map(|x| to_srgb(x * a));
        [c[0], c[1], c[2], a * a]
    })
}

pub struct EyeInputs<'a> {
    pub size: u32,
    /// Iris drawing: brightness in red, shape in alpha.
    pub main: Option<&'a Image>,
    /// Gradient between the two colours.
    pub gradient: Option<&'a Image>,
    pub base: Rgba,
    pub sub: Rgba,
    pub blend: f32,
    /// Card values, 0 to 1.
    pub gradient_offset_y: f32,
    pub gradient_scale: f32,
}

/// Iris texture: a two-colour gradient overlaid on the iris drawing.
///
/// Shader create_eye, on linear values into an sRGB texture like `body`.
/// The offset and scale go through `Mathf.Lerp` in
/// `ChaControl.ChangeSettingEye`, which clamps them to 0..1; the blend is
/// passed as it is and can be above one.
pub fn eye(i: &EyeInputs) -> Image {
    let y = -0.5 + i.gradient_offset_y.clamp(0.0, 1.0);
    let s = 1.0 - (-1.0 + 2.0 * i.gradient_scale.clamp(0.0, 1.0));
    let (base, sub) = (linear_color(i.base), linear_color(i.sub));
    render(i.size, i.size, "eye_composed", |uv| {
        let g = match i.gradient {
            Some(t) => sample_linear(t, uv[0] * (2.0 * s) + (0.5 - s), (uv[1] + y) * (2.0 * s) + (0.5 - s))[0],
            None => 1.0,
        };
        let col = mix3(sub, base, g);
        let t = i.main.map(|t| sample_linear(t, uv[0], uv[1])).unwrap_or([1.0; 4]);
        let m = t[0];
        let mut out = [0.0; 3];
        for k in 0..3 {
            // Overlay blend of the colour with the drawing's brightness.
            // A division by zero gives an infinity that the shader's
            // saturate brings back into range (NaN becomes 0).
            let over = if m > 0.5 {
                col[k] / (2.0 * (1.0 - m))
            } else {
                1.0 - (1.0 - col[k]) * 0.5 / m
            };
            let over = if over.is_nan() { 0.0 } else { over.clamp(0.0, 1.0) };
            out[k] = col[k] * m + i.blend * (over - m * col[k]);
        }
        let mut out = stored(out);
        out[3] = t[3] * i.base[3];
        out
    })
}

/// Eye white (shader create_eyewhite): the drawing's red channel blends
/// between two colours, on linear values like `body`.
pub fn eye_white(size: u32, main: Option<&Image>, base: Rgba, sub: Rgba) -> Image {
    render(size, size, "eyewhite_composed", |uv| {
        let m = main.map(|t| sample_linear(t, uv[0], uv[1])[0]).unwrap_or(1.0);
        stored(mix3(linear_color(sub), linear_color(base), m))
    })
}

/// Which entries of a part's `colorInfo` feed the three colour slots of a
/// clothing piece, for a main part (`None`) or a sub-part of a top.
///
/// `ChaControl.ChangeCustomClothes`: the third sub-part (`!main && kind ==
/// 2`) takes the base colour, pattern colour and tiling of its first slot
/// from `colorInfo[3]`, and its pattern loop runs to four, so
/// `_PatternMask1` ends up as the pattern of `colorInfo[3]` too. Every
/// other piece uses entries 0, 1 and 2.
pub fn color_sources(sub_part: Option<usize>) -> [usize; 3] {
    match sub_part {
        Some(2) => [3, 1, 2],
        _ => [0, 1, 2],
    }
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
        // The product is taken on linear values: 0.8 is 0.6038 and 128 of
        // 255 is 0.2158, their product 0.1303 is stored as 101. A product of
        // the stored values would give 102, and 77 instead of 74 for blue.
        assert_eq!((p[0], p[1], p[2], p[3]), (128, 101, 74, 255));
    }

    #[test]
    fn body_masks_pick_secondary_and_nail_colours() {
        let green = solid([0, 255, 0, 255]);
        let blue = solid([0, 0, 255, 255]);
        let mut i = body_inputs();
        i.color_mask = Some(&green);
        assert_eq!(px(&body(&i), 0, 0)[..3], [255, 101, 74]);
        i.color_mask = Some(&blue);
        // Red is the nail's 0.5 exactly, which is 127.5 of 255.
        let p = px(&body(&i), 0, 0);
        assert!((127..=128).contains(&p[0]) && p[1] == 101 && p[2] == 153, "{p:?}");
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
        // Half-transparent red over the skin colour, mixed on linear values:
        // green 0.6038 times (1 - 128/255) is 0.3007, stored as 149.
        assert!(
            (p[0] as i32 - 255).abs() <= 1 && (p[1] as i32 - 149).abs() <= 1 && (p[2] as i32 - 111).abs() <= 1,
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
    fn face_layers_blend_on_linear_values() {
        // A half-transparent red cheek over an untinted white face. The
        // shader mixes linear values: green and blue become 1 - 128/255 =
        // 0.498, stored as 187. Mixing the stored values would give 127.
        let red = solid([255, 0, 0, 128]);
        let i = FaceInputs {
            size: 2,
            main: None,
            color_mask: None,
            skin_main: [1.0, 0.8, 0.6, 1.0],
            skin_sub: [1.0, 1.0, 1.0, 1.0],
            cheek: Decal {
                image: Some(&red),
                color: [1.0, 1.0, 1.0, 1.0],
                layout: [0.0; 4],
            },
            lip_line: Decal::none(),
            paint_mask: None,
            paints: [Decal::none(), Decal::none()],
            mole: Decal::none(),
        };
        let p = px(&face(&i), 0, 0);
        assert!(
            p[0] == 255 && (p[1] as i32 - 187).abs() <= 1 && (p[2] as i32 - 187).abs() <= 1,
            "{p:?}"
        );
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

    #[test]
    fn clothes_colours_follow_the_mask_and_blend_by_alpha() {
        let main = solid([128, 255, 255, 64]);
        let red_mask = solid([255, 0, 0, 255]);
        let plain = ClothColor {
            base: [1.0, 0.6, 0.0, 1.0],
            pattern: None,
            pattern_color: [0.0, 0.0, 1.0, 1.0],
            tiling: [0.0; 2],
        };
        let unused = ClothColor {
            base: [0.0; 4],
            pattern: None,
            pattern_color: [0.0; 4],
            tiling: [0.0; 2],
        };
        let out = clothes(&main, &red_mask, &[plain, unused, unused]);
        assert_eq!((out.width, out.height), (2, 2));
        // Alpha 64 of 255 is about a quarter. Red: 128 of 255 is 0.2158 in
        // linear, times the alpha 0.0542, stored as sRGB 65.8. Green: 0.6 is
        // 0.3185 in linear, times the alpha 0.0799, stored as 79.9.
        assert_eq!(px(&out, 0, 0), [66, 80, 0, 16]);
        // A solid base texture keeps colour and alpha.
        let solid_main = solid([128, 255, 255, 255]);
        assert_eq!(
            px(&clothes(&solid_main, &red_mask, &[plain, unused, unused]), 0, 0),
            [128, 153, 0, 255]
        );

        // A dark pattern texture switches the area to the pattern colour.
        let dark = solid([0, 0, 0, 255]);
        let patterned = ClothColor {
            pattern: Some(&dark),
            ..plain
        };
        assert_eq!(
            px(&clothes(&solid_main, &red_mask, &[patterned, unused, unused]), 0, 0)[..3],
            [0, 0, 255]
        );
        // With no mask coverage the base texture is left as it is.
        let none = solid([0, 0, 0, 255]);
        assert_eq!(
            px(&clothes(&solid_main, &none, &[plain, unused, unused]), 0, 0)[..3],
            [128, 255, 255]
        );
    }

    #[test]
    fn eye_gradient_and_overlay() {
        let mut i = EyeInputs {
            size: 2,
            main: None,
            gradient: None,
            base: [0.2, 0.4, 0.8, 1.0],
            sub: [1.0, 0.0, 0.0, 1.0],
            blend: 0.0,
            gradient_offset_y: 0.5,
            gradient_scale: 0.5,
        };
        // White drawing, no gradient texture: the base colour.
        assert_eq!(px(&eye(&i), 0, 0), [51, 102, 204, 255]);
        let dark = solid([0, 0, 0, 255]);
        i.gradient = Some(&dark);
        assert_eq!(
            px(&eye(&i), 0, 0)[..3],
            [255, 0, 0],
            "dark gradient picks the second colour"
        );
        // Grey drawing: plain multiply without blend, on linear values.
        // 200 of 255 is 0.5776 and 0.8 is 0.6038; the product is stored as 159.
        let grey = solid([200, 200, 200, 128]);
        i.gradient = None;
        i.main = Some(&grey);
        let p = px(&eye(&i), 0, 0);
        assert!(
            (p[0] as i32 - 38).abs() <= 1
                && (p[1] as i32 - 78).abs() <= 1
                && (p[2] as i32 - 159).abs() <= 1
                && (p[3] as i32 - 128).abs() <= 1,
            "{p:?}"
        );
        // Blend 1: the overlay colour/(2(1-m)) replaces the product.
        i.blend = 1.0;
        let p = px(&eye(&i), 0, 0);
        assert!(
            (p[0] as i32 - 56).abs() <= 1 && (p[1] as i32 - 110).abs() <= 1 && (p[2] as i32 - 220).abs() <= 1,
            "{p:?}"
        );
        // The game passes the blend unclamped, so mods can go above one.
        i.blend = 2.0;
        let p = px(&eye(&i), 0, 0);
        assert!(
            (p[0] as i32 - 69).abs() <= 1 && (p[1] as i32 - 134).abs() <= 1 && p[2] == 255,
            "{p:?}"
        );
        // A dark drawing takes the other branch, 1 - (1-colour)/(2m), which
        // darkens: 128 of 255 gives 81 for blue instead of the product's 101.
        let dark_grey = solid([128, 128, 128, 255]);
        i.main = Some(&dark_grey);
        i.blend = 1.0;
        assert!((px(&eye(&i), 0, 0)[2] as i32 - 81).abs() <= 1);
    }

    #[test]
    fn eye_gradient_placement_is_clamped_like_mathf_lerp() {
        // Gradient: dark lower half, bright upper half.
        let gradient = Image {
            name: "g".into(),
            width: 1,
            height: 2,
            rgba: vec![0, 0, 0, 255, 255, 255, 255, 255],
            srgb: true,
            repeat: false,
        };
        let inputs = |offset_y: f32, scale: f32| EyeInputs {
            size: 8,
            main: None,
            gradient: Some(&gradient),
            base: [1.0, 1.0, 1.0, 1.0],
            sub: [0.0, 0.0, 0.0, 1.0],
            blend: 0.0,
            gradient_offset_y: offset_y,
            gradient_scale: scale,
        };
        // Values outside 0..1 (slider unlocking mods) act as the nearest end.
        assert_eq!(eye(&inputs(0.5, -0.2)).rgba, eye(&inputs(0.5, 0.0)).rgba);
        assert_eq!(eye(&inputs(0.5, 1.3)).rgba, eye(&inputs(0.5, 1.0)).rgba);
        assert_eq!(eye(&inputs(-0.4, 0.5)).rgba, eye(&inputs(0.0, 0.5)).rgba);
        assert_eq!(eye(&inputs(1.7, 0.5)).rgba, eye(&inputs(1.0, 0.5)).rgba);
        // And the ends differ from the middle, so the test means something.
        assert_ne!(eye(&inputs(0.5, 0.0)).rgba, eye(&inputs(0.5, 0.5)).rgba);
        assert_ne!(eye(&inputs(0.0, 0.5)).rgba, eye(&inputs(0.5, 0.5)).rgba);
    }

    #[test]
    fn eye_white_blends_two_colours() {
        let half = solid([128, 0, 0, 255]);
        let out = eye_white(2, Some(&half), [1.0, 1.0, 1.0, 1.0], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(px(&out, 1, 1), [128, 128, 128, 255]);
        // The blend is on linear values: from 0.5 (0.2140) a share of 0.2158
        // towards white gives 0.3836, stored as 166. On stored values it
        // would be 191.
        let out = eye_white(2, Some(&half), [1.0, 1.0, 1.0, 1.0], [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(px(&out, 1, 1), [166, 166, 166, 255]);
    }

    #[test]
    fn overlays_blend_by_alpha_at_the_larger_size() {
        let base = solid([0, 0, 255, 255]);
        let mut over = Image {
            name: "o".into(),
            width: 4,
            height: 4,
            rgba: [255, 0, 0, 128].repeat(16),
            srgb: true,
            repeat: false,
        };
        let out = blend(&base, &over);
        assert_eq!((out.width, out.height), (4, 4));
        let p = px(&out, 1, 1);
        assert!(
            (p[0] as i32 - 128).abs() <= 1 && p[1] == 0 && (p[2] as i32 - 127).abs() <= 1 && p[3] == 255,
            "{p:?}"
        );
        // A transparent overlay leaves the base alone.
        over.rgba = [255, 0, 0, 0].repeat(16);
        assert_eq!(px(&blend(&base, &over), 2, 2), [0, 0, 255, 255]);
        // Over a cleared texture only the overlay remains.
        let empty = clear(2, 2, "c");
        over.rgba = [10, 20, 30, 255].repeat(16);
        assert_eq!(px(&blend(&empty, &over), 0, 0), [10, 20, 30, 255]);
    }
}
