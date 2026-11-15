// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Materials of the KKShadersPlus ("Vanilla Plus") shaders, which
//! MaterialEditor swaps onto cards: their draw settings and what
//! `xukmi.wgsl` reads. Values come from the material, then from the shader's
//! property table (`xukmi_defaults.rs`), then zero, as Unity fills a
//! uniform no property backs.

use crate::material::{
    linear, state_for_name, Kind, MaterialDesc, MaterialUniform, BLENDED, CASTER_FALLBACK, EXT, SLOTS,
};
use crate::xukmi_defaults::{table, Row};
use rk_chara::{Blend, Image, MaterialInst, ShadowPass};
use std::sync::Arc;

/// Which KKShadersPlus pixel programs a material runs. The number goes to
/// the shader in `info.z`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Xukmi {
    /// xukmi/MainOpaquePlus
    Opaque = 32,
    /// xukmi/MainAlphaPlus
    Alpha = 33,
    /// xukmi/MainItemPlus
    Item = 34,
    /// xukmi/MainItemAlphaPlus
    ItemAlpha = 35,
    /// xukmi/SkinPlus
    Skin = 36,
    /// xukmi/HairPlus
    Hair = 37,
    /// xukmi/HairFrontPlus
    HairFront = 38,
    /// xukmi/EyePlus
    Eye = 39,
    /// xukmi/EyeWPlus
    EyeW = 40,
}

/// The ported shaders by their exact Unity names. The tessellated variants
/// compile the pixel code of the plain ones for another shader model (the
/// programs have the same constants and steps) and draw it on a finer,
/// displaced mesh; here the mesh stays as it is. The reflect variants run
/// the same forward programs and add a REFLECT pass that is not drawn.
pub const NAMES: [(&str, Xukmi); 22] = [
    ("xukmi/MainOpaquePlus", Xukmi::Opaque),
    ("xukmi/MainOpaquePlusTess", Xukmi::Opaque),
    ("xukmi/MainAlphaPlus", Xukmi::Alpha),
    ("xukmi/MainAlphaPlusTess", Xukmi::Alpha),
    ("xukmi/MainItemPlus", Xukmi::Item),
    ("xukmi/MainItemPlusTess", Xukmi::Item),
    ("xukmi/MainItemAlphaPlus", Xukmi::ItemAlpha),
    ("xukmi/MainItemAlphaPlusTess", Xukmi::ItemAlpha),
    ("xukmi/SkinPlus", Xukmi::Skin),
    ("xukmi/SkinPlusTess", Xukmi::Skin),
    ("xukmi/SkinPlusReflect", Xukmi::Skin),
    ("xukmi/SkinPlusTessReflect", Xukmi::Skin),
    ("xukmi/HairPlus", Xukmi::Hair),
    ("xukmi/HairPlusTess", Xukmi::Hair),
    ("xukmi/HairPlusReflect", Xukmi::Hair),
    ("xukmi/HairPlusTessReflect", Xukmi::Hair),
    ("xukmi/HairFrontPlus", Xukmi::HairFront),
    ("xukmi/HairFrontPlusTess", Xukmi::HairFront),
    ("xukmi/HairFrontPlusReflect", Xukmi::HairFront),
    ("xukmi/HairFrontPlusTessReflect", Xukmi::HairFront),
    ("xukmi/EyePlus", Xukmi::Eye),
    ("xukmi/EyeWPlus", Xukmi::EyeW),
];

/// The KKShadersPlus shader of a Unity shader name, matched whole.
pub fn shader(name: &str) -> Option<Xukmi> {
    NAMES.iter().find(|(n, _)| *n == name).map(|(_, x)| *x)
}

impl Xukmi {
    pub fn kind(self) -> Kind {
        match self {
            Xukmi::Opaque | Xukmi::Alpha | Xukmi::Item | Xukmi::ItemAlpha => Kind::Item,
            Xukmi::Skin => Kind::Skin,
            Xukmi::Hair | Xukmi::HairFront => Kind::Hair,
            Xukmi::Eye => Kind::Eye,
            Xukmi::EyeW => Kind::Overlay,
        }
    }

