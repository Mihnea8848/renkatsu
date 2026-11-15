// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

// The KKShadersPlus ("Vanilla Plus") shaders that MaterialEditor puts on
// cards. Ported from the disassembly of the programs in Vanilla Plus 1.7.2
// (vanillaplus.unity3d): the forward base variants for one directional light
// without point lights, with and without the shadow mask, the outline pass
// and the shadow caster. Program numbers count the Direct3D 11 programs of
// each shader in the order the bundle stores them. The names follow the KKShadersPlus source (MIT, by
// xukmi), which this code follows where the installed build agrees with it.
// This file is appended to toon.wgsl.
//
// Left out everywhere: the liquid layers (main_skin in toon.wgsl has them), the
// point light terms (the scene has none; the programs mask unused lights
// out), the reflection probe and matcap blend (GetBlendReflections, which
// weighs by 1 - _UseKKMetal and is off at the default of one) and the turn
// _Clock gives over time.
//
// Texture slots: main_tex _MainTex, color_mask_tex _ColMask or _ColorMask,
// detail_tex _DetailMask, line_tex _LineMask, alpha_mask_tex _AlphaMask,
// over1_tex to over3_tex what the kind says, over4_tex _EmissionMask,
// over5_tex _NormalMap, over6_tex _NormalMapDetail.
//
// Uniform, shared by all kinds:
// st_main, st_alpha: placement of _MainTex and _AlphaMask
// p0.x _Cutoff, p1.y _LineWidthS, p2.xy _alpha_a and _alpha_b
// p3: placement of _DetailMask
// line: _OutlineColor
// ext[1]: _CustomAmbient, w _UseRampForShadows
// ext[2]: _KKPRimColor
// ext[3]: _EmissionColor
// ext[4]: _ShadowHSV, w _UseRampForSpecular
// ext[5]: _UseDetailRAsSpecularMap, _UseKKPRim, _NormalMapScale,
//         _DetailNormalMapScale
// ext[6]: _KKPRimSoft, _KKPRimIntensity, _KKPRimAsDiffuse, _KKPRimRotateX
// ext[7]: _KKPRimRotateY, _SpecularNormalScale, _SpecularDetailNormalScale,
//         _AdjustBackfaceNormals
// ext[8]: _rimReflectMode, _EmissionIntensity, _EmissionMaskMode,
//         _EmissionKeepCol
// ext[9..15]: placement of _NormalMap, _NormalMapDetail, _LineMask, the
//         colour mask, _EmissionMask, _AnotherRamp and _RampG
// ext[16]: _Saturation, _CullOption, _OutlineOn, _UseLightColorSpecular
// The rest is per kind and listed at each.

// Which KKShadersPlus shader a material follows (info.z).
const XK_FIRST: u32 = 32u;
const XK_OPAQUE: u32 = 32u;
const XK_ALPHA: u32 = 33u;
const XK_ITEM: u32 = 34u;
const XK_ITEM_ALPHA: u32 = 35u;
const XK_SKIN: u32 = 36u;
const XK_HAIR: u32 = 37u;
const XK_HAIR_FRONT: u32 = 38u;
const XK_EYE: u32 = 39u;
const XK_EYEW: u32 = 40u;

// The ninth to eleventh texture slot.
const HAS_OVER4: u32 = 1024u;
const HAS_OVER5: u32 = 2048u;
const HAS_OVER6: u32 = 4096u;

const XK_LUMA: vec3<f32> = vec3<f32>(0.212599993, 0.715200007, 0.0722000003);
const XK_TINY: vec3<f32> = vec3<f32>(9.99999997e-07);

fn xk_is(which: u32) -> bool {
    return m.info.z == which;
}

// _Saturation: towards the grey of the same brightness.
fn xk_saturation(c: vec3<f32>) -> vec3<f32> {
    let s = m.ext[16].x;
    return c * s + dot(c, XK_LUMA) * (1.0 - s);
}

// _LightColor0 * 0.6 + _CustomAmbient.
fn xk_light() -> vec3<f32> {
    return vec3<f32>(g.params.y * 0.600000024) + m.ext[1].rgb;
}

// The shadow mask as these shaders read it: saturate(mask * 2 - 1).
fn xk_atten(i: VsOut) -> f32 {
    return clamp(shadow_mask(i) * 2.0 - 1.0, 0.0, 1.0);
}

// _RampG at a light amount, placed by _RampG_ST.
fn xk_ramp(x: f32) -> f32 {
    return textureSample(ramp_tex, ramp_samp, vec2<f32>(x) * m.ext[15].xy + m.ext[15].zw).r;
}

// _AnotherRamp (over2_tex), white without one. The lookup stops at the
// outermost texels, as a clamped texture does.
fn xk_another_ramp(x: f32) -> f32 {
    let half = 0.5 / vec2<f32>(textureDimensions(over2_tex));
    let uv = clamp(vec2<f32>(x) * m.ext[14].xy + m.ext[14].zw, half, 1.0 - half);
    return select(1.0, textureSample(over2_tex, samp, uv).r, has(HAS_OVER2));
}

// Multiplier for the unlit side from a shade tint, before any clamp.
fn xk_factor(tint: vec3<f32>) -> vec3<f32> {
    let scene = scene_shadow();
    let dark = tint * scene * 1.79999995;
    let light = 1.0 - (1.0 - 2.0 * (tint * 0.899999976 - 0.5)) * (1.0 - scene);
    return select(dark, light, tint > vec3<f32>(0.555555582));
}

// What a _ShadowColor alpha below one adds: the shade pulled to the colour.
fn xk_shadow_alpha(c: vec4<f32>) -> vec3<f32> {
    let a = clamp(c.a + 9.99999997e-07, 0.0, 1.0);
    return (1.0 - a) * (c.rgb - 0.999998987) + 1.0;
}

// The detail and line mask where the material has them; both default to
// black.
fn xk_detail(i: VsOut, uv: vec2<f32>) -> vec4<f32> {
    return select(vec4<f32>(0.0), textureSample(detail_tex, samp, uv * m.p3.xy + m.p3.zw), has(HAS_DETAIL));
}

fn xk_line_mask(i: VsOut) -> vec4<f32> {
    let uv = i.uv0 * m.ext[11].xy + m.ext[11].zw;
    return select(vec4<f32>(0.0), textureSample(line_tex, samp, uv), has(HAS_LINE));
}

fn xk_main(i: VsOut) -> vec4<f32> {
    let uv = i.uv0 * m.st_main.xy + m.st_main.zw;
    return select(vec4<f32>(1.0), textureSample(main_tex, samp, uv), has(HAS_MAIN));
}

