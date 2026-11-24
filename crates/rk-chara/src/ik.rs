// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! A character's IK as CharaStudio runs it: the body's FullBodyBipedIK
//! (`crate::fbbik`, solved by `rk_ik`) set up when the character is
//! added, in the pose she loads in (`AddObjectFemale.Add`), with Studio's
//! 13 targets: the effectors' targets and the limbs' bend goals, whose
//! bend constraints get a weight of 1 (`AddObjectAssist.InitIKTarget`).
//! A group switched on gives its effectors and mapping a weight of 1
//! (`OCIChar.ActiveIKGroup`), and KKPE's Optimize IK zeroes the spine's
//! stiffness and the body's vertical pull.

use crate::fbbik::{self, Settings};
use crate::pose::{ik_target_turns, IkGroup, IkTarget, Layer, IK_TARGETS};
use crate::scene::{Scene, Trs};
use glam::{Mat4, Quat, Vec3};
use rk_ik::fbbik::{
    ChildConstraint, FbikChain, IkEffector, IkMappingBone, IkMappingLimb, IkMappingSpine, IkSolverFullBodyBiped,
    Smoothing,
};
use rk_ik::hierarchy::{Hierarchy, Transform};

/// The solver of a character and what it works on.
#[derive(Clone, Debug)]
pub struct Rig {
    solver: IkSolverFullBodyBiped,
    /// The character's nodes as the solver sees them, one to one.
    h: Hierarchy,
    /// The object the component sits on, whose up the body pulls along.
    root: usize,
    /// The game's own stiffness and pull, for Optimize IK off.
    spine_stiffness: f32,
    pull_body_vertical: f32,
    /// The bones the solver writes, with how they were loaded, to put back
    /// before the next frame.
    written: Vec<(usize, Trs)>,
    /// Where each target starts when IK is switched on: the bone it is
    /// copied from (`IKInfo.CopyBone`).
    sources: [usize; IK_TARGETS],
}

fn smoothing(s: fbbik::Smoothing) -> Smoothing {
    match s {
        fbbik::Smoothing::None => Smoothing::None,
        fbbik::Smoothing::Exponential => Smoothing::Exponential,
        fbbik::Smoothing::Cubic => Smoothing::Cubic,
    }
}

fn transform(t: &Trs, parent: Option<usize>) -> Transform {
    Transform {
        parent,
        local_position: t.pos,
        local_rotation: t.rot,
        local_scale: t.scale,
    }
}

