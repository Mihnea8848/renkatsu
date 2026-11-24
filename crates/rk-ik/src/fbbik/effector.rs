// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! `IKEffector`: pulls a node of the solver towards a position and
//! rotation. Port of RootMotion FinalIK (`IKEffector.cs`).

use super::chain::{get_chain_and_node_indexes, FbikChain};
use crate::hierarchy::Hierarchy;
use crate::unity;
use glam::{Quat, Vec3};

/// Where a transform outside the body (an effector's target) is in the
/// world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub position: Vec3,
    pub rotation: Quat,
}

#[derive(Clone, Debug)]
pub struct IkEffector {
    pub bone: usize,
    /// World place of the `target` transform, `None` when there is none.
    pub target: Option<Place>,
    pub position_weight: f32,
    pub rotation_weight: f32,
    pub position: Vec3,
    pub rotation: Quat,
    pub position_offset: Vec3,
    pub effect_child_nodes: bool,
    pub maintain_relative_position_weight: f32,
    pub child_bones: Vec<usize>,
    pub plane_bone1: Option<usize>,
    pub plane_bone2: Option<usize>,
    pub plane_bone3: Option<usize>,
    pub plane_rotation_offset: Quat,
    // Private in FinalIK.
    pub pos_w: f32,
    pub rot_w: f32,
    pub local_positions: Vec<Vec3>,
    pub use_plane_nodes: bool,
    pub animated_plane_rotation: Quat,
    pub animated_position: Vec3,
    pub first_update: bool,
    pub chain_index: i32,
    pub node_index: i32,
    pub plane1_chain_index: i32,
    pub plane1_node_index: i32,
    pub plane2_chain_index: i32,
    pub plane2_node_index: i32,
    pub plane3_chain_index: i32,
    pub plane3_node_index: i32,
    pub child_chain_indexes: Vec<i32>,
    pub child_node_indexes: Vec<i32>,
    pub is_end_effector: bool,
}

impl IkEffector {
    /// `new IKEffector()` with its bone.
    pub fn new(bone: usize) -> Self {
        Self {
            bone,
            target: None,
            position_weight: 0.0,
            rotation_weight: 0.0,
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            position_offset: Vec3::ZERO,
            effect_child_nodes: true,
            maintain_relative_position_weight: 0.0,
            child_bones: Vec::new(),
            plane_bone1: None,
            plane_bone2: None,
            plane_bone3: None,
            plane_rotation_offset: Quat::IDENTITY,
            pos_w: 0.0,
            rot_w: 0.0,
            local_positions: Vec::new(),
            use_plane_nodes: false,
            animated_plane_rotation: Quat::IDENTITY,
            animated_position: Vec3::ZERO,
            first_update: false,
            chain_index: -1,
            node_index: -1,
            plane1_chain_index: 0,
            plane1_node_index: -1,
            plane2_chain_index: -1,
            plane2_node_index: -1,
            plane3_chain_index: -1,
            plane3_node_index: -1,
            child_chain_indexes: Vec::new(),
            child_node_indexes: Vec::new(),
            is_end_effector: false,
        }
    }

