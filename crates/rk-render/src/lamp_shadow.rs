// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Shadows of lamps: the map and the characters seen from each point or
//! spot lamp that casts shadows, into one depth atlas the map's and the
//! characters' shaders both read.
//!
//! A spot takes one tile, looking along its cone; a point lamp takes six,
//! one per side of a cube around it; a directional lamp three, cascades
//! over nearer and farther slices of the camera's view, as the sun's. Two
//! lamps cast shadows at most, as
//! CharaStudio allows two added lights. The atlas is four tiles across and
//! three down, `SLOTS` times `FACES` tiles in all.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use crate::Lamp;

/// How many lamps cast shadows at once.
pub const SLOTS: usize = 2;
/// The tiles one lamp may take: the six sides of a point lamp's cube.
pub const FACES: usize = 6;
/// The side of one tile, in texels.
pub const TILE: u32 = 512;
const COLUMNS: u32 = 4;
const ROWS: u32 = 3;
/// The atlas's size in texels.
pub const ATLAS: [u32; 2] = [COLUMNS * TILE, ROWS * TILE];
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Nearer than this to a lamp nothing casts.
const NEAR: f32 = 0.05;
/// The slices of the camera's view a directional lamp's cascades cover,
/// in metres from the camera.
const CASCADES: [(f32, f32); 3] = [(0.03, 4.0), (4.0, 14.0), (14.0, 40.0)];
/// How far beyond the view towards a directional lamp casters still cast.
const REACH: f32 = 60.0;

/// What the shaders read: each tile's world to clip matrix, one over the
/// atlas's width and height, and for each slot the width of a texel of
/// its first three tiles in metres (a directional lamp's cascades).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct LampShadowUniform {
    pub view_proj: [[f32; 16]; SLOTS * FACES],
    pub params: [f32; 4],
    pub texel: [[f32; 4]; SLOTS],
}

/// The lamps that cast shadows by their slot, from lists in which a lamp
/// may come up more than once (the map's and the characters').
pub(crate) fn by_slot<'a>(lamps: impl IntoIterator<Item = &'a Lamp>) -> [Option<Lamp>; SLOTS] {
    let mut slots = [None; SLOTS];
    for lamp in lamps {
        if let Some(slot) = lamp.shadow {
            if let Some(free @ None) = slots.get_mut(slot as usize) {
                *free = Some(*lamp);
            }
        }
    }
    slots
}

/// The views of one lamp, world to clip, in the order of its tiles: one
/// for a spot, six for a point lamp (+X, -X, +Y, -Y, +Z, -Z, as the
/// shaders pick them), three for a directional lamp, fitted to `sight`.
pub(crate) fn views(lamp: &Lamp, sight: &crate::shadow::View) -> Vec<Mat4> {
    let Some(at) = lamp.at.map(Vec3::from) else {
        return cascades(Vec3::from(lamp.dir), sight);
    };
    let far = lamp.range.max(NEAR * 2.0);
    let look = |dir: Vec3, fov: f32| {
        let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
        Mat4::perspective_lh(fov.to_radians(), 1.0, NEAR, far) * Mat4::look_to_lh(at, dir, up)
    };
    match lamp.spot_angle {
        Some(angle) => {
            let dir = Vec3::from(lamp.dir).normalize_or(Vec3::NEG_Y);
            // A little wider than the cone, for its soft edge.
            vec![look(dir, (angle * 1.1).clamp(1.0, 170.0))]
        }
        None => [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z]
            .into_iter()
            .map(|dir| look(dir, 90.0))
            .collect(),
    }
}

/// A directional lamp's cascades: each a square around a slice of the
/// view, snapped to whole texels so it does not shimmer as the camera
/// moves, deep enough to take in what stands towards the lamp.
fn cascades(towards: Vec3, sight: &crate::shadow::View) -> Vec<Mat4> {
    let light = crate::shadow::light_view(towards);
    CASCADES
        .iter()
        .map(|&(near, far)| {
            let slice = sight.slice_corners(near, far);
            let centre = slice.iter().fold(Vec3::ZERO, |a, p| a + *p) / 8.0;
            let radius = slice.iter().map(|p| p.distance(centre)).fold(0.0, f32::max);
            let size = radius * 2.0 * (1.0 + 20.0 / TILE as f32);
            let texel = size / TILE as f32;
            let c = light.transform_point3(centre);
            let snap = |v: f32| (v / texel).floor() * texel;
            let (cx, cy, half) = (snap(c.x), snap(c.y), size * 0.5);
            let (lo, hi) = slice
                .iter()
                .map(|p| light.transform_point3(*p).z)
                .fold((f32::MAX, f32::MIN), |(lo, hi), z| (lo.min(z), hi.max(z)));
            Mat4::orthographic_lh(cx - half, cx + half, cy - half, cy + half, lo - REACH, hi + 1.0) * light
        })
        .collect()
}

