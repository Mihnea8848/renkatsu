// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Maps Unity materials to Renkatsu's material kinds and render settings.

use bytemuck::{Pod, Zeroable};
use rk_chara::{DrawState, Image, MaterialInst, Stencil};
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
pub const HAS_OVER3: u32 = 256;

/// Texture slots in bind order.
pub const SLOTS: [(&str, u32); 8] = [
    ("_MainTex", HAS_MAIN),
    ("_ColorMask", HAS_COLOR_MASK),
    ("_DetailMask", HAS_DETAIL),
    ("_LineMask", HAS_LINE),
    ("_AlphaMask", HAS_ALPHA_MASK),
    ("_overtex1", HAS_OVER1),
    ("_overtex2", HAS_OVER2),
    ("_overtex3", HAS_OVER3),
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
    pub over3: [f32; 4],
    pub st_main: [f32; 4],
    pub st_alpha: [f32; 4],
    pub p0: [f32; 4],
    pub p1: [f32; 4],
    pub p2: [f32; 4],
    pub p3: [f32; 4],
    /// The material's own outline colour (hair).
    pub line: [f32; 4],
    pub info: [u32; 4],
}

pub struct MaterialDesc {
    pub kind: Kind,
    /// Unity render queue: lower draws first.
    pub queue: i32,
    pub blended: bool,
    pub outline: bool,
    pub zwrite: bool,
    pub stencil: Stencil,
    pub stencil_ref: u8,
    pub uniform: MaterialUniform,
    pub textures: [Option<Arc<Image>>; 8],
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

/// Draw state for a shader that could not be read, guessed from its name.
/// Plugins carry their shaders inside their own files and name them after
/// the game shader they stand in for, so each guess is the state of that
/// game shader.
pub fn state_for_name(shader: &str, kind: Kind) -> DrawState {
    let name = shader.rsplit('/').next().unwrap_or(shader).to_lowercase();
    let solid = DrawState {
        queue: 2450,
        blend: false,
        zwrite: true,
        outline: true,
        stencil: Stencil::None,
        stencil_ref: 0,
    };
    let layer = DrawState {
        queue: 3000,
        blend: true,
        zwrite: false,
        outline: false,
        ..solid
    };
    match kind {
        // toon_nose
        Kind::Overlay if name.contains("nose") => layer,
        // toon_eye and toon_eyew mark their pixels with 2.
        Kind::Eye | Kind::Overlay => DrawState {
            stencil: Stencil::Write,
            stencil_ref: 2,
            ..layer
        },
        // main_hair_front leaves pixels marked 2 alone.
        Kind::Hair if name.contains("front") => DrawState {
            queue: 3040,
            blend: true,
            stencil: Stencil::SkipEqual,
            stencil_ref: 2,
            ..solid
        },
        // main_skin
        Kind::Skin => DrawState { queue: 2350, ..solid },
        // toon_glasses
        _ if name.contains("glass") => DrawState {
            queue: 4010,
            blend: true,
            outline: false,
            ..solid
        },
        // main_alpha
        _ if name.contains("alpha") => DrawState {
            queue: 3040,
            blend: true,
            ..solid
        },
        _ => solid,
    }
}

impl MaterialDesc {
    pub fn new(mat: Option<&MaterialInst>) -> Self {
        let empty = MaterialInst::default();
        let mat = mat.unwrap_or(&empty);
        let kind = classify(&mat.shader);
        let state = mat.draw.unwrap_or_else(|| state_for_name(&mat.shader, kind));
        let blended = state.blend;
        let color = |name: &str, default: [f32; 4]| mat.colors.get(name).copied().unwrap_or(default);
        let float = |name: &str, default: f32| mat.floats.get(name).copied().unwrap_or(default);

        let mut flags = if blended { BLENDED } else { 0 };
        let mut textures: [Option<Arc<Image>>; 8] = Default::default();
        let mut st_main = [1.0, 1.0, 0.0, 0.0];
        let mut st_alpha = st_main;
        let mut st_gloss = st_main;
        let mut st_over = [st_main; 2];
        for (i, (name, flag)) in SLOTS.iter().enumerate() {
            // Hair has no skin layers; its gloss mask takes the first layer slot.
            let name = if kind == Kind::Hair && *name == "_overtex1" {
                &"_HairGloss"
            } else {
                name
            };
            if let Some(slot) = mat.textures.get(*name) {
                textures[i] = Some(slot.image.clone());
                flags |= flag;
                let st = [slot.scale[0], slot.scale[1], slot.offset[0], slot.offset[1]];
                match *name {
                    "_MainTex" => st_main = st,
                    "_AlphaMask" => st_alpha = st,
                    "_HairGloss" => st_gloss = st,
                    "_overtex1" => st_over[0] = st,
                    "_overtex2" => st_over[1] = st,
                    _ => {}
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
            // The hair shaders cut at one half even when they blend.
            Kind::Hair => 0.5,
            _ if blended => 0.05,
            _ => float("_Cutoff", 0.5).clamp(0.01, 1.0),
        };
        let uniform = MaterialUniform {
            color1,
            // The iris has no second and third colour; these hold the scale
            // and offset of its two highlights.
            color2: if kind == Kind::Eye {
                st_over[0]
            } else if kind == Kind::Skin {
                color("_SpecularColor", [0.0; 4])
            } else {
                color("_Color2", [1.0; 4])
            },
            color3: if kind == Kind::Eye {
                st_over[1]
            } else if kind == Kind::Skin {
                [
                    float("_SpeclarHeight", 0.98),
                    float("_SpecularPower", 0.0),
                    float("_SpecularPowerNail", 0.0),
                    float("_DetailNormalMapScale", 1.0),
                ]
            } else {
                color("_Color3", [1.0; 4])
            },
            // The face layer shaders spell it in lower case.
            shadow: match kind {
                Kind::Eye | Kind::Overlay => color("_shadowcolor", [0.628, 0.628, 0.628, 1.0]),
                _ => color("_ShadowColor", [0.628, 0.628, 0.628, 1.0]),
            },
            over1: color("_overcolor1", [1.0; 4]),
            // Hair: scale and offset of the gloss mask.
            over2: if kind == Kind::Hair {
                st_gloss
            } else {
                color("_overcolor2", [1.0; 4])
            },
            over3: color("_overcolor3", [1.0; 4]),
            st_main,
            st_alpha,
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
            p3: if kind == Kind::Hair {
                let front = mat.shader.to_lowercase().contains("front");
                [
                    float("_rimpower", 0.5),
                    float("_rimV", 0.75),
                    float("_SpeclarHeight", 0.85),
                    if front { 1.0 } else { 0.0 },
                ]
            } else if kind == Kind::Eye {
                // The game switches highlights on unless the character hides
                // them; the stored value is not used.
                [1.0, float("_rotation", 0.0), 0.0, 0.0]
            } else if kind == Kind::Overlay {
                let nose = mat.shader.to_lowercase().contains("nose");
                [if nose { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0]
            } else {
                [float("_nip", 0.0), float("_nipsize", 0.5), 0.0, 0.0]
            },
            line: if kind == Kind::Skin {
                [
                    float("_linetexon", 1.0),
                    float("_notusetexspecular", 0.0),
                    float("_nip_specular", 0.5),
                    float("_tex1mask", 0.0),
                ]
            } else {
                color("_LineColor", [0.5, 0.5, 0.5, 1.0])
            },
            info: [kind as u32, flags, 0, 0],
        };
        Self {
            kind,
            queue: if mat.render_queue > 0 {
                mat.render_queue
            } else {
                state.queue
            },
            blended,
            outline: state.outline && matches!(kind, Kind::Item | Kind::Skin | Kind::Hair),
            zwrite: state.zwrite,
            stencil: state.stencil,
            stencil_ref: state.stencil_ref,
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
        // The game's see-through cloth shader has an outline pass and writes depth.
        assert_eq!(
            (sheer.queue, sheer.blended, sheer.outline, sheer.zwrite),
            (3040, true, true, true)
        );
        let fringe = MaterialDesc::new(Some(&mat("xukmi/HairFrontPlus", -1)));
        assert_eq!(
            (fringe.queue, fringe.blended, fringe.stencil, fringe.stencil_ref),
            (3040, true, Stencil::SkipEqual, 2)
        );
        let iris = MaterialDesc::new(Some(&mat("Shader Forge/toon_eye_lod0", -1)));
        assert_eq!(
            (iris.queue, iris.zwrite, iris.stencil, iris.stencil_ref),
            (3000, false, Stencil::Write, 2)
        );
        let nose = MaterialDesc::new(Some(&mat("Shader Forge/toon_nose_lod0", -1)));
        assert_eq!((nose.stencil, nose.uniform.p3[0]), (Stencil::None, 1.0));

        // A state read from the shader wins over the name.
        let mut read = mat("Shader Forge/main_opaque", -1);
        read.draw = Some(DrawState {
            queue: 3100,
            blend: true,
            zwrite: false,
            outline: false,
            stencil: Stencil::Write,
            stencil_ref: 7,
        });
        let d = MaterialDesc::new(Some(&read));
        assert_eq!(
            (d.queue, d.blended, d.zwrite, d.outline, d.stencil, d.stencil_ref),
            (3100, true, false, false, Stencil::Write, 7)
        );
        let none = MaterialDesc::new(None);
        assert_eq!((none.kind, none.uniform.info[1]), (Kind::Item, 0));
    }

    #[test]
    fn skin_carries_its_shine_and_detail_values() {
        let mut mat = MaterialInst {
            shader: "Shader Forge/main_skin".into(),
            ..Default::default()
        };
        // Without values: no shine, lines on, full detail strength.
        let plain = MaterialDesc::new(Some(&mat)).uniform;
        assert_eq!(plain.color2, [0.0; 4]);
        assert_eq!(plain.color3, [0.98, 0.0, 0.0, 1.0]);
        assert_eq!(plain.line, [1.0, 0.0, 0.5, 0.0]);

        mat.colors.insert("_SpecularColor".into(), [0.8, 0.7, 0.6, 0.5]);
        for (name, value) in [
            ("_SpeclarHeight", 0.9),
            ("_SpecularPower", 0.3),
            ("_SpecularPowerNail", 0.4),
            ("_DetailNormalMapScale", 0.25),
            ("_linetexon", 0.0),
            ("_notusetexspecular", 1.0),
            ("_nip_specular", 0.7),
            ("_tex1mask", 1.0),
        ] {
            mat.floats.insert(name.into(), value);
        }
        let set = MaterialDesc::new(Some(&mat)).uniform;
        assert_eq!(set.color2, [0.8, 0.7, 0.6, 0.5]);
        assert_eq!(set.color3, [0.9, 0.3, 0.4, 0.25]);
        assert_eq!(set.line, [0.0, 1.0, 0.7, 1.0]);
    }

    #[test]
    fn uniform_matches_the_shader_layout() {
        // WGSL: thirteen vec4<f32> and one vec4<u32>.
        assert_eq!(std::mem::size_of::<MaterialUniform>(), 240);
    }
}
