// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

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
/// main_item_studio's third pattern mask is bound (the ninth slot).
pub const HAS_PATTERN3: u32 = 1024;
/// The tenth and eleventh slot, which the KKShadersPlus materials use.
pub const HAS_OVER5: u32 = 2048;
pub const HAS_OVER6: u32 = 4096;
/// The twelfth slot: main_skin's liquid mask.
pub const HAS_OVER7: u32 = 8192;

/// Which of the game's clothes shaders an item follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemShader {
    /// Everything unknown.
    Other = 0,
    /// main_opaque
    Opaque = 1,
    /// main_alpha
    Alpha = 2,
    /// main_item_studio_alpha, which MaterialEditor carries.
    StudioAlpha = 3,
    /// toon_glasses_lod0: the lens of glasses.
    Glasses = 4,
    /// main_item: accessories.
    Main = 5,
    /// main_emblem: emblems on clothes.
    Emblem = 6,
    /// main_item_emission: accessories that glow.
    Emission = 7,
    /// main_item_ditherd: cut out by a dither pattern.
    Ditherd = 8,
    /// main_item_studio: accessories with patterns.
    Studio = 9,
    /// main_texture: main_item without light direction.
    Texture = 10,
    /// main_opaque_low
    OpaqueLow = 11,
    /// main_color: one flat colour.
    Color = 12,
}

/// Clothes shader from a Unity shader name. The KKShadersPlus shaders
/// (`xukmi/MainAlphaPlus`) have their own ports, see `xukmi.rs`.
pub fn item_shader(shader: &str) -> ItemShader {
    // Whole names, since main_opaque_low is another shader.
    let name = shader.rsplit('/').next().unwrap_or(shader).to_lowercase();
    match name.as_str() {
        "main_alpha" => ItemShader::Alpha,
        "main_opaque" | "main_opaque2" => ItemShader::Opaque,
        "main_item_studio_alpha" => ItemShader::StudioAlpha,
        "toon_glasses_lod0" => ItemShader::Glasses,
        "main_item" => ItemShader::Main,
        "main_emblem" => ItemShader::Emblem,
        "main_item_emission" => ItemShader::Emission,
        "main_item_ditherd" => ItemShader::Ditherd,
        "main_item_studio" => ItemShader::Studio,
        "main_texture" => ItemShader::Texture,
        "main_opaque_low" => ItemShader::OpaqueLow,
        "main_color" => ItemShader::Color,
        // xukmi's Studio variants are not ported; they extend these game
        // shaders, which are the closest stand-ins.
        n if n.starts_with("mainitemstudioalphaplus") => ItemShader::StudioAlpha,
        n if n.starts_with("mainitemstudioplus") => ItemShader::Studio,
        _ => ItemShader::Other,
    }
}

/// Shaders whose pixel program is ported from the game's own disassembly.
/// Everything else goes through an approximation; `rk audit` reports how
/// many materials that leaves.
pub const PORTED: [&str; 41] = [
    "Shader Forge/main_skin",
    "Shader Forge/main_hair",
    "Shader Forge/main_hair_front",
    "Shader Forge/toon_eye_lod0",
    "Shader Forge/toon_eyew_lod0",
    "Shader Forge/toon_nose_lod0",
    "Shader Forge/main_opaque",
    "Shader Forge/main_opaque2",
    "Shader Forge/main_alpha",
    "Shader Forge/main_item",
    "Shader Forge/main_item_studio_alpha",
    "Shader Forge/toon_glasses_lod0",
    "Shader Forge/main_emblem",
    "Shader Forge/main_item_emission",
    "Shader Forge/main_item_ditherd",
    "Shader Forge/main_item_studio",
    "Shader Forge/main_texture",
    "Shader Forge/main_opaque_low",
    "Shader Forge/main_color",
    // KKShadersPlus, ported from the programs of Vanilla Plus 1.7.2.
    "xukmi/MainOpaquePlus",
    "xukmi/MainOpaquePlusTess",
    "xukmi/MainAlphaPlus",
    "xukmi/MainAlphaPlusTess",
    "xukmi/MainItemPlus",
    "xukmi/MainItemPlusTess",
    "xukmi/MainItemAlphaPlus",
    "xukmi/MainItemAlphaPlusTess",
    "xukmi/SkinPlus",
    "xukmi/SkinPlusTess",
    "xukmi/SkinPlusReflect",
    "xukmi/SkinPlusTessReflect",
    "xukmi/HairPlus",
    "xukmi/HairPlusTess",
    "xukmi/HairPlusReflect",
    "xukmi/HairPlusTessReflect",
    "xukmi/HairFrontPlus",
    "xukmi/HairFrontPlusTess",
    "xukmi/HairFrontPlusReflect",
    "xukmi/HairFrontPlusTessReflect",
    "xukmi/EyePlus",
    "xukmi/EyeWPlus",
];

