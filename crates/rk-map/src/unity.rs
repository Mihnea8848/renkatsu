// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Typed readers for what a map's scene holds besides its meshes: its
//! lights, the scene's render and lightmap settings, reflection probes,
//! and two of the game's own components, `SunLightInfo` (sun, fog and
//! colour filter per time of day) and `LightmapPrefab` (which renderer
//! reads which lightmap).

use rk_unityfs::{PPtr, UObject, UValue};

/// Unity class ids of the scene objects read here.
pub mod class {
    pub const RENDER_SETTINGS: i32 = 104;
    pub const LIGHT: i32 = 108;
    pub const LIGHTMAP_SETTINGS: i32 = 157;
    pub const CUBEMAP: i32 = 89;
    pub const PARTICLE_SYSTEM: i32 = 198;
    pub const REFLECTION_PROBE: i32 = 215;
    pub const LIGHT_PROBES: i32 = 258;
    pub const VIDEO_PLAYER: i32 = 328;
}

/// A colour by its named channels, as Unity stores `ColorRGBA`.
pub fn color(v: Option<&UValue>) -> Option<[f32; 4]> {
    let v = v?;
    Some([
        v.get("r")?.as_f32()?,
        v.get("g")?.as_f32()?,
        v.get("b")?.as_f32()?,
        v.get("a")?.as_f32()?,
    ])
}

/// A `Vector3f` by its named fields.
pub fn vec3(v: Option<&UValue>) -> Option<[f32; 3]> {
    let v = v?;
    Some([v.get("x")?.as_f32()?, v.get("y")?.as_f32()?, v.get("z")?.as_f32()?])
}

/// A `Vector4f` by its named fields.
pub fn vec4(v: Option<&UValue>) -> Option<[f32; 4]> {
    let v = v?;
    Some([
        v.get("x")?.as_f32()?,
        v.get("y")?.as_f32()?,
        v.get("z")?.as_f32()?,
        v.get("w")?.as_f32()?,
    ])
}

fn pptrs(v: Option<&[UValue]>) -> Vec<PPtr> {
    v.unwrap_or_default().iter().filter_map(UValue::as_pptr).collect()
}

/// The kinds of light Unity has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightKind {
    Spot,
    Directional,
    Point,
    Area,
}

/// A Light component.
#[derive(Debug, Clone, PartialEq)]
pub struct Light {
    pub game_object: PPtr,
    pub enabled: bool,
    pub kind: LightKind,
    /// Linear colour as stored, before the intensity.
    pub color: [f32; 4],
    pub intensity: f32,
    pub range: f32,
    pub spot_angle: f32,
    /// 0 none, 1 hard, 2 soft.
    pub shadows: i32,
    pub shadow_strength: f32,
    pub shadow_bias: f32,
    pub shadow_normal_bias: f32,
    /// The layers it lights, a bit each.
    pub culling_mask: u32,
    /// 4 real-time, 1 mixed, 2 baked.
    pub lightmapping: i32,
}

impl Light {
    pub fn read(o: &UObject) -> Light {
        let shadows = o.get("m_Shadows");
        let shadow = |name: &str| {
            shadows
                .and_then(|s| s.get(name))
                .and_then(UValue::as_f32)
                .unwrap_or(0.0)
        };
        Light {
            game_object: o.pptr("m_GameObject").unwrap_or_default(),
            enabled: o.i64("m_Enabled").unwrap_or(1) != 0,
            kind: match o.i64("m_Type").unwrap_or(1) {
                0 => LightKind::Spot,
                2 => LightKind::Point,
                3 => LightKind::Area,
                _ => LightKind::Directional,
            },
            color: color(o.get("m_Color")).unwrap_or([1.0; 4]),
            intensity: o.f32("m_Intensity").unwrap_or(1.0),
            range: o.f32("m_Range").unwrap_or(10.0),
            spot_angle: o.f32("m_SpotAngle").unwrap_or(30.0),
            shadows: shadow("m_Type") as i32,
            shadow_strength: shadow("m_Strength"),
            shadow_bias: shadow("m_Bias"),
            shadow_normal_bias: shadow("m_NormalBias"),
            culling_mask: o
                .get("m_CullingMask")
                .and_then(|m| m.get("m_Bits"))
                .and_then(UValue::as_i64)
                .unwrap_or(-1) as u32,
            lightmapping: o.i64("m_Lightmapping").unwrap_or(4) as i32,
        }
    }
}

