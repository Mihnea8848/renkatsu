// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The game's FullBodyBipedIK component as the body's prefab stores it
//! (`p_cf_body_bone` in `chara/oo_base`): the bones it works on and every
//! setting of its solver, chains, effectors and mappings, by the names
//! FinalIK gives them. Read, never guessed; what FinalIK works out when it
//! starts (lengths, default rotations) is not stored and is left out.
//!
//! CharaStudio changes some of these when it adds a character: its IK
//! targets become the effectors' targets and the limbs' bend goals, the
//! bend constraints get a weight of 1 (`AddObjectAssist.InitIKTarget`),
//! and switching an IK group on sets its effectors' weights to 1
//! (`OCIChar.ActiveIKGroup`). KKPE's Optimize IK, on by default for every
//! character, sets `spineStiffness` and `pullBodyVertical` to 0
//! (`CharaPoseController.Start`); the captures were taken with it on.

use crate::scene::Scene;
use glam::{Quat, Vec3};
use rk_unityfs::{PPtr, UObject, UValue};

/// `FBIKChain.Smoothing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Smoothing {
    None,
    Exponential,
    Cubic,
}

impl Smoothing {
    fn of(n: i64) -> Smoothing {
        match n {
            1 => Smoothing::Exponential,
            2 => Smoothing::Cubic,
            _ => Smoothing::None,
        }
    }
}

/// `BipedReferences`: scene nodes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct References {
    pub root: Option<usize>,
    pub pelvis: Option<usize>,
    pub left_thigh: Option<usize>,
    pub left_calf: Option<usize>,
    pub left_foot: Option<usize>,
    pub right_thigh: Option<usize>,
    pub right_calf: Option<usize>,
    pub right_foot: Option<usize>,
    pub left_upper_arm: Option<usize>,
    pub left_forearm: Option<usize>,
    pub left_hand: Option<usize>,
    pub right_upper_arm: Option<usize>,
    pub right_forearm: Option<usize>,
    pub right_hand: Option<usize>,
    pub head: Option<usize>,
    pub spine: Vec<Option<usize>>,
    pub eyes: Vec<Option<usize>>,
}

/// `IKConstraintBend`.
#[derive(Debug, Clone, PartialEq)]
pub struct Bend {
    pub bone1: Option<usize>,
    pub bone2: Option<usize>,
    pub bone3: Option<usize>,
    pub bend_goal: Option<usize>,
    pub direction: Vec3,
    pub rotation_offset: Quat,
    pub weight: f32,
}

/// `FBIKChain.ChildConstraint`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChildConstraint {
    pub bone1: Option<usize>,
    pub bone2: Option<usize>,
    pub push_elasticity: f32,
    pub pull_elasticity: f32,
}

/// `IKSolver.Node` as stored: its bone and weight.
#[derive(Debug, Clone, PartialEq)]
pub struct ChainNode {
    pub transform: Option<usize>,
    pub weight: f32,
}

/// `FBIKChain`.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub pin: f32,
    pub pull: f32,
    pub push: f32,
    pub push_parent: f32,
    pub reach: f32,
    pub reach_smoothing: Smoothing,
    pub push_smoothing: Smoothing,
    pub nodes: Vec<ChainNode>,
    pub children: Vec<i32>,
    pub child_constraints: Vec<ChildConstraint>,
    pub bend_constraint: Bend,
}

/// `IKEffector`.
#[derive(Debug, Clone, PartialEq)]
pub struct Effector {
    pub bone: Option<usize>,
    pub target: Option<usize>,
    pub position_weight: f32,
    pub rotation_weight: f32,
    pub position: Vec3,
    pub rotation: Quat,
    pub position_offset: Vec3,
    pub effect_child_nodes: bool,
    pub maintain_relative_position_weight: f32,
    pub child_bones: Vec<Option<usize>>,
    pub plane_bone1: Option<usize>,
    pub plane_bone2: Option<usize>,
    pub plane_bone3: Option<usize>,
    pub plane_rotation_offset: Quat,
}

/// `IKMappingSpine`.
#[derive(Debug, Clone, PartialEq)]
pub struct SpineMapping {
    pub spine_bones: Vec<Option<usize>>,
    pub left_upper_arm_bone: Option<usize>,
    pub right_upper_arm_bone: Option<usize>,
    pub left_thigh_bone: Option<usize>,
    pub right_thigh_bone: Option<usize>,
    pub iterations: i32,
    pub twist_weight: f32,
}

