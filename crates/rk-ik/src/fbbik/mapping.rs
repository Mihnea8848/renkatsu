// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! `IKMapping` and its kinds: how the solved nodes are written back onto
//! the bones of the spine, the head and the limbs. Port of RootMotion
//! FinalIK (`IKMapping.cs`, `IKMappingSpine.cs`, `IKMappingBone.cs`,
//! `IKMappingLimb.cs`).

use super::chain::{get_chain_and_node_indexes, solve_fabrik_joint, FbikChain};
use super::tools;
use crate::hierarchy::Hierarchy;
use crate::unity;
use glam::{Quat, Vec3};

fn node_position(chain: &[FbikChain], chain_index: i32, node_index: i32) -> Vec3 {
    chain[chain_index as usize].nodes[node_index as usize].solver_position
}

/// `IKMapping.BoneMap`: one bone and what the mapping knows about it.
#[derive(Clone, Debug)]
pub struct BoneMap {
    pub transform: usize,
    pub chain_index: i32,
    pub node_index: i32,
    pub default_local_position: Vec3,
    pub default_local_rotation: Quat,
    pub local_swing_axis: Vec3,
    pub local_twist_axis: Vec3,
    pub plane_position: Vec3,
    pub ik_position: Vec3,
    pub default_local_target_rotation: Quat,
    pub maintain_rotation: Quat,
    pub length: f32,
    pub animated_rotation: Quat,
    pub plane_bone1: usize,
    pub plane_bone2: usize,
    pub plane_bone3: usize,
    pub plane1_chain_index: i32,
    pub plane1_node_index: i32,
    pub plane2_chain_index: i32,
    pub plane2_node_index: i32,
    pub plane3_chain_index: i32,
    pub plane3_node_index: i32,
}

impl Default for BoneMap {
    fn default() -> Self {
        // Rotations without an initialiser are all zero in C#.
        let zero = Quat::from_xyzw(0.0, 0.0, 0.0, 0.0);
        Self {
            transform: 0,
            chain_index: -1,
            node_index: -1,
            default_local_position: Vec3::ZERO,
            default_local_rotation: zero,
            local_swing_axis: Vec3::ZERO,
            local_twist_axis: Vec3::ZERO,
            plane_position: Vec3::ZERO,
            ik_position: Vec3::ZERO,
            default_local_target_rotation: zero,
            maintain_rotation: zero,
            length: 0.0,
            animated_rotation: zero,
            plane_bone1: 0,
            plane_bone2: 0,
            plane_bone3: 0,
            plane1_chain_index: -1,
            plane1_node_index: -1,
            plane2_chain_index: -1,
            plane2_node_index: -1,
            plane3_chain_index: -1,
            plane3_node_index: -1,
        }
    }
}

impl BoneMap {
    /// `swingDirection`.
    pub fn swing_direction(&self, h: &Hierarchy) -> Vec3 {
        unity::rotate(h.rotation(self.transform), self.local_swing_axis)
    }

    /// `isNodeBone`.
    pub fn is_node_bone(&self) -> bool {
        self.node_index != -1
    }

    /// `lastAnimatedTargetRotation`.
    fn last_animated_target_rotation(&self, h: &Hierarchy) -> Quat {
        let (p1, p2, p3) = (
            h.position(self.plane_bone1),
            h.position(self.plane_bone2),
            h.position(self.plane_bone3),
        );
        if unity::vec_eq(p1, p3) {
            return Quat::IDENTITY;
        }
        unity::look_rotation(p2 - p1, p3 - p1)
    }

    /// `Initiate`.
    pub fn initiate(&mut self, transform: usize, chain: &[FbikChain]) {
        self.transform = transform;
        (self.chain_index, self.node_index) = get_chain_and_node_indexes(chain, transform);
    }

    /// `StoreDefaultLocalState`.
    pub fn store_default_local_state(&mut self, h: &Hierarchy) {
        self.default_local_position = h.local_position(self.transform);
        self.default_local_rotation = h.local_rotation(self.transform);
    }

