// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

// A ghost of a character: its meshes in another pose, as one flat tint.
// The vertices are skinned here, from the buffers the skinning pass of
// `skin.wgsl` reads, with a pose buffer of the ghost's own; the push-out
// of skirts is left out.

struct Globals {
    view_proj: mat4x4<f32>,
    // Straight colour and opacity, as the screen shows them.
    tint: vec4<f32>,
}

// As in `skin.wgsl`.
struct Capsule {
    a: vec4<f32>,
    b: vec4<f32>,
}

struct Header {
    mode: u32,
    count: u32,
    has_normals: u32,
    has_tangents: u32,
    node: mat4x4<f32>,
    normal: mat4x4<f32>,
    push: vec4<u32>,
    floor: vec4<f32>,
    capsules: array<Capsule, 32>,
}

struct Bone {
    m: mat4x4<f32>,
    valid: vec4<u32>,
}

struct Pose {
    h: Header,
    bones: array<Bone>,
}

struct SkinVertex {
    bones: vec4<u32>,
    weights: vec4<f32>,
}

@group(0) @binding(0) var<uniform> g: Globals;
// The scene's depth, and the depth of the character itself alone, both
// cleared to 1.
@group(0) @binding(1) var scene_depth: texture_depth_2d;
@group(0) @binding(2) var own_depth: texture_depth_2d;

@group(1) @binding(0) var<storage, read> pose: Pose;
@group(1) @binding(1) var<storage, read> base: array<vec4<f32>>;
@group(1) @binding(2) var<storage, read> skin: array<SkinVertex>;

fn vertex_matrix(vi: u32) -> mat4x4<f32> {
    if pose.h.mode == 1u {
        return pose.h.node;
    }
    let sv = skin[vi];
    var acc = mat4x4<f32>(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0));
    var total = 0.0;
    var any_weight = false;
    let bone_count = arrayLength(&pose.bones);
    for (var k = 0u; k < 4u; k++) {
        let w = sv.weights[k];
        if w <= 0.0 {
            continue;
        }
        any_weight = true;
        let b = sv.bones[k];
        if b < bone_count && pose.bones[b].valid.x == 1u {
            acc += pose.bones[b].m * w;
            total += w;
        }
    }
    if total > 0.0 {
        return acc * (1.0 / total);
    }
    if !any_weight {
        return mat4x4<f32>(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0), pose.h.node[3]);
    }
    return pose.h.node;
}

// The same place from both passes, so that the tint's pass finds the
// depth the first pass left.
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @invariant @builtin(position) vec4<f32> {
    let world = vertex_matrix(vi) * vec4<f32>(base[vi * 3u].xyz, 1.0);
    return g.view_proj * vec4<f32>(world.xyz, 1.0);
}

@fragment
fn fs_main(@invariant @builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(scene_depth));
    let pixel = clamp(vec2<i32>(frag.xy), vec2<i32>(0), size - vec2<i32>(1));
    // Under the character itself, wherever it shows.
    if textureLoad(own_depth, pixel, 0) < 1.0 {
        discard;
    }
    // Behind what the scene drew there.
    let drawn = textureLoad(scene_depth, pixel, 0);
    if drawn < 1.0 && drawn < frag.z - 1e-5 {
        discard;
    }
    return vec4<f32>(g.tint.rgb * g.tint.a, g.tint.a);
}