// The body masks of clothes; the red channel alone goes out in y.
fn xk_body_mask(uv0: vec2<f32>) -> vec2<f32> {
    let am = select(vec4<f32>(1.0), textureSample(alpha_mask_tex, samp, uv0 * m.st_alpha.xy + m.st_alpha.zw), has(HAS_ALPHA_MASK));
    return vec2<f32>(min(max(am.r, 1.0 - m.p2.x), max(am.g, 1.0 - m.p2.y)), am.r);
}

// _CullOption: 1 leaves front faces out, 2 back faces.
fn xk_culled(front: bool) -> bool {
    let cull = m.ext[16].y;
    return (cull == 1.0 && front) || (cull == 2.0 && !front);
}

// Tangent space normal from the two maps (x in alpha, y in green) with
// their scales, blended as BlendNormals does.
fn xk_blend_normals(a: vec2<f32>, d: vec2<f32>, scale: f32, detail_scale: f32) -> vec3<f32> {
    let na = a * scale;
    let nd = d * detail_scale;
    let za = sqrt(1.0 - min(dot(na, na), 1.0));
    let zd = sqrt(1.0 - min(dot(nd, nd), 1.0));
    return normalize(vec3<f32>(na + nd, za * zd));
}

// From tangent space to the world. Back faces keep the normal of the front
// unless _AdjustBackfaceNormals is on.
fn xk_to_world(i: VsOut, t: vec3<f32>, front: bool) -> vec3<f32> {
    let n = normalize(t.x * i.tangent + t.y * i.bitangent + t.z * i.normal);
    return select(n, -n, floor(m.ext[7].w) != 0.0 && !front);
}

struct XkNormals {
    // The shading normal.
    n: vec3<f32>,
    // The one highlights use, with _SpecularNormalScale and
    // _SpecularDetailNormalScale on top.
    spec: vec3<f32>,
};

fn xk_normals(i: VsOut, front: bool) -> XkNormals {
    let ta = textureSample(over5_tex, samp, i.uv0 * m.ext[9].xy + m.ext[9].zw).ag * 2.0 - 1.0;
    let td = textureSample(over6_tex, samp, i.uv0 * m.ext[10].xy + m.ext[10].zw).ag * 2.0 - 1.0;
    let a = select(vec2<f32>(0.0), ta, has(HAS_OVER5));
    let d = select(vec2<f32>(0.0), td, has(HAS_OVER6));
    var o: XkNormals;
    o.n = xk_to_world(i, xk_blend_normals(a, d, m.ext[5].z, m.ext[5].w), front);
    o.spec = xk_to_world(i, xk_blend_normals(a, d, m.ext[5].z * m.ext[7].y, m.ext[5].w * m.ext[7].z), front);
    return o;
}

struct XkRim {
    // The surface colour with the rim colour over it, after _Saturation.
    color: vec3<f32>,
    // How much of it shows: the rim amount times the rim colour's alpha,
    // zero with _UseKKPRim off.
    amount: f32,
};

// The KKP rim: a fresnel term against the view direction turned by
// _KKPRimRotateX and _KKPRimRotateY. `least` is the least the cosine
// may be (the hair programs hold it at 0.1, the others not at all).
fn xk_rim(n: vec3<f32>, view: vec3<f32>, front: bool, base: vec3<f32>, least: f32) -> XkRim {
    let sx = sin(m.ext[6].w);
    let cx = cos(m.ext[6].w);
    let sy = sin(m.ext[7].x);
    let cy = cos(m.ext[7].x);
    let a = select(-view, view, front);
    let turned = vec3<f32>(
        a.x * cx - a.z * sx,
        dot(a, vec3<f32>(sx * sy, cy, cx * sy)),
        dot(a, vec3<f32>(sx * cy, -sy, cx * cy)),
    );
    let f = clamp(pow(max(1.0 - max(dot(n, turned), least), 0.0), m.ext[6].x) * m.ext[6].y, 0.0, 1.0);
    var o: XkRim;
    o.color = xk_saturation((1.0 - f) * base + f * m.ext[2].rgb);
    o.amount = select(0.0, f * m.ext[2].a, m.ext[5].y != 0.0);
    return o;
}

// _rimReflectMode: how the light amount weighs the rims. Zero leaves them
// alone, one keeps them to the lit side, minus one to the unlit side.
fn xk_rim_mode(lit: f32) -> f32 {
    let mode = m.ext[8].x;
    let w = mix(1.0 - lit, 1.0, min(mode + 1.0, 1.0));
    return mix(w, lit, max(mode, 0.0));
}