/// `IKMappingBone`.
#[derive(Debug, Clone, PartialEq)]
pub struct BoneMapping {
    pub bone: Option<usize>,
    pub maintain_rotation_weight: f32,
}

/// `IKMappingLimb`.
#[derive(Debug, Clone, PartialEq)]
pub struct LimbMapping {
    pub parent_bone: Option<usize>,
    pub bone1: Option<usize>,
    pub bone2: Option<usize>,
    pub bone3: Option<usize>,
    pub maintain_rotation_weight: f32,
    pub weight: f32,
}

/// `FullBodyBipedIK` with its `IKSolverFullBodyBiped`.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub enabled: bool,
    pub fix_transforms: bool,
    pub references: References,
    pub root: Option<usize>,
    pub root_node: Option<usize>,
    pub ik_position_weight: f32,
    pub iterations: i32,
    pub spine_stiffness: f32,
    pub pull_body_vertical: f32,
    pub pull_body_horizontal: f32,
    pub chains: Vec<Chain>,
    pub effectors: Vec<Effector>,
    pub spine_mapping: SpineMapping,
    pub bone_mappings: Vec<BoneMapping>,
    pub limb_mappings: Vec<LimbMapping>,
}

/// Reads the component; `node` gives the scene node of a Transform it
/// refers to.
pub fn read(obj: &UObject, node: &dyn Fn(PPtr) -> Option<usize>) -> Settings {
    let r = Read { node };
    let solver = obj.get("solver").and_then(UValue::as_object);
    let empty = UObject {
        ty: "".into(),
        fields: Vec::new(),
    };
    let s = solver.unwrap_or(&empty);
    let refs = obj.get("references").and_then(UValue::as_object).unwrap_or(&empty);
    let spine = s.get("spineMapping").and_then(UValue::as_object).unwrap_or(&empty);
    Settings {
        enabled: obj.i64("m_Enabled").unwrap_or(1) != 0,
        fix_transforms: obj.i64("fixTransforms").unwrap_or(1) != 0,
        references: References {
            root: r.bone(refs, "root"),
            pelvis: r.bone(refs, "pelvis"),
            left_thigh: r.bone(refs, "leftThigh"),
            left_calf: r.bone(refs, "leftCalf"),
            left_foot: r.bone(refs, "leftFoot"),
            right_thigh: r.bone(refs, "rightThigh"),
            right_calf: r.bone(refs, "rightCalf"),
            right_foot: r.bone(refs, "rightFoot"),
            left_upper_arm: r.bone(refs, "leftUpperArm"),
            left_forearm: r.bone(refs, "leftForearm"),
            left_hand: r.bone(refs, "leftHand"),
            right_upper_arm: r.bone(refs, "rightUpperArm"),
            right_forearm: r.bone(refs, "rightForearm"),
            right_hand: r.bone(refs, "rightHand"),
            head: r.bone(refs, "head"),
            spine: r.bones(refs, "spine"),
            eyes: r.bones(refs, "eyes"),
        },
        root: r.bone(s, "root"),
        root_node: r.bone(s, "rootNode"),
        ik_position_weight: s.f32("IKPositionWeight").unwrap_or(1.0),
        iterations: int(s, "iterations"),
        spine_stiffness: s.f32("spineStiffness").unwrap_or(0.0),
        pull_body_vertical: s.f32("pullBodyVertical").unwrap_or(0.0),
        pull_body_horizontal: s.f32("pullBodyHorizontal").unwrap_or(0.0),
        chains: objects(s, "chain").map(|c| r.chain(c)).collect(),
        effectors: objects(s, "effectors").map(|e| r.effector(e)).collect(),
        spine_mapping: SpineMapping {
            spine_bones: r.bones(spine, "spineBones"),
            left_upper_arm_bone: r.bone(spine, "leftUpperArmBone"),
            right_upper_arm_bone: r.bone(spine, "rightUpperArmBone"),
            left_thigh_bone: r.bone(spine, "leftThighBone"),
            right_thigh_bone: r.bone(spine, "rightThighBone"),
            iterations: int(spine, "iterations"),
            twist_weight: spine.f32("twistWeight").unwrap_or(0.0),
        },
        bone_mappings: objects(s, "boneMappings")
            .map(|m| BoneMapping {
                bone: r.bone(m, "bone"),
                maintain_rotation_weight: m.f32("maintainRotationWeight").unwrap_or(0.0),
            })
            .collect(),
        limb_mappings: objects(s, "limbMappings")
            .map(|m| LimbMapping {
                parent_bone: r.bone(m, "parentBone"),
                bone1: r.bone(m, "bone1"),
                bone2: r.bone(m, "bone2"),
                bone3: r.bone(m, "bone3"),
                maintain_rotation_weight: m.f32("maintainRotationWeight").unwrap_or(0.0),
                weight: m.f32("weight").unwrap_or(0.0),
            })
            .collect(),
    }
}

