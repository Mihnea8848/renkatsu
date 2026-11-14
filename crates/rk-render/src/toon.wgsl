// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

// Koikatsu reference shading. One module serves every material kind; the kind
// is a uniform so the pipeline count stays small. The game renders in linear
// colour space: the target is sRGB, so blending happens on linear values as
// in the game. The clothes shaders (main_opaque, main_alpha) work on linear
// values throughout. The older branches still do their maths on gamma values
// and are converted on the way out.

struct Globals {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    eye: vec4<f32>,
    // rgb: scene shadow colour, a: 1 - shadow depth
    ambient_shadow: vec4<f32>,
    // rgb unused by the game, a: 1 - line depth
    line_color: vec4<f32>,
    // x: line width, y: light intensity, z: ambient light,
    // w: 1 when the ramp texture holds sRGB values
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
const HAS_OVER3: u32 = 256u;
const MAIN_SRGB: u32 = 512u;
// Clothes materials bind their normal map and second ramp in the first two
// layer slots.
const HAS_NORMAL: u32 = 32u;
const HAS_ANOTHER_RAMP: u32 = 64u;

// Which game shader an item follows (info.z).
const ITEM_OPAQUE: u32 = 1u;
const ITEM_ALPHA: u32 = 2u;

struct Material {
    color1: vec4<f32>,
    color2: vec4<f32>,
    color3: vec4<f32>,
    shadow: vec4<f32>,
    over1: vec4<f32>,
    over2: vec4<f32>,
    over3: vec4<f32>,
    // xy: main uv scale, zw: offset
    st_main: vec4<f32>,
    // the same for the alpha mask
    st_alpha: vec4<f32>,
    // cutoff, shadow extend, shadow extend another, detail R as line R
    p0: vec4<f32>,
    // detail B as line G, outline width scale, alpha, unused
    p1: vec4<f32>,
    // body mask weights a and b, unused
    p2: vec4<f32>,
    // x: 1 when overlay 1 is placed like a nipple, y: its size
    p3: vec4<f32>,
    // the material's own outline colour (hair)
    line: vec4<f32>,
    // x: kind, y: flags, z: item shader
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
@group(1) @binding(8) var over3_tex: texture_2d<f32>;
@group(1) @binding(9) var samp: sampler;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv0: vec2<f32>,
    @location(3) uv1: vec2<f32>,
    @location(4) uv2: vec2<f32>,
    @location(5) uv3: vec2<f32>,
    @location(6) color: vec4<f32>,
    @location(7) tangent: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv0: vec2<f32>,
    @location(2) uv1: vec2<f32>,
    @location(3) uv2: vec2<f32>,
    @location(4) world: vec3<f32>,
    @location(5) uv3: vec2<f32>,
    @location(6) color: vec4<f32>,
    @location(7) bitangent: vec3<f32>,
    @location(8) tangent: vec3<f32>,
};

fn has(flag: u32) -> bool {
    return (m.info.y & flag) != 0u;
}

// Across the surface, at right angles to the tangent (Unity's convention).
fn bitangent_of(v: VsIn) -> vec3<f32> {
    let b = cross(v.normal, v.tangent.xyz) * v.tangent.w;
    let len = length(b);
    return select(vec3<f32>(0.0, 1.0, 0.0), b / len, len > 1e-6);
}

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = g.view_proj * vec4<f32>(v.pos, 1.0);
    o.normal = v.normal;
    o.uv0 = v.uv0;
    o.uv1 = v.uv1;
    o.uv2 = v.uv2;
    o.uv3 = v.uv3;
    o.color = v.color;
    o.world = v.pos;
    o.bitangent = bitangent_of(v);
    o.tangent = v.tangent.xyz;
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
    // Hair lines thin out where the strand fades.
    if (m.info.x == KIND_HAIR) {
        if (has(HAS_MAIN)) {
            width *= textureSampleLevel(main_tex, samp, v.uv0 * m.st_main.xy + m.st_main.zw, 0.0).a;
        }
        if (has(HAS_ALPHA_MASK)) {
            width *= textureSampleLevel(alpha_mask_tex, samp, v.uv0 * m.st_alpha.xy + m.st_alpha.zw, 0.0).r;
        }
    }
    let pos = v.pos + normalize(v.normal) * width;
    o.clip = g.view_proj * vec4<f32>(pos, 1.0);
    o.normal = v.normal;
    o.uv0 = v.uv0;
    o.uv1 = v.uv1;
    o.uv2 = v.uv2;
    o.uv3 = v.uv3;
    o.color = v.color;
    o.world = pos;
    o.bitangent = bitangent_of(v);
    o.tangent = v.tangent.xyz;
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
    return shadow_factor_of(shade_tint(diffuse));
}

