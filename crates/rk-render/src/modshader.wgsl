// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

// Shaders that mods carry in their own bundles, after their compiled
// programs. This file follows toon.wgsl and xukmi.wgsl in the same module.

// Which mod shader a material follows (info.z).
const MOD_FIRST: u32 = 64u;
const MOD_IBL_ALPHA: u32 = 64u;
const MOD_KKUSS_ALPHA: u32 = 65u;
// The twelfth texture slot is bound (HAS_OVER7 in material.rs).
const MOD_HAS_OVER7: u32 = 8192u;

// IBL_Shader_alpha (anse_pantyhose.unity3d; vertex program 2, pixel program
// 4 of its Direct3D 11 blob). A Unity surface shader: Standard lighting,
// alpha fade, back faces culled, no shadows received. It was compiled for
// gamma colour space and keeps those constants in the linear game.
//
// color1: _Color, over1..3: _overcolor1..3, st_main: _MainTex placement
// ext[0]: _Glossiness, _Metallic, _NormalScale, _NormalScale2
// ext[1]: _Parallax, _Occlusion, _rimpower, _rimV
// ext[2]: _rimcolor
// ext[3..7]: placement of _NormalMap, _NormalMap2, _overtex1..3
// ext[8]: x: 1 when _NormalMap2 repeats
// Textures: main_tex _MainTex, detail_tex _DetailMask, over1..3_tex
// _overtex1..3, over5_tex _NormalMap, over6_tex _NormalMap2.

// The pixel program's LinearToGammaSpace: max(1.055 * x^(1/2.4) - 0.055, 0).
fn mod_to_gamma(c: vec3<f32>) -> vec3<f32> {
    let p = pow(max(c, vec3<f32>(0.0)), vec3<f32>(0.416666657));
    return max(p * 1.05499995 - 0.0549999997, vec3<f32>(0.0));
}

// The constant and linear terms of the ambient probe (unity_SHA*).
fn mod_sh_l0l1(n: vec3<f32>) -> vec3<f32> {
    let v = vec4<f32>(n, 1.0);
    return vec3<f32>(dot(g.sh[0], v), dot(g.sh[1], v), dot(g.sh[2], v));
}

// Its quadratic terms (unity_SHB*, unity_SHC).
fn mod_sh_l2(n: vec3<f32>) -> vec3<f32> {
    let b = n.xyzz * n.yzzx;
    let c = n.x * n.x - n.y * n.y;
    return vec3<f32>(dot(g.sh[3], b), dot(g.sh[4], b), dot(g.sh[5], b)) + g.sh[6].rgb * c;
}

// A normal map's xy as Unity packs them (x in alpha, y in green), scaled,
// with the z that completes them.
fn mod_unpack(t: vec4<f32>, scale: f32) -> vec3<f32> {
    let xy = (t.ag * 2.0 - 1.0) * scale;
    return vec3<f32>(xy, sqrt(1.0 - min(dot(xy, xy), 1.0)));
}

