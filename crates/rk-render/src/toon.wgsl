// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

// Koikatsu reference shading. One module serves every material kind; the kind
// is a uniform so the pipeline count stays small. The game renders in linear
// colour space: sRGB textures are decoded when sampled, material colours
// arrive linearised and the target is sRGB, so every branch shades and
// blends on linear values as the game does. The two scene colours
// (ambient_shadow and line_color) are the exception: the game sets them as
// global colours, which reach its shaders as stored.

struct Globals {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    eye: vec4<f32>,
    // rgb: scene shadow colour as stored (not linearised), a: 1 - shadow depth
    ambient_shadow: vec4<f32>,
    // rgb unused by the game, a: 1 - line depth
    line_color: vec4<f32>,
    // x: line width, y: light intensity, z: ambient light,
    // w: 1 when the ramp texture holds sRGB values
    params: vec4<f32>,
    // x: what the shadow caster adds to clip depth, y: 1 when casters in
    // front of the near plane are flattened onto it, z: 1 when surfaces read
    // the shadow mask, w: how far the fallback's caster moves inwards
    shadow: vec4<f32>,
    // rgb: the scene's ambient light
    ambient_light: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var ramp_tex: texture_2d<f32>;
@group(0) @binding(2) var ramp_samp: sampler;
// The light's shadows over the picture (Unity's screen-space
// _ShadowMapTexture): 1 lit, down to one minus the shadow strength.
@group(0) @binding(3) var shadow_mask_tex: texture_2d<f32>;

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
// The shadow caster pass is the fallback shader's (Legacy Shaders/VertexLit).
const CASTER_FALLBACK: u32 = 512u;
// main_item_studio binds its third pattern mask in the ninth slot.
const HAS_PATTERN3: u32 = 1024u;
// main_skin: the liquid amounts (_Texture2), the liquid normal map
// (_Texture3) and the liquid mask.
const HAS_LIQUID: u32 = 2u;
const HAS_LIQUID_NORMAL: u32 = 1024u;
const HAS_LIQUID_MASK: u32 = 8192u;
// Clothes materials bind their normal map and second ramp in the first two
// layer slots.
const HAS_NORMAL: u32 = 32u;
const HAS_ANOTHER_RAMP: u32 = 64u;

// Which game shader an item follows (info.z).
const ITEM_OPAQUE: u32 = 1u;
const ITEM_ALPHA: u32 = 2u;
const ITEM_STUDIO_ALPHA: u32 = 3u;
const ITEM_GLASSES: u32 = 4u;
const ITEM_MAIN: u32 = 5u;
const ITEM_EMBLEM: u32 = 6u;
const ITEM_EMISSION: u32 = 7u;
const ITEM_DITHERD: u32 = 8u;
const ITEM_STUDIO: u32 = 9u;
const ITEM_TEXTURE: u32 = 10u;
const ITEM_OPAQUE_LOW: u32 = 11u;
const ITEM_COLOR: u32 = 12u;

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
    // What the later item shaders need beside the above; each says what
    // it keeps where.
    ext: array<vec4<f32>, 24>,
    // x: kind, y: flags, z: item shader, w: 1 when the renderer receives
    // shadows
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
@group(1) @binding(9) var over4_tex: texture_2d<f32>;
// The tenth and eleventh slot, which the KKShadersPlus materials use.
@group(1) @binding(10) var over5_tex: texture_2d<f32>;
@group(1) @binding(11) var over6_tex: texture_2d<f32>;
// The twelfth slot: main_skin's liquid mask.
@group(1) @binding(12) var over7_tex: texture_2d<f32>;
@group(1) @binding(13) var samp: sampler;
// Always tiles, for textures that repeat on a material whose main one does not.
@group(1) @binding(14) var wrap_samp: sampler;

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

// The shadow mask at this pixel, as the shaders' shadow variants sample it
// with their screen position. Without shadows the game runs the variants
// that do not have the term.
fn shadow_mask(i: VsOut) -> f32 {
    if (g.shadow.z == 0.0 || m.info.w == 0u) {
        return 1.0;
    }
    return textureLoad(shadow_mask_tex, vec2<i32>(i.clip.xy), 0).r;
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
        var uv = v.uv0;
        let item = m.info.z;
        if (m.info.x == KIND_ITEM && (item == ITEM_MAIN || item == ITEM_EMISSION || item == ITEM_STUDIO)) {
            uv = uv * m.p2.xy + m.p2.zw;
        }
        // The KKShadersPlus materials keep the detail mask's placement here.
        if (item >= XK_FIRST) {
            uv = uv * m.p3.xy + m.p3.zw;
        }
        width *= 1.0 - textureSampleLevel(detail_tex, samp, uv, 0.0).b;
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
    // The colours go where the colour mask's channels say. The shaders'
    // default mask is black, so without one the texture keeps its colours.
    if (has(HAS_COLOR_MASK) && kind != KIND_SKIN) {
        let mask = textureSample(color_mask_tex, samp, i.uv0);
        tint = mix(tint, m.color1.rgb, mask.r);
        tint = mix(tint, m.color2.rgb, mask.g);
        tint = mix(tint, m.color3.rgb, mask.b);
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

    var lit = clamp(ramp, 0.0, 1.0) * shadow_mask(i);
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

// Tangent space normal out of a normal map texel (x in alpha, y in green).
fn skin_unpack(t: vec4<f32>) -> vec3<f32> {
    let xy = t.ag * 2.0 - 1.0;
    return vec3<f32>(xy, sqrt(1.0 - min(dot(xy, xy), 1.0)));
}

// One tangent space normal turned by another, as main_skin lays the detail
// map over the base map and the liquid's over both.
fn skin_turn(base: vec3<f32>, top: vec3<f32>) -> vec3<f32> {
    let t = base + vec3<f32>(0.0, 0.0, 1.0);
    let u = top * vec3<f32>(-1.0, -1.0, 1.0);
    return t * dot(t, u) / t.z - u;
}

// How much liquid a level puts down: the red channel of the liquid texture
// at level one, its green channel as well at level two.
fn skin_liquid_level(level: vec4<f32>, texel: vec4<f32>) -> vec4<f32> {
    let one = clamp(level, vec4<f32>(0.0), vec4<f32>(1.0));
    let two = clamp(level - 1.0, vec4<f32>(0.0), vec4<f32>(1.0));
    return max(texel.r * one, texel.g * two);
}

// The liquid cover of a pixel. The mask says which level a place answers
// to: pure red the front top, pure green the front bottom, pure blue the
// back top, yellow the back bottom and cyan the face.
// `body`: _liquidftop, _liquidfbot, _liquidbtop, _liquidbbot.
fn skin_liquid_cover(body: vec4<f32>, face: f32, texel: vec4<f32>, mask: vec3<f32>) -> f32 {
    let top = skin_liquid_level(vec4<f32>(body.x), texel).x;
    let rest = skin_liquid_level(vec4<f32>(body.yzw, face), texel);
    let pure = mask - max(mask.bbg, mask.grr);
    let pair = min(mask.gb, mask.rg) * 1.11111104 - 0.111111097;
    var cover = min(top, pure.x);
    cover = max(cover, min(pure.y, rest.x));
    cover = max(min(pure.z, rest.y), cover);
    cover = max(min(pair.x, rest.z), cover);
    return clamp(max(min(pair.y, rest.w), cover), 0.0, 1.0);
}

struct SkinSurface {
    // world normal from the normal maps
    n: vec3<f32>,
    // liquid cover
    liquid: f32,
};

// main_skin's normal: _NormalMap, _NormalMapDetail laid over it by
// _DetailNormalMapScale, and the liquid's normal map where liquid lies.
// m.ext[0..4]: placement of _NormalMap, _NormalMapDetail, _Texture2,
// _Texture3 and _liquidmask. m.ext[5]: _LiquidTiling. m.ext[6]: _liquidftop,
// _liquidfbot, _liquidbtop, _liquidbbot. m.ext[7]: _liquidface, _rimpower,
// _rimV, which liquid textures tile (bit 0 _Texture2, bit 1 _Texture3).
fn skin_surface(i: VsOut, front: bool) -> SkinSurface {
    var s: SkinSurface;
    // The shader's default for the three normal maps is flat.
    var base = vec3<f32>(0.0, 0.0, 1.0);
    if (has(HAS_OVER5)) {
        base = skin_unpack(textureSample(over5_tex, samp, i.uv0 * m.ext[0].xy + m.ext[0].zw));
    }
    var detail = vec3<f32>(0.0, 0.0, 1.0);
    if (has(HAS_OVER6)) {
        detail = skin_unpack(textureSample(over6_tex, samp, i.uv0 * m.ext[1].xy + m.ext[1].zw));
    }
    var t = mix(base, skin_turn(base, detail), m.color3.w);

    // Without an amount the cover is zero whatever the textures hold, and
    // the default amount texture and mask are black.
    s.liquid = 0.0;
    let wet = max(max(m.ext[6].x, m.ext[6].y), max(max(m.ext[6].z, m.ext[6].w), m.ext[7].x)) > 0.0;
    if (wet && has(HAS_LIQUID) && has(HAS_LIQUID_MASK)) {
        let tiles = u32(m.ext[7].w);
        let luv = i.uv0 * m.ext[5].zw + m.ext[5].xy;
        let uv2 = luv * m.ext[2].xy + m.ext[2].zw;
        let uv3 = luv * m.ext[3].xy + m.ext[3].zw;
        var amount: vec4<f32>;
        if ((tiles & 1u) != 0u) {
            amount = textureSample(color_mask_tex, wrap_samp, uv2);
        } else {
            amount = textureSample(color_mask_tex, samp, uv2);
        }
        var ripple = vec3<f32>(0.0, 0.0, 1.0);
        if (has(HAS_LIQUID_NORMAL)) {
            if ((tiles & 2u) != 0u) {
                ripple = skin_unpack(textureSample(over4_tex, wrap_samp, uv3));
            } else {
                ripple = skin_unpack(textureSample(over4_tex, samp, uv3));
            }
        }
        let mask = textureSample(over7_tex, samp, i.uv0 * m.ext[4].xy + m.ext[4].zw).rgb;
        s.liquid = skin_liquid_cover(m.ext[6], m.ext[7].x, amount, mask);
        t = mix(t, skin_turn(t, ripple), s.liquid);
    }
    let n = normalize(i.normal) * select(-1.0, 1.0, front);
    s.n = normalize(t.x * i.tangent + t.y * i.bitangent + t.z * n);
    return s;
}

// The liquid's own colour: paler where lit, with a sharp highlight and a
// rim. `lambert` is the clamped light amount, `facing` and `ndv` the clamped
// normal against the half vector and the view. `dm` is the detail mask.
fn skin_liquid_color(liquid: f32, lambert: f32, facing: f32, ndv: f32, dm: vec4<f32>) -> vec3<f32> {
    let tone = (lambert + 0.5) * vec3<f32>(0.149999976, 0.199999988, 0.300000012) + vec3<f32>(0.850000024, 0.800000012, 0.699999988);
    let tint = liquid * vec3<f32>(0.350000024, 0.45294112, 0.607352912) + vec3<f32>(0.5, 0.397058904, 0.242647097);
    let glint = pow(facing, 256.0) * 0.5;
    var rim = pow(1.0 - ndv, m.ext[7].y * 9.0 + 1.0) * 5.0 - 2.0;
    rim = clamp(min(rim, (1.0 - dm.b) * 2.5 - 1.5), 0.0, 1.0) * (1.0 - dm.g);
    return tint * tone + glint + rim * m.ext[7].z;
}

// Skin, after the game's main_skin pixel shader. Left out: the face normal
// and face shadow switches (_NormalMask), which the game leaves at zero.
// m.color2: specular colour. m.color3: specular height, specular power,
// nail (or lip) specular power, detail strength. m.line: line switch, plain
// specular switch, nipple shine, layer mask switch. m.ext: see skin_surface.
fn skin_color(i: VsOut, diffuse: vec3<f32>, front: bool, dm: vec4<f32>, lm: vec4<f32>) -> vec3<f32> {
    let surface = skin_surface(i, front);
    let n = surface.n;
    let view = normalize(g.eye.xyz - i.world);
    let l = normalize(g.light_dir.xyz);
    let lambert = max(dot(l, n), 0.0);
    let lit_ramp = textureSample(ramp_tex, ramp_samp, vec2<f32>(lambert, 0.5)).r;

    // Shadows and lines painted into the masks. The detail strength scales
    // the ones that come with the chosen face or body detail.
    let strength = m.color3.w;
    let extend = m.p0.y * -1.20000005 + 1.0;
    let drawn = min(1.0 - lm.b * strength, 1.0 - dm.g) * (1.0 - extend) + extend;
    // The shadow variant takes max(face shadow, mask * 2 - 1); the face
    // shadow switch is zero.
    var shadowed = 1.0;
    if (g.shadow.z != 0.0 && m.info.w != 0u) {
        shadowed = clamp(shadow_mask(i) * 2.0 - 1.0, 0.0, 1.0);
    }
    let lit = lit_ramp * shadowed * drawn;

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
    var covered = mix(base * shade, base, lit);
    if (surface.liquid > 0.0) {
        let ndv = max(dot(n, view), 0.0);
        covered = mix(covered, skin_liquid_color(surface.liquid, lambert, facing, ndv, dm), surface.liquid);
    }
    let body = covered * max(light, g.ambient_shadow.rgb) * mix(shade, vec3<f32>(1.0), drawn);

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

// Every kind except the clothes shaders.
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
    ramp *= shadow_mask(i);
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

// Clothes and studio items (main_opaque, main_alpha, main_item_studio_alpha):
// everything below works on linear values.

// The main texture; the game bakes the card's colours into it.
fn item_main(i: VsOut) -> vec4<f32> {
    if (!has(HAS_MAIN)) {
        return vec4<f32>(1.0);
    }
    let t = textureSample(main_tex, samp, i.uv0 * m.st_main.xy + m.st_main.zw);
    return t;
}

// Body-side cut-outs: each channel of the mask can be switched off.
fn item_mask(i: VsOut) -> f32 {
    return body_mask(i.uv0);
}

fn body_mask(uv0: vec2<f32>) -> f32 {
    if (!has(HAS_ALPHA_MASK)) {
        return 1.0;
    }
    let am = textureSample(alpha_mask_tex, samp, uv0 * m.st_alpha.xy + m.st_alpha.zw);
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
    return textureSample(ramp_tex, ramp_samp, vec2<f32>(x, x)).r;
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
    let hsv = rgb_to_hsv(base * m.shadow.rgb);
    let tint = hsv_to_rgb(vec3<f32>(hsv.x, hsv.y * 0.5, 1.0));
    let a = g.ambient_shadow.a;
    let rgb = g.ambient_shadow.rgb;
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
// pass; liquid layers left out).
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
    let specular = min(shine * shine, 1.0) * m.over1.rgb
        + clamp(item_highlight(ndh, m.over2.y), 0.0, 1.0);

    let ramp = clamp(min(item_another_ramp(ndv), item_ramp(ndl)), 0.0, 1.0) * shadow_mask(i);
    let open = 1.0 - dm.g;
    let lit = ramp * open;
    let diffuse = clamp(specular * (1.0 - (1.0 - lit) * 0.5) + main.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let factor = item_shadow_factor(main.rgb);
    var rgb = mix(diffuse * factor, diffuse, lit);

    // Rim towards the silhouette, tinted by the hue of the scene shadow.
    let fresnel = max(1.0 - max(ndv, 0.0), 1e-6);
    let rim = clamp(pow(fresnel, m.over3.x * 9.0 + 1.0) * 2.5 - 0.5, 0.0, 1.0);
    let hue = rgb_to_hsv(g.ambient_shadow.rgb).x;
    let rim_rgb = min(rim * (hsv_to_rgb(vec3<f32>(hue, 1.0, 1.0)) * 0.330000013 + 0.330000013), vec3<f32>(0.5));
    rgb = clamp(open * m.over3.y * (1.0 - dm.b) * rim_rgb + rgb, vec3<f32>(0.0), vec3<f32>(1.0));

    let extend = max(1.0 - m.p0.y, max(ramp, open));
    rgb *= mix(factor, vec3<f32>(1.0), extend);
    rgb *= select(0.660000026, 1.0, front);

    let light = light_color();
    let lines = item_lines(main.rgb * g.line_color.rgb * factor, light);
    let keep = pow((1.0 - g.params.x) * 0.889999986 + 0.00999999978, dm.b * m.p1.x);
    let out = mix(lines, rgb * max(light, g.ambient_shadow.rgb), keep);
    return vec4<f32>(out, alpha);
}

// Solid cloth, after the game's main_opaque pixel shader (forward base pass;
// liquid layers left out).
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
    let spec_rgb = m.over1.rgb;
    let power = m.over2.y;

    // Light amount: the scene ramp, the material's own ramp where the line
    // mask says so, and the shadows painted into the masks.
    let line_r = mix(lm.r, dm.r, m.p0.w);
    let scene_ramp = item_ramp(ndl);
    let ramp = mix(scene_ramp, item_another_ramp(abs(ndh)), line_r) * shadow_mask(i);
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
    rgb *= max(light, g.ambient_shadow.rgb);
    rgb *= select(0.660000026, 1.0, front);
    rgb *= drawn * (1.0 - shade) + shade;

    let lines = item_lines(main.rgb * shade * 0.5, light);
    let keep = pow((1.0 - g.params.x) * 0.889999986 + 0.00999999978, mix(lm.g, dm.b, m.p1.x));
    return vec4<f32>(mix(lines, rgb, keep), 1.0);
}

// Studio item, after MaterialEditor's main_item_studio_alpha pixel shader
// (forward base pass). Left out: the three pattern
// masks, read as the white the shader defaults to, and the scale and offset
// of every texture but the main one.
// m.over1: second ramp everywhere, emission power, scene shadow off, alpha.
fn item_studio_alpha(i: VsOut, front: bool) -> vec4<f32> {
    // The pass culls back faces.
    if (!front) {
        discard;
    }
    let main = item_main(i);
    let alpha = main.a * m.over1.w;
    if (alpha * 0.500999987 - 0.000999987125 < 0.0) {
        discard;
    }

    // The three colours, placed by the colour mask.
    var tint = vec3<f32>(1.0);
    if (has(HAS_COLOR_MASK)) {
        let cm = textureSample(color_mask_tex, samp, i.uv0);
        tint = mix(mix(cm.r * (m.color1.rgb - 1.0) + 1.0, m.color2.rgb, cm.g), m.color3.rgb, cm.b);
    }
    let base = main.rgb * tint;

    var dm = vec4<f32>(0.0);
    if (has(HAS_DETAIL)) {
        dm = textureSample(detail_tex, samp, i.uv0);
    }
    var lm = vec4<f32>(0.0);
    if (has(HAS_LINE)) {
        lm = textureSample(line_tex, samp, i.uv0);
    }

    let n = item_normal(i, true);
    let l = normalize(g.light_dir.xyz);
    let view = normalize(g.eye.xyz - i.world);
    let ndl = dot(l, n);
    let ndh = max(dot(n, normalize(view + l)), 0.0);
    let ndv = dot(n, view);

    // Where the material's own ramp replaces the scene ramp.
    let line_r = mix(lm.r, dm.r, m.p0.w);
    let another = mix(line_r, 1.0, m.over1.x);
    let factor = item_shadow_factor(base);
    let shade = factor * (clamp(2.0 - m.p0.z - another, 0.0, 1.0) * 0.670000017 + 0.330000013);

    // Light amount, with the shadows painted into the masks.
    let ramp = mix(item_ramp(ndl), item_another_ramp(ndh), another) * shadow_mask(i);
    let extend = m.p0.y * -1.20000005 + 1.0;
    let drawn = min(1.0 - dm.g, 1.0 - lm.b) * (1.0 - extend) + extend;
    var rgb = mix(base * shade, base, ramp * drawn);

    // Highlight: the shine painted in the detail mask, or a plain one.
    let shine = item_shine(i, view, 0.899999976);
    let power = m.over2.y;
    var glint = 0.0;
    if (power > 0.0) {
        glint = (pow(ndh, power * 256.0) * 5.0 - 4.0) * power;
    }
    let textured = clamp(glint + min(shine * shine, 1.0), 0.0, 1.0);
    let plain = clamp(glint, 0.0, 1.0) * dm.r;
    rgb += mix(textured, plain, m.over2.w);

    // Rim towards the silhouette, tinted by the hue of the scene shadow.
    let fresnel = max(1.0 - max(ndv, 0.0), 1e-6);
    let rim = clamp(pow(fresnel, m.over3.x * 9.0 + 1.0) * 2.5 - 0.5, 0.0, 1.0) * m.over3.y
        * ((1.0 - dm.b) * 9.99999809 - 8.99999809);
    let hue = rgb_to_hsv(g.ambient_shadow.rgb).x;
    let rim_rgb = rim * (hsv_to_rgb(vec3<f32>(hue, 1.0, 1.0)) * 0.400000006 + 0.300000012);
    rgb += clamp((1.0 - dm.g) * rim_rgb, vec3<f32>(0.0), vec3<f32>(0.5));

    let light = light_color();
    rgb *= max(light, g.ambient_shadow.rgb);
    rgb *= drawn * (1.0 - shade) + shade;

    // Lines painted into the masks.
    let lines = item_lines(base * g.line_color.rgb * shade * light, light);
    rgb = mix(rgb, lines, mix(lm.g, dm.b, m.p1.x));

    // Glow, and the switch that shows the plain colour instead of the light.
    let off = m.over1.z;
    let glow = min(min(min(base, vec3<f32>(dm.r)), vec3<f32>(1.0 - dm.g)), vec3<f32>(m.over1.y)) * (m.over1.y * 3.0);
    rgb = min(vec3<f32>(1.0 - off), rgb) + mix(glow, base, off);
    return vec4<f32>(rgb, alpha);
}

// The lens of glasses, after the game's toon_glasses_lod0 pixel shader
// (forward base pass; all its variants are one program, without shadows).
// The colour comes out scaled by the alpha, for One with OneMinusSrcAlpha.
// m.color1: _Color, m.color2: _Color4, m.over1: colour inverse, colour sort,
// rim power. m.over2 places the normal map and m.st_alpha the ramp.
fn item_glasses(i: VsOut, front: bool) -> vec4<f32> {
    // The pass culls back faces.
    if (!front) {
        discard;
    }
    var t = vec2<f32>(0.0);
    if (has(HAS_NORMAL)) {
        t = textureSample(over1_tex, samp, i.uv0 * m.over2.xy + m.over2.zw).ag * 2.0 - 1.0;
    }
    let n = normalize(sqrt(1.0 - min(dot(t, t), 1.0)) * normalize(i.normal) + t.x * i.tangent + t.y * i.bitangent);
    let view = normalize(g.eye.xyz - i.world);
    let l = normalize(g.light_dir.xyz);
    let h = normalize(view + l);
    let ndl = dot(n, l);

    // The ramp, read along its diagonal by how the mirrored view meets the
    // half vector. Without a texture the shader reads black.
    var ramp = vec3<f32>(0.0);
    if (has(HAS_ANOTHER_RAMP)) {
        let x = abs(dot(reflect(-view, n), h)) * (n.x + n.y + n.z) * 0.5 + 0.5;
        ramp = textureSample(over2_tex, samp, vec2<f32>(x, x) * m.st_alpha.xy + m.st_alpha.zw).rgb;
    }
    let rim = clamp(pow(1.0 - max(dot(n, view), 0.0), m.over1.z) * 2.0 - 1.0, 0.0, 1.0);
    let glint = clamp(abs(dot(n, h)) * 3.99999905 - 3.79999900, 0.0, 1.0);

    // The two colours, mixed one way for the lens and the other for what
    // it mirrors.
    let spread = (m.color1.rgb - m.color2.rgb) * (1.0 - m.over1.y);
    let main = item_main(i);
    let base = main.rgb * (m.over1.x * spread + m.color2.rgb);
    let mirror = (1.0 - m.over1.x) * spread + m.color2.rgb;

    // Scene shadow colour and depth, overlaid with the material's shadow
    // colour; it fades out towards the light and away from it.
    let a = g.ambient_shadow;
    let scene = clamp(
        select(a.rgb * (a.a + a.a), 1.0 - (1.0 - (a.a - 0.5) * 2.0) * (1.0 - a.rgb), a.a > 0.5),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
    let shade = mix(overlay(scene, m.shadow.rgb), vec3<f32>(1.0), abs(ndl));
    let dark = base * shade;

    var rgb = (glint + ramp + rim) * mirror + rim * a.rgb + dark;
    rgb *= max(light_color(), a.rgb);
    rgb = clamp(rgb * m.color2.a + dark, vec3<f32>(0.0), vec3<f32>(1.0));
    // Light: the directional light and the scene's ambient light.
    rgb *= max(ndl, 0.0) * g.params.y + g.ambient_light.rgb;
    let alpha = main.a * m.color2.a;
    return vec4<f32>(rgb * alpha, alpha);
}

// Accessory, after the game's main_item pixel shaders. What the forward base
// and outline passes share: the cut at half of twice the texture alpha and
// the base colour (the three colours placed by the colour mask, darkened by
// the line mask's blue channel). Returns the base colour and twice the alpha.
// m.st_alpha, m.p2, m.p3 and m.line place the colour mask, detail mask, line
// mask and normal map. Left out: the turn _Clock gives the line mask over
// time (the picture is taken at an angle of zero), and the scale and offset
// of the two ramps.
fn item_main_base(i: VsOut) -> vec4<f32> {
    var main = vec4<f32>(1.0);
    if (has(HAS_MAIN)) {
        main = textureSample(main_tex, samp, i.uv0 * m.st_main.xy + m.st_main.zw);
    }
    let a = clamp(main.a + main.a, 0.0, 1.0);
    if (a - 0.5 < 0.0) {
        discard;
    }
    var tint = vec3<f32>(1.0);
    if (has(HAS_COLOR_MASK)) {
        let cm = textureSample(color_mask_tex, samp, i.uv0 * m.st_alpha.xy + m.st_alpha.zw);
        tint = mix(mix(cm.r * (m.color1.rgb - 1.0) + 1.0, m.color2.rgb, cm.g), m.color3.rgb, cm.b);
    }
    var base = main.rgb * tint;
    if (has(HAS_LINE)) {
        base *= 1.0 - textureSample(line_tex, samp, i.uv0 * m.p3.xy + m.p3.zw).b;
    }
    return vec4<f32>(base, a);
}

// How far the unlit side goes: the shadow factor, weakened where the
// material's own ramp is in use. Returns the factor on the base colour and,
// in w, the share of the second ramp.
fn item_main_shade(base: vec3<f32>, dm: vec4<f32>, lm: vec4<f32>) -> vec4<f32> {
    let line_r = mix(lm.r, dm.r, m.p0.w);
    let another = mix(line_r, 1.0, m.over1.x);
    let shade = item_shadow_factor(base) * (clamp(2.0 - m.p0.z - another, 0.0, 1.0) * 0.670000017 + 0.330000013);
    return vec4<f32>(shade, another);
}

// Forward base pass of main_item (programs 2 and 10, which differ by the
// shadow mask only). The colour is not scaled by the alpha it is blended
// with (One, OneMinusSrcAlpha), so the rim of a cut-out adds to what is
// behind it.
// m.over1.x: second ramp everywhere. m.over2: shine height, specular power,
// unused, no shine texture. m.over3: rim power, rim strength.
fn item_main_forward(i: VsOut, front: bool) -> vec4<f32> {
    let ba = item_main_base(i);
    let base = ba.rgb;

    var dm = vec4<f32>(0.0);
    if (has(HAS_DETAIL)) {
        dm = textureSample(detail_tex, samp, i.uv0 * m.p2.xy + m.p2.zw);
    }
    var lm = vec4<f32>(0.0);
    if (has(HAS_LINE)) {
        lm = textureSample(line_tex, samp, i.uv0 * m.p3.xy + m.p3.zw);
    }
    let sa = item_main_shade(base, dm, lm);
    let shade = sa.rgb;

    var t = vec2<f32>(0.0);
    if (has(HAS_NORMAL)) {
        t = textureSample(over1_tex, samp, i.uv0 * m.line.xy + m.line.zw).ag * 2.0 - 1.0;
    }
    let face = normalize(i.normal) * select(-1.0, 1.0, front);
    let n = normalize(sqrt(1.0 - min(dot(t, t), 1.0)) * face + t.x * i.tangent + t.y * i.bitangent);
    let l = normalize(g.light_dir.xyz);
    let view = normalize(g.eye.xyz - i.world);
    let ndl = dot(l, n);
    let ndh = max(dot(n, normalize(view + l)), 0.0);
    let ndv = dot(n, view);

    // Light amount: the scene ramp, the material's own where asked, and the
    // shadow mask.
    let ramp = mix(item_ramp(ndl), item_another_ramp(ndh), sa.w) * shadow_mask(i);
    var rgb = mix(base * shade, base, ramp);

    // Highlight: the shine painted in the detail mask, or a plain one.
    var shine = 0.0;
    if (has(HAS_DETAIL)) {
        let shift = (m.over2.x - 1.0) * 0.899999976;
        let uv = shift * vec2<f32>(dot(i.tangent, view), dot(i.bitangent, view)) + i.uv0;
        shine = textureSample(detail_tex, samp, uv * m.p2.xy + m.p2.zw).r;
    }
    let power = m.over2.y;
    var glint = 0.0;
    if (power > 0.0) {
        glint = (pow(ndh, power * 256.0) * 5.0 - 4.0) * power;
    }
    let textured = clamp(glint + min(shine * shine, 1.0), 0.0, 1.0);
    let plain = clamp(glint, 0.0, 1.0) * dm.r;
    rgb += mix(textured, plain, m.over2.w);

    // Rim towards the silhouette, tinted by the hue of the scene shadow.
    let fresnel = max(1.0 - max(ndv, 0.0), 1e-6);
    let rim = clamp(pow(fresnel, m.over3.x * 9.0 + 1.0) * 2.5 - 0.5, 0.0, 1.0) * m.over3.y
        * ((1.0 - dm.b) * 9.99999809 - 8.99999809);
    let hue = rgb_to_hsv(g.ambient_shadow.rgb).x;
    let rim_rgb = rim * (hsv_to_rgb(vec3<f32>(hue, 1.0, 1.0)) * 0.400000006 + 0.300000012);
    rgb += clamp((1.0 - dm.g) * rim_rgb, vec3<f32>(0.0), vec3<f32>(0.5));

    rgb *= max(light_color(), g.ambient_shadow.rgb);
    // Shadows painted into the detail mask.
    let extend = m.p0.y * -1.20000005 + 1.0;
    let drawn = (1.0 - dm.g) * (1.0 - extend) + extend;
    rgb *= drawn * (1.0 - shade) + shade;
    rgb *= select(0.660000026, 1.0, front);

    // Lines painted into the masks; the light does not reach them.
    let lines = item_lines(base * g.line_color.rgb * shade, vec3<f32>(1.0));
    rgb = mix(rgb, lines, mix(lm.g, dm.b, m.p1.x));
    return vec4<f32>(rgb, ba.a * 2.0 - 1.0);
}

// Outline pass of main_item (programs 5 and 6): the shaded base colour
// pulled toward black or white by the scene's line depth.
fn item_main_outline(i: VsOut) -> vec4<f32> {
    let base = item_main_base(i).rgb;
    var dm = vec4<f32>(0.0);
    if (has(HAS_DETAIL)) {
        dm = textureSample(detail_tex, samp, i.uv0 * m.p2.xy + m.p2.zw);
    }
    var lm = vec4<f32>(0.0);
    if (has(HAS_LINE)) {
        lm = textureSample(line_tex, samp, i.uv0 * m.p3.xy + m.p3.zw);
    }
    let shade = item_main_shade(base, dm, lm).rgb;
    return vec4<f32>(item_lines(base * shade, vec3<f32>(1.0)), 1.0);
}

// The item shaders below are ported from the game's forward base programs
// (the directional variants with and without the shadow mask, which differ
// by that term only). Left out in all of them: the scale and offset of the
// two ramps.

// A colour's hue at half its saturation and full brightness.
fn hue_tint(c: vec3<f32>) -> vec3<f32> {
    let hsv = rgb_to_hsv(c);
    return hsv_to_rgb(vec3<f32>(hsv.x, hsv.y * 0.5, 1.0));
}

// Multiplier for the unlit side as main_emblem and main_opaque_low blend
// it: shadow_factor_of() with their constants.
fn shadow_factor_low(tint: vec3<f32>) -> vec3<f32> {
    let scene = scene_shadow();
    let dark = tint * scene * 1.60000002;
    let light = 1.0 - (1.0 - 2.0 * (tint * 0.800000012 - 0.5)) * (1.0 - scene);
    return clamp(select(dark, light, tint > vec3<f32>(0.625)), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Normal of the accessory shaders, whose normal map is placed by m.line.
fn acc_normal(i: VsOut, front: bool) -> vec3<f32> {
    var t = vec2<f32>(0.0);
    if (has(HAS_NORMAL)) {
        t = textureSample(over1_tex, samp, i.uv0 * m.line.xy + m.line.zw).ag * 2.0 - 1.0;
    }
    let face = normalize(i.normal) * select(-1.0, 1.0, front);
    return normalize(sqrt(1.0 - min(dot(t, t), 1.0)) * face + t.x * i.tangent + t.y * i.bitangent);
}

// The three colours placed by the colour mask (m.st_alpha places it).
fn acc_tint(i: VsOut, c1: vec3<f32>, c2: vec3<f32>, c3: vec3<f32>) -> vec3<f32> {
    if (!has(HAS_COLOR_MASK)) {
        return vec3<f32>(1.0);
    }
    let cm = textureSample(color_mask_tex, samp, i.uv0 * m.st_alpha.xy + m.st_alpha.zw);
    return mix(mix(cm.r * (c1 - 1.0) + 1.0, c2, cm.g), c3, cm.b);
}

// Rim towards the silhouette of main_item_emission and main_item_studio,
// tinted by the hue of the scene shadow.
fn acc_rim(ndv: f32, dm: vec4<f32>) -> vec3<f32> {
    let fresnel = max(1.0 - max(ndv, 0.0), 1e-6);
    let rim = clamp(pow(fresnel, m.over3.x * 9.0 + 1.0) * 2.5 - 0.5, 0.0, 1.0) * m.over3.y
        * ((1.0 - dm.b) * 9.99999809 - 8.99999809);
    let hue = rgb_to_hsv(g.ambient_shadow.rgb).x;
    let rim_rgb = rim * (hsv_to_rgb(vec3<f32>(hue, 1.0, 1.0)) * 0.400000006 + 0.300000012);
    return clamp((1.0 - dm.g) * rim_rgb, vec3<f32>(0.0), vec3<f32>(0.5));
}

// Lines of main_item_emission and main_item_studio: the shaded base under
// the scene's line colour, overlaid with the material's line colour
// (m.ext[0]) pulled toward black or white by the scene's line depth.
fn acc_lines(base: vec3<f32>, shade: vec3<f32>) -> vec3<f32> {
    let light = light_color();
    let under = base * g.line_color.rgb * shade * light;
    let top = overlay(m.ext[0].rgb, vec3<f32>(g.line_color.a));
    return overlay(under, top) * light;
}

// The unlit side of main_item_emission and main_item_studio: the hue tint
// of the base times the shadow colour, weakened where the material's own
// ramp is in use. Returns the factor and, in w, the share of that ramp.
fn acc_shade(base: vec3<f32>, dm: vec4<f32>, lm: vec4<f32>) -> vec4<f32> {
    let line_r = mix(lm.r, dm.r, m.p0.w);
    let another = mix(line_r, 1.0, m.over1.x);
    let factor = shadow_factor_of(hue_tint(base) * m.shadow.rgb);
    let shade = factor * (clamp(2.0 - m.p0.z - another, 0.0, 1.0) * 0.670000017 + 0.330000013);
    return vec4<f32>(shade, another);
}

// Detail and line mask of the accessory shaders (m.p2 and m.p3 place them).
fn acc_detail(i: VsOut) -> vec4<f32> {
    if (!has(HAS_DETAIL)) {
        return vec4<f32>(0.0);
    }
    return textureSample(detail_tex, samp, i.uv0 * m.p2.xy + m.p2.zw);
}

fn acc_line_mask(i: VsOut) -> vec4<f32> {
    if (!has(HAS_LINE)) {
        return vec4<f32>(0.0);
    }
    return textureSample(line_tex, samp, i.uv0 * m.p3.xy + m.p3.zw);
}

// Texture and alpha of the accessory shaders: twice the alpha, cut at one
// half.
fn acc_main(i: VsOut) -> vec4<f32> {
    var main = vec4<f32>(1.0);
    if (has(HAS_MAIN)) {
        main = textureSample(main_tex, samp, i.uv0 * m.st_main.xy + m.st_main.zw);
    }
    let a = clamp(main.a + main.a, 0.0, 1.0);
    if (a - 0.5 < 0.0) {
        discard;
    }
    return vec4<f32>(main.rgb, a);
}

// main_emblem (programs 12 and 15): the texture shaded by the scene ramp
// on the mesh normal. The pass culls back faces. m.shadow is not used: the
// shader declares _shadowcolor and does not read it.
fn item_emblem(i: VsOut, front: bool) -> vec4<f32> {
    // The default texture is clear, which the cut below removes.
    if (!front || !has(HAS_MAIN)) {
        discard;
    }
    let main = textureSample(main_tex, samp, i.uv0 * m.st_main.xy + m.st_main.zw);
    if (round(pow(main.a, 0.300000012)) - 0.5 < 0.0) {
        discard;
    }
    let factor = shadow_factor_low(hue_tint(main.rgb));
    let ndl = dot(normalize(g.light_dir.xyz), normalize(i.normal));
    let lit = item_ramp(ndl) * shadow_mask(i);
    let rgb = mix(main.rgb * factor, main.rgb, lit);
    return vec4<f32>(rgb * max(light_color(), g.ambient_shadow.rgb), 1.0);
}

// main_color (program 0): the material colour, unlit. The pass culls back
// faces.
fn item_color(front: bool) -> vec4<f32> {
    if (!front) {
        discard;
    }
    return vec4<f32>(m.color1.rgb, 1.0);
}

// main_item_ditherd cuts by an ordered 4 by 4 pattern over the pixels
// (programs 1, 5 and 8): true where the pixel goes. m.p0.x: _Cutoff.
// The game counts the rows from the bottom when what it draws into is
// upside down for Direct3D; here they count from the top.
fn dither_cut(pixel: vec2<f32>, alpha: f32) -> bool {
    var table = array<f32, 16>(
        1.0, 9.0, 3.0, 11.0,
        13.0, 5.0, 15.0, 7.0,
        4.0, 12.0, 2.0, 10.0,
        16.0, 8.0, 14.0, 6.0,
    );
    let x = u32(floor(pixel.x)) & 3u;
    let y = u32(floor(pixel.y)) & 3u;
    let d = table[x * 4u + y] / 17.0;
    return round(d + (alpha - 0.5 * m.p0.x) - 1.5) < 0.0;
}

// main_item_ditherd (program 5): texture times colour, lit by the light
// colour only.
fn item_ditherd(i: VsOut) -> vec4<f32> {
    var main = vec4<f32>(1.0);
    if (has(HAS_MAIN)) {
        main = textureSample(main_tex, samp, i.uv0 * m.st_main.xy + m.st_main.zw);
    }
    if (dither_cut(i.clip.xy, main.a)) {
        discard;
    }
    return vec4<f32>(main.rgb * m.color1.rgb * light_color(), 1.0);
}

// main_texture (program 1): main_item without light direction, ramp,
// highlight, rim and lines. Its alpha is what main_item writes.
fn item_texture(i: VsOut) -> vec4<f32> {
    let ba = item_main_base(i);
    let dm = acc_detail(i);
    let lm = acc_line_mask(i);
    let shade = item_main_shade(ba.rgb, dm, lm).rgb;
    let extend = m.p0.y * -1.20000005 + 1.0;
    let drawn = (1.0 - dm.g) * (1.0 - extend) + extend;
    let rgb = ba.rgb * max(light_color(), g.ambient_shadow.rgb) * (drawn * (1.0 - shade) + shade);
    return vec4<f32>(rgb, ba.a * 2.0 - 1.0);
}

// Base colour of main_item_emission. The colours are overlaid with the
// animation mask (the alpha mask slot, placed by m.ext[2]) held to the
// detail mask's red, and to no less than one half minus half of that red.
// Left out: the scroll _UPanner and _VPanner (m.ext[1].yz) give the mask
// over time; the picture is taken at a time of zero.
fn item_emission_base(i: VsOut, main: vec3<f32>, dm: vec4<f32>) -> vec3<f32> {
    let tint = acc_tint(i, m.color1.rgb, m.color2.rgb, m.color3.rgb);
    var anim = 1.0;
    if (has(HAS_ALPHA_MASK)) {
        anim = textureSample(alpha_mask_tex, samp, i.uv0 * m.ext[2].xy + m.ext[2].zw).r;
    }
    let e = max(min(anim, dm.r), dm.r * -0.5 + 0.5);
    return main * overlay(vec3<f32>(e), tint);
}

// main_item_emission (programs 10 and 11). m.over1.x: second ramp
// everywhere. m.over2.w: no shine texture. m.over3: rim power, rim
// strength. m.ext[1].x: emission power.
fn item_emission(i: VsOut, front: bool) -> vec4<f32> {
    let main = acc_main(i);
    let dm = acc_detail(i);
    let lm = acc_line_mask(i);
    let base = item_emission_base(i, main.rgb, dm);
    let sa = acc_shade(base, dm, lm);
    let shade = sa.rgb;

    let n = acc_normal(i, front);
    let l = normalize(g.light_dir.xyz);
    let view = normalize(g.eye.xyz - i.world);
    let ndh = max(dot(n, normalize(view + l)), 0.0);

    // Light amount: the scene ramp, held under the material's own where
    // that is in use, and the shadows painted into the masks.
    let lit = min(item_ramp(dot(l, n)), max(1.0 - sa.w, item_another_ramp(ndh))) * shadow_mask(i);
    let drawn = min(1.0 - lm.b, 1.0 - dm.g) * m.p0.y + (1.0 - m.p0.y);
    var rgb = mix(base * shade, base, drawn * lit);
    rgb += acc_rim(dot(n, view), dm);
    rgb += min(mix(lm.r, dm.r, m.p0.w), 1.0 - m.over2.w);
    rgb *= max(light_color(), g.ambient_shadow.rgb);
    rgb *= select(0.660000026, 1.0, front);
    rgb *= drawn * (1.0 - shade) + shade;

    rgb = mix(rgb, acc_lines(base, shade), mix(lm.g, dm.b, m.p1.x));
    let power = m.ext[1].x;
    let glow = dm.r * base * (1.0 - dm.g) * power;
    return vec4<f32>(glow * (power * 3.0) + rgb, main.a * 2.0 - 1.0);
}

// Outline pass of main_item_emission (programs 3 and 12).
fn item_emission_outline(i: VsOut) -> vec4<f32> {
    let main = acc_main(i);
    let dm = acc_detail(i);
    let base = item_emission_base(i, main.rgb, dm);
    let shade = acc_shade(base, dm, acc_line_mask(i)).rgb;
    return vec4<f32>(acc_lines(base, shade), 1.0);
}

// One pattern of main_item_studio: where its mask is read. `place` is
// _Patternuv (offset, then scale about the middle), `turn` the rotation in
// half turns, `hold` keeps the lookup inside the mask.
fn pattern_uv(uv0: vec2<f32>, place: vec4<f32>, turn: f32, hold: f32, st: vec4<f32>) -> vec2<f32> {
    let p = uv0 + place.xy - 0.5;
    let s = sin(turn * 3.14159274);
    let c = cos(turn * 3.14159274);
    var q = vec2<f32>(dot(p, vec2<f32>(c, s)), dot(p, vec2<f32>(-s, c))) + 0.5;
    q = q * place.zw + (place.zw - 1.0) * -0.5;
    q = mix(q, clamp(q, vec2<f32>(0.0), vec2<f32>(1.0)), hold);
    return q * st.xy + st.zw;
}

// The colour a pattern gives: `top` where the mask is red or clear, the
// second colour elsewhere, as far as that colour's alpha goes.
fn pattern_color(pm: vec4<f32>, second: vec4<f32>, top: vec3<f32>) -> vec3<f32> {
    let w = clamp(max(1.0 - pm.a, pm.r) + (1.0 - second.a), 0.0, 1.0);
    return mix(second.rgb, top, w);
}

// Base colour of main_item_studio: each of the three colours carries a
// pattern. The masks sit in the alpha mask slot, the third layer slot and
// the ninth slot (m.ext[2..4] place them); m.ext[5..7] are the second
// colours, m.ext[8..10] _Patternuv1..3, m.ext[11] and m.ext[12] the
// rotation and clamp of each.
fn item_studio_base(i: VsOut, main: vec3<f32>) -> vec3<f32> {
    var pm1 = vec4<f32>(1.0);
    if (has(HAS_ALPHA_MASK)) {
        pm1 = textureSample(alpha_mask_tex, samp, pattern_uv(i.uv0, m.ext[8], m.ext[11].x, m.ext[11].y, m.ext[2]));
    }
    var pm2 = vec4<f32>(1.0);
    if (has(HAS_OVER3)) {
        pm2 = textureSample(over3_tex, samp, pattern_uv(i.uv0, m.ext[9], m.ext[11].z, m.ext[11].w, m.ext[3]));
    }
    var pm3 = vec4<f32>(1.0);
    if (has(HAS_PATTERN3)) {
        pm3 = textureSample(over4_tex, samp, pattern_uv(i.uv0, m.ext[10], m.ext[12].x, m.ext[12].y, m.ext[4]));
    }
    let c1 = pattern_color(pm1, m.ext[5], m.color1.rgb);
    let c2 = pattern_color(pm2, m.ext[6], m.color2.rgb);
    let c3 = pattern_color(pm3, m.ext[7], m.color3.rgb);
    return main * acc_tint(i, c1, c2, c3);
}

// main_item_studio (programs 1 and 5). m.over1.x: second ramp everywhere.
// m.over2: shine height, specular power, unused, no shine texture.
// m.over3: rim power, rim strength. m.ext[1].x: emission power.
fn item_studio(i: VsOut, front: bool) -> vec4<f32> {
    let main = acc_main(i);
    let base = item_studio_base(i, main.rgb);
    let dm = acc_detail(i);
    let lm = acc_line_mask(i);
    let sa = acc_shade(base, dm, lm);
    let shade = sa.rgb;

    let n = acc_normal(i, front);
    let l = normalize(g.light_dir.xyz);
    let view = normalize(g.eye.xyz - i.world);
    let ndh = max(dot(n, normalize(view + l)), 0.0);

    // Light amount: the scene ramp, the material's own where asked, and
    // the shadows painted into the masks.
    let lit = mix(item_ramp(dot(l, n)), item_another_ramp(ndh), sa.w) * shadow_mask(i);
    let drawn = min(1.0 - lm.b, 1.0 - dm.g) * m.p0.y + (1.0 - m.p0.y);
    var rgb = mix(base * shade, base, drawn * lit);

    // Highlight: the shine painted in the detail mask, or a plain one.
    var shine = 0.0;
    if (has(HAS_DETAIL)) {
        let shift = (m.over2.x - 1.0) * 0.899999976;
        let uv = shift * vec2<f32>(dot(i.tangent, view), dot(i.bitangent, view)) + i.uv0;
        shine = textureSample(detail_tex, samp, uv * m.p2.xy + m.p2.zw).r;
    }
    let power = m.over2.y;
    var glint = 0.0;
    if (power > 0.0) {
        glint = (pow(ndh, power * 256.0) * 5.0 - 4.0) * power;
    }
    let textured = clamp(glint + min(shine * shine, 1.0), 0.0, 1.0);
    let plain = clamp(glint, 0.0, 1.0) * dm.r;
    rgb += mix(textured, plain, m.over2.w);

    rgb += acc_rim(dot(n, view), dm);
    rgb *= max(light_color(), g.ambient_shadow.rgb);
    rgb *= select(0.660000026, 1.0, front);
    rgb *= drawn * (1.0 - shade) + shade;

    rgb = mix(rgb, acc_lines(base, shade), mix(lm.g, dm.b, m.p1.x));
    let glow = min(min(min(base, vec3<f32>(dm.r)), vec3<f32>(m.ext[1].x)), vec3<f32>(1.0 - dm.g));
    return vec4<f32>(glow * (m.ext[1].x * 3.0) + rgb, main.a * 2.0 - 1.0);
}

// Outline pass of main_item_studio (programs 0 and 12).
fn item_studio_outline(i: VsOut) -> vec4<f32> {
    let main = acc_main(i);
    let base = item_studio_base(i, main.rgb);
    let shade = acc_shade(base, acc_detail(i), acc_line_mask(i)).rgb;
    return vec4<f32>(acc_lines(base, shade), 1.0);
}

// main_opaque_low (programs 5 and 6): main_opaque without highlight shape,
// rim and line pass. The three colours tint by the colour mask; the detail
// mask's blue draws lines with the scene's line depth. m.ext[0..2] place
// the colour mask, the detail mask and the normal map. m.over2.y:
// specular power, m.p0.w: detail red switch (the two multiply the detail
// mask's red into an added shine).
fn item_opaque_low(i: VsOut, front: bool) -> vec4<f32> {
    let main = item_main(i);
    if (min(item_mask(i), main.a) - 0.5 < 0.0) {
        discard;
    }
    let factor = shadow_factor_low(hue_tint(main.rgb * m.shadow.rgb));
    var tint = vec3<f32>(1.0);
    if (has(HAS_COLOR_MASK)) {
        let cm = textureSample(color_mask_tex, samp, i.uv0 * m.ext[0].xy + m.ext[0].zw);
        tint = mix(mix(cm.r * (m.color1.rgb - 1.0) + 1.0, m.color2.rgb, cm.g), m.color3.rgb, cm.b);
    }
    var dm = vec4<f32>(0.0);
    if (has(HAS_DETAIL)) {
        dm = textureSample(detail_tex, samp, i.uv0 * m.ext[1].xy + m.ext[1].zw);
    }
    let base = main.rgb * tint * (dm.b * (g.line_color.a - 1.0) + 1.0);
    let dark = base * factor;

    var t = vec2<f32>(0.0);
    if (has(HAS_NORMAL)) {
        t = textureSample(over1_tex, samp, i.uv0 * m.ext[2].xy + m.ext[2].zw).ag * 2.0 - 1.0;
    }
    let face = normalize(i.normal) * select(-1.0, 1.0, front);
    let n = normalize(sqrt(1.0 - min(dot(t, t), 1.0)) * face + t.x * i.tangent + t.y * i.bitangent);
    let ndl = dot(normalize(g.light_dir.xyz), n);

    let extend = m.p0.y * -1.20000005 + 1.0;
    let drawn = (1.0 - dm.g) * (1.0 - extend) + extend;
    let lit = item_ramp(ndl) * shadow_mask(i) * drawn * select(0.5, 1.0, front);
    var rgb = mix(dark, base, lit) + m.p0.w * (dm.r * m.over2.y);
    rgb *= max(light_color(), g.ambient_shadow.rgb);
    rgb *= drawn * (1.0 - factor) + factor;
    return vec4<f32>(rgb, 1.0);
}

@fragment
fn fs_main(i: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if (m.info.z >= XK_FIRST) {
        return xk_forward(i, front);
    }
    if (m.info.x == KIND_ITEM && m.info.z == ITEM_GLASSES) {
        return item_glasses(i, front);
    }
    if (m.info.x == KIND_ITEM) {
        switch m.info.z {
            case ITEM_EMBLEM: {
                return item_emblem(i, front);
            }
            case ITEM_EMISSION: {
                return item_emission(i, front);
            }
            case ITEM_DITHERD: {
                return item_ditherd(i);
            }
            case ITEM_STUDIO: {
                return item_studio(i, front);
            }
            case ITEM_TEXTURE: {
                return item_texture(i);
            }
            case ITEM_OPAQUE_LOW: {
                return item_opaque_low(i, front);
            }
            case ITEM_COLOR: {
                return item_color(front);
            }
            default: {
            }
        }
    }
    if (m.info.x == KIND_ITEM && m.info.z == ITEM_MAIN) {
        return item_main_forward(i, front);
    }
    if (m.info.x == KIND_ITEM && m.info.z == ITEM_STUDIO_ALPHA) {
        return item_studio_alpha(i, front);
    }
    if (m.info.x == KIND_ITEM && m.info.z == ITEM_ALPHA) {
        return item_alpha(i, front);
    }
    if (m.info.x == KIND_ITEM && m.info.z == ITEM_OPAQUE) {
        return item_opaque(i, front);
    }
    return shade(i, front);
}

// Outline pass of main_alpha (pixel program 5). The pass culls front faces,
// so every pixel is an inside one and gets the forward pass's cut for the
// inside: nothing is drawn while the outline switch is on.
fn item_alpha_outline(i: VsOut) -> vec4<f32> {
    let main = item_main(i);
    let alpha = min(item_mask(i), main.a);
    if (min(1.0 - m.over3.z, alpha * 10.0) < 0.5) {
        discard;
    }
    let factor = item_shadow_factor(main.rgb);
    return vec4<f32>(item_lines(main.rgb * g.line_color.rgb * factor, light_color()), 1.0);
}

@fragment
fn fs_outline(i: VsOut) -> @location(0) vec4<f32> {
    if (m.info.z >= XK_FIRST) {
        return xk_outline(i);
    }
    if (m.info.x == KIND_ITEM && m.info.z == ITEM_MAIN) {
        return item_main_outline(i);
    }
    if (m.info.x == KIND_ITEM && m.info.z == ITEM_EMISSION) {
        return item_emission_outline(i);
    }
    if (m.info.x == KIND_ITEM && m.info.z == ITEM_STUDIO) {
        return item_studio_outline(i);
    }
    if (m.info.x == KIND_ITEM && m.info.z == ITEM_ALPHA) {
        return item_alpha_outline(i);
    }
    return outline_color(i);
}

// Shadow caster pass. The game draws it into the light's shadow map and,
// seen from the camera, into the depth texture the shadow mask is built
// from.
struct CasterOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv0: vec2<f32>,
};

// After the shaders' SHADOWCASTER vertex programs (UnityApplyLinearShadowBias;
// they apply no normal bias). Depth here grows away from the light, where
// the game's shrinks.
@vertex
fn vs_caster(v: VsIn) -> CasterOut {
    var o: CasterOut;
    var pos = v.pos;
    if (has(CASTER_FALLBACK)) {
        // VertexLit's pass also shrinks the mesh along its normals
        // (UnityClipSpaceShadowCasterPos), most where the light grazes.
        let n = normalize(v.normal);
        let c = dot(n, normalize(g.light_dir.xyz));
        pos -= n * (g.shadow.w * sqrt(max(1.0 - c * c, 0.0)));
    }
    var clip = g.view_proj * vec4<f32>(pos, 1.0);
    clip.z += clamp(g.shadow.x / clip.w, 0.0, 1.0);
    clip.z = mix(clip.z, max(clip.z, 0.0), g.shadow.y);
    o.clip = clip;
    o.uv0 = v.uv0;
    return o;
}

// What each shader's SHADOWCASTER pixel program keeps: at least one half.
@fragment
fn fs_caster(i: CasterOut, @builtin(front_facing) front: bool) {
    let kind = m.info.x;
    let item = m.info.z;
    var main_a = 1.0;
    if (has(HAS_MAIN)) {
        main_a = textureSample(main_tex, samp, i.uv0 * m.st_main.xy + m.st_main.zw).a;
    }
    var keep = 1.0;
    if (has(CASTER_FALLBACK)) {
        // Legacy Shaders/VertexLit cuts nothing out.
        keep = 1.0;
    } else if (item >= XK_FIRST) {
        keep = select(0.0, 1.0, xk_casts(i.uv0, main_a));
    } else if (kind == KIND_SKIN) {
        // main_skin: the body masks only.
        keep = body_mask(i.uv0);
    } else if (kind == KIND_HAIR) {
        // main_hair, main_hair_front: alpha mask times texture alpha.
        var mask = 1.0;
        if (has(HAS_ALPHA_MASK)) {
            mask = textureSample(alpha_mask_tex, samp, i.uv0 * m.st_alpha.xy + m.st_alpha.zw).r;
        }
        keep = mask * main_a;
    } else if (kind == KIND_ITEM && item == ITEM_ALPHA) {
        // main_alpha: as its forward pass, without the inside when the
        // outline switch is on.
        let side = select(1.0 - m.over3.z, 1.0, front);
        keep = min(side, min(body_mask(i.uv0), main_a) * 10.0);
    } else if (kind == KIND_ITEM && (item == ITEM_OPAQUE || item == ITEM_OPAQUE_LOW)) {
        keep = min(body_mask(i.uv0), main_a);
    } else if (kind == KIND_ITEM && item == ITEM_EMBLEM) {
        // main_emblem: the texture alpha to the power 0.3, rounded. Its
        // default texture is clear.
        keep = select(0.0, round(pow(main_a, 0.300000012)), has(HAS_MAIN));
    } else if (kind == KIND_ITEM && item == ITEM_DITHERD) {
        // main_item_ditherd: the forward pass's pattern, over the pixels
        // of the map being drawn.
        keep = select(1.0, 0.0, dither_cut(i.clip.xy, main_a));
    } else if (kind == KIND_ITEM && item == ITEM_STUDIO_ALPHA) {
        // main_item_studio_alpha: anything not fully clear.
        keep = select(1.0, 0.0, main_a * m.over1.w * 0.500999987 - 0.000999987125 < 0.0);
    } else if (kind == KIND_ITEM) {
        // main_item, main_item_emission, main_item_studio, main_texture:
        // twice the texture alpha.
        keep = clamp(main_a + main_a, 0.0, 1.0);
    } else {
        // toon_eyew.
        keep = main_a;
    }
    if (keep - 0.5 < 0.0) {
        discard;
    }
}
