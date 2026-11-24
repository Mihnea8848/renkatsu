// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! `FBIKChain`: one chain of the full body solver (the body, or a limb of
//! three nodes) and the constraints between its child chains. Port of
//! RootMotion FinalIK (`FBIKChain.cs`).
//!
//! A chain reaches into the other chains of the solver, so the methods
//! take the solver's chains and the index of the one they work on.

use super::constraint_bend::IkConstraintBend;
use super::point::Node;
use crate::hierarchy::Hierarchy;
use crate::unity;
use glam::Vec3;

/// `IKSolverFullBody.GetChainAndNodeIndexes`: where a transform is among
/// the nodes, or -1 and -1.
pub fn get_chain_and_node_indexes(chain: &[FbikChain], transform: usize) -> (i32, i32) {
    for (i, c) in chain.iter().enumerate() {
        for (j, node) in c.nodes.iter().enumerate() {
            if node.transform == transform {
                return (i as i32, j as i32);
            }
        }
    }
    (-1, -1)
}

/// `IKSolverFullBody.GetChainIndex`.
pub fn get_chain_index(chain: &[FbikChain], transform: usize) -> i32 {
    get_chain_and_node_indexes(chain, transform).0
}

/// `FBIKChain.Smoothing`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Smoothing {
    None,
    #[default]
    Exponential,
    Cubic,
}

/// `FBIKChain.ChildConstraint`: holds the first nodes of two child chains
/// at their animated distance.
#[derive(Clone, Copy, Debug)]
pub struct ChildConstraint {
    pub push_elasticity: f32,
    pub pull_elasticity: f32,
    pub bone1: usize,
    pub bone2: usize,
    // Private in FinalIK.
    pub cross_fade: f32,
    pub inverse_cross_fade: f32,
    pub chain1_index: i32,
    pub chain2_index: i32,
    pub nominal_distance: f32,
    pub is_rigid: bool,
}

impl ChildConstraint {
    pub fn new(bone1: usize, bone2: usize, push_elasticity: f32, pull_elasticity: f32) -> Self {
        Self {
            push_elasticity,
            pull_elasticity,
            bone1,
            bone2,
            cross_fade: 0.0,
            inverse_cross_fade: 0.0,
            chain1_index: 0,
            chain2_index: 0,
            nominal_distance: 0.0,
            is_rigid: false,
        }
    }

    /// `Initiate`.
    pub fn initiate(&mut self, chain: &[FbikChain], h: &Hierarchy) {
        self.chain1_index = get_chain_index(chain, self.bone1);
        self.chain2_index = get_chain_index(chain, self.bone2);
        self.on_pre_solve(chain, h);
    }

    /// `OnPreSolve`.
    pub fn on_pre_solve(&mut self, chain: &[FbikChain], h: &Hierarchy) {
        let (c1, c2) = (&chain[self.chain1_index as usize], &chain[self.chain2_index as usize]);
        self.nominal_distance = unity::distance(h.position(c1.nodes[0].transform), h.position(c2.nodes[0].transform));
        self.is_rigid = self.push_elasticity <= 0.0 && self.pull_elasticity <= 0.0;
        if self.is_rigid {
            let num = c1.pull - c2.pull;
            self.cross_fade = 1.0 - (0.5 + num * 0.5);
        } else {
            self.cross_fade = 0.5;
        }
        self.inverse_cross_fade = 1.0 - self.cross_fade;
    }

