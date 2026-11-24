// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! `IKSolverFullBodyBiped` with the parts of `IKSolverFullBody` and
//! `IKSolver` it is built on: initiation, and one update (read the pose,
//! solve, write the pose). Port of RootMotion FinalIK (`IKSolver.cs`,
//! `IKSolverFullBody.cs`, `IKSolverFullBodyBiped.cs`).
//!
//! The delegates (`OnPreRead`, `OnPostSolve` and the others) are left out;
//! `read_pose`, `solve` and `write_pose` can be called one by one instead.

use super::chain::{ChildConstraint, FbikChain};
use super::effector::IkEffector;
use super::mapping::{IkMappingBone, IkMappingLimb, IkMappingSpine};
use super::tools;
use crate::hierarchy::Hierarchy;
use crate::unity;
use glam::Vec3;

/// `FullBodyBipedEffector`: the effectors in the solver's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullBodyBipedEffector {
    Body,
    LeftShoulder,
    RightShoulder,
    LeftThigh,
    RightThigh,
    LeftHand,
    RightHand,
    LeftFoot,
    RightFoot,
}

/// `FullBodyBipedChain`: the limbs; chain `1 + this`, limb mapping `this`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullBodyBipedChain {
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
}

/// `BipedReferences`: the bones of a biped, by index in the hierarchy.
#[derive(Clone, Debug)]
pub struct BipedReferences {
    pub pelvis: usize,
    pub left_thigh: usize,
    pub left_calf: usize,
    pub left_foot: usize,
    pub right_thigh: usize,
    pub right_calf: usize,
    pub right_foot: usize,
    pub left_upper_arm: usize,
    pub left_forearm: usize,
    pub left_hand: usize,
    pub right_upper_arm: usize,
    pub right_forearm: usize,
    pub right_hand: usize,
    pub head: Option<usize>,
    pub spine: Vec<usize>,
}

/// Why the solver would not initiate (`IsValid`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invalid(pub String);

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Invalid {}

#[derive(Clone, Debug)]
pub struct IkSolverFullBodyBiped {
    // IKSolver.
    pub ik_position: Vec3,
    pub ik_position_weight: f32,
    pub first_initiation: bool,
    pub initiated: bool,
    /// `root.up`: the up axis of the transform the component sits on, in
    /// the world. Only body pulling reads it.
    pub root_up: Vec3,
    // IKSolverFullBody.
    pub iterations: i32,
    pub chain: Vec<FbikChain>,
    pub effectors: Vec<IkEffector>,
    pub spine_mapping: IkMappingSpine,
    pub bone_mappings: Vec<IkMappingBone>,
    pub limb_mappings: Vec<IkMappingLimb>,
    // IKSolverFullBodyBiped.
    pub root_node: usize,
    pub spine_stiffness: f32,
    pub pull_body_vertical: f32,
    pub pull_body_horizontal: f32,
    /// Private in FinalIK.
    pub offset: Vec3,
}

