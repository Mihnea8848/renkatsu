// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed views of the card's MessagePack blocks. Field names follow the
//! game's own (camelCase on the wire). Unknown fields are ignored and missing
//! or nil ones fall back to defaults, so cards from older versions load.

use serde::{Deserialize, Deserializer};

pub type Color = [f32; 4];
pub type Vec2 = [f32; 2];
pub type Vec3 = [f32; 3];
pub type Vec4 = [f32; 4];

pub const WHITE: Color = [1.0, 1.0, 1.0, 1.0];

fn nil<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

fn white() -> Color {
    WHITE
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BlockHeader {
    #[serde(default, deserialize_with = "nil")]
    pub lst_info: Vec<BlockHeaderInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BlockHeaderInfo {
    pub name: String,
    #[serde(default, deserialize_with = "nil")]
    pub version: String,
    pub pos: i64,
    pub size: i64,
}

/// `T[,]` as MessagePack-CSharp writes it: `[rows, cols, [flat items]]`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Array2<T>(pub usize, pub usize, pub Vec<T>);

impl<T> Array2<T> {
    pub fn get(&self, row: usize, col: usize) -> Option<&T> {
        if row < self.0 && col < self.1 {
            self.2.get(row * self.1 + col)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PupilInfo {
    pub id: i32,
    pub base_color: Color,
    pub sub_color: Color,
    pub grad_mask_id: i32,
    pub grad_blend: f32,
    pub grad_offset_y: f32,
    pub grad_scale: f32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Makeup {
    #[serde(deserialize_with = "nil")]
    pub version: String,
    pub eyeshadow_id: i32,
    pub eyeshadow_color: Color,
    pub cheek_id: i32,
    pub cheek_color: Color,
    pub lip_id: i32,
    pub lip_color: Color,
    #[serde(deserialize_with = "nil")]
    pub paint_id: Vec<i32>,
    #[serde(deserialize_with = "nil")]
    pub paint_color: Vec<Color>,
    #[serde(deserialize_with = "nil")]
    pub paint_layout: Vec<Vec4>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Face {
    #[serde(deserialize_with = "nil")]
    pub version: String,
    #[serde(deserialize_with = "nil")]
    pub shape_value_face: Vec<f32>,
    pub head_id: i32,
    pub skin_id: i32,
    pub detail_id: i32,
    pub detail_power: f32,
    pub cheek_gloss_power: f32,
    pub eyebrow_id: i32,
    pub eyebrow_color: Color,
    pub nose_id: i32,
    #[serde(deserialize_with = "nil")]
    pub pupil: Vec<PupilInfo>,
    pub hl_up_id: i32,
    pub hl_up_color: Color,
    pub hl_down_id: i32,
    pub hl_down_color: Color,
    pub white_id: i32,
    pub white_base_color: Color,
    pub white_sub_color: Color,
    pub pupil_width: f32,
    pub pupil_height: f32,
    pub pupil_x: f32,
    pub pupil_y: f32,
    pub hl_up_y: f32,
    pub hl_down_y: f32,
    pub eyeline_up_id: i32,
    pub eyeline_up_weight: f32,
    pub eyeline_down_id: i32,
    pub eyeline_color: Color,
    pub mole_id: i32,
    pub mole_color: Color,
    pub mole_layout: Vec4,
    pub lip_line_id: i32,
    pub lip_line_color: Color,
    pub lip_gloss_power: f32,
    pub double_tooth: bool,
    #[serde(deserialize_with = "nil")]
    pub base_makeup: Makeup,
    pub foreground_eyes: u8,
    pub foreground_eyebrow: u8,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Body {
    #[serde(deserialize_with = "nil")]
    pub version: String,
    #[serde(deserialize_with = "nil")]
    pub shape_value_body: Vec<f32>,
    pub bust_softness: f32,
    pub bust_weight: f32,
    pub skin_id: i32,
    pub detail_id: i32,
    pub detail_power: f32,
    pub skin_main_color: Color,
    pub skin_sub_color: Color,
    pub skin_gloss_power: f32,
    #[serde(deserialize_with = "nil")]
    pub paint_id: Vec<i32>,
    #[serde(deserialize_with = "nil")]
    pub paint_color: Vec<Color>,
    #[serde(deserialize_with = "nil")]
    pub paint_layout_id: Vec<i32>,
    #[serde(deserialize_with = "nil")]
    pub paint_layout: Vec<Vec4>,
    pub sunburn_id: i32,
    pub sunburn_color: Color,
    pub nip_id: i32,
    pub nip_color: Color,
    pub nip_gloss_power: f32,
    pub areola_size: f32,
    pub underhair_id: i32,
    pub underhair_color: Color,
    pub nail_color: Color,
    pub nail_gloss_power: f32,
    pub draw_add_line: bool,
    pub type_bone: i32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HairPart {
    pub id: i32,
    pub base_color: Color,
    pub start_color: Color,
    pub end_color: Color,
    pub length: f32,
    pub pos: Vec3,
    pub rot: Vec3,
    pub scl: Vec3,
    #[serde(deserialize_with = "nil")]
    pub acs_color: Vec<Color>,
    pub outline_color: Color,
    pub no_shake: bool,
}

/// Hair part slots, in card order.
pub const HAIR_BACK: usize = 0;
pub const HAIR_FRONT: usize = 1;
pub const HAIR_SIDE: usize = 2;
pub const HAIR_OPTION: usize = 3;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Hair {
    #[serde(deserialize_with = "nil")]
    pub version: String,
    #[serde(deserialize_with = "nil")]
    pub parts: Vec<HairPart>,
    pub kind: i32,
    pub gloss_id: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClothesColor {
    pub base_color: Color,
    pub pattern: i32,
    pub tiling: Vec2,
    pub pattern_color: Color,
}

impl Default for ClothesColor {
    fn default() -> Self {
        Self {
            base_color: WHITE,
            pattern: 0,
            tiling: [0.0; 2],
            pattern_color: WHITE,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClothesPart {
    pub id: i32,
    #[serde(deserialize_with = "nil")]
    pub color_info: Vec<ClothesColor>,
    pub embleme_id: i32,
    pub embleme_id2: i32,
    #[serde(deserialize_with = "nil")]
    pub hide_opt: Vec<bool>,
    pub sleeves_type: i32,
}

/// Clothing part slots, in card order.
pub const CLOTHES_KIND_NAMES: [&str; 9] = [
    "top",
    "bottom",
    "bra",
    "shorts",
    "gloves",
    "panst",
    "socks",
    "shoes_inner",
    "shoes_outer",
];

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Clothes {
    #[serde(deserialize_with = "nil")]
    pub version: String,
    #[serde(deserialize_with = "nil")]
    pub parts: Vec<ClothesPart>,
    /// Sub-parts used when the top is a composite (sailor/jacket) type.
    #[serde(deserialize_with = "nil")]
    pub sub_parts_id: Vec<i32>,
    #[serde(deserialize_with = "nil")]
    pub hide_bra_opt: Vec<bool>,
    #[serde(deserialize_with = "nil")]
    pub hide_shorts_opt: Vec<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccessoryPart {
    /// Item category number (120 = none).
    #[serde(rename = "type")]
    pub kind: i32,
    pub id: i32,
    #[serde(deserialize_with = "nil")]
    pub parent_key: String,
    /// `[2, 3]`: two move targets, each position / rotation / scale.
    #[serde(deserialize_with = "nil")]
    pub add_move: Array2<Vec3>,
    #[serde(deserialize_with = "nil")]
    pub color: Vec<Color>,
    pub hide_category: i32,
    pub no_shake: bool,
}

impl Default for AccessoryPart {
    fn default() -> Self {
        Self {
            kind: 120,
            id: 0,
            parent_key: String::new(),
            add_move: Array2::default(),
            color: vec![white(); 4],
            hide_category: 0,
            no_shake: false,
        }
    }
}

impl AccessoryPart {
    /// True for unused slots (category "none", or the -1 some plugins write).
    pub fn is_empty(&self) -> bool {
        self.kind <= 120
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Accessories {
    #[serde(deserialize_with = "nil")]
    pub version: String,
    #[serde(deserialize_with = "nil")]
    pub parts: Vec<AccessoryPart>,
}

#[derive(Debug, Clone, Default)]
pub struct Coordinate {
    pub clothes: Clothes,
    pub accessory: Accessories,
    pub enable_makeup: bool,
    pub makeup: Makeup,
}
