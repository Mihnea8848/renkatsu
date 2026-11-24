// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The port of FinalIK's full body solver against CharaStudio: each
//! `*.ik.json` under `tests/fixtures/studio-2026-11-24` holds one solver
//! update of the game (the capture plugin's IkWatch): every bone as the
//! solver was about to read it, the solver's settings, and every bone as
//! it left them. The same update is run here and compared.
//!
//! What the capture does not hold and where it comes from here:
//! - The child constraints of the body chain (`childConstraints`): the
//!   game's serialized component, as `rk ik settings` prints it (which is
//!   also what FinalIK's `SetToReferences` sets).
//! - The mappings' bone maps (`boneMapParent.localSwingAxis` is the one
//!   only worked out at initiation): from `initiate` on the captured pose.
//!   It is the upper arm's place in the shoulder's space, which a pose
//!   does not change.
//! - `root.up`: not needed, body pulling being off in every case.
//!
//! Studio initiates the solver when the character is added to the scene
//! (`AddObjectFemale.Add`), in the pose the body has then, not in the
//! animation's. What it worked out there for the bend constraints
//! (`defaultLocalDirection`, `defaultChildDirection`) is in the capture and
//! is used; that run is held to the limits. A second run uses what
//! `initiate` works out from the captured pose instead and is only
//! reported: it is a different pose, so it need not agree.
//!
//! `solved` holds one point only (`GetPoints` of this FinalIK fills only
//! its first slot, with the last node). The nodes in `settings` are from
//! the update before, in a scene that stands still; they are compared too
//! and told apart in the report.

use glam::{Quat, Vec3};
use rk_ik::fbbik::{
    ChildConstraint, FbikChain, IkEffector, IkMappingBone, IkMappingLimb, IkMappingSpine, IkSolverFullBodyBiped, Place,
    Smoothing,
};
use rk_ik::hierarchy::{Hierarchy, Transform};
use serde_json::Value;
use std::collections::HashMap;

/// The limits a pose has to hold: 0.1 mm and 0.05 degrees per bone.
const LIMIT_MM: f64 = 0.1;
const LIMIT_DEG: f64 = 0.05;

fn f(v: &Value) -> f32 {
    v.as_f64().unwrap_or_else(|| panic!("a number, not {v}")) as f32
}

fn v3(v: &Value) -> Vec3 {
    Vec3::new(f(&v[0]), f(&v[1]), f(&v[2]))
}

fn q4(v: &Value) -> Quat {
    Quat::from_xyzw(f(&v[0]), f(&v[1]), f(&v[2]), f(&v[3]))
}

fn list(v: &Value) -> &Vec<Value> {
    v.as_array().unwrap_or_else(|| panic!("a list, not {v}"))
}

/// Distance in millimetres.
fn mm(a: Vec3, b: Vec3) -> f64 {
    f64::from((a - b).length()) * 1000.0
}

/// Angle between two rotations in degrees, exact for small angles.
fn degrees(a: Quat, b: Quat) -> f64 {
    let d = (a.conjugate() * b).as_dquat();
    let vector = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
    (2.0 * vector.atan2(d.w.abs())).to_degrees()
}

struct Case {
    name: String,
    json: Value,
    names: HashMap<String, usize>,
}

impl Case {
    fn bone(&self, name: &Value) -> usize {
        let name = name.as_str().unwrap_or_else(|| panic!("a bone name, not {name}"));
        *self
            .names
            .get(name)
            .unwrap_or_else(|| panic!("{}: no bone {name}", self.name))
    }

    fn maybe_bone(&self, name: &Value) -> Option<usize> {
        (!name.is_null()).then(|| self.bone(name))
    }

    /// The bones as the solver was about to read them.
    fn hierarchy(&self) -> Hierarchy {
        Hierarchy::new(
            list(&self.json["read"])
                .iter()
                .map(|b| Transform {
                    parent: usize::try_from(b["parent"].as_i64().expect("a parent index")).ok(),
                    local_position: v3(&b["t"]),
                    local_rotation: q4(&b["r"]),
                    local_scale: v3(&b["s"]),
                })
                .collect(),
        )
    }

