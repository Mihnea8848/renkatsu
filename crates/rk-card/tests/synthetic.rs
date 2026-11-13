// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parser tests on card files built in memory, so they run without a game
//! install.

use rk_card::{lz4_msgpack, Card, CardError, CardKind, CoordinateFile, Value};

fn map(pairs: Vec<(&str, Value)>) -> Value {
    Value::Map(pairs.into_iter().map(|(k, v)| (Value::from(k), v)).collect())
}

fn mp(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    rmpv::encode::write_value(&mut out, v).unwrap();
    out
}

fn floats(v: &[f32]) -> Value {
    Value::Array(v.iter().map(|f| Value::F32(*f)).collect())
}

/// .NET BinaryWriter string.
fn net_string(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut n = s.len();
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        out.push(if n > 0 { b | 0x80 } else { b });
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(s.as_bytes());
    out
}

fn len_prefixed(b: &[u8]) -> Vec<u8> {
    let mut out = (b.len() as i32).to_le_bytes().to_vec();
    out.extend_from_slice(b);
    out
}

/// Smallest chunk sequence the PNG walker accepts.
fn tiny_png() -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, data) in [(b"IHDR", vec![0u8; 13]), (b"IEND", vec![])] {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(&data);
        out.extend_from_slice(&[0; 4]);
    }
    out
}

fn coordinate_bytes(top_id: i64) -> Vec<u8> {
    let parts: Vec<Value> = (0..9)
        .map(|i| {
            map(vec![
                ("id", Value::from(if i == 0 { top_id } else { 0 })),
                ("colorInfo", Value::Nil),
            ])
        })
        .collect();
    let clothes = map(vec![
        ("version", "0.0.1".into()),
        ("parts", Value::Array(parts)),
        ("subPartsId", Value::Array(vec![1.into(), 2.into(), 3.into()])),
    ]);
    let moves: Vec<Value> = (0..6).map(|i| floats(&[i as f32, 0.0, 1.0])).collect();
    let acc = map(vec![
        ("type", 122.into()),
        ("id", 5.into()),
        ("parentKey", "a_n_headtop".into()),
        ("addMove", Value::Array(vec![2.into(), 3.into(), Value::Array(moves)])),
    ]);
    let empty = map(vec![("type", 120.into()), ("id", 0.into())]);
    let accessory = map(vec![
        ("version", "0.0.2".into()),
        ("parts", Value::Array(vec![acc, empty])),
    ]);
    let makeup = map(vec![("version", "0.0.0".into()), ("lipId", 2.into())]);
    let mut out = len_prefixed(&mp(&clothes));
    out.extend(len_prefixed(&mp(&accessory)));
    out.push(1);
    out.extend(len_prefixed(&mp(&makeup)));
    out
}

fn ext_block() -> Vec<u8> {
    let nested = mp(&map(vec![("inner", 7.into())]));
    let plugin = Value::Array(vec![
        3.into(),
        map(vec![("blob", Value::Binary(nested)), ("flag", true.into())]),
    ]);
    mp(&map(vec![
        ("test.plugin", plugin),
        ("empty.plugin", Value::Array(vec![1.into(), Value::Nil])),
    ]))
}