impl Rig {
    /// The rig of a character from the game's settings, initiated with her
    /// bones at `rest`, as loaded. `None` when the settings lack a bone.
    pub fn new(s: &Settings, scene: &Scene, rest: &[Trs], root: usize) -> Option<Rig> {
        let h = Hierarchy::new(
            scene
                .nodes
                .iter()
                .zip(rest)
                .map(|(n, t)| transform(t, n.parent))
                .collect(),
        );
        let mut chain: Vec<FbikChain> = Vec::new();
        for c in &s.chains {
            let mut out = FbikChain {
                pin: c.pin,
                pull: c.pull,
                push: c.push,
                push_parent: c.push_parent,
                reach: c.reach,
                reach_smoothing: smoothing(c.reach_smoothing),
                push_smoothing: smoothing(c.push_smoothing),
                children: c.children.iter().map(|&i| i.max(0) as usize).collect(),
                ..FbikChain::default()
            };
            let nodes: Option<Vec<usize>> = c.nodes.iter().map(|n| n.transform).collect();
            out.set_nodes(&nodes?);
            for (node, weight) in out.nodes.iter_mut().zip(&c.nodes) {
                node.weight = weight.weight;
            }
            out.bend_constraint.weight = c.bend_constraint.weight;
            out.bend_constraint.direction = c.bend_constraint.direction;
            out.bend_constraint.rotation_offset = c.bend_constraint.rotation_offset;
            out.child_constraints = c
                .child_constraints
                .iter()
                .filter_map(|k| {
                    Some(ChildConstraint::new(
                        k.bone1?,
                        k.bone2?,
                        k.push_elasticity,
                        k.pull_elasticity,
                    ))
                })
                .collect();
            chain.push(out);
        }
        // Studio's bend goals on the four limbs weigh 1.
        for limb in chain.iter_mut().skip(1) {
            limb.bend_constraint.weight = 1.0;
        }
        let mut effectors = Vec::new();
        for e in &s.effectors {
            let mut out = IkEffector::new(e.bone?);
            out.position_weight = e.position_weight;
            out.rotation_weight = e.rotation_weight;
            out.position = e.position;
            out.rotation = e.rotation;
            out.position_offset = e.position_offset;
            out.effect_child_nodes = e.effect_child_nodes;
            out.maintain_relative_position_weight = e.maintain_relative_position_weight;
            out.child_bones = e.child_bones.iter().flatten().copied().collect();
            out.plane_bone1 = e.plane_bone1;
            out.plane_bone2 = e.plane_bone2;
            out.plane_bone3 = e.plane_bone3;
            out.plane_rotation_offset = e.plane_rotation_offset;
            effectors.push(out);
        }
        let m = &s.spine_mapping;
        let mut spine_mapping = IkMappingSpine::new(
            m.spine_bones.iter().copied().collect::<Option<Vec<_>>>()?,
            m.left_upper_arm_bone?,
            m.right_upper_arm_bone?,
            m.left_thigh_bone?,
            m.right_thigh_bone?,
        );
        spine_mapping.iterations = m.iterations;
        spine_mapping.twist_weight = m.twist_weight;
        let bone_mappings = s
            .bone_mappings
            .iter()
            .filter_map(|b| {
                let mut out = IkMappingBone::new(b.bone?);
                out.maintain_rotation_weight = b.maintain_rotation_weight;
                Some(out)
            })
            .collect::<Vec<_>>();
        let mut limb_mappings = Vec::new();
        for l in &s.limb_mappings {
            let mut out = IkMappingLimb::new(l.bone1?, l.bone2?, l.bone3?, l.parent_bone);
            out.maintain_rotation_weight = l.maintain_rotation_weight;
            out.weight = l.weight;
            limb_mappings.push(out);
        }
        if chain.len() != 5 || effectors.len() != 9 || limb_mappings.len() != 4 {
            return None;
        }
        // What the solver writes: its chains, the spine and the mapped bones.
        let mut written: Vec<usize> = chain.iter().flat_map(|c| c.nodes.iter().map(|n| n.transform)).collect();
        written.extend(m.spine_bones.iter().flatten());
        written.extend(s.bone_mappings.iter().filter_map(|b| b.bone));
        for l in &s.limb_mappings {
            written.extend([l.parent_bone, l.bone1, l.bone2, l.bone3].into_iter().flatten());
        }
        written.sort_unstable();
        written.dedup();
        let written: Vec<(usize, Trs)> = written.into_iter().filter_map(|n| Some((n, *rest.get(n)?))).collect();
        let r = &s.references;
        let sources = [
            r.pelvis?,
            r.left_upper_arm?,
            r.left_forearm?,
            r.left_hand?,
            r.right_upper_arm?,
            r.right_forearm?,
            r.right_hand?,
            r.left_thigh?,
            r.left_calf?,
            r.left_foot?,
            r.right_thigh?,
            r.right_calf?,
            r.right_foot?,
        ];
        let mut solver = IkSolverFullBodyBiped {
            ik_position: Vec3::ZERO,
            ik_position_weight: s.ik_position_weight,
            first_initiation: true,
            initiated: false,
            root_up: Vec3::Y,
            iterations: s.iterations,
            chain,
            effectors,
            spine_mapping,
            bone_mappings,
            limb_mappings,
            root_node: s.root_node?,
            spine_stiffness: s.spine_stiffness,
            pull_body_vertical: s.pull_body_vertical,
            pull_body_horizontal: s.pull_body_horizontal,
            offset: Vec3::ZERO,
        };
        solver.initiate(&h).ok()?;
        Some(Rig {
            solver,
            h,
            root,
            spine_stiffness: s.spine_stiffness,
            pull_body_vertical: s.pull_body_vertical,
            written,
            sources,
        })
    }

    /// Puts the bones the solver writes back to how they were loaded, before
    /// a frame is posed (`FixTransforms`), so that it does not build on
    /// its own last answer.
    pub fn fix(&self, scene: &mut Scene) {
        for &(node, rest) in &self.written {
            if let Some(n) = scene.nodes.get_mut(node) {
                n.local = rest;
            }
        }
    }

    /// The targets on the bones as they stand, in the space of `top`, as
    /// Studio's copy puts them: the hands and feet turned as theirs, the
    /// others with no turn of their own.
    pub fn targets_on_bones(&self, scene: &Scene, top: usize) -> Vec<IkTarget> {
        let (world, _) = scene.world();
        let to_own = world[top].inverse();
        let (_, top_turn, _) = world[top].to_scale_rotation_translation();
        self.sources
            .iter()
            .enumerate()
            .map(|(i, &bone)| {
                let (_, turn, at) = world[bone].to_scale_rotation_translation();
                IkTarget {
                    pos: to_own.transform_point3(at),
                    rot: if ik_target_turns(i) {
                        (top_turn.inverse() * turn).normalize()
                    } else {
                        Quat::IDENTITY
                    },
                }
            })
            .collect()
    }