    /// Texture properties in bind order; see the head of `xukmi.wgsl`.
    fn slots(self) -> [&'static str; 11] {
        let common = |mask, o1, o2, o3| {
            [
                "_MainTex",
                mask,
                "_DetailMask",
                "_LineMask",
                "_AlphaMask",
                o1,
                o2,
                o3,
                "_EmissionMask",
                "_NormalMap",
                "_NormalMapDetail",
            ]
        };
        match self {
            Xukmi::Opaque | Xukmi::Alpha => common("_ColMask", "", "_AnotherRamp", ""),
            Xukmi::Item | Xukmi::ItemAlpha => common("_ColorMask", "", "_AnotherRamp", ""),
            Xukmi::Skin => common("_ColMask", "_overtex1", "_overtex2", "_overtex3"),
            Xukmi::Hair | Xukmi::HairFront => common("_ColorMask", "_HairGloss", "_AnotherRamp", ""),
            Xukmi::Eye => common("", "_overtex1", "_overtex2", "_expression"),
            Xukmi::EyeW => common("", "", "", ""),
        }
    }
}

/// A material's values with the shader's defaults behind them.
/// Colours and vectors marked [Gamma] reach the shader linearised.
struct Props<'a> {
    mat: &'a MaterialInst,
    table: &'static [Row],
}

impl Props<'_> {
    fn row(&self, name: &str) -> Option<&'static Row> {
        self.table.iter().find(|r| r.0 == name)
    }

    /// A float or range property.
    fn f(&self, name: &str) -> f32 {
        let default = self.row(name).map_or(0.0, |r| r.2[0]);
        self.mat.floats.get(name).copied().unwrap_or(default)
    }

    /// A colour or vector property. Colours reach the shader linearised,
    /// vectors as stored.
    fn v(&self, name: &str) -> [f32; 4] {
        let (color, default) = self.row(name).map_or((false, [0.0; 4]), |r| (r.1, r.2));
        let value = self.mat.colors.get(name).copied().unwrap_or(default);
        if color {
            linear(value)
        } else {
            value
        }
    }

    /// Scale and offset of a texture property.
    fn st(&self, name: &str) -> [f32; 4] {
        // A bound texture carries the placement the game last gave it.
        if let Some(t) = self.mat.textures.get(name) {
            return [t.scale[0], t.scale[1], t.offset[0], t.offset[1]];
        }
        self.mat.tex_st.get(name).copied().unwrap_or([1.0, 1.0, 0.0, 0.0])
    }

    fn fs(&self, names: [&str; 4]) -> [f32; 4] {
        names.map(|n| if n.is_empty() { 0.0 } else { self.f(n) })
    }
}