    /// `Solve`.
    pub fn solve(&self, chain: &mut [FbikChain]) {
        if self.push_elasticity >= 1.0 && self.pull_elasticity >= 1.0 {
            return;
        }
        let (i1, i2) = (self.chain1_index as usize, self.chain2_index as usize);
        let vector = chain[i2].nodes[0].solver_position - chain[i1].nodes[0].solver_position;
        let magnitude = unity::magnitude(vector);
        if magnitude != self.nominal_distance && magnitude != 0.0 {
            let mut num = 1.0;
            if !self.is_rigid {
                let num2 = if magnitude > self.nominal_distance {
                    self.pull_elasticity
                } else {
                    self.push_elasticity
                };
                num = 1.0 - num2;
            }
            num *= 1.0 - self.nominal_distance / magnitude;
            let vector2 = vector * num;
            chain[i1].nodes[0].solver_position += vector2 * self.cross_fade;
            chain[i2].nodes[0].solver_position -= vector2 * self.inverse_cross_fade;
        }
    }
}

#[derive(Clone, Debug)]
pub struct FbikChain {
    pub pin: f32,
    pub pull: f32,
    pub push: f32,
    pub push_parent: f32,
    pub reach: f32,
    pub reach_smoothing: Smoothing,
    pub push_smoothing: Smoothing,
    pub nodes: Vec<Node>,
    pub children: Vec<usize>,
    pub child_constraints: Vec<ChildConstraint>,
    pub bend_constraint: IkConstraintBend,
    // Private in FinalIK.
    pub root_length: f32,
    pub initiated: bool,
    pub length: f32,
    pub distance: f32,
    pub reach_force: f32,
    pub pull_parent_sum: f32,
    pub cross_fades: Vec<f32>,
    pub sqr_mag1: f32,
    pub sqr_mag2: f32,
    pub sqr_mag_dif: f32,
}

impl Default for FbikChain {
    fn default() -> Self {
        Self {
            pin: 0.0,
            pull: 1.0,
            push: 0.0,
            push_parent: 0.0,
            reach: 0.1,
            reach_smoothing: Smoothing::Exponential,
            push_smoothing: Smoothing::Exponential,
            nodes: Vec::new(),
            children: Vec::new(),
            child_constraints: Vec::new(),
            bend_constraint: IkConstraintBend::default(),
            root_length: 0.0,
            initiated: false,
            length: 0.0,
            distance: 0.0,
            reach_force: 0.0,
            pull_parent_sum: 0.0,
            cross_fades: Vec::new(),
            sqr_mag1: 0.0,
            sqr_mag2: 0.0,
            sqr_mag_dif: 0.0,
        }
    }
}

/// `FBIKChain.maxLimbLength`.
const MAX_LIMB_LENGTH: f32 = 0.99999;

/// `SolveFABRIKJoint`.
pub fn solve_fabrik_joint(pos1: Vec3, pos2: Vec3, length: f32) -> Vec3 {
    pos2 + unity::normalize(pos1 - pos2) * length
}

impl FbikChain {
    /// `SetNodes`.
    pub fn set_nodes(&mut self, bone_transforms: &[usize]) {
        self.nodes = bone_transforms.iter().map(|&t| Node::new(t)).collect();
    }

    /// `GetNodeIndex`.
    pub fn get_node_index(&self, bone_transform: usize) -> i32 {
        self.nodes
            .iter()
            .position(|n| n.transform == bone_transform)
            .map_or(-1, |i| i as i32)
    }

    fn last(&self) -> usize {
        self.nodes.len() - 1
    }

    /// `Initiate`, for `chain[i]`.
    pub fn initiate(chain: &mut [FbikChain], i: usize, h: &Hierarchy) {
        chain[i].initiated = false;
        for node in &mut chain[i].nodes {
            node.solver_position = h.position(node.transform);
        }
        Self::calculate_bone_lengths(chain, i, h);
        for k in 0..chain[i].child_constraints.len() {
            let mut constraint = chain[i].child_constraints[k];
            constraint.initiate(chain, h);
            chain[i].child_constraints[k] = constraint;
        }
        if chain[i].nodes.len() == 3 {
            let (b1, b2, b3) = (
                chain[i].nodes[0].transform,
                chain[i].nodes[1].transform,
                chain[i].nodes[2].transform,
            );
            chain[i].bend_constraint.set_bones(b1, b2, b3);
            IkConstraintBend::initiate(chain, i, h);
        }
        chain[i].cross_fades = vec![0.0; chain[i].children.len()];
        chain[i].initiated = true;
    }

