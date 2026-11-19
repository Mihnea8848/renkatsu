// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Shaders that mods carry in their own bundles, ported from the compiled
//! programs in those bundles: their draw settings and what `modshader.wgsl`
//! reads. Values come from the material, then from the shader's property
//! table, as Unity fills them.

use crate::material::{linear, Kind, MaterialDesc, MaterialUniform, BLENDED, EXT, SLOTS};
use rk_chara::{Blend, DrawState, Image, MaterialInst, ShadowPass, Stencil};
use std::sync::Arc;

/// Which mod shader's pixel program a material runs. The number goes to the
/// shader in `info.z`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModShader {
    /// `IBL_Shader_alpha`: a Unity surface shader with Standard lighting,
    /// alpha fade and a depth pass ahead of it. Seen in anse's pantyhose.
    IblAlpha = 64,
    /// `KKUSSalpha` (nHaruka's KKUSS pack, which MaterialEditor loads from
    /// `chara/kkussalpha.unity3d`): the game's clothes inputs under Unity's
    /// Standard lighting, alpha blended with the colour multiplied in.
    KkussAlpha = 65,
}

/// The ported shaders by their exact Unity names.
pub const NAMES: [(&str, ModShader); 2] = [
    ("IBL_Shader_alpha", ModShader::IblAlpha),
    ("KKUSSalpha", ModShader::KkussAlpha),
];

/// The mod shader of a Unity shader name, matched whole.
pub fn shader(name: &str) -> Option<ModShader> {
    NAMES.iter().find(|(n, _)| *n == name).map(|(_, x)| *x)
}

/// Property defaults of `IBL_Shader_alpha`, from the shader's property
/// table: name, whether it is a colour, value.
const IBL_ALPHA_DEFAULTS: [(&str, bool, [f32; 4]); 14] = [
    ("_Color", true, [1.0, 1.0, 1.0, 1.0]),
    ("_NormalScale", false, [1.0, 0.0, 0.0, 0.0]),
    ("_NormalScale2", false, [1.0, 0.0, 0.0, 0.0]),
    ("_Occlusion", false, [1.0, 0.0, 0.0, 0.0]),
    ("_Parallax", false, [0.003, 0.0, 0.0, 0.0]),
    ("_Glossiness", false, [0.5, 0.0, 0.0, 0.0]),
    ("_Metallic", false, [0.0, 0.0, 0.0, 0.0]),
    ("_rimcolor", true, [1.0, 1.0, 1.0, 0.0]),
    ("_rimpower", false, [1.0, 0.0, 0.0, 0.0]),
    ("_rimV", false, [1.0, 0.0, 0.0, 0.0]),
    ("_overcolor1", true, [1.0, 1.0, 1.0, 1.0]),
    ("_overcolor2", true, [1.0, 1.0, 1.0, 1.0]),
    ("_overcolor3", true, [1.0, 1.0, 1.0, 1.0]),
    ("_Cutoff", false, [0.5, 0.0, 0.0, 0.0]),
];

/// Texture properties of `IBL_Shader_alpha` with the slot each binds to.
const IBL_ALPHA_SLOTS: [(&str, usize); 7] = [
    ("_MainTex", 0),
    ("_DetailMask", 2),
    ("_overtex1", 5),
    ("_overtex2", 6),
    ("_overtex3", 7),
    ("_NormalMap", 9),
    ("_NormalMap2", 10),
];

/// The passes of `IBL_Shader_alpha` when the shader object could not be
/// read: SrcAlpha with OneMinusSrcAlpha, no depth write in the lit pass, no
/// outline and no shadow caster pass (its fallback chain has none either).
/// The queue is the shader's AlphaTest.
pub const IBL_ALPHA_STATE: DrawState = DrawState {
    queue: 2450,
    blend: Blend::Alpha,
    zwrite: false,
    outline: false,
    stencil: Stencil::None,
    stencil_ref: 0,
    shadow: ShadowPass::None,
};

