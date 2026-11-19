// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

// Maps seen from the sun, depth only: one cascade of the shadow map at a
// time. Cut-out surfaces keep only what their alpha keeps.

struct Cascade {
    view_proj: mat4x4<f32>,
}

// The same as the map pass's material.
struct Material {
    color: vec4<f32>,
    st: vec4<f32>,
    emission: vec4<f32>,
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> cascade: Cascade;
@group(1) @binding(0) var<uniform> m: Material;
@group(1) @binding(1) var albedo: texture_2d<f32>;
@group(1) @binding(2) var albedo_sampler: sampler;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv2: vec2<f32>,
    @location(4) color: vec4<f32>,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = cascade.view_proj * vec4<f32>(v.pos, 1.0);
    out.uv = v.uv;
    return out;
}

@fragment
fn fs_cutout(in: VsOut) {
    let a = textureSample(albedo, albedo_sampler, in.uv * m.st.xy + m.st.zw).a * m.color.a;
    if a < m.params.x {
        discard;
    }
}
