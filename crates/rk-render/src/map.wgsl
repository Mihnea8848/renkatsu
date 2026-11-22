// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

// Maps. Materials of Unity's Standard shader are lit as its forward base
// pass lights them (ported from the programs in the game's map bundles,
// see `standard` below); the legacy particle shaders, unlit ones and the
// rest keep a plain path: texture and colour under the sun and the scene's
// ambient light. Linear fog over all. Vertices arrive in world space.

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
    // Unity's unity_IndirectSpecColor, what a material with its
    // reflections switched off reflects.
    indirect_spec: vec4<f32>,
    // Lamps, as the character shaders take them (toon.wgsl), in the
    // map's own space.
    lamps: array<vec4<f32>, 24>,
    // x: how many lamps, y: how much of the map's baked and ambient light
    // and reflections is kept (1 all, 0 none)
    lamp_info: vec4<f32>,
    // Renkatsu's own fog on blended surfaces, see `fog_kept`; w 0 for none.
    fog_own: vec4<f32>,
}

// Renkatsu's own fog on what is drawn after GlobalFog (see-through
// surfaces): how much of the colour is kept, by GlobalFog's maths for its
// exponential squared distance fog and its height fog. `fog_own`: density, the
// mist's top, half its height density, and 1 for distance fog plus 2 for
// height fog (0 for none).
fn fog_kept(world: vec3<f32>, eye: vec3<f32>, p: vec4<f32>) -> f32 {
    let v = world - eye;
    let len = length(v);
    let flags = u32(p.w);
    var way = 0.0;
    if (flags & 1u) != 0u {
        way += len;
    }
    if (flags & 2u) != 0u {
        let over = eye.y - p.y;
        let inside = select(0.0, 1.0, over <= 0.0);
        let point_over = v.y + over;
        let c1 = inside * (point_over + over);
        let c2 = min((1.0 - 2.0 * inside) * point_over, 0.0);
        way += -len * p.z * (c1 - c2 * c2 / abs(v.y + 1e-5));
    }
    let d = p.x * max(way, 0.0);
    return clamp(exp(-d * d), 0.0, 1.0);
}


// The way towards lamp `k` from a point and the light it brings there:
// Unity's falloff over its range and a spot's cone, as on characters.
// Lamps are Renkatsu's own: on the map they shine half as strong again as
// the shader's maths gives. In daylight a map's baked light is brighter
// than a character's own shading and its light falls off sooner with the
// angle than her toon edge, so the same lamp showed less on walls; twice
// the strength matched daylight but outshone her in a dark room.
const LAMP_GAIN: f32 = 1.5;

fn lamp_at(world: vec3<f32>, k: u32) -> array<vec3<f32>, 2> {
    let place = g.lamps[k * 3u];
    let colour = g.lamps[k * 3u + 1u];
    let spot = g.lamps[k * 3u + 2u];
    if place.w < 0.5 {
        return array<vec3<f32>, 2>(normalize(place.xyz), colour.rgb);
    }
    let d = place.xyz - world;
    let q = dot(d, d) / max(colour.w * colour.w, 1e-4);
    var atten = clamp((1.0 - q) * 5.0, 0.0, 1.0) / (1.0 + 25.0 * q);
    let l = normalize(d);
    if spot.w > -1.0 {
        atten *= smoothstep(spot.w, mix(spot.w, 1.0, 0.2), dot(-l, normalize(spot.xyz)));
    }
    return array<vec3<f32>, 2>(l, colour.rgb * atten * LAMP_GAIN * lamp_shadow(k, world));
}

struct Material {
    color: vec4<f32>,
    // Texture scale in xy, offset in zw.
    st: vec4<f32>,
    emission: vec4<f32>,
    // Cut-out threshold (below 0 for none), then 1 when blended, then 1
    // when unlit, then 1 when shadows fall on it.
    params: vec4<f32>,
    // Standard: metallic, smoothness (the scale of the texture's when it
    // comes from one), the normal map's scale, the occlusion's strength.
    surface: vec4<f32>,
    // The detail maps' scale in xy and offset in zw.
    detail_st: vec4<f32>,
    // The detail normal map's scale, 1 when the detail maps read the
    // second UV set, the height map's scale.
    detail: vec4<f32>,
    // The specular setup's specular colour.
    spec_color: vec4<f32>,
    // x: what the material uses, a bit each (see the constants below).
    flags: vec4<u32>,
}