    /// `ReadPose`, for `chain[i]`.
    pub fn read_pose(chain: &mut [FbikChain], i: usize, h: &Hierarchy, full_body: bool) {
        if !chain[i].initiated {
            return;
        }
        for node in &mut chain[i].nodes {
            node.solver_position = h.position(node.transform) + node.offset;
        }
        Self::calculate_bone_lengths(chain, i, h);
        if !full_body {
            return;
        }
        for k in 0..chain[i].child_constraints.len() {
            let mut constraint = chain[i].child_constraints[k];
            constraint.on_pre_solve(chain, h);
            chain[i].child_constraints[k] = constraint;
        }
        let children = chain[i].children.clone();
        if !children.is_empty() {
            let mut num = chain[i].nodes[chain[i].last()].effector_position_weight;
            for &child in &children {
                num += chain[child].nodes[0].effector_position_weight * chain[child].pull;
            }
            num = unity::clamp(num, 1.0, f32::INFINITY);
            for (l, &child) in children.iter().enumerate() {
                chain[i].cross_fades[l] = chain[child].nodes[0].effector_position_weight * chain[child].pull / num;
            }
        }
        let mut pull_parent_sum = 0.0f32;
        for &child in &children {
            pull_parent_sum += chain[child].pull;
        }
        let c = &mut chain[i];
        c.pull_parent_sum = unity::clamp(pull_parent_sum, 1.0, f32::INFINITY);
        if c.nodes.len() == 3 {
            c.reach_force = c.reach * unity::clamp(c.nodes[2].effector_position_weight, 0.0, 1.0);
        } else {
            c.reach_force = 0.0;
        }
        if c.push > 0.0 && c.nodes.len() > 1 {
            c.distance = unity::distance(
                h.position(c.nodes[0].transform),
                h.position(c.nodes[c.last()].transform),
            );
        }
    }

    fn calculate_bone_lengths(chain: &mut [FbikChain], i: usize, h: &Hierarchy) {
        chain[i].length = 0.0;
        for n in 0..chain[i].last() {
            let c = &mut chain[i];
            c.nodes[n].length = unity::distance(h.position(c.nodes[n].transform), h.position(c.nodes[n + 1].transform));
            c.length += c.nodes[n].length;
            if c.nodes[n].length == 0.0 {
                // "Bone length is zero, can not solve."
                return;
            }
        }
        let end = h.position(chain[i].nodes[chain[i].last()].transform);
        for j in 0..chain[i].children.len() {
            let child = chain[i].children[j];
            chain[child].root_length = unity::magnitude(h.position(chain[child].nodes[0].transform) - end);
            if chain[child].root_length == 0.0 {
                return;
            }
        }
        let c = &mut chain[i];
        if c.nodes.len() == 3 {
            c.sqr_mag1 = c.nodes[0].length * c.nodes[0].length;
            c.sqr_mag2 = c.nodes[1].length * c.nodes[1].length;
            c.sqr_mag_dif = c.sqr_mag1 - c.sqr_mag2;
        }
    }

    /// `Reach`, for `chain[i]`.
    pub fn reach(chain: &mut [FbikChain], i: usize) {
        if !chain[i].initiated {
            return;
        }
        for k in 0..chain[i].children.len() {
            let child = chain[i].children[k];
            Self::reach(chain, child);
        }
        let c = &mut chain[i];
        if c.reach_force <= 0.0 {
            return;
        }
        let vector = c.nodes[2].solver_position - c.nodes[0].solver_position;
        if unity::vec_eq(vector, Vec3::ZERO) {
            return;
        }
        let magnitude = unity::magnitude(vector);
        let vector2 = vector / magnitude * c.length;
        let mut num = unity::clamp(magnitude / c.length, 1.0 - c.reach_force, 1.0 + c.reach_force) - 1.0;
        num = unity::clamp(num + c.reach_force, -1.0, 1.0);
        match c.reach_smoothing {
            Smoothing::Exponential => num *= num,
            Smoothing::Cubic => num = num * (num * num),
            Smoothing::None => {}
        }
        let vector3 = vector2 * unity::clamp(num, 0.0, magnitude);
        let weight = 1.0 - c.nodes[0].effector_position_weight;
        c.nodes[0].solver_position += vector3 * weight;
        c.nodes[2].solver_position += vector3;
    }

