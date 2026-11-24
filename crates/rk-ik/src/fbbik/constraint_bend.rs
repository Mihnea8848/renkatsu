// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! `IKConstraintBend`: which way a limb's middle joint bends. Port of
//! RootMotion FinalIK (`IKConstraintBend.cs`).

use super::chain::{get_chain_and_node_indexes, FbikChain};
use super::tools;
use crate::hierarchy::Hierarchy;
use crate::unity;
use glam::{Quat, Vec3};

#[derive(Clone, Debug)]
pub struct IkConstraintBend {
    pub bone1: usize,
    pub bone2: usize,
    pub bone3: usize,
    /// World position of the `bendGoal` transform, `None` when there is
    /// none. The goal lives outside the body, so its place is given.
    pub bend_goal: Option<Vec3>,
    pub direction: Vec3,
    pub rotation_offset: Quat,
    pub weight: f32,
    pub default_local_direction: Vec3,
    pub default_child_direction: Vec3,
    pub clamp_f: f32,
    // Private in FinalIK: worked out at initiation.
    pub chain_index1: i32,
    pub node_index1: i32,
    pub chain_index2: i32,
    pub node_index2: i32,
    pub chain_index3: i32,
    pub node_index3: i32,
    pub initiated: bool,
}

impl Default for IkConstraintBend {
    fn default() -> Self {
        Self {
            bone1: 0,
            bone2: 0,
            bone3: 0,
            bend_goal: None,
            direction: Vec3::X,
            // A field without an initialiser: all zero in C#.
            rotation_offset: Quat::from_xyzw(0.0, 0.0, 0.0, 0.0),
            weight: 0.0,
            default_local_direction: Vec3::ZERO,
            default_child_direction: Vec3::ZERO,
            clamp_f: 0.505,
            chain_index1: 0,
            node_index1: 0,
            chain_index2: 0,
            node_index2: 0,
            chain_index3: 0,
            node_index3: 0,
            initiated: false,
        }
    }
}

fn node_position(chain: &[FbikChain], chain_index: i32, node_index: i32) -> Vec3 {
    chain[chain_index as usize].nodes[node_index as usize].solver_position
}

impl IkConstraintBend {
    /// `SetBones`.
    pub fn set_bones(&mut self, bone1: usize, bone2: usize, bone3: usize) {
        self.bone1 = bone1;
        self.bone2 = bone2;
        self.bone3 = bone3;
    }

    /// `Initiate`, for the bend constraint of `chain[i]`.
    pub fn initiate(chain: &mut [FbikChain], i: usize, h: &Hierarchy) {
        let (bone1, bone2, bone3) = {
            let b = &chain[i].bend_constraint;
            (b.bone1, b.bone2, b.bone3)
        };
        let (c1, n1) = get_chain_and_node_indexes(chain, bone1);
        let (c2, n2) = get_chain_and_node_indexes(chain, bone2);
        let (c3, n3) = get_chain_and_node_indexes(chain, bone3);
        // OrthoToLimb, then OrthoToBone1.
        let mut tangent = h.position(bone2) - h.position(bone1);
        let mut normal = node_position(chain, c3, n3) - node_position(chain, c1, n1);
        unity::ortho_normalize(&mut normal, &mut tangent);
        let mut normal = node_position(chain, c2, n2) - node_position(chain, c1, n1);
        unity::ortho_normalize(&mut normal, &mut tangent);
        let direction = tangent;
        let default_local_direction = unity::rotate(unity::inverse(h.rotation(bone1)), direction);
        let vector = unity::cross(unity::normalize(h.position(bone3) - h.position(bone1)), direction);
        let default_child_direction = unity::rotate(unity::inverse(h.rotation(bone3)), vector);
        let b = &mut chain[i].bend_constraint;
        (b.chain_index1, b.node_index1) = (c1, n1);
        (b.chain_index2, b.node_index2) = (c2, n2);
        (b.chain_index3, b.node_index3) = (c3, n3);
        b.direction = direction;
        b.default_local_direction = default_local_direction;
        b.default_child_direction = default_child_direction;
        b.initiated = true;
    }

    /// `SetLimbOrientation`.
    pub fn set_limb_orientation(&mut self, upper: Vec3, _lower: Vec3, last: Vec3) {
        self.default_local_direction = unity::normalize(upper);
        self.default_child_direction = unity::normalize(last);
    }

    /// `LimitBend`: keeps the animated limb from bending the wrong way
    /// before the solver reads it.
    pub fn limit_bend(&self, h: &mut Hierarchy, solver_weight: f32, position_weight: f32) {
        if !self.initiated {
            return;
        }
        let vector = unity::rotate(h.rotation(self.bone1), -self.default_local_direction);
        let from_direction = h.position(self.bone3) - h.position(self.bone2);
        let (to_direction, changed) = tools::clamp_direction(from_direction, vector, self.clamp_f * solver_weight, 0);
        let rotation = h.rotation(self.bone3);
        if changed {
            let quaternion = unity::from_to_rotation(from_direction, to_direction);
            h.set_rotation(self.bone2, unity::mul(quaternion, h.rotation(self.bone2)));
        }
        if position_weight > 0.0 {
            let mut normal = h.position(self.bone2) - h.position(self.bone1);
            let mut tangent = h.position(self.bone3) - h.position(self.bone2);
            unity::ortho_normalize(&mut normal, &mut tangent);
            let quaternion2 = unity::from_to_rotation(tangent, vector);
            let now = h.rotation(self.bone2);
            h.set_rotation(
                self.bone2,
                unity::quat_lerp(now, unity::mul(quaternion2, now), position_weight * solver_weight),
            );
        }
        if changed || position_weight > 0.0 {
            h.set_rotation(self.bone3, rotation);
        }
    }

    /// `GetDir`, for the bend constraint of `chain[i]`: the direction the
    /// middle joint goes, from the bend goal, the animation and the end
    /// effector's rotation.
    pub fn get_dir(chain: &mut [FbikChain], i: usize, h: &Hierarchy, ik_position_weight: f32) -> Vec3 {
        let b = chain[i].bend_constraint.clone();
        if !b.initiated {
            return Vec3::ZERO;
        }
        let num = b.weight * ik_position_weight;
        let node1 = node_position(chain, b.chain_index1, b.node_index1);
        let mut direction = b.direction;
        if let Some(goal) = b.bend_goal {
            let vector = goal - node1;
            if !unity::vec_eq(vector, Vec3::ZERO) {
                direction = vector;
                chain[i].bend_constraint.direction = direction;
            }
        }
        if num >= 1.0 {
            return unity::normalize(direction);
        }
        let node3 = chain[b.chain_index3 as usize].nodes[b.node_index3 as usize].clone();
        let vector2 = node3.solver_position - node1;
        let quaternion = unity::from_to_rotation(h.position(b.bone3) - h.position(b.bone1), vector2);
        let mut vector3 = unity::rotate(quaternion, h.position(b.bone2) - h.position(b.bone1));
        if node3.effector_rotation_weight > 0.0 {
            let towards = -unity::cross(vector2, unity::rotate(node3.solver_rotation, b.default_child_direction));
            vector3 = unity::lerp(vector3, towards, node3.effector_rotation_weight);
        }
        if !unity::quat_eq(b.rotation_offset, Quat::IDENTITY) {
            let quaternion2 = unity::from_to_rotation(unity::rotate(b.rotation_offset, vector2), vector2);
            vector3 = unity::rotate(unity::mul(quaternion2, b.rotation_offset), vector3);
        }
        if num <= 0.0 {
            return vector3;
        }
        unity::lerp(vector3, unity::normalize(direction), num)
    }
}