const F_STANDARD: u32 = 1u;
// Metallic in the map's red and smoothness in its alpha; in the specular
// setup the specular colour in its rgb.
const F_GLOSS_MAP: u32 = 2u;
const F_NORMAL_MAP: u32 = 4u;
const F_DETAIL: u32 = 8u;
const F_PARALLAX: u32 = 16u;
// Smoothness from the albedo's alpha.
const F_SMOOTH_ALBEDO: u32 = 32u;
const F_NO_HIGHLIGHTS: u32 = 64u;
const F_NO_REFLECTIONS: u32 = 128u;
const F_PREMULTIPLY: u32 = 256u;
const F_SPECULAR_SETUP: u32 = 512u;

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

// The lamps' shadow tiles: world to clip each, then one over the atlas's
// width and height.
struct LampShadow {
    view_proj: array<mat4x4<f32>, 12>,
    params: vec4<f32>,
}
@group(0) @binding(4) var lamp_atlas: texture_depth_2d;
@group(0) @binding(5) var<uniform> ls: LampShadow;

// How much of a lamp that casts shadows reaches `world`: its slot from the
// place's last number, the cube side (or the spot's one view) facing the
// point, then nine comparisons in its tile of the lamps' atlas. The point
// steps towards the lamp by about a texel there, against shading itself.
fn lamp_shadow(k: u32, world: vec3<f32>) -> f32 {
    let place = g.lamps[k * 3u];
    let slot = i32(round((place.w - 1.0) * 0.5)) - 1;
    if slot < 0 {
        return 1.0;
    }
    let spot = g.lamps[k * 3u + 2u];
    let d = world - place.xyz;
    var face = 0;
    if spot.w <= -1.0 {
        let a = abs(d);
        if a.x >= a.y && a.x >= a.z {
            face = select(1, 0, d.x > 0.0);
        } else if a.y >= a.z {
            face = select(3, 2, d.y > 0.0);
        } else {
            face = select(5, 4, d.z > 0.0);
        }
    }
    let tile = slot * 6 + face;
    let dist = length(d);
    let at = world - d / max(dist, 1e-4) * (0.02 + dist * 0.006);
    let clip = ls.view_proj[tile] * vec4<f32>(at, 1.0);
    if clip.w <= 0.0 {
        return 1.0;
    }
    let p = clip.xyz / clip.w;
    if abs(p.x) > 1.0 || abs(p.y) > 1.0 || p.z < 0.0 || p.z > 1.0 {
        return 1.0;
    }
    let size = vec2<f32>(0.25, 1.0 / 3.0);
    let cell = vec2<f32>(f32(tile % 4), f32(tile / 4)) * size;
    let uv = cell + (p.xy * vec2<f32>(0.5, -0.5) + 0.5) * size;
    // Kept inside the tile, off its neighbours.
    let lo = cell + ls.params.xy * 1.5;
    let hi = cell + size - ls.params.xy * 1.5;
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let tap = clamp(uv + vec2<f32>(f32(x), f32(y)) * ls.params.xy, lo, hi);
            lit += textureSampleCompareLevel(lamp_atlas, shadow_sampler, tap, p.z);
        }
    }
    return lit / 9.0;
}

// The constant and linear part of the ambient light arriving at a surface
// facing `n` (Unity's SHEvalLinearL0L1).
fn sh_l0l1(n: vec3<f32>) -> vec3<f32> {
    let n4 = vec4<f32>(n, 1.0);
    return vec3<f32>(dot(g.sh[0], n4), dot(g.sh[1], n4), dot(g.sh[2], n4));
}