impl IkSolverFullBodyBiped {
    /// A solver set to a biped's bones (`SetToReferences`) with FinalIK's
    /// default values. `root_node` of `None` picks it (`DetectRootNodeBone`).
    pub fn from_references(references: &BipedReferences, root_node: Option<usize>, h: &Hierarchy) -> Option<Self> {
        let root_node = match root_node {
            Some(node) => node,
            None => Self::detect_root_node_bone(references, h)?,
        };
        let r = references;
        let limbs = [
            [r.left_upper_arm, r.left_forearm, r.left_hand],
            [r.right_upper_arm, r.right_forearm, r.right_hand],
            [r.left_thigh, r.left_calf, r.left_foot],
            [r.right_thigh, r.right_calf, r.right_foot],
        ];
        let mut chain = vec![FbikChain::default(); 5];
        chain[0].pin = 0.0;
        chain[0].set_nodes(&[root_node]);
        chain[0].children = vec![1, 2, 3, 4];
        for (i, limb) in limbs.iter().enumerate() {
            chain[i + 1].set_nodes(limb);
        }
        chain[0].child_constraints = vec![
            ChildConstraint::new(r.left_upper_arm, r.right_thigh, 0.0, 1.0),
            ChildConstraint::new(r.right_upper_arm, r.left_thigh, 0.0, 1.0),
            ChildConstraint::new(r.left_upper_arm, r.right_upper_arm, 0.0, 0.0),
            ChildConstraint::new(r.left_thigh, r.right_thigh, 0.0, 0.0),
        ];
        let mut effectors: Vec<IkEffector> = [
            root_node,
            r.left_upper_arm,
            r.right_upper_arm,
            r.left_thigh,
            r.right_thigh,
            r.left_hand,
            r.right_hand,
            r.left_foot,
            r.right_foot,
        ]
        .into_iter()
        .map(IkEffector::new)
        .collect();
        effectors[0].child_bones = vec![r.left_thigh, r.right_thigh];
        let planes = [
            (r.left_upper_arm, r.right_upper_arm),
            (r.right_upper_arm, r.left_upper_arm),
            (r.left_thigh, r.right_thigh),
            (r.right_thigh, r.left_thigh),
        ];
        for (i, (bone1, bone2)) in planes.into_iter().enumerate() {
            effectors[5 + i].plane_bone1 = Some(bone1);
            effectors[5 + i].plane_bone2 = Some(bone2);
            effectors[5 + i].plane_bone3 = Some(root_node);
        }
        let mut spine_bones = vec![r.pelvis];
        spine_bones.extend(&r.spine);
        let spine_mapping = IkMappingSpine::new(
            spine_bones,
            r.left_upper_arm,
            r.right_upper_arm,
            r.left_thigh,
            r.right_thigh,
        );
        let bone_mappings = r
            .head
            .map(|head| {
                let mut mapping = IkMappingBone::new(head);
                mapping.maintain_rotation_weight = 0.0;
                mapping
            })
            .into_iter()
            .collect();
        // The clavicle: the upper arm's parent when it is not a spine bone.
        let clavicle = |upper_arm: usize| h.parent(upper_arm).filter(|p| !r.spine.contains(p));
        let mut limb_mappings = vec![
            IkMappingLimb::new(limbs[0][0], limbs[0][1], limbs[0][2], clavicle(r.left_upper_arm)),
            IkMappingLimb::new(limbs[1][0], limbs[1][1], limbs[1][2], clavicle(r.right_upper_arm)),
            IkMappingLimb::new(limbs[2][0], limbs[2][1], limbs[2][2], None),
            IkMappingLimb::new(limbs[3][0], limbs[3][1], limbs[3][2], None),
        ];
        limb_mappings[2].maintain_rotation_weight = 1.0;
        limb_mappings[3].maintain_rotation_weight = 1.0;
        Some(Self {
            ik_position: Vec3::ZERO,
            ik_position_weight: 1.0,
            first_initiation: true,
            initiated: false,
            root_up: Vec3::Y,
            iterations: 4,
            chain,
            effectors,
            spine_mapping,
            bone_mappings,
            limb_mappings,
            root_node,
            spine_stiffness: 0.5,
            pull_body_vertical: 0.5,
            pull_body_horizontal: 0.0,
            offset: Vec3::ZERO,
        })
    }

    /// `DetectRootNodeBone`: the highest spine bone still in the lower
    /// half between the thighs and the upper arms.
    pub fn detect_root_node_bone(references: &BipedReferences, h: &Hierarchy) -> Option<usize> {
        let r = references;
        let num = r.spine.len();
        if num < 1 {
            return None;
        }
        if num == 1 {
            return Some(r.spine[0]);
        }
        let vector = unity::lerp(h.position(r.left_thigh), h.position(r.right_thigh), 0.5);
        let vector2 = unity::lerp(h.position(r.left_upper_arm), h.position(r.right_upper_arm), 0.5);
        let on_normal = vector2 - vector;
        let magnitude = unity::magnitude(on_normal);
        let mut num2 = 0;
        for i in 1..num {
            let vector3 = h.position(r.spine[i]) - vector;
            let vector4 = unity::project(vector3, on_normal);
            let num3 = unity::dot(unity::normalize(vector4), unity::normalize(on_normal));
            if num3 > 0.0 {
                let num4 = unity::magnitude(vector4) / magnitude;
                if num4 < 0.5 {
                    num2 = i;
                }
            }
        }
        Some(r.spine[num2])
    }

