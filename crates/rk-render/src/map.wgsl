// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

// Maps, plainly lit: the material's texture and colour under the sun and
// the scene's ambient light, with linear fog. Vertices arrive in world
// space.

struct Globals {
    view_proj: mat4x4<f32>,
    // The eye, and in w the height above which nothing is drawn.
    eye: vec4<f32>,
    // Unit vector towards the sun, and the sun's linear colour times its
    // intensity.
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    // Unity's unity_SHAr, SHAg, SHAb, SHBr, SHBg, SHBb and SHC.
    sh: array<vec4<f32>, 7>,
    // Linear colour, then start and end in metres; w of `fog_range` is 1
    // when fog is on.
    fog_color: vec4<f32>,
    fog_range: vec4<f32>,
}

struct Material {
    color: vec4<f32>,
    // Texture scale in xy, offset in zw.
    st: vec4<f32>,
    emission: vec4<f32>,
    // Cut-out threshold (below 0 for none), then 1 when blended, then 1
    // when unlit, then 1 when shadows fall on it.
    params: vec4<f32>,
}

// The sun's cascaded shadow map: world to atlas for each cascade, where
// each cascade ends (distance along the view), and the shadow's strength,
// whether there is one, one atlas texel in uv and in each cascade's metres.
struct Shadow {
    to_atlas: array<mat4x4<f32>, 4>,
    ends: vec4<f32>,
    params: vec4<f32>,
    texel_world: vec4<f32>,
    forward: vec4<f32>,
}

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var<uniform> sh: Shadow;
@group(0) @binding(2) var shadow_atlas: texture_depth_2d;
@group(0) @binding(3) var shadow_sampler: sampler_comparison;

// Unity's ShadeSH9: the ambient light arriving at a surface facing `n`.
fn shade_sh9(n: vec3<f32>) -> vec3<f32> {
    let n4 = vec4<f32>(n, 1.0);
    let l1 = vec3<f32>(dot(g.sh[0], n4), dot(g.sh[1], n4), dot(g.sh[2], n4));
    let b = n.xyzz * n.yzzx;
    let l2 = vec3<f32>(dot(g.sh[3], b), dot(g.sh[4], b), dot(g.sh[5], b));
    let c = n.x * n.x - n.y * n.y;
    return max(l1 + l2 + g.sh[6].rgb * c, vec3<f32>(0.0));
}

// How much of the sun reaches a point: 1 in full sun.
fn sun_visibility(world: vec3<f32>, n: vec3<f32>) -> f32 {
    if sh.params.y < 0.5 {
        return 1.0;
    }
    let depth = dot(world - g.eye.xyz, sh.forward.xyz);
    if depth > sh.ends.w {
        return 1.0;
    }
    var c = 0;
    if depth > sh.ends.x { c = 1; }
    if depth > sh.ends.y { c = 2; }
    if depth > sh.ends.z { c = 3; }
    // Off the surface by a texel and a half, against the surface shadowing
    // itself.
    let p = sh.to_atlas[c] * vec4<f32>(world + n * sh.texel_world[c] * 1.5, 1.0);
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let at = p.xy + vec2<f32>(f32(x), f32(y)) * sh.params.z;
            lit += textureSampleCompareLevel(shadow_atlas, shadow_sampler, at, p.z - 0.0005);
        }
    }
    lit = lit / 9.0;
    return 1.0 - sh.params.x * (1.0 - lit);
}
@group(1) @binding(0) var<uniform> m: Material;
@group(1) @binding(1) var albedo: texture_2d<f32>;
@group(1) @binding(2) var albedo_sampler: sampler;
@group(1) @binding(3) var emission_map: texture_2d<f32>;

// 1 when the object has a lightmap, 1 when it is directional, then the
// RGBM scale and exponent.
struct LightmapInfo {
    info: vec4<f32>,
}

@group(2) @binding(0) var<uniform> lm: LightmapInfo;
@group(2) @binding(1) var lightmap: texture_2d<f32>;
@group(2) @binding(2) var lightmap_dir: texture_2d<f32>;
@group(2) @binding(3) var lightmap_sampler: sampler;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv2: vec2<f32>,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv2: vec2<f32>,
}

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = g.view_proj * vec4<f32>(v.pos, 1.0);
    out.world = v.pos;
    out.normal = v.normal;
    out.uv = v.uv;
    out.uv2 = v.uv2;
    return out;
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if in.world.y > g.eye.w {
        discard;
    }
    let uv = in.uv * m.st.xy + m.st.zw;
    let texel = textureSample(albedo, albedo_sampler, uv);
    // Standard reads every map with the main texture's tiling.
    let glow = textureSample(emission_map, albedo_sampler, uv).rgb;
    // Sampled before anything is discarded, where control flow is uniform.
    let baked_rgbm = textureSample(lightmap, lightmap_sampler, in.uv2);
    let baked_dir = textureSample(lightmap_dir, lightmap_sampler, in.uv2);
    let base = texel * m.color;
    if m.params.x >= 0.0 && base.a < m.params.x {
        discard;
    }
    var n = normalize(in.normal);
    if !front {
        n = -n;
    }
    // params.w: 1 when shadows fall on it, 2 more when the sun lights it.
    let sunlit = m.params.w > 1.5;
    let receives = m.params.w - select(0.0, 2.0, sunlit) > 0.5;
    var facing = 0.0;
    if sunlit {
        facing = max(dot(n, g.sun_dir.xyz), 0.0);
    }
    var shade = 1.0;
    if facing > 0.0 && receives {
        shade = sun_visibility(in.world, n);
    }
    let sun = g.sun_color.rgb * facing * shade;
    // The baked light, Unity's DecodeLightmapRGBM in linear colour, and for
    // a directional lightmap DecodeDirectionalLightmap's half Lambert term.
    // The map's sun is real-time, so the lightmap holds the rest: sky,
    // lamps and bounces.
    var indirect = shade_sh9(n);
    if lm.info.x > 0.5 {
        var baked = lm.info.z * pow(baked_rgbm.a, lm.info.w) * baked_rgbm.rgb;
        if lm.info.y > 0.5 {
            let half_lambert = dot(n, baked_dir.xyz - 0.5) + 0.5;
            baked = baked * half_lambert / max(1e-4, baked_dir.w);
        }
        indirect = baked;
    }
    let light = select(indirect + sun, vec3<f32>(1.0), m.params.z > 0.5);
    var color = base.rgb * light + m.emission.rgb * glow;
    if g.fog_range.w > 0.5 {
        let d = distance(in.world, g.eye.xyz);
        let f = clamp((g.fog_range.y - d) / max(g.fog_range.y - g.fog_range.x, 1e-4), 0.0, 1.0);
        color = mix(g.fog_color.rgb, color, f);
    }
    let alpha = select(1.0, base.a, m.params.y > 0.5);
    return vec4<f32>(color, alpha);
}