    /// `FixTransform`.
    pub fn fix_transform(&self, h: &mut Hierarchy, position: bool) {
        if position {
            h.set_local_position(self.transform, self.default_local_position);
        }
        h.set_local_rotation(self.transform, self.default_local_rotation);
    }

    /// `SetLength`.
    pub fn set_length(&mut self, h: &Hierarchy, next_bone: usize) {
        self.length = unity::distance(h.position(self.transform), h.position(next_bone));
    }

    /// `SetLocalSwingAxis(bone1, bone2)`; the one argument form passes this
    /// bone as `bone2`.
    pub fn set_local_swing_axis(&mut self, h: &Hierarchy, bone1: usize, bone2: usize) {
        self.local_swing_axis = unity::rotate(
            unity::inverse(h.rotation(self.transform)),
            h.position(bone1) - h.position(bone2),
        );
    }

    /// `SetLocalTwistAxis`.
    pub fn set_local_twist_axis(&mut self, h: &Hierarchy, twist_direction: Vec3, normal_direction: Vec3) {
        let (mut normal_direction, mut twist_direction) = (normal_direction, twist_direction);
        unity::ortho_normalize(&mut normal_direction, &mut twist_direction);
        self.local_twist_axis = unity::rotate(unity::inverse(h.rotation(self.transform)), twist_direction);
    }

    /// `SetPlane`.
    pub fn set_plane(
        &mut self,
        chain: &[FbikChain],
        h: &Hierarchy,
        plane_bone1: usize,
        plane_bone2: usize,
        plane_bone3: usize,
    ) {
        self.plane_bone1 = plane_bone1;
        self.plane_bone2 = plane_bone2;
        self.plane_bone3 = plane_bone3;
        (self.plane1_chain_index, self.plane1_node_index) = get_chain_and_node_indexes(chain, plane_bone1);
        (self.plane2_chain_index, self.plane2_node_index) = get_chain_and_node_indexes(chain, plane_bone2);
        (self.plane3_chain_index, self.plane3_node_index) = get_chain_and_node_indexes(chain, plane_bone3);
        self.update_plane(h, true, true);
    }

    /// `UpdatePlane`.
    pub fn update_plane(&mut self, h: &Hierarchy, rotation: bool, position: bool) {
        let rotation2 = self.last_animated_target_rotation(h);
        if rotation {
            self.default_local_target_rotation = tools::rotation_to_local_space(h.rotation(self.transform), rotation2);
        }
        if position {
            self.plane_position = unity::rotate(
                unity::inverse(rotation2),
                h.position(self.transform) - h.position(self.plane_bone1),
            );
        }
    }

    /// `SetIKPosition`.
    pub fn set_ik_position(&mut self, h: &Hierarchy) {
        self.ik_position = h.position(self.transform);
    }

    /// `MaintainRotation`.
    pub fn maintain_rotation(&mut self, h: &Hierarchy) {
        self.maintain_rotation = h.rotation(self.transform);
    }

    /// `SetToIKPosition`.
    pub fn set_to_ik_position(&self, h: &mut Hierarchy) {
        h.set_position(self.transform, self.ik_position);
    }

    /// `FixToNode`, `fixNode` being this bone's own node.
    pub fn fix_to_node(&self, chain: &[FbikChain], h: &mut Hierarchy, weight: f32) {
        let fix = node_position(chain, self.chain_index, self.node_index);
        if weight >= 1.0 {
            h.set_position(self.transform, fix);
        } else {
            h.set_position(self.transform, unity::lerp(h.position(self.transform), fix, weight));
        }
    }

    /// `GetPlanePosition`.
    pub fn get_plane_position(&self, chain: &[FbikChain]) -> Vec3 {
        node_position(chain, self.plane1_chain_index, self.plane1_node_index)
            + unity::rotate(self.get_target_rotation(chain), self.plane_position)
    }

    /// `PositionToPlane`.
    pub fn position_to_plane(&self, chain: &[FbikChain], h: &mut Hierarchy) {
        h.set_position(self.transform, self.get_plane_position(chain));
    }

    fn blend_rotation(&self, h: &mut Hierarchy, to: Quat, weight: f32) {
        if weight >= 1.0 {
            h.set_rotation(self.transform, to);
        } else {
            h.set_rotation(self.transform, unity::quat_lerp(h.rotation(self.transform), to, weight));
        }
    }