// Its quadratic part (Unity's SHEvalLinearL2).
fn sh_l2(n: vec3<f32>) -> vec3<f32> {
    let b = n.xyzz * n.yzzx;
    let l2 = vec3<f32>(dot(g.sh[3], b), dot(g.sh[4], b), dot(g.sh[5], b));
    let c = n.x * n.x - n.y * n.y;
    return l2 + g.sh[6].rgb * c;
}

// Unity's ShadeSH9: the ambient light arriving at a surface facing `n`.
fn shade_sh9(n: vec3<f32>) -> vec3<f32> {
    return max(sh_l0l1(n) + sh_l2(n), vec3<f32>(0.0));
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
    // The depth bias is one texel of the cascade, as a distance along the
    // rays: the depth row's length is the depth of one metre. A fixed depth
    // would grow with the map's size and lift the shadow of whatever
    // stands on the ground.
    let m = sh.to_atlas[c];
    let bias = sh.texel_world[c] * length(vec3<f32>(m[0].z, m[1].z, m[2].z));
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let at = p.xy + vec2<f32>(f32(x), f32(y)) * sh.params.z;
            lit += textureSampleCompareLevel(shadow_atlas, shadow_sampler, at, p.z - bias);
        }
    }
    lit = lit / 9.0;
    return 1.0 - sh.params.x * (1.0 - lit);
}
@group(1) @binding(0) var<uniform> m: Material;
@group(1) @binding(1) var albedo: texture_2d<f32>;
@group(1) @binding(2) var albedo_sampler: sampler;
@group(1) @binding(3) var emission_map: texture_2d<f32>;
@group(1) @binding(4) var gloss_map: texture_2d<f32>;
@group(1) @binding(5) var normal_map: texture_2d<f32>;
@group(1) @binding(6) var occlusion_map: texture_2d<f32>;
@group(1) @binding(7) var detail_albedo: texture_2d<f32>;
@group(1) @binding(8) var detail_normal: texture_2d<f32>;
@group(1) @binding(9) var detail_mask: texture_2d<f32>;
@group(1) @binding(10) var height_map: texture_2d<f32>;
@group(1) @binding(11) var detail_sampler: sampler;

// 1 when the object has a lightmap, 1 when it is directional, then the
// RGBM scale and exponent.
struct LightmapInfo {
    info: vec4<f32>,
}

@group(2) @binding(0) var<uniform> lm: LightmapInfo;
@group(2) @binding(1) var lightmap: texture_2d<f32>;
@group(2) @binding(2) var lightmap_dir: texture_2d<f32>;
@group(2) @binding(3) var lightmap_sampler: sampler;

// A reflection probe as Unity hands it to shaders: unity_SpecCube0_BoxMax,
// _BoxMin (w: how much of the first probe shows over the second),
// _ProbePosition (w above 0 when the cubemap is projected on the box) and
// _HDR (how the cubemap's colour is decoded).
struct Probe {
    box_max: vec4<f32>,
    box_min: vec4<f32>,
    position: vec4<f32>,
    hdr: vec4<f32>,
}

struct Reflection {
    probes: array<Probe, 2>,
}

@group(3) @binding(0) var<uniform> refl: Reflection;
@group(3) @binding(1) var spec_cube0: texture_cube<f32>;
@group(3) @binding(2) var spec_cube1: texture_cube<f32>;
@group(3) @binding(3) var spec_sampler: sampler;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv2: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) tangent: vec4<f32>,
    @location(6) uv1: vec2<f32>,
    // The object's own matrix, by columns: from where its mesh was baked
    // to where it stands. The identity for an object that never moves.
    @location(7) model0: vec4<f32>,
    @location(8) model1: vec4<f32>,
    @location(9) model2: vec4<f32>,
    @location(10) model3: vec4<f32>,
}

