// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Splits every triangle of a mesh in four, for the study of how fine a
//! garment has to be to follow the cloth net. Renkatsu's own. Each new
//! vertex is the middle of an edge and takes the mean of what its two
//! ends carry, so the picture is unchanged until the cloth moves it.

use rk_unityfs::objects::{BlendShapeVertex, BoneWeight, Mesh};
use std::collections::HashMap;

/// The mean of two skinned vertices' weights: the four bones that carry
/// most, made to sum to one.
fn mean_skin(a: &BoneWeight, b: &BoneWeight) -> BoneWeight {
    let mut all: Vec<(u32, f32)> = Vec::with_capacity(8);
    for sv in [a, b] {
        for k in 0..4 {
            if sv.weights[k] <= 0.0 {
                continue;
            }
            match all.iter_mut().find(|(bone, _)| *bone == sv.bones[k]) {
                Some((_, w)) => *w += sv.weights[k] * 0.5,
                None => all.push((sv.bones[k], sv.weights[k] * 0.5)),
            }
        }
    }
    all.sort_by(|x, y| y.1.total_cmp(&x.1));
    all.truncate(4);
    let total: f32 = all.iter().map(|(_, w)| w).sum();
    let mut out = BoneWeight::default();
    for (k, (bone, w)) in all.into_iter().enumerate() {
        out.bones[k] = bone;
        out.weights[k] = if total > 0.0 { w / total } else { 0.0 };
    }
    out
}

/// The mesh with every triangle split in four at the middles of its
/// edges. Submeshes that are not triangles are left as they are.
pub fn subdivide(mesh: &Mesh) -> Mesh {
    let mut out = mesh.clone();
    let mut middle: HashMap<(u32, u32), u32> = HashMap::new();
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    let mut indices: Vec<u32> = Vec::with_capacity(mesh.indices.len() * 4);
    let count = mesh.positions.len() as u32;
    let mut mid = |a: u32, b: u32, pairs: &mut Vec<(u32, u32)>| -> u32 {
        let key = (a.min(b), a.max(b));
        *middle.entry(key).or_insert_with(|| {
            pairs.push(key);
            count + pairs.len() as u32 - 1
        })
    };
    for sub in &mut out.submeshes {
        let from = sub.first_index as usize;
        let old = &mesh.indices[from..from + sub.index_count as usize];
        sub.first_index = indices.len() as u32;
        if sub.topology != 0 {
            indices.extend_from_slice(old);
        } else {
            for t in old.chunks_exact(3) {
                let (a, b, c) = (t[0], t[1], t[2]);
                let (ab, bc, ca) = (mid(a, b, &mut pairs), mid(b, c, &mut pairs), mid(c, a, &mut pairs));
                indices.extend_from_slice(&[a, ab, ca, ab, b, bc, ca, bc, c, ab, bc, ca]);
            }
        }
        sub.index_count = indices.len() as u32 - sub.first_index;
    }
    out.indices = indices;
    let mean3 = |v: &[[f32; 3]], a: u32, b: u32| {
        let (p, q) = (v[a as usize], v[b as usize]);
        [(p[0] + q[0]) * 0.5, (p[1] + q[1]) * 0.5, (p[2] + q[2]) * 0.5]
    };
    let full = mesh.positions.len();
    for &(a, b) in &pairs {
        out.positions.push(mean3(&mesh.positions, a, b));
        if mesh.normals.len() == full {
            out.normals.push(mean3(&mesh.normals, a, b));
        }
        if mesh.tangents.len() == full {
            let (p, q) = (mesh.tangents[a as usize], mesh.tangents[b as usize]);
            out.tangents
                .push([(p[0] + q[0]) * 0.5, (p[1] + q[1]) * 0.5, (p[2] + q[2]) * 0.5, p[3]]);
        }
        if mesh.colors.len() == full {
            let (p, q) = (mesh.colors[a as usize], mesh.colors[b as usize]);
            out.colors.push(std::array::from_fn(|k| (p[k] + q[k]) * 0.5));
        }
        for (uv, from) in out.uv.iter_mut().zip(&mesh.uv) {
            if from.len() == full {
                let (p, q) = (from[a as usize], from[b as usize]);
                uv.push([(p[0] + q[0]) * 0.5, (p[1] + q[1]) * 0.5]);
            }
        }
        if mesh.skin.len() == full {
            out.skin.push(mean_skin(&mesh.skin[a as usize], &mesh.skin[b as usize]));
        }
    }
    // A blend shape moves a new vertex by the mean of what it moves the
    // two ends by.
    for (shape, from) in out.blend_shapes.iter_mut().zip(&mesh.blend_shapes) {
        let by_index: HashMap<u32, &BlendShapeVertex> = from.vertices.iter().map(|v| (v.index, v)).collect();
        for (k, &(a, b)) in pairs.iter().enumerate() {
            let (va, vb) = (by_index.get(&a), by_index.get(&b));
            if va.is_none() && vb.is_none() {
                continue;
            }
            let mut v = **va.or(vb).expect("one of the two");
            let half = |x: Option<&&BlendShapeVertex>, f: fn(&BlendShapeVertex) -> [f32; 3]| {
                x.map_or([0.0; 3], |v| f(v).map(|c| c * 0.5))
            };
            let sum = |p: [f32; 3], q: [f32; 3]| [p[0] + q[0], p[1] + q[1], p[2] + q[2]];
            v.index = count + k as u32;
            v.position = sum(half(va, |v| v.position), half(vb, |v| v.position));
            v.normal = sum(half(va, |v| v.normal), half(vb, |v| v.normal));
            shape.vertices.push(v);
        }
    }
    out
}

