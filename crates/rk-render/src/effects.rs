// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! CharaStudio's screen effects.
//!
//! The Studio camera carries, in this order, GlobalFog, Amplify Occlusion,
//! BloomAndFlares, Amplify Color, SunShafts, VignetteAndChromaticAberration
//! and DepthOfField. The
//! values here are the ones the Studio scene stores for those components
//! (`CharaStudio_Data/level1`) with what `Studio.SceneInfo.Init` and
//! `SystemButtonCtrl` write over them. The camera is HDR, so everything up
//! to Amplify Color works on unclamped linear colour, and what follows it
//! on eight bit targets (`[ImageEffectTransformsToLDR]`). The shader side
//! is `effects.wgsl`.

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use rk_chara::Image;
use wgpu::util::DeviceExt;

/// Amplify Occlusion as the Studio scene sets it up: applied as a post
/// effect, medium sample count, normals rebuilt from depth, no distance
/// fade, no downsampling, and one blur pass of radius 4.
#[derive(Debug, Clone, PartialEq)]
pub struct AmbientOcclusion {
    /// `Tint`: the colour fully occluded pixels are multiplied by. The
    /// shader gets it as stored (`Shader.SetGlobalColor` in Unity 5.6).
    pub tint: [f32; 3],
    pub intensity: f32,
    /// `Radius`, in world units.
    pub radius: f32,
    pub power_exponent: f32,
    pub bias: f32,
    pub blur_sharpness: f32,
}

impl Default for AmbientOcclusion {
    fn default() -> Self {
        Self {
            // SceneInfo.Init: Utility.ConvertColor(180, 180, 180).
            tint: [180.0 / 255.0; 3],
            intensity: 1.0,
            // SceneInfo.Init: aoeRadius.
            radius: 0.1,
            power_exponent: 1.0,
            bias: 0.25,
            blur_sharpness: 10.0,
        }
    }
}

/// BloomAndFlares with the scene's Add blend mode, no lens flares and no
/// alpha mask. On the HDR camera the threshold is subtracted as it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Bloom {
    pub intensity: f32,
    pub threshold: f32,
    /// `sepBlurSpread`.
    pub blur_spread: f32,
    /// `bloomBlurIterations`.
    pub iterations: u32,
}

impl Default for Bloom {
    fn default() -> Self {
        // SceneInfo.Init: bloomIntensity, bloomThreshold, bloomBlur.
        Self {
            intensity: 0.4,
            threshold: 0.6,
            blur_spread: 0.8,
            iterations: 2,
        }
    }
}

/// Amplify Color: a 1024 by 32 colour table of 32 blue slices, looked up
/// in gamma space. The scene has no tone mapping and no dithering.
#[derive(Debug, Clone)]
pub struct ColorFilter {
    /// The table, as a data texture (not sRGB).
    pub lut: Arc<Image>,
    pub exposure: f32,
    /// Amplify Color's `BlendAmount` (CharaStudio's `aceBlend`), 0 to 1.
    /// The scene sets no second table, so the table fades to the neutral
    /// one. 0 reads the table as it is.
    pub blend: f32,
}

/// VignetteAndChromaticAberration, with the scene's zero aberration.
#[derive(Debug, Clone, PartialEq)]
pub struct Vignette {
    pub intensity: f32,
    pub blur: f32,
    pub blur_spread: f32,
}

impl Default for Vignette {
    fn default() -> Self {
        Self {
            intensity: 0.1,
            blur: 0.2,
            blur_spread: 0.75,
        }
    }
}

/// `RenderSettings.fogMode` with the numbers each mode reads, which
/// GlobalFog takes from the scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FogMode {
    /// `FogMode.Linear`: what a map with a `SunLightInfo` sets, with the
    /// map's start and end distance.
    Linear { start: f32, end: f32 },
    /// `FogMode.Exponential`.
    Exponential { density: f32 },
    /// `FogMode.ExponentialSquared`: the Studio scene's own, density 0.01.
    ExponentialSquared { density: f32 },
}

/// GlobalFog (`UnityStandardAssets.ImageEffects.GlobalFog`) as the Studio
/// camera carries it (`CharaStudio_Data/level1`, MonoBehaviour 8974):
/// height fog on, distance fog off, radial distance on, height density
/// 0.001. CharaStudio's Fog panel writes the colour, `height` and
/// `start_distance` (`SystemButtonCtrl.FogInfo`); the rest never changes
/// in the Studio.
#[derive(Debug, Clone, PartialEq)]
pub struct Fog {
    /// `RenderSettings.fogColor`, sRGB as the scene stores it.
    pub color: [f32; 3],
    /// The fog's top, a world height (0 to 50 in the Studio).
    pub height: f32,
    /// Pushes the fog away from the camera (0 to 50 in the Studio).
    pub start_distance: f32,
    pub height_density: f32,
    pub height_fog: bool,
    pub distance_fog: bool,
    /// Distance fog by the distance to the eye, not by depth.
    pub radial: bool,
    pub mode: FogMode,
    /// The Studio camera's clip planes. GlobalFog takes a pixel nothing
    /// was drawn on as lying on the far plane, and distance fog starts at
    /// the near plane.
    pub camera_near: f32,
    pub camera_far: f32,
    /// Take the map's own fog colour, when a map with fog is shown.
    pub color_from_map: bool,
    /// Renkatsu's own fog, set in full: a map's fog neither changes it nor
    /// is drawn under it.
    pub own: bool,
}

impl Default for Fog {
    fn default() -> Self {
        Self {
            color: [137.0 / 255.0, 193.0 / 255.0, 221.0 / 255.0],
            height: 1.0,
            start_distance: 0.0,
            height_density: 0.001,
            height_fog: true,
            distance_fog: false,
            radial: true,
            mode: FogMode::ExponentialSquared { density: 0.01 },
            camera_near: 0.06,
            camera_far: 1000.0,
            color_from_map: false,
            own: false,
        }
    }
}

/// DepthOfField (`UnityStandardAssets.ImageEffects.DepthOfField`) as the
/// Studio camera carries it (`CharaStudio_Data/level1`, MonoBehaviour
/// 8968): disc blur, high sample count, not high resolution, near blur on.
/// CharaStudio's panel writes `focal_size` and `aperture`
/// (`SystemButtonCtrl.DOFInfo`); the rest never changes in the Studio.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthOfField {
    /// `focalSize`, 0 to 2: how far around the focus stays sharp.
    pub focal_size: f32,
    /// `aperture`, 0 to 1.
    pub aperture: f32,
    /// The eye depth of the point in focus. The component's
    /// `focalTransform` is the camera's `CameraTarget`, which
    /// `Studio.CameraControl` keeps on the point the camera orbits; `None`
    /// takes the frame camera's target.
    pub focus: Option<f32>,
    /// `maxBlurSize`.
    pub max_blur_size: f32,
    /// `nearBlur`: what is in front of the focus blurs over it.
    pub near_blur: bool,
    /// `foregroundOverlap`.
    pub foreground_overlap: f32,
    /// The Studio camera's far plane, which depths are measured in.
    pub camera_far: f32,
}

impl Default for DepthOfField {
    fn default() -> Self {
        Self {
            // SceneInfo.Init: depthFocalSize, depthAperture.
            focal_size: 0.95,
            aperture: 0.6,
            focus: None,
            max_blur_size: 2.0,
            near_blur: true,
            foreground_overlap: 2.0,
            camera_far: 1000.0,
        }
    }
}

/// `SunShafts.ShaftsScreenBlendMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaftBlend {
    Screen,
    Add,
}

/// SunShafts (`UnityStandardAssets.ImageEffects.SunShafts`) as the Studio
/// camera carries it (`CharaStudio_Data/level1`, MonoBehaviour 8970):
/// normal resolution, Add, two blur iterations, blur radius 3, intensity
/// 2.2, largest radius 0.5, with the depth texture. CharaStudio's panel
/// writes the two colours and the caster (`SystemButtonCtrl.SunShaftsInfo`,
/// `SetSunCaster`); a map with a `SunLightInfo` sets the colour and the
/// caster of its time of day (`SunLightInfo.Info.SetSunShafts`).
#[derive(Debug, Clone, PartialEq)]
pub struct SunShafts {
    /// `sunThreshold`, as the colour is stored: the shader compares it
    /// with the linear picture as it is (`SetVector`).
    pub threshold: [f32; 3],
    /// `sunColor`, as stored (`SetVector`).
    pub color: [f32; 3],
    /// `sunTransform.position`, in the world. `None` is the component
    /// without a caster: the middle of the screen.
    pub caster: Option<Vec3>,
    /// What `resolution` divides the frame by: 4 (Low), 2 (Normal), 1
    /// (High).
    pub divisor: u32,
    pub blend: ShaftBlend,
    /// `radialBlurIterations`, 1 to 4.
    pub iterations: u32,
    /// `sunShaftBlurRadius`.
    pub blur_radius: f32,
    /// `sunShaftIntensity`.
    pub intensity: f32,
    /// `maxRadius`, in screen heights and widths alike.
    pub max_radius: f32,
    /// The Studio camera's far plane: rays start where the depth is past
    /// 0.99 of it.
    pub camera_far: f32,
}

impl Default for SunShafts {
    fn default() -> Self {
        Self {
            // SceneInfo.Init: sunThresholdColor (128, 128, 128), sunColor
            // (255, 255, 255), sunCaster -1.
            threshold: [128.0 / 255.0; 3],
            color: [1.0; 3],
            caster: None,
            divisor: 2,
            blend: ShaftBlend::Add,
            iterations: 2,
            blur_radius: 3.0,
            intensity: 2.2,
            max_radius: 0.5,
            camera_far: 1000.0,
        }
    }
}

/// The screen effects of a CharaStudio scene. `None` switches one off.
#[derive(Debug, Clone, Default)]
pub struct StudioEffects {
    pub ao: Option<AmbientOcclusion>,
    pub bloom: Option<Bloom>,
    pub color: Option<ColorFilter>,
    pub vignette: Option<Vignette>,
    /// GlobalFog. Off in a new scene.
    pub fog: Option<Fog>,
    /// SunShafts. Off in a new scene.
    pub shafts: Option<SunShafts>,
    /// DepthOfField. Off in a new scene.
    pub dof: Option<DepthOfField>,
    /// A plain colour (sRGB) for the parts of the picture where nothing
    /// was drawn, kept out of the effects: the editor's grey when the
    /// scene has no map. `None` puts the effects on the whole picture, as
    /// CharaStudio does.
    pub backdrop: Option<[f64; 4]>,
}

