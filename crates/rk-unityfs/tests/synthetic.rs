// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reader tests on a bundle built in memory, so they run without a game
//! install.

use rk_unityfs::objects::{Material, Mesh, Texture2D};
use rk_unityfs::{class, Bundle, SerializedFile, UObject, UValue};
use std::sync::Arc;

fn cstr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

/// A version 17 serialized file holding one TextAsset with a type tree.
fn serialized_file(name: &str, script: &[u8]) -> Vec<u8> {
    // Local string buffer; "string" comes from the built-in table instead.
    let names = [
        "TextAsset",
        "Base",
        "m_Name",
        "Array",
        "int",
        "size",
        "char",
        "data",
        "m_Script",
    ];
    let mut strings = Vec::new();
    let offset = |s: &str| -> u32 {
        let i = names.iter().position(|n| *n == s).unwrap();
        names[..i].iter().map(|n| n.len() as u32 + 1).sum()
    };
    for n in names {
        strings.extend(cstr(n));
    }
    const STRING: u32 = 0x8000_0000 | 840;
    let nodes: Vec<(u8, u32, u32, i32, u32)> = vec![
        (0, offset("TextAsset"), offset("Base"), -1, 0),
        (1, STRING, offset("m_Name"), -1, 0x8000),
        (2, offset("Array"), offset("Array"), -1, 0x4001),
        (3, offset("int"), offset("size"), 4, 1),
        (3, offset("char"), offset("data"), 1, 1),
        (1, STRING, offset("m_Script"), -1, 0),
        (2, offset("Array"), offset("Array"), -1, 0x4001),
        (3, offset("int"), offset("size"), 4, 1),
        (3, offset("char"), offset("data"), 1, 1),
    ];

    let mut meta = cstr("5.6.2f1");
    meta.extend_from_slice(&5i32.to_le_bytes());
    meta.push(1); // type trees present
    meta.extend_from_slice(&1i32.to_le_bytes());
    meta.extend_from_slice(&class::TEXT_ASSET.to_le_bytes());
    meta.push(0);
    meta.extend_from_slice(&(-1i16).to_le_bytes());
    meta.extend_from_slice(&[0; 16]);
    meta.extend_from_slice(&(nodes.len() as u32).to_le_bytes());
    meta.extend_from_slice(&(strings.len() as u32).to_le_bytes());
    for (i, (level, ty, name, size, flags)) in nodes.iter().enumerate() {
        meta.extend_from_slice(&1u16.to_le_bytes());
        meta.push(*level);
        meta.push(0);
        meta.extend_from_slice(&ty.to_le_bytes());
        meta.extend_from_slice(&name.to_le_bytes());
        meta.extend_from_slice(&size.to_le_bytes());
        meta.extend_from_slice(&(i as i32).to_le_bytes());
        meta.extend_from_slice(&flags.to_le_bytes());
    }
    meta.extend_from_slice(&strings);

    let mut object = Vec::new();
    for field in [name.as_bytes(), script] {
        object.extend_from_slice(&(field.len() as i32).to_le_bytes());
        object.extend_from_slice(field);
        while !object.len().is_multiple_of(4) {
            object.push(0);
        }
    }

    const HEADER: usize = 20;
    meta.extend_from_slice(&1i32.to_le_bytes());
    while !(HEADER + meta.len()).is_multiple_of(4) {
        meta.push(0);
    }
    meta.extend_from_slice(&77i64.to_le_bytes());
    meta.extend_from_slice(&0u32.to_le_bytes());
    meta.extend_from_slice(&(object.len() as u32).to_le_bytes());
    meta.extend_from_slice(&0i32.to_le_bytes());
    meta.extend_from_slice(&0i32.to_le_bytes()); // script types
    meta.extend_from_slice(&1i32.to_le_bytes()); // externals
    meta.extend(cstr(""));
    meta.extend_from_slice(&[0; 20]);
    meta.extend(cstr("archive:/CAB-other/CAB-other"));

    let data_offset = (HEADER + meta.len() + 15) & !15;
    let mut file = Vec::new();
    file.extend_from_slice(&(meta.len() as u32).to_be_bytes());
    file.extend_from_slice(&((data_offset + object.len()) as u32).to_be_bytes());
    file.extend_from_slice(&17u32.to_be_bytes());
    file.extend_from_slice(&(data_offset as u32).to_be_bytes());
    file.extend_from_slice(&[0; 4]);
    file.extend_from_slice(&meta);
    file.resize(data_offset, 0);
    file.extend_from_slice(&object);
    file
}

