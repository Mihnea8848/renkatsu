// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Maps Unity materials to Renkatsu's material kinds and render settings.

use bytemuck::{Pod, Zeroable};
use rk_chara::{Blend, DrawState, Image, MaterialInst, ShadowPass, Stencil};
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
/// The shadow caster pass is the fallback shader's: nothing is cut out.
pub const CASTER_FALLBACK: u32 = 512;

/// Which of the game's clothes shaders an item follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemShader {
    /// main_item and everything unknown.
    Other = 0,
    /// main_opaque
    Opaque = 1,
    /// main_alpha
    Alpha = 2,
    /// main_item_studio_alpha, which MaterialEditor carries.
    StudioAlpha = 3,
    /// toon_glasses_lod0: the lens of glasses.
    Glasses = 4,
}

/// Clothes shader from a Unity shader name. Mod shaders follow the game's
/// naming (`xukmi/MainAlphaPlus`), so matching is by substring.
pub fn item_shader(shader: &str) -> ItemShader {
    // Whole names, since main_opaque_low is another shader. KKShadersPlus stands in for the two under its own names.
    let name = shader.rsplit('/').next().unwrap_or(shader).to_lowercase();
    match name.as_str() {
        "main_alpha" => ItemShader::Alpha,
        "main_opaque" | "main_opaque2" => ItemShader::Opaque,
        "main_item_studio_alpha" => ItemShader::StudioAlpha,
        "toon_glasses_lod0" => ItemShader::Glasses,
        n if n.starts_with("mainalphaplus") => ItemShader::Alpha,
        n if n.starts_with("mainopaqueplus") => ItemShader::Opaque,
        _ => ItemShader::Other,
    }
}

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
    /// The shader writes an alpha that the blend uses.
    pub blended: bool,
    /// The blend the surface is drawn with.
    pub blend: Blend,
    pub outline: bool,
    pub zwrite: bool,
    pub stencil: Stencil,
    pub stencil_ref: u8,
    /// The shader's shadow caster pass.
    pub shadow: ShadowPass,
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
        blend: Blend::Off,
        zwrite: true,
        outline: true,
        stencil: Stencil::None,
        stencil_ref: 0,
        shadow: ShadowPass::TwoSided,
    };
    // toon_nose, toon_eye and toon_glasses have no shadow pass of their own
    // and take the one of their fallback.
    let layer = DrawState {
        queue: 3000,
        blend: Blend::Alpha,
        zwrite: false,
        outline: false,
        shadow: ShadowPass::Fallback,
        ..solid
    };
    match kind {
        // toon_nose
        Kind::Overlay if name.contains("nose") => layer,
        // toon_eye and toon_eyew mark their pixels with 2. The shadow pass
        // of toon_eyew leaves back faces out.
        Kind::Eye | Kind::Overlay => DrawState {
            stencil: Stencil::Write,
            stencil_ref: 2,
            shadow: if kind == Kind::Eye {
                ShadowPass::Fallback
            } else {
                ShadowPass::CullBack
            },
            ..layer
        },
        // main_hair_front leaves pixels marked 2 alone.
        Kind::Hair if name.contains("front") => DrawState {
            queue: 3040,
            blend: Blend::Alpha,
            stencil: Stencil::SkipEqual,
            stencil_ref: 2,
            ..solid
        },
        // main_skin
        Kind::Skin => DrawState { queue: 2350, ..solid },
        // toon_glasses
        _ if name.contains("glass") => DrawState {
            queue: 4010,
            blend: Blend::Premultiplied,
            outline: false,
            shadow: ShadowPass::Fallback,
            ..solid
        },
        // main_alpha
        _ if name.contains("alpha") => DrawState {
            queue: 3040,
            blend: Blend::Alpha,
            ..solid
        },
        _ => solid,
    }
}

/// sRGB to linear for one channel.
pub fn srgb_to_linear(x: f32) -> f32 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// A colour as Unity hands it to shaders in linear colour space: the colour
/// channels converted, alpha left as it is.
pub fn linear(c: [f32; 4]) -> [f32; 4] {
    [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), c[3]]
}