impl StudioEffects {
    /// What a new Studio scene has on: occlusion, bloom, the colour table
    /// of filter 0 when the caller found it, and the vignette.
    pub fn studio(lut: Option<Arc<Image>>) -> Self {
        Self {
            ao: Some(AmbientOcclusion::default()),
            bloom: Some(Bloom::default()),
            color: lut.map(|lut| ColorFilter {
                lut,
                exposure: 1.0,
                blend: 0.0,
            }),
            vignette: Some(Vignette::default()),
            fog: None,
            shafts: None,
            dof: None,
            backdrop: None,
        }
    }
}

/// The first values of the game's `RandomTable.Values`, which Amplify
/// Occlusion builds its 4 by 4 random texture from.
const RANDOM_TABLE: [f32; 32] = [
    0.4639, 0.34, 0.223, 0.4684, 0.3222, 0.9792, 0.0317, 0.9733, 0.7783, 0.4561, 0.2585, 0.33, 0.3873, 0.3801, 0.1798,
    0.9107, 0.5116, 0.0929, 0.1807, 0.6201, 0.1013, 0.5563, 0.6424, 0.442, 0.2151, 0.4752, 0.1573, 0.5688, 0.5012,
    0.6292, 0.6992, 0.7077,
];

/// The random texture's pixels (`GenerateRandomizationData`): the cosine
/// and sine of an angle within an eighth of a turn, and a ray offset. The
/// texture is ARGB32, so each value is rounded to eight bits.
pub fn random_data() -> [[f32; 4]; 16] {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5).floor() / 255.0;
    std::array::from_fn(|i| {
        let angle = std::f32::consts::TAU * RANDOM_TABLE[i * 2] / 8.0;
        [byte(angle.cos()), byte(angle.sin()), byte(RANDOM_TABLE[i * 2 + 1]), 0.0]
    })
}

/// `_AO_UVToView` and `_AO_HalfProjScale` (`UpdateGlobalShaderConstants`).
pub fn ao_projection(fov_y_deg: f32, width: u32, height: u32) -> ([f32; 4], f32) {
    let tan = (fov_y_deg.to_radians() * 0.5).tan();
    let inv = [tan * width as f32 / height as f32, tan];
    let proj_scale = height as f32 / (tan * 2.0);
    ([2.0 * inv[0], -2.0 * inv[1], -inv[0], inv[1]], 0.5 * proj_scale)
}

/// What the picture is multiplied by where the occlusion term is
/// `occlusion` (1 is open, 0 fully occluded).
pub fn ao_factor(ao: &AmbientOcclusion, occlusion: f32) -> [f32; 3] {
    ao.tint
        .map(|t| 1.0 + ao.intensity * ((t + (1.0 - t) * occlusion) - 1.0))
}

/// Weight of a blur tap `k` pixels from the centre, for eye depths
/// `centre` and `tap`.
pub fn ao_blur_weight(k: i32, centre: f32, tap: f32, sharpness: f32) -> f32 {
    let sharp = ((1.0 - centre).clamp(0.0, 1.0) * 100.0 + 1.0) * sharpness;
    let dd = (centre - tap) * sharp;
    (-dd * dd - (k * k) as f32 * 0.125).exp()
}

/// Tap weights of the bloom blur (Hidden/SeparableBlurPlus): the centre,
/// then the pairs at 1, 2, 3 and 6.5 steps.
pub const BLOOM_BLUR: [(f32, f32); 5] = [(0.0, 0.225), (1.0, 0.15), (2.0, 0.11), (3.0, 0.075), (6.5, 0.0525)];
/// Tap weights of the vignette blur (Hidden/SeparableBlur).
pub const VIGNETTE_BLUR: [(f32, f32); 4] = [(0.0, 0.4), (1.0, 0.15), (2.0, 0.1), (3.0, 0.05)];

/// The vertical and horizontal step of bloom blur iteration `i`, in UV.
pub fn bloom_steps(bloom: &Bloom, i: u32, aspect: f32) -> [[f32; 2]; 2] {
    let spread = (1.0 + i as f32 * 0.5) * bloom.blur_spread;
    let one = 2.0 / 1024.0;
    [[0.0, spread * one], [spread / aspect * one, 0.0]]
}

/// The vertical and horizontal step of the vignette blur, in UV.
pub fn vignette_steps(v: &Vignette, aspect: f32) -> [[f32; 2]; 2] {
    let one = v.blur_spread * (2.0 / 1024.0);
    [[0.0, one], [one / aspect, 0.0]]
}

/// `_Intensity` and `_Blur` as the vignette material gets them.
pub fn vignette_params(v: &Vignette) -> [f32; 2] {
    [1.0 / (1.0 - v.intensity) - 1.0, 1.0 / (1.0 - v.blur) - 1.0]
}

/// How far the picture fades to its blurred copy and what it is multiplied
/// by, at `uv`.
pub fn vignette_at(v: &Vignette, uv: [f32; 2]) -> (f32, f32) {
    let [intensity, blur] = vignette_params(v);
    let c = [(uv[0] - 0.5) * 2.0, (uv[1] - 0.5) * 2.0];
    let d = c[0] * c[0] + c[1] * c[1];
    ((d * blur).clamp(0.0, 1.0), 1.0 - d * intensity)
}

/// Amplify Color's linear to gamma step.
pub fn to_gamma(x: f32) -> f32 {
    (x.max(0.0).powf(0.416_666_66) * 1.055 - 0.055).clamp(0.0, 1.0)
}

/// Amplify Color's gamma to linear step.
pub fn to_linear(t: f32) -> f32 {
    t * (t * (t * 0.305_306_02 + 0.682_171_1) + 0.012_522_878)
}

/// The two table positions a gamma colour is read at, and the weight of
/// the second. V counts from the image's bottom row.
pub fn lut_coords(g: [f32; 3]) -> ([f32; 2], [f32; 2], f32) {
    let blue = g[2] * 31.0;
    let f = blue.fract();
    let slice = blue - f;
    let u = g[0] * (31.0 / 1024.0) + 0.5 / 1024.0 + slice / 32.0;
    let v = g[1] * (31.0 / 32.0) + 0.5 / 32.0;
    ([u, v], [u + 1.0 / 32.0, v], f)
}

/// A texel of Amplify Color's neutral table (`CreateDefaultLut`): red
/// along a slice, green up the rows, blue from slice to slice, each cut
/// down to a byte.
pub fn neutral_texel(x: u32, y: u32) -> [u8; 3] {
    [x % 32, y, x / 32].map(|k| (k * 255 / 31) as u8)
}

/// A texel of the blend cache: the table faded to the neutral one by
/// `blend` (Hidden/Amplify Color/BlendCache), stored in eight bits.
pub fn blend_texel(table: [u8; 3], neutral: [u8; 3], blend: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| {
        let (a, b) = (table[i] as f32 / 255.0, neutral[i] as f32 / 255.0);
        ((a + blend * (b - a)) * 255.0 + 0.5).floor() / 255.0
    })
}