// `v` as long as `like`, or left alone when it has no length.
fn as_long_as(v: vec3<f32>, like: vec3<f32>) -> vec3<f32> {
    let len = length(v);
    if len > 0.0 {
        return v * (length(like) / len);
    }
    return v;
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv2: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) tangent: vec3<f32>,
    @location(6) binormal: vec3<f32>,
    @location(7) uv1: vec2<f32>,
    @location(8) ambient_l2: vec3<f32>,
}

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var out: VsOut;
    // Meshes are baked in the map's space; an object that moves is taken
    // from there by its matrix. Normals turn by its cofactors (the
    // inverse transpose, up to scale) and keep their length; the tangent
    // turns with the surface, and a matrix that mirrors turns the
    // binormal, as Unity's `unity_WorldTransformParams.w` does.
    let c0 = v.model0.xyz;
    let c1 = v.model1.xyz;
    let c2 = v.model2.xyz;
    let at = c0 * v.pos.x + c1 * v.pos.y + c2 * v.pos.z + v.model3.xyz;
    let n1 = cross(c2, c0);
    let turned = cross(c1, c2) * v.normal.x + n1 * v.normal.y + cross(c0, c1) * v.normal.z;
    let normal = as_long_as(turned, v.normal);
    let tangent = as_long_as(c0 * v.tangent.x + c1 * v.tangent.y + c2 * v.tangent.z, v.tangent.xyz);
    let side = select(-1.0, 1.0, dot(c1, n1) >= 0.0);
    out.clip = g.view_proj * vec4<f32>(at, 1.0);
    out.world = at;
    out.normal = normal;
    out.uv = v.uv;
    out.uv2 = v.uv2;
    out.color = v.color;
    // Standard's vertex program: the binormal is normal x tangent, turned
    // by the tangent's sign (which already holds the sign of a mirrored
    // object), and the quadratic part of the ambient light is worked out
    // here, from the vertex normal.
    out.tangent = tangent;
    out.binormal = cross(normal, tangent) * (v.tangent.w * side);
    out.uv1 = v.uv1;
    out.ambient_l2 = sh_l2(normal);
    return out;
}

// Where a reflected ray leaves the probe's box, seen from the probe: the
// direction its cubemap is read at (Unity's BoxProjectedCubemapDirection).
fn probe_direction(r: vec3<f32>, world: vec3<f32>, p: Probe) -> vec3<f32> {
    if p.position.w > 0.0 {
        let nr = normalize(r);
        let to_max = (p.box_max.xyz - world) / nr;
        let to_min = (p.box_min.xyz - world) / nr;
        let far = select(to_min, to_max, nr > vec3<f32>(0.0));
        let reach = min(min(far.x, far.y), far.z);
        return world - p.position.xyz + nr * reach;
    }
    return r;
}

// Unity's DecodeHDR as the programs compute it in linear colour.
fn decode_hdr(c: vec4<f32>, hdr: vec4<f32>) -> vec3<f32> {
    let alpha = hdr.w * (c.a - 1.0) + 1.0;
    return c.rgb * (hdr.x * pow(max(alpha, 0.0), hdr.y));
}

// What the surroundings reflect along `r`: the object's reflection probe,
// rougher surfaces reading a smaller level of its cubemap, and a second
// probe blended in when Unity blends two.
fn environment(r: vec3<f32>, world: vec3<f32>, rough: f32) -> vec3<f32> {
    let level = rough * (1.7 - 0.7 * rough) * 6.0;
    let d0 = probe_direction(r, world, refl.probes[0]);
    let c0 = decode_hdr(textureSampleLevel(spec_cube0, spec_sampler, d0, level), refl.probes[0].hdr);
    let blend = refl.probes[0].box_min.w;
    if blend < 0.99999 {
        let d1 = probe_direction(r, world, refl.probes[1]);
        let c1 = decode_hdr(textureSampleLevel(spec_cube1, spec_sampler, d1, level), refl.probes[1].hdr);
        return mix(c1, c0, blend);
    }
    return c0;
}

fn pow5(x: f32) -> f32 {
    let x2 = x * x;
    return x2 * x2 * x;
}