// The same for a shade tint that is already known.
fn shadow_factor_of(tint: vec3<f32>) -> vec3<f32> {
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
    // Skin has its colours baked into the main texture already.
    if (has(HAS_COLOR_MASK) && kind != KIND_SKIN) {
        let mask = textureSample(color_mask_tex, samp, i.uv0);
        tint = mix(tint, m.color1.rgb, mask.r);
        tint = mix(tint, m.color2.rgb, mask.g);
        tint = mix(tint, m.color3.rgb, mask.b);
    } else if (kind == KIND_HAIR || !has(HAS_MAIN)) {
        tint = m.color1.rgb;
    }
    s.diffuse = texel.rgb * tint;

    // Skin layers drawn over the base texture: lips, blush and eyeshadow on
    // the face; nipples and underhair on the body. Vertex colours scale the
    // UVs so each layer only lands where it belongs.
    if (kind == KIND_SKIN) {
        if (has(HAS_OVER1)) {
            let plain = i.uv1 * i.color.r;
            let ring = clamp(length(i.uv1 - 0.5) * 16.6666698 - 1.0, 0.0, 1.0);
            let scale = m.p3.y * vec2<f32>(-1.39999998, 0.699999988) + vec2<f32>(2.0, -0.5);
            let grown = (ring * (i.uv1 * scale.x + scale.y - i.uv1) + i.uv1) * i.color.r;
            let t = textureSample(over1_tex, samp, mix(plain, grown, m.p3.x));
            // With the mask switch on, red is the layer's strength and green a shine.
            let flat = t.rgb * m.over1.rgb;
            let shiny = t.r * (m.over1.rgb + t.g * m.line.z * 0.330000013);
            let layer = mix(flat, shiny, clamp(m.line.w, 0.0, 1.0));
            s.diffuse = mix(s.diffuse, layer, t.a * m.over1.a);
        }
        if (has(HAS_OVER2)) {
            let t = textureSample(over2_tex, samp, i.uv2 * i.color.b);
            s.diffuse = mix(s.diffuse, t.rgb * m.over2.rgb, t.a * m.over2.a);
        }
        if (has(HAS_OVER3)) {
            let t = textureSample(over3_tex, samp, i.uv3);
            s.diffuse = mix(s.diffuse, t.rgb * m.over3.rgb, t.a * m.over3.a);
        }
    }

    var mask_alpha = 1.0;
    if (has(HAS_ALPHA_MASK)) {
        let am = textureSample(alpha_mask_tex, samp, i.uv0 * m.st_alpha.xy + m.st_alpha.zw);
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

// Overlay blend, per channel.
fn overlay(base: vec3<f32>, top: vec3<f32>) -> vec3<f32> {
    let dark = 2.0 * base * top;
    let bright = 1.0 - (1.0 - 2.0 * (top - 0.5)) * (1.0 - base);
    return clamp(select(dark, bright, top > vec3<f32>(0.5)), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Hair, after the game's main_hair pixel shader: shade colour from the
// material, a gloss mask that slides along the strand with the view angle and
// is added on top, and a rim towards the silhouette. `light` is the unclamped
// cosine between normal and light.
fn hair_color(i: VsOut, base: vec3<f32>, n: vec3<f32>, light: f32, ramp: f32, detail: vec4<f32>, front: bool) -> vec3<f32> {
    let view = normalize(g.eye.xyz - i.world);

    // Rim tint: the hue of the scene shadow colour, kept faint.
    let hue = rgb_to_hsv(g.ambient_shadow.rgb).x;
    let rim_tint = hsv_to_rgb(vec3<f32>(hue, 1.0, 1.0)) * 0.330000013 + 0.330000013;
    let facing = max(dot(view, n), 0.0);
    let fresnel = clamp(pow(1.0 - facing, m.p3.x * 9.0 + 1.0) * 5.0 - 1.5, 0.0, 1.0);
    let rim = min(rim_tint * fresnel, vec3<f32>(0.5));

    // Gloss mask, read twice a little apart along the strand.
    var gloss_edge = 0.0;
    var gloss = 0.0;
    if (has(HAS_OVER1)) {
        let slide = dot(view, i.bitangent) * (m.p3.z - 1.0);
        let gx = select(light * 0.00499999989 + i.uv1.x, light * 0.0199999809 + i.uv1.x + 0.99000001, m.p3.w > 0.5);
        let gz = slide + i.uv1.y;
        let st = m.over2;
        let a = textureSample(over1_tex, samp, vec2<f32>(gx, gz + 0.00800000038) * st.xy + st.zw).r;
        gloss = textureSample(over1_tex, samp, vec2<f32>(gx, gz) * st.xy + st.zw).r;
        gloss_edge = (a - gloss) * 0.5;
    }

    let shadow = overlay(scene_shadow(), m.shadow.rgb);
    let edge = gloss_edge * (shadow - 1.0) + 1.0;
    let dark = base * edge * shadow;
    var diffuse = base * edge - dark;

    var lit = clamp(ramp, 0.0, 1.0);
    let open = 1.0 - detail.gb;
    diffuse = lit * open.x * diffuse + dark;
    lit = max(lit, open.x);
    diffuse += gloss * (ramp * 0.5 + 0.5);

    var rgb = clamp(open.x * m.p3.y * open.y * rim + diffuse, vec3<f32>(0.0), vec3<f32>(1.0));
    lit = max(lit, 1.0 - m.p0.y);
    rgb *= lit * (1.0 - shadow) + shadow;
    rgb *= select(0.660000026, 1.0, front);
    return rgb * max(light_color(), g.ambient_shadow.rgb);
}

// Skin, after the game's main_skin pixel shader. Left out: the two normal
// maps, the liquid layers, the shadow map and the face normal and face
// shadow switches, which the game leaves at zero.
// m.color2: specular colour. m.color3: specular height, specular power,
// nail (or lip) specular power, detail strength. m.line: line switch, plain
// specular switch, nipple shine, layer mask switch.
fn skin_color(i: VsOut, diffuse: vec3<f32>, front: bool, dm: vec4<f32>, lm: vec4<f32>) -> vec3<f32> {
    let n = normalize(i.normal) * select(-1.0, 1.0, front);
    let view = normalize(g.eye.xyz - i.world);
    let l = normalize(g.light_dir.xyz);
    let lambert = max(dot(l, n), 0.0);
    let lit_ramp = textureSample(ramp_tex, ramp_samp, vec2<f32>(lambert, 0.5)).r;

    // Shadows and lines painted into the masks. The detail strength scales
    // the ones that come with the chosen face or body detail.
    let strength = m.color3.w;
    let extend = m.p0.y * -1.20000005 + 1.0;
    let drawn = min(1.0 - lm.b * strength, 1.0 - dm.g) * (1.0 - extend) + extend;
    let lit = lit_ramp * drawn;

    // Shine from the detail mask's red channel, shifted with the view.
    let hsv = rgb_to_hsv(diffuse);
    let shift = (m.color3.x - 1.0) * 0.800000012;
    var shine = 0.0;
    if (has(HAS_DETAIL)) {
        let uv = shift * vec2<f32>(dot(i.tangent, view), dot(i.bitangent, view)) + i.uv0;
        shine = clamp(textureSample(detail_tex, samp, uv).r * 1.66666698, 0.0, 1.0);
    }
    let core = shine * shine * shine;
    let power = max(dm.a * m.color3.y, (1.0 - dm.a) * m.color3.z);
    let glow = power * core * m.color2.rgb;
    var soft = min(dot(m.color2.rgb, vec3<f32>(0.300000012, 0.589999974, 0.109999999)), (shine - core) * power);
    soft = min(min(lit, soft), dm.a);
    let warm = hsv_to_rgb(vec3<f32>(hsv.x - 0.0799999982, 0.400000006, 1.0)) * 0.970000029 - 1.0;
    let textured = diffuse * (soft * warm + 1.0) + glow;

    // Plain highlight, used when the material switches the textured one off.
    let half_dir = normalize(view + l);
    let facing = max(dot(n, half_dir), 0.0);
    let plain_power = clamp(pow(facing, m.color3.y * 256.0) * m.color3.y * m.color2.a, 0.0, 1.0);
    let plain = m.color2.rgb * plain_power + diffuse;
    let base = clamp(mix(textured, plain, m.line.y), vec3<f32>(0.0), vec3<f32>(1.0));

    // Skin shade: the skin's own hue at two thirds of its saturation.
    let shade = shadow_factor_of(hsv_to_rgb(vec3<f32>(hsv.x, hsv.y * 0.660000026, 1.0)));
    let light = light_color();
    let body = mix(base * shade, base, lit) * max(light, g.ambient_shadow.rgb) * mix(shade, vec3<f32>(1.0), drawn);

    // Lines painted into the masks.
    let ink = diffuse * g.line_color.rgb * shade * ((1.0 - dm.g) * 0.5 + 0.5);
    let w = g.line_color.a;
    let toward_white = 1.0 - (1.0 - 2.0 * (w - 0.5)) * (1.0 - ink);
    let line_rgb = clamp(select(ink * (w * 2.0), toward_white, w > 0.5), vec3<f32>(0.0), vec3<f32>(1.0)) * light;
    var keep = pow((1.0 - g.params.x) * 0.800000012 + 0.200000003, lm.g);
    keep = min((1.0 - lm.r * strength) * 0.5 + 0.5, keep);
    keep = m.line.x * (keep - 1.0) + 1.0;
    return mix(line_rgb, body, keep);
}

// Every kind except the clothes shaders; the result is in gamma space.
fn shade(i: VsOut, front: bool) -> vec4<f32> {
    let kind = m.info.x;

    if (kind == KIND_OVERLAY) {
        let t = textureSample(main_tex, samp, i.uv0 * m.st_main.xy + m.st_main.zw);
        if (m.p3.x > 0.5) {
            // Nose line (toon_nose): red is the line, green a highlight.
            return vec4<f32>(m.color1.rgb * (1.0 - t.r) + t.g, t.a * m.color1.a);
        }
        // Brows, lashes and eye whites (toon_eyew): only pixels at least half
        // solid are kept, lit through the ramp with the material's shade.
        if (t.a < 0.5) {
            discard;
        }
        let shade = overlay(scene_shadow(), m.shadow.rgb);
        let base = t.rgb * m.color1.rgb;
        let dark = shade * base;
        let lit = textureSample(ramp_tex, ramp_samp, vec2<f32>(dot(normalize(g.light_dir.xyz), normalize(i.normal)), 0.5)).r;
        let rgb = lit * (base - dark) + dark;
        return vec4<f32>(rgb * max(light_color(), g.ambient_shadow.rgb), t.a);
    }

    if (kind == KIND_EYE) {
        // Iris (toon_eye): the drawing turned by the material's rotation, the
        // two highlights on top, shaded by the light unless a highlight
        // covers the pixel.
        let turn = m.p3.y * 6.28318548;
        let d = i.uv0 - 0.5;
        let uv = vec2<f32>(dot(d, vec2<f32>(cos(turn), sin(turn))), dot(d, vec2<f32>(-sin(turn), cos(turn)))) + 0.5;
        let iris = textureSample(main_tex, samp, uv * m.st_main.xy + m.st_main.zw);
        var over = vec4<f32>(0.0);
        if (has(HAS_OVER1)) {
            over = textureSample(over1_tex, samp, i.uv1 * m.color2.xy + m.color2.zw).a * m.over1;
        }
        if (has(HAS_OVER2)) {
            over = max(over, textureSample(over2_tex, samp, i.uv2 * m.color3.xy + m.color3.zw).a * m.over2);
        }
        let highlight = over.a * m.p3.x;
        let base = mix(iris.rgb, over.rgb, highlight);
        let shade = overlay(scene_shadow(), m.shadow.rgb);
        let dark = shade * base;
        let lambert = max(dot(normalize(g.light_dir.xyz), normalize(i.normal)), 0.0);
        let lit = clamp(highlight + lambert, 0.0, 1.0);
        let rgb = lit * (base - dark) + dark;
        let out = clamp(rgb * max(light_color(), g.ambient_shadow.rgb), vec3<f32>(0.0), vec3<f32>(1.0));
        return vec4<f32>(out, max(iris.a, highlight));
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

    if (kind == KIND_SKIN) {
        return vec4<f32>(skin_color(i, s.diffuse, front, detail, line), 1.0);
    }

    if (kind == KIND_HAIR) {
        let rgb = hair_color(i, s.diffuse, n, dot(n, normalize(g.light_dir.xyz)), ramp, detail, front);
        return vec4<f32>(rgb, select(1.0, s.alpha, blended));
    }

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
    // The game draws the inside of a surface at two thirds brightness.
    let facing = select(0.660000026, 1.0, front);
    let rgb = clamp(mix(lines, body * facing, keep), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(rgb, select(1.0, s.alpha * m.p1.z, blended));
}

fn outline_color(i: VsOut) -> vec4<f32> {
    let s = read_surface(i);
    if (s.alpha < max(m.p0.x, 0.5)) {
        discard;
    }
    if (m.info.x == KIND_HAIR) {
        // Hair: the hair colour overlaid with the material's line colour.
        let l = m.line.rgb;
        let dark = 2.0 * s.diffuse * l;
        let bright = 1.0 - (1.0 - 2.0 * (l - 0.5)) * (1.0 - s.diffuse);
        let rgb = clamp(select(dark, bright, l > vec3<f32>(0.5)), vec3<f32>(0.0), vec3<f32>(1.0));
        return vec4<f32>(rgb * light_color(), 1.0);
    }
    let shade = shadow_factor(s.diffuse);
    return vec4<f32>(line_color(s.diffuse * shade, light_color()), 1.0);
}

// sRGB to linear, as Unity converts colours and textures in linear colour space.
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    let low = x / 12.9200001;
    let high = pow((x + 0.0549999997) / 1.05499995, vec3<f32>(2.4000001));
    return select(high, low, x <= vec3<f32>(0.0404499993));
}

// Clothes (main_opaque, main_alpha): everything below works on linear values.

// The main texture; the game bakes the card's colours into it.
fn item_main(i: VsOut) -> vec4<f32> {
    if (!has(HAS_MAIN)) {
        return vec4<f32>(1.0);
    }
    let t = textureSample(main_tex, samp, i.uv0 * m.st_main.xy + m.st_main.zw);
    return vec4<f32>(select(t.rgb, to_linear(t.rgb), has(MAIN_SRGB)), t.a);
}

// Body-side cut-outs: each channel of the mask can be switched off.
fn item_mask(i: VsOut) -> f32 {
    if (!has(HAS_ALPHA_MASK)) {
        return 1.0;
    }
    let am = textureSample(alpha_mask_tex, samp, i.uv0 * m.st_alpha.xy + m.st_alpha.zw);
    return min(max(1.0 - m.p2.x, am.r), max(1.0 - m.p2.y, am.g));
}

// Surface normal from the normal map (x in alpha, y in green).
fn item_normal(i: VsOut, front: bool) -> vec3<f32> {
    var t = vec3<f32>(0.0, 0.0, 1.0);
    if (has(HAS_NORMAL)) {
        let nm = textureSample(over1_tex, samp, i.uv0);
        let xy = nm.ag * 2.0 - 1.0;
        t = vec3<f32>(xy, sqrt(1.0 - min(dot(xy, xy), 1.0)));
    }
    let n = normalize(i.normal) * select(-1.0, 1.0, front);
    return normalize(t.x * i.tangent + t.y * i.bitangent + t.z * n);
}

// The scene ramp at a light amount.
fn item_ramp(x: f32) -> f32 {
    let v = textureSample(ramp_tex, ramp_samp, vec2<f32>(x, x)).r;
    return select(v, to_linear(vec3<f32>(v)).x, g.params.w > 0.5);
}

// The material's own ramp; white when it has none. The lookup stops at the
// outermost texels, as a clamped texture does.
fn item_another_ramp(x: f32) -> f32 {
    if (!has(HAS_ANOTHER_RAMP)) {
        return 1.0;
    }
    let half = 0.5 / vec2<f32>(textureDimensions(over2_tex));
    return textureSample(over2_tex, samp, clamp(vec2<f32>(x, x), half, 1.0 - half)).r;
}

// Shine painted in the detail mask's red channel, read where the view
// shifts it to. `depth` is the shader's own factor on the height slider.
fn item_shine(i: VsOut, view: vec3<f32>, depth: f32) -> f32 {
    if (!has(HAS_DETAIL)) {
        return 0.0;
    }
    let shift = (m.over2.x - 1.0) * depth;
    let uv = shift * vec2<f32>(dot(i.tangent, view), dot(i.bitangent, view)) + i.uv0;
    return textureSample(detail_tex, samp, uv).r;
}

// Multiplier for the unlit side: shadow_factor() on linear values.
fn item_shadow_factor(base: vec3<f32>) -> vec3<f32> {
    let hsv = rgb_to_hsv(base * to_linear(m.shadow.rgb));
    let tint = hsv_to_rgb(vec3<f32>(hsv.x, hsv.y * 0.5, 1.0));
    let a = g.ambient_shadow.a;
    let rgb = to_linear(g.ambient_shadow.rgb);
    let deep = 1.0 - (1.0 - a) * (1.0 - rgb);
    let shallow = (a + 1.0) * rgb;
    let scene = clamp(select(shallow, deep, a > 0.0), vec3<f32>(0.0), vec3<f32>(1.0));
    let dark = tint * scene * 1.79999995;
    let light = 1.0 - (1.0 - 2.0 * (tint * 0.899999976 - 0.5)) * (1.0 - scene);
    return clamp(select(dark, light, tint > vec3<f32>(0.555555582)), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Drawn lines: `base` pulled toward black or white by the scene's line depth.
fn item_lines(base: vec3<f32>, light: vec3<f32>) -> vec3<f32> {
    let w = g.line_color.a;
    let toward_white = 1.0 - (1.0 - 2.0 * (w - 0.5)) * (1.0 - base);
    let toward_black = base * (w * 2.0);
    return clamp(select(toward_black, toward_white, w > 0.5), vec3<f32>(0.0), vec3<f32>(1.0)) * light;
}

// The highlight both shaders share: pow(n.h, 256 p) * p.
fn item_highlight(ndh: f32, power: f32) -> f32 {
    if (power <= 0.0) {
        return 0.0;
    }
    return pow(max(ndh, 0.0), power * 256.0) * power;
}

// See-through cloth, after the game's main_alpha pixel shader (forward base
// pass without the shadow map; liquid layers left out).
fn item_alpha(i: VsOut, front: bool) -> vec4<f32> {
    let main = item_main(i);
    let alpha = min(item_mask(i), main.a);
    // With the outline switch on, the inside of the cloth is not drawn.
    let side = select(1.0 - m.over3.z, 1.0, front);
    if (min(side, alpha * 10.0) < 0.5) {
        discard;
    }
    let n = item_normal(i, front);
    let l = normalize(g.light_dir.xyz);
    let view = normalize(g.eye.xyz - i.world);
    let ndl = dot(l, n);
    let ndh = dot(n, normalize(view + l));
    let ndv = dot(n, view);

    var dm = vec4<f32>(0.0);
    if (has(HAS_DETAIL)) {
        dm = textureSample(detail_tex, samp, i.uv0);
    }
    let shine = item_shine(i, view, 0.899999976);
    let specular = min(shine * shine, 1.0) * to_linear(m.over1.rgb)
        + clamp(item_highlight(ndh, m.over2.y), 0.0, 1.0);

    let ramp = clamp(min(item_another_ramp(ndv), item_ramp(ndl)), 0.0, 1.0);
    let open = 1.0 - dm.g;
    let lit = ramp * open;
    let diffuse = clamp(specular * (1.0 - (1.0 - lit) * 0.5) + main.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let factor = item_shadow_factor(main.rgb);
    var rgb = mix(diffuse * factor, diffuse, lit);

    // Rim towards the silhouette, tinted by the hue of the scene shadow.
    let fresnel = max(1.0 - max(ndv, 0.0), 1e-6);
    let rim = clamp(pow(fresnel, m.over3.x * 9.0 + 1.0) * 2.5 - 0.5, 0.0, 1.0);
    let hue = rgb_to_hsv(to_linear(g.ambient_shadow.rgb)).x;
    let rim_rgb = min(rim * (hsv_to_rgb(vec3<f32>(hue, 1.0, 1.0)) * 0.330000013 + 0.330000013), vec3<f32>(0.5));
    rgb = clamp(open * m.over3.y * (1.0 - dm.b) * rim_rgb + rgb, vec3<f32>(0.0), vec3<f32>(1.0));

    let extend = max(1.0 - m.p0.y, max(ramp, open));
    rgb *= mix(factor, vec3<f32>(1.0), extend);
    rgb *= select(0.660000026, 1.0, front);

    let light = light_color();
    let lines = item_lines(main.rgb * to_linear(g.line_color.rgb) * factor, light);
    let keep = pow((1.0 - g.params.x) * 0.889999986 + 0.00999999978, dm.b * m.p1.x);
    let out = mix(lines, rgb * max(light, to_linear(g.ambient_shadow.rgb)), keep);
    return vec4<f32>(out, alpha);
}

// Solid cloth, after the game's main_opaque pixel shader (forward base pass
// without the shadow map; liquid layers left out).
fn item_opaque(i: VsOut, front: bool) -> vec4<f32> {
    let main = item_main(i);
    if (min(item_mask(i), main.a) < 0.5) {
        discard;
    }
    let n = item_normal(i, front);
    let l = normalize(g.light_dir.xyz);
    let view = normalize(g.eye.xyz - i.world);
    let ndl = dot(l, n);
    let ndh = dot(n, normalize(view + l));
    let fresnel = max(1.0 - max(dot(n, view), 0.0), 1e-6);

    var dm = vec4<f32>(0.0);
    if (has(HAS_DETAIL)) {
        dm = textureSample(detail_tex, samp, i.uv0);
    }
    var lm = vec4<f32>(0.0);
    if (has(HAS_LINE)) {
        lm = textureSample(line_tex, samp, i.uv0);
    }
    let spec_rgb = to_linear(m.over1.rgb);
    let power = m.over2.y;

    // Light amount: the scene ramp, the material's own ramp where the line
    // mask says so, and the shadows painted into the masks.
    let line_r = mix(lm.r, dm.r, m.p0.w);
    let scene_ramp = item_ramp(ndl);
    let ramp = mix(scene_ramp, item_another_ramp(abs(ndh)), line_r);
    let extend = m.p0.y * -1.20000005 + 1.0;
    let drawn = min(1.0 - lm.b, 1.0 - dm.g) * (1.0 - extend) + extend;
    let lit = ramp * drawn;

    let factor = item_shadow_factor(main.rgb);
    let shade = clamp(max(1.0 - line_r, 1.0 - m.p0.z) * factor, vec3<f32>(0.0), vec3<f32>(1.0));

    // Shine from the detail mask, and the darkening that goes with it.
    let shine = item_shine(i, view, 0.800000012);
    let gloss = min(shine * shine, 1.0);
    var deepen = min(clamp(shine - gloss, 0.0, 1.0), power);
    deepen = min(min(deepen, lit), dot(spec_rgb, vec3<f32>(0.300000012, 0.589999974, 0.109999999)));
    let highlight = clamp(item_highlight(ndh, power) * m.over1.a, 0.0, 1.0);
    let painted = min(max(dm.a, m.over2.z), min(gloss, max(1.0 - dm.a, power)));
    let with_mask = main.rgb * (1.0 + deepen * (shade * 2.0 - 2.0))
        + (highlight * (1.0 - dm.r) + painted) * spec_rgb;
    let without_mask = spec_rgb * ((1.0 - dm.r) * highlight) + main.rgb;
    let diffuse = clamp(mix(with_mask, without_mask, m.over2.w), vec3<f32>(0.0), vec3<f32>(1.0));
    var rgb = mix(diffuse * shade, diffuse, lit);

    // Rim in the specular colour, kept off the drawn lines.
    let rim = clamp(((1.0 - dm.b) * 2.77777791 - 1.77777803) * pow(fresnel, m.over3.x * 9.0 + 1.0), 0.0, 0.600000024);
    rgb = clamp(rim * spec_rgb * m.over3.y * (1.0 - dm.g) + rgb, vec3<f32>(0.0), vec3<f32>(1.0));

    let light = light_color();
    rgb *= max(light, to_linear(g.ambient_shadow.rgb));
    rgb *= select(0.660000026, 1.0, front);
    rgb *= drawn * (1.0 - shade) + shade;

    let lines = item_lines(main.rgb * shade * 0.5, light);
    let keep = pow((1.0 - g.params.x) * 0.889999986 + 0.00999999978, mix(lm.g, dm.b, m.p1.x));
    return vec4<f32>(mix(lines, rgb, keep), 1.0);
}

@fragment
fn fs_main(i: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if (m.info.x == KIND_ITEM && m.info.z == ITEM_ALPHA) {
        return item_alpha(i, front);
    }
    if (m.info.x == KIND_ITEM && m.info.z == ITEM_OPAQUE) {
        return item_opaque(i, front);
    }
    let c = shade(i, front);
    return vec4<f32>(to_linear(c.rgb), c.a);
}

@fragment
fn fs_outline(i: VsOut) -> @location(0) vec4<f32> {
    let c = outline_color(i);
    return vec4<f32>(to_linear(c.rgb), c.a);
}
