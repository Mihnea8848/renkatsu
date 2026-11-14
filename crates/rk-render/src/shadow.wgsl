// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

// The screen-sized shadow mask of the directional light, after Unity's
// Hidden/Internal-ScreenSpaceShadows pixel shader as the game ships it
// (unity_builtin_extra, the PCF 5x5 program for several cascades picked by
// view depth). For every pixel of the camera's depth texture it finds the
// surface's place in the shadow map and writes how lit it is.

struct Shadow {
    // camera clip space to world
    inv_view_proj: mat4x4<f32>,
    // world to atlas place and depth, per cascade (unity_WorldToShadow)
    world_to_shadow: array<mat4x4<f32>, 4>,
    // view depth where each cascade starts and ends
    splits_near: vec4<f32>,
    splits_far: vec4<f32>,
    eye: vec4<f32>,
    forward: vec4<f32>,
    // x: mask value in full shadow (_LightShadowData.x), y: atlas side in
    // texels, z: one over it
    params: vec4<f32>,
};
@group(0) @binding(0) var<uniform> s: Shadow;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var shadow_tex: texture_depth_2d;
@group(0) @binding(3) var shadow_samp: sampler_comparison;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

// One triangle over the whole target.
@vertex
fn vs_mask(@builtin(vertex_index) index: u32) -> VsOut {
    var o: VsOut;
    let p = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    o.clip = vec4<f32>(p, 0.0, 1.0);
    o.ndc = p;
    return o;
}

// Weights and places of the three taps of one axis of the 5x5 tent.
struct Axis {
    w: vec3<f32>,
    at: vec3<f32>,
};

fn pcf_axis(frac: f32) -> Axis {
    var a: Axis;
    a.w = vec3<f32>(4.0 - 3.0 * frac, 7.0, 1.0 + 3.0 * frac);
    a.at = vec3<f32>((3.0 - 2.0 * frac) / a.w.x - 2.0, (3.0 + frac) * 0.142857149, frac / a.w.z + 2.0);
    return a;
}

fn tap(base: vec2<f32>, x: f32, y: f32, depth: f32) -> f32 {
    return textureSampleCompareLevel(shadow_tex, shadow_samp, (base + vec2<f32>(x, y)) * s.params.z, depth);
}

@fragment
fn fs_mask(i: VsOut) -> @location(0) vec4<f32> {
    let depth = textureLoad(depth_tex, vec2<i32>(i.clip.xy), 0);
    let h = s.inv_view_proj * vec4<f32>(i.ndc, depth, 1.0);
    let world = h.xyz / h.w;
    let view_z = dot(world - s.eye.xyz, s.forward.xyz);

    // The cascade whose depth range holds the pixel.
    let z = vec4<f32>(view_z);
    var weights = vec4<f32>(z >= s.splits_near) * vec4<f32>(z < s.splits_far);
    // Nothing drawn here: the game's far plane lies beyond every cascade.
    if (depth >= 1.0) {
        weights = vec4<f32>(0.0);
    }
    let p = vec4<f32>(world, 1.0);
    let coord = (s.world_to_shadow[0] * p).xyz * weights.x
        + (s.world_to_shadow[1] * p).xyz * weights.y
        + (s.world_to_shadow[2] * p).xyz * weights.z
        + (s.world_to_shadow[3] * p).xyz * weights.w;

    // 5x5 tent from nine bilinear comparisons.
    let t = coord.xy * s.params.y + 0.5;
    let frac = fract(t);
    let base = floor(t) - 0.5;
    let ax = pcf_axis(frac.x);
    let ay = pcf_axis(frac.y);
    var sum = 0.0;
    sum += ax.w.x * ay.w.x * tap(base, ax.at.x, ay.at.x, coord.z);
    sum += ax.w.y * ay.w.x * tap(base, ax.at.y, ay.at.x, coord.z);
    sum += ax.w.z * ay.w.x * tap(base, ax.at.z, ay.at.x, coord.z);
    sum += ax.w.x * ay.w.y * tap(base, ax.at.x, ay.at.y, coord.z);
    sum += ax.w.y * ay.w.y * tap(base, ax.at.y, ay.at.y, coord.z);
    sum += ax.w.z * ay.w.y * tap(base, ax.at.z, ay.at.y, coord.z);
    sum += ax.w.x * ay.w.z * tap(base, ax.at.x, ay.at.z, coord.z);
    sum += ax.w.y * ay.w.z * tap(base, ax.at.y, ay.at.z, coord.z);
    sum += ax.w.z * ay.w.z * tap(base, ax.at.z, ay.at.z, coord.z);
    var lit = sum * 0.00694444450;
    // Outside every cascade the game compares against the nearest depth,
    // which always passes.
    if (dot(weights, vec4<f32>(1.0)) == 0.0) {
        lit = 1.0;
    }
    let value = lit * (1.0 - s.params.x) + s.params.x;
    return vec4<f32>(value);
}