// A normal map's texel as Standard unpacks it: x in alpha, y in green,
// both scaled, z from what is left of the unit length.
fn unpack_normal(texel: vec4<f32>, scale: f32) -> vec3<f32> {
    let xy = (texel.ag * 2.0 - 1.0) * scale;
    return vec3<f32>(xy, sqrt(1.0 - min(dot(xy, xy), 1.0)));
}

// What a Standard surface is made of, after its textures are read.
struct Surface {
    diffuse: vec3<f32>,
    specular: vec3<f32>,
    one_minus_reflectivity: f32,
    smoothness: f32,
    occlusion: f32,
    // The shading normal and the unit vector from the eye to the surface.
    n: vec3<f32>,
    eye: vec3<f32>,
}

// Unity's Standard shader, forward base pass, BRDF1, from the programs of
// the `Standard` shader in the map bundles (2f.unity3d: 470 plain, 726 and
// 727 with every map, without and with a lightmap, 654 and 718 with
// highlights and reflections off; their tier has box projection and probe
// blending). `light` is the sun's colour after its shadow, `ambient` the
// baked or ambient light on the surface.
// How much of the map's sun shines at its Light intensity: it dims with
// the rest of the map's light, but keeps a little, so that a ray of sun or
// moon through a window still shows in a dark room.
fn sun_share() -> f32 {
    return mix(0.15, 1.0, g.lamp_info.y);
}

fn standard(s: Surface, world: vec3<f32>, light: vec3<f32>, ambient: vec3<f32>) -> vec3<f32> {
    let flags = m.flags.x;
    let rough = 1.0 - s.smoothness;
    let r = s.eye - 2.0 * dot(s.eye, s.n) * s.n;
    var env = g.indirect_spec.rgb;
    if (flags & F_NO_REFLECTIONS) == 0u {
        env = environment(r, world, rough);
    }
    env = env * s.occlusion * g.lamp_info.y;
    let a = rough * rough;
    let nv = abs(dot(s.n, -s.eye));
    let surface_reduction = 1.0 / (a * a + 1.0);
    let grazing = clamp(s.smoothness + 1.0 - s.one_minus_reflectivity, 0.0, 1.0);
    let fresnel_env = mix(s.specular, vec3<f32>(grazing), pow5(1.0 - nv));
    var lit = s.diffuse * ambient * s.occlusion + surface_reduction * env * fresnel_env;
    lit += standard_light(s, g.sun_dir.xyz, light);
    // The lamps, which cast no shadows.
    for (var k = 0u; k < u32(g.lamp_info.x); k++) {
        let lamp = lamp_at(world, k);
        lit += standard_light(s, lamp[0], lamp[1]);
    }
    return lit;
}