/// Splits the triangles of every skinned mesh that hangs on skirt bones
/// `times` over: a switch for the study (`RK_NET_SUBDIV`), to run before
/// the cloth is fitted.
pub fn skirts(scene: &mut crate::scene::Scene, times: u32) {
    let nodes = &scene.nodes;
    for r in &mut scene.renderers {
        let on_skirt = r.bones.iter().flatten().any(|&n| nodes[n].name.contains("_sk_"));
        if !r.skinned || !on_skirt {
            continue;
        }
        for _ in 0..times {
            r.mesh = std::sync::Arc::new(subdivide(&r.mesh));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_unityfs::objects::SubMesh;

    #[test]
    fn a_quad_becomes_eight_triangles_that_share_their_middles() {
        let mesh = Mesh {
            positions: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [0.0, 2.0, 0.0]],
            uv: [
                vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                vec![],
                vec![],
                vec![],
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            submeshes: vec![SubMesh {
                first_index: 0,
                index_count: 6,
                topology: 0,
            }],
            skin: vec![
                BoneWeight {
                    weights: [1.0, 0.0, 0.0, 0.0],
                    bones: [0, 0, 0, 0],
                },
                BoneWeight {
                    weights: [1.0, 0.0, 0.0, 0.0],
                    bones: [1, 0, 0, 0],
                },
                BoneWeight {
                    weights: [1.0, 0.0, 0.0, 0.0],
                    bones: [1, 0, 0, 0],
                },
                BoneWeight {
                    weights: [1.0, 0.0, 0.0, 0.0],
                    bones: [0, 0, 0, 0],
                },
            ],
            ..Mesh::default()
        };
        let fine = subdivide(&mesh);
        // Four corners and five edges, the diagonal shared.
        assert_eq!(fine.positions.len(), 9);
        assert_eq!(fine.indices.len(), 24);
        assert_eq!(fine.submeshes[0].index_count, 24);
        assert_eq!(fine.uv[0].len(), 9);
        // The middle of the edge from 0 to 1.
        assert_eq!(fine.positions[4], [1.0, 0.0, 0.0]);
        assert_eq!(fine.uv[0][4], [0.5, 0.0]);
        let sv = fine.skin[4];
        assert!((sv.weights[0] - 0.5).abs() < 1e-6 && (sv.weights[1] - 0.5).abs() < 1e-6);
        assert_ne!(sv.bones[0], sv.bones[1]);
    }
}