    /// `RotateToPlane`.
    pub fn rotate_to_plane(&self, chain: &[FbikChain], h: &mut Hierarchy, weight: f32) {
        let quaternion = unity::mul(self.get_target_rotation(chain), self.default_local_target_rotation);
        self.blend_rotation(h, quaternion, weight);
    }

    /// `Swing(swingTarget, weight)`: from this bone's place.
    pub fn swing(&self, h: &mut Hierarchy, swing_target: Vec3, weight: f32) {
        let own = h.position(self.transform);
        self.swing_between(h, swing_target, own, weight);
    }

    /// `Swing(pos1, pos2, weight)`.
    pub fn swing_between(&self, h: &mut Hierarchy, pos1: Vec3, pos2: Vec3, weight: f32) {
        let rotation = h.rotation(self.transform);
        let quaternion = unity::mul(
            unity::from_to_rotation(unity::rotate(rotation, self.local_swing_axis), pos1 - pos2),
            rotation,
        );
        self.blend_rotation(h, quaternion, weight);
    }

    /// `Twist`.
    pub fn twist(&self, h: &mut Hierarchy, twist_direction: Vec3, normal_direction: Vec3, weight: f32) {
        let (mut normal_direction, mut twist_direction) = (normal_direction, twist_direction);
        unity::ortho_normalize(&mut normal_direction, &mut twist_direction);
        let rotation = h.rotation(self.transform);
        let quaternion = unity::mul(
            unity::from_to_rotation(unity::rotate(rotation, self.local_twist_axis), twist_direction),
            rotation,
        );
        self.blend_rotation(h, quaternion, weight);
    }

    /// `RotateToMaintain`.
    pub fn rotate_to_maintain(&self, h: &mut Hierarchy, weight: f32) {
        if weight <= 0.0 {
            return;
        }
        h.set_rotation(
            self.transform,
            unity::quat_lerp(h.rotation(self.transform), self.maintain_rotation, weight),
        );
    }

    /// `RotateToEffector`.
    pub fn rotate_to_effector(&self, chain: &[FbikChain], h: &mut Hierarchy, weight: f32) {
        if !self.is_node_bone() {
            return;
        }
        let node = &chain[self.chain_index as usize].nodes[self.node_index as usize];
        let num = weight * node.effector_rotation_weight;
        if num <= 0.0 {
            return;
        }
        self.blend_rotation(h, node.solver_rotation, num);
    }

    fn get_target_rotation(&self, chain: &[FbikChain]) -> Quat {
        let solver_position = node_position(chain, self.plane1_chain_index, self.plane1_node_index);
        let solver_position2 = node_position(chain, self.plane2_chain_index, self.plane2_node_index);
        let solver_position3 = node_position(chain, self.plane3_chain_index, self.plane3_node_index);
        if unity::vec_eq(solver_position, solver_position3) {
            return Quat::IDENTITY;
        }
        unity::look_rotation(solver_position2 - solver_position, solver_position3 - solver_position)
    }
}

/// `IKMappingSpine`.
#[derive(Clone, Debug)]
pub struct IkMappingSpine {
    pub spine_bones: Vec<usize>,
    pub left_upper_arm_bone: usize,
    pub right_upper_arm_bone: usize,
    pub left_thigh_bone: usize,
    pub right_thigh_bone: usize,
    pub iterations: i32,
    pub twist_weight: f32,
    // Private in FinalIK.
    pub root_node_index: i32,
    pub spine: Vec<BoneMap>,
    pub left_upper_arm: BoneMap,
    pub right_upper_arm: BoneMap,
    pub left_thigh: BoneMap,
    pub right_thigh: BoneMap,
    pub use_fabrik: bool,
}

