// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Maps Unity materials to Renkatsu's material kinds and render settings.

use bytemuck::{Pod, Zeroable};
use rk_chara::{Image, MaterialInst};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Clothes, accessories and everything unknown.
    Item = 1,
    Skin = 2,
    Hair = 3,
    /// Iris with its highlights.
    Eye = 4,
    /// Flat textured layers on the face: eye whites, brows, lashes, nose line.
    Overlay = 5,
}

pub const HAS_MAIN: u32 = 1;
pub const HAS_COLOR_MASK: u32 = 2;
pub const HAS_DETAIL: u32 = 4;
pub const HAS_LINE: u32 = 8;
pub const HAS_ALPHA_MASK: u32 = 16;
pub const HAS_OVER1: u32 = 32;
pub const HAS_OVER2: u32 = 64;
pub const BLENDED: u32 = 128;

/// Texture slots in bind order.
pub const SLOTS: [(&str, u32); 7] = [
    ("_MainTex", HAS_MAIN),
    ("_ColorMask", HAS_COLOR_MASK),
    ("_DetailMask", HAS_DETAIL),
    ("_LineMask", HAS_LINE),
    ("_AlphaMask", HAS_ALPHA_MASK),
    ("_overtex1", HAS_OVER1),
    ("_overtex2", HAS_OVER2),
];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct MaterialUniform {
    pub color1: [f32; 4],
    pub color2: [f32; 4],
    pub color3: [f32; 4],
    pub shadow: [f32; 4],
    pub over1: [f32; 4],
    pub over2: [f32; 4],
    pub st_main: [f32; 4],
    pub p0: [f32; 4],
    pub p1: [f32; 4],
    pub p2: [f32; 4],
    pub info: [u32; 4],
}

pub struct MaterialDesc {
    pub kind: Kind,
    /// Unity render queue: lower draws first.
    pub queue: i32,
    pub blended: bool,
    pub outline: bool,
    pub uniform: MaterialUniform,
    pub textures: [Option<Arc<Image>>; 7],
}

/// Material kind from a Unity shader name such as `Shader Forge/main_skin`.
/// Mod shaders follow the same naming, so matching is by substring.
pub fn classify(shader: &str) -> Kind {
    let name = shader.rsplit('/').next().unwrap_or(shader).to_lowercase();
    if name.contains("eyew") || name.contains("nose") || name.contains("eyebrow") || name.contains("eyelash") {
        Kind::Overlay
    } else if name.contains("eye") {
        Kind::Eye
    } else if name.contains("skin") || name.contains("body") || name.contains("face") {
        Kind::Skin
    } else if name.contains("hair") {
        Kind::Hair
    } else {
        Kind::Item
    }
}

fn is_blended(shader: &str, kind: Kind) -> bool {
    let name = shader.rsplit('/').next().unwrap_or(shader).to_lowercase();
    matches!(kind, Kind::Eye | Kind::Overlay) || name.contains("alpha") || name.contains("glass")
}