/// The game's `MirrorReflection` component, on a mirror's renderer.
#[derive(Debug, Clone, PartialEq)]
pub struct MirrorReflection {
    pub game_object: PPtr,
    pub enabled: bool,
    /// `m_DisablePixelLights`: the reflection is drawn with no pixel
    /// lights but the main one.
    pub disable_pixel_lights: bool,
    /// `m_TextureSize`: the side of the reflection's square texture.
    pub texture_size: i32,
    /// `m_ClipPlaneOffset`.
    pub clip_offset: f32,
    /// `m_ReflectLayers`: the layers reflected, a bit each.
    pub layers: u32,
}

impl MirrorReflection {
    /// Reads the component; a field left out has the script's default.
    pub fn read(o: &UObject) -> MirrorReflection {
        MirrorReflection {
            game_object: o.pptr("m_GameObject").unwrap_or_default(),
            enabled: o.i64("m_Enabled").unwrap_or(1) != 0,
            disable_pixel_lights: o.i64("m_DisablePixelLights").unwrap_or(1) != 0,
            texture_size: o.i64("m_TextureSize").unwrap_or(256) as i32,
            clip_offset: o.f32("m_ClipPlaneOffset").unwrap_or(0.07),
            layers: o
                .get("m_ReflectLayers")
                .and_then(|m| m.get("m_Bits"))
                .and_then(UValue::as_i64)
                .unwrap_or(-1) as u32,
        }
    }
}

/// How fog thickens with distance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FogMode {
    Linear,
    Exponential,
    ExponentialSquared,
}

/// Where a scene's ambient light comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbientMode {
    /// The sky, as the spherical harmonics in `ambient_probe`.
    Skybox,
    /// Sky, equator and ground colours.
    Gradient,
    /// The sky colour alone.
    Flat,
}

/// The scene's `RenderSettings`: fog, ambient light, sky and reflections.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderSettings {
    pub fog: bool,
    pub fog_mode: FogMode,
    pub fog_color: [f32; 4],
    pub fog_density: f32,
    pub fog_start: f32,
    pub fog_end: f32,
    pub ambient_mode: AmbientMode,
    pub ambient_sky: [f32; 4],
    pub ambient_equator: [f32; 4],
    pub ambient_ground: [f32; 4],
    pub ambient_intensity: f32,
    /// Unity's `SphericalHarmonicsL2`, 27 numbers.
    pub ambient_probe: Vec<f32>,
    pub skybox: PPtr,
    pub sun: PPtr,
    /// 0 the sky, 1 `custom_reflection`.
    pub reflection_mode: i32,
    pub custom_reflection: PPtr,
    /// The cubemap Unity baked from the sky.
    pub sky_reflection: PPtr,
    pub reflection_intensity: f32,
    /// What Unity hands to shaders as `unity_IndirectSpecColor`, as stored.
    pub indirect_specular: [f32; 4],
}

impl RenderSettings {
    pub fn read(o: &UObject) -> RenderSettings {
        let probe = o
            .get("m_AmbientProbe")
            .and_then(UValue::as_object)
            .map(|p| p.fields.iter().filter_map(|(_, v)| v.as_f32()).collect())
            .unwrap_or_default();
        RenderSettings {
            fog: o.bool("m_Fog").unwrap_or(false),
            fog_mode: match o.i64("m_FogMode").unwrap_or(3) {
                1 => FogMode::Linear,
                2 => FogMode::Exponential,
                _ => FogMode::ExponentialSquared,
            },
            fog_color: color(o.get("m_FogColor")).unwrap_or([0.5, 0.5, 0.5, 1.0]),
            fog_density: o.f32("m_FogDensity").unwrap_or(0.01),
            fog_start: o.f32("m_LinearFogStart").unwrap_or(0.0),
            fog_end: o.f32("m_LinearFogEnd").unwrap_or(300.0),
            ambient_mode: match o.i64("m_AmbientMode").unwrap_or(0) {
                1 => AmbientMode::Gradient,
                3 => AmbientMode::Flat,
                _ => AmbientMode::Skybox,
            },
            ambient_sky: color(o.get("m_AmbientSkyColor")).unwrap_or([0.0; 4]),
            ambient_equator: color(o.get("m_AmbientEquatorColor")).unwrap_or([0.0; 4]),
            ambient_ground: color(o.get("m_AmbientGroundColor")).unwrap_or([0.0; 4]),
            ambient_intensity: o.f32("m_AmbientIntensity").unwrap_or(1.0),
            ambient_probe: probe,
            skybox: o.pptr("m_SkyboxMaterial").unwrap_or_default(),
            sun: o.pptr("m_Sun").unwrap_or_default(),
            reflection_mode: o.i64("m_DefaultReflectionMode").unwrap_or(0) as i32,
            custom_reflection: o.pptr("m_CustomReflection").unwrap_or_default(),
            sky_reflection: o.pptr("m_GeneratedSkyboxReflection").unwrap_or_default(),
            reflection_intensity: o.f32("m_ReflectionIntensity").unwrap_or(1.0),
            indirect_specular: color(o.get("m_IndirectSpecularColor")).unwrap_or([0.0, 0.0, 0.0, 1.0]),
        }
    }
}

