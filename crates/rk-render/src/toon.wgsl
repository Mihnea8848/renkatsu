// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

// Koikatsu reference shading. Colour maths stays in gamma space, as the
// game's own shaders work. One module serves every material kind; the kind
// is a uniform so the pipeline count stays small.

struct Globals {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    eye: vec4<f32>,
    // rgb: scene shadow colour, a: 1 - shadow depth
    ambient_shadow: vec4<f32>,
    // rgb unused by the game, a: 1 - line depth
    line_color: vec4<f32>,
    // x: line width, y: light intensity, z: ambient light
    params: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var ramp_tex: texture_2d<f32>;
@group(0) @binding(2) var ramp_samp: sampler;

const KIND_ITEM: u32 = 1u;
const KIND_SKIN: u32 = 2u;
const KIND_HAIR: u32 = 3u;
const KIND_EYE: u32 = 4u;
const KIND_OVERLAY: u32 = 5u;

const HAS_MAIN: u32 = 1u;
const HAS_COLOR_MASK: u32 = 2u;
const HAS_DETAIL: u32 = 4u;
const HAS_LINE: u32 = 8u;
const HAS_ALPHA_MASK: u32 = 16u;
const HAS_OVER1: u32 = 32u;
const HAS_OVER2: u32 = 64u;
const BLENDED: u32 = 128u;

struct Material {
    color1: vec4<f32>,
    color2: vec4<f32>,
    color3: vec4<f32>,
    shadow: vec4<f32>,
    over1: vec4<f32>,
    over2: vec4<f32>,
    // xy: main uv scale, zw: offset
    st_main: vec4<f32>,
    // cutoff, shadow extend, shadow extend another, detail R as line R
    p0: vec4<f32>,
    // detail B as line G, outline width scale, alpha, unused
    p1: vec4<f32>,
    // body mask weights a and b, unused
    p2: vec4<f32>,
    // x: kind, y: flags
    info: vec4<u32>,
};
@group(1) @binding(0) var<uniform> m: Material;
@group(1) @binding(1) var main_tex: texture_2d<f32>;
@group(1) @binding(2) var color_mask_tex: texture_2d<f32>;
@group(1) @binding(3) var detail_tex: texture_2d<f32>;
@group(1) @binding(4) var line_tex: texture_2d<f32>;
@group(1) @binding(5) var alpha_mask_tex: texture_2d<f32>;
@group(1) @binding(6) var over1_tex: texture_2d<f32>;
@group(1) @binding(7) var over2_tex: texture_2d<f32>;
@group(1) @binding(8) var samp: sampler;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv0: vec2<f32>,
    @location(3) uv1: vec2<f32>,
    @location(4) uv2: vec2<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv0: vec2<f32>,
    @location(2) uv1: vec2<f32>,
    @location(3) uv2: vec2<f32>,
    @location(4) world: vec3<f32>,
};

fn has(flag: u32) -> bool {
    return (m.info.y & flag) != 0u;
}

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = g.view_proj * vec4<f32>(v.pos, 1.0);
    o.normal = v.normal;
    o.uv0 = v.uv0;
    o.uv1 = v.uv1;
    o.uv2 = v.uv2;
    o.world = v.pos;
    return o;
}

// Inverted hull: push each vertex out along its normal. The width grows
// with distance so lines keep a steady thickness on screen, and the detail
// mask's blue channel thins it where artists painted lines out.
@vertex
fn vs_outline(v: VsIn) -> VsOut {
    var o: VsOut;
    let dist = length(g.eye.xyz - v.pos);
    var width = (dist * 0.0999999866 + 0.300000012) * g.params.x * 0.00499999989 * m.p1.y;
    if (has(HAS_DETAIL)) {
        width *= 1.0 - textureSampleLevel(detail_tex, samp, v.uv0, 0.0).b;
    }
    let pos = v.pos + normalize(v.normal) * width;
    o.clip = g.view_proj * vec4<f32>(pos, 1.0);
    o.normal = v.normal;
    o.uv0 = v.uv0;
    o.uv1 = v.uv1;
    o.uv2 = v.uv2;
    o.world = pos;
    return o;
}

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let k = vec4<f32>(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = mix(vec4<f32>(c.bg, k.wz), vec4<f32>(c.gb, k.xy), step(c.b, c.g));
    let q = mix(vec4<f32>(p.xyw, c.r), vec4<f32>(c.r, p.yzx), step(p.x, c.r));
    let d = q.x - min(q.w, q.y);
    return vec3<f32>(abs(q.z + (q.w - q.y) / (6.0 * d + 1e-10)), d / (q.x + 1e-10), q.x);
}