impl IkMappingSpine {
    /// `new IKMappingSpine(...)`.
    pub fn new(
        spine_bones: Vec<usize>,
        left_upper_arm_bone: usize,
        right_upper_arm_bone: usize,
        left_thigh_bone: usize,
        right_thigh_bone: usize,
    ) -> Self {
        Self {
            spine_bones,
            left_upper_arm_bone,
            right_upper_arm_bone,
            left_thigh_bone,
            right_thigh_bone,
            iterations: 3,
            twist_weight: 1.0,
            root_node_index: 0,
            spine: Vec::new(),
            left_upper_arm: BoneMap::default(),
            right_upper_arm: BoneMap::default(),
            left_thigh: BoneMap::default(),
            right_thigh: BoneMap::default(),
            use_fabrik: false,
        }
    }

    /// `StoreDefaultLocalState`.
    pub fn store_default_local_state(&mut self, h: &Hierarchy) {
        for bone in &mut self.spine {
            bone.store_default_local_state(h);
        }
    }

    /// `FixTransforms`.
    pub fn fix_transforms(&self, h: &mut Hierarchy) {
        for (i, bone) in self.spine.iter().enumerate() {
            bone.fix_transform(h, i == 0 || i == self.spine.len() - 1);
        }
    }

    /// `Initiate`.
    pub fn initiate(&mut self, chain: &[FbikChain], h: &Hierarchy) {
        if self.iterations <= 0 {
            self.iterations = 3;
        }
        if self.spine.len() != self.spine_bones.len() {
            self.spine = vec![BoneMap::default(); self.spine_bones.len()];
        }
        self.root_node_index = -1;
        for i in 0..self.spine_bones.len() {
            self.spine[i].initiate(self.spine_bones[i], chain);
            if self.spine[i].is_node_bone() {
                self.root_node_index = i as i32;
            }
        }
        self.left_upper_arm.initiate(self.left_upper_arm_bone, chain);
        self.right_upper_arm.initiate(self.right_upper_arm_bone, chain);
        self.left_thigh.initiate(self.left_thigh_bone, chain);
        self.right_thigh.initiate(self.right_thigh_bone, chain);
        for bone in &mut self.spine {
            bone.set_ik_position(h);
        }
        let root = self.spine[self.root_node_index as usize].transform;
        self.spine[0].set_plane(chain, h, root, self.left_thigh.transform, self.right_thigh.transform);
        self.set_axes(h);
        let last = self.spine.len() - 1;
        self.spine[last].set_plane(
            chain,
            h,
            root,
            self.left_upper_arm.transform,
            self.right_upper_arm.transform,
        );
        self.spine[last].set_local_swing_axis(h, self.left_upper_arm.transform, self.right_upper_arm.transform);
        self.use_fabrik = self.use_fabrik();
    }

    /// The loop `Initiate` and `ReadPose` share: length, swing axis and
    /// twist axis of every spine bone but the last.
    fn set_axes(&mut self, h: &Hierarchy) {
        let across = h.position(self.left_upper_arm.transform) - h.position(self.right_upper_arm.transform);
        for k in 0..self.spine.len() - 1 {
            let (own, next) = (self.spine[k].transform, self.spine[k + 1].transform);
            self.spine[k].set_length(h, next);
            self.spine[k].set_local_swing_axis(h, next, own);
            self.spine[k].set_local_twist_axis(h, across, h.position(next) - h.position(own));
        }
    }

    fn use_fabrik(&self) -> bool {
        self.spine.len() > 3 || self.root_node_index != 1
    }

    /// `ReadPose`.
    pub fn read_pose(&mut self, h: &Hierarchy) {
        self.spine[0].update_plane(h, true, true);
        self.set_axes(h);
        let last = self.spine.len() - 1;
        self.spine[last].update_plane(h, true, true);
        self.spine[last].set_local_swing_axis(h, self.left_upper_arm.transform, self.right_upper_arm.transform);
    }

    /// `WritePose`.
    pub fn write_pose(&mut self, chain: &[FbikChain], h: &mut Hierarchy) {
        let last = self.spine.len() - 1;
        let root = self.root_node_index as usize;
        let plane_position = self.spine[0].get_plane_position(chain);
        let solver_position = node_position(chain, self.spine[root].chain_index, self.spine[root].node_index);
        let plane_position2 = self.spine[last].get_plane_position(chain);
        if self.use_fabrik {
            let vector = solver_position - h.position(self.spine[root].transform);
            for bone in &mut self.spine {
                bone.ik_position = h.position(bone.transform) + vector;
            }
            for _ in 0..self.iterations {
                self.forward_reach(plane_position2);
                self.backward_reach(plane_position);
                self.spine[root].ik_position = solver_position;
            }
        } else {
            self.spine[0].ik_position = plane_position;
            self.spine[root].ik_position = solver_position;
        }
        self.spine[last].ik_position = plane_position2;
        self.map_to_solver_positions(chain, h);
    }

