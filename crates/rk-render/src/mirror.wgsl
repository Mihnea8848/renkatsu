// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

// A mirror's surface: the game's FX/MirrorReflection_Transparent.
// Its vertex program hands on ComputeScreenPos(position); its pixel
// program reads _ReflectionTex at xy / w, which is the pixel's own place
// on the screen, and multiplies it by _MainTex and _Color:
//   rgb = reflection.rgb * main.rgb * _Color.rgb
//   a   = main.a * _Color.a
// No mirror of the game has a _MainTex, which then reads white.

struct Surface {
    view_proj: mat4x4<f32>,
    color: vec4<f32>,
    // xy: the picture's width and height in pixels.
    screen: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: Surface;
@group(0) @binding(1) var reflection: texture_2d<f32>;
@group(0) @binding(2) var reflection_sampler: sampler;

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return u.view_proj * vec4<f32>(position, 1.0);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy / u.screen.xy;
    // The reflection is drawn flipped left to right (see mirror.rs).
    let r = textureSampleLevel(reflection, reflection_sampler, vec2<f32>(1.0 - uv.x, uv.y), 0.0);
    // The game's texture has 8 bits a channel and holds nothing above 1.
    let rgb = clamp(r.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(rgb * u.color.rgb, u.color.a);
}