    /// The solver with the captured settings, not initiated.
    fn solver(&self) -> IkSolverFullBodyBiped {
        let set = &self.json["settings"];
        let smoothing = |v: &Value| match v.as_str() {
            Some("None") => Smoothing::None,
            Some("Exponential") => Smoothing::Exponential,
            Some("Cubic") => Smoothing::Cubic,
            _ => panic!("a smoothing, not {v}"),
        };
        let mut chain: Vec<FbikChain> = list(&set["chains"])
            .iter()
            .map(|c| {
                let k = &c["chain"];
                let mut chain = FbikChain {
                    pin: f(&k["pin"]),
                    pull: f(&k["pull"]),
                    push: f(&k["push"]),
                    push_parent: f(&k["pushParent"]),
                    reach: f(&k["reach"]),
                    reach_smoothing: smoothing(&k["reachSmoothing"]),
                    push_smoothing: smoothing(&k["pushSmoothing"]),
                    children: list(&k["children"])
                        .iter()
                        .map(|i| i.as_u64().unwrap() as usize)
                        .collect(),
                    ..FbikChain::default()
                };
                let nodes: Vec<usize> = list(&c["nodes"]).iter().map(|n| self.bone(&n["transform"])).collect();
                chain.set_nodes(&nodes);
                chain.bend_constraint.weight = f(&c["bend"]["weight"]);
                chain.bend_constraint.clamp_f = f(&c["bend"]["clampF"]);
                chain.bend_constraint.bend_goal = (!c["bend_goal"].is_null()).then(|| v3(&c["bend_goal"]["pos"]));
                chain
            })
            .collect();
        // Not in the capture: the game's serialized values.
        let first = |i: usize| chain[i].nodes[0].transform;
        chain[0].child_constraints = vec![
            ChildConstraint::new(first(1), first(4), 0.0, 1.0),
            ChildConstraint::new(first(2), first(3), 0.0, 1.0),
            ChildConstraint::new(first(1), first(2), 0.0, 0.0),
            ChildConstraint::new(first(3), first(4), 0.0, 0.0),
        ];
        let effectors = list(&set["effectors"])
            .iter()
            .map(|e| {
                let k = &e["effector"];
                let mut effector = IkEffector::new(self.bone(&k["bone"]));
                effector.target = (!e["target_place"].is_null()).then(|| Place {
                    position: v3(&e["target_place"]["pos"]),
                    rotation: q4(&e["target_place"]["rot"]),
                });
                effector.position_weight = f(&k["positionWeight"]);
                effector.rotation_weight = f(&k["rotationWeight"]);
                effector.position = v3(&k["position"]);
                effector.rotation = q4(&k["rotation"]);
                effector.position_offset = v3(&k["positionOffset"]);
                effector.effect_child_nodes = k["effectChildNodes"].as_bool().unwrap();
                effector.maintain_relative_position_weight = f(&k["maintainRelativePositionWeight"]);
                effector.child_bones = list(&k["childBones"]).iter().map(|b| self.bone(b)).collect();
                effector.plane_bone1 = self.maybe_bone(&k["planeBone1"]);
                effector.plane_bone2 = self.maybe_bone(&k["planeBone2"]);
                effector.plane_bone3 = self.maybe_bone(&k["planeBone3"]);
                effector
            })
            .collect();
        let s = &set["spine_mapping"];
        let mut spine_mapping = IkMappingSpine::new(
            list(&s["spineBones"]).iter().map(|b| self.bone(b)).collect(),
            self.bone(&s["leftUpperArmBone"]),
            self.bone(&s["rightUpperArmBone"]),
            self.bone(&s["leftThighBone"]),
            self.bone(&s["rightThighBone"]),
        );
        spine_mapping.iterations = s["iterations"].as_i64().unwrap() as i32;
        spine_mapping.twist_weight = f(&s["twistWeight"]);
        let bone_mappings = list(&set["bone_mappings"])
            .iter()
            .map(|m| {
                let mut mapping = IkMappingBone::new(self.bone(&m["bone"]));
                mapping.maintain_rotation_weight = f(&m["maintainRotationWeight"]);
                mapping
            })
            .collect();
        let limb_mappings = list(&set["limb_mappings"])
            .iter()
            .map(|m| {
                let mut mapping = IkMappingLimb::new(
                    self.bone(&m["bone1"]),
                    self.bone(&m["bone2"]),
                    self.bone(&m["bone3"]),
                    self.maybe_bone(&m["parentBone"]),
                );
                mapping.maintain_rotation_weight = f(&m["maintainRotationWeight"]);
                mapping.weight = f(&m["weight"]);
                mapping
            })
            .collect();
        let k = &set["solver"];
        // `root.up` is not in the capture and only body pulling reads it.
        assert_eq!(
            f(&k["pullBodyVertical"]),
            0.0,
            "{}: body pulling needs root.up",
            self.name
        );
        assert_eq!(
            f(&k["pullBodyHorizontal"]),
            0.0,
            "{}: body pulling needs root.up",
            self.name
        );
        IkSolverFullBodyBiped {
            ik_position: v3(&k["IKPosition"]),
            ik_position_weight: f(&k["IKPositionWeight"]),
            first_initiation: true,
            initiated: false,
            root_up: Vec3::Y,
            iterations: k["iterations"].as_i64().unwrap() as i32,
            chain,
            effectors,
            spine_mapping,
            bone_mappings,
            limb_mappings,
            root_node: self.bone(&k["rootNode"]),
            spine_stiffness: f(&k["spineStiffness"]),
            pull_body_vertical: f(&k["pullBodyVertical"]),
            pull_body_horizontal: f(&k["pullBodyHorizontal"]),
            offset: Vec3::ZERO,
        }
    }
}