    fn node<'a>(&self, chain: &'a mut [FbikChain]) -> &'a mut super::point::Node {
        &mut chain[self.chain_index as usize].nodes[self.node_index as usize]
    }

    /// `PinToBone`.
    pub fn pin_to_bone(&mut self, h: &Hierarchy, position_weight: f32, rotation_weight: f32) {
        self.position = h.position(self.bone);
        self.position_weight = unity::clamp(position_weight, 0.0, 1.0);
        self.rotation = h.rotation(self.bone);
        self.rotation_weight = unity::clamp(rotation_weight, 0.0, 1.0);
    }

    /// `Initiate`.
    pub fn initiate(&mut self, chain: &[FbikChain], h: &Hierarchy) {
        self.position = h.position(self.bone);
        self.rotation = h.rotation(self.bone);
        self.animated_plane_rotation = Quat::IDENTITY;
        (self.chain_index, self.node_index) = get_chain_and_node_indexes(chain, self.bone);
        self.child_chain_indexes.clear();
        self.child_node_indexes.clear();
        for &child in &self.child_bones {
            let (c, n) = get_chain_and_node_indexes(chain, child);
            self.child_chain_indexes.push(c);
            self.child_node_indexes.push(n);
        }
        self.local_positions = vec![Vec3::ZERO; self.child_bones.len()];
        self.use_plane_nodes = false;
        if let Some(plane_bone1) = self.plane_bone1 {
            (self.plane1_chain_index, self.plane1_node_index) = get_chain_and_node_indexes(chain, plane_bone1);
            if let Some(plane_bone2) = self.plane_bone2 {
                (self.plane2_chain_index, self.plane2_node_index) = get_chain_and_node_indexes(chain, plane_bone2);
                if let Some(plane_bone3) = self.plane_bone3 {
                    (self.plane3_chain_index, self.plane3_node_index) = get_chain_and_node_indexes(chain, plane_bone3);
                    self.use_plane_nodes = true;
                }
            }
            self.is_end_effector = true;
        } else {
            self.is_end_effector = false;
        }
    }

    /// `ResetOffset`.
    pub fn reset_offset(&self, chain: &mut [FbikChain]) {
        self.node(chain).offset = Vec3::ZERO;
        for (&c, &n) in self.child_chain_indexes.iter().zip(&self.child_node_indexes) {
            chain[c as usize].nodes[n as usize].offset = Vec3::ZERO;
        }
    }

    /// `SetToTarget`.
    pub fn set_to_target(&mut self) {
        if let Some(target) = self.target {
            self.position = target.position;
            self.rotation = target.rotation;
        }
    }

    /// `OnPreSolve`.
    pub fn on_pre_solve(&mut self, chain: &mut [FbikChain], h: &Hierarchy, ik_position_weight: f32, iterations: i32) {
        self.position_weight = unity::clamp(self.position_weight, 0.0, 1.0);
        self.rotation_weight = unity::clamp(self.rotation_weight, 0.0, 1.0);
        self.maintain_relative_position_weight = unity::clamp(self.maintain_relative_position_weight, 0.0, 1.0);
        self.pos_w = self.position_weight * ik_position_weight;
        self.rot_w = self.rotation_weight * ik_position_weight;
        let offset = self.position_offset * ik_position_weight;
        let (pos_w, rot_w, rotation) = (self.pos_w, self.rot_w, self.rotation);
        let node = self.node(chain);
        node.effector_position_weight = pos_w;
        node.effector_rotation_weight = rot_w;
        node.solver_rotation = rotation;
        node.offset += offset;
        if self.effect_child_nodes && iterations > 0 {
            for i in 0..self.child_bones.len() {
                self.local_positions[i] = h.position(self.child_bones[i]) - h.position(self.bone);
                chain[self.child_chain_indexes[i] as usize].nodes[self.child_node_indexes[i] as usize].offset += offset;
            }
        }
        if self.use_plane_nodes && self.maintain_relative_position_weight > 0.0 {
            if let (Some(p1), Some(p2), Some(p3)) = (self.plane_bone1, self.plane_bone2, self.plane_bone3) {
                self.animated_plane_rotation =
                    unity::look_rotation(h.position(p2) - h.position(p1), h.position(p3) - h.position(p1));
            }
        }
        self.first_update = true;
    }

    /// `OnPostWrite`.
    pub fn on_post_write(&mut self) {
        self.position_offset = Vec3::ZERO;
    }

    fn get_plane_rotation(&self, chain: &[FbikChain]) -> Quat {
        let at = |c: i32, n: i32| chain[c as usize].nodes[n as usize].solver_position;
        let solver_position = at(self.plane1_chain_index, self.plane1_node_index);
        let solver_position2 = at(self.plane2_chain_index, self.plane2_node_index);
        let solver_position3 = at(self.plane3_chain_index, self.plane3_node_index);
        let vector = solver_position2 - solver_position;
        let upwards = solver_position3 - solver_position;
        if unity::vec_eq(vector, Vec3::ZERO) {
            // "Make sure you are not placing 2 or more FBBIK effectors of
            // the same chain to exactly the same position."
            return Quat::IDENTITY;
        }
        unity::look_rotation(vector, upwards)
    }

    /// `Update`: moves the effector's node (and the child nodes) to where
    /// the effector wants them.
    pub fn update(&mut self, chain: &mut [FbikChain], h: &Hierarchy) {
        if self.first_update {
            self.animated_position = h.position(self.bone) + self.node(chain).offset;
            self.first_update = false;
        }
        let from = self.get_position(chain, h);
        let (position, pos_w) = (self.position, self.pos_w);
        self.node(chain).solver_position = unity::lerp(from, position, pos_w);
        if self.effect_child_nodes {
            let own = self.node(chain).solver_position;
            for i in 0..self.child_bones.len() {
                let child = &mut chain[self.child_chain_indexes[i] as usize].nodes[self.child_node_indexes[i] as usize];
                child.solver_position = unity::lerp(child.solver_position, own + self.local_positions[i], pos_w);
            }
        }
    }

    /// `GetPosition`: sets `planeRotationOffset` on the way.
    fn get_position(&mut self, chain: &mut [FbikChain], h: &Hierarchy) -> Vec3 {
        self.plane_rotation_offset = Quat::IDENTITY;
        if !self.is_end_effector {
            return self.node(chain).solver_position;
        }
        if self.maintain_relative_position_weight <= 0.0 {
            return self.animated_position;
        }
        let Some(plane_bone1) = self.plane_bone1 else {
            return self.animated_position;
        };
        let vector2 = h.position(self.bone) - h.position(plane_bone1);
        self.plane_rotation_offset = unity::mul(
            self.get_plane_rotation(chain),
            unity::inverse(self.animated_plane_rotation),
        );
        let plane1 = chain[self.plane1_chain_index as usize].nodes[self.plane1_node_index as usize].solver_position;
        let vector = plane1 + unity::rotate(self.plane_rotation_offset, vector2);
        self.plane_rotation_offset = unity::quat_lerp(
            Quat::IDENTITY,
            self.plane_rotation_offset,
            self.maintain_relative_position_weight,
        );
        unity::lerp(
            self.animated_position,
            vector + self.node(chain).offset,
            self.maintain_relative_position_weight,
        )
    }
}