/// Wraps files in a UnityFS container. `compression` 0 stores, 3 uses LZ4.
fn bundle(files: &[(&str, &[u8])], compression: u8, block_size: usize) -> Vec<u8> {
    let payload: Vec<u8> = files.iter().flat_map(|(_, d)| d.iter().copied()).collect();
    let pack = |d: &[u8]| {
        if compression == 0 {
            d.to_vec()
        } else {
            lz4_flex::block::compress(d)
        }
    };
    let mut info = vec![0u8; 16];
    let chunks: Vec<&[u8]> = payload.chunks(block_size).collect();
    let packed: Vec<Vec<u8>> = chunks.iter().map(|c| pack(c)).collect();
    info.extend_from_slice(&(chunks.len() as u32).to_be_bytes());
    for (c, p) in chunks.iter().zip(&packed) {
        info.extend_from_slice(&(c.len() as u32).to_be_bytes());
        info.extend_from_slice(&(p.len() as u32).to_be_bytes());
        info.extend_from_slice(&(compression as u16).to_be_bytes());
    }
    info.extend_from_slice(&(files.len() as u32).to_be_bytes());
    let mut offset = 0u64;
    for (name, d) in files {
        info.extend_from_slice(&offset.to_be_bytes());
        info.extend_from_slice(&(d.len() as u64).to_be_bytes());
        info.extend_from_slice(&(if name.ends_with(".resS") { 0u32 } else { 4 }).to_be_bytes());
        info.extend(cstr(name));
        offset += d.len() as u64;
    }
    let packed_info = pack(&info);

    let mut out = cstr("UnityFS");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend(cstr("5.x.x"));
    out.extend(cstr("5.6.2f1"));
    out.extend_from_slice(&0u64.to_be_bytes());
    out.extend_from_slice(&(packed_info.len() as u32).to_be_bytes());
    out.extend_from_slice(&(info.len() as u32).to_be_bytes());
    out.extend_from_slice(&(0x40 | compression as u32).to_be_bytes());
    out.extend_from_slice(&packed_info);
    for p in packed {
        out.extend_from_slice(&p);
    }
    out
}

#[test]
fn bundle_and_object_round_trip() {
    let script: Vec<u8> = (0..=255u8).cycle().take(700).collect();
    let sf = serialized_file("cf_anmShapeBody", &script);
    let res: Vec<u8> = (0..5000u32).flat_map(|i| i.to_le_bytes()).collect();
    for (compression, block) in [(0u8, usize::MAX), (3, usize::MAX), (3, 512)] {
        let bytes = bundle(
            &[("CAB-test", &sf), ("CAB-test.resS", &res)],
            compression,
            block.min(1 << 20),
        );
        assert!(Bundle::is_bundle(&bytes));
        let b = Bundle::from_bytes(bytes).unwrap();
        assert_eq!(b.unity_version, "5.6.2f1");
        assert_eq!(b.nodes.len(), 2);
        assert!(b.nodes[0].is_serialized() && !b.nodes[1].is_serialized());

        let f = SerializedFile::parse(b.read_node(&b.nodes[0]).unwrap()).unwrap();
        assert_eq!(f.version, 17);
        assert_eq!(f.externals[0].file_name(), "CAB-other");
        let obj = f.object(77).unwrap();
        assert_eq!(obj.class_id, class::TEXT_ASSET);
        assert_eq!(f.name(obj).as_deref(), Some("cf_anmShapeBody"));
        let v = f.read_strict(obj).unwrap();
        // Binary payloads declared as strings come back as bytes.
        assert_eq!(v.bytes("m_Script"), Some(&script[..]));
        assert_eq!(f.read_until(obj, "m_Name").unwrap().fields.len(), 1);

        // A range that crosses block boundaries inside the second node.
        let node = b.node("cab-test.ress").unwrap();
        assert_eq!(b.read_range(node.offset + 1001, 3000).unwrap(), res[1001..4001]);
        assert!(b.read_range(node.offset + node.size - 4, 8).is_err());
    }
}

#[test]
fn damaged_input_is_an_error() {
    let sf = serialized_file("x", b"abc");
    let bytes = bundle(&[("CAB-test", &sf)], 3, 1 << 20);
    assert!(Bundle::from_bytes(b"not a bundle at all".to_vec()).is_err());
    for cut in (8..bytes.len()).step_by(11) {
        let ok = Bundle::from_bytes(bytes[..cut].to_vec()).and_then(|b| b.read_node(&b.nodes[0]).map(|_| ()));
        assert!(ok.is_err(), "truncated bundle at {cut} was accepted");
    }
    for cut in (0..sf.len() - 1).step_by(13) {
        let parsed = SerializedFile::parse(Arc::from(&sf[..cut]));
        let ok = parsed.and_then(|f| f.read(&f.objects[0]).map(|_| ()));
        assert!(ok.is_err(), "truncated file at {cut} was accepted");
    }
}