/// The largest difference of a kind and where it is.
#[derive(Default, Clone)]
struct Worst {
    value: f64,
    at: String,
}

impl Worst {
    fn see(&mut self, value: f64, at: &str) {
        if value > self.value {
            self.value = value;
            self.at = at.to_string();
        }
    }
}

/// How far `initiate` on the captured pose is from what the game's solver
/// held: the bend constraints' directions as angles, the rest checked to
/// be the same.
fn compare_initiation(case: &Case, solver: &IkSolverFullBodyBiped) -> (Worst, Worst) {
    let set = &case.json["settings"];
    let (mut local, mut child) = (Worst::default(), Worst::default());
    let angle = |a: Vec3, b: Vec3| f64::from(a.angle_between(b)).to_degrees();
    for (i, c) in list(&set["chains"]).iter().enumerate() {
        let (k, b) = (&c["chain"], &c["bend"]);
        let mine = &solver.chain[i];
        assert_eq!(
            mine.bend_constraint.initiated,
            b["<initiated>k__BackingField"].as_bool().unwrap()
        );
        if !mine.bend_constraint.initiated {
            continue;
        }
        let bend = &mine.bend_constraint;
        let index = |key: &str| b[key].as_i64().unwrap() as i32;
        assert_eq!(
            [
                bend.chain_index1,
                bend.node_index1,
                bend.chain_index2,
                bend.node_index2,
                bend.chain_index3,
                bend.node_index3
            ],
            [
                "chainIndex1",
                "nodeIndex1",
                "chainIndex2",
                "nodeIndex2",
                "chainIndex3",
                "nodeIndex3"
            ]
            .map(index)
        );
        local.see(
            angle(bend.default_local_direction, v3(&b["defaultLocalDirection"])),
            &format!("chain {i}"),
        );
        child.see(
            angle(bend.default_child_direction, v3(&b["defaultChildDirection"])),
            &format!("chain {i}"),
        );
        // Lengths do not depend on the pose.
        assert!(
            (mine.length - f(&k["length"])).abs() < 1e-6,
            "{} chain {i} length",
            case.name
        );
        assert!(
            (mine.root_length - f(&k["rootLength"])).abs() < 1e-6,
            "{} chain {i} rootLength",
            case.name
        );
    }
    for (i, e) in list(&set["effectors"]).iter().enumerate() {
        let (k, mine) = (&e["effector"], &solver.effectors[i]);
        let index = |key: &str| k[key].as_i64().unwrap() as i32;
        assert_eq!(
            [
                mine.chain_index,
                mine.node_index,
                mine.plane1_chain_index,
                mine.plane1_node_index,
                mine.plane2_chain_index,
                mine.plane2_node_index,
                mine.plane3_chain_index,
                mine.plane3_node_index
            ],
            [
                "chainIndex",
                "nodeIndex",
                "plane1ChainIndex",
                "plane1NodeIndex",
                "plane2ChainIndex",
                "plane2NodeIndex",
                "plane3ChainIndex",
                "plane3NodeIndex"
            ]
            .map(index),
            "{} effector {i}",
            case.name
        );
        assert_eq!(
            mine.is_end_effector,
            k["<isEndEffector>k__BackingField"].as_bool().unwrap()
        );
        assert_eq!(mine.use_plane_nodes, k["usePlaneNodes"].as_bool().unwrap());
        let children: Vec<i32> = list(&k["childChainIndexes"])
            .iter()
            .map(|n| n.as_i64().unwrap() as i32)
            .collect();
        assert_eq!(mine.child_chain_indexes, children);
    }
    let s = &set["spine_mapping"];
    assert_eq!(
        i64::from(solver.spine_mapping.root_node_index),
        s["rootNodeIndex"].as_i64().unwrap()
    );
    assert_eq!(solver.spine_mapping.use_fabrik, s["useFABRIK"].as_bool().unwrap());
    (local, child)
}