struct Read<'a> {
    node: &'a dyn Fn(PPtr) -> Option<usize>,
}

impl Read<'_> {
    fn bone(&self, o: &UObject, name: &str) -> Option<usize> {
        o.pptr(name).filter(|p| !p.is_null()).and_then(self.node)
    }

    fn bones(&self, o: &UObject, name: &str) -> Vec<Option<usize>> {
        o.array(name)
            .unwrap_or_default()
            .iter()
            .map(|v| v.as_pptr().filter(|p| !p.is_null()).and_then(self.node))
            .collect()
    }

    fn chain(&self, c: &UObject) -> Chain {
        let bend = c.get("bendConstraint").and_then(UValue::as_object);
        let b = |name: &str| bend.and_then(|o| self.bone(o, name));
        Chain {
            pin: c.f32("pin").unwrap_or(0.0),
            pull: c.f32("pull").unwrap_or(1.0),
            push: c.f32("push").unwrap_or(0.0),
            push_parent: c.f32("pushParent").unwrap_or(0.0),
            reach: c.f32("reach").unwrap_or(0.0),
            reach_smoothing: Smoothing::of(c.i64("reachSmoothing").unwrap_or(0)),
            push_smoothing: Smoothing::of(c.i64("pushSmoothing").unwrap_or(0)),
            nodes: objects(c, "nodes")
                .map(|n| ChainNode {
                    transform: self.bone(n, "transform"),
                    weight: n.f32("weight").unwrap_or(1.0),
                })
                .collect(),
            children: c
                .array("children")
                .unwrap_or_default()
                .iter()
                .filter_map(|v| v.as_i64().map(|i| i as i32))
                .collect(),
            child_constraints: objects(c, "childConstraints")
                .map(|k| ChildConstraint {
                    bone1: self.bone(k, "bone1"),
                    bone2: self.bone(k, "bone2"),
                    push_elasticity: k.f32("pushElasticity").unwrap_or(0.0),
                    pull_elasticity: k.f32("pullElasticity").unwrap_or(0.0),
                })
                .collect(),
            bend_constraint: Bend {
                bone1: b("bone1"),
                bone2: b("bone2"),
                bone3: b("bone3"),
                bend_goal: b("bendGoal"),
                direction: bend.map_or(Vec3::X, |o| vec3(o, "direction")),
                rotation_offset: bend.map_or(Quat::IDENTITY, |o| quat(o, "rotationOffset")),
                weight: bend.and_then(|o| o.f32("weight")).unwrap_or(0.0),
            },
        }
    }

    fn effector(&self, e: &UObject) -> Effector {
        Effector {
            bone: self.bone(e, "bone"),
            target: self.bone(e, "target"),
            position_weight: e.f32("positionWeight").unwrap_or(0.0),
            rotation_weight: e.f32("rotationWeight").unwrap_or(0.0),
            position: vec3(e, "position"),
            rotation: quat(e, "rotation"),
            position_offset: vec3(e, "positionOffset"),
            effect_child_nodes: e.i64("effectChildNodes").unwrap_or(1) != 0,
            maintain_relative_position_weight: e.f32("maintainRelativePositionWeight").unwrap_or(0.0),
            child_bones: self.bones(e, "childBones"),
            plane_bone1: self.bone(e, "planeBone1"),
            plane_bone2: self.bone(e, "planeBone2"),
            plane_bone3: self.bone(e, "planeBone3"),
            plane_rotation_offset: quat(e, "planeRotationOffset"),
        }
    }
}

fn objects<'o>(o: &'o UObject, name: &str) -> impl Iterator<Item = &'o UObject> {
    o.array(name).unwrap_or_default().iter().filter_map(UValue::as_object)
}

fn int(o: &UObject, name: &str) -> i32 {
    o.i64(name).unwrap_or(0) as i32
}