    /// `GetEffector(FullBodyBipedEffector)`.
    pub fn get_effector(&mut self, effector: FullBodyBipedEffector) -> &mut IkEffector {
        &mut self.effectors[effector as usize]
    }

    /// `GetChain(FullBodyBipedChain)`.
    pub fn get_chain(&mut self, c: FullBodyBipedChain) -> &mut FbikChain {
        &mut self.chain[1 + c as usize]
    }

    /// `GetEndEffector`.
    pub fn get_end_effector(&mut self, c: FullBodyBipedChain) -> &mut IkEffector {
        &mut self.effectors[5 + c as usize]
    }

    /// `GetLimbMapping(FullBodyBipedChain)`.
    pub fn get_limb_mapping(&mut self, c: FullBodyBipedChain) -> &mut IkMappingLimb {
        &mut self.limb_mappings[c as usize]
    }

    /// `SetChainWeights`.
    pub fn set_chain_weights(&mut self, c: FullBodyBipedChain, pull: f32, reach: f32) {
        let chain = self.get_chain(c);
        chain.pull = pull;
        chain.reach = reach;
    }

    /// `SetEffectorWeights`.
    pub fn set_effector_weights(
        &mut self,
        effector: FullBodyBipedEffector,
        position_weight: f32,
        rotation_weight: f32,
    ) {
        let e = self.get_effector(effector);
        e.position_weight = unity::clamp(position_weight, 0.0, 1.0);
        e.rotation_weight = unity::clamp(rotation_weight, 0.0, 1.0);
    }

    /// `SetLimbOrientation`, for one limb: the bend axes given instead of
    /// read from the pose.
    pub fn set_limb_orientation(
        &mut self,
        h: &Hierarchy,
        c: FullBodyBipedChain,
        upper_bone_forward_axis: Vec3,
        lower_bone_forward_axis: Vec3,
        last_bone_left_axis: Vec3,
    ) {
        let (upper, lower, last) = match c {
            FullBodyBipedChain::LeftArm | FullBodyBipedChain::RightArm => {
                (-upper_bone_forward_axis, -lower_bone_forward_axis, -last_bone_left_axis)
            }
            _ => (upper_bone_forward_axis, lower_bone_forward_axis, last_bone_left_axis),
        };
        self.get_chain(c)
            .bend_constraint
            .set_limb_orientation(upper, lower, last);
        self.get_limb_mapping(c).set_limb_orientation(h, upper, lower);
    }

    fn is_node(&self, transform: usize) -> bool {
        super::chain::get_chain_index(&self.chain, transform) != -1
    }

    /// `IsValid`: what can still be wrong when bones are indexes.
    pub fn is_valid(&self) -> Result<(), Invalid> {
        let no = |text: &str| Err(Invalid(text.to_string()));
        if self.chain.is_empty() {
            return no("FBIK chain length is 0, can't initiate solver.");
        }
        if self.chain.iter().any(|c| c.nodes.is_empty()) {
            return no("FBIK chain contains no nodes.");
        }
        for e in &self.effectors {
            let planes = [e.plane_bone1, e.plane_bone2, e.plane_bone3];
            let bones = std::iter::once(e.bone)
                .chain(e.child_bones.iter().copied())
                .chain(planes.into_iter().flatten());
            for bone in bones {
                if !self.is_node(bone) {
                    return no("IK Effector is referencing to a bone that does not excist in the Node Chain.");
                }
            }
        }
        let s = &self.spine_mapping;
        if !s.spine_bones.iter().any(|&b| self.is_node(b)) {
            return no("IKMappingSpine does not contain any nodes.");
        }
        let corners = [
            s.left_upper_arm_bone,
            s.right_upper_arm_bone,
            s.left_thigh_bone,
            s.right_thigh_bone,
        ];
        if corners.into_iter().any(|b| !self.is_node(b)) {
            return no("Full Body IK is missing an upper arm or thigh node.");
        }
        for l in &self.limb_mappings {
            if [l.bone1, l.bone2, l.bone3].into_iter().any(|b| !self.is_node(b)) {
                return no("IKMappingLimb is referencing to a bone that does not excist in the Node Chain.");
            }
        }
        let limbs_of_three = self.chain.len() == 5 && self.chain[1..].iter().all(|c| c.nodes.len() == 3);
        if !limbs_of_three
            || self.chain[0].nodes.len() != 1
            || self.effectors.len() != 9
            || self.limb_mappings.len() != 4
        {
            return no("Invalid FBBIK setup. Please right-click on the component header and select 'Reinitiate'.");
        }
        Ok(())
    }