fn obj(fields: Vec<(&str, UValue)>) -> UValue {
    UValue::Object(Box::new(UObject {
        ty: "".into(),
        fields: fields.into_iter().map(|(k, v)| (k.into(), v)).collect(),
    }))
}

fn root(fields: Vec<(&str, UValue)>) -> UObject {
    match obj(fields) {
        UValue::Object(o) => *o,
        _ => unreachable!(),
    }
}

fn pptr(id: i64) -> UValue {
    obj(vec![("m_FileID", UValue::Int(0)), ("m_PathID", UValue::Int(id))])
}

#[test]
fn mesh_streams_decode() {
    // Stream 0: position + normal (24 bytes). Stream 1: colour bytes + uv.
    let positions = [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
    let mut data = Vec::new();
    for p in positions {
        for v in p.iter().chain(&[0.0, 0.0, 1.0]) {
            data.extend_from_slice(&v.to_le_bytes());
        }
    }
    data.resize(80, 0); // streams are aligned to 16 bytes
    for i in 0..3u8 {
        data.extend_from_slice(&[255, 128, 0, 255]);
        data.extend_from_slice(&(i as f32 * 0.5).to_le_bytes());
        data.extend_from_slice(&1.0f32.to_le_bytes());
    }
    let chan = |stream: i64, offset: i64, format: i64, dim: i64| {
        obj(vec![
            ("stream", UValue::Int(stream)),
            ("offset", UValue::Int(offset)),
            ("format", UValue::Int(format)),
            ("dimension", UValue::Int(dim)),
        ])
    };
    let unused = || chan(0, 0, 0, 0);
    let mut identity = Vec::new();
    for r in 0..4 {
        for c in 0..4 {
            identity.push((format!("e{r}{c}"), UValue::Float(if r == c { 1.0 } else { 0.0 })));
        }
    }
    let bind = UValue::Object(Box::new(UObject {
        ty: "".into(),
        fields: identity.into_iter().map(|(k, v)| (k.into(), v)).collect(),
    }));
    let skin = |bone: i64| {
        obj(vec![
            ("weight[0]", UValue::Float(0.75)),
            ("weight[1]", UValue::Float(0.25)),
            ("weight[2]", UValue::Float(0.0)),
            ("weight[3]", UValue::Float(0.0)),
            ("boneIndex[0]", UValue::Int(bone)),
            ("boneIndex[1]", UValue::Int(1)),
            ("boneIndex[2]", UValue::Int(0)),
            ("boneIndex[3]", UValue::Int(0)),
        ])
    };
    let vec3 = |x: f64, y: f64, z: f64| {
        obj(vec![
            ("x", UValue::Float(x)),
            ("y", UValue::Float(y)),
            ("z", UValue::Float(z)),
        ])
    };
    let mesh = root(vec![
        ("m_Name", UValue::Str("tri".into())),
        (
            "m_SubMeshes",
            UValue::Array(vec![obj(vec![
                ("firstByte", UValue::Int(0)),
                ("indexCount", UValue::Int(3)),
                ("topology", UValue::Int(0)),
            ])]),
        ),
        (
            "m_Shapes",
            obj(vec![
                (
                    "vertices",
                    UValue::Array(vec![obj(vec![
                        ("vertex", vec3(0.0, 0.5, 0.0)),
                        ("normal", vec3(0.0, 0.0, 0.0)),
                        ("index", UValue::Int(2)),
                    ])]),
                ),
                (
                    "shapes",
                    UValue::Array(vec![obj(vec![
                        ("firstVertex", UValue::Int(0)),
                        ("vertexCount", UValue::Int(1)),
                    ])]),
                ),
                (
                    "channels",
                    UValue::Array(vec![obj(vec![
                        ("name", UValue::Str("smile".into())),
                        ("frameIndex", UValue::Int(0)),
                        ("frameCount", UValue::Int(1)),
                    ])]),
                ),
                ("fullWeights", UValue::Array(vec![UValue::Float(100.0)])),
            ]),
        ),
        ("m_BindPose", UValue::Array(vec![bind.clone(), bind])),
        ("m_MeshCompression", UValue::Int(0)),
        ("m_IndexBuffer", UValue::Bytes(vec![0, 0, 1, 0, 2, 0])),
        ("m_Skin", UValue::Array(vec![skin(0), skin(0), skin(1)])),
        (
            "m_VertexData",
            obj(vec![
                ("m_VertexCount", UValue::Int(3)),
                (
                    "m_Channels",
                    UValue::Array(vec![
                        chan(0, 0, 0, 3),
                        chan(0, 12, 0, 3),
                        chan(1, 0, 2, 4),
                        chan(1, 4, 0, 2),
                        unused(),
                        unused(),
                        unused(),
                        unused(),
                    ]),
                ),
                ("m_DataSize", UValue::Bytes(data)),
            ]),
        ),
    ]);
    let m = Mesh::read(&mesh).unwrap();
    assert_eq!(m.positions, positions);
    assert_eq!(m.normals[2], [0.0, 0.0, 1.0]);
    assert_eq!(m.uv[0], [[0.0, 1.0], [0.5, 1.0], [1.0, 1.0]]);
    assert!((m.colors[0][1] - 128.0 / 255.0).abs() < 1e-6);
    assert_eq!(m.indices, [0, 1, 2]);
    assert_eq!((m.submeshes[0].first_index, m.submeshes[0].index_count), (0, 3));
    assert_eq!(m.skin[2].bones, [1, 1, 0, 0]);
    assert_eq!(m.skin[0].weights, [0.75, 0.25, 0.0, 0.0]);
    assert_eq!(m.bind_poses.len(), 2);
    assert_eq!(m.blend_shapes[0].name, "smile");
    assert_eq!(
        (
            m.blend_shapes[0].vertices[0].index,
            m.blend_shapes[0].vertices[0].position
        ),
        (2, [0.0, 0.5, 0.0])
    );
}

#[test]
fn material_properties_decode() {
    let entry = |k: &str, v: UValue| obj(vec![("first", UValue::Str(k.into())), ("second", v)]);
    let old_key = obj(vec![
        ("first", obj(vec![("name", UValue::Str("_Legacy".into()))])),
        ("second", UValue::Float(2.0)),
    ]);
    let color = obj(vec![
        ("r", UValue::Float(1.0)),
        ("g", UValue::Float(0.5)),
        ("b", UValue::Float(0.25)),
        ("a", UValue::Float(1.0)),
    ]);
    let tex = obj(vec![
        ("m_Texture", pptr(9)),
        (
            "m_Scale",
            obj(vec![("x", UValue::Float(2.0)), ("y", UValue::Float(3.0))]),
        ),
        (
            "m_Offset",
            obj(vec![("x", UValue::Float(0.5)), ("y", UValue::Float(0.0))]),
        ),
    ]);
    let mat = root(vec![
        ("m_Name", UValue::Str("cf_m_body".into())),
        ("m_Shader", pptr(4)),
        ("m_ShaderKeywords", UValue::Str("A B".into())),
        ("m_CustomRenderQueue", UValue::Int(2450)),
        (
            "m_SavedProperties",
            obj(vec![
                ("m_TexEnvs", UValue::Array(vec![entry("_MainTex", tex)])),
                (
                    "m_Floats",
                    UValue::Array(vec![entry("_Cutoff", UValue::Float(0.5)), old_key]),
                ),
                ("m_Colors", UValue::Array(vec![entry("_Color", color)])),
            ]),
        ),
    ]);
    let m = Material::read(&mat).unwrap();
    assert_eq!((m.shader.path_id, m.render_queue), (4, 2450));
    assert_eq!(m.float("_Cutoff"), Some(0.5));
    assert_eq!(m.float("_Legacy"), Some(2.0));
    assert_eq!(m.color("_Color"), Some([1.0, 0.5, 0.25, 1.0]));
    let t = m.texture("_MainTex").unwrap();
    assert_eq!((t.texture.path_id, t.scale, t.offset), (9, [2.0, 3.0], [0.5, 0.0]));
}

fn texture(format: i64, width: i64, height: i64) -> Texture2D {
    Texture2D::read(&root(vec![
        ("m_Name", UValue::Str("t".into())),
        ("m_Width", UValue::Int(width)),
        ("m_Height", UValue::Int(height)),
        ("m_TextureFormat", UValue::Int(format)),
    ]))
    .unwrap()
}

#[test]
fn uncompressed_texture_formats_decode() {
    assert_eq!(texture(4, 1, 1).decode_rgba8(&[1, 2, 3, 4]).unwrap(), [1, 2, 3, 4]); // RGBA32
    assert_eq!(texture(5, 1, 1).decode_rgba8(&[4, 1, 2, 3]).unwrap(), [1, 2, 3, 4]); // ARGB32
    assert_eq!(texture(14, 1, 1).decode_rgba8(&[3, 2, 1, 4]).unwrap(), [1, 2, 3, 4]); // BGRA32
    assert_eq!(
        texture(3, 2, 1).decode_rgba8(&[1, 2, 3, 4, 5, 6]).unwrap(),
        [1, 2, 3, 255, 4, 5, 6, 255]
    ); // RGB24
    assert_eq!(texture(1, 1, 1).decode_rgba8(&[9]).unwrap(), [255, 255, 255, 9]); // Alpha8
    assert_eq!(
        texture(7, 1, 1).decode_rgba8(&0xF800u16.to_le_bytes()).unwrap(),
        [255, 0, 0, 255]
    ); // RGB565
    assert_eq!(
        texture(13, 1, 1).decode_rgba8(&0xF00Fu16.to_le_bytes()).unwrap(),
        [255, 0, 0, 255]
    ); // RGBA4444
    assert_eq!(
        texture(2, 1, 1).decode_rgba8(&0xF00Fu16.to_le_bytes()).unwrap(),
        [0, 0, 255, 255]
    ); // ARGB4444
       // DXT1 block of solid red: two RGB565 endpoints, all indices 0.
    let dxt1 = [0x00, 0xF8, 0x00, 0xF8, 0, 0, 0, 0];
    let px = texture(10, 4, 4).decode_rgba8(&dxt1).unwrap();
    assert!(px.chunks(4).all(|p| p == [255, 0, 0, 255]));
    // Short data and unknown formats are errors.
    assert!(texture(4, 2, 2).decode_rgba8(&[0; 15]).is_err());
    assert!(texture(9999, 1, 1).decode_rgba8(&[0; 16]).is_err());
}

/// A Shader object with one subshader and the given passes (name, light
/// mode, culling) and fallback.
fn shader(passes: &[(&str, &str, f64)], fallback: &str) -> UObject {
    let val = |v: f64| obj(vec![("val", UValue::Float(v))]);
    let passes = passes
        .iter()
        .map(|(name, mode, cull)| {
            let tag = obj(vec![
                ("first", UValue::Str("LIGHTMODE".into())),
                ("second", UValue::Str((*mode).into())),
            ]);
            obj(vec![(
                "m_State",
                obj(vec![
                    ("m_Name", UValue::Str((*name).into())),
                    ("m_Tags", obj(vec![("tags", UValue::Array(vec![tag]))])),
                    ("culling", val(*cull)),
                    ("zWrite", val(1.0)),
                ]),
            )])
        })
        .collect();
    let sub = obj(vec![("m_Passes", UValue::Array(passes))]);
    root(vec![(
        "m_ParsedForm",
        obj(vec![
            ("m_SubShaders", UValue::Array(vec![sub])),
            ("m_FallbackName", UValue::Str(fallback.into())),
        ]),
    )])
}

#[test]
fn shadow_caster_pass_comes_from_the_shader_or_its_fallback() {
    use rk_unityfs::objects::{fallback_casts_shadows, shader_passes};
    // Its own pass, with the culling it states.
    let own = shader_passes(&shader(
        &[("FORWARD", "ForwardBase", 0.0), ("SHADOWCASTER", "ShadowCaster", 2.0)],
        "Unlit/Texture",
    ));
    let caster: Vec<_> = own.iter().filter(|p| p.light_mode == "SHADOWCASTER").collect();
    assert_eq!(caster.len(), 1);
    assert_eq!((caster[0].cull, caster[0].from_fallback), (2, false));
    assert_eq!(own[0].cull, 0);

    // None of its own: Diffuse brings VertexLit's.
    let lent = shader_passes(&shader(&[("FORWARD", "ForwardBase", 2.0)], "Diffuse"));
    assert_eq!(lent.len(), 2);
    assert_eq!(
        (lent[1].light_mode.as_str(), lent[1].cull, lent[1].from_fallback),
        ("SHADOWCASTER", 2, true)
    );
    // Unlit/Texture has none to lend, and an own pass is never doubled.
    assert_eq!(
        shader_passes(&shader(&[("FORWARD", "ForwardBase", 2.0)], "Unlit/Texture")).len(),
        1
    );
    assert_eq!(
        shader_passes(&shader(&[("SHADOWCASTER", "ShadowCaster", 0.0)], "Diffuse")).len(),
        1
    );
    assert!(fallback_casts_shadows("Legacy Shaders/VertexLit"));
    assert!(!fallback_casts_shadows(""));
}