/// One baked lightmap: the light texture and its direction texture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lightmap {
    pub light: PPtr,
    pub direction: PPtr,
}

/// The scene's `LightmapSettings`.
#[derive(Debug, Clone, PartialEq)]
pub struct LightmapSettings {
    pub lightmaps: Vec<Lightmap>,
    /// 0 non-directional, 1 directional.
    pub mode: i32,
    pub light_probes: PPtr,
}

impl LightmapSettings {
    pub fn read(o: &UObject) -> LightmapSettings {
        let lightmaps = o
            .array("m_Lightmaps")
            .unwrap_or_default()
            .iter()
            .filter_map(|l| {
                Some(Lightmap {
                    light: l.get("m_Lightmap")?.as_pptr()?,
                    direction: l.get("m_DirLightmap").and_then(UValue::as_pptr).unwrap_or_default(),
                })
            })
            .collect();
        LightmapSettings {
            lightmaps,
            mode: o.i64("m_LightmapsMode").unwrap_or(1) as i32,
            light_probes: o.pptr("m_LightProbes").unwrap_or_default(),
        }
    }
}

/// A ReflectionProbe component.
#[derive(Debug, Clone, PartialEq)]
pub struct ReflectionProbe {
    pub game_object: PPtr,
    pub enabled: bool,
    /// 0 baked, 1 real-time, 2 custom.
    pub mode: i32,
    pub size: [f32; 3],
    pub offset: [f32; 3],
    pub box_projection: bool,
    pub intensity: f32,
    pub importance: i32,
    pub baked: PPtr,
    pub custom: PPtr,
}

impl ReflectionProbe {
    pub fn read(o: &UObject) -> ReflectionProbe {
        ReflectionProbe {
            game_object: o.pptr("m_GameObject").unwrap_or_default(),
            enabled: o.i64("m_Enabled").unwrap_or(1) != 0,
            mode: o.i64("m_Mode").unwrap_or(0) as i32,
            size: vec3(o.get("m_BoxSize")).unwrap_or([10.0; 3]),
            offset: vec3(o.get("m_BoxOffset")).unwrap_or([0.0; 3]),
            box_projection: o.bool("m_BoxProjection").unwrap_or(false),
            intensity: o.f32("m_IntensityMultiplier").unwrap_or(1.0),
            importance: o.i64("m_Importance").unwrap_or(1) as i32,
            baked: o.pptr("m_BakedTexture").unwrap_or_default(),
            custom: o.pptr("m_CustomBakedTexture").unwrap_or_default(),
        }
    }
}

/// The times of day `SunLightInfo` holds, as `sceneInfo.sunLightType`
/// numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeOfDay {
    Day,
    Evening,
    Night,
}

impl TimeOfDay {
    pub fn from_number(n: i64) -> Option<TimeOfDay> {
        match n {
            0 => Some(TimeOfDay::Day),
            1 => Some(TimeOfDay::Evening),
            2 => Some(TimeOfDay::Night),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            TimeOfDay::Day => "Day",
            TimeOfDay::Evening => "Evening",
            TimeOfDay::Night => "Night",
        }
    }
}

/// One time of day of `SunLightInfo`: what `SunLightInfo.Info.Set` puts
/// on the scene.
#[derive(Debug, Clone, PartialEq)]
pub struct SunTime {
    pub time: Option<TimeOfDay>,
    /// Objects shown only at this time.
    pub visible: Vec<PPtr>,
    /// The sun's Euler angles in degrees, its colour and intensity.
    pub angle: [f32; 3],
    pub color: [f32; 4],
    pub intensity: f32,
    pub fog: bool,
    pub fog_color: [f32; 4],
    pub fog_start: f32,
    pub fog_end: f32,
    pub sun_shafts_color: [f32; 4],
    pub sun_shafts: PPtr,
    /// The colour filter: a LUT, a second one blended in, and how much.
    pub lut: PPtr,
    pub lut_blend: PPtr,
    pub lut_blend_amount: f32,
    /// A flat picture behind everything, rarely set.
    pub back: PPtr,
}