    /// `Initiate`: takes the pose the bones are in as the solver's
    /// starting state.
    pub fn initiate(&mut self, h: &Hierarchy) -> Result<(), Invalid> {
        self.initiated = false;
        self.is_valid()?;
        // OnInitiate.
        for i in 0..self.chain.len() {
            FbikChain::initiate(&mut self.chain, i, h);
        }
        for effector in &mut self.effectors {
            effector.initiate(&self.chain, h);
        }
        self.spine_mapping.initiate(&self.chain, h);
        for mapping in &mut self.bone_mappings {
            mapping.initiate(&self.chain);
        }
        for mapping in &mut self.limb_mappings {
            mapping.initiate(&self.chain, h);
        }
        self.store_default_local_state(h);
        self.initiated = true;
        self.first_initiation = false;
        Ok(())
    }

    /// `StoreDefaultLocalState`.
    pub fn store_default_local_state(&mut self, h: &Hierarchy) {
        self.spine_mapping.store_default_local_state(h);
        for mapping in &mut self.limb_mappings {
            mapping.store_default_local_state(h);
        }
        for mapping in &mut self.bone_mappings {
            mapping.store_default_local_state(h);
        }
    }

    /// `FixTransforms`: puts the solver's bones back to their stored local
    /// state, as the component does each frame before the animation.
    pub fn fix_transforms(&self, h: &mut Hierarchy) {
        if self.ik_position_weight <= 0.0 {
            return;
        }
        self.spine_mapping.fix_transforms(h);
        for mapping in &self.limb_mappings {
            mapping.fix_transforms(h);
        }
        for mapping in &self.bone_mappings {
            mapping.fix_transforms(h);
        }
    }

    /// `Update`: initiates the first time, then reads, solves and writes.
    pub fn update(&mut self, h: &mut Hierarchy) -> Result<(), Invalid> {
        if self.first_initiation {
            self.initiate(h)?;
        }
        if !self.initiated {
            return Ok(());
        }
        // OnUpdate.
        if self.ik_position_weight <= 0.0 {
            for effector in &mut self.effectors {
                effector.position_offset = Vec3::ZERO;
            }
        } else if !self.chain.is_empty() {
            self.ik_position_weight = unity::clamp(self.ik_position_weight, 0.0, 1.0);
            self.read_pose(h);
            self.solve(h);
            self.write_pose(h);
            for effector in &mut self.effectors {
                effector.on_post_write();
            }
        }
        Ok(())
    }