/// Property defaults of `KKUSSalpha`, from the shader's property table.
/// `_LiquidTiling` is a vector, not a colour.
const KKUSS_ALPHA_DEFAULTS: [(&str, bool, [f32; 4]); 26] = [
    ("_overcolor1", true, [1.0, 1.0, 1.0, 0.0]),
    ("_overcolor2", true, [1.0, 1.0, 1.0, 0.0]),
    ("_overcolor3", true, [1.0, 1.0, 1.0, 0.0]),
    ("_DetailNormalMapScale", false, [0.0, 0.0, 0.0, 0.0]),
    ("_SpecularPower", false, [0.12, 0.0, 0.0, 0.0]),
    ("_nipsize", false, [0.5, 0.0, 0.0, 0.0]),
    ("_alpha_a", false, [1.0, 0.0, 0.0, 0.0]),
    ("_alpha_b", false, [1.0, 0.0, 0.0, 0.0]),
    ("_notusetexspecular", false, [0.3, 0.0, 0.0, 0.0]),
    ("_nip", false, [0.0, 0.0, 0.0, 0.0]),
    ("_Color", true, [1.0, 1.0, 1.0, 1.0]),
    ("_Color2", true, [1.0, 1.0, 1.0, 1.0]),
    ("_Color3", true, [1.0, 1.0, 1.0, 1.0]),
    ("_LiquidTiling", false, [0.0, 0.0, 2.0, 2.0]),
    ("_liquidftop", false, [0.0, 0.0, 0.0, 0.0]),
    ("_liquidfbot", false, [0.0, 0.0, 0.0, 0.0]),
    ("_liquidbtop", false, [0.0, 0.0, 0.0, 0.0]),
    ("_liquidbbot", false, [0.0, 0.0, 0.0, 0.0]),
    ("_liquidface", false, [0.0, 0.0, 0.0, 0.0]),
    ("_Occlusion", false, [1.0, 0.0, 0.0, 0.0]),
    ("_Metallic", false, [0.0, 0.0, 0.0, 0.0]),
    ("_Emission", false, [0.0, 0.0, 0.0, 0.0]),
    ("_Addcolor", true, [1.0, 1.0, 1.0, 1.0]),
    ("_DetailMaskScale", false, [0.3, 0.0, 0.0, 0.0]),
    ("_ShadowPower", false, [0.8, 0.0, 0.0, 0.0]),
    ("_alpha", false, [1.0, 0.0, 0.0, 0.0]),
];

/// Texture properties of `KKUSSalpha` with the slot each binds to. The
/// liquid amounts (`_Texture2`) take the slot of `_PatternMask3`, which the
/// shader does not have.
const KKUSS_ALPHA_SLOTS: [(&str, usize); 11] = [
    ("_MainTex", 0),
    ("_ColorMask", 1),
    ("_DetailMask", 2),
    ("_AlphaMask", 4),
    ("_overtex1", 5),
    ("_overtex2", 6),
    ("_overtex3", 7),
    ("_Texture2", 8),
    ("_NormalMap", 9),
    ("_NormalMapDetail", 10),
    ("_liquidmask", 11),
];

/// The passes of `KKUSSalpha` when the shader object could not be read: its
/// forward pass blends One with OneMinusSrcAlpha, writes no depth and culls
/// back faces; the queue is Transparent. There is no depth pass, no outline
/// and no shadow caster (its fallback, Legacy Transparent/Diffuse, ends in
/// Transparent/VertexLit, which has none).
pub const KKUSS_ALPHA_STATE: DrawState = DrawState {
    queue: 3000,
    blend: Blend::Premultiplied,
    zwrite: false,
    outline: false,
    stencil: Stencil::None,
    stencil_ref: 0,
    shadow: ShadowPass::None,
};

struct Props<'a> {
    mat: &'a MaterialInst,
    defaults: &'static [(&'static str, bool, [f32; 4])],
}

impl Props<'_> {
    fn row(&self, name: &str) -> Option<&'static (&'static str, bool, [f32; 4])> {
        self.defaults.iter().find(|r| r.0 == name)
    }

    fn f(&self, name: &str) -> f32 {
        let default = self.row(name).map_or(0.0, |r| r.2[0]);
        self.mat.floats.get(name).copied().unwrap_or(default)
    }

    /// A vector property as stored. The scene keeps vectors with the
    /// colours.
    fn v(&self, name: &str) -> [f32; 4] {
        let default = self.row(name).map_or([0.0; 4], |r| r.2);
        self.mat.colors.get(name).copied().unwrap_or(default)
    }

    /// A colour property, which reaches the shader linearised.
    fn c(&self, name: &str) -> [f32; 4] {
        linear(self.v(name))
    }

    /// Whether a texture repeats. An unbound one is one of Unity's small
    /// default textures, where it makes no difference.
    fn repeats(&self, name: &str) -> f32 {
        let repeat = self.mat.textures.get(name).is_none_or(|t| t.image.repeat);
        f32::from(u8::from(repeat))
    }

    /// The textures of a slot table and the flags of the bound ones.
    fn textures(&self, slots: &[(&str, usize)]) -> ([Option<Arc<Image>>; SLOTS.len()], u32) {
        let mut flags = 0;
        let mut textures: [Option<Arc<Image>>; SLOTS.len()] = Default::default();
        for (name, i) in slots {
            if let Some(slot) = self.mat.textures.get(*name) {
                textures[*i] = Some(slot.image.clone());
                flags |= SLOTS[*i].1;
            }
        }
        (textures, flags)
    }

    /// Scale and offset of a texture property.
    fn st(&self, name: &str) -> [f32; 4] {
        if let Some(t) = self.mat.textures.get(name) {
            return [t.scale[0], t.scale[1], t.offset[0], t.offset[1]];
        }
        self.mat.tex_st.get(name).copied().unwrap_or([1.0, 1.0, 0.0, 0.0])
    }
}