fn bilinear(image: &Image, uv: [f32; 2], blend: f32) -> [f32; 3] {
    let (w, h) = (image.width as i64, image.height as i64);
    let x = uv[0] * w as f32 - 0.5;
    let y = uv[1] * h as f32 - 0.5;
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |x: i64, y: i64| {
        let (x, y) = (x.clamp(0, w - 1), y.clamp(0, h - 1));
        let i = ((y * w + x) * 4) as usize;
        let table = [0, 1, 2].map(|c| image.rgba[i + c]);
        if blend == 0.0 {
            table.map(|c| c as f32 / 255.0)
        } else {
            blend_texel(table, neutral_texel(x as u32, y as u32), blend)
        }
    };
    let (x0, y0) = (x0 as i64, y0 as i64);
    let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
    [0, 1, 2].map(|i| (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy)
}

/// A linear colour through the colour table, as the shader does it. With
/// a blend the game reads the blend cache in place of the table, with the
/// same lookup (Hidden/Amplify Color/Blend).
pub fn filter_color(filter: &ColorFilter, linear: [f32; 3]) -> [f32; 3] {
    let g = linear.map(|c| to_gamma(c * filter.exposure));
    let (a, b, f) = lut_coords(g);
    let blend = filter.blend.clamp(0.0, 1.0);
    let (a, b) = (bilinear(&filter.lut, a, blend), bilinear(&filter.lut, b, blend));
    [0, 1, 2].map(|i| to_linear(a[i] + (b[i] - a[i]) * f))
}

/// `_HeightParams` as `GlobalFog.OnRenderImage` sets it: the fog's top,
/// the camera's height over it, whether the camera is inside the fog, and
/// half the height density.
pub fn fog_height_params(fog: &Fog, camera_y: f32) -> [f32; 4] {
    let over = camera_y - fog.height;
    [
        fog.height,
        over,
        if over <= 0.0 { 1.0 } else { 0.0 },
        fog.height_density * 0.5,
    ]
}

/// `_SceneFogMode.x` and `_SceneFogParams`: the scene's fog mode as a
/// number, and its density over the square root of ln 2, its density over
/// ln 2, and the linear mode's slope and offset.
pub fn fog_scene_params(mode: FogMode) -> (f32, [f32; 4]) {
    // The component writes 1.442695f, which is this number as a float.
    use std::f32::consts::LOG2_E;
    match mode {
        FogMode::Linear { start, end } => {
            let range = end - start;
            let inv = if range.abs() > 0.0001 { 1.0 / range } else { 0.0 };
            (1.0, [0.0, 0.0, -inv, end * inv])
        }
        FogMode::Exponential { density } => (2.0, [density * 1.201_122_4, density * LOG2_E, 0.0, 0.0]),
        FogMode::ExponentialSquared { density } => (3.0, [density * 1.201_122_4, density * LOG2_E, 0.0, 0.0]),
    }
}

/// Renkatsu's own fog as the shaders take it for what is drawn after
/// GlobalFog: its linear colour, and its density, the mist's top, half the
/// mist's height density and its parts (1 distance, 2 height). None for
/// CharaStudio's fog, which those surfaces do not take.
pub fn own_fog_params(fog: &Fog) -> Option<([f32; 4], [f32; 4])> {
    if !fog.own {
        return None;
    }
    let density = match fog.mode {
        FogMode::ExponentialSquared { density } => density,
        _ => return None,
    };
    let [r, g, b, _] = crate::material::linear([fog.color[0], fog.color[1], fog.color[2], 1.0]);
    let flags = u8::from(fog.distance_fog) | u8::from(fog.height_fog) << 1;
    Some((
        [r, g, b, 1.0],
        [density, fog.height, fog.height_density * 0.5, f32::from(flags)],
    ))
}

/// How much of the picture GlobalFog keeps at a point, 1 for no fog
/// (Hidden/GlobalFog, programs 0, 2 and 3: distance, both, height).
/// `v` goes from the camera to the point and `eye_depth` is the point's
/// depth along the view.
pub fn fog_factor(fog: &Fog, camera_y: f32, v: [f32; 3], eye_depth: f32) -> f32 {
    let length = |s: f32| ((v[0] * s).powi(2) + (v[1] * s).powi(2) + (v[2] * s).powi(2)).sqrt();
    let mut g = -fog.start_distance.max(0.0);
    if fog.distance_fog {
        let distance = if fog.radial { length(1.0) } else { eye_depth };
        g += distance - fog.camera_near;
    }
    if fog.height_fog {
        let [top, over, inside, density] = fog_height_params(fog, camera_y);
        let point_over = v[1] + camera_y - top;
        let c1 = inside * (point_over + over);
        let c2 = ((1.0 - 2.0 * inside) * point_over).min(0.0);
        g += -length(density) * (c1 - c2 * c2 / (v[1] + 1.0e-5).abs());
    }
    let g = g.max(0.0);
    let (mode, p) = fog_scene_params(fog.mode);
    let factor = if mode == 1.0 {
        g * p[2] + p[3]
    } else if mode == 2.0 {
        (-g * p[1]).exp2()
    } else {
        (-(g * p[0]).powi(2)).exp2()
    };
    factor.clamp(0.0, 1.0)
}

/// GlobalFog's rays, as its `_FrustumCornersWS` gives them between the
/// corners: the way from the camera to a pixel's point for one unit of
/// eye depth is `z + x * ndc.x + y * ndc.y`.
pub fn fog_rays(forward: Vec3, fov_y_deg: f32, aspect: f32) -> [Vec3; 3] {
    let right = Vec3::Y.cross(forward).normalize_or_zero();
    let up = forward.cross(right);
    let tan = (fov_y_deg.to_radians() * 0.5).tan();
    [right * tan * aspect, up * tan, forward]
}

/// A point of the world as `Camera.WorldToViewportPoint` gives it: across
/// and up the screen from 0 to 1, and the depth along the view.
pub fn viewport_point(point: Vec3, eye: Vec3, forward: Vec3, fov_y_deg: f32, aspect: f32) -> [f32; 3] {
    let [x, y, z] = fog_rays(forward, fov_y_deg, aspect);
    let v = point - eye;
    let depth = v.dot(z);
    if depth.abs() <= 1.0e-7 {
        return [0.0; 3];
    }
    let ndc = |axis: Vec3| v.dot(axis) / (axis.length_squared() * depth);
    [ndc(x) * 0.5 + 0.5, ndc(y) * 0.5 + 0.5, depth]
}

/// `_SunPosition.xyz` as `SunShafts.OnRenderImage` sets it: the caster on
/// the screen, or the middle of the screen without one.
pub fn shaft_sun(shafts: &SunShafts, eye: Vec3, forward: Vec3, fov_y_deg: f32, aspect: f32) -> [f32; 3] {
    match shafts.caster {
        Some(p) => viewport_point(p, eye, forward, fov_y_deg, aspect),
        None => [0.5, 0.5, 0.0],
    }
}

/// `_SunColor`: the shaft colour times the intensity, and nothing with
/// the caster behind the camera.
pub fn shaft_color(shafts: &SunShafts, sun_depth: f32) -> [f32; 3] {
    if sun_depth >= 0.0 {
        shafts.color.map(|c| c * shafts.intensity)
    } else {
        [0.0; 3]
    }
}

/// `_BlurRadius4.x` of each radial blur, in the order the component runs
/// them: two per iteration, each reaching further.
pub fn shaft_radii(shafts: &SunShafts) -> Vec<f32> {
    let r = shafts.blur_radius;
    let mut radius = r * 0.001_302_083_4;
    let mut out = Vec::new();
    for i in 0..shafts.iterations.clamp(1, 4) {
        let i = i as f32;
        out.push(radius);
        radius = r * ((i * 2.0 + 1.0) * 6.0) / 768.0;
        out.push(radius);
        radius = r * ((i * 2.0 + 2.0) * 6.0) / 768.0;
    }
    out
}

/// What a pixel gives the rays (Hidden/SunShaftsComposite, pass 2,
/// program 6): how far its colour is over the threshold, by how near the
/// caster it is, and only where the depth is the sky's. `sun` and `uv`
/// are viewport points, `depth01` the depth over the far plane.
pub fn shaft_mask(shafts: &SunShafts, sun: [f32; 3], uv: [f32; 2], rgb: [f32; 3], depth01: f32) -> f32 {
    if depth01 <= 0.99 {
        return 0.0;
    }
    let distance = ((sun[0] - uv[0]).powi(2) + (sun[1] - uv[1]).powi(2)).sqrt();
    let near = (shafts.max_radius - distance).clamp(0.0, 1.0);
    let over: f32 = (0..3).map(|i| (rgb[i] - shafts.threshold[i]).max(0.0)).sum();
    over * near
}

/// The rays over the picture (Hidden/SunShaftsComposite, pass 4 for Add
/// and pass 0 for Screen; programs 4 and 5). `rays` is the blurred mask.
pub fn shaft_blend(shafts: &SunShafts, sun_depth: f32, picture: [f32; 3], rays: f32) -> [f32; 3] {
    let color = shaft_color(shafts, sun_depth);
    [0, 1, 2].map(|i| {
        let ray = (rays * color[i]).clamp(0.0, 1.0);
        match shafts.blend {
            ShaftBlend::Add => picture[i] + ray,
            ShaftBlend::Screen => 1.0 - (1.0 - picture[i]) * (1.0 - ray),
        }
    })
}

/// `_CurveParams` as `DepthOfField.OnRenderImage` sets it: 1, the focal
/// size, the aperture term and the focus distance over the far plane.
/// `focus` is the eye depth of the point in focus.
pub fn dof_curve_params(dof: &DepthOfField, focus: f32) -> [f32; 4] {
    // An aperture of 1 divides by zero in the component. The largest
    // float gives the same picture: everything off the focus at full blur.
    let aperture = (1.0 / (1.0 - dof.aperture.max(0.0)) - 1.0).min(f32::MAX);
    [1.0, dof.focal_size.clamp(0.0, 2.0), aperture, focus / dof.camera_far]
}

/// The circle of confusion of a depth, 0 sharp to 1 (Hidden/Dof/
/// DepthOfFieldHdr, pass 0, program 16). Depths are over the far plane.
pub fn dof_coc(curve: [f32; 4], depth01: f32) -> f32 {
    ((depth01 - curve[3]).abs() * curve[2] / (depth01 + 1.0e-5) - curve[1])
        .max(0.0)
        .min(curve[0])
}

/// The circle of confusion in front of the focus alone (pass 4, program
/// 20), which near blur spreads over what is behind.
pub fn dof_coc_front(curve: [f32; 4], depth01: f32) -> f32 {
    ((curve[3] - depth01) * curve[2] / (depth01 + 1.0e-5) - curve[1])
        .max(0.0)
        .min(curve[0])
}

fn smooth01(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

/// The weight of a tap of the foreground's blur before its place in the
/// kernel (pass 2, program 15): taps with a larger circle than the
/// centre's count in full, smaller ones fade out.
pub fn dof_spread_weight(tap: f32, centre: f32) -> f32 {
    smooth01(((tap - centre + 0.5) * 2.0).clamp(0.0, 1.0))
}

/// The weight of a tap of the disc blur (programs 18 and 4): full when
/// the tap's circle is at least `reach`, none 0.265 under it.
pub fn dof_disc_weight(tap: f32, reach: f32) -> f32 {
    smooth01(((tap - reach + 0.265) * 3.773_585).clamp(0.0, 1.0))
}

/// How much of the half size blur the last pass takes for a circle of
/// confusion (pass 18, program 4): none to 0.65, all from 0.85.
pub fn dof_low_blend(coc: f32) -> f32 {
    smooth01(((coc - 0.65) * 4.999_999).clamp(0.0, 1.0))
}

/// The blur widths the component hands its passes for a frame `width`
/// pixels wide: `_Offsets` of the foreground's blur, of the disc blur
/// (`.zw`) and of the last pass.
pub fn dof_widths(dof: &DepthOfField, width: u32) -> (f32, [f32; 2], f32) {
    let blur = dof.max_blur_size.max(0.1);
    let half = (width >> 1).max(1);
    (
        blur * dof.foreground_overlap,
        [0.1, blur],
        width as f32 / half as f32 * blur,
    )
}

/// The disc of Hidden/Dof/DepthOfFieldHdr's blurs: 28 offsets and their
/// lengths, the immediate constants of programs 18 and 4.
pub const DOF_DISC: [[f32; 3]; 28] = [
    [0.62463, 0.54337, 0.8279],
    [-0.13414, -0.94488, 0.95435],
    [0.38772, -0.43475, 0.58253],
    [0.12126, -0.19282, 0.22778],
    [-0.20388, 0.11133, 0.2323],
    [0.83114, -0.29218, 0.881],
    [0.10759, -0.57839, 0.58831],
    [0.28285, 0.79036, 0.83945],
    [-0.36622, 0.39516, 0.53876],
    [0.75591, 0.21916, 0.78704],
    [-0.5261, 0.02386, 0.52664],
    [-0.88216, -0.24471, 0.91547],
    [-0.48888, -0.2933, 0.57011],
    [0.44014, -0.08558, 0.44838],
    [0.21179, 0.51373, 0.55567],
    [0.05483, 0.95701, 0.95858],
    [-0.59001, -0.70509, 0.91938],
    [-0.80065, 0.24631, 0.83768],
    [-0.19424, -0.18402, 0.26757],
    [-0.43667, 0.76751, 0.88304],
    [0.21666, 0.11602, 0.24577],
    [0.15696, -0.856, 0.87027],
    [-0.75821, 0.58363, 0.95682],
    [0.99284, -0.02904, 0.99327],
    [-0.22234, -0.57907, 0.62029],
    [0.55052, -0.66984, 0.86704],
    [0.46431, 0.28115, 0.5428],
    [-0.07214, 0.60554, 0.60982],
];

/// `Fx` in `effects.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FxUniform {
    size: [f32; 4],
    src: [f32; 4],
    ao: [f32; 4],
    ao2: [f32; 4],
    levels: [f32; 4],
    uv_to_view: [f32; 4],
    bloom: [f32; 4],
    vignette: [f32; 4],
    fog_color: [f32; 4],
    fog_height: [f32; 4],
    fog_distance: [f32; 4],
    fog_scene: [f32; 4],
    fog_ray_x: [f32; 4],
    fog_ray_y: [f32; 4],
    fog_ray_z: [f32; 4],
    dof: [f32; 4],
    shaft_sun: [f32; 4],
    shaft_color: [f32; 4],
    shaft_threshold: [f32; 4],
    rand: [[f32; 4]; 16],
}

/// What the effects need to know about a frame.
pub(crate) struct Frame<'a> {
    pub fx: &'a StudioEffects,
    pub width: u32,
    pub height: u32,
    pub fov_y_deg: f32,
    pub near: f32,
    pub far: f32,
    /// Where the camera is and where it looks, in the world.
    pub eye: Vec3,
    pub forward: Vec3,
    /// The eye depth of the point the camera looks at.
    pub focus: f32,
    /// The camera's depth texture.
    pub depth: &'a wgpu::TextureView,
    /// The depth aspect of the main pass's multisampled depth, which tells
    /// the backdrop where something was drawn.
    pub scene_depth: &'a wgpu::TextureView,
    pub white: &'a wgpu::TextureView,
    /// The colour table, uploaded.
    pub lut: Option<&'a wgpu::TextureView>,
}

