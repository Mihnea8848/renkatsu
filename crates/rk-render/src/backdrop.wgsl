// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

// The plain backdrop of a scene without a map: the screen effects stay on
// what was drawn, and the empty parts show the backdrop colour as it is.
// How much of a pixel was drawn on comes from the main pass's depth
// samples, so outlines keep their smoothing.

// The backdrop colour, linear.
@group(0) @binding(0) var<uniform> backdrop: vec4f;
@group(0) @binding(1) var picture: texture_2d<f32>;
@group(0) @binding(2) var scene_depth: texture_depth_multisampled_2d;

@vertex
fn vs_backdrop(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_backdrop(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    let p = vec2i(pos.xy);
    let samples = i32(textureNumSamples(scene_depth));
    var drawn = 0.0;
    for (var s = 0; s < samples; s++) {
        if textureLoad(scene_depth, p, s) < 1.0 {
            drawn += 1.0;
        }
    }
    let c = textureLoad(picture, p, 0);
    return mix(backdrop, c, drawn / f32(samples));
}