/// The game's `SunLightInfo` component on a map's root.
#[derive(Debug, Clone, PartialEq)]
pub struct SunLightInfo {
    pub times: Vec<SunTime>,
}

impl SunLightInfo {
    pub fn read(o: &UObject) -> SunLightInfo {
        let times = o
            .array("_infos")
            .unwrap_or_default()
            .iter()
            .filter_map(UValue::as_object)
            .map(|i| SunTime {
                time: i.i64("type").and_then(TimeOfDay::from_number),
                visible: pptrs(i.array("visibleList")),
                angle: vec3(i.get("angle")).unwrap_or([50.0, -30.0, 0.0]),
                color: color(i.get("color")).unwrap_or([1.0; 4]),
                intensity: i.f32("intensity").unwrap_or(1.0),
                fog: i.i64("fogUse").unwrap_or(0) != 0,
                fog_color: color(i.get("fogColor")).unwrap_or([0.5, 0.5, 0.5, 1.0]),
                fog_start: i.f32("fogStart").unwrap_or(0.0),
                fog_end: i.f32("fogEnd").unwrap_or(300.0),
                sun_shafts_color: color(i.get("sunShaftsColor")).unwrap_or([0.0; 4]),
                sun_shafts: i.pptr("sunShaftsTransform").unwrap_or_default(),
                lut: i.pptr("aceLutTexture").unwrap_or_default(),
                lut_blend: i.pptr("aceLutBlendTexture").unwrap_or_default(),
                lut_blend_amount: i.f32("aceBlendAmount").unwrap_or(0.0),
                back: i.pptr("backTexture").unwrap_or_default(),
            })
            .collect();
        SunLightInfo { times }
    }
}

/// One renderer's lightmap as `LightmapPrefab` stores it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightmapUse {
    pub renderer: PPtr,
    pub index: i32,
    pub scale_offset: [f32; 4],
}

/// The game's `LightmapPrefab` component: the lightmap of each renderer,
/// put back on them when the scene starts.
pub fn lightmap_prefab(o: &UObject) -> Vec<LightmapUse> {
    o.array("lightmapParameters")
        .unwrap_or_default()
        .iter()
        .filter_map(|p| {
            Some(LightmapUse {
                renderer: p.get("renderer")?.as_pptr()?,
                index: p.get("lightmapIndex")?.as_i64()? as i32,
                scale_offset: vec4(p.get("scaleOffset")).unwrap_or([1.0, 1.0, 0.0, 0.0]),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn obj(fields: Vec<(&str, UValue)>) -> UObject {
        UObject {
            ty: Arc::from("x"),
            fields: fields.into_iter().map(|(n, v)| (Arc::from(n), v)).collect(),
        }
    }

    fn rgba(r: f64, g: f64, b: f64, a: f64) -> UValue {
        // Stored in any order; read by name.
        UValue::Object(Box::new(obj(vec![
            ("a", UValue::Float(a)),
            ("b", UValue::Float(b)),
            ("g", UValue::Float(g)),
            ("r", UValue::Float(r)),
        ])))
    }

    #[test]
    fn colours_are_read_by_channel_name() {
        assert_eq!(color(Some(&rgba(0.25, 0.5, 0.75, 1.0))), Some([0.25, 0.5, 0.75, 1.0]));
    }

    #[test]
    fn a_light_reads_its_kind_and_shadows() {
        let shadows = UValue::Object(Box::new(obj(vec![
            ("m_Type", UValue::Int(2)),
            ("m_Strength", UValue::Float(0.8)),
        ])));
        let mask = UValue::Object(Box::new(obj(vec![("m_Bits", UValue::Int(2048))])));
        let light = Light::read(&obj(vec![
            ("m_Type", UValue::Int(2)),
            ("m_Color", rgba(1.0, 0.5, 0.25, 1.0)),
            ("m_Intensity", UValue::Float(1.5)),
            ("m_Shadows", shadows),
            ("m_CullingMask", mask),
        ]));
        assert_eq!(light.kind, LightKind::Point);
        assert_eq!(light.shadows, 2);
        assert!((light.shadow_strength - 0.8).abs() < 1e-6);
        assert_eq!(light.culling_mask, 2048);
    }
}
