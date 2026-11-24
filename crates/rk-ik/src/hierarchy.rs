// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! A tree of transforms with Unity's parent and child rules, as far as the
//! solver needs them: the world place of a transform from the local ones
//! above it, and setting a world position or rotation on a child.
//!
//! Checked against the captured bones of CharaStudio
//! (`tests/fbbik_studio.rs`): a world position is the local one carried
//! through every parent's scale, rotation and position; a world rotation is
//! the product of the local rotations, scale left out. Setting a world
//! rotation or position is written as the engine is known to do it (the
//! parent's inverse applied, the rotation made unit) and is checked only
//! through the solver's results. Negative scales are not handled.

use crate::unity;
use glam::{Quat, Vec3};

/// One transform: its parent and its local place.
#[derive(Clone, Debug)]
pub struct Transform {
    pub parent: Option<usize>,
    pub local_position: Vec3,
    pub local_rotation: Quat,
    pub local_scale: Vec3,
}

/// Transforms by index; a parent comes before or after its children, any
/// order works.
#[derive(Clone, Debug, Default)]
pub struct Hierarchy {
    pub transforms: Vec<Transform>,
}

impl Hierarchy {
    pub fn new(transforms: Vec<Transform>) -> Self {
        Self { transforms }
    }

    /// Adds a transform and gives its index.
    pub fn push(&mut self, transform: Transform) -> usize {
        self.transforms.push(transform);
        self.transforms.len() - 1
    }

    /// `Transform.parent`.
    pub fn parent(&self, i: usize) -> Option<usize> {
        self.transforms[i].parent
    }

    /// `Transform.localPosition`.
    pub fn local_position(&self, i: usize) -> Vec3 {
        self.transforms[i].local_position
    }

    /// `Transform.localRotation`.
    pub fn local_rotation(&self, i: usize) -> Quat {
        self.transforms[i].local_rotation
    }

    pub fn set_local_position(&mut self, i: usize, p: Vec3) {
        self.transforms[i].local_position = p;
    }

    pub fn set_local_rotation(&mut self, i: usize, q: Quat) {
        self.transforms[i].local_rotation = q;
    }

    /// `Transform.position`.
    pub fn position(&self, i: usize) -> Vec3 {
        let t = &self.transforms[i];
        match t.parent {
            Some(p) => self.transform_point(p, t.local_position),
            None => t.local_position,
        }
    }

    /// `Transform.rotation`.
    pub fn rotation(&self, i: usize) -> Quat {
        let mut q = self.transforms[i].local_rotation;
        let mut at = self.transforms[i].parent;
        while let Some(p) = at {
            q = unity::mul(self.transforms[p].local_rotation, q);
            at = self.transforms[p].parent;
        }
        q
    }

    /// `Transform.TransformPoint`: a point of this transform's space, in
    /// the world.
    pub fn transform_point(&self, i: usize, point: Vec3) -> Vec3 {
        let mut p = point;
        let mut at = Some(i);
        while let Some(n) = at {
            let t = &self.transforms[n];
            p = t.local_position + unity::rotate(t.local_rotation, t.local_scale * p);
            at = t.parent;
        }
        p
    }

    /// `Transform.InverseTransformPoint`: a point of the world, in this
    /// transform's space. A scale of nothing gives nothing.
    pub fn inverse_transform_point(&self, i: usize, point: Vec3) -> Vec3 {
        let t = &self.transforms[i];
        let p = match t.parent {
            Some(parent) => self.inverse_transform_point(parent, point),
            None => point,
        };
        let p = unity::rotate(unity::inverse(t.local_rotation), p - t.local_position);
        let inv = |s: f32| if s.abs() > f32::EPSILON { 1.0 / s } else { 0.0 };
        Vec3::new(
            p.x * inv(t.local_scale.x),
            p.y * inv(t.local_scale.y),
            p.z * inv(t.local_scale.z),
        )
    }

    /// `Transform.position = p`.
    pub fn set_position(&mut self, i: usize, p: Vec3) {
        let local = match self.transforms[i].parent {
            Some(parent) => self.inverse_transform_point(parent, p),
            None => p,
        };
        self.transforms[i].local_position = local;
    }

    /// `Transform.rotation = q`.
    pub fn set_rotation(&mut self, i: usize, q: Quat) {
        let local = match self.transforms[i].parent {
            Some(parent) => unity::mul(unity::inverse(self.rotation(parent)), q),
            None => q,
        };
        self.transforms[i].local_rotation = normalize_safe(local);
    }
}

/// The engine's `NormalizeSafe` for rotations: identity when there is no
/// length to speak of.
fn normalize_safe(q: Quat) -> Quat {
    let m = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
    if m < 1e-5 {
        Quat::IDENTITY
    } else {
        Quat::from_xyzw(q.x / m, q.y / m, q.z / m, q.w / m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Hierarchy {
        let mut h = Hierarchy::default();
        let root = h.push(Transform {
            parent: None,
            local_position: Vec3::new(1.0, 2.0, 3.0),
            local_rotation: unity::angle_axis(90.0, Vec3::Y),
            local_scale: Vec3::splat(2.0),
        });
        let mid = h.push(Transform {
            parent: Some(root),
            local_position: Vec3::new(0.0, 1.0, 0.0),
            local_rotation: unity::angle_axis(30.0, Vec3::X),
            local_scale: Vec3::new(1.0, 0.5, 1.0),
        });
        h.push(Transform {
            parent: Some(mid),
            local_position: Vec3::new(0.0, 0.0, 1.0),
            local_rotation: Quat::IDENTITY,
            local_scale: Vec3::ONE,
        });
        h
    }

    #[test]
    fn world_place_goes_through_every_parent() {
        let h = tree();
        // The middle one: (0, 1, 0) doubled, turned (Y stays), moved.
        assert!((h.position(1) - Vec3::new(1.0, 4.0, 3.0)).length() < 1e-6);
        // Rotation is the product of the local ones.
        let want = unity::mul(unity::angle_axis(90.0, Vec3::Y), unity::angle_axis(30.0, Vec3::X));
        assert!(unity::quat_dot(h.rotation(2), want).abs() > 0.999_999);
    }

    #[test]
    fn setting_a_world_place_reads_back() {
        let mut h = tree();
        let p = Vec3::new(-0.3, 0.7, 2.5);
        h.set_position(2, p);
        assert!((h.position(2) - p).length() < 1e-5, "{}", h.position(2));
        let q = unity::look_rotation(Vec3::new(0.2, 0.5, -0.8), Vec3::Y);
        h.set_rotation(2, q);
        assert!(unity::quat_dot(h.rotation(2), q).abs() > 0.999_999);
        // The parent did not move.
        assert!((h.position(1) - Vec3::new(1.0, 4.0, 3.0)).length() < 1e-6);
    }

    #[test]
    fn points_go_to_local_space_and_back() {
        let h = tree();
        let p = Vec3::new(0.4, -1.2, 0.9);
        let back = h.transform_point(1, h.inverse_transform_point(1, p));
        assert!((back - p).length() < 1e-5);
    }
}