fn ibl_alpha(i: VsOut, front: bool) -> vec4<f32> {
    // Cull Back.
    if (!front) {
        discard;
    }
    let to_eye = g.eye.xyz - i.world;
    let view = normalize(to_eye);
    // The rows the vertex program hands over: tangent, bitangent, normal.
    let view_t = vec3<f32>(dot(i.tangent, view), dot(i.bitangent, view), dot(i.normal, view));

    let uv_main = i.uv0 * m.st_main.xy + m.st_main.zw;
    var c = vec4<f32>(1.0);
    if (has(HAS_MAIN)) {
        c = textureSample(main_tex, samp, uv_main);
    }
    c *= m.color1;
    var albedo = c.rgb;
    // The three overlays, each over the last by its alpha. Unbound ones
    // are Unity's clear "black".
    if (has(HAS_OVER1)) {
        let o = textureSample(over1_tex, samp, i.uv0 * m.ext[5].xy + m.ext[5].zw);
        albedo = mix(albedo, o.rgb * m.over1.rgb, o.a * m.over1.a);
    }
    if (has(HAS_OVER2)) {
        let o = textureSample(over2_tex, samp, i.uv0 * m.ext[6].xy + m.ext[6].zw);
        albedo = mix(albedo, o.rgb * m.over2.rgb, o.a * m.over2.a);
    }
    if (has(HAS_OVER3)) {
        let o = textureSample(over3_tex, samp, i.uv0 * m.ext[7].xy + m.ext[7].zw);
        albedo = mix(albedo, o.rgb * m.over3.rgb, o.a * m.over3.a);
    }

    // Parallax: the detail mask's alpha is the height, read at the normal
    // map's placement; the offset moves the main placement.
    var height = 0.0;
    if (has(HAS_DETAIL)) {
        height = textureSample(detail_tex, samp, i.uv0 * m.ext[3].xy + m.ext[3].zw).a;
    }
    let vt = normalize(view_t);
    let lift = height * m.ext[1].x - m.ext[1].x * 0.5;
    let uv = lift * (vt.xy / (vt.z + 0.419999987)) + uv_main;

    var occlusion = 1.0;
    if (has(HAS_DETAIL)) {
        occlusion = 1.0 - textureSample(detail_tex, samp, uv).g * m.ext[1].y;
    }
    // Unbound normal maps are Unity's flat "bump".
    var t1 = vec4<f32>(0.5, 0.5, 1.0, 0.5);
    if (has(HAS_OVER5)) {
        t1 = textureSample(over5_tex, samp, uv);
    }
    var t2 = vec4<f32>(0.5, 0.5, 1.0, 0.5);
    if (has(HAS_OVER6)) {
        let uv2 = i.uv0 * m.ext[4].xy + m.ext[4].zw;
        if (m.ext[8].x != 0.0) {
            t2 = textureSample(over6_tex, wrap_samp, uv2);
        } else {
            t2 = textureSample(over6_tex, samp, uv2);
        }
    }
    let n1 = mod_unpack(t1, m.ext[0].z);
    let n2 = mod_unpack(t2, m.ext[0].w);
    let nt = normalize(vec3<f32>(n1.xy + n2.xy, n1.z * n2.z));

    // Emission: a rim lit by the ambient probe, which is read with the
    // tangent space normal as the program does.
    let rim = pow(1.0 - clamp(dot(view_t, nt), 0.0, 1.0), m.ext[1].z * 10.0);
    let rim_light = mod_to_gamma(mod_sh_l0l1(nt) + mod_sh_l2(nt));
    let emission = rim * m.ext[2].rgb * rim_light * m.ext[1].w;

    let n = normalize(i.tangent * nt.x + i.bitangent * nt.y + i.normal * nt.z);
    let rough_p = 1.0 - m.ext[0].x;
    let light_dir = normalize(g.light_dir.xyz);
    // _LightColor0 of the scene's white light. The pass has no shadow
    // variant, so the light is never dimmed.
    let light = vec3<f32>(g.params.y);

    // Ambient light: the linear terms per pixel; the program takes the
    // quadratic ones per vertex, with the mesh normal.
    let ambient = mod_to_gamma(mod_sh_l0l1(n) + mod_sh_l2(normalize(i.normal)));
    // The reflection probe at the mip of this roughness; the scene's probe
    // is one colour.
    let probe = g.reflection.rgb * occlusion;

    // Metallic setup with the gamma space constants.
    let metallic = m.ext[0].y;
    let spec_color = mix(vec3<f32>(0.220916301), albedo, metallic);
    let one_minus_reflectivity = 0.779083729 - metallic * 0.779083729;
    let diff_color = albedo * one_minus_reflectivity;

    // Unity's BRDF1 (Disney diffuse, GGX, Smith joint visibility).
    let h_raw = view + light_dir;
    let h = h_raw * inverseSqrt(max(dot(h_raw, h_raw), 0.00100000005));
    let nv = abs(dot(n, view));
    let nl = clamp(dot(n, light_dir), 0.0, 1.0);
    let nh = clamp(dot(n, h), 0.0, 1.0);
    let lh = clamp(dot(light_dir, h), 0.0, 1.0);

    // fd90 - 1, with fd90 = 0.5 + 2 lh^2 roughness.
    let fd = 2.0 * lh * lh * rough_p - 0.5;
    let p_nl = 1.0 - nl;
    let p_nv = 1.0 - nv;
    let nl5 = p_nl * p_nl * p_nl * p_nl * p_nl;
    let nv5 = p_nv * p_nv * p_nv * p_nv * p_nv;
    let diffuse_term = (1.0 + fd * nl5) * (1.0 + fd * nv5) * nl;

    let rough = rough_p * rough_p;
    let lambda_v = nl * (nv * (1.0 - rough) + rough);
    let lambda_l = nv * (nl * (1.0 - rough) + rough);
    let vis = 0.5 / (lambda_v + lambda_l + 0.00000999999975);
    let a2 = rough * rough;
    let d = (nh * a2 - nh) * nh + 1.0;
    let ggx = 0.318309873 * a2 / (d * d + 0.00000010000000116860974);
    var spec_term = sqrt(max(vis * ggx * 3.14159274, 0.0000999999975)) * nl;
    if (dot(spec_color, spec_color) == 0.0) {
        spec_term = 0.0;
    }
    let surface_reduction = 1.0 - 0.280000001 * rough * rough_p;
    let grazing = clamp(m.ext[0].x + (1.0 - one_minus_reflectivity), 0.0, 1.0);

    let p_lh = 1.0 - lh;
    let lh5 = p_lh * p_lh * p_lh * p_lh * p_lh;
    let fresnel = spec_color + (1.0 - spec_color) * lh5;
    let fresnel_probe = mix(spec_color, vec3<f32>(grazing), nv5);

    let rgb = diff_color * (light * diffuse_term + ambient * occlusion)
        + spec_term * light * fresnel
        + surface_reduction * probe * fresnel_probe
        + emission * 5.0;
    return vec4<f32>(rgb, c.a);
}