// _ShadowHSV: the colour shifted in hue, saturation and lightness (HSL)
// where the light amount is below one half. The programs divide by a
// global `Epsilon` that nothing sets; a grey has no hue here, as there.
fn xk_shadow_hsv(c: vec3<f32>, lit: f32) -> vec3<f32> {
    let p = select(vec4<f32>(c.g, c.b, 0.0, -1.0 / 3.0), vec4<f32>(c.b, c.g, -1.0, 2.0 / 3.0), c.g < c.b);
    let q = select(vec4<f32>(c.r, p.y, p.z, p.x), vec4<f32>(p.x, p.y, p.w, c.r), c.r < p.x);
    let d = q.x - min(q.w, q.y);
    let l = q.x - d * 0.5;
    let span = 1.0 - abs(l * 2.0 - 1.0);
    var hue = vec3<f32>(0.0);
    if (d != 0.0) {
        let h = abs(q.z + (q.w - q.y) / (d * 6.0)) + m.ext[4].x;
        hue = clamp(abs(h * 6.0 - vec3<f32>(3.0, 2.0, 4.0)) * vec3<f32>(1.0, -1.0, -1.0) + vec3<f32>(-1.0, 2.0, 2.0), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    let s = select(0.0, d / span, span != 0.0) + m.ext[4].y;
    let l2 = l + m.ext[4].z;
    let shifted = (hue - 0.5) * (s * (1.0 - abs(l2 * 2.0 - 1.0))) + l2;
    return mix(shifted, c, clamp(lit + 0.5, 0.0, 1.0));
}

// GetEmission and how the programs lay it over the colour: mixed in by the
// mask's alpha (keeping the colour under it as far as _EmissionKeepCol
// says), or multiplied on with _EmissionMaskMode.
fn xk_emission(i: VsOut, rgb: vec3<f32>) -> vec3<f32> {
    return xk_emission_at(i.uv0 * m.ext[13].xy + m.ext[13].zw, rgb);
}

fn xk_emission_at(uv: vec2<f32>, rgb: vec3<f32>) -> vec3<f32> {
    let em = select(vec4<f32>(0.0), textureSample(over4_tex, samp, uv), has(HAS_OVER4));
    let glow = m.ext[3].rgb * m.ext[8].y * em.rgb;
    let a = em.a * m.ext[3].a;
    let c = max(rgb, XK_TINY);
    let multiplied = c * glow * a + c;
    let mixed = c * (1.0 - a * (1.0 - m.ext[8].w)) + glow * a;
    return max((1.0 - m.ext[8].z) * mixed + m.ext[8].z * multiplied, XK_TINY);
}

// The rim laid over the finished colour (the share _KKPRimAsDiffuse leaves),
// then the emission.
fn xk_finish(i: VsOut, rgb: vec3<f32>, rim: XkRim, mode: f32) -> vec3<f32> {
    let c = mix(max(rgb, XK_TINY), rim.color, rim.amount * mode * (1.0 - m.ext[6].z));
    return xk_emission(i, c);
}

// The cut the clothes shaders share: the body masks and the texture alpha
// against _Cutoff unless _AlphaOptionCutoff is off. The see-through ones
// go on to their alpha: the texture's, faded by the mask's red towards
// the cutoff when the option is 2 (Smooth), times _Alpha.
// Returns the texture and the alpha in w.
fn xk_cut(i: VsOut, sheer: bool) -> vec4<f32> {
    let main = xk_main(i);
    let mask = xk_body_mask(i.uv0);
    let cutoff = m.p0.x;
    if (min(main.a, mask.x) - cutoff < 0.0 && m.p1.w != 0.0) {
        discard;
    }
    var alpha = 1.0;
    if (sheer) {
        let fade = 1.0 - (mask.y - cutoff + 9.99999975e-05) / (1.00010002 - cutoff);
        alpha = main.a * (1.0 - fade * floor(m.p1.w * 0.5)) * m.p1.z;
        if (alpha <= 0.0) {
            discard;
        }
    }
    return vec4<f32>(main.rgb, alpha);
}

// xukmi/MainOpaquePlus and xukmi/MainAlphaPlus (forward programs 13 and 3
// of the first, 1 and 5 of the second, which adds the alpha). The
// tessellated variants run the same pixel programs.
// color1..3 and ext[0]: _Col0.._Col3, placed by _ColMask. shadow:
// _ShadowColor. over1: _SpecularColor. over2: _SpeclarHeight,
// _SpecularPower, _SpecularPowerNail, _notusetexspecular. over3: _rimpower,
// _rimV, _AnotherRampFull, _UseKKMetal. p0: _Cutoff, _ShadowExtend,
// _ShadowExtendAnother, _DetailRLineR. p1: _DetailBLineG, _LineWidthS,
// _Alpha, _AlphaOptionCutoff.
fn xk_cloth(i: VsOut, front: bool) -> vec4<f32> {
    let cut = xk_cut(i, xk_is(XK_ALPHA));
    if (xk_culled(front)) {
        discard;
    }
    let l = normalize(g.light_dir.xyz);
    let view = normalize(g.eye.xyz - i.world);
    let h = normalize(view + l);

    let cm = select(vec4<f32>(0.0), textureSample(color_mask_tex, samp, i.uv0 * m.ext[12].xy + m.ext[12].zw), has(HAS_COLOR_MASK));
    var col = max(m.color1.rgb, XK_TINY);
    col = mix(col, max(m.color2.rgb, XK_TINY), cm.r);
    col = mix(col, max(m.color3.rgb, XK_TINY), cm.g);
    col = mix(col, max(m.ext[0].rgb, XK_TINY), cm.b);
    let base = cut.rgb * col;
    let tint = hue_tint(base * m.shadow.rgb);

    let nn = xk_normals(i, front);
    let rim = xk_rim(nn.n, view, front, base, -1.0);
    let as_diffuse = rim.amount * m.ext[6].z;
    var d = mix(base, rim.color, as_diffuse);

    let ndl = clamp(dot(l, nn.n), 0.0, 1.0);
    let spec = max(dot(h, nn.spec), 0.0);
    let fresnel = 1.0 - max(dot(view, nn.n), 0.0);
    let att = xk_atten(i);
    let ramp_shadows = m.ext[1].w != 0.0;
    var ramp = xk_ramp(ndl * select(1.0, att, ramp_shadows)) * select(att, 1.0, ramp_shadows);

    let dm = xk_detail(i, i.uv0);
    let lm = xk_line_mask(i);
    var line_r = mix(lm.r, dm.r, m.p0.w);
    line_r = m.over3.z * (1.0 - line_r) + line_r;
    let metal = line_r * m.over3.w;
    ramp = ramp * (1.0 - metal) + metal * xk_another_ramp(spec);

    let extend = m.p0.y * -1.20000005 + 1.0;
    let power = select(1.0, dm.r, m.ext[5].x != 0.0) * m.over2.y;
    let drawn = min(1.0 - lm.b, 1.0 - dm.g) * (1.0 - extend) + extend;
    let lit = ramp * drawn;
    let mode = xk_rim_mode(lit);
    d = mix(d, rim.color, as_diffuse * mode);

    // Shine from the detail mask, read where the view shifts it to, and the
    // darkening that goes with it.
    let shift = (m.over2.x - 1.0) * 0.800000012;
    let shine_uv = shift * vec2<f32>(dot(i.tangent, view), dot(i.bitangent, view)) + i.uv0;
    let shine = xk_detail(i, shine_uv).r;
    let gloss = min(shine * shine, 1.0);
    var deepen = min(clamp(shine - gloss, 0.0, 1.0), power);
    deepen = min(min(deepen, lit), dot(m.over1.rgb, vec3<f32>(0.300000012, 0.589999974, 0.109999999)));

    let factor = clamp(xk_factor(tint) * xk_shadow_alpha(m.shadow), vec3<f32>(0.0), vec3<f32>(1.0));
    let shade = clamp(factor * max(1.0 - metal, 1.0 - m.p0.z), vec3<f32>(0.0), vec3<f32>(1.0));

    // The highlight on the mesh: pow(n.h, 256 p), through the ramp when
    // _UseRampForSpecular says so.
    var mesh = 0.0;
    if (power > 0.0) {
        mesh = pow(spec, power * 256.0);
    }
    mesh = select(mesh, xk_ramp(mesh), m.ext[4].w != 0.0);
    mesh = clamp(mesh * power * m.over1.a, 0.0, 1.0);
    let painted = min(min(max(power, 1.0 - dm.a), gloss), max(dm.a, m.over2.z));
    let plain = m.over2.w;
    let with_mask = d * (deepen * (shade * 2.0 - 2.0) + 1.0) + ((1.0 - dm.r) * mesh * plain + painted) * m.over1.rgb;
    let without_mask = m.over1.rgb * (mesh * plain) + d;
    var rgb = clamp(mix(with_mask, without_mask, plain), vec3<f32>(0.0), vec3<f32>(1.0));
    rgb = mix(rgb * shade, rgb, lit);

    // Rim in the specular colour, kept off the drawn lines. The KKP rim
    // replaces it.
    let rim_v = clamp(pow(max(fresnel, 1e-6), m.over3.x * 9.0 + 1.0) * ((1.0 - dm.b) * 2.77777791 - 1.77777803), 0.0, 0.600000262);
    let rim_w = mode * m.over3.y * select(1.0, 0.0, m.ext[5].y != 0.0);
    rgb = clamp(rim_v * m.over1.rgb * rim_w * (1.0 - dm.g) + rgb, vec3<f32>(0.0), vec3<f32>(1.0));

    let light = xk_light();
    rgb *= max(light, g.ambient_shadow.rgb);
    rgb *= drawn * (1.0 - shade) + shade;

    // Lines painted into the masks.
    let lines = item_lines(d * shade * 0.5, light);
    let keep = pow((1.0 - g.params.x) * 0.889999986 + 0.00999999978, mix(lm.g, dm.b, m.p1.x));
    rgb = xk_shadow_hsv(xk_saturation(mix(lines, rgb, keep)), lit);
    return vec4<f32>(xk_finish(i, rgb, rim, mode), cut.a);
}

// Outline pass of xukmi/MainOpaquePlus and xukmi/MainAlphaPlus (pixel
// programs 5 and 2): the shaded texture pulled toward black or white by
// the scene's line depth, under the light, then _OutlineColor as far as
// its alpha says. _ColMask is not read here.
fn xk_cloth_outline(i: VsOut) -> vec4<f32> {
    let main = xk_main(i);
    let mask = xk_body_mask(i.uv0);
    var a = select(0.0, main.a, m.ext[16].z != 0.0);
    if (xk_is(XK_ALPHA)) {
        a *= m.p1.z;
    }
    if (min(a, mask.x) - m.p0.x < 0.0 && m.p1.w != 0.0) {
        discard;
    }
    let dm = xk_detail(i, i.uv0);
    let lm = xk_line_mask(i);
    let line_r = mix(lm.r, dm.r, m.p0.w);
    let factor = clamp(xk_factor(hue_tint(main.rgb * m.shadow.rgb)), vec3<f32>(0.0), vec3<f32>(1.0));
    let shade = clamp(factor * max(1.0 - line_r, 1.0 - m.p0.z), vec3<f32>(0.0), vec3<f32>(1.0));
    let lines = item_lines(main.rgb * shade * 0.5, xk_light());
    return vec4<f32>(xk_saturation(mix(lines, m.line.rgb, m.line.a)), 1.0);
}

// What xukmi/MainItemPlus and xukmi/MainItemAlphaPlus share between their
// forward and outline passes.
struct XkItem {
    // The texture under the three colours, darkened by the line mask's blue.
    base: vec3<f32>,
    // The colours before that darkening.
    plain: vec3<f32>,
    alpha: f32,
    dm: vec4<f32>,
    lm: vec4<f32>,
    // The line mask's red, or the detail mask's, raised by _AnotherRampFull.
    line_r: f32,
};

fn xk_item_base(i: VsOut, cut: vec4<f32>) -> XkItem {
    var o: XkItem;
    let cm = select(vec4<f32>(0.0), textureSample(color_mask_tex, samp, i.uv0 * m.ext[12].xy + m.ext[12].zw), has(HAS_COLOR_MASK));
    let tint = mix(mix(cm.r * (m.color1.rgb - 1.0) + 1.0, m.color2.rgb, cm.g), m.color3.rgb, cm.b);
    o.dm = xk_detail(i, i.uv0);
    o.lm = xk_line_mask(i);
    o.plain = cut.rgb * tint;
    o.base = o.plain * (1.0 - o.lm.b);
    o.alpha = cut.a;
    let line_r = mix(o.lm.r, o.dm.r, m.p0.w);
    o.line_r = m.over3.z * (1.0 - line_r) + line_r;
    return o;
}

// xukmi/MainItemPlus and xukmi/MainItemAlphaPlus (forward programs 8 and
// 13 of the first, 9 and 5 of the second, which adds the alpha). The
// tessellated variants run the same pixel programs.
// color1..3: _Color, _Color2, _Color3, placed by _ColorMask. The rest as
// xk_cloth; _SpecularPowerNail is not read.
fn xk_item(i: VsOut, front: bool) -> vec4<f32> {
    let sheer = xk_is(XK_ITEM_ALPHA);
    let cut = xk_cut(i, sheer);
    if (xk_culled(front)) {
        discard;
    }
    let l = normalize(g.light_dir.xyz);
    let view = normalize(g.eye.xyz - i.world);
    let h = normalize(view + l);
    let b = xk_item_base(i, cut);
    let dm = b.dm;
    let lm = b.lm;

    let nn = xk_normals(i, front);
    let rim = xk_rim(nn.n, view, front, b.plain, -1.0);

    // The unlit side: weakened where the material's own ramp is in use.
    // MainItemAlphaPlus leaves the alpha of _ShadowColor out.
    var factor = xk_factor(hue_tint(b.base * m.shadow.rgb));
    if (!sheer) {
        factor *= xk_shadow_alpha(m.shadow);
    }
    factor = clamp(factor, vec3<f32>(0.0), vec3<f32>(1.0));
    let metal = b.line_r * m.over3.w;
    let shade = factor * (clamp(2.0 - m.p0.z - metal, 0.0, 1.0) * 0.670000017 + 0.330000013);

    let ndl = max(dot(l, nn.n), 0.0);
    let spec = max(dot(nn.spec, h), 0.0);
    let fresnel = 1.0 - max(dot(nn.n, view), 0.0);
    let lit = xk_atten(i) * mix(xk_ramp(ndl), xk_another_ramp(spec), metal);
    let mode = xk_rim_mode(lit);
    var rgb = mix(b.base * shade, b.base, lit);
    // The KKP rim as diffuse reaches the lines only.
    let line_base = mix(b.base, rim.color, rim.amount * m.ext[6].z * mode);

    // Highlight: the shine painted in the detail mask, or a plain one.
    let shift = (m.over2.x - 1.0) * 0.800000012;
    let shine_uv = shift * vec2<f32>(dot(i.tangent, view), dot(i.bitangent, view)) + i.uv0;
    let shine = xk_detail(i, shine_uv).r;
    let power = select(1.0, dm.r, m.ext[5].x != 0.0) * m.over2.y;
    var glint = 1.0;
    if (power > 0.0) {
        glint = pow(spec, power * 256.0) * 5.0 - 4.0;
    }
    let textured = clamp(glint * power + min(shine * shine, 1.0), 0.0, 1.0);
    let plain = clamp(power * select(glint, xk_ramp(glint), m.ext[4].w != 0.0), 0.0, 1.0);
    let highlight = clamp(mix(textured, plain, m.over2.w), 0.0, 1.0) * m.over1.rgb;

    // Rim towards the silhouette, tinted by the hue of the scene shadow.
    let hue = rgb_to_hsv(g.ambient_shadow.rgb).x;
    let rim_tint = hsv_to_rgb(vec3<f32>(hue, 1.0, 1.0)) * 0.400000006 + 0.300000012;
    let rim_v = clamp(pow(max(fresnel, 1e-6), m.over3.x * 9.0 + 1.0) * 2.5 - 0.5, 0.0, 1.0) * m.over3.y * mode
        * ((1.0 - dm.b) * 9.99999809 - 8.99999809);
    rgb += clamp(rim_v * rim_tint * (1.0 - dm.g), vec3<f32>(0.0), vec3<f32>(0.5));

    rgb *= max(xk_light(), g.ambient_shadow.rgb);
    let extend = m.p0.y * -1.20000005 + 1.0;
    let drawn = (1.0 - dm.g) * (1.0 - extend) + extend;
    rgb *= drawn * (1.0 - shade) + shade;

    // Lines painted into the masks; the light does not reach them. The
    // highlight goes on top of both.
    let lines = item_lines(line_base * g.line_color.rgb * shade, vec3<f32>(1.0));
    rgb = mix(rgb, lines, mix(lm.g, dm.b, m.p1.x)) + highlight * m.over1.a;
    rgb = xk_shadow_hsv(xk_saturation(rgb), lit);
    return vec4<f32>(xk_finish(i, rgb, rim, mode), b.alpha);
}

// Outline pass of xukmi/MainItemPlus and xukmi/MainItemAlphaPlus (pixel
// programs 10 and 7): the shaded base under the scene's line colour, pulled
// toward black or white by its depth, then _OutlineColor. Here the alpha of
// _ShadowColor counts for MainItemAlphaPlus and not for MainItemPlus.
fn xk_item_outline(i: VsOut) -> vec4<f32> {
    let main = xk_main(i);
    let mask = xk_body_mask(i.uv0);
    let sheer = xk_is(XK_ITEM_ALPHA);
    var a = select(0.0, main.a, m.ext[16].z != 0.0);
    if (sheer) {
        a *= m.p1.z;
    }
    if (min(a, mask.x) - m.p0.x < 0.0 && m.p1.w != 0.0) {
        discard;
    }
    let b = xk_item_base(i, main);
    var factor = xk_factor(hue_tint(b.base * m.shadow.rgb));
    if (sheer) {
        factor *= xk_shadow_alpha(m.shadow);
    }
    factor = clamp(factor, vec3<f32>(0.0), vec3<f32>(1.0));
    let shade = factor * (clamp(2.0 - m.p0.z - b.line_r, 0.0, 1.0) * 0.670000017 + 0.330000013);
    let x = b.base * g.line_color.rgb * shade;
    let w = g.line_color.a;
    let lines = select(x * (w * 2.0), 1.0 - (1.0 - 2.0 * (w - 0.5)) * (1.0 - x), w > 0.5);
    return vec4<f32>(xk_saturation(mix(lines, m.line.rgb, m.line.a)), 1.0);
}

// The skin's colour: the texture under _Col0.._Col3 (placed by _ColMask),
// then the three layers as main_skin lays them. ext[19..21] place the
// layers; p2.zw: _nip and _nipsize; p1.zw: _tex1mask and _nip_specular.
fn xk_skin_diffuse(i: VsOut) -> vec3<f32> {
    let cm = select(vec4<f32>(0.0), textureSample(color_mask_tex, samp, i.uv0 * m.ext[12].xy + m.ext[12].zw), has(HAS_COLOR_MASK));
    var col = max(m.color1.rgb, XK_TINY);
    col = mix(col, max(m.color2.rgb, XK_TINY), cm.r);
    col = mix(col, max(m.color3.rgb, XK_TINY), cm.g);
    col = mix(col, max(m.ext[0].rgb, XK_TINY), cm.b);
    var d = xk_main(i).rgb * col;

    let ring = clamp(length(i.uv1 - 0.5) * 16.6666698 - 1.0, 0.0, 1.0);
    let scale = m.p2.w * vec2<f32>(-1.39999998, 0.699999988) + vec2<f32>(2.0, -0.5);
    let plain = i.uv1 * i.color.r;
    let grown = (ring * (i.uv1 * scale.x + scale.y - i.uv1) + i.uv1) * i.color.r;
    let uv1 = mix(plain, grown, m.p2.z) * m.ext[19].xy + m.ext[19].zw;
    let t1 = select(vec4<f32>(0.0), textureSample(over1_tex, samp, uv1), has(HAS_OVER1));
    let flat = t1.rgb * m.over1.rgb;
    let shiny = t1.r * (m.over1.rgb + t1.g * m.p1.w * 0.330000013);
    d = mix(d, mix(flat, shiny, clamp(m.p1.z, 0.0, 1.0)), t1.a * m.over1.a);

    let uv2 = i.uv2 * i.color.b * m.ext[20].xy + m.ext[20].zw;
    let t2 = select(vec4<f32>(0.0), textureSample(over2_tex, samp, uv2), has(HAS_OVER2));
    d = mix(d, t2.rgb * m.over2.rgb, t2.a * m.over2.a);
    let uv3 = i.uv3 * m.ext[21].xy + m.ext[21].zw;
    let t3 = select(vec4<f32>(0.0), textureSample(over3_tex, samp, uv3), has(HAS_OVER3));
    return mix(d, t3.rgb * m.over3.rgb, t3.a * m.over3.a);
}

// xukmi/SkinPlus (forward programs 3 and 9; SkinPlusReflect's program 4 is
// the same). Left out beside what the head of the file lists: the face
// shadow switch _FaceShadowG with its _NormalMask, which the game leaves at
// zero, and the rim, which these programs give the liquid layers only.
// color1..3 and ext[0]: _Col0.._Col3. shadow: _ShadowColor. over1..3:
// _overcolor1.._overcolor3. p0.y: _ShadowExtend. p1.x: _linetexon.
// ext[17]: _SpecularColor. ext[18]: _SpeclarHeight, _SpecularPower,
// _SpecularPowerNail, _notusetexspecular.
fn xk_skin(i: VsOut, front: bool) -> vec4<f32> {
    if (xk_body_mask(i.uv0).x - 0.5 < 0.0) {
        discard;
    }
    let l = normalize(g.light_dir.xyz);
    let view = normalize(g.eye.xyz - i.world);
    var d = xk_skin_diffuse(i);

    // Skin shade: the skin's own hue at two thirds of its saturation; and
    // the warm tone its shine takes.
    let hsv = rgb_to_hsv(d);
    let tint = hsv_to_rgb(vec3<f32>(hsv.x, hsv.y * 0.660000026, 1.0));
    let warm = hsv_to_rgb(vec3<f32>(hsv.x - 0.0799999982, 0.400000006, 1.0)) * 0.970000029 - 1.0;

    let nn = xk_normals(i, front);
    let dm = xk_detail(i, i.uv0);
    let lm = xk_line_mask(i);
    let spec_rgba = m.ext[17];
    let power = select(1.0, dm.r, m.ext[5].x != 0.0) * m.ext[18].y;
    // What scales the lines and shadows that come with the chosen detail.
    let strength = m.ext[7].z * m.ext[5].w;
    let rim = xk_rim(nn.n, view, front, d, -1.0);

    let extend = m.p0.y * -1.20000005 + 1.0;
    let att = xk_atten(i);
    let ramp_shadows = m.ext[1].w != 0.0;
    let ndl = max(dot(l, nn.n), 0.0);
    let ramp = xk_ramp(ndl * select(1.0, att, ramp_shadows)) * select(att, 1.0, ramp_shadows);
    let drawn = min(1.0 - dm.g, 1.0 - lm.b * strength) * (1.0 - extend) + extend;
    let lit = drawn * ramp;
    let mode = xk_rim_mode(lit);
    d = mix(d, rim.color, rim.amount * m.ext[6].z * mode);

    // Shine from the detail mask's red channel, shifted with the view.
    let shift = (m.ext[18].x - 1.0) * 0.800000012;
    let shine_uv = shift * vec2<f32>(dot(i.tangent, view), dot(i.bitangent, view)) + i.uv0;
    let shine = clamp(xk_detail(i, shine_uv).r * 1.66666698, 0.0, 1.0);
    let core = shine * shine * shine;
    let strength_s = max(power * dm.a, (1.0 - dm.a) * m.ext[18].z);
    let glow = strength_s * core * spec_rgba.rgb;
    var soft = min((shine - core) * strength_s, dot(glow, vec3<f32>(0.300000012, 0.589999974, 0.109999999)));
    soft = min(min(lit, soft), dm.a);
    let textured = d * (soft * warm + 1.0) + glow;

    // Plain highlight, used when the material switches the textured one
    // off: in the light's colour unless _UseLightColorSpecular is off.
    let sdot = max(dot(normalize(l + view), nn.spec), 0.0);
    var plain = 0.0;
    if (power > 0.0) {
        plain = pow(sdot, power * 256.0) * power;
    }
    plain = clamp(plain * spec_rgba.a, 0.0, 1.0);
    plain = select(plain, xk_ramp(plain), m.ext[4].w != 0.0);
    let plain_rgb = select(spec_rgba.rgb, vec3<f32>(g.params.y), m.ext[16].w != 0.0) * spec_rgba.a;
    let off = m.ext[18].w;
    let base = off * (plain * plain_rgb + d) + clamp(textured * (1.0 - off), vec3<f32>(0.0), vec3<f32>(1.0));

    let factor = clamp(xk_factor(tint) * xk_shadow_alpha(m.shadow), vec3<f32>(0.0), vec3<f32>(1.0));
    let light = xk_light();
    let body = mix(base * factor, base, lit) * max(light, g.ambient_shadow.rgb) * mix(factor, vec3<f32>(1.0), drawn);

    // Lines painted into the masks.
    let ink = factor * d * ((1.0 - dm.g) * 0.5 + 0.5);
    let w = g.line_color.a;
    let toward_white = 1.0 - (1.0 - 2.0 * (w - 0.5)) * (1.0 - ink);
    let line_rgb = clamp(select(ink * (w * 2.0), toward_white, w > 0.5), vec3<f32>(0.0), vec3<f32>(1.0)) * light;
    var keep = pow((1.0 - g.params.x) * 0.800000012 + 0.200000003, lm.g);
    keep = min((1.0 - lm.r * strength) * 0.5 + 0.5, keep);
    keep = m.p1.x * (keep - 1.0) + 1.0;
    let rgb = xk_shadow_hsv(xk_saturation(mix(line_rgb, body, keep)), lit);
    return vec4<f32>(xk_finish(i, rgb, rim, mode), 1.0);
}

// Outline pass of xukmi/SkinPlus (pixel program 13).
fn xk_skin_outline(i: VsOut) -> vec4<f32> {
    if (xk_body_mask(i.uv0).x * select(0.0, 1.0, m.ext[16].z != 0.0) - 0.5 < 0.0) {
        discard;
    }
    let d = xk_skin_diffuse(i);
    let hsv = rgb_to_hsv(d);
    let tint = hsv_to_rgb(vec3<f32>(hsv.x, hsv.y * 0.660000026, 1.0));
    let factor = clamp(xk_factor(tint), vec3<f32>(0.0), vec3<f32>(1.0));
    let x = d * factor * ((1.0 - xk_detail(i, i.uv0).g) * 0.5 + 0.5);
    let w = g.line_color.a;
    let toward_white = 1.0 - (1.0 - 2.0 * (w - 0.5)) * (1.0 - x);
    let lines = clamp(select(x * (w * 2.0), toward_white, w > 0.5), vec3<f32>(0.0), vec3<f32>(1.0)) * xk_light();
    return vec4<f32>(max(xk_saturation(mix(lines, m.line.rgb, m.line.a)), XK_TINY), 1.0);
}

// The hair's colour: the texture under _Color, _Color2 and _Color3, placed
// by _ColorMask.
fn xk_hair_base(i: VsOut, main: vec3<f32>) -> vec3<f32> {
    let cm = select(vec4<f32>(0.0), textureSample(color_mask_tex, samp, i.uv0 * m.ext[12].xy + m.ext[12].zw), has(HAS_COLOR_MASK));
    return main * mix(mix(cm.r * (m.color1.rgb - 1.0) + 1.0, m.color2.rgb, cm.g), m.color3.rgb, cm.b);
}

// xukmi/HairPlus and xukmi/HairFrontPlus (forward programs 12 and 6 of the
// first, 1 and 9 of the second, which reads its gloss mask elsewhere). The
// reflect and tessellated variants run the same programs.
// Left out: the passes of HairFrontPlus that draw the hair over the eyes
// (MAINALPHA and OUTLINEALPHA), which write an alpha of one minus
// _transparency and so nothing at the default of one.
// color1..3: _Color.._Color3. shadow: _ShadowColor. over1: _SpecularColor.
// over2: placement of _HairGloss (over1_tex). over3: _rimpower, _rimV,
// _SpeclarHeight, _SpecularHeightInvert. p0: _Cutoff, _ShadowExtend.
// line: _LineColor. ext[17]: _GlossColor. ext[18]: _SpecularHairPower,
// _SpecularIsHighlights, _SpecularIsHighlightsRange,
// _SpecularIsHighLightsPow. ext[19].x: _UseMeshSpecular.
fn xk_hair(i: VsOut, front: bool) -> vec4<f32> {
    let main = xk_main(i);
    let alpha = main.a * xk_body_mask(i.uv0).y;
    if (alpha - m.p0.x < 0.0) {
        discard;
    }
    if (xk_culled(front)) {
        discard;
    }
    let l = normalize(g.light_dir.xyz);
    let view = normalize(g.eye.xyz - i.world);
    let base = xk_hair_base(i, main.rgb);

    let nn = xk_normals(i, front);
    let rim = xk_rim(nn.n, view, front, base, 0.100000001);

    // Rim towards the silhouette, tinted by the hue of the scene shadow.
    // The KKP rim replaces it.
    let hue = rgb_to_hsv(g.ambient_shadow.rgb).x;
    let rim_tint = hsv_to_rgb(vec3<f32>(hue, 1.0, 1.0)) * 0.330000013 + 0.330000013;
    let ndv = max(dot(view, nn.n), 0.0);
    let fresnel = clamp(pow(max(1.0 - ndv, 1e-6), m.over3.x * 9.0 + 1.0) * 5.0 - 1.5, 0.0, 1.0);
    let rim_rgb = select(min(fresnel * rim_tint, vec3<f32>(0.5)), vec3<f32>(0.0), m.ext[5].y != 0.0);

    // Highlights on the mesh: one for _UseMeshSpecular, one that stands in
    // for the gloss mask with _SpecularIsHighlights.
    let spec_rgba = m.over1;
    let sdot = max(dot(normalize(view + l), nn.spec), 0.0);
    let hair_power = m.ext[18].x;
    var mesh = 0.0;
    if (hair_power > 0.0) {
        mesh = pow(sdot, hair_power * 256.0) * hair_power;
    }
    mesh = clamp(mesh * spec_rgba.a, 0.0, 1.0);
    mesh = select(mesh, xk_ramp(mesh), m.ext[4].w != 0.0);
    var streak = 1.0;
    if (m.ext[18].w > 0.0) {
        streak = pow(sdot, m.ext[18].w);
    }
    streak = clamp(streak * spec_rgba.a, 0.0, 1.0);
    let mesh_rgb = mesh * select(spec_rgba.rgb, vec3<f32>(g.params.y), m.ext[16].w != 0.0) * spec_rgba.a;

    let ndl = clamp(dot(l, nn.n), 0.0, 1.0);
    let ramp = xk_ramp(ndl);

    // Gloss mask, read twice a little apart along the strand.
    let slide = dot(view, i.bitangent) * (m.over3.z - 1.0) * select(1.0, -1.0, m.over3.w != 0.0);
    let gx = select(ndl * 0.00499999989 + i.uv1.x, ndl * 0.0199999809 + i.uv1.x + 0.99000001, xk_is(XK_HAIR_FRONT));
    let gz = slide + i.uv1.y;
    let st = m.over2;
    let far = textureSample(over1_tex, samp, vec2<f32>(gx, gz + 0.00800000038) * st.xy + st.zw).r;
    let near = textureSample(over1_tex, samp, vec2<f32>(gx, gz) * st.xy + st.zw).r;
    let gloss = select(0.0, near, has(HAS_OVER1));
    let gloss_edge = select(0.0, (far - near) * 0.5, has(HAS_OVER1));

    let shadow = overlay(scene_shadow(), m.shadow.rgb);
    let lit = clamp(min(xk_another_ramp(ndv), ramp), 0.0, 1.0) * shadow_mask(i);
    let mode = xk_rim_mode(lit);
    let d = mix(base, rim.color, rim.amount * m.ext[6].z * mode);
    let edged = d * (gloss_edge * (shadow.r - 1.0) + 1.0);
    let dark = shadow * edged;

    let dm = xk_detail(i, i.uv0);
    let open = 1.0 - dm.gb;
    var rgb = lit * open.x * (edged - dark) + dark;

    // The gloss, in _GlossColor.
    let shine = select(gloss, streak * m.ext[18].z, m.ext[18].y != 0.0) * select(1.0, dm.r, m.ext[5].x != 0.0)
        * (ramp * 0.5 + 0.5);
    let gc = m.ext[17];
    rgb = rgb * clamp(1.0 - shine * gc.a, 0.0, 1.0) + shine * gc.a * gc.rgb * gc.a;

    rgb = clamp(open.x * m.over3.y * mode * open.y * rim_rgb + rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    rgb += m.ext[19].x * mesh_rgb;
    let reach = max(1.0 - m.p0.y, max(lit, open.x));
    rgb *= reach * (1.0 - shadow) + shadow;
    rgb *= max(xk_light(), g.ambient_shadow.rgb);
    rgb = xk_shadow_hsv(xk_saturation(rgb), lit * open.x);
    return vec4<f32>(xk_finish(i, rgb, rim, mode), select(1.0, alpha, has(BLENDED)));
}

// Outline pass of xukmi/HairPlus and xukmi/HairFrontPlus (pixel programs
// 10 and 5): the hair colour overlaid with _LineColor, under the light.
fn xk_hair_outline(i: VsOut) -> vec4<f32> {
    let main = xk_main(i);
    let a = select(0.0, main.a, m.ext[16].z != 0.0);
    if (xk_body_mask(i.uv0).y * a - m.p0.x < 0.0) {
        discard;
    }
    let rgb = overlay(xk_hair_base(i, main.rgb), m.line.rgb) * xk_light();
    return vec4<f32>(xk_saturation(rgb), 1.0);
}

// xukmi/EyePlus (forward program 2): the iris turned by _rotation, the
// expression texture and the two highlights over it, lit unless one of
// those covers the pixel. Its REFLECT pass multiplies the picture by a
// matcap that defaults to white and is not drawn.
// color1, color2: _overcolor1, _overcolor2. shadow: _shadowcolor. over1,
// over2: placement of the highlights (over1_tex, over2_tex). over3_tex:
// _expression. ext[17]: _isHighLight, _exppower, _ExpressionSize,
// _ExpressionDepth. ext[18].x: _rotation.
fn xk_eye(i: VsOut) -> vec4<f32> {
    let turn = m.ext[18].x * -6.28318548;
    let s = sin(turn);
    let c = cos(turn);
    let d = i.uv0 - 0.5;
    let turned = vec2<f32>(c * d.x - s * d.y, c * d.y + s * d.x) + 0.5;
    let iris = select(vec4<f32>(1.0), textureSample(main_tex, samp, turned * m.st_main.xy + m.st_main.zw), has(HAS_MAIN));

    // The expression sits a little under the surface.
    let view = normalize(g.eye.xyz - i.world);
    let sunk = vec2<f32>(dot(i.tangent, view), dot(i.bitangent, view)) * m.ext[17].w * -0.0599999987;
    var euv = (sunk + i.uv0) * m.st_main.xy + m.st_main.zw - 0.5;
    euv = euv / max(m.ext[17].z, 0.100000001) + vec2<f32>(0.5, 0.600000024);
    let expr = select(vec4<f32>(0.0), textureSample(over3_tex, samp, euv), has(HAS_OVER3));
    let shown = expr.a * m.ext[17].y;

    let a1 = select(0.0, textureSample(over1_tex, samp, i.uv1 * m.over1.xy + m.over1.zw).a, has(HAS_OVER1));
    let a2 = select(0.0, textureSample(over2_tex, samp, i.uv2 * m.over2.xy + m.over2.zw).a, has(HAS_OVER2));
    let over = 1.0 - (1.0 - a1 * m.color1) * (1.0 - a2 * m.color2);
    let highlight = clamp(over.a * m.ext[17].x, 0.0, 1.0);
    let covered = shown + highlight;
    let alpha = clamp(iris.a + covered, 0.0, 1.0);
    if (alpha < 0.00999999978) {
        discard;
    }

    let shade = overlay(scene_shadow(), m.shadow.rgb);
    let base = mix(mix(iris.rgb, expr.rgb, shown), over.rgb, highlight);
    let dark = shade * base;
    let lit = clamp(covered + max(dot(g.light_dir.xyz, i.normal), 0.0), 0.0, 1.0);
    var rgb = (lit * (base - dark) + dark) * max(xk_light(), g.ambient_shadow.rgb);
    rgb = xk_shadow_hsv(xk_saturation(rgb), clamp(xk_ramp(lit), 0.0, 1.0));

    // The emission mask follows the iris; its placement is applied twice.
    var uv = (sunk + turned) * m.st_main.xy + m.st_main.zw;
    uv = uv * m.ext[13].xy + m.ext[13].zw;
    uv = uv * m.ext[13].xy + m.ext[13].zw;
    return vec4<f32>(xk_emission_at(uv, rgb), alpha);
}

// xukmi/EyeWPlus (forward program 1): brows, lashes and eye whites, cut at
// _Cutoff and lit through the ramp with the material's shade. The REFLECT
// pass is as EyePlus's. color1: _Color. shadow: _shadowcolor.
fn xk_eyew(i: VsOut) -> vec4<f32> {
    let main = xk_main(i);
    if (main.a - m.p0.x < 0.0) {
        discard;
    }
    let shade = overlay(scene_shadow(), m.shadow.rgb);
    let base = main.rgb * m.color1.rgb;
    let dark = shade * base;
    let ramp = xk_ramp(dot(g.light_dir.xyz, i.normal));
    var rgb = (ramp * (base - dark) + dark) * max(xk_light(), g.ambient_shadow.rgb);
    rgb = xk_shadow_hsv(xk_saturation(rgb), clamp(ramp, 0.0, 1.0));
    return vec4<f32>(xk_emission(i, rgb), select(1.0, main.a, has(BLENDED)));
}

// Forward pass of every KKShadersPlus kind.
fn xk_forward(i: VsOut, front: bool) -> vec4<f32> {
    switch m.info.z {
        case XK_OPAQUE, XK_ALPHA: {
            return xk_cloth(i, front);
        }
        case XK_SKIN: {
            return xk_skin(i, front);
        }
        case XK_HAIR, XK_HAIR_FRONT: {
            return xk_hair(i, front);
        }
        case XK_EYE: {
            return xk_eye(i);
        }
        case XK_EYEW: {
            return xk_eyew(i);
        }
        default: {
            return xk_item(i, front);
        }
    }
}

fn xk_outline(i: VsOut) -> vec4<f32> {
    switch m.info.z {
        case XK_OPAQUE, XK_ALPHA: {
            return xk_cloth_outline(i);
        }
        case XK_SKIN: {
            return xk_skin_outline(i);
        }
        case XK_HAIR, XK_HAIR_FRONT: {
            return xk_hair_outline(i);
        }
        default: {
            return xk_item_outline(i);
        }
    }
}

// What the shadow caster programs keep: true where the pixel casts.
// `main_a` is the texture's alpha.
fn xk_casts(uv0: vec2<f32>, main_a: f32) -> bool {
    let mask = xk_body_mask(uv0);
    let v = mask.x * main_a;
    switch m.info.z {
        // MainAlphaPlus (program 13): cut at one half plus _Cutoff unless
        // _AlphaOptionCutoff is off.
        case XK_ALPHA: {
            return !(v - 0.5 < m.p0.x && m.p1.w != 0.0);
        }
        // MainItemAlphaPlus (program 11): cut at _Cutoff.
        case XK_ITEM_ALPHA: {
            return v - m.p0.x >= 0.0;
        }
        // SkinPlus (program 5): the body masks only.
        case XK_SKIN: {
            return mask.x - 0.5 >= 0.0;
        }
        // HairPlus and HairFrontPlus (programs 13 and 7): the alpha mask's
        // red times the texture alpha, cut at _Cutoff.
        case XK_HAIR, XK_HAIR_FRONT: {
            return mask.y * main_a - m.p0.x >= 0.0;
        }
        // EyeWPlus (program 9). EyePlus has no caster of its own.
        case XK_EYE, XK_EYEW: {
            return main_a - 0.5 >= 0.0;
        }
        // MainOpaquePlus and MainItemPlus (program 1 of each).
        default: {
            return v - 0.5 >= 0.0;
        }
    }
}