    /// `ForwardReach`.
    pub fn forward_reach(&mut self, position: Vec3) {
        let last = self.spine_bones.len() - 1;
        self.spine[last].ik_position = position;
        for n in (0..self.spine.len() - 1).rev() {
            self.spine[n].ik_position = solve_fabrik_joint(
                self.spine[n].ik_position,
                self.spine[n + 1].ik_position,
                self.spine[n].length,
            );
        }
    }

    fn backward_reach(&mut self, position: Vec3) {
        self.spine[0].ik_position = position;
        for n in 1..self.spine.len() {
            self.spine[n].ik_position = solve_fabrik_joint(
                self.spine[n].ik_position,
                self.spine[n - 1].ik_position,
                self.spine[n - 1].length,
            );
        }
    }

    fn map_to_solver_positions(&self, chain: &[FbikChain], h: &mut Hierarchy) {
        let len = self.spine.len();
        self.spine[0].set_to_ik_position(h);
        self.spine[0].rotate_to_plane(chain, h, 1.0);
        for i in 1..len - 1 {
            self.spine[i].swing(h, self.spine[i + 1].ik_position, 1.0);
            if self.twist_weight > 0.0 {
                let num = i as f32 / (len as f32 - 2.0);
                let solver_position =
                    node_position(chain, self.left_upper_arm.chain_index, self.left_upper_arm.node_index);
                let solver_position2 =
                    node_position(chain, self.right_upper_arm.chain_index, self.right_upper_arm.node_index);
                let normal = self.spine[i + 1].ik_position - h.position(self.spine[i].transform);
                self.spine[i].twist(h, solver_position - solver_position2, normal, num * self.twist_weight);
            }
        }
        self.spine[len - 1].set_to_ik_position(h);
        self.spine[len - 1].rotate_to_plane(chain, h, 1.0);
    }
}

/// `IKMappingBone`: a bone that keeps its animated rotation (the head).
#[derive(Clone, Debug)]
pub struct IkMappingBone {
    pub bone: usize,
    pub maintain_rotation_weight: f32,
    // Private in FinalIK.
    pub bone_map: BoneMap,
}

impl IkMappingBone {
    /// `new IKMappingBone(bone)`.
    pub fn new(bone: usize) -> Self {
        Self {
            bone,
            maintain_rotation_weight: 1.0,
            bone_map: BoneMap::default(),
        }
    }

    /// `StoreDefaultLocalState`.
    pub fn store_default_local_state(&mut self, h: &Hierarchy) {
        self.bone_map.store_default_local_state(h);
    }

    /// `FixTransforms`.
    pub fn fix_transforms(&self, h: &mut Hierarchy) {
        self.bone_map.fix_transform(h, false);
    }

    /// `Initiate`.
    pub fn initiate(&mut self, chain: &[FbikChain]) {
        self.bone_map.initiate(self.bone, chain);
    }

    /// `ReadPose`.
    pub fn read_pose(&mut self, h: &Hierarchy) {
        self.bone_map.maintain_rotation(h);
    }

    /// `WritePose`.
    pub fn write_pose(&self, h: &mut Hierarchy, solver_weight: f32) {
        self.bone_map
            .rotate_to_maintain(h, solver_weight * self.maintain_rotation_weight);
    }
}

/// `IKMappingLimb`: the three bones of a limb and, for arms, the shoulder
/// above them.
#[derive(Clone, Debug)]
pub struct IkMappingLimb {
    pub parent_bone: Option<usize>,
    pub bone1: usize,
    pub bone2: usize,
    pub bone3: usize,
    pub maintain_rotation_weight: f32,
    pub weight: f32,
    // Private in FinalIK.
    pub bone_map_parent: BoneMap,
    pub bone_map1: BoneMap,
    pub bone_map2: BoneMap,
    pub bone_map3: BoneMap,
}

