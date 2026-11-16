// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

// Skinning on the GPU: carries a mesh's vertices into the world and writes
// them into its vertex buffer, as `DrawMesh::posed` does on the CPU.

struct Header {
    // 0: bone blending, 1: one matrix for the whole mesh.
    mode: u32,
    count: u32,
    has_normals: u32,
    has_tangents: u32,
    // The renderer's world matrix, or the rigid matrix.
    node: mat4x4<f32>,
    // The rigid matrix's inverse transpose, for normals.
    normal: mat4x4<f32>,
}

struct Bone {
    m: mat4x4<f32>,
    // x is 1 when the bone exists.
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

@group(0) @binding(0) var<storage, read> pose: Pose;
// Three per vertex: position, normal, tangent (with its sign in w).
@group(0) @binding(1) var<storage, read> base: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> skin: array<SkinVertex>;
// The vertex buffer, as floats.
@group(0) @binding(3) var<storage, read_write> out: array<f32>;

// Floats per vertex, and where the posed fields sit in a vertex.
const STRIDE: u32 = 22u;
const POS: u32 = 0u;
const NORMAL: u32 = 3u;
const TANGENT: u32 = 18u;

// As glam's normalize_or_zero.
fn normalize_or_zero(v: vec3<f32>) -> vec3<f32> {
    let len = length(v);
    if len > 0.0 {
        let rcp = 1.0 / len;
        if rcp < 3.4e38 {
            return v * rcp;
        }
    }
    return vec3<f32>(0.0);
}

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
        // A vertex without weights lands on the origin of the skinned space.
        return mat4x4<f32>(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0), pose.h.node[3]);
    }
    return pose.h.node;
}

@compute @workgroup_size(64)
fn cs_skin(@builtin(global_invocation_id) id: vec3<u32>) {
    let vi = id.x;
    if vi >= pose.h.count {
        return;
    }
    let m = vertex_matrix(vi);
    let o = vi * STRIDE;
    let p = (m * vec4<f32>(base[vi * 3u].xyz, 1.0)).xyz;
    out[o + POS] = p.x;
    out[o + POS + 1u] = p.y;
    out[o + POS + 2u] = p.z;

    var n = vec3<f32>(0.0, 0.0, 1.0);
    if pose.h.has_normals == 1u {
        var nm = m;
        if pose.h.mode == 1u {
            nm = pose.h.normal;
        }
        n = normalize_or_zero((nm * vec4<f32>(base[vi * 3u + 1u].xyz, 0.0)).xyz);
    }
    out[o + NORMAL] = n.x;
    out[o + NORMAL + 1u] = n.y;
    out[o + NORMAL + 2u] = n.z;

    var t = vec4<f32>(1.0, 0.0, 0.0, 1.0);
    if pose.h.has_tangents == 1u {
        let bt = base[vi * 3u + 2u];
        t = vec4<f32>(normalize_or_zero((m * vec4<f32>(bt.xyz, 0.0)).xyz), bt.w);
    }
    out[o + TANGENT] = t.x;
    out[o + TANGENT + 1u] = t.y;
    out[o + TANGENT + 2u] = t.z;
    out[o + TANGENT + 3u] = t.w;
}
