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
}

/// The ported shaders by their exact Unity names.
pub const NAMES: [(&str, ModShader); 1] = [("IBL_Shader_alpha", ModShader::IblAlpha)];

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

struct Props<'a> {
    mat: &'a MaterialInst,
}

impl Props<'_> {
    fn row(name: &str) -> Option<&'static (&'static str, bool, [f32; 4])> {
        IBL_ALPHA_DEFAULTS.iter().find(|r| r.0 == name)
    }

    fn f(&self, name: &str) -> f32 {
        let default = Self::row(name).map_or(0.0, |r| r.2[0]);
        self.mat.floats.get(name).copied().unwrap_or(default)
    }

    /// A colour property, which reaches the shader linearised.
    fn c(&self, name: &str) -> [f32; 4] {
        let default = Self::row(name).map_or([0.0; 4], |r| r.2);
        linear(self.mat.colors.get(name).copied().unwrap_or(default))
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
    let ModShader::IblAlpha = x;
    let state = mat.draw.unwrap_or(IBL_ALPHA_STATE);
    let p = Props { mat };
    let mut flags = BLENDED;
    let mut textures: [Option<Arc<Image>>; SLOTS.len()] = Default::default();
    for (name, i) in IBL_ALPHA_SLOTS {
        if let Some(slot) = mat.textures.get(name) {
            textures[i] = Some(slot.image.clone());
            flags |= SLOTS[i].1;
        }
    }
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
    use crate::material::{HAS_DETAIL, HAS_MAIN, HAS_OVER5, HAS_OVER6};
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
