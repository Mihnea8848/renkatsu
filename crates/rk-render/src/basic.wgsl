// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

// First-pass shading: base texture times colour with a soft toon ramp.
// Colour maths stays in gamma space, as Koikatsu's own shaders work.

struct Globals {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    eye: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;

struct Material {
    color: vec4<f32>,
    // xy: uv scale, zw: uv offset
    st: vec4<f32>,
    // x: alpha cutoff, y: 1 when unlit
    params: vec4<f32>,
};
@group(1) @binding(0) var<uniform> m: Material;
@group(1) @binding(1) var tex: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) world: vec3<f32>,
};

@vertex
fn vs(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>) -> VsOut {
    var o: VsOut;
    o.clip = g.view_proj * vec4<f32>(pos, 1.0);
    o.normal = normal;
    o.uv = uv * m.st.xy + m.st.zw;
    o.world = pos;
    return o;
}

@fragment
fn fs(i: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, i.uv);
    var c = t * m.color;
    if (c.a < m.params.x) {
        discard;
    }
    var n = normalize(i.normal);
    if (!front) {
        n = -n;
    }
    let ndl = dot(n, normalize(g.light_dir.xyz));
    let lit = smoothstep(-0.05, 0.25, ndl);
    let shade = mix(0.78, 1.0, lit);
    let rgb = mix(c.rgb * shade, c.rgb, m.params.y);
    return vec4<f32>(rgb, 1.0);
}
