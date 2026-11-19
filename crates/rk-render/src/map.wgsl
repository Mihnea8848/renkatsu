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
    ambient: vec4<f32>,
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
    // when unlit.
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> m: Material;
@group(1) @binding(1) var albedo: texture_2d<f32>;
@group(1) @binding(2) var albedo_sampler: sampler;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = g.view_proj * vec4<f32>(v.pos, 1.0);
    out.world = v.pos;
    out.normal = v.normal;
    out.uv = v.uv;
    return out;
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if in.world.y > g.eye.w {
        discard;
    }
    let texel = textureSample(albedo, albedo_sampler, in.uv * m.st.xy + m.st.zw);
    let base = texel * m.color;
    if m.params.x >= 0.0 && base.a < m.params.x {
        discard;
    }
    var n = normalize(in.normal);
    if !front {
        n = -n;
    }
    let sun = g.sun_color.rgb * max(dot(n, g.sun_dir.xyz), 0.0);
    let light = select(g.ambient.rgb + sun, vec3<f32>(1.0), m.params.z > 0.5);
    var color = base.rgb * light + m.emission.rgb;
    if g.fog_range.w > 0.5 {
        let d = distance(in.world, g.eye.xyz);
        let f = clamp((g.fog_range.y - d) / max(g.fog_range.y - g.fog_range.x, 1e-4), 0.0, 1.0);
        color = mix(g.fog_color.rgb, color, f);
    }
    let alpha = select(1.0, base.a, m.params.y > 0.5);
    return vec4<f32>(color, alpha);
}