/// Render settings and uniform of a mod shader's material.
pub fn desc(mat: &MaterialInst, x: ModShader) -> MaterialDesc {
    match x {
        ModShader::IblAlpha => ibl_alpha(mat),
        ModShader::KkussAlpha => kkuss_alpha(mat),
    }
}

/// A uniform with every value at zero but the ones given.
fn uniform(flags: u32, x: ModShader, ext: [[f32; 4]; EXT]) -> MaterialUniform {
    MaterialUniform {
        color1: [0.0; 4],
        color2: [0.0; 4],
        color3: [0.0; 4],
        shadow: [0.0; 4],
        over1: [0.0; 4],
        over2: [0.0; 4],
        over3: [0.0; 4],
        st_main: [1.0, 1.0, 0.0, 0.0],
        st_alpha: [1.0, 1.0, 0.0, 0.0],
        p0: [0.0; 4],
        p1: [0.0; 4],
        p2: [0.0; 4],
        p3: [0.0; 4],
        line: [0.0; 4],
        ext,
        info: [Kind::Item as u32, flags, x as u32, 0],
    }
}

fn kkuss_alpha(mat: &MaterialInst) -> MaterialDesc {
    let x = ModShader::KkussAlpha;
    let mut state = mat.draw.unwrap_or(KKUSS_ALPHA_STATE);
    // The fallback chain is in the shader's own bundle and has no shadow
    // caster pass.
    state.shadow = ShadowPass::None;
    let p = Props {
        mat,
        defaults: &KKUSS_ALPHA_DEFAULTS,
    };
    let (textures, flags) = p.textures(&KKUSS_ALPHA_SLOTS);
    let mut ext = [[0.0; 4]; EXT];
    ext[0] = [
        p.f("_SpecularPower"),
        p.f("_Metallic"),
        p.f("_Occlusion"),
        p.f("_Emission"),
    ];
    ext[1] = [
        p.f("_DetailNormalMapScale"),
        p.f("_DetailMaskScale"),
        p.f("_notusetexspecular"),
        p.f("_ShadowPower"),
    ];
    ext[2] = [p.f("_alpha"), p.f("_alpha_a"), p.f("_alpha_b"), 0.0];
    ext[3] = [p.f("_nipsize"), p.f("_nip"), 0.0, 0.0];
    ext[4] = p.c("_Addcolor");
    ext[5] = p.st("_NormalMap");
    ext[6] = p.st("_NormalMapDetail");
    ext[7] = p.st("_overtex1");
    ext[8] = p.st("_overtex2");
    ext[9] = p.st("_overtex3");
    ext[10] = p.v("_LiquidTiling");
    ext[11] = [
        p.f("_liquidftop"),
        p.f("_liquidfbot"),
        p.f("_liquidbtop"),
        p.f("_liquidbbot"),
    ];
    // Unity samples each texture with its own wrap mode; the two tiled ones
    // say which.
    ext[12] = [
        p.f("_liquidface"),
        p.repeats("_NormalMapDetail"),
        p.repeats("_Texture2"),
        0.0,
    ];
    let uniform = MaterialUniform {
        color1: p.c("_Color"),
        color2: p.c("_Color2"),
        color3: p.c("_Color3"),
        over1: p.c("_overcolor1"),
        over2: p.c("_overcolor2"),
        over3: p.c("_overcolor3"),
        st_main: p.st("_MainTex"),
        ..uniform(flags | BLENDED, x, ext)
    };
    MaterialDesc {
        kind: Kind::Item,
        queue: if mat.render_queue > 0 {
            mat.render_queue
        } else {
            state.queue
        },
        blended: true,
        blend: state.blend,
        outline: false,
        zwrite: state.zwrite,
        prepass: false,
        stencil: state.stencil,
        stencil_ref: state.stencil_ref,
        shadow: state.shadow,
        uniform,
        textures,
    }
}