impl MaterialDesc {
    pub fn new(mat: Option<&MaterialInst>) -> Self {
        let empty = MaterialInst::default();
        let mat = mat.unwrap_or(&empty);
        let kind = classify(&mat.shader);
        let blended = is_blended(&mat.shader, kind);
        let color = |name: &str, default: [f32; 4]| mat.colors.get(name).copied().unwrap_or(default);
        let float = |name: &str, default: f32| mat.floats.get(name).copied().unwrap_or(default);

        let mut flags = if blended { BLENDED } else { 0 };
        let mut textures: [Option<Arc<Image>>; 7] = Default::default();
        let mut st_main = [1.0, 1.0, 0.0, 0.0];
        for (i, (name, flag)) in SLOTS.iter().enumerate() {
            if let Some(slot) = mat.textures.get(*name) {
                textures[i] = Some(slot.image.clone());
                flags |= flag;
                if i == 0 {
                    st_main = [slot.scale[0], slot.scale[1], slot.offset[0], slot.offset[1]];
                }
            }
        }

        // Skin gets its texture built at load time. Until then use a plain tone.
        let mut color1 = color("_Color", [1.0; 4]);
        if kind == Kind::Skin && flags & HAS_MAIN == 0 {
            color1 = [1.0, 0.88, 0.83, 1.0];
        }
        let cutoff = match kind {
            Kind::Overlay | Kind::Eye => 0.02,
            _ if blended => 0.02,
            _ => float("_Cutoff", 0.5).clamp(0.01, 1.0),
        };
        let uniform = MaterialUniform {
            color1,
            color2: color("_Color2", [1.0; 4]),
            color3: color("_Color3", [1.0; 4]),
            shadow: color("_ShadowColor", [0.628, 0.628, 0.628, 1.0]),
            over1: color("_overcolor1", [1.0; 4]),
            over2: color("_overcolor2", [1.0; 4]),
            st_main,
            p0: [
                cutoff,
                float("_ShadowExtend", 1.0),
                float("_ShadowExtendAnother", 1.0),
                float("_DetailRLineR", 0.0),
            ],
            p1: [
                float("_DetailBLineG", 0.0),
                float("_LineWidthS", 1.0),
                float("_Alpha", 1.0),
                0.0,
            ],
            p2: [float("_alpha_a", 1.0), float("_alpha_b", 1.0), 0.0, 0.0],
            info: [kind as u32, flags, 0, 0],
        };
        let default_queue = match kind {
            Kind::Skin => 2350,
            Kind::Overlay => 2999,
            Kind::Eye => 3000,
            _ if blended => 3040,
            _ => 2450,
        };
        Self {
            kind,
            queue: if mat.render_queue > 0 {
                mat.render_queue
            } else {
                default_queue
            },
            blended,
            outline: !blended && matches!(kind, Kind::Item | Kind::Skin | Kind::Hair),
            uniform,
            textures,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_names_map_to_kinds() {
        for (name, kind) in [
            ("Shader Forge/main_skin", Kind::Skin),
            ("Shader Forge/main_hair", Kind::Hair),
            ("Shader Forge/main_hair_front", Kind::Hair),
            ("Shader Forge/main_item", Kind::Item),
            ("Shader Forge/main_opaque", Kind::Item),
            ("Shader Forge/main_alpha", Kind::Item),
            ("Shader Forge/main_emblem", Kind::Item),
            ("Shader Forge/toon_eye_lod0", Kind::Eye),
            ("Shader Forge/toon_eyew_lod0", Kind::Overlay),
            ("Shader Forge/toon_nose_lod0", Kind::Overlay),
            ("xukmi/SkinPlus", Kind::Skin),
            ("xukmi/HairPlus", Kind::Hair),
            ("xukmi/MainOpaquePlus", Kind::Item),
            ("Standard", Kind::Item),
            ("", Kind::Item),
        ] {
            assert_eq!(classify(name), kind, "{name}");
        }
    }

    #[test]
    fn render_settings_follow_the_kind() {
        let mat = |shader: &str, queue: i32| MaterialInst {
            shader: shader.into(),
            render_queue: queue,
            ..Default::default()
        };
        let skin = MaterialDesc::new(Some(&mat("Shader Forge/main_skin", -1)));
        assert_eq!((skin.queue, skin.blended, skin.outline), (2350, false, true));
        let brow = MaterialDesc::new(Some(&mat("Shader Forge/toon_eyew_lod0", 2460)));
        assert_eq!((brow.queue, brow.blended, brow.outline), (2460, true, false));
        let sheer = MaterialDesc::new(Some(&mat("Shader Forge/main_alpha", -1)));
        assert_eq!((sheer.queue, sheer.blended, sheer.outline), (3040, true, false));
        let none = MaterialDesc::new(None);
        assert_eq!((none.kind, none.uniform.info[1]), (Kind::Item, 0));
    }

    #[test]
    fn uniform_matches_the_shader_layout() {
        // WGSL: ten vec4<f32> and one vec4<u32>.
        assert_eq!(std::mem::size_of::<MaterialUniform>(), 176);
    }
}