// KKUSSalpha (kkussalpha.unity3d of nHaruka's KKUSS pack; vertex program 5,
// pixel program 1 of its Direct3D 11 blob). A Unity surface shader over the
// game's clothes inputs: Standard lighting with the linear colour space
// constants, the light softened by _ShadowPower, colour multiplied by alpha
// ahead of a One, OneMinusSrcAlpha blend. Back faces culled, no shadows
// received.
//
// color1..3: _Color.._Color3, over1..3: _overcolor1..3, st_main: _MainTex
// placement
// ext[0]: _SpecularPower, _Metallic, _Occlusion, _Emission
// ext[1]: _DetailNormalMapScale, _DetailMaskScale, _notusetexspecular,
//         _ShadowPower
// ext[2]: _alpha, _alpha_a, _alpha_b
// ext[3]: _nipsize, _nip
// ext[4]: _Addcolor
// ext[5..9]: placement of _NormalMap, _NormalMapDetail, _overtex1..3
// ext[10]: _LiquidTiling (xy offset, zw scale)
// ext[11]: _liquidftop, _liquidfbot, _liquidbtop, _liquidbbot
// ext[12]: _liquidface, 1 when _NormalMapDetail repeats, 1 when _Texture2
//          repeats
// Textures: main_tex _MainTex, color_mask_tex _ColorMask, detail_tex
// _DetailMask, alpha_mask_tex _AlphaMask, over1..3_tex _overtex1..3,
// over4_tex _Texture2, over5_tex _NormalMap, over6_tex _NormalMapDetail,
// over7_tex _liquidmask.
fn kkuss_alpha(i: VsOut, front: bool) -> vec4<f32> {
    // Cull Back.
    if (!front) {
        discard;
    }
    let view = normalize(g.eye.xyz - i.world);
    let uv = i.uv0 * m.st_main.xy + m.st_main.zw;

    // The two normal maps, the detail one scaled, blended as Unity's
    // BlendNormals. Unbound ones are Unity's flat "bump".
    var t1 = vec4<f32>(0.5, 0.5, 1.0, 0.5);
    if (has(HAS_OVER5)) {
        t1 = textureSample(over5_tex, samp, uv * m.ext[5].xy + m.ext[5].zw);
    }
    var t2 = vec4<f32>(0.5, 0.5, 1.0, 0.5);
    if (has(HAS_OVER6)) {
        let uv2 = uv * m.ext[6].xy + m.ext[6].zw;
        if (m.ext[12].y != 0.0) {
            t2 = textureSample(over6_tex, wrap_samp, uv2);
        } else {
            t2 = textureSample(over6_tex, samp, uv2);
        }
    }
    let n1 = mod_unpack(t1, 1.0);
    let n2 = mod_unpack(t2, m.ext[1].x);
    let nt = normalize(vec3<f32>(n1.xy + n2.xy, n1.z * n2.z));

    // Alpha: the body mask's two channels, each switched by its toggle,
    // the main texture's alpha and _alpha.
    var mask = vec2<f32>(1.0);
    if (has(HAS_ALPHA_MASK)) {
        mask = textureSample(alpha_mask_tex, samp, uv).rg;
    }
    mask = max(mask, 1.0 - m.ext[2].yz);
    var texel = vec4<f32>(1.0);
    if (has(HAS_MAIN)) {
        texel = textureSample(main_tex, samp, uv);
    }
    let alpha = min(min(mask.x, mask.y), texel.a) * m.ext[2].x;

    // Liquid: the game's cover, added to the colour as it is.
    var amount = vec4<f32>(0.0);
    if (has(HAS_PATTERN3)) {
        let uv_l = uv * m.ext[10].zw + m.ext[10].xy;
        if (m.ext[12].z != 0.0) {
            amount = textureSample(over4_tex, wrap_samp, uv_l);
        } else {
            amount = textureSample(over4_tex, samp, uv_l);
        }
    }
    var liquid_mask = vec3<f32>(0.0);
    if (has(MOD_HAS_OVER7)) {
        liquid_mask = textureSample(over7_tex, samp, uv).rgb;
    }
    let liquid = skin_liquid_cover(m.ext[11], m.ext[12].x, amount, liquid_mask);

    // The three colours go where the colour mask says, from white.
    var tint = vec3<f32>(1.0);
    if (has(HAS_COLOR_MASK)) {
        let cm = textureSample(color_mask_tex, samp, uv).rgb;
        tint = cm.r * (m.color1.rgb - 1.0) + 1.0;
        tint = mix(tint, m.color2.rgb, cm.g);
        tint = mix(tint, m.color3.rgb, cm.b);
    }
    var c = texel.rgb * tint;

    // The three overlays. The first two scale their second and third UV
    // sets by the vertex colour, and the first can grow around its centre,
    // as the game's skin shader does. Unbound ones are Unity's clear
    // "black".
    if (has(HAS_OVER1)) {
        let uv1 = i.uv1 * m.ext[7].xy + m.ext[7].zw;
        let ring = clamp(length(uv1 - 0.5) * 16.6666698 - 1.0, 0.0, 1.0);
        let scale = m.ext[3].x * vec2<f32>(-1.39999998, 0.699999988) + vec2<f32>(2.0, -0.5);
        let grown = ring * (uv1 * scale.x + scale.y - uv1) + uv1;
        let plain = uv1 * i.color.r;
        let o = textureSample(over1_tex, samp, m.ext[3].y * (grown * i.color.r - plain) + plain);
        c = mix(o.rgb * m.over1.rgb, c, 1.0 - m.over1.a * o.a);
    }
    if (has(HAS_OVER2)) {
        let uv2 = (i.uv2 * m.ext[8].xy + m.ext[8].zw) * i.color.b;
        let o = textureSample(over2_tex, samp, uv2);
        c = mix(o.rgb * m.over2.rgb, c, 1.0 - m.over2.a * o.a);
    }
    if (has(HAS_OVER3)) {
        let o = textureSample(over3_tex, samp, i.uv3 * m.ext[9].xy + m.ext[9].zw);
        c = mix(o.rgb * m.over3.rgb, c, 1.0 - o.a * m.over3.a);
    }

    // The detail mask lifts by its red and darkens by its green.
    var detail = vec2<f32>(0.0);
    if (has(HAS_DETAIL)) {
        detail = textureSample(detail_tex, samp, uv).rg;
    }
    c += liquid;
    c += (m.ext[1].z * detail.x - detail.y) * m.ext[1].y;
    let albedo = c * m.ext[4].rgb;

    // The world normal. The ambient probe and the reflection read it
    // before it is normalised.
    let n_raw = i.tangent * nt.x + i.bitangent * nt.y + i.normal * nt.z;
    let n = normalize(n_raw);
    let rough_p = 1.0 - m.ext[0].x;
    let light_dir = normalize(g.light_dir.xyz);
    // _LightColor0 of the scene's white light. The pass has no shadow
    // variant, so the light is never dimmed.
    let light = vec3<f32>(g.params.y);

    // Ambient light: the linear terms per pixel; the program takes the
    // quadratic ones per vertex, with the mesh normal.
    let ambient = max(mod_sh_l0l1(n_raw) + mod_sh_l2(normalize(i.normal)), vec3<f32>(0.0));
    // The reflection probe at the mip of this roughness; the scene's probe
    // is one colour.
    let probe = g.reflection.rgb * m.ext[0].z;

    // Metallic setup with the linear space constants.
    let metallic = m.ext[0].y;
    let spec_color = mix(vec3<f32>(0.0399999991), albedo, metallic);
    let one_minus_reflectivity = 0.959999979 - metallic * 0.959999979;
    let reflectivity = 1.0 - one_minus_reflectivity;
    // Alpha goes into the diffuse colour only.
    let diff_color = albedo * one_minus_reflectivity * alpha;
    let out_alpha = alpha * one_minus_reflectivity + reflectivity;

    // Unity's BRDF1 (Disney diffuse, GGX, Smith joint visibility).
    let h_raw = view + light_dir;
    let h = h_raw * inverseSqrt(max(dot(h_raw, h_raw), 0.00100000005));
    let nv = abs(dot(n, view));
    let nl = clamp(dot(n, light_dir), 0.0, 1.0);
    let nh = clamp(dot(n, h), 0.0, 1.0);
    let lh = clamp(dot(light_dir, h), 0.0, 1.0);

    // fd90 - 1, with fd90 = 0.5 + 2 lh^2 roughness.
    let fd = 2.0 * lh * lh * rough_p - 0.5;
    let p_nl = 1.0 - nl;
    let p_nv = 1.0 - nv;
    let nl5 = p_nl * p_nl * p_nl * p_nl * p_nl;
    let nv5 = p_nv * p_nv * p_nv * p_nv * p_nv;
    let diffuse_term = (1.0 + fd * nl5) * (1.0 + fd * nv5) * nl;

    let rough = max(rough_p * rough_p, 0.00200000009);
    let lambda_v = nl * (nv * (1.0 - rough) + rough);
    let lambda_l = nv * (nl * (1.0 - rough) + rough);
    let vis = 0.5 / (lambda_v + lambda_l + 0.00000999999975);
    let a2 = rough * rough;
    let d = (nh * a2 - nh) * nh + 1.0;
    let ggx = 0.318309873 * a2 / (d * d + 0.00000010000000116860974);
    var spec_term = max(vis * ggx * 3.14159274 * nl, 0.0);
    if (dot(spec_color, spec_color) == 0.0) {
        spec_term = 0.0;
    }
    let surface_reduction = 1.0 / (rough * rough + 1.0);
    let grazing = clamp(reflectivity + m.ext[0].x, 0.0, 1.0);

    let p_lh = 1.0 - lh;
    let lh5 = p_lh * p_lh * p_lh * p_lh * p_lh;
    let fresnel = spec_color + (1.0 - spec_color) * lh5;
    let fresnel_probe = mix(spec_color, vec3<f32>(grazing), nv5);

    // The shader's own change to the lighting: the direct light takes
    // _ShadowPower of its strength and the rest is plain white.
    let lit = ambient * m.ext[0].z + light * diffuse_term * m.ext[1].w + (1.0 - m.ext[1].w);
    let rgb = diff_color * lit
        + spec_term * light * fresnel
        + surface_reduction * probe * fresnel_probe
        + m.ext[0].w;
    return vec4<f32>(rgb, out_alpha);
}

fn mod_forward(i: VsOut, front: bool) -> vec4<f32> {
    if (m.info.z == MOD_KKUSS_ALPHA) {
        return kkuss_alpha(i, front);
    }
    return ibl_alpha(i, front);
}