impl IkMappingLimb {
    /// `new IKMappingLimb(bone1, bone2, bone3, parentBone)`.
    pub fn new(bone1: usize, bone2: usize, bone3: usize, parent_bone: Option<usize>) -> Self {
        Self {
            parent_bone,
            bone1,
            bone2,
            bone3,
            maintain_rotation_weight: 0.0,
            weight: 1.0,
            bone_map_parent: BoneMap::default(),
            bone_map1: BoneMap::default(),
            bone_map2: BoneMap::default(),
            bone_map3: BoneMap::default(),
        }
    }

    /// `SetLimbOrientation`.
    pub fn set_limb_orientation(&mut self, h: &Hierarchy, upper: Vec3, lower: Vec3) {
        let (r1, r2) = (h.rotation(self.bone1), h.rotation(self.bone2));
        let (p1, p2, p3) = (h.position(self.bone1), h.position(self.bone2), h.position(self.bone3));
        self.bone_map1.default_local_target_rotation = unity::inverse(unity::mul(
            unity::inverse(r1),
            unity::look_rotation(p2 - p1, unity::rotate(r1, -upper)),
        ));
        self.bone_map2.default_local_target_rotation = unity::inverse(unity::mul(
            unity::inverse(r2),
            unity::look_rotation(p3 - p2, unity::rotate(r2, -lower)),
        ));
    }

    /// `StoreDefaultLocalState`.
    pub fn store_default_local_state(&mut self, h: &Hierarchy) {
        if self.parent_bone.is_some() {
            self.bone_map_parent.store_default_local_state(h);
        }
        self.bone_map1.store_default_local_state(h);
        self.bone_map2.store_default_local_state(h);
        self.bone_map3.store_default_local_state(h);
    }

    /// `FixTransforms`.
    pub fn fix_transforms(&self, h: &mut Hierarchy) {
        if self.parent_bone.is_some() {
            self.bone_map_parent.fix_transform(h, false);
        }
        self.bone_map1.fix_transform(h, true);
        self.bone_map2.fix_transform(h, false);
        self.bone_map3.fix_transform(h, false);
    }

    /// `Initiate`.
    pub fn initiate(&mut self, chain: &[FbikChain], h: &Hierarchy) {
        if let Some(parent_bone) = self.parent_bone {
            self.bone_map_parent.initiate(parent_bone, chain);
        }
        self.bone_map1.initiate(self.bone1, chain);
        self.bone_map2.initiate(self.bone2, chain);
        self.bone_map3.initiate(self.bone3, chain);
        self.bone_map1.set_plane(chain, h, self.bone1, self.bone2, self.bone3);
        self.bone_map2.set_plane(chain, h, self.bone2, self.bone3, self.bone1);
        if let Some(parent_bone) = self.parent_bone {
            self.bone_map_parent.set_local_swing_axis(h, self.bone1, parent_bone);
        }
    }

    /// `ReadPose`.
    pub fn read_pose(&mut self, h: &Hierarchy) {
        self.bone_map1.update_plane(h, true, true);
        self.bone_map2.update_plane(h, true, false);
        self.weight = unity::clamp(self.weight, 0.0, 1.0);
        self.bone_map3.maintain_rotation(h);
    }

    /// `WritePose`.
    pub fn write_pose(&self, chain: &[FbikChain], h: &mut Hierarchy, ik_position_weight: f32, full_body: bool) {
        if self.weight <= 0.0 {
            return;
        }
        if full_body && self.parent_bone.is_some() {
            let target = node_position(chain, self.bone_map1.chain_index, self.bone_map1.node_index);
            self.bone_map_parent.swing(h, target, self.weight);
        }
        self.bone_map1.rotate_to_plane(chain, h, self.weight);
        self.bone_map2.rotate_to_plane(chain, h, self.weight);
        self.bone_map3
            .rotate_to_maintain(h, self.maintain_rotation_weight * self.weight * ik_position_weight);
        self.bone_map3.rotate_to_effector(chain, h, self.weight);
    }
}