impl Frame<'_> {
    fn uniform(&self) -> FxUniform {
        let (w, h) = (self.width as f32, self.height as f32);
        let (uv_to_view, half_proj_scale) = ao_projection(self.fov_y_deg, self.width, self.height);
        let ao = self.fx.ao.clone().unwrap_or_default();
        let bias = ao.bias.clamp(0.0, 1.0);
        let bloom = self.fx.bloom.as_ref();
        let fog = self.fx.fog.clone().unwrap_or_default();
        let [ray_x, ray_y, ray_z] = fog_rays(self.forward, self.fov_y_deg, w / h);
        let (fog_mode, fog_scene) = fog_scene_params(fog.mode);
        let fog_flags = u8::from(fog.distance_fog) | u8::from(fog.radial) << 1 | u8::from(fog.height_fog) << 2;
        // A colour, which Unity hands to shaders linearised.
        let fog_color = crate::material::linear([fog.color[0], fog.color[1], fog.color[2], 1.0]);
        let dof = self.fx.dof.clone().unwrap_or_default();
        let [_, focal_size, aperture, focus] = dof_curve_params(&dof, dof.focus.unwrap_or(self.focus));
        let shafts = self.fx.shafts.clone().unwrap_or_default();
        let sun = shaft_sun(&shafts, self.eye, self.forward, self.fov_y_deg, w / h);
        let [sr, sg, sb] = shaft_color(&shafts, sun[2]);
        let screen = if shafts.blend == ShaftBlend::Screen { 1.0 } else { 0.0 };
        FxUniform {
            size: [w, h, 1.0 / w, 1.0 / h],
            src: [0.0; 4],
            ao: [ao.radius, half_proj_scale, ao.power_exponent, bias],
            ao2: [1.0 / (1.0 - bias), ao.blur_sharpness, self.near, self.far],
            levels: [ao.tint[0], ao.tint[1], ao.tint[2], ao.intensity],
            uv_to_view,
            bloom: [
                bloom.map_or(0.0, |b| b.threshold),
                bloom.map_or(0.0, |b| b.intensity),
                self.fx.color.as_ref().map_or(1.0, |c| c.exposure),
                if self.lut.is_some() && self.fx.color.is_some() {
                    1.0
                } else {
                    0.0
                },
            ],
            vignette: {
                let [i, b] = self.fx.vignette.as_ref().map_or([0.0; 2], vignette_params);
                let blend = self.fx.color.as_ref().map_or(0.0, |c| c.blend.clamp(0.0, 1.0));
                [i, b, blend, 0.0]
            },
            fog_color,
            fog_height: fog_height_params(&fog, self.eye.y),
            fog_distance: [
                -fog.start_distance.max(0.0),
                fog.camera_far,
                fog_mode,
                f32::from(fog_flags),
            ],
            fog_scene,
            fog_ray_x: [ray_x.x, ray_x.y, ray_x.z, self.eye.y],
            fog_ray_y: [ray_y.x, ray_y.y, ray_y.z, 0.0],
            fog_ray_z: [ray_z.x, ray_z.y, ray_z.z, fog.camera_near],
            dof: [focal_size, aperture, focus, dof.camera_far],
            // The viewport counts rows from the bottom, the passes' UV
            // from the top.
            shaft_sun: [sun[0], 1.0 - sun[1], shafts.max_radius, shafts.camera_far],
            shaft_color: [sr, sg, sb, screen],
            shaft_threshold: [shafts.threshold[0], shafts.threshold[1], shafts.threshold[2], 0.0],
            rand: random_data(),
        }
    }
}

/// The camera renders in half floats.
pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Occlusion and eye depth (`RenderTextureFormat.RGHalf`).
const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

struct Targets {
    size: [u32; 2],
    shaft_divisor: u32,
    hdr_msaa: wgpu::TextureView,
    hdr: wgpu::TextureView,
    ao: [wgpu::TextureView; 2],
    half: wgpu::TextureView,
    quarter: [wgpu::TextureView; 3],
    /// The picture after Amplify Color, which the later effects hand to
    /// each other.
    ldr: [wgpu::TextureView; 2],
    ldr_half: [wgpu::TextureView; 2],
    /// The sun shafts' buffers, the frame over the resolution's divisor.
    shafts: [wgpu::TextureView; 2],
    /// The finished effects, before the backdrop goes under them.
    done: wgpu::TextureView,
}