    /// `ReadPose`, the biped's and then the full body's.
    pub fn read_pose(&mut self, h: &mut Hierarchy) {
        for effector in &mut self.effectors {
            effector.set_to_target();
        }
        self.pull_body(h);
        let push_elasticity = unity::clamp(1.0 - self.spine_stiffness, 0.0, 1.0);
        self.chain[0].child_constraints[0].push_elasticity = push_elasticity;
        self.chain[0].child_constraints[1].push_elasticity = push_elasticity;

        // IKSolverFullBody.ReadPose.
        for i in 0..self.chain.len() {
            if self.chain[i].bend_constraint.initiated {
                let end = self.chain[i].nodes[2].transform;
                let position_weight = self
                    .effectors
                    .iter()
                    .find(|e| e.bone == end)
                    .map(|e| e.position_weight)
                    .expect("a limb's end node has an effector");
                self.chain[i]
                    .bend_constraint
                    .limit_bend(h, self.ik_position_weight, position_weight);
            }
        }
        for effector in &self.effectors {
            effector.reset_offset(&mut self.chain);
        }
        for effector in &mut self.effectors {
            effector.on_pre_solve(&mut self.chain, h, self.ik_position_weight, self.iterations);
        }
        for i in 0..self.chain.len() {
            FbikChain::read_pose(&mut self.chain, i, h, self.iterations > 0);
        }
        if self.iterations > 0 {
            self.spine_mapping.read_pose(h);
            for mapping in &mut self.bone_mappings {
                mapping.read_pose(h);
            }
        }
        for mapping in &mut self.limb_mappings {
            mapping.read_pose(h);
        }
    }

    fn pull_body(&mut self, h: &Hierarchy) {
        if self.iterations >= 1 && (self.pull_body_vertical != 0.0 || self.pull_body_horizontal != 0.0) {
            let body_offset = self.get_body_offset(h);
            let pulled = tools::extract_vertical(body_offset, self.root_up, self.pull_body_vertical)
                + tools::extract_horizontal(body_offset, self.root_up, self.pull_body_horizontal);
            self.effectors[0].position_offset += pulled;
        }
    }

    fn get_body_offset(&self, h: &Hierarchy) -> Vec3 {
        let (left, right) = (&self.effectors[5], &self.effectors[6]);
        let vector = Vec3::ZERO
            + self.get_hand_body_pull(h, left, &self.chain[1], Vec3::ZERO)
                * unity::clamp(left.position_weight, 0.0, 1.0);
        vector
            + self.get_hand_body_pull(h, right, &self.chain[2], vector) * unity::clamp(right.position_weight, 0.0, 1.0)
    }

    fn get_hand_body_pull(&self, h: &Hierarchy, effector: &IkEffector, arm: &FbikChain, offset: Vec3) -> Vec3 {
        let vector = effector.position - (h.position(arm.nodes[0].transform) + offset);
        let num = arm.nodes[0].length + arm.nodes[1].length;
        let magnitude = unity::magnitude(vector);
        if magnitude < num {
            return Vec3::ZERO;
        }
        let num2 = magnitude - num;
        vector / magnitude * num2
    }

    fn update_effectors(&mut self, h: &Hierarchy, end_effectors: bool) {
        for effector in &mut self.effectors {
            if effector.is_end_effector == end_effectors {
                effector.update(&mut self.chain, h);
            }
        }
    }

    /// `Solve`: moves the nodes; no bone is touched.
    pub fn solve(&mut self, h: &Hierarchy) {
        for _ in 0..self.iterations.max(0) {
            self.update_effectors(h, true);
            FbikChain::push(&mut self.chain, 0);
            FbikChain::reach(&mut self.chain, 0);
            self.update_effectors(h, false);
            FbikChain::solve_trigonometric(&mut self.chain, 0, h, self.ik_position_weight, false);
            FbikChain::stage1(&mut self.chain, 0);
            self.update_effectors(h, false);
            let position = self.chain[0].nodes[0].solver_position;
            FbikChain::stage2(&mut self.chain, 0, self.iterations, position);
        }
        self.update_effectors(h, true);
        self.apply_bend_constraints(h);
    }

    fn apply_bend_constraints(&mut self, h: &Hierarchy) {
        if self.iterations > 0 {
            for limb in 0..4 {
                self.chain[1 + limb].bend_constraint.rotation_offset = self.effectors[5 + limb].plane_rotation_offset;
            }
        } else {
            let body = &self.effectors[0];
            self.offset = unity::lerp(
                body.position_offset,
                body.position - (h.position(body.bone) + body.position_offset),
                body.position_weight,
            );
            for i in 0..5 {
                let (c, n) = (self.effectors[i].chain_index, self.effectors[i].node_index);
                self.chain[c as usize].nodes[n as usize].solver_position += self.offset;
            }
        }
        FbikChain::solve_trigonometric(&mut self.chain, 0, h, self.ik_position_weight, true);
    }