fn hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let p = abs(fract(c.xxx + vec3<f32>(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0);
    return c.z * mix(vec3<f32>(1.0), clamp(p - 1.0, vec3<f32>(0.0), vec3<f32>(1.0)), c.y);
}

// The game's shade colour: the hue of the surface tinted by the material's
// shadow colour, at half saturation and full brightness.
fn shade_tint(diffuse: vec3<f32>) -> vec3<f32> {
    let hsv = rgb_to_hsv(diffuse * m.shadow.rgb);
    return hsv_to_rgb(vec3<f32>(hsv.x, hsv.y * 0.5, 1.0));
}

// Scene shadow colour and depth folded into one factor.
fn scene_shadow() -> vec3<f32> {
    let a = g.ambient_shadow.a;
    let rgb = g.ambient_shadow.rgb;
    let deep = 1.0 - (1.0 - a) * (1.0 - rgb);
    let shallow = (a + 1.0) * rgb;
    return clamp(select(shallow, deep, a > 0.0), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Multiplier applied to the unlit side of a surface.
fn shadow_factor(diffuse: vec3<f32>) -> vec3<f32> {
    let tint = shade_tint(diffuse);
    let scene = scene_shadow();
    let dark = tint * scene * 1.79999995;
    let light = 1.0 - (1.0 - 2.0 * (tint * 0.899999976 - 0.5)) * (1.0 - scene);
    return clamp(select(dark, light, tint > vec3<f32>(0.555555582)), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Colour of drawn lines and outlines: the shaded surface pulled toward
// black or white by the scene's line depth.
fn line_color(shaded_diffuse: vec3<f32>, light: vec3<f32>) -> vec3<f32> {
    let w = g.line_color.a;
    let toward_white = 1.0 - (1.0 - 2.0 * (w - 0.5)) * (1.0 - shaded_diffuse * 0.5);
    let toward_black = shaded_diffuse * 0.5 * (w * 2.0);
    return clamp(select(toward_black, toward_white, w > 0.5), vec3<f32>(0.0), vec3<f32>(1.0)) * light;
}

struct Surface {
    diffuse: vec3<f32>,
    alpha: f32,
};

fn read_surface(i: VsOut) -> Surface {
    var s: Surface;
    let uv = i.uv0 * m.st_main.xy + m.st_main.zw;
    var texel = vec4<f32>(1.0);
    if (has(HAS_MAIN)) {
        texel = textureSample(main_tex, samp, uv);
    }
    let kind = m.info.x;
    var tint = vec3<f32>(1.0);
    if (has(HAS_COLOR_MASK)) {
        let mask = textureSample(color_mask_tex, samp, i.uv0);
        tint = mix(tint, m.color1.rgb, mask.r);
        tint = mix(tint, m.color2.rgb, mask.g);
        tint = mix(tint, m.color3.rgb, mask.b);
    } else if (kind == KIND_HAIR || !has(HAS_MAIN)) {
        tint = m.color1.rgb;
    }
    s.diffuse = texel.rgb * tint;

    var mask_alpha = 1.0;
    if (has(HAS_ALPHA_MASK)) {
        let am = textureSample(alpha_mask_tex, samp, i.uv0);
        if (kind == KIND_HAIR) {
            mask_alpha = am.r;
        } else {
            // Body masks from clothes: each channel can be switched off.
            mask_alpha = min(max(1.0 - m.p2.x, am.r), max(1.0 - m.p2.y, am.g));
        }
    }
    s.alpha = min(mask_alpha, texel.a);
    return s;
}

fn light_color() -> vec3<f32> {
    return vec3<f32>(g.params.y * 0.600000024 + g.params.z);
}

@fragment
fn fs_main(i: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let kind = m.info.x;

    if (kind == KIND_OVERLAY) {
        let t = textureSample(main_tex, samp, i.uv0 * m.st_main.xy + m.st_main.zw);
        if (t.a < m.p0.x) {
            discard;
        }
        return vec4<f32>(t.rgb * m.color1.rgb, t.a * m.p1.z);
    }

    if (kind == KIND_EYE) {
        let iris = textureSample(main_tex, samp, i.uv0 * m.st_main.xy + m.st_main.zw);
        var over = vec4<f32>(0.0);
        if (has(HAS_OVER1)) {
            over = textureSample(over1_tex, samp, i.uv1).a * m.over1;
        }
        if (has(HAS_OVER2)) {
            over = max(over, textureSample(over2_tex, samp, i.uv2).a * m.over2);
        }
        let rgb = mix(iris.rgb, over.rgb, over.a);
        return vec4<f32>(rgb, max(over.a, iris.a));
    }

    let s = read_surface(i);
    let blended = has(BLENDED);
    if (s.alpha < m.p0.x) {
        discard;
    }

    var n = normalize(i.normal);
    if (!front) {
        n = -n;
    }
    let lambert = clamp(dot(n, normalize(g.light_dir.xyz)), 0.0, 1.0);
    var ramp = textureSample(ramp_tex, ramp_samp, vec2<f32>(lambert, 0.5)).r;

    var detail = vec4<f32>(0.0);
    if (has(HAS_DETAIL)) {
        detail = textureSample(detail_tex, samp, i.uv0);
    }
    var line = vec4<f32>(0.0);
    if (has(HAS_LINE)) {
        line = textureSample(line_tex, samp, i.uv0);
    }
    line.r = mix(line.r, detail.r, m.p0.w);

    // Shadows painted into the masks, softened by the shadow-extend sliders.
    let extend = m.p0.y * -1.20000005 + 1.0;
    var drawn_shadow = min(1.0 - line.b, 1.0 - detail.g);
    drawn_shadow = drawn_shadow * (1.0 - extend) + extend;
    ramp *= drawn_shadow;

    let factor = shadow_factor(s.diffuse);
    let shade = clamp(max(1.0 - line.r, 1.0 - m.p0.z) * factor, vec3<f32>(0.0), vec3<f32>(1.0));
    let light = light_color();
    let lit = mix(s.diffuse * shade, s.diffuse, ramp) * max(light, g.ambient_shadow.rgb);

    // Lines painted into the masks.
    let lines = line_color(s.diffuse * shade, light);
    let drawn_lines = mix(line.g, detail.b, m.p1.x);
    let keep = pow((1.0 - g.params.x) * 0.889999986 + 0.00999999978, drawn_lines);
    let body = lit * (drawn_shadow * (1.0 - shade) + shade);
    let rgb = clamp(mix(lines, body, keep), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(rgb, select(1.0, s.alpha * m.p1.z, blended));
}

@fragment
fn fs_outline(i: VsOut) -> @location(0) vec4<f32> {
    let s = read_surface(i);
    if (s.alpha < max(m.p0.x, 0.5)) {
        discard;
    }
    let shade = shadow_factor(s.diffuse);
    return vec4<f32>(line_color(s.diffuse * shade, light_color()), 1.0);
}