/// The effects' GPU side, made the first time a frame asks for effects.
pub(crate) struct Gpu {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipelines: HashMap<(&'static str, wgpu::TextureFormat), wgpu::RenderPipeline>,
    ao_apply: wgpu::RenderPipeline,
    fog_apply: wgpu::RenderPipeline,
    backdrop_layout: wgpu::BindGroupLayout,
    backdrop: wgpu::RenderPipeline,
    targets: Option<Targets>,
}

impl Gpu {
    pub fn new(
        device: &wgpu::Device,
        ldr_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects"),
            source: wgpu::ShaderSource::Wgsl(include_str!("effects.wgsl").into()),
        });
        let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let float = wgpu::TextureSampleType::Float { filterable: true };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(1, float),
                texture(2, float),
                texture(3, wgpu::TextureSampleType::Depth),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture(5, float),
                texture(6, float),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let make = |entry: &'static str,
                    target: wgpu::ColorTargetState,
                    depth_stencil: Option<wgpu::DepthStencilState>,
                    count: u32| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_fx"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(target)],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil,
                multisample: wgpu::MultisampleState {
                    count,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        let mut pipelines = HashMap::new();
        for (entry, format) in [
            ("fs_ao", AO_FORMAT),
            ("fs_ao_blur", AO_FORMAT),
            ("fs_down4", HDR_FORMAT),
            ("fs_bright", HDR_FORMAT),
            ("fs_blur_plus", HDR_FORMAT),
            ("fs_color", ldr_format),
            ("fs_down4", ldr_format),
            ("fs_blur", ldr_format),
            ("fs_vignette", ldr_format),
            ("fs_shaft_mask", ldr_format),
            ("fs_shaft_blur", ldr_format),
            ("fs_shaft_blend", ldr_format),
            ("fs_dof_front", ldr_format),
            ("fs_dof_spread", ldr_format),
            ("fs_dof_coc", ldr_format),
            ("fs_dof_half", ldr_format),
            ("fs_dof_disc", ldr_format),
            ("fs_dof_apply", ldr_format),
        ] {
            pipelines.insert((entry, format), make(entry, format.into(), None, 1));
        }
        // Blend DstColor Zero, drawn into the camera's multisampled target
        // between the solid and the later queues.
        let multiply = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Dst,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        };
        let multiply_alpha = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::DstAlpha,
            ..multiply
        };
        let ao_apply = make(
            "fs_ao_apply",
            wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: Some(wgpu::BlendState {
                    color: multiply,
                    alpha: multiply_alpha,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            },
            Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            samples,
        );
        // GlobalFog writes lerp(fog colour, picture, factor): the fog colour
        // over the picture by what the factor leaves out. The fog colour's
        // alpha is 1.
        let fog_apply = make(
            "fs_fog",
            wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            },
            Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            samples,
        );
        let backdrop_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("backdrop"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(1, float),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: true,
                    },
                    count: None,
                },
            ],
        });
        let backdrop_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("backdrop"),
            source: wgpu::ShaderSource::Wgsl(include_str!("backdrop.wgsl").into()),
        });
        let backdrop = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("backdrop"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("backdrop"),
                bind_group_layouts: &[Some(&backdrop_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &backdrop_shader,
                entry_point: Some("vs_backdrop"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &backdrop_shader,
                entry_point: Some("fs_backdrop"),
                targets: &[Some(ldr_format.into())],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effects"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            layout,
            sampler,
            pipelines,
            ao_apply,
            fog_apply,
            backdrop_layout,
            backdrop,
            targets: None,
        }
    }

    /// Makes the frame's targets when the size changed, and returns the
    /// multisampled HDR target with the texture it resolves to.
    /// `shaft_divisor` is `SunShafts::divisor`.
    pub fn hdr_targets(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        samples: u32,
        ldr_format: wgpu::TextureFormat,
        shaft_divisor: u32,
    ) -> (wgpu::TextureView, wgpu::TextureView) {
        let shaft_divisor = shaft_divisor.max(1);
        if self
            .targets
            .as_ref()
            .is_none_or(|t| t.size != [width, height] || t.shaft_divisor != shaft_divisor)
        {
            let make = |w: u32, h: u32, format, count| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("effects"),
                        size: wgpu::Extent3d {
                            width: w.max(1),
                            height: h.max(1),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: count,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: if count > 1 {
                            wgpu::TextureUsages::RENDER_ATTACHMENT
                        } else {
                            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING
                        },
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            // BloomAndFlares and the vignette size their buffers by integer
            // division of the frame's.
            let (hw, hh, qw, qh) = (width / 2, height / 2, width / 4, height / 4);
            self.targets = Some(Targets {
                size: [width, height],
                shaft_divisor,
                hdr_msaa: make(width, height, HDR_FORMAT, samples),
                hdr: make(width, height, HDR_FORMAT, 1),
                ao: std::array::from_fn(|_| make(width, height, AO_FORMAT, 1)),
                half: make(hw, hh, HDR_FORMAT, 1),
                quarter: std::array::from_fn(|_| make(qw, qh, HDR_FORMAT, 1)),
                ldr: std::array::from_fn(|_| make(width, height, ldr_format, 1)),
                ldr_half: std::array::from_fn(|_| make(hw, hh, ldr_format, 1)),
                shafts: std::array::from_fn(|_| make(width / shaft_divisor, height / shaft_divisor, ldr_format, 1)),
                done: make(width, height, ldr_format, 1),
            });
        }
        let t = self.targets.as_ref().expect("made above");
        (t.hdr_msaa.clone(), t.hdr.clone())
    }

    fn group(
        &self,
        device: &wgpu::Device,
        f: &Frame,
        uniform: FxUniform,
        a: &wgpu::TextureView,
        b: &wgpu::TextureView,
        c: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("effects"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let view = wgpu::BindingResource::TextureView;
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: view(a),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: view(b),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: view(f.depth),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: view(f.lut.unwrap_or(f.white)),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: view(c),
                },
            ],
        })
    }

    /// One full-screen pass of `entry` into `target`.
    #[allow(clippy::too_many_arguments)]
    fn pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        f: &Frame,
        entry: &'static str,
        format: wgpu::TextureFormat,
        target: &wgpu::TextureView,
        uniform: FxUniform,
        a: &wgpu::TextureView,
        b: &wgpu::TextureView,
    ) {
        self.pass3(device, encoder, f, entry, format, target, uniform, [a, b, f.white]);
    }

    /// `pass` with a third texture.
    #[allow(clippy::too_many_arguments)]
    fn pass3(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        f: &Frame,
        entry: &'static str,
        format: wgpu::TextureFormat,
        target: &wgpu::TextureView,
        uniform: FxUniform,
        [a, b, c]: [&wgpu::TextureView; 3],
    ) {
        let group = self.group(device, f, uniform, a, b, c);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(entry),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipelines[&(entry, format)]);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Computes and blurs the occlusion from the depth texture. Returns
    /// the pipeline and bind group that multiply it into the camera's
    /// target; `None` when occlusion is off.
    pub fn occlusion(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        f: &Frame,
    ) -> Option<(&wgpu::RenderPipeline, wgpu::BindGroup)> {
        f.fx.ao.as_ref()?;
        let t = self.targets.as_ref()?;
        let base = f.uniform();
        self.pass(device, encoder, f, "fs_ao", AO_FORMAT, &t.ao[0], base, f.white, f.white);
        // BlurPasses is 1: once across, once down.
        let across = FxUniform {
            src: [0.0, 0.0, 1.0, 0.0],
            ..base
        };
        let down = FxUniform {
            src: [0.0, 0.0, 0.0, 1.0],
            ..base
        };
        self.pass(
            device,
            encoder,
            f,
            "fs_ao_blur",
            AO_FORMAT,
            &t.ao[1],
            across,
            &t.ao[0],
            f.white,
        );
        self.pass(
            device,
            encoder,
            f,
            "fs_ao_blur",
            AO_FORMAT,
            &t.ao[0],
            down,
            &t.ao[1],
            f.white,
        );
        Some((&self.ao_apply, self.group(device, f, base, &t.ao[0], f.white, f.white)))
    }

    /// The pipeline and bind group that lay GlobalFog over the camera's
    /// target from the depth texture, after the solid queues and the sky
    /// (`[ImageEffectOpaque]`); `None` when fog is off.
    pub fn fog(&self, device: &wgpu::Device, f: &Frame) -> Option<(&wgpu::RenderPipeline, wgpu::BindGroup)> {
        f.fx.fog.as_ref()?;
        Some((
            &self.fog_apply,
            self.group(device, f, f.uniform(), f.white, f.white, f.white),
        ))
    }

    /// Runs bloom, the colour table and the vignette on the resolved HDR
    /// picture and writes the result to `target`.
    pub fn finish(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        f: &Frame,
        ldr_format: wgpu::TextureFormat,
        target: &wgpu::TextureView,
    ) {
        let Some(t) = self.targets.as_ref() else { return };
        // With a backdrop the effects finish in `done`, and the backdrop
        // pass then writes the target.
        let out = if f.fx.backdrop.is_some() { &t.done } else { target };
        let base = f.uniform();
        let (w, h) = (f.width as f32, f.height as f32);
        let aspect = w / h;
        let texel = |w: u32, h: u32| [1.0 / w.max(1) as f32, 1.0 / h.max(1) as f32];
        let with = |texel: [f32; 2], step: [f32; 2]| FxUniform {
            src: [texel[0], texel[1], step[0], step[1]],
            ..base
        };
        let full = texel(f.width, f.height);
        let half = texel(f.width / 2, f.height / 2);

        let bloom = if let Some(bloom) = &f.fx.bloom {
            let q = &t.quarter;
            self.pass(
                device,
                encoder,
                f,
                "fs_down4",
                HDR_FORMAT,
                &t.half,
                with(full, [0.0; 2]),
                &t.hdr,
                f.white,
            );
            self.pass(
                device,
                encoder,
                f,
                "fs_down4",
                HDR_FORMAT,
                &q[0],
                with(half, [0.0; 2]),
                &t.half,
                f.white,
            );
            self.pass(device, encoder, f, "fs_bright", HDR_FORMAT, &q[1], base, &q[0], f.white);
            let mut source = 1;
            for i in 0..bloom.iterations.max(1) {
                let [down, across] = bloom_steps(bloom, i, aspect);
                self.pass(
                    device,
                    encoder,
                    f,
                    "fs_blur_plus",
                    HDR_FORMAT,
                    &q[2],
                    with(half, down),
                    &q[source],
                    f.white,
                );
                self.pass(
                    device,
                    encoder,
                    f,
                    "fs_blur_plus",
                    HDR_FORMAT,
                    &q[0],
                    with(half, across),
                    &q[2],
                    f.white,
                );
                source = 0;
            }
            &q[0]
        } else {
            // No bloom: the uniform's intensity is zero.
            f.white
        };

        // The effects after Amplify Color, in the camera's order. Each
        // reads the picture the one before left and the last writes `out`.
        let mut later =
            usize::from(f.fx.shafts.is_some()) + usize::from(f.fx.vignette.is_some()) + usize::from(f.fx.dof.is_some());
        let mut current = 0;
        let first = if later == 0 { out } else { &t.ldr[0] };
        self.pass(device, encoder, f, "fs_color", ldr_format, first, base, &t.hdr, bloom);

        if let Some(shafts) = &f.fx.shafts {
            later -= 1;
            let to = if later == 0 { out } else { &t.ldr[1 - current] };
            let (sw, sh) = (f.width / t.shaft_divisor, f.height / t.shaft_divisor);
            let [a, b] = &t.shafts;
            let picture = &t.ldr[current];
            self.pass(
                device,
                encoder,
                f,
                "fs_shaft_mask",
                ldr_format,
                a,
                with(texel(sw, sh), [0.0; 2]),
                picture,
                f.white,
            );
            let (mut from, mut into) = (a, b);
            for radius in shaft_radii(shafts) {
                self.pass(
                    device,
                    encoder,
                    f,
                    "fs_shaft_blur",
                    ldr_format,
                    into,
                    with(texel(sw, sh), [radius; 2]),
                    from,
                    f.white,
                );
                std::mem::swap(&mut from, &mut into);
            }
            self.pass(
                device,
                encoder,
                f,
                "fs_shaft_blend",
                ldr_format,
                to,
                base,
                picture,
                from,
            );
            current = 1 - current;
        }

        if let Some(vignette) = &f.fx.vignette {
            later -= 1;
            let to = if later == 0 { out } else { &t.ldr[1 - current] };
            let picture = &t.ldr[current];
            let [a, b] = &t.ldr_half;
            self.pass(
                device,
                encoder,
                f,
                "fs_down4",
                ldr_format,
                a,
                with(full, [0.0; 2]),
                picture,
                f.white,
            );
            let [down, across] = vignette_steps(vignette, aspect);
            for _ in 0..2 {
                self.pass(
                    device,
                    encoder,
                    f,
                    "fs_blur",
                    ldr_format,
                    b,
                    with(half, down),
                    a,
                    f.white,
                );
                self.pass(
                    device,
                    encoder,
                    f,
                    "fs_blur",
                    ldr_format,
                    a,
                    with(half, across),
                    b,
                    f.white,
                );
            }
            self.pass(device, encoder, f, "fs_vignette", ldr_format, to, base, picture, a);
            current = 1 - current;
        }

        if let Some(dof) = &f.fx.dof {
            let picture = &t.ldr[current];
            let circles = &t.ldr[1 - current];
            let [a, b] = &t.ldr_half;
            let (overlap, disc, last) = dof_widths(dof, f.width);
            // WriteCoc: the circles of what is in front, spread down and
            // across, then every pixel's circle into the picture's alpha.
            if dof.near_blur {
                self.pass(
                    device,
                    encoder,
                    f,
                    "fs_dof_front",
                    ldr_format,
                    a,
                    base,
                    f.white,
                    f.white,
                );
                let down = with(half, [0.0, overlap * half[1]]);
                let across = with(half, [overlap * half[0], 0.0]);
                self.pass(device, encoder, f, "fs_dof_spread", ldr_format, b, down, a, f.white);
                self.pass(device, encoder, f, "fs_dof_spread", ldr_format, a, across, b, f.white);
            }
            let near = if dof.near_blur { 1.0 } else { 0.0 };
            self.pass(
                device,
                encoder,
                f,
                "fs_dof_coc",
                ldr_format,
                circles,
                with(full, [near, 0.0]),
                picture,
                a,
            );
            // The picture at half size, its disc blur, and the two put
            // together.
            self.pass(
                device,
                encoder,
                f,
                "fs_dof_half",
                ldr_format,
                a,
                with(full, [0.0; 2]),
                circles,
                f.white,
            );
            self.pass(
                device,
                encoder,
                f,
                "fs_dof_disc",
                ldr_format,
                b,
                with(half, disc),
                a,
                f.white,
            );
            self.pass3(
                device,
                encoder,
                f,
                "fs_dof_apply",
                ldr_format,
                out,
                with(full, [0.0, last]),
                [circles, b, picture],
            );
        }
        self.under(device, encoder, f, target);
    }

    /// Puts the backdrop under the finished effects, when the frame has one.
    fn under(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, f: &Frame, target: &wgpu::TextureView) {
        let (Some(colour), Some(t)) = (f.fx.backdrop, self.targets.as_ref()) else {
            return;
        };
        let linear = backdrop_linear(colour);
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("backdrop"),
            contents: bytemuck::bytes_of(&linear),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("backdrop"),
            layout: &self.backdrop_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&t.done),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(f.scene_depth),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("backdrop"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.backdrop);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// A backdrop colour as the shader takes it: linear, since the target