    /// `Push`, for `chain[i]`.
    pub fn push(chain: &mut [FbikChain], i: usize) -> Vec3 {
        let mut zero = Vec3::ZERO;
        for k in 0..chain[i].children.len() {
            let child = chain[i].children[k];
            let pushed = Self::push(chain, child);
            zero += pushed * chain[child].push_parent;
        }
        let c = &mut chain[i];
        let last = c.last();
        c.nodes[last].solver_position += zero;
        if c.nodes.len() < 2 {
            return Vec3::ZERO;
        }
        if c.push <= 0.0 {
            return Vec3::ZERO;
        }
        let vector = c.nodes[2].solver_position - c.nodes[0].solver_position;
        let magnitude = unity::magnitude(vector);
        if magnitude == 0.0 {
            return Vec3::ZERO;
        }
        let mut num = 1.0 - magnitude / c.distance;
        if num <= 0.0 {
            return Vec3::ZERO;
        }
        match c.push_smoothing {
            Smoothing::Exponential => num *= num,
            Smoothing::Cubic => num = num * (num * num),
            Smoothing::None => {}
        }
        let vector2 = -vector * num * c.push;
        c.nodes[0].solver_position += vector2;
        vector2
    }

    /// `SolveTrigonometric`, for `chain[i]`: places the middle node of
    /// every limb below.
    pub fn solve_trigonometric(
        chain: &mut [FbikChain],
        i: usize,
        h: &Hierarchy,
        ik_position_weight: f32,
        calculate_bend_direction: bool,
    ) {
        if !chain[i].initiated {
            return;
        }
        for k in 0..chain[i].children.len() {
            let child = chain[i].children[k];
            Self::solve_trigonometric(chain, child, h, ik_position_weight, calculate_bend_direction);
        }
        if chain[i].nodes.len() != 3 {
            return;
        }
        let vector = chain[i].nodes[2].solver_position - chain[i].nodes[0].solver_position;
        let magnitude = unity::magnitude(vector);
        if magnitude == 0.0 {
            return;
        }
        let num = unity::clamp(magnitude, 0.0, chain[i].length * MAX_LIMB_LENGTH);
        let direction = vector / magnitude * num;
        let bend_direction = if calculate_bend_direction && chain[i].bend_constraint.initiated {
            IkConstraintBend::get_dir(chain, i, h, ik_position_weight)
        } else {
            chain[i].nodes[1].solver_position - chain[i].nodes[0].solver_position
        };
        let c = &mut chain[i];
        let dir_to_bend_point = c.get_dir_to_bend_point(direction, bend_direction, num);
        c.nodes[1].solver_position = c.nodes[0].solver_position + dir_to_bend_point;
    }

    /// `Stage1`, for `chain[i]`: from the ends towards the body.
    pub fn stage1(chain: &mut [FbikChain], i: usize) {
        for k in 0..chain[i].children.len() {
            let child = chain[i].children[k];
            Self::stage1(chain, child);
        }
        let last = chain[i].last();
        if chain[i].children.is_empty() {
            let position = chain[i].nodes[last].solver_position;
            chain[i].forward_reach(position);
            return;
        }
        let mut solver_position = chain[i].nodes[last].solver_position;
        Self::solve_child_constraints(chain, i);
        for k in 0..chain[i].children.len() {
            let child = chain[i].children[k];
            let end = chain[i].nodes[last].solver_position;
            let mut vector = chain[child].nodes[0].solver_position;
            if chain[child].root_length > 0.0 {
                vector = solve_fabrik_joint(end, chain[child].nodes[0].solver_position, chain[child].root_length);
            }
            if chain[i].pull_parent_sum > 0.0 {
                solver_position += (vector - end) * (chain[child].pull / chain[i].pull_parent_sum);
            }
        }
        let c = &mut chain[i];
        let position = unity::lerp(solver_position, c.nodes[last].solver_position, c.pin);
        c.forward_reach(position);
    }