/// Texture slots in bind order.
pub const SLOTS: [(&str, u32); 12] = [
    ("_MainTex", HAS_MAIN),
    ("_ColorMask", HAS_COLOR_MASK),
    ("_DetailMask", HAS_DETAIL),
    ("_LineMask", HAS_LINE),
    ("_AlphaMask", HAS_ALPHA_MASK),
    ("_overtex1", HAS_OVER1),
    ("_overtex2", HAS_OVER2),
    ("_overtex3", HAS_OVER3),
    ("_PatternMask3", HAS_PATTERN3),
    // The KKShadersPlus materials and main_skin fill these two.
    ("_NormalMap", HAS_OVER5),
    ("_NormalMapDetail", HAS_OVER6),
    // main_skin alone.
    ("_liquidmask", HAS_OVER7),
];

/// Vectors in the `ext` block of the material uniform.
pub const EXT: usize = 24;

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
    /// What the later item shaders need beside the above; toon.wgsl and
    /// xukmi.wgsl say what each keeps where.
    pub ext: [[f32; 4]; EXT],
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
    pub textures: [Option<Arc<Image>>; SLOTS.len()],
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
        // main_item, main_item_emission and main_item_studio: One with
        // OneMinusSrcAlpha.
        _ if matches!(name.as_str(), "main_item" | "main_item_emission" | "main_item_studio") => DrawState {
            blend: Blend::Premultiplied,
            ..solid
        },
        // main_texture: the same without an outline pass.
        _ if name == "main_texture" => DrawState {
            blend: Blend::Premultiplied,
            outline: false,
            ..solid
        },
        // main_emblem: its passes cull back faces.
        _ if name == "main_emblem" => DrawState {
            blend: Blend::Premultiplied,
            outline: false,
            shadow: ShadowPass::CullBack,
            ..solid
        },
        // main_opaque_low and main_item_ditherd name no queue.
        _ if name == "main_opaque_low" => DrawState {
            queue: 2000,
            blend: Blend::Premultiplied,
            outline: false,
            ..solid
        },
        _ if name == "main_item_ditherd" => DrawState {
            queue: 2000,
            outline: false,
            ..solid
        },
        // main_color has a forward pass only; shadows come from its
        // fallback.
        _ if name == "main_color" => DrawState {
            queue: 2000,
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
        if let Some(x) = crate::xukmi::shader(&mat.shader) {
            return crate::xukmi::desc(mat, x);
        }
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
        // is no blending. main_item writes twice its cut alpha minus one
        // beside a colour it does not scale.
        // main_item_emission, main_item_studio and main_texture write the
        // same alpha and share main_item's slots.
        let emission = item == ItemShader::Emission;
        let patterned = item == ItemShader::Studio;
        let accessory = matches!(item, ItemShader::Main | ItemShader::Texture) || emission || patterned;
        let low = item == ItemShader::OpaqueLow;
        let blend = match state.blend {
            Blend::Premultiplied if !glasses && !accessory => Blend::Off,
            other => other,
        };
        let blended = blend != Blend::Off;
        let cloth = matches!(item, ItemShader::Opaque | ItemShader::Alpha);
        let sheer = item == ItemShader::Alpha;
        let studio = item == ItemShader::StudioAlpha;
        // Shaders whose normal map and second ramp take the layer slots.
        let mapped = cloth || studio || glasses || accessory || low;
        // Colour properties reach the shader linearised; vector properties,
        // which the material stores beside them, do not go through this.
        let color = |name: &str, default: [f32; 4]| linear(mat.colors.get(name).copied().unwrap_or(default));
        let float = |name: &str, default: f32| mat.floats.get(name).copied().unwrap_or(default);

        let mut flags = if blended { BLENDED } else { 0 };
        if state.shadow == ShadowPass::Fallback {
            flags |= CASTER_FALLBACK;
        }
        let mut textures: [Option<Arc<Image>>; SLOTS.len()] = Default::default();
        let mut st_main = [1.0, 1.0, 0.0, 0.0];
        let mut st_alpha = st_main;
        let mut st_gloss = st_main;
        let mut st_over = [st_main; 2];
        let mut st_normal = st_main;
        let mut st_ramp = st_main;
        let mut st_color_mask = st_main;
        let mut st_detail = st_main;
        let mut st_line = st_main;
        // The animation mask or first pattern mask, then the other two.
        let mut st_extra = [st_main; 3];
        // main_skin: the detail normal map, the liquid layers and their mask.
        let skin = kind == Kind::Skin;
        let mut st_normal_detail = st_main;
        let mut st_liquid = [st_main; 3];
        // Which of the two liquid textures tile.
        let mut liquid_wrap = 0u32;
        for (i, (name, flag)) in SLOTS.iter().enumerate() {
            // Hair has no skin layers; its gloss mask takes the first layer slot.
            // Clothes have none either; their normal map and second ramp
            // take the first two.
            let name = match *name {
                "_overtex1" if kind == Kind::Hair => "_HairGloss",
                // main_skin has no colour mask and no patterns; its two
                // liquid textures take those slots, its normal maps and
                // liquid mask the last three.
                "_ColorMask" if skin => "_Texture2",
                "_PatternMask3" if skin => "_Texture3",
                "_NormalMap" | "_NormalMapDetail" | "_liquidmask" if skin => name,
                "_overtex1" if mapped => "_NormalMap",
                "_overtex2" if glasses => "_GlassRamp",
                "_overtex2" if mapped => "_AnotherRamp",
                // main_item_emission's animation mask and main_item_studio's
                // pattern masks take the slots those two leave free, and
                // the ninth.
                "_AlphaMask" if emission => "_AnimationMask",
                "_AlphaMask" if patterned => "_PatternMask1",
                "_overtex3" if patterned => "_PatternMask2",
                "_PatternMask3" if !patterned => continue,
                // The last slots are for the KKShadersPlus materials and skin.
                "_NormalMap" | "_NormalMapDetail" | "_liquidmask" => continue,
                // Not properties of main_item_studio_alpha or main_item.
                "_AlphaMask" | "_overtex3" if studio || accessory => continue,
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
                    "_NormalMapDetail" => st_normal_detail = st,
                    "_Texture2" => {
                        st_liquid[0] = st;
                        liquid_wrap |= u32::from(slot.image.repeat);
                    }
                    "_Texture3" => {
                        st_liquid[1] = st;
                        liquid_wrap |= u32::from(slot.image.repeat) << 1;
                    }
                    "_liquidmask" => st_liquid[2] = st,
                    "_GlassRamp" => st_ramp = st,
                    "_ColorMask" => st_color_mask = st,
                    "_DetailMask" => st_detail = st,
                    "_LineMask" => st_line = st,
                    "_AnimationMask" | "_PatternMask1" => st_extra[0] = st,
                    "_PatternMask2" => st_extra[1] = st,
                    "_PatternMask3" => st_extra[2] = st,
                    _ => {}
                }
            }
        }

        // Skin gets its texture built at load time. Until then use a plain tone.
        // main_item_studio_alpha declares its own default for the three colours.
        // So do main_item_emission and main_item_studio.
        let tint = if studio || emission || patterned {
            [0.960_784_3, 0.960_784_3, 0.960_784_3, 1.0]
        } else {
            [1.0; 4]
        };
        // main_item and main_texture declare a default for each; they show
        // only where a colour mask is bound. main_opaque_low has its own.
        let tints = if accessory && !emission && !patterned {
            [[1.0, 0.0, 0.0, 1.0], [0.117_241_9, 0.0, 1.0, 1.0], [0.5, 0.5, 0.5, 1.0]]
        } else if low {
            [
                [0.901_960_8, 0.901_960_8, 0.901_960_8, 1.0],
                [0.377_595_2, 0.432_856_4, 0.529_411_8, 1.0],
                [0.784_313_7, 0.353_710_1, 0.353_710_1, 1.0],
            ]
        } else if item == ItemShader::Color {
            [[0.5, 0.5, 0.5, 1.0]; 3]
        } else {
            [tint; 3]
        };
        let mut color1 = color("_Color", tints[0]);
        if kind == Kind::Skin && flags & HAS_MAIN == 0 {
            color1 = linear([1.0, 0.88, 0.83, 1.0]);
        }
        let cutoff = match kind {
            Kind::Overlay | Kind::Eye => 0.02,
            // The hair shaders cut at one half even when they blend.
            Kind::Hair => 0.5,
            _ if blended => 0.05,
            // main_item_ditherd takes half the stored value off the alpha.
            _ if item == ItemShader::Ditherd => float("_Cutoff", 1.0),
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
                color("_Color2", tints[1])
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
                color("_Color3", tints[2])
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
            } else if accessory {
                [float("_AnotherRampFull", 0.0), 0.0, 0.0, 0.0]
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
                    float("_SpecularPower", if low { 0.5 } else { 0.0 }),
                    float("_SpecularPowerNail", 0.0),
                    float("_notusetexspecular", if emission { 1.0 } else { 0.0 }),
                ]
            } else {
                color("_overcolor2", [1.0; 4])
            },
            over3: if mapped {
                [
                    float("_rimpower", if emission { 0.0 } else { 0.5 }),
                    float("_rimV", if sheer { 0.75 } else { 0.5 }),
                    float("_outline", 1.0),
                    0.0,
                ]
            } else {
                color("_overcolor3", [1.0; 4])
            },
            st_main,
            // The lens has no alpha mask; its ramp's placement goes here.
            // main_item has none either; its colour mask's goes here.
            st_alpha: if glasses {
                st_ramp
            } else if accessory {
                st_color_mask
            } else {
                st_alpha
            },
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
            // main_item: scale and offset of the detail mask, the line mask
            // (p3) and the normal map (line).
            p2: if accessory {
                st_detail
            } else {
                [float("_alpha_a", 1.0), float("_alpha_b", 1.0), 0.0, 0.0]
            },
            p3: if accessory {
                st_line
            } else if kind == Kind::Hair {
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
            line: if accessory {
                st_normal
            } else if kind == Kind::Skin {
                [
                    float("_linetexon", 1.0),
                    float("_notusetexspecular", 0.0),
                    float("_nip_specular", 0.5),
                    float("_tex1mask", 0.0),
                ]
            } else {
                color("_LineColor", [0.5, 0.5, 0.5, 1.0])
            },
            ext: if emission || patterned {
                // Vectors and floats as stored; the colours linearised.
                let vector = |name: &str| mat.colors.get(name).copied().unwrap_or([0.0, 0.0, 1.0, 1.0]);
                let second = [0.392_156_9, 0.392_156_9, 0.392_156_9, 1.0];
                let mut ext = [[0.0; 4]; EXT];
                ext[..13].copy_from_slice(&[
                    color("_LineColor", [0.501_960_8, 0.501_960_8, 0.501_960_8, 1.0]),
                    [
                        float("_EmissionPower", 0.0),
                        float("_UPanner", 0.0),
                        float("_VPanner", 0.0),
                        0.0,
                    ],
                    st_extra[0],
                    st_extra[1],
                    st_extra[2],
                    color("_Color1_2", second),
                    color("_Color2_2", second),
                    color("_Color3_2", second),
                    vector("_Patternuv1"),
                    vector("_Patternuv2"),
                    vector("_Patternuv3"),
                    [
                        float("_patternrotator1", 0.0),
                        float("_patternclamp1", 0.0),
                        float("_patternrotator2", 0.0),
                        float("_patternclamp2", 0.0),
                    ],
                    [float("_patternrotator3", 0.0), float("_patternclamp3", 0.0), 0.0, 0.0],
                ]);
                ext
            } else if low {
                let mut ext = [[0.0; 4]; EXT];
                ext[..3].copy_from_slice(&[st_color_mask, st_detail, st_normal]);
                ext
            } else if skin {
                let mut ext = [[0.0; 4]; EXT];
                ext[..8].copy_from_slice(&[
                    st_normal,
                    st_normal_detail,
                    st_liquid[0],
                    st_liquid[1],
                    st_liquid[2],
                    // A vector: offset, then scale.
                    mat.colors.get("_LiquidTiling").copied().unwrap_or([0.0, 0.0, 2.0, 2.0]),
                    [
                        float("_liquidftop", 0.0),
                        float("_liquidfbot", 0.0),
                        float("_liquidbtop", 0.0),
                        float("_liquidbbot", 0.0),
                    ],
                    [
                        float("_liquidface", 0.0),
                        float("_rimpower", 0.5),
                        float("_rimV", 0.0),
                        liquid_wrap as f32,
                    ],
                ]);
                ext
            } else {
                [[0.0; 4]; EXT]
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
    fn skin_carries_its_normal_maps_and_liquid_layers() {
        let slot = |scale: f32, repeat: bool| rk_chara::TexSlot {
            image: Arc::new(Image {
                name: "t".into(),
                width: 1,
                height: 1,
                rgba: vec![255; 4],
                srgb: false,
                repeat,
            }),
            scale: [scale, scale],
            offset: [0.5, 0.25],
        };
        let mut mat = MaterialInst {
            shader: "Shader Forge/main_skin".into(),
            ..Default::default()
        };
        // Without values: no textures, the shader's tiling, rim and amounts.
        let plain = MaterialDesc::new(Some(&mat));
        assert_eq!(plain.uniform.info[1], 0);
        assert_eq!(plain.uniform.ext[5], [0.0, 0.0, 2.0, 2.0]);
        assert_eq!(plain.uniform.ext[6], [0.0; 4]);
        assert_eq!(plain.uniform.ext[7], [0.0, 0.5, 0.0, 0.0]);

        for (n, name) in [
            "_NormalMap",
            "_NormalMapDetail",
            "_Texture2",
            "_Texture3",
            "_liquidmask",
        ]
        .into_iter()
        .enumerate()
        {
            // Only the liquid's normal map tiles here.
            mat.textures
                .insert(name.into(), slot(1.0 + n as f32, name == "_Texture3"));
        }
        mat.colors.insert("_LiquidTiling".into(), [0.1, 0.2, 3.5, 3.5]);
        for (name, value) in [
            ("_liquidftop", 1.0),
            ("_liquidfbot", 2.0),
            ("_liquidbtop", 0.0),
            ("_liquidbbot", 1.0),
            ("_liquidface", 2.0),
            ("_rimpower", 0.1),
            ("_rimV", 0.4),
        ] {
            mat.floats.insert(name.into(), value);
        }
        let d = MaterialDesc::new(Some(&mat));
        let u = d.uniform;
        // The liquid textures sit in the slots skin leaves free.
        let slots = [9, 10, 1, 8, 11];
        assert!(slots.iter().all(|&i| d.textures[i].is_some()));
        assert_eq!(u.info[1], slots.iter().fold(0, |f, &i| f | SLOTS[i].1));
        for n in 0..5 {
            let scale = 1.0 + n as f32;
            assert_eq!(u.ext[n], [scale, scale, 0.5, 0.25]);
        }
        // The tiling is a vector, not a colour.
        assert_eq!(u.ext[5], [0.1, 0.2, 3.5, 3.5]);
        assert_eq!(u.ext[6], [1.0, 2.0, 0.0, 1.0]);
        assert_eq!(u.ext[7], [2.0, 0.1, 0.4, 2.0]);
    }

    #[test]
    fn clothes_follow_their_game_shader() {
        for (name, shader) in [
            ("Shader Forge/main_alpha", ItemShader::Alpha),
            ("Shader Forge/main_opaque", ItemShader::Opaque),
            // The KKShadersPlus shaders have their own ports.
            ("xukmi/MainAlphaPlus", ItemShader::Other),
            ("xukmi/MainOpaquePlus", ItemShader::Other),
            ("Shader Forge/main_item", ItemShader::Main),
            ("Shader Forge/main_item_studio_alpha", ItemShader::StudioAlpha),
            ("Shader Forge/main_opaque_low", ItemShader::OpaqueLow),
            ("Shader Forge/main_emblem", ItemShader::Emblem),
            ("Shader Forge/main_item_emission", ItemShader::Emission),
            ("Shader Forge/main_item_ditherd", ItemShader::Ditherd),
            ("Shader Forge/main_item_studio", ItemShader::Studio),
            ("Shader Forge/main_texture", ItemShader::Texture),
            ("Shader Forge/main_color", ItemShader::Color),
            ("Shader Forge/main_item_studio_add", ItemShader::Other),
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
            shader: "Shader Forge/main_item_studio_add".into(),
            ..Default::default()
        };
        other.textures.insert("_NormalMap".into(), slot(false));
        other.textures.insert("_MainTex".into(), slot(false));
        let d = MaterialDesc::new(Some(&other));
        assert_eq!((d.uniform.info[1], d.uniform.info[2]), (HAS_MAIN, 0));
        assert_eq!(d.uniform.over1, [1.0; 4]);
    }

    #[test]
    fn accessories_follow_main_item() {
        let mut item = MaterialInst {
            shader: "Shader Forge/main_item".into(),
            ..Default::default()
        };
        // Unread state: the pass blends One with OneMinusSrcAlpha, writes
        // depth and has an outline. Defaults are the shader's own.
        let d = MaterialDesc::new(Some(&item));
        assert_eq!(
            (d.queue, d.blend, d.blended, d.zwrite, d.outline, d.shadow),
            (2450, Blend::Premultiplied, true, true, true, ShadowPass::TwoSided)
        );
        let u = d.uniform;
        assert_eq!((u.info[1], u.info[2]), (BLENDED, ItemShader::Main as u32));
        assert_eq!(u.color1, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(u.color2, linear([0.117_241_9, 0.0, 1.0, 1.0]));
        assert_eq!(u.color3, linear([0.5, 0.5, 0.5, 1.0]));
        assert_eq!(u.shadow, linear([0.628, 0.628, 0.628, 1.0]));
        assert_eq!(u.over1, [0.0; 4]);
        assert_eq!(u.over2, [0.98, 0.0, 0.0, 0.0]);
        assert_eq!(u.over3[..2], [0.5, 0.5]);
        assert_eq!(u.p0[1..], [1.0, 1.0, 0.0]);
        let unit = [1.0, 1.0, 0.0, 0.0];
        assert_eq!((u.st_alpha, u.p2, u.p3, u.line), (unit, unit, unit, unit));

        // Colours reach the shader linear, numbers as they are, and each
        // texture with its own placement.
        item.colors.insert("_Color".into(), [0.5, 0.25, 1.0, 1.0]);
        item.colors.insert("_Color2".into(), [0.2, 0.4, 0.6, 1.0]);
        item.colors.insert("_Color3".into(), [0.1, 0.9, 0.3, 1.0]);
        item.colors.insert("_ShadowColor".into(), [0.8, 0.7, 0.9, 1.0]);
        for (name, value) in [
            ("_AnotherRampFull", 1.0),
            ("_SpeclarHeight", 0.9),
            ("_SpecularPower", 0.3),
            ("_notusetexspecular", 1.0),
            ("_rimpower", 0.2),
            ("_rimV", 0.7),
            ("_ShadowExtend", 0.4),
            ("_ShadowExtendAnother", 0.6),
            ("_DetailRLineR", 1.0),
            ("_DetailBLineG", 1.0),
            ("_LineWidthS", 2.0),
        ] {
            item.floats.insert(name.into(), value);
        }
        let slot = |scale: f32| rk_chara::TexSlot {
            image: Arc::new(Image {
                name: "t".into(),
                width: 1,
                height: 1,
                rgba: vec![255; 4],
                srgb: false,
                repeat: false,
            }),
            scale: [scale, scale],
            offset: [0.5, 0.25],
        };
        for (n, name) in [
            "_MainTex",
            "_ColorMask",
            "_DetailMask",
            "_LineMask",
            "_NormalMap",
            "_AnotherRamp",
            "_AlphaMask",
        ]
        .into_iter()
        .enumerate()
        {
            item.textures.insert(name.into(), slot(n as f32 + 2.0));
        }
        let d = MaterialDesc::new(Some(&item));
        let u = d.uniform;
        assert_eq!(
            u.info[1],
            BLENDED | HAS_MAIN | HAS_COLOR_MASK | HAS_DETAIL | HAS_LINE | HAS_OVER1 | HAS_OVER2
        );
        // main_item has no alpha mask.
        assert!(d.textures[4].is_none());
        assert_eq!(u.color1, linear([0.5, 0.25, 1.0, 1.0]));
        assert_eq!(u.color2, linear([0.2, 0.4, 0.6, 1.0]));
        assert_eq!(u.color3, linear([0.1, 0.9, 0.3, 1.0]));
        assert_eq!(u.shadow, linear([0.8, 0.7, 0.9, 1.0]));
        assert_eq!(u.over1[0], 1.0);
        assert_eq!(u.over2, [0.9, 0.3, 0.0, 1.0]);
        assert_eq!(u.over3[..2], [0.2, 0.7]);
        assert_eq!(u.p0[1..], [0.4, 0.6, 1.0]);
        assert_eq!(u.p1[..2], [1.0, 2.0]);
        let st = |s: f32| [s, s, 0.5, 0.25];
        assert_eq!(
            (u.st_main, u.st_alpha, u.p2, u.p3, u.line),
            (st(2.0), st(3.0), st(4.0), st(5.0), st(6.0))
        );

        // A state read from the shader that does not blend stays solid.
        item.draw = Some(DrawState {
            queue: 2450,
            blend: Blend::Off,
            zwrite: true,
            outline: true,
            stencil: Stencil::None,
            stencil_ref: 0,
            shadow: ShadowPass::TwoSided,
        });
        assert_eq!(MaterialDesc::new(Some(&item)).blend, Blend::Off);
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
        for name in ["Shader Forge/main_skin", "Shader Forge/main_opaque"] {
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

    fn tex(scale: f32) -> rk_chara::TexSlot {
        rk_chara::TexSlot {
            image: Arc::new(Image {
                name: "t".into(),
                width: 1,
                height: 1,
                rgba: vec![255; 4],
                srgb: false,
                repeat: false,
            }),
            scale: [scale, scale],
            offset: [0.5, 0.25],
        }
    }

    fn inst(shader: &str) -> MaterialInst {
        MaterialInst {
            shader: shader.into(),
            ..Default::default()
        }
    }

    #[test]
    fn every_ported_item_shader_has_its_own_program() {
        for name in PORTED {
            if crate::xukmi::shader(name).is_some() {
                continue;
            }
            if classify(name) == Kind::Item {
                assert_ne!(item_shader(name), ItemShader::Other, "{name}");
            }
        }
    }

    #[test]
    fn unread_states_are_those_of_the_game_shaders() {
        // (queue, blend after the alpha rule, outline, shadow pass)
        for (name, want) in [
            ("main_emblem", (2450, Blend::Off, false, ShadowPass::CullBack)),
            (
                "main_item_emission",
                (2450, Blend::Premultiplied, true, ShadowPass::TwoSided),
            ),
            (
                "main_item_studio",
                (2450, Blend::Premultiplied, true, ShadowPass::TwoSided),
            ),
            (
                "main_texture",
                (2450, Blend::Premultiplied, false, ShadowPass::TwoSided),
            ),
            ("main_item_ditherd", (2000, Blend::Off, false, ShadowPass::TwoSided)),
            ("main_opaque_low", (2000, Blend::Off, false, ShadowPass::TwoSided)),
            ("main_color", (2000, Blend::Off, false, ShadowPass::Fallback)),
        ] {
            let d = MaterialDesc::new(Some(&inst(&format!("Shader Forge/{name}"))));
            assert_eq!((d.queue, d.blend, d.outline, d.shadow), want, "{name}");
            assert!(d.zwrite, "{name}");
        }
    }

    #[test]
    fn glowing_accessories_follow_main_item_emission() {
        let mut m = inst("Shader Forge/main_item_emission");
        let u = MaterialDesc::new(Some(&m)).uniform;
        assert_eq!(u.info[2], ItemShader::Emission as u32);
        // The defaults of its property list.
        let grey = srgb_to_linear(0.960_784_3);
        assert_eq!(u.color1, [grey, grey, grey, 1.0]);
        assert_eq!((u.color2, u.color3), (u.color1, u.color1));
        assert_eq!(u.over1, [0.0; 4]);
        // No shine texture, no rim power.
        assert_eq!(u.over2[3], 1.0);
        assert_eq!(u.over3[..2], [0.0, 0.5]);
        assert_eq!(u.p0[1..], [1.0, 1.0, 0.0]);
        assert_eq!(u.ext[0], linear([0.501_960_8, 0.501_960_8, 0.501_960_8, 1.0]));
        assert_eq!(u.ext[1], [0.0; 4]);

        m.floats.insert("_EmissionPower".into(), 0.75);
        m.floats.insert("_UPanner".into(), -0.5);
        m.floats.insert("_VPanner".into(), 0.25);
        m.floats.insert("_rimpower".into(), 0.3);
        m.colors.insert("_LineColor".into(), [0.5, 0.0, 1.0, 1.0]);
        m.textures.insert("_AnimationMask".into(), tex(2.0));
        m.textures.insert("_DetailMask".into(), tex(3.0));
        // Not a property of the shader.
        m.textures.insert("_AlphaMask".into(), tex(4.0));
        m.textures.insert("_PatternMask3".into(), tex(4.0));
        let d = MaterialDesc::new(Some(&m));
        let u = d.uniform;
        assert_eq!(u.ext[1], [0.75, -0.5, 0.25, 0.0]);
        assert_eq!(u.over3[0], 0.3);
        assert_eq!(u.ext[0], linear([0.5, 0.0, 1.0, 1.0]));
        // The animation mask sits in the alpha mask slot with its placement.
        assert_eq!(u.info[1], BLENDED | HAS_DETAIL | HAS_ALPHA_MASK);
        assert_eq!(u.ext[2], [2.0, 2.0, 0.5, 0.25]);
        assert_eq!(u.p2, [3.0, 3.0, 0.5, 0.25]);
        assert!(d.textures[4].is_some() && d.textures[8].is_none());
    }

    #[test]
    fn patterned_accessories_follow_main_item_studio() {
        let mut m = inst("Shader Forge/main_item_studio");
        let u = MaterialDesc::new(Some(&m)).uniform;
        assert_eq!(u.info[2], ItemShader::Studio as u32);
        let grey = srgb_to_linear(0.960_784_3);
        assert_eq!(u.color1, [grey, grey, grey, 1.0]);
        assert_eq!(u.over2, [0.98, 0.0, 0.0, 0.0]);
        assert_eq!(u.over3[..2], [0.5, 0.5]);
        let second = linear([0.392_156_9, 0.392_156_9, 0.392_156_9, 1.0]);
        assert_eq!(u.ext[5..8], [second; 3]);
        assert_eq!(u.ext[8..11], [[0.0, 0.0, 1.0, 1.0]; 3]);
        assert_eq!((u.ext[11], u.ext[12]), ([0.0; 4], [0.0; 4]));

        for (k, scale) in [(1, 2.0), (2, 3.0), (3, 4.0)] {
            m.textures.insert(format!("_PatternMask{k}"), tex(scale));
            m.floats.insert(format!("_patternrotator{k}"), 0.1 * k as f32);
            m.floats.insert(format!("_patternclamp{k}"), 1.0);
        }
        // A vector stays as stored; a colour is linearised.
        m.colors.insert("_Patternuv2".into(), [0.5, 0.25, 2.0, 3.0]);
        m.colors.insert("_Color2_2".into(), [0.5, 0.5, 0.5, 0.25]);
        let d = MaterialDesc::new(Some(&m));
        let u = d.uniform;
        assert_eq!(u.info[1], BLENDED | HAS_ALPHA_MASK | HAS_OVER3 | HAS_PATTERN3);
        assert!(d.textures[4].is_some() && d.textures[7].is_some() && d.textures[8].is_some());
        assert_eq!(u.ext[2], [2.0, 2.0, 0.5, 0.25]);
        assert_eq!(u.ext[3], [3.0, 3.0, 0.5, 0.25]);
        assert_eq!(u.ext[4], [4.0, 4.0, 0.5, 0.25]);
        assert_eq!(u.ext[9], [0.5, 0.25, 2.0, 3.0]);
        assert_eq!(u.ext[6], linear([0.5, 0.5, 0.5, 0.25]));
        assert_eq!(u.ext[11], [0.1, 1.0, 0.2, 1.0]);
        assert_eq!(u.ext[12][..2], [0.1 * 3.0, 1.0]);
    }

    #[test]
    fn the_small_item_shaders_take_their_own_defaults() {
        // main_texture: main_item's colours and slots.
        let mut m = inst("Shader Forge/main_texture");
        m.textures.insert("_ColorMask".into(), tex(2.0));
        m.textures.insert("_LineMask".into(), tex(3.0));
        let d = MaterialDesc::new(Some(&m));
        let u = d.uniform;
        assert_eq!((d.blend, u.info[2]), (Blend::Premultiplied, ItemShader::Texture as u32));
        assert_eq!(u.color1, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(u.color2, linear([0.117_241_9, 0.0, 1.0, 1.0]));
        assert_eq!(u.st_alpha, [2.0, 2.0, 0.5, 0.25]);
        assert_eq!(u.p3, [3.0, 3.0, 0.5, 0.25]);
        assert_eq!(u.p0[1..], [1.0, 1.0, 0.0]);

        // main_opaque_low: its three colours, the masks' placements.
        let mut m = inst("Shader Forge/main_opaque_low");
        m.textures.insert("_ColorMask".into(), tex(2.0));
        m.textures.insert("_DetailMask".into(), tex(3.0));
        m.textures.insert("_NormalMap".into(), tex(4.0));
        m.textures.insert("_AlphaMask".into(), tex(5.0));
        let d = MaterialDesc::new(Some(&m));
        let u = d.uniform;
        assert_eq!((d.blend, u.info[2]), (Blend::Off, ItemShader::OpaqueLow as u32));
        assert_eq!(u.info[1], HAS_COLOR_MASK | HAS_DETAIL | HAS_ALPHA_MASK | HAS_OVER1);
        assert_eq!(u.color1, linear([0.901_960_8, 0.901_960_8, 0.901_960_8, 1.0]));
        assert_eq!(u.color2, linear([0.377_595_2, 0.432_856_4, 0.529_411_8, 1.0]));
        assert_eq!(u.color3, linear([0.784_313_7, 0.353_710_1, 0.353_710_1, 1.0]));
        assert_eq!(u.ext[0], [2.0, 2.0, 0.5, 0.25]);
        assert_eq!(u.ext[1], [3.0, 3.0, 0.5, 0.25]);
        assert_eq!(u.ext[2], [4.0, 4.0, 0.5, 0.25]);
        assert_eq!(u.st_alpha, [5.0, 5.0, 0.5, 0.25]);
        // Shadow extend, specular power, the two body mask switches.
        assert_eq!((u.p0[1], u.over2[1], u.p2[0], u.p2[1]), (1.0, 0.5, 1.0, 1.0));

        // main_item_ditherd: white, and the cutoff as stored.
        let mut m = inst("Shader Forge/main_item_ditherd");
        let u = MaterialDesc::new(Some(&m)).uniform;
        assert_eq!(
            (u.color1, u.p0[0], u.info[2]),
            ([1.0; 4], 1.0, ItemShader::Ditherd as u32)
        );
        m.floats.insert("_Cutoff".into(), 0.0);
        assert_eq!(MaterialDesc::new(Some(&m)).uniform.p0[0], 0.0);

        // main_color: mid grey unless the material says otherwise.
        let mut m = inst("Shader Forge/main_color");
        let u = MaterialDesc::new(Some(&m)).uniform;
        assert_eq!(
            (u.color1, u.info[2]),
            (linear([0.5, 0.5, 0.5, 1.0]), ItemShader::Color as u32)
        );
        m.colors.insert("_Color".into(), [1.0, 0.5, 0.0, 1.0]);
        assert_eq!(MaterialDesc::new(Some(&m)).uniform.color1, linear([1.0, 0.5, 0.0, 1.0]));

        // main_emblem: the texture only.
        let mut m = inst("Shader Forge/main_emblem");
        m.textures.insert("_MainTex".into(), tex(2.0));
        let u = MaterialDesc::new(Some(&m)).uniform;
        assert_eq!((u.info[1], u.info[2]), (HAS_MAIN, ItemShader::Emblem as u32));
        assert_eq!(u.st_main, [2.0, 2.0, 0.5, 0.25]);
    }

    #[test]
    fn uniform_matches_the_shader_layout() {
        // WGSL: fourteen vec4<f32>, an array of twenty-four and one vec4<u32>.
        assert_eq!(std::mem::size_of::<MaterialUniform>(), 624);
    }
}