/// stores sRGB.
fn backdrop_linear(c: [f64; 4]) -> [f32; 4] {
    let lin = |x: f64| {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    [lin(c[0]) as f32, lin(c[1]) as f32, lin(c[2]) as f32, c[3] as f32]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backdrop_shader_is_valid_wgsl_and_its_colour_is_linear() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("backdrop.wgsl")).expect("parses");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("validates");
        let grey = backdrop_linear([0.22, 0.22, 0.23, 1.0]);
        assert!(
            (grey[0] - 0.0395).abs() < 0.001 && (grey[3] - 1.0).abs() < 1e-6,
            "{grey:?}"
        );
        assert_eq!(backdrop_linear([0.0, 1.0, 2.0, 1.0])[..3], [0.0, 1.0, 1.0]);
    }

    #[test]
    fn effects_shader_is_valid_wgsl() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("effects.wgsl")).expect("parses");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validates");
    }

    #[test]
    fn uniform_matches_the_shader() {
        // Eight vectors, the fog's seven, one for the depth of field, the
        // sun shafts' three and the sixteen random pixels.
        assert_eq!(std::mem::size_of::<FxUniform>(), (8 + 7 + 1 + 3 + 16) * 16);
    }

    #[test]
    fn studio_defaults_are_the_scene_values() {
        let fx = StudioEffects::studio(None);
        let ao = fx.ao.expect("on");
        assert!((ao.tint[0] - 0.705_882_4).abs() < 1e-6);
        assert_eq!((ao.radius, ao.power_exponent, ao.bias), (0.1, 1.0, 0.25));
        let bloom = fx.bloom.expect("on");
        assert_eq!(
            (bloom.intensity, bloom.threshold, bloom.blur_spread, bloom.iterations),
            (0.4, 0.6, 0.8, 2)
        );
        assert!(fx.color.is_none());
        assert_eq!(
            fx.vignette.expect("on"),
            Vignette {
                intensity: 0.1,
                blur: 0.2,
                blur_spread: 0.75
            }
        );
    }

    #[test]
    fn fog_is_the_studio_cameras_component() {
        // CharaStudio_Data/level1, GlobalFog 8974 and the RenderSettings.
        let fog = Fog::default();
        assert!(fog.height_fog && !fog.distance_fog && fog.radial);
        assert_eq!((fog.height, fog.height_density, fog.start_distance), (1.0, 0.001, 0.0));
        assert_eq!(fog.mode, FogMode::ExponentialSquared { density: 0.01 });
        assert_eq!((fog.camera_near, fog.camera_far), (0.06, 1000.0));
        assert!(StudioEffects::studio(None).fog.is_none());
    }

    #[test]
    fn fog_parameters_follow_the_component() {
        let fog = Fog {
            height: 50.0,
            ..Fog::default()
        };
        // Inside the fog the third number is 1.
        assert_eq!(fog_height_params(&fog, 1.0), [50.0, -49.0, 1.0, 0.0005]);
        assert_eq!(fog_height_params(&fog, 60.0), [50.0, 10.0, 0.0, 0.0005]);
        let (mode, p) = fog_scene_params(FogMode::Linear {
            start: 10.0,
            end: 110.0,
        });
        assert_eq!(mode, 1.0);
        assert!((p[2] + 0.01).abs() < 1e-7 && (p[3] - 1.1).abs() < 1e-6, "{p:?}");
        // A range of nothing gives no slope, as the component guards it.
        assert_eq!(fog_scene_params(FogMode::Linear { start: 5.0, end: 5.0 }).1, [0.0; 4]);
        let (mode, p) = fog_scene_params(FogMode::ExponentialSquared { density: 0.01 });
        assert_eq!(mode, 3.0);
        assert!((p[0] - 0.012_011_224).abs() < 1e-8 && (p[1] - 0.014_426_95).abs() < 1e-8);
        assert_eq!(fog_scene_params(FogMode::Exponential { density: 0.01 }).0, 2.0);
        assert_eq!("1.442695".parse::<f32>(), Ok(std::f32::consts::LOG2_E));
    }

    #[test]
    fn height_fog_thickens_with_the_way_through_it() {
        // The camera inside the fog, 49 under its top, and a point 10 away
        // at the same height: g = 0.0005 * 10 * (49 + 49) = 0.49.
        let inside = Fog {
            height: 50.0,
            mode: FogMode::Linear { start: 0.0, end: 300.0 },
            ..Fog::default()
        };
        let f = fog_factor(&inside, 1.0, [0.0, 0.0, 10.0], 10.0);
        assert!((f - (300.0 - 0.49) / 300.0).abs() < 1e-6, "{f}");
        // The start distance is taken off before the fog's curve.
        let pushed = Fog {
            start_distance: 0.2,
            ..inside.clone()
        };
        let f = fog_factor(&pushed, 1.0, [0.0, 0.0, 10.0], 10.0);
        assert!((f - (300.0 - 0.29) / 300.0).abs() < 1e-6, "{f}");
        // Exponential squared, the Studio's own mode: exp(-(0.01 g)^2).
        let squared = Fog {
            height: 50.0,
            ..Fog::default()
        };
        let f = fog_factor(&squared, 1.0, [0.0, 0.0, 1000.0], 1000.0);
        assert!((f - (-(0.49f32).powi(2)).exp()).abs() < 1e-5, "{f}");
        let exponential = Fog {
            mode: FogMode::Exponential { density: 0.01 },
            ..squared.clone()
        };
        let f = fog_factor(&exponential, 1.0, [0.0, 0.0, 1000.0], 1000.0);
        assert!((f - (-0.49f32).exp()).abs() < 1e-5, "{f}");
        // The camera over the fog: a point over it too is clear, one under
        // it has g = |aV| * depth under the top squared / |V.y|.
        let over = Fog {
            mode: FogMode::Linear { start: 0.0, end: 1.0 },
            ..Fog::default()
        };
        assert_eq!(fog_factor(&over, 2.0, [3.0, -0.5, 0.0], 3.0), 1.0);
        let f = fog_factor(&over, 2.0, [3.0, -2.0, 0.0], 3.0);
        let g = 0.0005 * 13f32.sqrt() * 1.0 / (2.0 - 1.0e-5);
        assert!((f - (1.0 - g)).abs() < 1e-6, "{f}");
    }

    #[test]
    fn fog_rays_reach_the_point_a_pixel_shows() {
        let (eye, target) = (Vec3::new(1.0, 1.5, -3.0), Vec3::new(0.2, 0.9, 0.0));
        let forward = (target - eye).normalize();
        let (fov, aspect) = (23.0f32, 16.0 / 9.0);
        let view = glam::Mat4::look_at_lh(eye, target, Vec3::Y);
        let proj = glam::Mat4::perspective_lh(fov.to_radians(), aspect, 0.06, 1000.0);
        let [x, y, z] = fog_rays(forward, fov, aspect);
        for point in [Vec3::new(0.5, 0.0, 0.3), Vec3::new(-0.4, 1.7, 1.0)] {
            let ndc = (proj * view).project_point3(point);
            let depth = view.transform_point3(point).z;
            let reached = eye + (z + x * ndc.x + y * ndc.y) * depth;
            assert!((reached - point).length() < 1e-4, "{reached} {point}");
        }
    }

    #[test]
    fn distance_fog_counts_from_the_near_plane() {
        // Hidden/GlobalFog programs 0 and 2, which the Studio leaves off.
        let fog = Fog {
            height_fog: false,
            distance_fog: true,
            mode: FogMode::Linear { start: 0.0, end: 100.0 },
            ..Fog::default()
        };
        let f = fog_factor(&fog, 1.0, [3.0, 0.0, 4.0], 4.0);
        assert!((f - (100.0 - (5.0 - 0.06)) / 100.0).abs() < 1e-6, "{f}");
        let by_depth = Fog {
            radial: false,
            ..fog.clone()
        };
        let f = fog_factor(&by_depth, 1.0, [3.0, 0.0, 4.0], 4.0);
        assert!((f - (100.0 - (4.0 - 0.06)) / 100.0).abs() < 1e-6, "{f}");
        // Past the end everything is fog.
        assert_eq!(fog_factor(&fog, 1.0, [0.0, 0.0, 500.0], 500.0), 0.0);
    }

    #[test]
    fn random_pixels_are_unit_directions_in_eight_bits() {
        let data = random_data();
        // The first: angle 2 pi * 0.4639 / 8, offset 0.34.
        let angle = std::f32::consts::TAU * 0.4639 / 8.0;
        assert!((data[0][0] - angle.cos()).abs() < 0.5 / 255.0 + 1e-6);
        assert!((data[0][1] - angle.sin()).abs() < 0.5 / 255.0 + 1e-6);
        assert!((data[0][2] - 87.0 / 255.0).abs() < 1e-6);
        for p in data {
            assert!((p[0] * 255.0).fract().min(1.0 - (p[0] * 255.0).fract()) < 1e-3);
            assert!(p[0] > 0.7 && p[1] >= 0.0 && p[1] < 0.71);
        }
    }

    #[test]
    fn occlusion_projection_follows_the_camera() {
        let (uv_to_view, half) = ao_projection(23.0, 900, 1400);
        let tan = (11.5f32).to_radians().tan();
        assert!((uv_to_view[1] + 2.0 * tan).abs() < 1e-6);
        assert!((uv_to_view[3] - tan).abs() < 1e-6);
        assert!((uv_to_view[0] - 2.0 * tan * 900.0 / 1400.0).abs() < 1e-6);
        assert!((half - 1400.0 / (4.0 * tan)).abs() < 1e-2);
    }

    #[test]
    fn occlusion_multiplies_by_the_tint() {
        let ao = AmbientOcclusion::default();
        assert_eq!(ao_factor(&ao, 1.0), [1.0; 3]);
        assert!((ao_factor(&ao, 0.0)[0] - 180.0 / 255.0).abs() < 1e-6);
        let half = ao_factor(&ao, 0.5)[1];
        assert!((half - (1.0 + 180.0 / 255.0) / 2.0).abs() < 1e-6);
    }

    #[test]
    fn occlusion_blur_stops_at_depth_edges() {
        // Same depth: the Gaussian alone, sigma 2.
        assert!((ao_blur_weight(2, 3.0, 3.0, 10.0) - (-0.5f32).exp()).abs() < 1e-6);
        assert!((ao_blur_weight(4, 3.0, 3.0, 10.0) - (-2.0f32).exp()).abs() < 1e-6);
        // Half a unit behind: nothing.
        assert!(ao_blur_weight(1, 3.0, 3.5, 10.0) < 1e-9);
        // Closer than one unit the edge gets sharper.
        assert!(ao_blur_weight(1, 0.5, 0.51, 10.0) < ao_blur_weight(1, 3.0, 3.01, 10.0));
    }

    #[test]
    fn blur_weights_add_up_to_one() {
        for taps in [&BLOOM_BLUR[..], &VIGNETTE_BLUR[..]] {
            let sum: f32 = taps.iter().map(|(o, w)| if *o == 0.0 { *w } else { 2.0 * w }).sum();
            assert!((sum - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn bloom_blur_widens_with_each_iteration() {
        let bloom = Bloom::default();
        let aspect = 900.0 / 1400.0;
        let [down, across] = bloom_steps(&bloom, 0, aspect);
        assert_eq!(down, [0.0, 0.8 * 2.0 / 1024.0]);
        assert!((across[0] - 0.8 / aspect * 2.0 / 1024.0).abs() < 1e-9);
        let [down, _] = bloom_steps(&bloom, 1, aspect);
        assert!((down[1] - 1.2 * 2.0 / 1024.0).abs() < 1e-9);
    }

    #[test]
    fn vignette_darkens_and_blurs_towards_the_corners() {
        let v = Vignette::default();
        let [intensity, blur] = vignette_params(&v);
        assert!((intensity - 1.0 / 9.0).abs() < 1e-6);
        assert!((blur - 0.25).abs() < 1e-6);
        assert_eq!(vignette_at(&v, [0.5, 0.5]), (0.0, 1.0));
        let (soft, mask) = vignette_at(&v, [0.0, 0.0]);
        assert!((soft - 0.5).abs() < 1e-6);
        assert!((mask - (1.0 - 2.0 / 9.0)).abs() < 1e-6);
        let (soft, mask) = vignette_at(&v, [1.0, 0.5]);
        assert!((soft - 0.25).abs() < 1e-6);
        assert!((mask - (1.0 - 1.0 / 9.0)).abs() < 1e-6);
    }

    /// Amplify Color's neutral table (`CreateDefaultLut`).
    fn neutral_lut() -> Image {
        let mut rgba = vec![0u8; 1024 * 32 * 4];
        for b in 0..32 {
            for g in 0..32 {
                for r in 0..32 {
                    let i = (b * 32 + g * 1024 + r) * 4;
                    let byte = |k: usize| (k as f32 / 31.0 * 255.0) as u8;
                    rgba[i..i + 4].copy_from_slice(&[byte(r), byte(g), byte(b), 255]);
                }
            }
        }
        Image {
            name: "DefaultLut".into(),
            width: 1024,
            height: 32,
            rgba,
            srgb: false,
            repeat: false,
        }
    }

    #[test]
    fn table_positions_split_blue_into_slices() {
        let (a, b, f) = lut_coords([0.0, 0.0, 0.0]);
        assert_eq!((a, f), ([0.5 / 1024.0, 0.5 / 32.0], 0.0));
        assert!((b[0] - a[0] - 1.0 / 32.0).abs() < 1e-9);
        // Full red and green land on the last texel centre of the slice.
        let (a, _, f) = lut_coords([1.0, 1.0, 1.0]);
        assert!((a[0] - (31.0 * 32.0 + 31.5) / 1024.0).abs() < 1e-6);
        assert!((a[1] - 31.5 / 32.0).abs() < 1e-6);
        assert_eq!(f, 0.0);
        // Blue between two slices blends them.
        let (a, _, f) = lut_coords([0.0, 0.0, 10.5 / 31.0]);
        assert!((a[0] - (10.0 * 32.0 + 0.5) / 1024.0).abs() < 1e-6);
        assert!((f - 0.5).abs() < 1e-4);
    }

    #[test]
    fn gamma_steps_undo_each_other() {
        for x in [0.02f32, 0.2, 0.5, 0.85, 1.0] {
            assert!((to_linear(to_gamma(x)) - x).abs() < 4e-3, "{x}");
        }
        assert_eq!(to_gamma(-1.0), 0.0);
        assert_eq!(to_gamma(7.0), 1.0);
    }

    #[test]
    fn neutral_table_keeps_colours() {
        let filter = ColorFilter {
            lut: Arc::new(neutral_lut()),
            exposure: 1.0,
            blend: 0.0,
        };
        for c in [[0.1f32, 0.5, 0.9], [0.8, 0.8, 0.85], [0.3, 0.02, 0.6]] {
            let out = filter_color(&filter, c);
            for i in 0..3 {
                // The table's bytes are cut, not rounded: up to 1/255 low.
                assert!((out[i] - c[i]).abs() < 0.012, "{c:?} gave {out:?}");
            }
        }
        // HDR values clip at white.
        let out = filter_color(&filter, [4.0, 4.0, 4.0]);
        assert!(out.iter().all(|v| (v - 1.0).abs() < 1e-3));
    }

    /// The neutral table with every colour inverted.
    fn inverted_lut() -> Image {
        let mut image = neutral_lut();
        for px in image.rgba.chunks_exact_mut(4) {
            for c in &mut px[..3] {
                *c = 255 - *c;
            }
        }
        image
    }

    #[test]
    fn neutral_texels_are_the_default_table() {
        let image = neutral_lut();
        for (i, px) in image.rgba.chunks_exact(4).enumerate() {
            let (x, y) = (i as u32 % 1024, i as u32 / 1024);
            assert_eq!(neutral_texel(x, y), [px[0], px[1], px[2]], "{x} {y}");
        }
    }

    #[test]
    fn no_blend_reads_the_table_itself() {
        let lut = Arc::new(inverted_lut());
        let filter = ColorFilter {
            lut: lut.clone(),
            exposure: 1.3,
            blend: 0.0,
        };
        // The lookup as it was before the blend existed.
        let plain = |uv: [f32; 2]| {
            let x = uv[0] * 1024.0 - 0.5;
            let y = uv[1] * 32.0 - 0.5;
            let (x0, y0) = (x.floor(), y.floor());
            let (fx, fy) = (x - x0, y - y0);
            let at = |x: i64, y: i64| {
                let i = ((y.clamp(0, 31) * 1024 + x.clamp(0, 1023)) * 4) as usize;
                [0, 1, 2].map(|c| lut.rgba[i + c] as f32 / 255.0)
            };
            let (x0, y0) = (x0 as i64, y0 as i64);
            let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
            [0, 1, 2].map(|i| (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy)
        };
        for c in [[0.1f32, 0.5, 0.9], [0.8, 0.8, 0.85], [0.3, 0.02, 0.6], [2.0, 0.0, 0.4]] {
            let g = c.map(|v| to_gamma(v * 1.3));
            let (a, b, f) = lut_coords(g);
            let (a, b) = (plain(a), plain(b));
            let want = [0, 1, 2].map(|i| to_linear(a[i] + (b[i] - a[i]) * f));
            assert_eq!(filter_color(&filter, c), want, "{c:?}");
        }
    }

    #[test]
    fn blend_fades_the_table_to_the_neutral_one() {
        // BlendCache: table + amount * (neutral - table), in eight bits.
        assert_eq!(
            blend_texel([255, 0, 100], [0, 255, 100], 0.5),
            [128.0 / 255.0, 128.0 / 255.0, 100.0 / 255.0]
        );
        assert_eq!(
            blend_texel([200, 10, 0], [100, 110, 255], 0.25),
            [175.0 / 255.0, 35.0 / 255.0, 64.0 / 255.0]
        );
        assert_eq!(
            blend_texel([200, 10, 0], [100, 110, 255], 1.0),
            [100.0 / 255.0, 110.0 / 255.0, 1.0]
        );

        let lut = Arc::new(inverted_lut());
        let filter = |blend: f32| ColorFilter {
            lut: lut.clone(),
            exposure: 1.0,
            blend,
        };
        // A colour on a texel centre: slice 10, row 20, column 5.
        let c = [5.0f32 / 31.0, 20.0 / 31.0, 10.0 / 31.0].map(to_linear);
        let n = neutral_texel(10 * 32 + 5, 20);
        let want = blend_texel(n.map(|v| 255 - v), n, 0.5).map(to_linear);
        let out = filter_color(&filter(0.5), c);
        for i in 0..3 {
            assert!((out[i] - want[i]).abs() < 2e-3, "{out:?} {want:?}");
        }
        // Half way between a table and its inverse is mid grey.
        assert!(out.iter().all(|v| (v - to_linear(0.5)).abs() < 4e-3), "{out:?}");

        // A full blend is the neutral table, and the amount is clamped.
        let neutral = ColorFilter {
            lut: Arc::new(neutral_lut()),
            exposure: 1.0,
            blend: 0.0,
        };
        for c in [[0.1f32, 0.5, 0.9], [0.3, 0.02, 0.6]] {
            assert_eq!(filter_color(&filter(1.0), c), filter_color(&neutral, c));
            assert_eq!(filter_color(&filter(3.0), c), filter_color(&neutral, c));
            assert_eq!(filter_color(&filter(-1.0), c), filter_color(&filter(0.0), c));
        }
    }
    #[test]
    fn depth_of_field_is_the_studio_cameras_component() {
        let fx = StudioEffects::studio(None);
        assert!(fx.dof.is_none() && fx.shafts.is_none(), "off in a new scene");
        let dof = DepthOfField::default();
        assert_eq!((dof.focal_size, dof.aperture), (0.95, 0.6));
        assert_eq!(
            (dof.max_blur_size, dof.foreground_overlap, dof.near_blur),
            (2.0, 2.0, true)
        );
        // _CurveParams: 1, focalSize, 1 / (1 - aperture) - 1, distance / far.
        let curve = dof_curve_params(&dof, 5.0);
        assert_eq!((curve[0], curve[1]), (1.0, 0.95));
        assert!(
            (curve[2] - 1.5).abs() < 1e-6 && (curve[3] - 0.005).abs() < 1e-9,
            "{curve:?}"
        );
        // The component clamps the focal size and the aperture.
        let wide = DepthOfField {
            focal_size: 3.0,
            aperture: -1.0,
            ..dof.clone()
        };
        assert_eq!(dof_curve_params(&wide, 5.0)[1..3], [2.0, 0.0]);
        // _Offsets: maxBlurSize * foregroundOverlap, (0.1, maxBlurSize), and
        // the frame's width over the half buffer's times maxBlurSize.
        assert_eq!(dof_widths(&dof, 1280), (4.0, [0.1, 2.0], 4.0));
        assert_eq!(dof_widths(&dof, 1281).2, 1281.0 / 640.0 * 2.0);
        let thin = DepthOfField {
            max_blur_size: 0.0,
            ..dof
        };
        assert_eq!(dof_widths(&thin, 1280).1, [0.1, 0.1]);
    }

    #[test]
    fn circle_of_confusion_follows_the_shader() {
        let dof = DepthOfField::default();
        let curve = dof_curve_params(&dof, 5.0);
        // In focus, and anywhere the focal size still covers.
        assert_eq!(dof_coc(curve, 0.005), 0.0);
        assert_eq!(dof_coc(curve, 0.01), 0.0);
        // Ten times the focus distance: 0.045 * 1.5 / 0.05001 - 0.95.
        assert!(
            (dof_coc(curve, 0.05) - 0.399_73).abs() < 1e-4,
            "{}",
            dof_coc(curve, 0.05)
        );
        // Behind the focus the circle never passes the aperture term less
        // the focal size.
        assert!((dof_coc(curve, 1.0) - (1.5 * 0.995 / 1.00001 - 0.95)).abs() < 1e-5);
        // Close to the camera it reaches the cap.
        assert_eq!(dof_coc(curve, 0.001), 1.0);
        assert_eq!(dof_coc_front(curve, 0.001), 1.0);
        assert!((dof_coc_front(curve, 0.003) - dof_coc(curve, 0.003)).abs() < 1e-7);
        assert_eq!(dof_coc_front(curve, 0.05), 0.0);
        // An aperture of 1: sharp only at the focus itself.
        let open = dof_curve_params(&DepthOfField { aperture: 1.0, ..dof }, 5.0);
        assert!(open[2].is_finite());
        assert_eq!(dof_coc(open, 0.005), 0.0);
        assert_eq!(dof_coc(open, 0.0051), 1.0);
    }

    #[test]
    fn depth_of_field_weights_follow_the_shader() {
        assert_eq!(dof_spread_weight(0.3, 0.3), 1.0);
        assert_eq!(dof_spread_weight(0.9, 0.3), 1.0);
        assert_eq!(dof_spread_weight(0.05, 0.3), 0.5);
        assert_eq!(dof_spread_weight(0.0, 0.5), 0.0);
        assert!((dof_disc_weight(0.4, 0.4) - 1.0).abs() < 1e-5);
        assert_eq!(dof_disc_weight(0.1, 0.4), 0.0);
        assert!((dof_disc_weight(0.4 - 0.1325, 0.4) - 0.5).abs() < 1e-5);
        assert_eq!(dof_low_blend(0.65), 0.0);
        assert!((dof_low_blend(0.75) - 0.5).abs() < 1e-5);
        assert!((dof_low_blend(0.85) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn disc_is_the_shaders_and_the_same_on_both_sides() {
        let wgsl = include_str!("effects.wgsl");
        for [x, y, z] in DOF_DISC {
            assert!(((x * x + y * y).sqrt() - z).abs() < 2e-5, "{x} {y} {z}");
            assert!(wgsl.contains(&format!("vec3f({x}, {y}, {z}),")), "{x} {y} {z}");
        }
        assert_eq!(wgsl.matches("    vec3f(").count(), DOF_DISC.len());
    }

    #[test]
    fn sun_shafts_are_the_studio_cameras_component() {
        let shafts = SunShafts::default();
        assert!((shafts.threshold[0] - 0.501_960_8).abs() < 1e-6);
        assert_eq!(
            (shafts.divisor, shafts.blend, shafts.iterations),
            (2, ShaftBlend::Add, 2)
        );
        assert_eq!(
            (shafts.blur_radius, shafts.intensity, shafts.max_radius),
            (3.0, 2.2, 0.5)
        );
        // sunShaftBlurRadius / 768, then 6, 12 and 18 times that.
        let radii = shaft_radii(&shafts);
        let want = [3.0 / 768.0, 18.0 / 768.0, 36.0 / 768.0, 54.0 / 768.0];
        assert_eq!(radii.len(), 4);
        for (r, w) in radii.iter().zip(want) {
            assert!((r - w).abs() < 1e-7, "{radii:?}");
        }
        // radialBlurIterations is kept within 1 to 4.
        let many = SunShafts {
            iterations: 9,
            ..shafts.clone()
        };
        assert_eq!(shaft_radii(&many).len(), 8);
        let none = SunShafts {
            iterations: 0,
            ..shafts
        };
        assert_eq!(shaft_radii(&none).len(), 2);
    }

    #[test]
    fn caster_lands_where_the_camera_shows_it() {
        let eye = Vec3::new(1.0, 2.0, -6.0);
        let target = Vec3::new(0.5, 1.0, 0.0);
        let forward = (target - eye).normalize();
        let (fov, aspect) = (23.0f32, 16.0 / 9.0);
        let point = Vec3::new(-0.4, 2.2, 3.0);
        let clip = glam::Mat4::perspective_lh(fov.to_radians(), aspect, 0.1, 100.0)
            * glam::Mat4::look_at_lh(eye, target, Vec3::Y)
            * point.extend(1.0);
        let got = viewport_point(point, eye, forward, fov, aspect);
        assert!((got[0] - (clip.x / clip.w * 0.5 + 0.5)).abs() < 1e-5, "{got:?}");
        assert!((got[1] - (clip.y / clip.w * 0.5 + 0.5)).abs() < 1e-5, "{got:?}");
        assert!((got[2] - clip.w).abs() < 1e-4, "{got:?}");
        // The point the camera looks at is the middle, its distance away.
        let middle = viewport_point(target, eye, forward, fov, aspect);
        assert!((middle[0] - 0.5).abs() < 1e-6 && (middle[1] - 0.5).abs() < 1e-6);
        assert!((middle[2] - (target - eye).length()).abs() < 1e-5);
        // Behind the camera the depth is negative, which puts the rays out.
        let shafts = SunShafts {
            caster: Some(eye - forward * 3.0),
            ..Default::default()
        };
        let sun = shaft_sun(&shafts, eye, forward, fov, aspect);
        assert!(sun[2] < 0.0);
        assert_eq!(shaft_color(&shafts, sun[2]), [0.0; 3]);
        // No caster: the middle of the screen at depth zero, rays on.
        let free = SunShafts::default();
        assert_eq!(shaft_sun(&free, eye, forward, fov, aspect), [0.5, 0.5, 0.0]);
        assert_eq!(shaft_color(&free, 0.0), [2.2; 3]);
    }

    #[test]
    fn rays_start_at_bright_sky_near_the_caster() {
        let shafts = SunShafts::default();
        let sun = [0.5, 0.75, 10.0];
        let white = [1.0; 3];
        // Each channel is 1 - 128/255 over the threshold; at the caster the
        // distance term is maxRadius.
        let over = 3.0 * (1.0 - 128.0 / 255.0);
        assert!((shaft_mask(&shafts, sun, [0.5, 0.75], white, 1.0) - over * 0.5).abs() < 1e-6);
        assert!((shaft_mask(&shafts, sun, [0.5, 0.5], white, 1.0) - over * 0.25).abs() < 1e-6);
        assert_eq!(shaft_mask(&shafts, sun, [0.5, 0.2], white, 1.0), 0.0);
        // Anything drawn nearer than 0.99 of the far plane gives nothing.
        assert_eq!(shaft_mask(&shafts, sun, [0.5, 0.75], white, 0.99), 0.0);
        assert_eq!(shaft_mask(&shafts, sun, [0.5, 0.75], [0.5; 3], 1.0), 0.0);
        // Only the channels over the threshold count.
        let red = shaft_mask(&shafts, sun, [0.5, 0.75], [1.0, 0.2, 0.2], 1.0);
        assert!((red - over / 3.0 * 0.5).abs() < 1e-6);
    }

    #[test]
    fn rays_go_over_the_picture_by_the_blend_mode() {
        let add = SunShafts {
            color: [1.0, 0.5, 0.0],
            ..Default::default()
        };
        // sat(rays * colour * 2.2) added.
        let got = shaft_blend(&add, 1.0, [0.2, 0.2, 0.2], 0.25);
        assert!((got[0] - 0.75).abs() < 1e-6 && (got[1] - 0.475).abs() < 1e-6 && got[2] == 0.2);
        assert!((shaft_blend(&add, 1.0, [0.2; 3], 5.0)[0] - 1.2).abs() < 1e-6);
        let screen = SunShafts {
            blend: ShaftBlend::Screen,
            ..add.clone()
        };
        let got = shaft_blend(&screen, 1.0, [0.2, 0.2, 0.2], 0.25);
        assert!((got[0] - (1.0 - 0.8 * 0.45)).abs() < 1e-6 && (got[2] - 0.2).abs() < 1e-6);
        assert_eq!(shaft_blend(&add, -1.0, [0.2; 3], 5.0), [0.2; 3]);
    }
}
