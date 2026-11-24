// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! `IKSolver.Point` and `IKSolver.Node`, the points the full body solver
//! moves. Port of RootMotion FinalIK (`IKSolver.cs`). `IKSolver.Bone` is
//! left out: the full body solver does not use it.

use glam::{Quat, Vec3};

/// `IKSolver.Node` with the fields of `IKSolver.Point` it extends.
#[derive(Clone, Debug)]
pub struct Node {
    /// The transform, by its index in the hierarchy.
    pub transform: usize,
    pub weight: f32,
    pub solver_position: Vec3,
    pub solver_rotation: Quat,
    pub default_local_position: Vec3,
    pub default_local_rotation: Quat,
    pub length: f32,
    pub effector_position_weight: f32,
    pub effector_rotation_weight: f32,
    pub offset: Vec3,
}

impl Node {
    /// `new IKSolver.Node(transform)`.
    pub fn new(transform: usize) -> Self {
        Self {
            transform,
            weight: 1.0,
            solver_position: Vec3::ZERO,
            solver_rotation: Quat::IDENTITY,
            default_local_position: Vec3::ZERO,
            default_local_rotation: Quat::IDENTITY,
            length: 0.0,
            effector_position_weight: 0.0,
            effector_rotation_weight: 0.0,
            offset: Vec3::ZERO,
        }
    }
}