fn ibl_alpha(mat: &MaterialInst) -> MaterialDesc {
    let x = ModShader::IblAlpha;
    let state = mat.draw.unwrap_or(IBL_ALPHA_STATE);
    let p = Props {
        mat,
        defaults: &IBL_ALPHA_DEFAULTS,
    };
    let (textures, flags) = p.textures(&IBL_ALPHA_SLOTS);
    let flags = flags | BLENDED;
    // Unity samples each texture with its own wrap mode; the fabric normal
    // is tiled, whatever the main texture does.
    let repeat2 = mat.textures.get("_NormalMap2").is_none_or(|t| t.image.repeat);
    let mut ext = [[0.0; 4]; EXT];
    ext[0] = [
        p.f("_Glossiness"),
        p.f("_Metallic"),
        p.f("_NormalScale"),
        p.f("_NormalScale2"),
    ];
    ext[1] = [p.f("_Parallax"), p.f("_Occlusion"), p.f("_rimpower"), p.f("_rimV")];
    ext[2] = p.c("_rimcolor");
    ext[3] = p.st("_NormalMap");
    ext[4] = p.st("_NormalMap2");
    ext[5] = p.st("_overtex1");
    ext[6] = p.st("_overtex2");
    ext[7] = p.st("_overtex3");
    ext[8] = [f32::from(u8::from(repeat2)), 0.0, 0.0, 0.0];
    let uniform = MaterialUniform {
        color1: p.c("_Color"),
        color2: [0.0; 4],
        color3: [0.0; 4],
        shadow: [0.0; 4],
        over1: p.c("_overcolor1"),
        over2: p.c("_overcolor2"),
        over3: p.c("_overcolor3"),
        st_main: p.st("_MainTex"),
        st_alpha: [1.0, 1.0, 0.0, 0.0],
        p0: [0.0; 4],
        p1: [0.0; 4],
        p2: [0.0; 4],
        p3: [0.0; 4],
        line: [0.0; 4],
        ext,
        info: [Kind::Item as u32, flags, x as u32, 0],
    };
    MaterialDesc {
        kind: Kind::Item,
        queue: if mat.render_queue > 0 {
            mat.render_queue
        } else {
            state.queue
        },
        blended: true,
        blend: state.blend,
        outline: false,
        zwrite: state.zwrite,
        // The shader's first pass: depth of the front faces, no colour.
        prepass: true,
        stencil: state.stencil,
        stencil_ref: state.stencil_ref,
        shadow: state.shadow,
        uniform,
        textures,
    }
}