    /// `WritePose`: puts the bones where the nodes are.
    pub fn write_pose(&mut self, h: &mut Hierarchy) {
        if self.iterations == 0 {
            let hips = self.spine_mapping.spine_bones[0];
            h.set_position(hips, h.position(hips) + self.offset);
        }
        if self.ik_position_weight <= 0.0 {
            return;
        }
        if self.iterations > 0 {
            self.spine_mapping.write_pose(&self.chain, h);
            for mapping in &self.bone_mappings {
                mapping.write_pose(h, self.ik_position_weight);
            }
        }
        for mapping in &self.limb_mappings {
            mapping.write_pose(&self.chain, h, self.ik_position_weight, self.iterations > 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fbbik::Place;
    use crate::hierarchy::Transform;
    use glam::Quat;

    /// A small biped standing with its arms out, elbows and knees a little
    /// bent: the tree and its references.
    fn biped() -> (Hierarchy, BipedReferences) {
        fn add(h: &mut Hierarchy, parent: Option<usize>, x: f32, y: f32, z: f32) -> usize {
            h.push(Transform {
                parent,
                local_position: Vec3::new(x, y, z),
                local_rotation: Quat::IDENTITY,
                local_scale: Vec3::ONE,
            })
        }
        fn arm(h: &mut Hierarchy, spine: usize, side: f32) -> [usize; 3] {
            let shoulder = add(h, Some(spine), 0.05 * side, 0.2, 0.0);
            let upper = add(h, Some(shoulder), 0.1 * side, 0.0, 0.0);
            let forearm = add(h, Some(upper), 0.25 * side, 0.0, -0.02);
            let hand = add(h, Some(forearm), 0.25 * side, 0.0, 0.02);
            [upper, forearm, hand]
        }
        fn leg(h: &mut Hierarchy, pelvis: usize, side: f32) -> [usize; 3] {
            let thigh = add(h, Some(pelvis), 0.1 * side, -0.05, 0.0);
            let calf = add(h, Some(thigh), 0.0, -0.45, 0.02);
            let foot = add(h, Some(calf), 0.0, -0.45, -0.02);
            [thigh, calf, foot]
        }
        let mut h = Hierarchy::default();
        let root = add(&mut h, None, 0.0, 0.0, 0.0);
        let pelvis = add(&mut h, Some(root), 0.0, 1.0, 0.0);
        let spine1 = add(&mut h, Some(pelvis), 0.0, 0.1, 0.0);
        let spine2 = add(&mut h, Some(spine1), 0.0, 0.15, 0.0);
        let neck = add(&mut h, Some(spine2), 0.0, 0.25, 0.0);
        let head = add(&mut h, Some(neck), 0.0, 0.1, 0.0);
        let (left_arm, right_arm) = (arm(&mut h, spine2, -1.0), arm(&mut h, spine2, 1.0));
        let (left_leg, right_leg) = (leg(&mut h, pelvis, -1.0), leg(&mut h, pelvis, 1.0));
        let references = BipedReferences {
            pelvis,
            left_thigh: left_leg[0],
            left_calf: left_leg[1],
            left_foot: left_leg[2],
            right_thigh: right_leg[0],
            right_calf: right_leg[1],
            right_foot: right_leg[2],
            left_upper_arm: left_arm[0],
            left_forearm: left_arm[1],
            left_hand: left_arm[2],
            right_upper_arm: right_arm[0],
            right_forearm: right_arm[1],
            right_hand: right_arm[2],
            head: Some(head),
            spine: vec![spine1, spine2, neck],
        };
        (h, references)
    }

    fn places(h: &Hierarchy) -> Vec<Vec3> {
        (0..h.transforms.len()).map(|i| h.position(i)).collect()
    }

    fn furthest(a: &[Vec3], b: &[Vec3]) -> f32 {
        a.iter().zip(b).map(|(x, y)| (*x - *y).length()).fold(0.0, f32::max)
    }

    #[test]
    fn references_give_the_solver_finalik_sets_up() {
        let (h, references) = biped();
        let solver = IkSolverFullBodyBiped::from_references(&references, None, &h).unwrap();
        // The lowest spine bone is below the middle of thighs and arms.
        assert_eq!(solver.root_node, references.spine[0]);
        assert_eq!(solver.is_valid(), Ok(()));
        assert_eq!(solver.chain[0].children, vec![1, 2, 3, 4]);
        assert_eq!(solver.chain[0].child_constraints.len(), 4);
        assert_eq!(
            solver.effectors[0].child_bones,
            vec![references.left_thigh, references.right_thigh]
        );
        // Arms get their shoulder as the parent bone, legs get none.
        assert_eq!(solver.limb_mappings[0].parent_bone, h.parent(references.left_upper_arm));
        assert_eq!(solver.limb_mappings[2].parent_bone, None);
        assert_eq!(solver.limb_mappings[2].maintain_rotation_weight, 1.0);
        assert_eq!(solver.bone_mappings[0].maintain_rotation_weight, 0.0);
    }

    #[test]
    fn initiation_works_out_the_structure() {
        let (h, references) = biped();
        let mut solver = IkSolverFullBodyBiped::from_references(&references, None, &h).unwrap();
        solver.initiate(&h).unwrap();
        assert!(solver.initiated && !solver.first_initiation);
        // Hands and feet are end effectors on the last node of their limb.
        for i in 5..9 {
            let e = &solver.effectors[i];
            assert!(e.is_end_effector && e.use_plane_nodes);
            assert_eq!((e.chain_index, e.node_index), (i as i32 - 4, 2));
        }
        assert!(!solver.effectors[0].is_end_effector);
        assert_eq!(solver.effectors[0].child_chain_indexes, vec![3, 4]);
        // The spine has four bones, so it is solved with FABRIK.
        assert_eq!(solver.spine_mapping.root_node_index, 1);
        assert!(solver.spine_mapping.use_fabrik);
        // Limb lengths are the bones' distances.
        let arm = &solver.chain[1];
        assert!((arm.nodes[0].length - (0.25f32 * 0.25 + 0.02 * 0.02).sqrt()).abs() < 1e-6);
        assert!((arm.length - arm.nodes[0].length - arm.nodes[1].length).abs() < 1e-6);
        // The bend direction is square to the upper arm, towards the elbow.
        let bend = &arm.bend_constraint;
        assert!(bend.initiated);
        let upper = h.position(references.left_forearm) - h.position(references.left_upper_arm);
        assert!(unity::dot(bend.direction, upper).abs() < 1e-5);
        assert!(bend.direction.z < -0.99);
    }

    #[test]
    fn a_setup_with_a_stray_bone_does_not_initiate() {
        let (h, references) = biped();
        let mut solver = IkSolverFullBodyBiped::from_references(&references, None, &h).unwrap();
        solver.effectors[5].bone = references.head.unwrap();
        assert!(solver.initiate(&h).is_err());
        assert!(!solver.initiated);
        let mut moved = h.clone();
        assert!(solver.update(&mut moved).is_err());
        assert_eq!(furthest(&places(&h), &places(&moved)), 0.0);
    }

    #[test]
    fn with_no_weights_the_pose_stays() {
        let (mut h, references) = biped();
        let before = places(&h);
        let mut solver = IkSolverFullBodyBiped::from_references(&references, None, &h).unwrap();
        solver.update(&mut h).unwrap();
        let off = furthest(&before, &places(&h));
        assert!(off < 1e-4, "{off}");
    }

    #[test]
    fn a_hand_goes_to_its_effector_and_bones_keep_their_length() {
        let (mut h, references) = biped();
        let mut solver = IkSolverFullBodyBiped::from_references(&references, None, &h).unwrap();
        solver.initiate(&h).unwrap();
        let hand = references.left_hand;
        let target = h.position(hand) + Vec3::new(0.1, 0.15, 0.2);
        let e = solver.get_effector(FullBodyBipedEffector::LeftHand);
        e.position = target;
        e.position_weight = 1.0;
        let lengths = |h: &Hierarchy| {
            (
                unity::distance(
                    h.position(references.left_upper_arm),
                    h.position(references.left_forearm),
                ),
                unity::distance(h.position(references.left_forearm), h.position(hand)),
            )
        };
        let (upper, lower) = lengths(&h);
        solver.update(&mut h).unwrap();
        assert!((h.position(hand) - target).length() < 1e-4, "{}", h.position(hand));
        let (now_upper, now_lower) = lengths(&h);
        assert!((upper - now_upper).abs() < 1e-5 && (lower - now_lower).abs() < 1e-5);
    }

    #[test]
    fn a_target_and_a_rotation_weight_turn_the_hand() {
        let (mut h, references) = biped();
        let mut solver = IkSolverFullBodyBiped::from_references(&references, None, &h).unwrap();
        solver.initiate(&h).unwrap();
        let hand = references.right_hand;
        let place = Place {
            position: h.position(hand) + Vec3::new(-0.1, 0.1, 0.1),
            rotation: unity::angle_axis(40.0, Vec3::new(0.3, 1.0, 0.2)),
        };
        solver.get_effector(FullBodyBipedEffector::RightHand).target = Some(place);
        solver.set_effector_weights(FullBodyBipedEffector::RightHand, 1.0, 1.0);
        solver.update(&mut h).unwrap();
        assert!((h.position(hand) - place.position).length() < 1e-4);
        assert!(unity::quat_dot(h.rotation(hand), place.rotation).abs() > 0.999_999);
    }

    #[test]
    fn fix_transforms_puts_the_bones_back() {
        let (mut h, references) = biped();
        let before = places(&h);
        let mut solver = IkSolverFullBodyBiped::from_references(&references, None, &h).unwrap();
        solver.initiate(&h).unwrap();
        let e = solver.get_effector(FullBodyBipedEffector::LeftFoot);
        e.position += Vec3::new(0.0, 0.2, 0.1);
        e.position_weight = 1.0;
        solver.update(&mut h).unwrap();
        assert!(furthest(&before, &places(&h)) > 0.1);
        solver.fix_transforms(&mut h);
        assert!(furthest(&before, &places(&h)) < 1e-6);
    }

    #[test]
    fn a_hand_out_of_reach_pulls_the_body_when_asked() {
        let (h, references) = biped();
        // The offsets the body and thigh nodes are read with.
        let offsets = |pull: f32| {
            let mut h = h.clone();
            let mut solver = IkSolverFullBodyBiped::from_references(&references, None, &h).unwrap();
            solver.pull_body_vertical = pull;
            solver.initiate(&h).unwrap();
            let e = solver.get_effector(FullBodyBipedEffector::LeftHand);
            e.position += Vec3::new(0.0, 0.6, 0.0);
            e.position_weight = 1.0;
            solver.read_pose(&mut h);
            (solver.chain[0].nodes[0].offset, solver.chain[3].nodes[0].offset)
        };
        // The hand is 0.6 above, 0.5 to the side of the shoulder; the arm
        // is as long as its two bones.
        let bone = (0.25f32 * 0.25 + 0.02 * 0.02).sqrt();
        let away = (0.5f32 * 0.5 + 0.6 * 0.6).sqrt();
        let up = 0.6 / away * (away - 2.0 * bone) * 0.5;
        let (body, thigh) = offsets(0.5);
        assert!((body - Vec3::new(0.0, up, 0.0)).length() < 1e-5, "{body}");
        assert_eq!(body, thigh);
        assert_eq!(offsets(0.0), (Vec3::ZERO, Vec3::ZERO));
    }
}