fn card_bytes(with_image: bool, marker: &str) -> Vec<u8> {
    let face = map(vec![
        ("version", "0.0.2".into()),
        ("headId", 4.into()),
        ("shapeValueFace", floats(&[0.5; 52])),
        (
            "pupil",
            Value::Array(vec![map(vec![("id", 18.into())]), map(vec![("id", 19.into())])]),
        ),
        ("moleLayout", floats(&[0.1, 0.2, 0.3, 0.4])),
        ("baseMakeup", Value::Nil),
        ("doubleTooth", true.into()),
    ]);
    let body = map(vec![
        ("shapeValueBody", floats(&[0.25; 44])),
        ("skinMainColor", floats(&[1.0, 0.8, 0.7, 1.0])),
        ("typeBone", 1.into()),
    ]);
    let hair_parts: Vec<Value> = [13, 35, 0, 2]
        .iter()
        .map(|id| map(vec![("id", (*id).into())]))
        .collect();
    let hair = map(vec![("parts", Value::Array(hair_parts)), ("glossId", 3.into())]);
    let mut custom = len_prefixed(&mp(&face));
    custom.extend(len_prefixed(&mp(&body)));
    custom.extend(len_prefixed(&mp(&hair)));

    let coordinate = mp(&Value::Array(vec![
        Value::Binary(coordinate_bytes(105)),
        Value::Binary(coordinate_bytes(7)),
    ]));
    let parameter = mp(&map(vec![
        ("sex", 1.into()),
        ("lastname", "Honma".into()),
        ("firstname", "Yukie".into()),
    ]));
    let status = mp(&map(vec![("shoesType", 0.into())]));
    let blocks = [
        ("Custom", custom),
        ("Coordinate", coordinate),
        ("Parameter", parameter),
        ("Status", status),
        ("KKEx", ext_block()),
    ];

    let mut infos = Vec::new();
    let mut data = Vec::new();
    for (name, bytes) in &blocks {
        infos.push(map(vec![
            ("name", (*name).into()),
            ("version", "0.0.0".into()),
            ("pos", (data.len() as i64).into()),
            ("size", (bytes.len() as i64).into()),
        ]));
        data.extend_from_slice(bytes);
    }
    let header = mp(&map(vec![("lstInfo", Value::Array(infos))]));

    let mut out = if with_image { tiny_png() } else { Vec::new() };
    out.extend_from_slice(&100i32.to_le_bytes());
    out.extend(net_string(marker));
    out.extend(net_string("0.0.0"));
    out.extend(len_prefixed(b"face-image"));
    out.extend(len_prefixed(&header));
    out.extend_from_slice(&(data.len() as i64).to_le_bytes());
    out.extend(data);
    out
}

#[test]
fn card_fields_round_trip() {
    let bytes = card_bytes(true, "【KoiKatuChara】");
    let card = Card::parse(&bytes).unwrap();
    assert_eq!(card.kind, CardKind::Koikatsu);
    assert_eq!(card.image_len, tiny_png().len());
    assert_eq!(card.face_png, b"face-image");
    assert_eq!(card.blocks.len(), 5);
    assert_eq!(card.face.head_id, 4);
    assert_eq!(card.face.shape_value_face.len(), 52);
    assert_eq!(card.face.pupil[1].id, 19);
    assert_eq!(card.face.mole_layout, [0.1, 0.2, 0.3, 0.4]);
    assert!(card.face.double_tooth);
    // A nil nested object falls back to defaults.
    assert_eq!(card.face.base_makeup.lip_id, 0);
    assert_eq!(card.body.type_bone, 1);
    assert_eq!(card.body.skin_main_color[1], 0.8);
    assert_eq!(card.hair.parts.iter().map(|p| p.id).collect::<Vec<_>>(), [13, 35, 0, 2]);
    assert_eq!(card.full_name(), "Honma Yukie");
    assert_eq!(card.sex(), 1);
}

#[test]
fn coordinates_and_accessories_parse() {
    let card = Card::parse(&card_bytes(true, "【KoiKatuChara】")).unwrap();
    assert_eq!(card.coordinates.len(), 2);
    let co = &card.coordinates[0];
    assert_eq!(co.clothes.parts.len(), 9);
    assert_eq!(co.clothes.parts[0].id, 105);
    assert!(co.clothes.parts[0].color_info.is_empty());
    assert_eq!(co.clothes.sub_parts_id, [1, 2, 3]);
    assert!(co.enable_makeup);
    assert_eq!(co.makeup.lip_id, 2);
    let acc = &co.accessory.parts[0];
    assert!(!acc.is_empty());
    assert_eq!((acc.kind, acc.id, acc.parent_key.as_str()), (122, 5, "a_n_headtop"));
    // addMove is a 2x3 grid: [target][position, rotation, scale].
    assert_eq!(acc.add_move.get(1, 2), Some(&[5.0, 0.0, 1.0]));
    assert_eq!(acc.add_move.get(2, 0), None);
    assert!(co.accessory.parts[1].is_empty());
    assert_eq!(card.coordinates[1].clothes.parts[0].id, 7);
}