/// The tile of a lamp's view in the atlas, by slot and side.
pub(crate) fn tile(slot: usize, face: usize) -> usize {
    slot * FACES + face
}

/// Where a tile starts in the atlas, in texels.
pub(crate) fn origin(tile: usize) -> [u32; 2] {
    let tile = tile as u32;
    [(tile % COLUMNS) * TILE, (tile / COLUMNS) * TILE]
}

/// The uniform for the slots' views, each taken after `space` (identity
/// for the world; the map's root for the map's own space).
pub(crate) fn uniform(views: &[Vec<Mat4>; SLOTS], space: Mat4) -> LampShadowUniform {
    let mut u = LampShadowUniform::zeroed();
    for (slot, views) in views.iter().enumerate() {
        for (face, view) in views.iter().enumerate() {
            u.view_proj[tile(slot, face)] = (*view * space).to_cols_array();
            if face < 3 {
                // An orthographic view's first row is two over its width.
                let row = Vec3::new(view.x_axis.x, view.y_axis.x, view.z_axis.x).length();
                u.texel[slot][face] = 2.0 / row.max(1e-6) / TILE as f32;
            }
        }
    }
    u.params = [1.0 / ATLAS[0] as f32, 1.0 / ATLAS[1] as f32, 0.0, 0.0];
    u
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lamp(at: [f32; 3], spot: Option<f32>, shadow: Option<u8>) -> Lamp {
        Lamp {
            at: Some(at),
            dir: [0.0, -1.0, 0.0],
            color: [1.0; 3],
            intensity: 1.0,
            range: 4.0,
            spot_angle: spot,
            shadow,
        }
    }

    #[test]
    fn a_point_lamp_sees_each_side_through_one_face() {
        let l = lamp([1.0, 2.0, 3.0], None, Some(0));
        let views = views(&l, &sight());
        assert_eq!(views.len(), FACES);
        let at = Vec3::new(1.0, 2.0, 3.0);
        // A point a metre off each way lies in the middle of its face.
        for (face, dir) in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z]
            .iter()
            .enumerate()
        {
            let p = views[face].project_point3(at + *dir);
            assert!(
                p.x.abs() < 1e-4 && p.y.abs() < 1e-4 && p.z > 0.0 && p.z < 1.0,
                "{face}: {p}"
            );
        }
    }

    #[test]
    fn a_spot_looks_along_its_cone() {
        let l = lamp([0.0, 3.0, 0.0], Some(60.0), Some(1));
        let views = views(&l, &sight());
        assert_eq!(views.len(), 1);
        let p = views[0].project_point3(Vec3::new(0.0, 1.0, 0.0));
        assert!(p.x.abs() < 1e-4 && p.y.abs() < 1e-4 && p.z > 0.0 && p.z < 1.0, "{p}");
    }

    fn sight() -> crate::shadow::View {
        crate::shadow::View {
            eye: Vec3::new(0.0, 1.5, -4.0),
            forward: Vec3::Z,
            up: Vec3::Y,
            fov_y_deg: 30.0,
            aspect: 16.0 / 9.0,
        }
    }

    #[test]
    fn a_directional_lamp_covers_the_view_near_and_far() {
        let mut l = lamp([0.0; 3], None, Some(0));
        l.at = None;
        l.dir = [0.3, 0.8, -0.4];
        let views = views(&l, &sight());
        assert_eq!(views.len(), 3);
        // What the camera sees near, middling and far each lies in its
        // cascade, and something above towards the lamp still casts.
        for (i, depth) in [2.0, 9.0, 30.0].into_iter().enumerate() {
            let seen = sight().eye + Vec3::Z * depth;
            let p = views[i].project_point3(seen);
            assert!(p.x.abs() < 1.0 && p.y.abs() < 1.0 && p.z > 0.0 && p.z < 1.0, "{i}: {p}");
            let above = seen + Vec3::from(l.dir).normalize() * 10.0;
            let q = views[i].project_point3(above);
            assert!(q.z > 0.0 && q.z < p.z, "{i}: {q}");
        }
        let u = uniform(&[views, Vec::new()], Mat4::IDENTITY);
        assert!(u.texel[0][0] > 0.0 && u.texel[0][0] < u.texel[0][2], "{:?}", u.texel[0]);
    }

    #[test]
    fn lamps_keep_their_slots_once_each() {
        let a = lamp([0.0; 3], None, Some(1));
        let b = lamp([1.0; 3], Some(30.0), Some(0));
        let c = lamp([2.0; 3], None, None);
        let mut d = lamp([3.0; 3], None, Some(1));
        d.at = None;
        let slots = by_slot([&a, &b, &c, &a, &d]);
        assert_eq!(slots[0].map(|l| l.at), Some(b.at));
        assert_eq!(slots[1].map(|l| l.at), Some(a.at));
        // Tiles of the second lamp follow the first's six.
        assert_eq!(tile(1, 0), FACES);
        assert_eq!(origin(tile(1, 0)), [2 * TILE, TILE]);
    }
}
