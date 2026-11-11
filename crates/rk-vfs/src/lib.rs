// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Virtual file system over the game's `abdata` folder and all zipmods,
//! plus the item database and card ID resolution.

pub mod assets;
pub mod index;
pub mod items;
pub mod resolve;

pub use assets::{AssetFile, Assets, Game, LoadedBundle, ObjRef};
pub use index::{Index, ModInfo};
pub use items::{resolve_infos, Item, ItemDb, Lookup, ResolveInfo};

/// Item list categories (`ChaListDefine.CategoryNo` in the game).
pub mod cat {
    pub const BO_HEAD: i32 = 100;
    pub const BO_HAIR_B: i32 = 101;
    pub const BO_HAIR_F: i32 = 102;
    pub const BO_HAIR_S: i32 = 103;
    pub const BO_HAIR_O: i32 = 104;
    pub const CO_TOP: i32 = 105;
    pub const CO_BOT: i32 = 106;
    pub const CO_BRA: i32 = 107;
    pub const CO_SHORTS: i32 = 108;
    pub const CO_GLOVES: i32 = 109;
    pub const CO_PANST: i32 = 110;
    pub const CO_SOCKS: i32 = 111;
    pub const CO_SHOES: i32 = 112;
    pub const AO_NONE: i32 = 120;
    pub const CPO_SAILOR_A: i32 = 200;
    pub const CPO_SAILOR_B: i32 = 201;
    pub const CPO_SAILOR_C: i32 = 202;
    pub const CPO_JACKET_A: i32 = 210;
    pub const CPO_JACKET_B: i32 = 211;
    pub const CPO_JACKET_C: i32 = 212;
    pub const MT_FACE_DETAIL: i32 = 400;
    pub const MT_EYESHADOW: i32 = 401;
    pub const MT_CHEEK: i32 = 402;
    pub const MT_LIP: i32 = 403;
    pub const MT_LIPLINE: i32 = 404;
    pub const MT_FACE_PAINT: i32 = 405;
    pub const MT_EYEBROW: i32 = 406;
    pub const MT_EYE_WHITE: i32 = 407;
    pub const MT_EYE: i32 = 408;
    pub const MT_EYE_GRADATION: i32 = 409;
    pub const MT_EYE_HI_UP: i32 = 410;
    pub const MT_EYE_HI_DOWN: i32 = 411;
    pub const MT_EYELINE_UP: i32 = 412;
    pub const MT_EYELINE_DOWN: i32 = 413;
    pub const MT_NOSE: i32 = 414;
    pub const MT_MOLE: i32 = 415;
    pub const MT_BODY_DETAIL: i32 = 420;
    pub const MT_BODY_PAINT: i32 = 421;
    pub const MT_SUNBURN: i32 = 422;
    pub const MT_NIP: i32 = 423;
    pub const MT_UNDERHAIR: i32 = 424;
    pub const MT_PATTERN: i32 = 430;
    pub const MT_EMBLEM: i32 = 431;
    pub const MT_RAMP: i32 = 432;
    pub const MT_HAIRGLOSS: i32 = 433;

    const NAMES: &[(&str, i32)] = &[
        ("cha_sample_f", 0),
        ("cha_sample_m", 1),
        ("cha_eyeset", 2),
        ("bodypaint_layout", 3),
        ("cha_default_cos_f", 4),
        ("cha_default_cos_m", 5),
        ("cha_sample_voice", 6),
        ("facepaint_layout", 7),
        ("mole_layout", 8),
        ("cha_sample_m_ex", 9),
        ("bo_head", 100),
        ("bo_hair_b", 101),
        ("bo_hair_f", 102),
        ("bo_hair_s", 103),
        ("bo_hair_o", 104),
        ("co_top", 105),
        ("co_bot", 106),
        ("co_bra", 107),
        ("co_shorts", 108),
        ("co_gloves", 109),
        ("co_panst", 110),
        ("co_socks", 111),
        ("co_shoes", 112),
        ("ao_none", 120),
        ("ao_hair", 121),
        ("ao_head", 122),
        ("ao_face", 123),
        ("ao_neck", 124),
        ("ao_body", 125),
        ("ao_waist", 126),
        ("ao_leg", 127),
        ("ao_arm", 128),
        ("ao_hand", 129),
        ("ao_kokan", 130),
        ("cpo_sailor_a", 200),
        ("cpo_sailor_b", 201),
        ("cpo_sailor_c", 202),
        ("cpo_jacket_a", 210),
        ("cpo_jacket_b", 211),
        ("cpo_jacket_c", 212),
        ("mt_face_detail", 400),
        ("mt_eyeshadow", 401),
        ("mt_cheek", 402),
        ("mt_lip", 403),
        ("mt_lipline", 404),
        ("mt_face_paint", 405),
        ("mt_eyebrow", 406),
        ("mt_eye_white", 407),
        ("mt_eye", 408),
        ("mt_eye_gradation", 409),
        ("mt_eye_hi_up", 410),
        ("mt_eye_hi_down", 411),
        ("mt_eyeline_up", 412),
        ("mt_eyeline_down", 413),
        ("mt_nose", 414),
        ("mt_mole", 415),
        ("face_preset", 416),
        ("mt_body_detail", 420),
        ("mt_body_paint", 421),
        ("mt_sunburn", 422),
        ("mt_nip", 423),
        ("mt_underhair", 424),
        ("mt_pattern", 430),
        ("mt_emblem", 431),
        ("mt_ramp", 432),
        ("mt_hairgloss", 433),
        ("ex_bo_body", 500),
        ("ex_bo_head", 501),
        ("ex_bo_hair", 502),
        ("ex_co_clothes", 503),
        ("ex_co_shoes", 504),
    ];

    /// Category number from its name (`ao_head`) or its decimal form.
    pub fn from_name(s: &str) -> Option<i32> {
        s.parse()
            .ok()
            .or_else(|| NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(s)).map(|(_, v)| *v))
    }

    pub fn name(category: i32) -> &'static str {
        NAMES
            .iter()
            .find(|(_, v)| *v == category)
            .map(|(n, _)| *n)
            .unwrap_or("?")
    }

    /// Categories of the nine clothing slots, in card order.
    pub const CLOTHES: [i32; 9] = [
        CO_TOP, CO_BOT, CO_BRA, CO_SHORTS, CO_GLOVES, CO_PANST, CO_SOCKS, CO_SHOES, CO_SHOES,
    ];
    /// Sideloader property names of the nine clothing slots.
    pub const CLOTHES_PROPS: [&str; 9] = [
        "ClothesTop",
        "ClothesBot",
        "ClothesBra",
        "ClothesShorts",
        "ClothesGloves",
        "ClothesPants",
        "ClothesSocks",
        "ClothesShoesInner",
        "ClothesShoesOuter",
    ];
    pub const HAIR: [i32; 4] = [BO_HAIR_B, BO_HAIR_F, BO_HAIR_S, BO_HAIR_O];
    pub const HAIR_PROPS: [&str; 4] = ["HairBack", "HairFront", "HairSide", "HairOption"];
}