#[test]
fn plugin_data_is_exposed() {
    let card = Card::parse(&card_bytes(true, "【KoiKatuChara】")).unwrap();
    let p = &card.ext["test.plugin"];
    assert_eq!(p.version, 3);
    assert_eq!(p.data["flag"], Value::Boolean(true));
    let nested = p.nested("blob").unwrap();
    assert_eq!(rk_card::map_get(&nested, "inner").and_then(Value::as_i64), Some(7));
    assert!(card.ext["empty.plugin"].data.is_empty());
}

#[test]
fn exported_card_without_image_parses() {
    let card = Card::parse(&card_bytes(false, "【KoiKatuChara】")).unwrap();
    assert_eq!(card.image_len, 0);
    assert_eq!(card.face.head_id, 4);
}

#[test]
fn marker_variants_and_errors() {
    assert_eq!(
        Card::parse(&card_bytes(true, "【KoiKatuCharaSP】")).unwrap().kind,
        CardKind::KoikatsuPartySpecial
    );
    assert!(matches!(
        Card::parse(&card_bytes(true, "【AIS_Chara】")),
        Err(CardError::Marker(_))
    ));
    assert!(matches!(Card::parse(&tiny_png()), Err(CardError::NoTrailer)));
    let mut wrong_product = tiny_png();
    wrong_product.extend_from_slice(&0x4747_4747i32.to_le_bytes());
    assert!(matches!(Card::parse(&wrong_product), Err(CardError::Product(_))));
    // Truncation anywhere must be an error, never a panic.
    let full = card_bytes(true, "【KoiKatuChara】");
    for cut in (tiny_png().len() + 1..full.len()).step_by(7) {
        assert!(Card::parse(&full[..cut]).is_err(), "truncated at {cut}");
    }
}

#[test]
fn coordinate_file_with_plugin_data() {
    let mut bytes = tiny_png();
    bytes.extend_from_slice(&100i32.to_le_bytes());
    bytes.extend(net_string("【KoiKatuClothes】"));
    bytes.extend(net_string("0.0.0"));
    bytes.extend(net_string("Uniform"));
    bytes.extend(len_prefixed(&coordinate_bytes(42)));
    let without_ext = bytes.clone();
    bytes.extend(net_string("KKEx"));
    bytes.extend_from_slice(&3i32.to_le_bytes());
    bytes.extend(len_prefixed(&ext_block()));

    let file = CoordinateFile::parse(&bytes).unwrap();
    assert_eq!(file.name, "Uniform");
    assert_eq!(file.coordinate.clothes.parts[0].id, 42);
    assert_eq!(file.ext["test.plugin"].version, 3);
    assert!(CoordinateFile::parse(&without_ext).unwrap().ext.is_empty());
}

#[test]
fn lz4_msgpack_plain_and_compressed() {
    let value = map(vec![(
        "bones",
        Value::Array((0..200).map(|i| Value::from(i % 3)).collect()),
    )]);
    let plain = mp(&value);
    assert_eq!(lz4_msgpack(&plain), Some(value.clone()));

    // Extension 99: msgpack int32 with the plain size, then an LZ4 block.
    let mut payload = vec![0xd2];
    payload.extend_from_slice(&(plain.len() as i32).to_be_bytes());
    payload.extend(lz4_flex::block::compress(&plain));
    assert_eq!(lz4_msgpack(&mp(&Value::Ext(99, payload))), Some(value));

    assert_eq!(lz4_msgpack(&mp(&Value::Ext(99, vec![0xd2, 0, 0, 1, 0, 0xff]))), None);
}