// What one light `light`, coming from `l`, adds to a Standard surface.
fn standard_light(s: Surface, l: vec3<f32>, light: vec3<f32>) -> vec3<f32> {
    let flags = m.flags.x;
    let rough = 1.0 - s.smoothness;

    let to_half = l - s.eye;
    let h = to_half * inverseSqrt(max(dot(to_half, to_half), 0.001));
    let nv = abs(dot(s.n, -s.eye));
    let nl = clamp(dot(s.n, l), 0.0, 1.0);
    let nh = clamp(dot(s.n, h), 0.0, 1.0);
    let lh = clamp(dot(l, h), 0.0, 1.0);

    // Disney's diffuse term.
    let fd90 = 2.0 * lh * lh * rough - 0.5;
    let light_scatter = 1.0 + fd90 * pow5(1.0 - nl);
    let view_scatter = 1.0 + fd90 * pow5(1.0 - nv);
    let diffuse_term = light_scatter * view_scatter * nl;

    // GGX: Smith's joint visibility and the distribution.
    let a = rough * rough;
    var specular_term = 0.0;
    if (flags & F_NO_HIGHLIGHTS) == 0u {
        let lambda_v = nl * (nv * (1.0 - a) + a);
        let lambda_l = nv * (nl * (1.0 - a) + a);
        let visibility = 0.5 / (lambda_v + lambda_l + 1e-5);
        let a2 = a * a;
        let d = (nh * a2 - nh) * nh + 1.0;
        let distribution = a2 * 0.31830987 / (d * d + 1e-7);
        specular_term = max(visibility * distribution * 3.1415927 * nl, 0.0);
        if dot(s.specular, s.specular) == 0.0 {
            specular_term = 0.0;
        }
    }
    let fresnel = s.specular + (1.0 - s.specular) * pow5(1.0 - lh);
    return s.diffuse * light * diffuse_term + specular_term * light * fresnel;
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if in.world.y > g.eye.w {
        discard;
    }
    let flags = m.flags.x;
    let is_standard = (flags & F_STANDARD) != 0u;
    let to_eye = g.eye.xyz - in.world;
    var uv = in.uv * m.st.xy + m.st.zw;
    // The detail maps have their own tiling and may read the second UV set.
    var uv_detail = select(in.uv, in.uv1, m.detail.y > 0.5) * m.detail_st.xy + m.detail_st.zw;
    // Standard's height map moves every map along the view, by the height
    // read at the place the surface is seen (Unity's ParallaxOffset1Step).
    let height = textureSample(height_map, albedo_sampler, uv).g;
    if (flags & F_PARALLAX) != 0u {
        var view = normalize(vec3<f32>(dot(in.tangent, to_eye), dot(in.binormal, to_eye), dot(in.normal, to_eye)));
        view.z = view.z + 0.42;
        let shift = (height * m.detail.z - m.detail.z * 0.5) * (view.xy / view.z);
        uv = uv + shift;
        uv_detail = uv_detail + shift;
    }
    let texel = textureSample(albedo, albedo_sampler, uv);
    // Standard reads every map with the main texture's tiling.
    let glow = textureSample(emission_map, albedo_sampler, uv).rgb;
    let gloss_texel = textureSample(gloss_map, albedo_sampler, uv);
    let normal_texel = textureSample(normal_map, albedo_sampler, uv);
    let occlusion_texel = textureSample(occlusion_map, albedo_sampler, uv).g;
    let detail_mask_texel = textureSample(detail_mask, albedo_sampler, uv).a;
    let detail_texel = textureSample(detail_albedo, detail_sampler, uv_detail).rgb;
    let detail_normal_texel = textureSample(detail_normal, detail_sampler, uv_detail);
    // Sampled before anything is discarded, where control flow is uniform.
    let baked_rgbm = textureSample(lightmap, lightmap_sampler, in.uv2);
    let baked_dir = textureSample(lightmap_dir, lightmap_sampler, in.uv2);
    // Unity's legacy particle shaders (params.z 2), which paint foliage:
    // 2 * vertex colour * tint * texture, alpha too, as their plain
    // variant computes it.
    let particle = m.params.z > 1.5;
    let base = select(texel * m.color, 2.0 * in.color * m.color * texel, particle);
    if m.params.x >= 0.0 && base.a < m.params.x {
        discard;
    }
    var n = normalize(in.normal);
    if is_standard {
        // Standard does not turn the normal of a back face, two-sided or
        // not. With a normal map the frame's vectors are used as they
        // arrive, not made unit again.
        if (flags & F_NORMAL_MAP) != 0u {
            var nt = unpack_normal(normal_texel, m.surface.z);
            if (flags & F_DETAIL) != 0u {
                let dn = unpack_normal(detail_normal_texel, m.detail.x);
                let both = normalize(vec3<f32>(nt.xy + dn.xy, nt.z * dn.z));
                nt = mix(nt, both, detail_mask_texel);
            }
            n = normalize(in.tangent * nt.x + in.binormal * nt.y + in.normal * nt.z);
        }
    } else if !front {
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
    // The baked light, Unity's DecodeLightmapRGBM in linear colour, and for
    // a directional lightmap DecodeDirectionalLightmap's half Lambert term.
    // The map's sun is real-time, so the lightmap holds the rest: sky,
    // lamps and bounces.
    let lightmapped = lm.info.x > 0.5;
    var baked = lm.info.z * pow(baked_rgbm.a, lm.info.w) * baked_rgbm.rgb;
    if lm.info.y > 0.5 {
        let half_lambert = dot(n, baked_dir.xyz - 0.5) + 0.5;
        baked = baked * half_lambert / max(1e-4, baked_dir.w);
    }
    var color: vec3<f32>;
    var alpha = base.a;
    if is_standard {
        var albedo_rgb = base.rgb;
        if (flags & F_DETAIL) != 0u {
            // Twice the detail (4.59 is 2 in linear colour) where the mask
            // lets it show.
            albedo_rgb = albedo_rgb * (detail_texel * detail_mask_texel * 4.5947938 + (1.0 - detail_mask_texel));
        }
        var s: Surface;
        var gloss = m.surface.y;
        if (flags & F_SMOOTH_ALBEDO) != 0u {
            gloss = texel.a * m.surface.y;
        } else if (flags & F_GLOSS_MAP) != 0u {
            gloss = gloss_texel.a * m.surface.y;
        }
        if (flags & F_SPECULAR_SETUP) != 0u {
            // The specular setup: the specular colour is given, and the
            // diffuse colour gives way to its strongest channel.
            s.specular = select(m.spec_color.rgb, gloss_texel.rgb, (flags & F_GLOSS_MAP) != 0u);
            s.one_minus_reflectivity = 1.0 - max(max(s.specular.r, s.specular.g), s.specular.b);
        } else {
            let metallic = select(m.surface.x, gloss_texel.r, (flags & F_GLOSS_MAP) != 0u);
            s.specular = mix(vec3<f32>(0.04), albedo_rgb, metallic);
            s.one_minus_reflectivity = 0.96 - metallic * 0.96;
        }
        s.diffuse = albedo_rgb * s.one_minus_reflectivity;
        if (flags & F_PREMULTIPLY) != 0u {
            // Glass: the diffuse colour fades with alpha, reflections stay.
            s.diffuse = s.diffuse * alpha;
            alpha = 1.0 - s.one_minus_reflectivity + alpha * s.one_minus_reflectivity;
        }
        s.smoothness = gloss;
        s.occlusion = 1.0 - m.surface.w + occlusion_texel * m.surface.w;
        s.n = n;
        s.eye = -normalize(to_eye);
        var ambient = baked;
        if !lightmapped {
            ambient = max(sh_l0l1(n) + in.ambient_l2, vec3<f32>(0.0));
        }
        ambient *= g.lamp_info.y;
        let light = g.sun_color.rgb * shade * select(0.0, 1.0, sunlit) * sun_share();
        color = standard(s, in.world, light, ambient) + m.emission.rgb * glow * g.lamp_info.y;
    } else {
        var sun = g.sun_color.rgb * facing * shade * sun_share();
        for (var k = 0u; k < u32(g.lamp_info.x); k++) {
            let lamp = lamp_at(in.world, k);
            sun += lamp[1] * max(dot(n, lamp[0]), 0.0);
        }
        var indirect = shade_sh9(n);
        if lightmapped {
            indirect = baked;
        }
        indirect *= g.lamp_info.y;
        // An unlit surface's texture holds its light: it is baked light too.
        let light = select(indirect + sun, vec3<f32>(g.lamp_info.y), m.params.z > 0.5);
        color = base.rgb * light + m.emission.rgb * glow * g.lamp_info.y;
    }
    if g.fog_range.w > 0.5 {
        let d = distance(in.world, g.eye.xyz);
        let f = clamp((g.fog_range.y - d) / max(g.fog_range.y - g.fog_range.x, 1e-4), 0.0, 1.0);
        color = mix(g.fog_color.rgb, color, f);
    }
    // GlobalFog runs before the blended queue; these take the fog here.
    if g.fog_own.w > 0.5 && m.params.y > 0.5 {
        color = mix(g.fog_color.rgb, color, fog_kept(in.world, g.eye.xyz, g.fog_own));
    }
    return vec4<f32>(color, select(1.0, alpha, m.params.y > 0.5));
}