    /// Moves the body to the layer's targets, over the pose the bones hold
    /// now, while IK is on.
    pub fn solve(&mut self, scene: &mut Scene, top: usize, layer: &Layer) {
        if !layer.ik_on || layer.ik.len() != IK_TARGETS {
            return;
        }
        // The scene as it stands; nodes added since the rig was made (an
        // outfit, an accessory) come after the ones it knows.
        self.h.transforms.clear();
        self.h
            .transforms
            .extend(scene.nodes.iter().map(|n| transform(&n.local, n.parent)));
        let (world, _) = scene.world();
        let own: Mat4 = world[top];
        let (_, own_turn, _) = own.to_scale_rotation_translation();
        let place = |t: &IkTarget| rk_ik::fbbik::Place {
            position: own.transform_point3(t.pos),
            rotation: (own_turn * t.rot).normalize(),
        };
        let s = &mut self.solver;
        // The groups' weights, as Studio switches them.
        for group in IkGroup::ALL {
            let w = if layer.ik_groups.contains(&group) { 1.0 } else { 0.0 };
            let (effectors, limb): (&[usize], Option<usize>) = match group {
                IkGroup::Body => (&[0], None),
                IkGroup::LeftArm => (&[1, 5], Some(0)),
                IkGroup::RightArm => (&[2, 6], Some(1)),
                IkGroup::LeftLeg => (&[3, 7], Some(2)),
                IkGroup::RightLeg => (&[4, 8], Some(3)),
            };
            for &e in effectors {
                s.effectors[e].position_weight = w;
                s.effectors[e].rotation_weight = w;
            }
            match limb {
                Some(l) => s.limb_mappings[l].weight = w,
                None => s.spine_mapping.twist_weight = w,
            }
        }
        (s.spine_stiffness, s.pull_body_vertical) = if layer.optimize_ik {
            (0.0, 0.0)
        } else {
            (self.spine_stiffness, self.pull_body_vertical)
        };
        // Targets by effector, in the solver's order, and the bend goals.
        let effector_targets = [0, 1, 4, 7, 10, 3, 6, 9, 12];
        for (e, &target) in effector_targets.iter().enumerate() {
            s.effectors[e].target = Some(place(&layer.ik[target]));
        }
        for (c, target) in [(1, 2), (2, 5), (3, 8), (4, 11)] {
            s.chain[c].bend_constraint.bend_goal = Some(place(&layer.ik[target]).position);
        }
        let (_, root_turn, _) = world[self.root].to_scale_rotation_translation();
        s.root_up = root_turn * Vec3::Y;
        if s.update(&mut self.h).is_err() {
            return;
        }
        for &(node, _) in &self.written {
            let t = &self.h.transforms[node];
            let local = &mut scene.nodes[node].local;
            local.pos = t.local_position;
            local.rot = t.local_rotation;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simple, a card of the installed game, when there is one.
    fn simple() -> Option<crate::build::Character> {
        let game = rk_vfs::Game::open_user(&[]).ok()?;
        let dir = rk_core::Config::load().ok()?.game_dir;
        let card = rk_card::Card::load(dir.join("UserData/chara/female/Simple.png")).ok()?;
        let options = crate::BuildOptions {
            clothes: false,
            accessories: false,
            physics: false,
            ..Default::default()
        };
        crate::build(&game, &card, &options).ok()
    }

    #[test]
    fn a_moved_hand_target_brings_the_hand_and_still_targets_keep_the_pose() {
        let Some(mut chara) = simple() else {
            eprintln!("skipped: no game with Simple.png");
            return;
        };
        let rig = chara.ik_rig.clone().expect("the body has FullBodyBipedIK");
        let hand = rig.sources[3];
        let world = |c: &crate::build::Character, n: usize| c.scene.world().0[n].w_axis.truncate();
        let before = world(&chara, hand);
        let mut layer = Layer {
            ik_on: true,
            ..Layer::default()
        };
        layer.ik = rig.targets_on_bones(&chara.scene, chara.top);
        chara.pose_frame(None, &[], Some(&layer), &Default::default());
        let still = world(&chara, hand);
        assert!(
            still.distance(before) < 0.002,
            "targets on the bones keep the hand: {}",
            still.distance(before)
        );
        // A point the arm reaches: a third of the way to the shoulder and
        // up a little.
        let shoulder = world(&chara, rig.sources[1]);
        let want = before.lerp(shoulder, 0.3) + Vec3::new(0.0, 0.05, 0.05);
        let own = chara.scene.world().0[chara.top].inverse();
        layer.ik[3].pos = own.transform_point3(want);
        chara.pose_frame(None, &[], Some(&layer), &Default::default());
        let moved = world(&chara, hand);
        assert!(
            moved.distance(want) < 0.002,
            "the hand goes where its target went: {} m off",
            moved.distance(want)
        );
        // A target out of reach: the arm reaches towards it, straight.
        layer.ik[3].pos += Vec3::new(0.0, 0.5, 0.0);
        chara.pose_frame(None, &[], Some(&layer), &Default::default());
        assert!(world(&chara, hand).y > moved.y + 0.1);
    }
}