/// Puts the game's own initiation values on the bend constraints.
fn take_captured_initiation(case: &Case, solver: &mut IkSolverFullBodyBiped) {
    for (i, c) in list(&case.json["settings"]["chains"]).iter().enumerate() {
        let bend = &mut solver.chain[i].bend_constraint;
        if bend.initiated {
            bend.default_local_direction = v3(&c["bend"]["defaultLocalDirection"]);
            bend.default_child_direction = v3(&c["bend"]["defaultChildDirection"]);
            bend.direction = v3(&c["bend"]["direction"]);
        }
    }
}

#[derive(Default, Clone)]
struct Result {
    solved_mm: Worst,
    previous_mm: Worst,
    values: Worst,
    written_mm: Worst,
    written_deg: Worst,
    moved: usize,
    moved_mm: Worst,
}

/// One update of the solver on the captured pose, against the capture.
fn run(case: &Case, own_initiation: bool) -> Result {
    let mut h = case.hierarchy();
    let mut solver = case.solver();
    solver.initiate(&h).unwrap_or_else(|e| panic!("{}: {e}", case.name));
    if !own_initiation {
        take_captured_initiation(case, &mut solver);
    }
    solver.update(&mut h).unwrap_or_else(|e| panic!("{}: {e}", case.name));

    let mut result = Result::default();
    let set = &case.json["settings"];
    // (a) The solver's points.
    for point in list(&case.json["solved"]).iter().filter(|p| !p.is_null()) {
        let bone = case.bone(&point["transform"]);
        let node = solver
            .chain
            .iter()
            .flat_map(|c| &c.nodes)
            .find(|n| n.transform == bone)
            .expect("a solved point is a node");
        let at = point["transform"].as_str().unwrap();
        result
            .solved_mm
            .see(mm(node.solver_position, v3(&point["solverPosition"])), at);
        result.values.see(
            degrees(node.solver_rotation, q4(&point["solverRotation"])),
            &format!("{at} solverRotation (degrees)"),
        );
    }
    // The nodes and worked out values of the update before.
    for (i, c) in list(&set["chains"]).iter().enumerate() {
        let mine = &solver.chain[i];
        for (j, n) in list(&c["nodes"]).iter().enumerate() {
            let at = n["transform"].as_str().unwrap();
            result
                .previous_mm
                .see(mm(mine.nodes[j].solver_position, v3(&n["solverPosition"])), at);
            result.values.see(
                f64::from((mine.nodes[j].length - f(&n["length"])).abs()) * 1000.0,
                &format!("{at} length (mm)"),
            );
        }
        let k = &c["chain"];
        for (key, value) in [
            ("length", mine.length),
            ("rootLength", mine.root_length),
            ("reachForce", mine.reach_force),
            ("pullParentSum", mine.pull_parent_sum),
            ("sqrMag1", mine.sqr_mag1),
            ("sqrMag2", mine.sqr_mag2),
            ("sqrMagDif", mine.sqr_mag_dif),
        ] {
            result
                .values
                .see(f64::from((value - f(&k[key])).abs()), &format!("chain {i} {key}"));
        }
        for (l, fade) in list(&k["crossFades"]).iter().enumerate() {
            result.values.see(
                f64::from((mine.cross_fades[l] - f(fade)).abs()),
                &format!("chain {i} crossFades"),
            );
        }
        if mine.bend_constraint.initiated {
            let direction = v3(&c["bend"]["direction"]);
            result.values.see(
                f64::from((mine.bend_constraint.direction - direction).length()),
                &format!("chain {i} bend direction"),
            );
        }
    }
    for (i, e) in list(&set["effectors"]).iter().enumerate() {
        let (k, mine) = (&e["effector"], &solver.effectors[i]);
        result.values.see(
            f64::from((mine.animated_position - v3(&k["animatedPosition"])).length()),
            &format!("effector {i} animatedPosition"),
        );
        result.values.see(
            f64::from((mine.pos_w - f(&k["posW"])).abs()),
            &format!("effector {i} posW"),
        );
        result.values.see(
            f64::from((mine.rot_w - f(&k["rotW"])).abs()),
            &format!("effector {i} rotW"),
        );
        for (l, p) in list(&k["localPositions"]).iter().enumerate() {
            result.values.see(
                f64::from((mine.local_positions[l] - v3(p)).length()),
                &format!("effector {i} localPositions"),
            );
        }
    }
    // (b) Every bone as the solver left it.
    let read = list(&case.json["read"]);
    for (i, b) in list(&case.json["written"]).iter().enumerate() {
        let at = b["name"].as_str().unwrap();
        result.written_mm.see(mm(h.position(i), v3(&b["pos"])), at);
        result.written_deg.see(degrees(h.rotation(i), q4(&b["rot"])), at);
        if b["t"] != read[i]["t"] || b["r"] != read[i]["r"] {
            result.moved += 1;
        }
        result.moved_mm.see(mm(v3(&read[i]["pos"]), v3(&b["pos"])), at);
    }
    result
}