/// Unity's `unity_SHAr` to `unity_SHC` from an ambient probe
/// (`SphericalHarmonicsL2`, 27 coefficients: nine per colour channel), as
/// the engine's documentation gives the packing.
pub fn sh_constants(probe: &[f32; 27]) -> [[f32; 4]; 7] {
    let c = |ch: usize, k: usize| probe[ch * 9 + k];
    let mut out = [[0.0; 4]; 7];
    for ch in 0..3 {
        out[ch] = [c(ch, 3), c(ch, 1), c(ch, 2), c(ch, 0) - c(ch, 6)];
        out[3 + ch] = [c(ch, 4), c(ch, 5), c(ch, 6) * 3.0, c(ch, 7)];
    }
    out[6] = [c(0, 8), c(1, 8), c(2, 8), 1.0];
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::{
        HAS_ALPHA_MASK, HAS_COLOR_MASK, HAS_DETAIL, HAS_MAIN, HAS_OVER1, HAS_OVER5, HAS_OVER6, HAS_OVER7, HAS_PATTERN3,
    };
    use rk_chara::TexSlot;

    fn tex(scale: f32, repeat: bool) -> TexSlot {
        TexSlot {
            image: Arc::new(Image {
                repeat,
                ..Image::unity_black()
            }),
            scale: [scale, scale],
            offset: [0.0, 0.0],
        }
    }

    /// anse's `cf_m_pantyhose_b` as its bundle stores it.
    fn pantyhose() -> MaterialInst {
        let mut m = MaterialInst {
            shader: "IBL_Shader_alpha".into(),
            render_queue: 3100,
            ..Default::default()
        };
        m.textures.insert("_MainTex".into(), tex(1.0, false));
        m.textures.insert("_DetailMask".into(), tex(1.0, true));
        m.textures.insert("_NormalMap".into(), tex(1.0, true));
        m.textures.insert("_NormalMap2".into(), tex(6.0, true));
        for (k, v) in [
            ("_Glossiness", 0.3),
            ("_Metallic", 0.36),
            ("_Parallax", 0.001),
            ("_rimV", -0.03),
            ("_rimpower", 0.04),
            ("_NormalScale", 1.0),
            ("_NormalScale2", 0.3),
            ("_Occlusion", 1.0),
        ] {
            m.floats.insert(k.into(), v);
        }
        m.colors.insert("_Color".into(), [1.0, 1.0, 1.0, 0.9]);
        m
    }

    #[test]
    fn names_match_whole() {
        assert_eq!(shader("IBL_Shader_alpha"), Some(ModShader::IblAlpha));
        assert_eq!(shader("IBL_Shader"), None);
        assert_eq!(shader("Custom/IBL_Shader_alpha"), None);
        assert_eq!(shader("Shader Forge/main_alpha"), None);
    }

    #[test]
    fn pantyhose_blends_after_a_depth_pass() {
        let d = MaterialDesc::new(Some(&pantyhose()));
        assert_eq!(d.kind, Kind::Item);
        // The material's own queue wins over the shader's.
        assert_eq!(d.queue, 3100);
        assert_eq!(d.blend, Blend::Alpha);
        assert!(d.blended && d.prepass);
        assert!(!d.zwrite && !d.outline);
        assert_eq!(d.shadow, ShadowPass::None);
        assert_eq!(d.uniform.info[2], ModShader::IblAlpha as u32);
    }

    #[test]
    fn the_shader_queue_holds_without_an_override() {
        let mut m = pantyhose();
        m.render_queue = -1;
        assert_eq!(MaterialDesc::new(Some(&m)).queue, 2450);
    }

    #[test]
    fn pantyhose_values_reach_the_uniform() {
        let d = MaterialDesc::new(Some(&pantyhose()));
        let u = d.uniform;
        assert_eq!(u.color1, [1.0, 1.0, 1.0, 0.9]);
        assert_eq!(u.ext[0], [0.3, 0.36, 1.0, 0.3]);
        assert_eq!(u.ext[1], [0.001, 1.0, 0.04, -0.03]);
        // The fabric normal tiles six times and repeats.
        assert_eq!(u.ext[4], [6.0, 6.0, 0.0, 0.0]);
        assert_eq!(u.ext[8][0], 1.0);
        for flag in [HAS_MAIN, HAS_DETAIL, HAS_OVER5, HAS_OVER6] {
            assert_ne!(u.info[1] & flag, 0);
        }
        assert!(d.textures[9].is_some() && d.textures[10].is_some());
        // No overlay is bound: Unity's "black" default leaves the base.
        assert!(d.textures[5].is_none());
    }

    #[test]
    fn missing_values_take_the_shader_defaults() {
        let m = MaterialInst {
            shader: "IBL_Shader_alpha".into(),
            ..Default::default()
        };
        let u = MaterialDesc::new(Some(&m)).uniform;
        assert_eq!(u.color1, [1.0; 4]);
        assert_eq!(u.ext[0], [0.5, 0.0, 1.0, 1.0]);
        assert_eq!(u.ext[1], [0.003, 1.0, 1.0, 1.0]);
        assert_eq!(u.ext[2], [1.0, 1.0, 1.0, 0.0]);
        assert_eq!(u.ext[3], [1.0, 1.0, 0.0, 0.0]);
    }

    /// The game's `cf_m_panst_01` after MaterialEditor moved it to
    /// `KKUSSalpha`, with the values the cards carry.
    fn tights() -> MaterialInst {
        let mut m = MaterialInst {
            shader: "KKUSSalpha".into(),
            render_queue: -1,
            ..Default::default()
        };
        m.textures.insert("_MainTex".into(), tex(1.0, false));
        m.textures.insert("_DetailMask".into(), tex(1.0, true));
        m.textures.insert("_NormalMap".into(), tex(1.0, true));
        m.textures.insert("_NormalMapDetail".into(), tex(9.0, true));
        m.textures.insert("_Texture2".into(), tex(1.0, true));
        m.textures.insert("_liquidmask".into(), tex(1.0, true));
        for (k, v) in [
            ("_SpecularPower", 0.43),
            ("_DetailNormalMapScale", 0.53),
            ("_notusetexspecular", 0.0),
            ("_nipsize", 0.5),
        ] {
            m.floats.insert(k.into(), v);
        }
        m.colors.insert("_Color".into(), [0.5, 0.5, 0.5, 1.0]);
        m.colors.insert("_LiquidTiling".into(), [0.0, 0.0, 2.0, 4.0]);
        m
    }

    #[test]
    fn kkuss_alpha_blends_with_colour_multiplied_in() {
        assert_eq!(shader("KKUSSalpha"), Some(ModShader::KkussAlpha));
        assert_eq!(shader("KKUSS"), None);
        let d = MaterialDesc::new(Some(&tights()));
        assert_eq!(d.kind, Kind::Item);
        assert_eq!(d.queue, 3000);
        assert_eq!(d.blend, Blend::Premultiplied);
        assert!(d.blended && !d.prepass && !d.zwrite && !d.outline);
        assert_eq!(d.uniform.info[2], ModShader::KkussAlpha as u32);
    }

    #[test]
    fn kkuss_alpha_casts_no_shadow_whatever_the_fallback_says() {
        let mut m = tights();
        m.draw = Some(DrawState {
            shadow: ShadowPass::Fallback,
            ..KKUSS_ALPHA_STATE
        });
        assert_eq!(MaterialDesc::new(Some(&m)).shadow, ShadowPass::None);
        m.render_queue = 3100;
        assert_eq!(MaterialDesc::new(Some(&m)).queue, 3100);
    }

    #[test]
    fn kkuss_alpha_values_reach_the_uniform() {
        let d = MaterialDesc::new(Some(&tights()));
        let u = d.uniform;
        // Colours are linearised, the tiling vector is not.
        let grey = linear([0.5, 0.5, 0.5, 1.0]);
        assert_eq!(u.color1, grey);
        assert!(grey[0] < 0.25);
        assert_eq!(u.ext[10], [0.0, 0.0, 2.0, 4.0]);
        // Given values, then the shader's defaults.
        assert_eq!(u.ext[0], [0.43, 0.0, 1.0, 0.0]);
        assert_eq!(u.ext[1], [0.53, 0.3, 0.0, 0.8]);
        assert_eq!(u.ext[2], [1.0, 1.0, 1.0, 0.0]);
        assert_eq!(u.ext[3], [0.5, 0.0, 0.0, 0.0]);
        assert_eq!(u.ext[4], [1.0; 4]);
        assert_eq!(u.over1, [1.0, 1.0, 1.0, 0.0]);
        // The fabric normal tiles nine times and repeats.
        assert_eq!(u.ext[6], [9.0, 9.0, 0.0, 0.0]);
        assert_eq!(u.ext[12], [0.0, 1.0, 1.0, 0.0]);
        for flag in [HAS_MAIN, HAS_DETAIL, HAS_OVER5, HAS_OVER6, HAS_PATTERN3, HAS_OVER7] {
            assert_ne!(u.info[1] & flag, 0);
        }
        // _Texture2 sits in the slot of _PatternMask3.
        assert!(d.textures[8].is_some() && d.textures[11].is_some());
        assert_eq!(u.info[1] & (HAS_COLOR_MASK | HAS_ALPHA_MASK | HAS_OVER1), 0);
    }

    #[test]
    fn kkuss_alpha_defaults_match_the_property_table() {
        let m = MaterialInst {
            shader: "KKUSSalpha".into(),
            ..Default::default()
        };
        let u = MaterialDesc::new(Some(&m)).uniform;
        assert_eq!(u.ext[0], [0.12, 0.0, 1.0, 0.0]);
        assert_eq!(u.ext[1], [0.0, 0.3, 0.3, 0.8]);
        assert_eq!(u.ext[10], [0.0, 0.0, 2.0, 2.0]);
        assert_eq!(u.ext[11], [0.0; 4]);
        assert_eq!(u.color1, [1.0; 4]);
    }

    #[test]
    fn a_flat_probe_packs_into_the_constant_terms() {
        let mut probe = [0.0; 27];
        probe[0] = 0.25;
        probe[9] = 0.5;
        probe[18] = 0.75;
        let sh = sh_constants(&probe);
        assert_eq!(sh[0], [0.0, 0.0, 0.0, 0.25]);
        assert_eq!(sh[1], [0.0, 0.0, 0.0, 0.5]);
        assert_eq!(sh[2], [0.0, 0.0, 0.0, 0.75]);
        assert_eq!(sh[6], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(sh_constants(&[0.0; 27])[..6], [[0.0; 4]; 6]);
    }
}