/// Render settings and uniform of a KKShadersPlus material.
pub fn desc(mat: &MaterialInst, x: Xukmi) -> MaterialDesc {
    let kind = x.kind();
    let state = mat.draw.unwrap_or_else(|| state_for_name(&mat.shader, kind));
    let p = Props {
        mat,
        table: table(&mat.shader).unwrap_or(&[]),
    };

    // One with OneMinusSrcAlpha under an alpha of one is no blending.
    let blend = match state.blend {
        Blend::Premultiplied => Blend::Off,
        other => other,
    };
    let blended = blend != Blend::Off;
    let mut flags = if blended { BLENDED } else { 0 };
    if state.shadow == ShadowPass::Fallback {
        flags |= CASTER_FALLBACK;
    }
    let mut textures: [Option<Arc<Image>>; 11] = Default::default();
    for (i, name) in x.slots().iter().enumerate() {
        if let Some(slot) = mat.textures.get(*name) {
            textures[i] = Some(slot.image.clone());
            flags |= SLOTS[i].1;
        }
    }

    let sheer = matches!(x, Xukmi::Alpha | Xukmi::ItemAlpha);
    let mut u = MaterialUniform {
        color1: [0.0; 4],
        color2: [0.0; 4],
        color3: [0.0; 4],
        shadow: [0.0; 4],
        over1: [0.0; 4],
        over2: [0.0; 4],
        over3: [0.0; 4],
        st_main: p.st("_MainTex"),
        st_alpha: p.st("_AlphaMask"),
        p0: [p.f("_Cutoff"), 0.0, 0.0, 0.0],
        p1: [0.0, p.f("_LineWidthS"), 0.0, 0.0],
        p2: [p.f("_alpha_a"), p.f("_alpha_b"), 0.0, 0.0],
        p3: p.st("_DetailMask"),
        line: p.v("_OutlineColor"),
        ext: [[0.0; 4]; EXT],
        info: [kind as u32, flags, x as u32, 0],
    };
    let amb = p.v("_CustomAmbient");
    u.ext[1] = [amb[0], amb[1], amb[2], p.f("_UseRampForShadows")];
    u.ext[2] = p.v("_KKPRimColor");
    u.ext[3] = p.v("_EmissionColor");
    let hsv = p.v("_ShadowHSV");
    u.ext[4] = [hsv[0], hsv[1], hsv[2], p.f("_UseRampForSpecular")];
    u.ext[5] = p.fs([
        "_UseDetailRAsSpecularMap",
        "_UseKKPRim",
        "_NormalMapScale",
        "_DetailNormalMapScale",
    ]);
    u.ext[6] = p.fs(["_KKPRimSoft", "_KKPRimIntensity", "_KKPRimAsDiffuse", "_KKPRimRotateX"]);
    u.ext[7] = p.fs([
        "_KKPRimRotateY",
        "_SpecularNormalScale",
        "_SpecularDetailNormalScale",
        "_AdjustBackfaceNormals",
    ]);
    u.ext[8] = p.fs([
        "_rimReflectMode",
        "_EmissionIntensity",
        "_EmissionMaskMode",
        "_EmissionKeepCol",
    ]);
    u.ext[9] = p.st("_NormalMap");
    u.ext[10] = p.st("_NormalMapDetail");
    u.ext[11] = p.st("_LineMask");
    u.ext[12] = p.st(x.slots()[1]);
    u.ext[13] = p.st("_EmissionMask");
    u.ext[14] = p.st("_AnotherRamp");
    u.ext[15] = p.st("_RampG");
    u.ext[16] = p.fs(["_Saturation", "_CullOption", "_OutlineOn", "_UseLightColorSpecular"]);

    match x {
        Xukmi::Opaque | Xukmi::Alpha | Xukmi::Item | Xukmi::ItemAlpha => {
            let cloth = matches!(x, Xukmi::Opaque | Xukmi::Alpha);
            let colors = if cloth {
                ["_Col0", "_Col1", "_Col2"]
            } else {
                ["_Color", "_Color2", "_Color3"]
            };
            u.color1 = p.v(colors[0]);
            u.color2 = p.v(colors[1]);
            u.color3 = p.v(colors[2]);
            u.ext[0] = p.v("_Col3");
            u.shadow = p.v("_ShadowColor");
            u.over1 = p.v("_SpecularColor");
            u.over2 = p.fs([
                "_SpeclarHeight",
                "_SpecularPower",
                "_SpecularPowerNail",
                "_notusetexspecular",
            ]);
            u.over3 = p.fs(["_rimpower", "_rimV", "_AnotherRampFull", "_UseKKMetal"]);
            u.p0 = p.fs(["_Cutoff", "_ShadowExtend", "_ShadowExtendAnother", "_DetailRLineR"]);
            // MainOpaquePlus and MainItemPlus have no _Alpha.
            u.p1 = [
                p.f("_DetailBLineG"),
                p.f("_LineWidthS"),
                if sheer { p.f("_Alpha") } else { 1.0 },
                p.f("_AlphaOptionCutoff"),
            ];
        }
        Xukmi::Skin => {
            u.color1 = p.v("_Col0");
            u.color2 = p.v("_Col1");
            u.color3 = p.v("_Col2");
            u.ext[0] = p.v("_Col3");
            u.shadow = p.v("_ShadowColor");
            u.over1 = p.v("_overcolor1");
            u.over2 = p.v("_overcolor2");
            u.over3 = p.v("_overcolor3");
            u.p0 = [0.5, p.f("_ShadowExtend"), 0.0, 0.0];
            u.p1 = p.fs(["_linetexon", "_LineWidthS", "_tex1mask", "_nip_specular"]);
            u.p2 = p.fs(["_alpha_a", "_alpha_b", "_nip", "_nipsize"]);
            u.ext[17] = p.v("_SpecularColor");
            u.ext[18] = p.fs([
                "_SpeclarHeight",
                "_SpecularPower",
                "_SpecularPowerNail",
                "_notusetexspecular",
            ]);
            u.ext[19] = p.st("_overtex1");
            u.ext[20] = p.st("_overtex2");
            u.ext[21] = p.st("_overtex3");
        }
        Xukmi::Hair | Xukmi::HairFront => {
            u.color1 = p.v("_Color");
            u.color2 = p.v("_Color2");
            u.color3 = p.v("_Color3");
            u.shadow = p.v("_ShadowColor");
            u.over1 = p.v("_SpecularColor");
            u.over2 = p.st("_HairGloss");
            u.over3 = p.fs(["_rimpower", "_rimV", "_SpeclarHeight", "_SpecularHeightInvert"]);
            u.p0 = [p.f("_Cutoff"), p.f("_ShadowExtend"), 0.0, 0.0];
            // The hair shaders draw their outline in _LineColor.
            u.line = p.v("_LineColor");
            u.ext[17] = p.v("_GlossColor");
            u.ext[18] = p.fs([
                "_SpecularHairPower",
                "_SpecularIsHighlights",
                "_SpecularIsHighlightsRange",
                "_SpecularIsHighLightsPow",
            ]);
            u.ext[19] = [p.f("_UseMeshSpecular"), 0.0, 0.0, 0.0];
        }
        Xukmi::Eye => {
            u.color1 = p.v("_overcolor1");
            u.color2 = p.v("_overcolor2");
            u.shadow = p.v("_shadowcolor");
            u.over1 = p.st("_overtex1");
            u.over2 = p.st("_overtex2");
            // The game switches highlights on unless the character hides
            // them; the stored value is not used.
            u.ext[17] = [1.0, p.f("_exppower"), p.f("_ExpressionSize"), p.f("_ExpressionDepth")];
            u.ext[18] = [p.f("_rotation"), 0.0, 0.0, 0.0];
        }
        Xukmi::EyeW => {
            u.color1 = p.v("_Color");
            u.shadow = p.v("_shadowcolor");
        }
    }

    // The see-through clothes take ZWrite from _AlphaOptionZWrite. The
    // outline pass draws nothing with _OutlineOn off.
    let zwrite = if sheer {
        p.f("_AlphaOptionZWrite") != 0.0
    } else {
        state.zwrite
    };
    let outline = state.outline && p.f("_OutlineOn") != 0.0;
    MaterialDesc {
        kind,
        queue: if mat.render_queue > 0 {
            mat.render_queue
        } else {
            state.queue
        },
        blended,
        blend,
        outline: outline && matches!(kind, Kind::Item | Kind::Skin | Kind::Hair),
        zwrite,
        stencil: state.stencil,
        stencil_ref: state.stencil_ref,
        shadow: state.shadow,
        uniform: u,
        textures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_chara::TexSlot;

    fn inst(shader: &str) -> MaterialInst {
        MaterialInst {
            shader: shader.into(),
            ..Default::default()
        }
    }

    fn tex() -> TexSlot {
        TexSlot {
            image: Arc::new(Image::unity_black()),
            scale: [2.0, 2.0],
            offset: [0.5, 0.25],
        }
    }

    #[test]
    fn names_match_whole_and_carry_their_program() {
        assert_eq!(shader("xukmi/MainOpaquePlus"), Some(Xukmi::Opaque));
        assert_eq!(shader("xukmi/MainOpaquePlusTess"), Some(Xukmi::Opaque));
        assert_eq!(shader("xukmi/MainItemAlphaPlus"), Some(Xukmi::ItemAlpha));
        assert_eq!(shader("xukmi/SkinPlusReflect"), Some(Xukmi::Skin));
        assert_eq!(shader("xukmi/HairFrontPlus"), Some(Xukmi::HairFront));
        assert_eq!(shader("xukmi/EyeWPlus"), Some(Xukmi::EyeW));
        // Not ported, and not a part of another name.
        assert_eq!(shader("xukmi/MainItemStudioPlus"), None);
        assert_eq!(shader("MainOpaquePlus"), None);
        assert_eq!(shader("Shader Forge/main_opaque"), None);
        for (name, x) in NAMES {
            assert!(table(name).is_some(), "{name} has a property table");
            let d = MaterialDesc::new(Some(&inst(name)));
            assert_eq!(d.uniform.info[2], x as u32);
            assert_eq!(d.kind, x.kind());
        }
    }

    #[test]
    fn cloth_takes_the_defaults_of_its_property_table() {
        let u = MaterialDesc::new(Some(&inst("xukmi/MainOpaquePlus"))).uniform;
        // _Col0.._Col3 are white. _ShadowColor is a vector marked [Gamma],
        // which Unity linearises as it does a colour.
        assert_eq!(u.color1, [1.0; 4]);
        assert_eq!(u.ext[0], [1.0; 4]);
        assert!((u.shadow[0] - 0.352).abs() < 1e-3);
        assert_eq!(u.shadow[3], 1.0);
        assert_eq!(u.over1, [1.0; 4]);
        assert_eq!(u.over2, [0.98, 0.0, 0.0, 0.0]);
        // _rimpower, _rimV, _AnotherRampFull, _UseKKMetal.
        assert_eq!(u.over3, [0.5, 0.5, 0.0, 1.0]);
        // _Cutoff, _ShadowExtend, _ShadowExtendAnother, _DetailRLineR.
        assert_eq!(u.p0, [0.5, 1.0, 0.0, 0.0]);
        assert_eq!(u.p1, [0.0, 1.0, 1.0, 1.0]);
        assert_eq!(u.p2, [1.0, 1.0, 0.0, 0.0]);
        // _CustomAmbient is a colour: two thirds, linearised.
        assert!((u.ext[1][0] - 0.402).abs() < 1e-3);
        assert_eq!(u.ext[1][3], 0.0);
        // _KKPRimSoft, _KKPRimIntensity; the scales of the normal maps.
        assert_eq!(u.ext[6], [1.5, 0.75, 0.0, 0.0]);
        assert_eq!(u.ext[5], [0.0, 0.0, 1.0, 1.0]);
        // _Saturation, _CullOption, _OutlineOn, _UseLightColorSpecular.
        assert_eq!(u.ext[16], [1.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn accessories_have_their_own_defaults() {
        let d = MaterialDesc::new(Some(&inst("xukmi/MainItemPlus")));
        let u = d.uniform;
        assert_eq!(u.color1, [1.0, 0.0, 0.0, 1.0]);
        assert!((u.color3[0] - 0.214).abs() < 1e-3);
        assert!((u.shadow[0] - 0.352).abs() < 1e-3);
        // _ShadowExtendAnother is one; the shader has no body masks, so
        // nothing sets their switches.
        assert_eq!(u.p0, [0.5, 1.0, 1.0, 0.0]);
        assert_eq!(u.p2, [0.0, 0.0, 0.0, 0.0]);
        assert!(d.outline);

        // MainItemAlphaPlus: no outline unless switched on, back faces
        // culled, depth written unless switched off.
        let mut m = inst("xukmi/MainItemAlphaPlus");
        let d = MaterialDesc::new(Some(&m));
        assert!(!d.outline);
        assert!(d.zwrite);
        assert_eq!(d.uniform.ext[16][1], 2.0);
        m.floats.insert("_AlphaOptionZWrite".into(), 0.0);
        m.floats.insert("_OutlineOn".into(), 1.0);
        let d = MaterialDesc::new(Some(&m));
        assert!(!d.zwrite);
        assert_eq!(d.uniform.ext[16][2], 1.0);
    }

    #[test]
    fn skin_hair_and_eyes_fill_their_own_fields() {
        let mut m = inst("xukmi/SkinPlus");
        m.textures.insert("_overtex1".into(), tex());
        let d = MaterialDesc::new(Some(&m));
        let u = d.uniform;
        // The body masks are on by default; the cut is at one half.
        assert_eq!(u.p0, [0.5, 1.0, 0.0, 0.0]);
        assert_eq!(u.p2, [1.0, 1.0, 0.0, 0.5]);
        // _linetexon, _LineWidthS, _tex1mask, _nip_specular.
        assert_eq!(u.p1, [1.0, 1.0, 0.0, 0.5]);
        // _SpecularColor has no alpha by default; _SpeclarHeight is 0.98.
        assert_eq!(u.ext[17], [1.0, 1.0, 1.0, 0.0]);
        assert_eq!(u.ext[18][0], 0.98);
        assert_eq!(u.ext[19], [2.0, 2.0, 0.5, 0.25]);
        assert_eq!(u.info[1], SLOTS[5].1);
        assert!(d.outline);

        let u = MaterialDesc::new(Some(&inst("xukmi/HairFrontPlus"))).uniform;
        // _Color2 and _LineColor are vectors marked [Gamma].
        assert!((u.color2[0] - 0.578).abs() < 1e-3);
        assert!((u.line[0] - 0.214).abs() < 1e-3);
        // _rimpower, _rimV, _SpeclarHeight, _SpecularHeightInvert.
        assert_eq!(u.over3, [0.5, 0.75, 0.85, 0.0]);
        assert_eq!(u.p0, [0.5, 0.5, 0.0, 0.0]);
        assert_eq!(u.ext[17], [1.0; 4]);
        assert_eq!(u.ext[18], [1.0, 0.0, 5.0, 64.0]);

        let mut m = inst("xukmi/EyePlus");
        m.floats.insert("_isHighLight".into(), 0.0);
        m.floats.insert("_rotation".into(), 0.25);
        let u = MaterialDesc::new(Some(&m)).uniform;
        // Highlights are on whatever the material stores.
        assert_eq!(u.ext[17], [1.0, 1.0, 0.35, 1.0]);
        assert_eq!(u.ext[18][0], 0.25);
        assert!((u.shadow[2] - 0.517).abs() < 1e-3);

        let u = MaterialDesc::new(Some(&inst("xukmi/EyeWPlus"))).uniform;
        assert!((u.color1[0] - 0.214).abs() < 1e-3);
        assert_eq!(u.p0[0], 0.5);
    }

    #[test]
    fn a_bound_texture_places_itself() {
        let mut m = inst("xukmi/EyePlus");
        m.tex_st.insert("_MainTex".into(), [3.0, 3.0, 0.0, 0.0]);
        assert_eq!(MaterialDesc::new(Some(&m)).uniform.st_main, [3.0, 3.0, 0.0, 0.0]);
        m.textures.insert("_MainTex".into(), tex());
        assert_eq!(MaterialDesc::new(Some(&m)).uniform.st_main, [2.0, 2.0, 0.5, 0.25]);
    }

    #[test]
    fn material_values_win_and_only_colours_are_linearised() {
        let mut m = inst("xukmi/MainOpaquePlus");
        m.colors.insert("_Col0".into(), [0.5, 0.5, 0.5, 1.0]);
        m.colors.insert("_ShadowHSV".into(), [0.5, 0.5, 0.5, 0.25]);
        m.floats.insert("_UseKKMetal".into(), 0.25);
        m.floats.insert("_Cutoff".into(), 0.1);
        m.textures.insert("_NormalMap".into(), tex());
        m.textures.insert("_EmissionMask".into(), tex());
        m.textures.insert("_ColMask".into(), tex());
        let d = MaterialDesc::new(Some(&m));
        let u = d.uniform;
        assert!((u.color1[0] - 0.214).abs() < 1e-3);
        // _ShadowHSV is a plain vector.
        assert_eq!(u.ext[4], [0.5, 0.5, 0.5, 0.0]);
        assert_eq!(u.over3[3], 0.25);
        assert_eq!(u.p0[0], 0.1);
        // The normal map sits in the tenth slot, the emission mask in the
        // ninth, the colour mask in the second.
        assert!(d.textures[9].is_some() && d.textures[8].is_some() && d.textures[1].is_some());
        assert_eq!(u.info[1], SLOTS[9].1 | SLOTS[8].1 | SLOTS[1].1);
        assert_eq!(u.ext[9], [2.0, 2.0, 0.5, 0.25]);
        assert_eq!(u.ext[12], [2.0, 2.0, 0.5, 0.25]);
        assert_eq!(u.ext[10], [1.0, 1.0, 0.0, 0.0]);
    }
}