fn cases() -> Vec<Case> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/studio-2026-11-24");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".ik.json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let json: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(
                json["watched"],
                Value::Bool(true),
                "{}: the solver did not run",
                path.display()
            );
            let mut names = HashMap::new();
            for (i, b) in list(&json["read"]).iter().enumerate() {
                names.entry(b["name"].as_str().unwrap().to_string()).or_insert(i);
            }
            let file = path.file_name().unwrap().to_string_lossy();
            // "<card>.<case>.ik.json": the case name is enough to tell them apart.
            let name = file
                .trim_end_matches(".ik.json")
                .rsplit('.')
                .next()
                .unwrap()
                .to_string();
            Case { name, json, names }
        })
        .collect()
}

#[test]
fn the_hierarchy_places_bones_as_unity_does() {
    for case in cases() {
        for key in ["read", "written"] {
            let bones = list(&case.json[key]);
            let h = Hierarchy::new(
                bones
                    .iter()
                    .map(|b| Transform {
                        parent: usize::try_from(b["parent"].as_i64().unwrap()).ok(),
                        local_position: v3(&b["t"]),
                        local_rotation: q4(&b["r"]),
                        local_scale: v3(&b["s"]),
                    })
                    .collect(),
            );
            for (i, b) in bones.iter().enumerate() {
                let at = b["name"].as_str().unwrap();
                assert!(
                    mm(h.position(i), v3(&b["pos"])) < 0.001,
                    "{} {key} {at} position",
                    case.name
                );
                assert!(
                    degrees(h.rotation(i), q4(&b["rot"])) < 0.001,
                    "{} {key} {at} rotation",
                    case.name
                );
            }
        }
    }
}

#[test]
fn one_update_lands_where_charastudio_does() {
    let cases = cases();
    assert!(!cases.is_empty(), "no captured cases");
    let mut failed = Vec::new();
    eprintln!("case, initiation: solved point mm | nodes of the update before mm | written bones mm, degrees");
    for case in &cases {
        let mut probe = case.solver();
        probe.initiate(&case.hierarchy()).unwrap();
        let (local, child) = compare_initiation(case, &probe);
        eprintln!(
            "{:<12} initiate on the captured pose against the game's: defaultLocalDirection {:.4} degrees ({}), defaultChildDirection {:.4} degrees ({})",
            case.name, local.value, local.at, child.value, child.at
        );
        for own in [false, true] {
            let r = run(case, own);
            let which = if own { "own" } else { "captured" };
            eprintln!(
                "{:<12} {which:<8}: {:.5} ({}) | {:.5} ({}) | {:.5} ({}), {:.5} ({}); the game moved {} bones, up to {:.1} mm; other values within {:.2e} ({})",
                case.name,
                r.solved_mm.value,
                r.solved_mm.at,
                r.previous_mm.value,
                r.previous_mm.at,
                r.written_mm.value,
                r.written_mm.at,
                r.written_deg.value,
                r.written_deg.at,
                r.moved,
                r.moved_mm.value,
                r.values.value,
                r.values.at,
            );
            let over = r.solved_mm.value > LIMIT_MM
                || r.previous_mm.value > LIMIT_MM
                || r.written_mm.value > LIMIT_MM
                || r.written_deg.value > LIMIT_DEG;
            // Only the run on the game's own initiation values has to hold.
            if over && !own {
                failed.push(case.name.clone());
            }
        }
    }
    assert!(
        failed.is_empty(),
        "over {LIMIT_MM} mm or {LIMIT_DEG} degrees: {failed:?}"
    );
}
