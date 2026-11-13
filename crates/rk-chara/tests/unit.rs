// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that need no game install: slider rules, scene maths, skinning,
//! weight rebinding and ABMX data.

use glam::{Quat, Vec3};
use rk_card::{ExtData, PluginData, Value};
use rk_chara::loader::assign_weights;
use rk_chara::scene::{euler_deg, Instance, Part, RendererInst, Scene, Trs};
use rk_chara::shape::{Rules, ShapeSet, BODY_RULES, HEAD_RULES};
use rk_unityfs::objects::{BoneWeight, Mesh};
use std::collections::HashMap;
use std::sync::Arc;

fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

fn net_string(s: &str) -> Vec<u8> {
    let mut out = vec![s.len() as u8];
    out.extend_from_slice(s.as_bytes());
    out
}

/// Key table with one source and the given keys (9 channels each).
fn key_table(name: &str, keys: &[[f32; 9]]) -> Vec<u8> {
    let mut out = 1i32.to_le_bytes().to_vec();
    out.extend(net_string(name));
    out.extend_from_slice(&(keys.len() as i32).to_le_bytes());
    for (i, k) in keys.iter().enumerate() {
        out.extend_from_slice(&(i as i32).to_le_bytes());
        for v in k {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

#[test]
fn shipped_rule_tables_parse() {
    Rules::parse(BODY_RULES).unwrap();
    Rules::parse(HEAD_RULES).unwrap();
}

#[test]
fn bad_rules_are_rejected() {
    assert!(Rules::parse("px = 1").is_err(), "rule before any bone");
    assert!(Rules::parse("bone a\n  qq = 1").is_err(), "unknown target");
    assert!(Rules::parse("bone a\n  px = 1 +").is_err(), "dangling operator");
    assert!(Rules::parse("bone a\n  px = (1").is_err(), "unbalanced bracket");
    assert!(Rules::parse("bone a\n  px = s.src.zz").is_err(), "unknown channel");
}

#[test]
fn sliders_drive_bones_through_rules() {
    let keys = key_table(
        "src_a",
        &[
            [0.0, 0.0, 0.0, 350.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            [2.0, 0.0, 0.0, 10.0, 0.0, 0.0, 3.0, 1.0, 1.0],
        ],
    );
    // Slider 0 feeds position x, rotation x and scale x of src_a.
    let table = b"0\tsrc_a\t1\t0\t0\t1\t0\t0\t1\t0\t0\nbroken line\n";
    let mut set = ShapeSet::from_tables(&keys, table).unwrap();
    set.vars.insert("k".into(), 10.0);
    let rules = Rules::parse(
        "# comment\nbone target\n  px = s.src_a.px * 2 + 1\n  py = -(s.src_a.sx - 1) / 2\n  pz = v.k + c.0.px\n  sx = s.src_a.sx\n  rx = wrap(s.src_a.rx)\nbone absent\n  px = 5\n",
    )
    .unwrap();

    let mut scene = Scene::default();
    let root = scene.add_node("root", None, Trs::default());
    let target = scene.add_node(
        "target",
        Some(root),
        Trs {
            pos: Vec3::new(9.0, 9.0, 9.0),
            ..Default::default()
        },
    );

    set.set(0, 0.5);
    set.apply(&mut scene, root, &rules, None);
    let local = scene.nodes[target].local;
    // Midway: px 1, sx 2; the angle takes the short way from 350 to 10.
    assert!(near(local.pos, Vec3::new(3.0, -0.5, 10.0)), "{:?}", local.pos);
    assert!((local.scale.x - 2.0).abs() < 1e-5 && local.scale.y == 1.0);
    assert!(near(local.rot * Vec3::Y, Vec3::Y));

    // Unlocked sliders extrapolate past the last key.
    set.set(0, 1.5);
    set.apply(
        &mut scene,
        root,
        &rules,
        Some(&[[100.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]]),
    );
    let local = scene.nodes[target].local;
    assert!(near(local.pos, Vec3::new(7.0, -1.5, 110.0)), "{:?}", local.pos);
}

#[test]
fn euler_angles_follow_unity_order() {
    // Yaw 90 turns forward (+Z) to the right (+X).
    assert!(near(euler_deg(0.0, 90.0, 0.0) * Vec3::Z, Vec3::X));
    // Pitch 90 turns forward down.
    assert!(near(euler_deg(90.0, 0.0, 0.0) * Vec3::Z, -Vec3::Y));
    // Z is applied first, then X, then Y.
    let q = euler_deg(90.0, 90.0, 90.0);
    let expected = Quat::from_rotation_y(90f32.to_radians())
        * Quat::from_rotation_x(90f32.to_radians())
        * Quat::from_rotation_z(90f32.to_radians());
    for v in [Vec3::X, Vec3::Y, Vec3::Z] {
        assert!(near(q * v, expected * v));
    }
}

#[test]
fn world_matrices_and_activity_inherit() {
    let mut scene = Scene::default();
    let a = scene.add_node(
        "a",
        None,
        Trs {
            pos: Vec3::new(1.0, 0.0, 0.0),
            scale: Vec3::splat(2.0),
            ..Default::default()
        },
    );
    let b = scene.add_node(
        "b",
        Some(a),
        Trs {
            pos: Vec3::new(0.0, 1.0, 0.0),
            ..Default::default()
        },
    );
    let c = scene.add_node("c", Some(b), Trs::default());
    scene.nodes[b].active = false;
    let (world, active) = scene.world();
    assert!(near(world[c].transform_point3(Vec3::ZERO), Vec3::new(1.0, 2.0, 0.0)));
    assert_eq!(active, [true, false, false]);
    assert_eq!(scene.find(a, "c"), Some(c));
    assert_eq!(scene.subtree(a).len(), 3);
    scene.remove(b);
    assert_eq!(scene.find(a, "c"), None);
}

fn skinned_scene() -> (Scene, usize, usize) {
    let mut scene = Scene::default();
    let root = scene.add_node("root", None, Trs::default());
    let bone = scene.add_node("bone", Some(root), Trs::default());
    let node = scene.add_node("mesh", Some(root), Trs::default());
    let identity = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let mesh = Mesh {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        indices: vec![0, 1, 2],
        skin: vec![
            BoneWeight {
                weights: [1.0, 0.0, 0.0, 0.0],
                bones: [0, 0, 0, 0],
            },
            BoneWeight {
                weights: [0.5, 0.5, 0.0, 0.0],
                bones: [0, 1, 0, 0],
            },
            BoneWeight {
                weights: [1.0, 0.0, 0.0, 0.0],
                bones: [1, 0, 0, 0],
            },
        ],
        bind_poses: vec![identity, identity],
        ..Default::default()
    };
    scene.renderers.push(RendererInst {
        name: "mesh".into(),
        node,
        part: Part::Body,
        mesh: Arc::new(mesh),
        materials: Vec::new(),
        skinned: true,
        bones: vec![Some(bone), Some(root)],
        enabled: true,
        shadows_only: false,
    });
    (scene, bone, node)
}

#[test]
fn skinning_blends_bone_transforms() {
    let (mut scene, bone, node) = skinned_scene();
    scene.nodes[bone].local.pos = Vec3::new(0.0, 0.0, 2.0);
    let draws = scene.pose();
    assert_eq!(draws.len(), 1);
    let p: Vec<Vec3> = draws[0].positions.iter().map(|p| Vec3::from(*p)).collect();
    assert!(near(p[0], Vec3::new(0.0, 0.0, 2.0)), "fully on the moved bone");
    assert!(near(p[1], Vec3::new(1.0, 0.0, 1.0)), "half on each bone");
    assert!(near(p[2], Vec3::new(0.0, 1.0, 0.0)), "on the still bone");
    drop(draws);

    // A missing bone drops out and the remaining weights are renormalised.
    scene.renderers[0].bones[1] = None;
    let draws = scene.pose();
    assert!(near(Vec3::from(draws[0].positions[1]), Vec3::new(1.0, 0.0, 2.0)));
    drop(draws);

    // Hidden and shadow-only renderers are not drawn.
    scene.nodes[node].active = false;
    assert!(scene.pose().is_empty());
    scene.nodes[node].active = true;
    scene.renderers[0].shadows_only = true;
    assert!(scene.pose().is_empty());
}

#[test]
fn weights_rebind_by_name_and_extra_bones_are_grafted() {
    let mut scene = Scene::default();
    // Main skeleton.
    let skel = scene.add_node("cf_j_root", None, Trs::default());
    let hips = scene.add_node("cf_j_hips", Some(skel), Trs::default());
    let dict = scene.name_map(skel);

    // A clothing prefab with its own copy of the skeleton plus one extra bone.
    let top = scene.add_node("clothes", None, Trs::default());
    let own_root = scene.add_node("cf_j_root", Some(top), Trs::default());
    let own_hips = scene.add_node("cf_j_hips", Some(own_root), Trs::default());
    let extra = scene.add_node("ribbon_bone", Some(own_hips), Trs::default());
    let mesh_node = scene.add_node("o_top", Some(top), Trs::default());
    scene.renderers.push(RendererInst {
        name: "o_top".into(),
        node: mesh_node,
        part: Part::Clothes(0),
        mesh: Arc::new(Mesh::default()),
        materials: Vec::new(),
        skinned: true,
        bones: vec![Some(own_hips), Some(extra), None],
        enabled: true,
        shadows_only: false,
    });
    let inst = Instance {
        root: top,
        nodes: vec![top, own_root, own_hips, extra, mesh_node],
        renderers: vec![0],
        renderer_keys: HashMap::new(),
        scripts: Vec::new(),
    };

    assign_weights(&mut scene, &inst, &dict, "cf_j_root");
    assert_eq!(scene.renderers[0].bones, [Some(hips), Some(extra), None]);
    assert_eq!(
        scene.nodes[extra].parent,
        Some(hips),
        "extra bone moves onto the main skeleton"
    );
    assert_eq!(
        scene.nodes[own_root].parent, None,
        "the prefab's skeleton copy is removed"
    );
    let (_, active) = scene.world();
    assert!(active[extra] && !active[own_hips]);
}

#[test]
fn abmx_modifiers_read_and_apply() {
    fn floats(v: [f64; 3]) -> Value {
        Value::Array(v.iter().map(|f| Value::F64(*f)).collect())
    }
    let modifier = |scale: [f64; 3], length: f64, pos: [f64; 3], rot: [f64; 3]| {
        Value::Array(vec![floats(scale), Value::F64(length), floats(pos), floats(rot)])
    };
    let list = Value::Array(vec![
        // One modifier for every outfit.
        Value::Array(vec![
            "bone_a".into(),
            Value::Array(vec![modifier([2.0, 1.0, 1.0], 1.5, [0.0, 0.1, 0.0], [0.0, 90.0, 0.0])]),
        ]),
        // One per outfit, scoped to accessory slot 3.
        Value::Array(vec![
            "bone_b".into(),
            Value::Array(vec![
                modifier([1.0; 3], 1.0, [0.0; 3], [0.0; 3]),
                modifier([1.0, 3.0, 1.0], 1.0, [0.0; 3], [0.0; 3]),
            ]),
            13.into(),
        ]),
    ]);
    let mut bytes = Vec::new();
    rmpv::encode::write_value(&mut bytes, &list).unwrap();
    let mut ext = ExtData::new();
    let mut plugin = PluginData {
        version: 2,
        ..Default::default()
    };
    plugin.data.insert("boneData".into(), Value::Binary(bytes));
    ext.insert(rk_chara::abmx::KEY.into(), plugin);

    let mods = rk_chara::abmx::read(&ext);
    assert_eq!(mods.len(), 2);
    assert_eq!(
        (mods[0].bone.as_str(), mods[0].location, mods[0].per_outfit.len()),
        ("bone_a", 0, 1)
    );
    assert_eq!((mods[1].location, mods[1].per_outfit.len()), (13, 2));

    let mut scene = Scene::default();
    let a = scene.add_node(
        "bone_a",
        None,
        Trs {
            pos: Vec3::new(0.0, 2.0, 0.0),
            scale: Vec3::splat(2.0),
            ..Default::default()
        },
    );
    let b = scene.add_node("bone_b", None, Trs::default());
    let bones: HashMap<String, usize> = [("bone_a".to_string(), a)].into();
    let accessories: HashMap<usize, HashMap<String, usize>> = [(3, [("bone_b".to_string(), b)].into())].into();
    rk_chara::abmx::apply(&mut scene, &mods, 1, &bones, &accessories);

    let la = scene.nodes[a].local;
    assert!(
        near(la.scale, Vec3::new(4.0, 2.0, 2.0)),
        "scale multiplies the baseline"
    );
    assert!(
        near(la.pos, Vec3::new(0.0, 3.1, 0.0)),
        "length scales the offset, then position is added"
    );
    assert!(near(la.rot * Vec3::Z, Vec3::X));
    assert!(
        near(scene.nodes[b].local.scale, Vec3::new(1.0, 3.0, 1.0)),
        "second outfit's modifier is used"
    );

    assert!(rk_chara::abmx::read(&ExtData::new()).is_empty());
}