impl MaterialDesc {
    pub fn new(mat: Option<&MaterialInst>) -> Self {
        let empty = MaterialInst::default();
        let mat = mat.unwrap_or(&empty);
        let kind = classify(&mat.shader);
        let state = mat.draw.unwrap_or_else(|| state_for_name(&mat.shader, kind));
        // The clothes shaders have their own defaults and texture slots.
        let item = if kind == Kind::Item {
            item_shader(&mat.shader)
        } else {
            ItemShader::Other
        };
        let glasses = item == ItemShader::Glasses;
        // One with OneMinusSrcAlpha is drawn as such for the lens, which
        // scales its colour by its alpha. main_skin, main_opaque and
        // main_emblem have the same factors and write an alpha of one, which
        // is no blending. main_item writes twice its cut alpha minus one;
        // that is not followed yet and drawn solid.
        let blend = match state.blend {
            Blend::Premultiplied if !glasses => Blend::Off,
            other => other,
        };
        let blended = blend != Blend::Off;
        let cloth = matches!(item, ItemShader::Opaque | ItemShader::Alpha);
        let sheer = item == ItemShader::Alpha;
        let studio = item == ItemShader::StudioAlpha;
        // Shaders whose normal map and second ramp take the layer slots.
        let mapped = cloth || studio || glasses;
        // Colour properties reach the shader linearised; vector properties,
        // which the material stores beside them, do not go through this.
        let color = |name: &str, default: [f32; 4]| linear(mat.colors.get(name).copied().unwrap_or(default));
        let float = |name: &str, default: f32| mat.floats.get(name).copied().unwrap_or(default);

        let mut flags = if blended { BLENDED } else { 0 };
        if state.shadow == ShadowPass::Fallback {
            flags |= CASTER_FALLBACK;
        }
        let mut textures: [Option<Arc<Image>>; 8] = Default::default();
        let mut st_main = [1.0, 1.0, 0.0, 0.0];
        let mut st_alpha = st_main;
        let mut st_gloss = st_main;
        let mut st_over = [st_main; 2];
        let mut st_normal = st_main;
        let mut st_ramp = st_main;
        for (i, (name, flag)) in SLOTS.iter().enumerate() {
            // Hair has no skin layers; its gloss mask takes the first layer slot.
            // Clothes have none either; their normal map and second ramp
            // take the first two.
            let name = match *name {
                "_overtex1" if kind == Kind::Hair => "_HairGloss",
                "_overtex1" if mapped => "_NormalMap",
                "_overtex2" if glasses => "_GlassRamp",
                "_overtex2" if mapped => "_AnotherRamp",
                // Not properties of main_item_studio_alpha.
                "_AlphaMask" | "_overtex3" if studio => continue,
                // toon_glasses has the main texture, the normal map and
                // the ramp only.
                "_ColorMask" | "_DetailMask" | "_LineMask" | "_AlphaMask" | "_overtex3" if glasses => continue,
                other => other,
            };
            if let Some(slot) = mat.textures.get(name) {
                textures[i] = Some(slot.image.clone());
                flags |= flag;
                let st = [slot.scale[0], slot.scale[1], slot.offset[0], slot.offset[1]];
                match name {
                    "_MainTex" => st_main = st,
                    "_AlphaMask" => st_alpha = st,
                    "_HairGloss" => st_gloss = st,
                    "_overtex1" => st_over[0] = st,
                    "_overtex2" => st_over[1] = st,
                    "_NormalMap" => st_normal = st,
                    "_GlassRamp" => st_ramp = st,
                    _ => {}
                }
            }
        }

        // Skin gets its texture built at load time. Until then use a plain tone.
        // main_item_studio_alpha declares its own default for the three colours.
        let tint = if studio {
            [0.960_784_3, 0.960_784_3, 0.960_784_3, 1.0]
        } else {
            [1.0; 4]
        };
        let mut color1 = color("_Color", tint);
        if kind == Kind::Skin && flags & HAS_MAIN == 0 {
            color1 = linear([1.0, 0.88, 0.83, 1.0]);
        }
        let cutoff = match kind {
            Kind::Overlay | Kind::Eye => 0.02,
            // The hair shaders cut at one half even when they blend.
            Kind::Hair => 0.5,
            _ if blended => 0.05,
            // main_opaque cuts at one half whatever the material says.
            _ if cloth => 0.5,
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
            } else if glasses {
                // The lens colour; its alpha is how solid the lens is.
                color("_Color4", [0.501_960_8, 0.501_960_8, 0.501_960_8, 1.0])
            } else {
                color("_Color2", tint)
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
                color("_Color3", tint)
            },
            // The face layer shaders spell it in lower case.
            shadow: match kind {
                Kind::Eye | Kind::Overlay => color("_shadowcolor", [0.628, 0.628, 0.628, 1.0]),
                // toon_glasses declares its own default.
                _ if glasses => color("_ShadowColor", [0.694_348_3, 0.757_679_5, 0.823_529_4, 1.0]),
                _ => color("_ShadowColor", [0.628, 0.628, 0.628, 1.0]),
            },
            over1: if cloth {
                color("_SpecularColor", [1.0, 1.0, 1.0, 0.0])
            } else if glasses {
                [
                    float("_ColorInverse", 0.0),
                    float("_ColorSort", 0.0),
                    float("_RimPower", 1.0),
                    0.0,
                ]
            } else if studio {
                [
                    float("_AnotherRampFull", 0.0),
                    float("_EmissionPower", 0.0),
                    float("_ambientshadowOFF", 0.0),
                    float("_alpha", 1.0),
                ]
            } else {
                color("_overcolor1", [1.0; 4])
            },
            // Hair: scale and offset of the gloss mask.
            over2: if kind == Kind::Hair {
                st_gloss
            } else if glasses {
                // Scale and offset of the normal map.
                st_normal
            } else if mapped {
                [
                    float("_SpeclarHeight", if sheer { 0.985 } else { 0.98 }),
                    float("_SpecularPower", 0.0),
                    float("_SpecularPowerNail", 0.0),
                    float("_notusetexspecular", 0.0),
                ]
            } else {
                color("_overcolor2", [1.0; 4])
            },
            over3: if mapped {
                [
                    float("_rimpower", 0.5),
                    float("_rimV", if sheer { 0.75 } else { 0.5 }),
                    float("_outline", 1.0),
                    0.0,
                ]
            } else {
                color("_overcolor3", [1.0; 4])
            },
            st_main,
            // The lens has no alpha mask; its ramp's placement goes here.
            st_alpha: if glasses { st_ramp } else { st_alpha },
            p0: [
                cutoff,
                float("_ShadowExtend", if sheer { 0.5 } else { 1.0 }),
                float("_ShadowExtendAnother", if cloth { 0.0 } else { 1.0 }),
                float("_DetailRLineR", 0.0),
            ],
            p1: [
                float(if sheer { "_DetailBLineGA" } else { "_DetailBLineG" }, 0.0),
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
            info: [kind as u32, flags, item as u32, 0],
        };
        Self {
            kind,
            queue: if mat.render_queue > 0 {
                mat.render_queue
            } else {
                state.queue
            },
            blended,
            blend,
            outline: state.outline && matches!(kind, Kind::Item | Kind::Skin | Kind::Hair),
            zwrite: state.zwrite,
            stencil: state.stencil,
            stencil_ref: state.stencil_ref,
            shadow: state.shadow,
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
            blend: Blend::Alpha,
            zwrite: false,
            outline: false,
            stencil: Stencil::Write,
            stencil_ref: 7,
            shadow: ShadowPass::CullBack,
        });
        let d = MaterialDesc::new(Some(&read));
        assert_eq!(
            (d.queue, d.blended, d.zwrite, d.outline, d.stencil, d.stencil_ref),
            (3100, true, false, false, Stencil::Write, 7)
        );
        assert_eq!(
            (d.shadow, d.uniform.info[1] & CASTER_FALLBACK),
            (ShadowPass::CullBack, 0)
        );
        // Guessed from the name: the game shader's own pass, or its fallback's.
        assert_eq!(skin.shadow, ShadowPass::TwoSided);
        assert_eq!(iris.shadow, ShadowPass::Fallback);
        assert_eq!(iris.uniform.info[1] & CASTER_FALLBACK, CASTER_FALLBACK);
        assert_eq!(nose.shadow, ShadowPass::Fallback);
        let brow = MaterialDesc::new(Some(&mat("Shader Forge/toon_eyew_lod0", -1)));
        assert_eq!(brow.shadow, ShadowPass::CullBack);
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
        assert_eq!(set.color2, linear([0.8, 0.7, 0.6, 0.5]));
        assert_eq!(set.color3, [0.9, 0.3, 0.4, 0.25]);
        assert_eq!(set.line, [0.0, 1.0, 0.7, 1.0]);
    }

    #[test]
    fn clothes_follow_their_game_shader() {
        for (name, shader) in [
            ("Shader Forge/main_alpha", ItemShader::Alpha),
            ("Shader Forge/main_opaque", ItemShader::Opaque),
            ("xukmi/MainAlphaPlus", ItemShader::Alpha),
            ("xukmi/MainOpaquePlus", ItemShader::Opaque),
            ("Shader Forge/main_item", ItemShader::Other),
            ("Shader Forge/main_item_studio_alpha", ItemShader::StudioAlpha),
            ("Shader Forge/main_opaque_low", ItemShader::Other),
            ("Standard", ItemShader::Other),
        ] {
            assert_eq!(item_shader(name), shader, "{name}");
        }
        let image = |srgb| {
            Arc::new(Image {
                name: "t".into(),
                width: 1,
                height: 1,
                rgba: vec![255; 4],
                srgb,
                repeat: false,
            })
        };
        let slot = |srgb| rk_chara::TexSlot {
            image: image(srgb),
            scale: [1.0, 1.0],
            offset: [0.0, 0.0],
        };

        // Without values: the defaults each shader declares.
        let mut sheer = MaterialInst {
            shader: "Shader Forge/main_alpha".into(),
            ..Default::default()
        };
        let u = MaterialDesc::new(Some(&sheer)).uniform;
        assert_eq!(u.info[2], ItemShader::Alpha as u32);
        assert_eq!(u.over1, [1.0, 1.0, 1.0, 0.0]);
        assert_eq!(u.over2, [0.985, 0.0, 0.0, 0.0]);
        assert_eq!(u.over3, [0.5, 0.75, 1.0, 0.0]);
        assert_eq!(u.p0, [0.05, 0.5, 0.0, 0.0]);
        let solid = MaterialInst {
            shader: "Shader Forge/main_opaque".into(),
            ..Default::default()
        };
        let u = MaterialDesc::new(Some(&solid)).uniform;
        assert_eq!(u.info[2], ItemShader::Opaque as u32);
        assert_eq!(u.over2, [0.98, 0.0, 0.0, 0.0]);
        assert_eq!(u.over3, [0.5, 0.5, 1.0, 0.0]);
        assert_eq!(u.p0, [0.5, 1.0, 0.0, 0.0]);

        // Material values and textures reach the shader.
        sheer.colors.insert("_SpecularColor".into(), [0.9, 0.8, 0.7, 0.3]);
        for (name, value) in [
            ("_SpeclarHeight", 0.96),
            ("_SpecularPower", 0.2),
            ("_rimpower", 0.4),
            ("_rimV", 0.2),
            ("_outline", 0.0),
            ("_ShadowExtend", 0.8),
            ("_DetailBLineGA", 1.0),
            ("_DetailBLineG", 0.5),
        ] {
            sheer.floats.insert(name.into(), value);
        }
        sheer.textures.insert("_MainTex".into(), slot(true));
        sheer.textures.insert("_NormalMap".into(), slot(false));
        sheer.textures.insert("_AnotherRamp".into(), slot(false));
        // Not a property of the shader: left out.
        sheer.textures.insert("_overtex1".into(), slot(false));
        let d = MaterialDesc::new(Some(&sheer));
        // A colour property arrives linearised; its alpha does not.
        let spec = d.uniform.over1;
        assert!((spec[0] - 0.7874).abs() < 1e-4 && (spec[2] - 0.4479).abs() < 1e-4);
        assert_eq!(spec[3], 0.3);
        assert_eq!(d.uniform.over2, [0.96, 0.2, 0.0, 0.0]);
        assert_eq!(d.uniform.over3, [0.4, 0.2, 0.0, 0.0]);
        assert_eq!((d.uniform.p0[1], d.uniform.p1[0]), (0.8, 1.0));
        assert_eq!(d.uniform.info[1], BLENDED | HAS_MAIN | HAS_OVER1 | HAS_OVER2);
        assert!(d.textures[5].is_some() && d.textures[6].is_some() && d.textures[7].is_none());

        // main_item_studio_alpha: the defaults its property list declares.
        let mut studio = MaterialInst {
            shader: "Shader Forge/main_item_studio_alpha".into(),
            ..Default::default()
        };
        let u = MaterialDesc::new(Some(&studio)).uniform;
        assert_eq!(u.info[2], ItemShader::StudioAlpha as u32);
        let grey = srgb_to_linear(0.960_784_3);
        assert_eq!(u.color1, [grey, grey, grey, 1.0]);
        assert_eq!((u.color2, u.color3), (u.color1, u.color1));
        assert_eq!(u.over1, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(u.over2, [0.98, 0.0, 0.0, 0.0]);
        assert_eq!(u.over3[..2], [0.5, 0.5]);
        assert_eq!(u.p0[1..], [1.0, 1.0, 0.0]);
        assert_eq!(u.p1[0], 0.0);
        // Its values and textures reach the shader; an alpha mask left over
        // from the shader the material had before does not.
        studio.colors.insert("_Color2".into(), [0.5, 0.5, 0.5, 0.0]);
        for (name, value) in [
            ("_AnotherRampFull", 1.0),
            ("_EmissionPower", 0.25),
            ("_ambientshadowOFF", 1.0),
            ("_alpha", 0.9),
            ("_Alpha", 0.1),
            ("_ShadowExtendAnother", 0.0),
            ("_notusetexspecular", 1.0),
        ] {
            studio.floats.insert(name.into(), value);
        }
        for name in [
            "_MainTex",
            "_ColorMask",
            "_NormalMap",
            "_AnotherRamp",
            "_AlphaMask",
            "_overtex3",
        ] {
            studio.textures.insert(name.into(), slot(false));
        }
        let d = MaterialDesc::new(Some(&studio));
        assert_eq!(d.uniform.over1, [1.0, 0.25, 1.0, 0.9]);
        assert!((d.uniform.color2[0] - 0.214_041_14).abs() < 1e-6);
        assert_eq!((d.uniform.p0[2], d.uniform.over2[3]), (0.0, 1.0));
        assert_eq!(
            d.uniform.info[1],
            BLENDED | HAS_MAIN | HAS_COLOR_MASK | HAS_OVER1 | HAS_OVER2
        );
        assert!(d.textures[4].is_none() && d.textures[7].is_none());

        // Other items keep their layer colours and ignore the normal map.
        let mut other = MaterialInst {
            shader: "Shader Forge/main_item".into(),
            ..Default::default()
        };
        other.textures.insert("_NormalMap".into(), slot(false));
        other.textures.insert("_MainTex".into(), slot(false));
        let d = MaterialDesc::new(Some(&other));
        assert_eq!((d.uniform.info[1], d.uniform.info[2]), (HAS_MAIN, 0));
        assert_eq!(d.uniform.over1, [1.0; 4]);
    }

    #[test]
    fn the_lens_of_glasses_follows_toon_glasses() {
        assert_eq!(item_shader("Shader Forge/toon_glasses_lod0"), ItemShader::Glasses);
        let pass = |blend| DrawState {
            queue: 4010,
            blend,
            zwrite: true,
            outline: false,
            stencil: Stencil::None,
            stencil_ref: 0,
            shadow: ShadowPass::Fallback,
        };
        let mut lens = MaterialInst {
            shader: "Shader Forge/toon_glasses_lod0".into(),
            draw: Some(pass(Blend::Premultiplied)),
            ..Default::default()
        };
        // Without values: the defaults the shader declares.
        let d = MaterialDesc::new(Some(&lens));
        assert_eq!(
            (d.queue, d.blend, d.blended, d.zwrite),
            (4010, Blend::Premultiplied, true, true)
        );
        let u = d.uniform;
        assert_eq!(u.info[1], BLENDED | CASTER_FALLBACK);
        assert_eq!(u.info[2], ItemShader::Glasses as u32);
        assert_eq!(u.color1, [1.0; 4]);
        let grey = srgb_to_linear(0.501_960_8);
        assert_eq!(u.color2, [grey, grey, grey, 1.0]);
        assert_eq!(u.shadow, linear([0.694_348_3, 0.757_679_5, 0.823_529_4, 1.0]));
        assert_eq!(u.over1, [0.0, 0.0, 1.0, 0.0]);

        // Values and the three textures reach the shader with their
        // placement; textures of other shaders do not.
        lens.colors.insert("_Color4".into(), [0.0, 0.5, 1.0, 0.125]);
        lens.colors.insert("_Color".into(), [0.5, 0.5, 0.5, 1.0]);
        for (name, value) in [("_ColorInverse", 0.25), ("_ColorSort", 0.3), ("_RimPower", 9.0)] {
            lens.floats.insert(name.into(), value);
        }
        let slot = |scale: f32| rk_chara::TexSlot {
            image: Arc::new(Image {
                name: "t".into(),
                width: 1,
                height: 1,
                rgba: vec![255; 4],
                srgb: false,
                repeat: true,
            }),
            scale: [scale, scale],
            offset: [0.5, 0.0],
        };
        lens.textures.insert("_GlassRamp".into(), slot(2.0));
        lens.textures.insert("_NormalMap".into(), slot(3.0));
        lens.textures.insert("_AlphaMask".into(), slot(1.0));
        lens.textures.insert("_DetailMask".into(), slot(1.0));
        let d = MaterialDesc::new(Some(&lens));
        let u = d.uniform;
        assert_eq!(u.info[1], BLENDED | CASTER_FALLBACK | HAS_OVER1 | HAS_OVER2);
        assert_eq!(u.color2, [0.0, srgb_to_linear(0.5), 1.0, 0.125]);
        assert_eq!(u.over1, [0.25, 0.3, 9.0, 0.0]);
        assert_eq!(u.over2, [3.0, 3.0, 0.5, 0.0]);
        assert_eq!(u.st_alpha, [2.0, 2.0, 0.5, 0.0]);
        assert!(d.textures[2].is_none() && d.textures[4].is_none());

        // A shader that plugins carry is known by its name.
        lens.draw = None;
        let d = MaterialDesc::new(Some(&lens));
        assert_eq!(
            (d.queue, d.blend, d.shadow),
            (4010, Blend::Premultiplied, ShadowPass::Fallback)
        );

        // The other shaders with these factors write an alpha of one and
        // are drawn solid.
        for name in [
            "Shader Forge/main_skin",
            "Shader Forge/main_item",
            "Shader Forge/main_opaque",
        ] {
            let solid = MaterialInst {
                shader: name.into(),
                draw: Some(pass(Blend::Premultiplied)),
                ..Default::default()
            };
            let d = MaterialDesc::new(Some(&solid));
            assert_eq!(
                (d.blend, d.blended, d.uniform.info[1] & BLENDED),
                (Blend::Off, false, 0),
                "{name}"
            );
        }
        let sheer = MaterialInst {
            shader: "Shader Forge/main_alpha".into(),
            draw: Some(pass(Blend::Alpha)),
            ..Default::default()
        };
        assert_eq!(MaterialDesc::new(Some(&sheer)).blend, Blend::Alpha);
    }

    #[test]
    fn colours_are_linearised_but_not_vectors() {
        assert_eq!(linear([0.0, 1.0, 0.04045, 0.5]), [0.0, 1.0, 0.04045 / 12.92, 0.5]);
        assert!((srgb_to_linear(0.5) - 0.214_041_14).abs() < 1e-6);
        let mut eye = MaterialInst {
            shader: "Shader Forge/toon_eye_lod0".into(),
            ..Default::default()
        };
        eye.colors.insert("_shadowcolor".into(), [0.5, 0.5, 0.5, 1.0]);
        eye.floats.insert("_rotation".into(), 0.5);
        let u = MaterialDesc::new(Some(&eye)).uniform;
        assert!((u.shadow[0] - 0.214_041_14).abs() < 1e-6);
        assert_eq!(u.shadow[3], 1.0);
        // The highlight placement is not a colour and stays as stored.
        assert_eq!(u.color2, [1.0, 1.0, 0.0, 0.0]);
        assert_eq!(u.p3[1], 0.5);
    }

    #[test]
    fn uniform_matches_the_shader_layout() {
        // WGSL: thirteen vec4<f32> and one vec4<u32>.
        assert_eq!(std::mem::size_of::<MaterialUniform>(), 240);
    }
}
