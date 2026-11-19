// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

// The outline of what is selected: the pixels just outside the
// silhouette drawn into `mask`, a depth texture cleared to 1.

struct Params {
    color: vec4<f32>,
    // The outline's width in pixels, then padding.
    width: vec4<f32>,
}

@group(0) @binding(0) var mask: texture_depth_2d;
@group(0) @binding(1) var<uniform> params: Params;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle over the whole target.
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn inside(p: vec2<i32>, size: vec2<i32>) -> bool {
    let q = clamp(p, vec2<i32>(0), size - vec2<i32>(1));
    return textureLoad(mask, q, 0) < 1.0;
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(mask));
    let p = vec2<i32>(pos.xy);
    if inside(p, size) {
        discard;
    }
    let r = i32(ceil(params.width.x));
    let reach = params.width.x * params.width.x;
    var near = false;
    for (var y = -r; y <= r; y++) {
        for (var x = -r; x <= r; x++) {
            if f32(x * x + y * y) <= reach && inside(p + vec2<i32>(x, y), size) {
                near = true;
            }
        }
    }
    if !near {
        discard;
    }
    return params.color;
}
