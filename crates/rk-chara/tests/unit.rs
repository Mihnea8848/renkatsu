// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that need no game install: slider rules, scene maths, skinning,
//! weight rebinding and ABMX data.

use glam::{Quat, Vec3};
use rk_card::{ExtData, PluginData, Value};
use rk_chara::loader::assign_weights;
use rk_chara::scene::{euler_deg, DrawState, Instance, Part, RendererInst, Scene, Stencil, Trs};
use rk_chara::shape::{Rules, ShapeSet, BODY_RULES, HEAD_RULES};
use rk_unityfs::objects::{parse_queue, BlendShape, BlendShapeVertex, BoneWeight, Mesh, ShaderPass};
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
        shape_weights: Vec::new(),
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
fn blend_shapes_move_vertices_before_skinning() {
    let (mut scene, bone, _) = skinned_scene();
    let mut mesh = (*scene.renderers[0].mesh).clone();
    mesh.blend_shapes = vec![
        BlendShape {
            name: "lift".into(),
            vertices: vec![BlendShapeVertex {
                index: 0,
                position: [0.0, 2.0, 0.0],
                normal: [0.0; 3],
            }],
            full_weight: 100.0,
        },
        BlendShape {
            name: "side".into(),
            vertices: vec![BlendShapeVertex {
                index: 0,
                position: [4.0, 0.0, 0.0],
                normal: [0.0; 3],
            }],
            full_weight: 100.0,
        },
    ];
    scene.renderers[0].mesh = Arc::new(mesh);

    // No weights: the stored mesh.
    assert!(near(Vec3::from(scene.pose()[0].positions[0]), Vec3::ZERO));

    // Half of the first shape; a missing entry for the second counts as 0.
    scene.renderers[0].shape_weights = vec![0.5];
    assert!(near(Vec3::from(scene.pose()[0].positions[0]), Vec3::new(0.0, 1.0, 0.0)));

    // Shapes add up, and the bone moves the reshaped vertex.
    scene.renderers[0].shape_weights = vec![0.5, 0.25];
    scene.nodes[bone].local.pos = Vec3::new(0.0, 0.0, 3.0);
    let posed = scene.pose();
    assert!(near(Vec3::from(posed[0].positions[0]), Vec3::new(1.0, 1.0, 3.0)));
    // Other vertices are untouched by the shapes.
    assert!(near(Vec3::from(posed[0].positions[2]), Vec3::new(0.0, 1.0, 0.0)));
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
        shape_weights: Vec::new(),
    });
    let inst = Instance {
        root: top,
        nodes: vec![top, own_root, own_hips, extra, mesh_node],
        renderers: vec![0],
        renderer_keys: HashMap::new(),
        object_nodes: HashMap::new(),
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

#[test]
fn material_editor_overrides_find_their_materials() {
    use rk_chara::material_editor::{apply, Owner, Targets, KEY};
    use rk_chara::scene::MaterialInst;

    fn pack(v: &Value) -> Value {
        let mut out = Vec::new();
        rmpv::encode::write_value(&mut out, v).unwrap();
        Value::Binary(out)
    }
    fn entry(fields: Vec<(&str, Value)>) -> Value {
        Value::Map(fields.into_iter().map(|(k, v)| (Value::from(k), v)).collect())
    }
    let base = |kind: i64, coordinate: i64, slot: i64, material: &str| {
        vec![
            ("ObjectType", Value::from(kind)),
            ("CoordinateIndex", Value::from(coordinate)),
            ("Slot", Value::from(slot)),
            ("MaterialName", Value::from(material)),
        ]
    };
    let with = |mut e: Vec<(&'static str, Value)>, more: Vec<(&'static str, Value)>| {
        e.extend(more);
        entry(e)
    };

    // A 1x1 red PNG.
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();

    let mut plugin = PluginData::default();
    plugin.data.insert(
        "MaterialFloatPropertyList".into(),
        pack(&Value::Array(vec![
            with(
                base(1, 0, 2, "cloth"),
                vec![("Property", "Cutoff".into()), ("Value", "0.25".into())],
            ),
            // Other outfit: must not apply.
            with(
                base(1, 3, 2, "cloth"),
                vec![("Property", "Cutoff".into()), ("Value", "0.9".into())],
            ),
        ])),
    );
    plugin.data.insert(
        "MaterialColorPropertyList".into(),
        pack(&Value::Array(vec![with(
            base(4, 0, 0, "skin"),
            vec![
                ("Property", "Color".into()),
                (
                    "Value",
                    Value::Array(vec![0.5.into(), 0.25.into(), 0.0.into(), 1.0.into()]),
                ),
            ],
        )])),
    );
    plugin.data.insert(
        "MaterialShaderList".into(),
        pack(&Value::Array(vec![with(
            base(3, 0, 1, "hair"),
            vec![
                ("ShaderName", "Shader Forge/main_alpha".into()),
                ("RenderQueue", 3100.into()),
            ],
        )])),
    );
    plugin.data.insert(
        "MaterialTexturePropertyList".into(),
        pack(&Value::Array(vec![with(
            base(1, 0, 2, "cloth"),
            vec![
                ("Property", "MainTex".into()),
                ("TexID", 7.into()),
                ("Scale", Value::Array(vec![2.0.into(), 3.0.into()])),
            ],
        )])),
    );
    plugin.data.insert(
        "TextureDictionary".into(),
        pack(&Value::Map(vec![(7.into(), Value::Binary(png))])),
    );
    plugin.data.insert(
        "RendererPropertyList".into(),
        pack(&Value::Array(vec![entry(vec![
            ("ObjectType", 1.into()),
            ("CoordinateIndex", 0.into()),
            ("Slot", 2.into()),
            ("RendererName", "cloth_mesh".into()),
            ("Property", 0.into()),
            ("Value", "0".into()),
        ])])),
    );
    let mut ext = ExtData::new();
    ext.insert(KEY.into(), plugin);

    let mut scene = Scene::default();
    let node = scene.add_node("n", None, Trs::default());
    for (name, material, part) in [
        ("cloth_mesh", "cloth", Part::Clothes(2)),
        ("hair_mesh", "hair", Part::Hair(1)),
        ("body_mesh", "skin", Part::Body),
    ] {
        scene.renderers.push(RendererInst {
            name: name.into(),
            node,
            part,
            mesh: Arc::new(Mesh::default()),
            materials: vec![MaterialInst {
                name: material.into(),
                ..Default::default()
            }],
            skinned: false,
            bones: Vec::new(),
            enabled: true,
            shadows_only: false,
            shape_weights: Vec::new(),
        });
    }
    let mut targets = Targets::new();
    targets.insert(Owner::Clothing(2), vec![0]);
    targets.insert(Owner::Hair(1), vec![1]);

    let known = |name: &str| {
        (name == "Shader Forge/main_alpha").then_some(rk_chara::material_editor::Shader {
            draw: rk_chara::scene::DrawState {
                queue: 3040,
                blend: true,
                zwrite: true,
                outline: true,
                stencil: rk_chara::scene::Stencil::None,
                stencil_ref: 0,
            },
            queue: None,
        })
    };
    let applied = apply(&mut scene, &ext, 0, &targets, &known);
    assert_eq!(
        (
            applied.floats,
            applied.colors,
            applied.textures,
            applied.shaders,
            applied.renderers
        ),
        (1, 1, 1, 1, 1)
    );
    let cloth = &scene.renderers[0].materials[0];
    assert_eq!(cloth.floats["_Cutoff"], 0.25);
    let tex = &cloth.textures["_MainTex"];
    assert_eq!(
        (tex.image.rgba.as_slice(), tex.scale),
        (&[255u8, 0, 0, 255][..], [2.0, 3.0])
    );
    assert!(!scene.renderers[0].enabled);
    let hair = &scene.renderers[1].materials[0];
    assert_eq!(
        (hair.shader.as_str(), hair.render_queue),
        ("Shader Forge/main_alpha", 3100)
    );
    // Character-wide entries reach any renderer by material name.
    assert_eq!(scene.renderers[2].materials[0].colors["_Color"], [0.5, 0.25, 0.0, 1.0]);
}

#[test]
fn old_more_accessories_xml_is_read() {
    let xml = r#"<additionalAccessories version="2.0.21.1"><accessorySet type="0"><accessory type="123" id="100" parentKey="a_n_headside" addMove00x="1.2" addMove00y="-9" addMove00z="-6" addMove02x="1" addMove02y="1.4" addMove02z="1" color0r="0.5" color0g="0.25" color0b="0" color0a="1" hideCategory="1" noShake="true" /><accessory type="120" id="0" parentKey="" /></accessorySet><accessorySet type="2" /></additionalAccessories>"#;
    let mut plugin = PluginData::default();
    plugin.data.insert("additionalAccessories".into(), Value::from(xml));
    let mut ext = ExtData::new();
    ext.insert(rk_chara::more_accessories::KEY.into(), plugin);

    let sets = rk_chara::more_accessories::read(&ext);
    assert_eq!(sets.len(), 2);
    assert!(sets[&2].is_empty());
    let first = &sets[&0][0];
    assert_eq!(
        (first.kind, first.id, first.parent_key.as_str()),
        (123, 100, "a_n_headside")
    );
    assert_eq!(first.add_move.get(0, 0), Some(&[1.2, -9.0, -6.0]));
    assert_eq!(first.add_move.get(0, 2), Some(&[1.0, 1.4, 1.0]));
    // Missing values fall back to no rotation and unit scale.
    assert_eq!(first.add_move.get(1, 1), Some(&[0.0, 0.0, 0.0]));
    assert_eq!(first.add_move.get(1, 2), Some(&[1.0, 1.0, 1.0]));
    assert_eq!(first.color[0], [0.5, 0.25, 0.0, 1.0]);
    assert_eq!((first.hide_category, first.no_shake), (1, true));
    assert!(sets[&0][1].is_empty());
    assert!(rk_chara::more_accessories::read(&ExtData::new()).is_empty());
}

#[test]
fn queue_tags_become_numbers() {
    assert_eq!(parse_queue("Geometry"), Some(2000));
    assert_eq!(parse_queue("AlphaTest-100"), Some(2350));
    assert_eq!(parse_queue("Transparent+40"), Some(3040));
    assert_eq!(parse_queue(" transparent + 1010 "), Some(4010));
    assert_eq!(parse_queue("Overlay"), Some(4000));
    assert_eq!(parse_queue("Nonsense"), None);
    assert_eq!(parse_queue("Transparent+x"), None);
}

#[test]
fn draw_state_comes_from_the_forward_pass() {
    let pass = |name: &str, mode: &str| ShaderPass {
        name: name.into(),
        light_mode: mode.into(),
        queue: Some(3040),
        zwrite: true,
        src_blend: 1,
        dst_blend: 0,
        ..Default::default()
    };
    // Front hair: outline, a blended forward pass that skips pixels marked 2,
    // and a shadow pass that must not be picked.
    let mut forward = pass("FORWARD", "FORWARDBASE");
    forward.src_blend = 5;
    forward.dst_blend = 10;
    forward.stencil_ref = 2;
    forward.stencil_comp = 6;
    let passes = [pass("OUTLINE", ""), forward, pass("SHADOWCASTER", "SHADOWCASTER")];
    assert_eq!(
        DrawState::from_passes(&passes),
        Some(DrawState {
            queue: 3040,
            blend: true,
            zwrite: true,
            outline: true,
            stencil: Stencil::SkipEqual,
            stencil_ref: 2,
        })
    );

    // Eye layer: no depth write, marks its pixels.
    let mut eye = pass("FORWARD", "FORWARDBASE");
    eye.queue = None;
    eye.zwrite = false;
    eye.stencil_ref = 2;
    eye.stencil_comp = 8;
    eye.stencil_pass = 2;
    let state = DrawState::from_passes(&[eye]).unwrap();
    assert_eq!(
        (state.queue, state.zwrite, state.outline, state.stencil),
        (2000, false, false, Stencil::Write)
    );

    // Without a tagged forward pass the first pass that draws a surface is used.
    let plain = DrawState::from_passes(&[pass("OUTLINE", ""), pass("BASE", "")]).unwrap();
    assert_eq!(
        (plain.blend, plain.stencil, plain.outline),
        (false, Stencil::None, true)
    );
    assert_eq!(DrawState::from_passes(&[pass("SHADOWCASTER", "SHADOWCASTER")]), None);
    assert_eq!(DrawState::from_passes(&[]), None);
}

#[test]
fn head_types_look_in_the_twin_bundle_first() {
    use rk_chara::build::head_variant;
    assert_eq!(
        head_variant("chara/mt_eyeline_down_00.unity3d", "cf_t_eyeline_down_03", 0),
        None
    );
    assert_eq!(
        head_variant("chara/mt_eyeline_down_00.unity3d", "cf_t_eyeline_down_03", 1),
        Some((
            "chara/mt_eyeline_down_50.unity3d".to_string(),
            "cf_t_eyeline_down_03_1".to_string()
        ))
    );
    // A mod bundle with any name gets the same treatment.
    assert_eq!(
        head_variant("mods/lash.unity3d", "lash", 2),
        Some(("mods/la50.unity3d".to_string(), "lash_2".to_string()))
    );
    assert_eq!(
        head_variant("a.unity3d", "t", 1),
        Some(("50.unity3d".to_string(), "t_1".to_string()))
    );
}
