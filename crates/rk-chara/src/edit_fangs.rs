// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Edit Fangs plugin: the card sets the length and spacing of each fang,
//! and the plugin moves the vertices of the 42-vertex fang mesh
//! `cf_O_canine` to match. On load it runs its adjustment six times (three
//! `readjust` calls, each adjusting twice); every pass turns the fang around
//! a pivot read from the mesh the pass before left, so the passes add up.

use glam::{Quat, Vec3};
use rk_card::{ExtData, Value};
use rk_unityfs::objects::Mesh;

pub const KEY: &str = "org.njaecha.plugins.editfangs";

/// The fang mesh's renderer, under the head.
pub const RENDERER: &str = "cf_O_canine";

/// Passes the plugin runs when a card loads.
const PASSES: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fangs {
    pub scale_l: f32,
    pub scale_r: f32,
    pub spacing_l: f32,
    pub spacing_r: f32,
}

impl Default for Fangs {
    fn default() -> Self {
        Self {
            scale_l: 0.1,
            scale_r: 0.1,
            spacing_l: 1.0,
            spacing_r: 1.0,
        }
    }
}

/// The card's fang settings. A card without them keeps the mesh as it is.
pub fn read(ext: &ExtData) -> Option<Fangs> {
    let v = ext.get(KEY)?.nested("fangData")?;
    let f = |i: usize| match &v {
        Value::Array(a) => a.get(i).and_then(Value::as_f64).map(|x| x as f32),
        _ => None,
    };
    let d = Fangs::default();
    Some(Fangs {
        scale_l: f(0).unwrap_or(d.scale_l),
        scale_r: f(1).unwrap_or(d.scale_r),
        spacing_l: f(2).unwrap_or(d.spacing_l),
        spacing_r: f(3).unwrap_or(d.spacing_r),
    })
}

/// The fang mesh as the plugin leaves it, or `None` when the mesh is not
/// the 42-vertex one the plugin works on.
pub fn apply(mesh: &Mesh, fangs: Fangs) -> Option<Mesh> {
    if mesh.positions.len() != 42 || mesh.normals.len() != 42 {
        return None;
    }
    let base: Vec<Vec3> = mesh.positions.iter().map(|p| Vec3::from(*p)).collect();
    let base_n: Vec<Vec3> = mesh.normals.iter().map(|n| Vec3::from(*n)).collect();
    // The tips are the first two vertices at the lowest height; the pivots
    // are found by exact position, as registerFangs does.
    let low = base.iter().map(|v| v.y).fold(f32::INFINITY, f32::min);
    let tips: Vec<usize> = (0..42).filter(|&i| base[i].y == low).collect();
    if tips.len() < 2 {
        return None;
    }
    let find = |p: Vec3| base.iter().position(|v| *v == p).unwrap_or(0);
    let rotp = [
        find(Vec3::new(-0.007_559_223, 0.016_371_48, 0.083_963_65)),
        find(Vec3::new(0.007_559_223, 0.016_371_48, 0.083_963_65)),
    ];
    let spread = |s: f32| (s * std::f32::consts::FRAC_PI_2).cos() * if s <= 1.0 { 1.0 } else { 2.3 };
    let (num_l, num_r) = (spread(fangs.spacing_l), spread(fangs.spacing_r));
    let (rot_l, rot_r) = (
        Quat::from_rotation_y((num_l * 30.0).to_radians()),
        Quat::from_rotation_y((num_r * -30.0).to_radians()),
    );
    let mut cur = base.clone();
    let mut normals = base_n.clone();
    for _ in 0..PASSES {
        let (pivot_l, pivot_r) = (cur[rotp[0]] * 0.99, cur[rotp[1]] * 0.99);
        let mut next = vec![Vec3::ZERO; 42];
        for i in 0..42 {
            let mut v = base[i];
            if i >= 21 {
                v.x -= 0.01 * (1.0 - fangs.spacing_r);
                v.y -= 0.001 * num_r;
                v.z += 0.0014 * num_r;
                v = pivot_r + rot_r * (v - pivot_r);
                normals[i] = rot_r * base_n[i];
                if i == tips[1] {
                    v.y += 0.01 * (0.1 - fangs.scale_r);
                    v.z += 0.002 * (0.1 - fangs.scale_r);
                }
            } else {
                v.x += 0.01 * (1.0 - fangs.spacing_l);
                v.y -= 0.001 * num_l;
                v.z += 0.0014 * num_l;
                v = pivot_l + rot_l * (v - pivot_l);
                normals[i] = rot_l * base_n[i];
                if i == tips[0] {
                    v.y += 0.01 * (0.1 - fangs.scale_l);
                    v.z += 0.002 * (0.1 - fangs.scale_l);
                }
            }
            next[i] = v;
        }
        cur = next;
    }
    let mut out = mesh.clone();
    out.positions = cur.iter().map(|v| v.to_array()).collect();
    out.normals = normals.iter().map(|v| v.to_array()).collect();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 42-vertex mesh: the two pivots, two tips at the lowest height, and
    /// the rest spread above them.
    fn fang_mesh() -> Mesh {
        let mut positions: Vec<[f32; 3]> = (0..42)
            .map(|i| [if i < 21 { -0.01 } else { 0.01 }, 0.03 + i as f32 * 0.0001, 0.08])
            .collect();
        positions[3] = [-0.007_559_223, 0.016_371_48, 0.083_963_65];
        positions[24] = [0.007_559_223, 0.016_371_48, 0.083_963_65];
        positions[5] = [-0.008, 0.0, 0.085];
        positions[26] = [0.008, 0.0, 0.085];
        Mesh {
            positions,
            normals: vec![[0.0, 0.0, 1.0]; 42],
            ..Default::default()
        }
    }

    #[test]
    fn default_settings_only_settle_the_pivots() {
        let m = fang_mesh();
        let out = apply(&m, Fangs::default()).unwrap();
        // At the default spacing cos(pi/2) is almost zero: nothing turns,
        // and the default length leaves the tips where they are.
        for (a, b) in m.positions.iter().zip(&out.positions) {
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() < 1e-5, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn a_longer_fang_moves_its_tip_down() {
        let m = fang_mesh();
        let long = Fangs {
            scale_l: 1.0,
            ..Fangs::default()
        };
        let out = apply(&m, long).unwrap();
        // 0.01 * (0.1 - 1.0) lower and 0.002 * (0.1 - 1.0) further back.
        assert!((out.positions[5][1] - (0.0 - 0.009)).abs() < 1e-5);
        assert!((out.positions[5][2] - (0.085 - 0.0018)).abs() < 1e-5);
        // The right fang is untouched.
        assert!((out.positions[26][1]).abs() < 1e-5);
    }

    #[test]
    fn other_meshes_are_left_alone() {
        let mut m = fang_mesh();
        m.positions.pop();
        assert!(apply(&m, Fangs::default()).is_none());
    }
}