fn vec3(o: &UObject, name: &str) -> Vec3 {
    let v = o.get(name).and_then(UValue::as_object);
    let c = |k: &str| v.and_then(|v| v.f32(k)).unwrap_or(0.0);
    Vec3::new(c("x"), c("y"), c("z"))
}

/// A stored rotation; an all-zero one (never set) is the identity.
fn quat(o: &UObject, name: &str) -> Quat {
    let v = o.get(name).and_then(UValue::as_object);
    let c = |k: &str| v.and_then(|v| v.f32(k)).unwrap_or(0.0);
    let q = Quat::from_xyzw(c("x"), c("y"), c("z"), c("w"));
    if q.length_squared() == 0.0 {
        Quat::IDENTITY
    } else {
        q
    }
}

/// The settings as text, bones by name, for `rk ik settings`.
pub fn describe(s: &Settings, scene: &Scene) -> String {
    use std::fmt::Write;
    let name = |n: Option<usize>| n.map_or("-".to_owned(), |n| scene.nodes[n].name.clone());
    let names = |v: &[Option<usize>]| v.iter().map(|n| name(*n)).collect::<Vec<_>>().join(", ");
    let mut o = String::new();
    let r = &s.references;
    let _ = writeln!(
        o,
        "FullBodyBipedIK (enabled {}, fixTransforms {})",
        s.enabled, s.fix_transforms
    );
    let _ = writeln!(
        o,
        "references: root {}, pelvis {}, head {}",
        name(r.root),
        name(r.pelvis),
        name(r.head)
    );
    let _ = writeln!(
        o,
        "  left leg {} {} {}, right leg {} {} {}",
        name(r.left_thigh),
        name(r.left_calf),
        name(r.left_foot),
        name(r.right_thigh),
        name(r.right_calf),
        name(r.right_foot)
    );
    let _ = writeln!(
        o,
        "  left arm {} {} {}, right arm {} {} {}",
        name(r.left_upper_arm),
        name(r.left_forearm),
        name(r.left_hand),
        name(r.right_upper_arm),
        name(r.right_forearm),
        name(r.right_hand)
    );
    let _ = writeln!(o, "  spine [{}], eyes [{}]", names(&r.spine), names(&r.eyes));
    let _ = writeln!(
        o,
        "solver: root {}, rootNode {}, IKPositionWeight {}, iterations {}, spineStiffness {}, pullBodyVertical {}, pullBodyHorizontal {}",
        name(s.root),
        name(s.root_node),
        s.ik_position_weight,
        s.iterations,
        s.spine_stiffness,
        s.pull_body_vertical,
        s.pull_body_horizontal
    );
    for (i, c) in s.chains.iter().enumerate() {
        let nodes: Vec<String> = c
            .nodes
            .iter()
            .map(|n| format!("{} ({})", name(n.transform), n.weight))
            .collect();
        let _ = writeln!(
            o,
            "chain {i}: [{}] children {:?}, pin {}, pull {}, push {}, pushParent {}, reach {}, reachSmoothing {:?}, pushSmoothing {:?}",
            nodes.join(", "),
            c.children,
            c.pin,
            c.pull,
            c.push,
            c.push_parent,
            c.reach,
            c.reach_smoothing,
            c.push_smoothing
        );
        for k in &c.child_constraints {
            let _ = writeln!(
                o,
                "  child constraint {} - {}: push {}, pull {}",
                name(k.bone1),
                name(k.bone2),
                k.push_elasticity,
                k.pull_elasticity
            );
        }
        let b = &c.bend_constraint;
        if b.bone1.is_some() {
            let _ = writeln!(
                o,
                "  bend {} {} {}, goal {}, weight {}, direction {:?}",
                name(b.bone1),
                name(b.bone2),
                name(b.bone3),
                name(b.bend_goal),
                b.weight,
                b.direction.to_array()
            );
        }
    }
    for (i, e) in s.effectors.iter().enumerate() {
        let _ = writeln!(
            o,
            "effector {i}: {} target {}, weights {} {}, effectChildNodes {}, maintainRelativePositionWeight {}, childBones [{}], plane {} {} {}",
            name(e.bone),
            name(e.target),
            e.position_weight,
            e.rotation_weight,
            e.effect_child_nodes,
            e.maintain_relative_position_weight,
            names(&e.child_bones),
            name(e.plane_bone1),
            name(e.plane_bone2),
            name(e.plane_bone3)
        );
    }
    let m = &s.spine_mapping;
    let _ = writeln!(
        o,
        "spine mapping: [{}], arms {} {}, thighs {} {}, iterations {}, twistWeight {}",
        names(&m.spine_bones),
        name(m.left_upper_arm_bone),
        name(m.right_upper_arm_bone),
        name(m.left_thigh_bone),
        name(m.right_thigh_bone),
        m.iterations,
        m.twist_weight
    );
    for b in &s.bone_mappings {
        let _ = writeln!(
            o,
            "bone mapping: {}, maintainRotationWeight {}",
            name(b.bone),
            b.maintain_rotation_weight
        );
    }
    for l in &s.limb_mappings {
        let _ = writeln!(
            o,
            "limb mapping: {} > {} {} {}, maintainRotationWeight {}, weight {}",
            name(l.parent_bone),
            name(l.bone1),
            name(l.bone2),
            name(l.bone3),
            l.maintain_rotation_weight,
            l.weight
        );
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn obj(fields: Vec<(&str, UValue)>) -> UValue {
        UValue::Object(Box::new(UObject {
            ty: "".into(),
            fields: fields.into_iter().map(|(k, v)| (Arc::from(k), v)).collect(),
        }))
    }

    fn pptr(id: i64) -> UValue {
        obj(vec![("m_FileID", UValue::Int(0)), ("m_PathID", UValue::Int(id))])
    }

    #[test]
    fn the_component_is_read_with_its_bones_as_nodes() {
        let quat0 = obj(vec![
            ("x", UValue::Float(0.0)),
            ("y", UValue::Float(0.0)),
            ("z", UValue::Float(0.0)),
            ("w", UValue::Float(0.0)),
        ]);
        let component = obj(vec![
            ("fixTransforms", UValue::Int(1)),
            ("m_Enabled", UValue::Int(0)),
            (
                "references",
                obj(vec![
                    ("pelvis", pptr(7)),
                    ("head", pptr(0)),
                    ("spine", UValue::Array(vec![pptr(2), pptr(3)])),
                ]),
            ),
            (
                "solver",
                obj(vec![
                    ("iterations", UValue::Int(4)),
                    ("spineStiffness", UValue::Float(0.5)),
                    (
                        "chain",
                        UValue::Array(vec![obj(vec![
                            ("reach", UValue::Float(0.1)),
                            ("reachSmoothing", UValue::Int(1)),
                            ("pushSmoothing", UValue::Int(2)),
                            ("children", UValue::Array(vec![UValue::Int(1), UValue::Int(2)])),
                            (
                                "nodes",
                                UValue::Array(vec![obj(vec![("transform", pptr(4)), ("weight", UValue::Float(1.0))])]),
                            ),
                            (
                                "bendConstraint",
                                obj(vec![("weight", UValue::Float(1.0)), ("rotationOffset", quat0.clone())]),
                            ),
                        ])]),
                    ),
                    (
                        "effectors",
                        UValue::Array(vec![obj(vec![
                            ("bone", pptr(5)),
                            ("target", pptr(99)),
                            ("rotation", quat0),
                            ("effectChildNodes", UValue::Int(0)),
                        ])]),
                    ),
                ]),
            ),
        ]);
        let UValue::Object(component) = component else {
            unreachable!()
        };
        // Path ids below 10 are bones of the prefab; 99 is not.
        let node = |p: PPtr| (p.path_id < 10).then_some(p.path_id as usize * 10);
        let s = read(&component, &node);
        assert!(!s.enabled && s.fix_transforms);
        assert_eq!(s.references.pelvis, Some(70));
        assert_eq!(s.references.head, None);
        assert_eq!(s.references.spine, vec![Some(20), Some(30)]);
        assert_eq!((s.iterations, s.spine_stiffness), (4, 0.5));
        let c = &s.chains[0];
        assert_eq!(
            (c.reach_smoothing, c.push_smoothing),
            (Smoothing::Exponential, Smoothing::Cubic)
        );
        assert_eq!(c.children, vec![1, 2]);
        assert_eq!(c.nodes[0].transform, Some(40));
        assert_eq!(c.bend_constraint.rotation_offset, Quat::IDENTITY);
        let e = &s.effectors[0];
        assert_eq!((e.bone, e.target), (Some(50), None));
        assert_eq!(e.rotation, Quat::IDENTITY);
        assert!(!e.effect_child_nodes);
    }
}