    /// `Stage2`, for `chain[i]`: from the body back to the ends.
    pub fn stage2(chain: &mut [FbikChain], i: usize, iterations: i32, position: Vec3) {
        chain[i].backward_reach(position);
        let num = iterations.clamp(2, 4);
        if !chain[i].child_constraints.is_empty() {
            for _ in 0..num {
                Self::solve_constraint_systems(chain, i);
            }
        }
        for k in 0..chain[i].children.len() {
            let child = chain[i].children[k];
            let end = chain[i].nodes[chain[i].last()].solver_position;
            Self::stage2(chain, child, iterations, end);
        }
    }

    /// `SolveConstraintSystems`, for `chain[i]`.
    pub fn solve_constraint_systems(chain: &mut [FbikChain], i: usize) {
        Self::solve_child_constraints(chain, i);
        let last = chain[i].last();
        for k in 0..chain[i].children.len() {
            let child = chain[i].children[k];
            let (cross_fade, distance) = (chain[i].cross_fades[k], chain[child].root_length);
            // SolveLinearConstraint.
            let vector = chain[child].nodes[0].solver_position - chain[i].nodes[last].solver_position;
            let magnitude = unity::magnitude(vector);
            if distance != magnitude && magnitude != 0.0 {
                let vector2 = vector * (1.0 - distance / magnitude);
                chain[i].nodes[last].solver_position += vector2 * cross_fade;
                chain[child].nodes[0].solver_position -= vector2 * (1.0 - cross_fade);
            }
        }
    }

    /// `GetDirToBendPoint`.
    fn get_dir_to_bend_point(&self, direction: Vec3, bend_direction: Vec3, direction_magnitude: f32) -> Vec3 {
        let num = (direction_magnitude * direction_magnitude + self.sqr_mag_dif) / 2.0 / direction_magnitude;
        let y = unity::sqrt(unity::clamp(self.sqr_mag1 - num * num, 0.0, f32::INFINITY));
        if unity::vec_eq(direction, Vec3::ZERO) {
            return Vec3::ZERO;
        }
        unity::rotate(unity::look_rotation(direction, bend_direction), Vec3::new(0.0, y, num))
    }

    fn solve_child_constraints(chain: &mut [FbikChain], i: usize) {
        for k in 0..chain[i].child_constraints.len() {
            let constraint = chain[i].child_constraints[k];
            constraint.solve(chain);
        }
    }

    /// `ForwardReach`.
    pub fn forward_reach(&mut self, position: Vec3) {
        let last = self.last();
        self.nodes[last].solver_position = position;
        for n in (0..last).rev() {
            self.nodes[n].solver_position = solve_fabrik_joint(
                self.nodes[n].solver_position,
                self.nodes[n + 1].solver_position,
                self.nodes[n].length,
            );
        }
    }

    fn backward_reach(&mut self, position: Vec3) {
        let mut position = position;
        if self.root_length > 0.0 {
            position = solve_fabrik_joint(self.nodes[0].solver_position, position, self.root_length);
        }
        self.nodes[0].solver_position = position;
        for n in 1..self.nodes.len() {
            self.nodes[n].solver_position = solve_fabrik_joint(
                self.nodes[n].solver_position,
                self.nodes[n - 1].solver_position,
                self.nodes[n - 1].length,
            );
        }
    }
}
