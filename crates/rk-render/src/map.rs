// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Drawing maps: many static objects, each a mesh already in world space
//! with a material per submesh. Materials of Unity's Standard shader are
//! lit as that shader lights them (sun, baked or ambient light, reflection
//! probes); the rest plainly. Characters keep their own path in `lib.rs`.

use crate::{Camera, Renderer, COLOR_FORMAT};
use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use rk_chara::scene::Image;
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SHADOW: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The side of the shadow atlas, four cascades of half that side each.
const SHADOW_ATLAS: u32 = 4096;
const SAMPLES: u32 = 4;

/// How a material's surface is drawn, as Unity's Standard shader's
/// rendering modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    Opaque,
    /// Alpha below the threshold is cut away; depth is written.
    Cutout,
    /// Blended by alpha, drawn after everything opaque, back to front.
    Fade,
    /// Added onto what is behind, by alpha, drawn with the blended ones.
    Additive,
    /// Standard's "Transparent" mode: the colour arrives already faded by
    /// its alpha (Unity's Blend One OneMinusSrcAlpha), so that reflections
    /// stay on clear glass.
    Premultiplied,
}

impl Blend {
    /// The pipeline's blending: 0 none, 1 by alpha, 2 added, 3 already
    /// faded.
    fn kind(self) -> u8 {
        match self {
            Blend::Opaque | Blend::Cutout => 0,
            Blend::Fade => 1,
            Blend::Additive => 2,
            Blend::Premultiplied => 3,
        }
    }
}

/// A map texture: the game's block-compressed data as it is stored, or
/// decoded colour for the formats that are not blocks.
#[derive(Debug)]
pub enum MapImage {
    /// BC1 (DXT1) or BC3 (DXT5) blocks, each mip level after the other,
    /// rows of blocks bottom-up as Unity stores them. The GPU decodes them
    /// as the game's does, so the picture is the game's exactly.
    Compressed {
        name: String,
        width: u32,
        height: u32,
        bc3: bool,
        levels: Vec<Vec<u8>>,
        srgb: bool,
        repeat: bool,
    },
    Decoded(Arc<Image>),
}

impl MapImage {
    pub fn repeat(&self) -> bool {
        match self {
            MapImage::Compressed { repeat, .. } => *repeat,
            MapImage::Decoded(i) => i.repeat,
        }
    }

    /// The bytes it takes on the GPU, mip levels included.
    pub fn gpu_bytes(&self) -> u64 {
        match self {
            MapImage::Compressed { levels, .. } => levels.iter().map(|l| l.len() as u64).sum(),
            // Mip levels add a third.
            MapImage::Decoded(i) => i.rgba.len() as u64 * 4 / 3,
        }
    }
}

/// Where a Standard material's smoothness comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SmoothnessFrom {
    /// `_Glossiness`, one value.
    #[default]
    Value,
    /// The alpha of the metallic (or specular) map, times `_GlossMapScale`.
    MapAlpha,
    /// The alpha of the albedo, times `_GlossMapScale`.
    AlbedoAlpha,
}

/// Standard's detail maps (`_DETAIL_MULX2`).
#[derive(Debug, Default)]
pub struct Detail {
    pub albedo: Option<Arc<MapImage>>,
    pub normal: Option<Arc<MapImage>>,
    /// Where the detail shows, in its alpha; everywhere without one.
    pub mask: Option<Arc<MapImage>>,
    pub normal_scale: f32,
    /// The detail maps' own scale in xy and offset in zw.
    pub st: [f32; 4],
    /// Read with the mesh's second UV set (`_UVSec` 1).
    pub second_uv: bool,
}

/// What Unity's Standard shader reads of a material besides its colour and
/// emission.
#[derive(Debug)]
pub struct Surface {
    pub metallic: f32,
    /// `_Glossiness`, or `_GlossMapScale` when a texture gives it.
    pub smoothness: f32,
    pub smoothness_from: SmoothnessFrom,
    /// `_MetallicGlossMap`: metallic in red, smoothness in alpha. In the
    /// specular setup `_SpecGlossMap`: specular colour and smoothness.
    pub gloss_map: Option<Arc<MapImage>>,
    /// The specular setup's `_SpecColor`, linear; `None` for the metallic
    /// setup.
    pub specular: Option<[f32; 3]>,
    pub normal_map: Option<Arc<MapImage>>,
    pub normal_scale: f32,
    pub occlusion_map: Option<Arc<MapImage>>,
    pub occlusion_strength: f32,
    pub detail: Option<Detail>,
    /// `_ParallaxMap` and `_Parallax`.
    pub height: Option<(Arc<MapImage>, f32)>,
    /// False with `_SPECULARHIGHLIGHTS_OFF`.
    pub highlights: bool,
    /// False with `_GLOSSYREFLECTIONS_OFF`.
    pub reflections: bool,
}

impl Default for Surface {
    /// Standard's own defaults.
    fn default() -> Surface {
        Surface {
            metallic: 0.0,
            smoothness: 0.5,
            smoothness_from: SmoothnessFrom::Value,
            gloss_map: None,
            specular: None,
            normal_map: None,
            normal_scale: 1.0,
            occlusion_map: None,
            occlusion_strength: 1.0,
            detail: None,
            height: None,
            highlights: true,
            reflections: true,
        }
    }
}

/// The shader's flags of a material (`F_*` in `map.wgsl`).
const F_STANDARD: u32 = 1;
const F_GLOSS_MAP: u32 = 2;
const F_NORMAL_MAP: u32 = 4;
const F_DETAIL: u32 = 8;
const F_PARALLAX: u32 = 16;
const F_SMOOTH_ALBEDO: u32 = 32;
const F_NO_HIGHLIGHTS: u32 = 64;
const F_NO_REFLECTIONS: u32 = 128;
const F_PREMULTIPLY: u32 = 256;
const F_SPECULAR_SETUP: u32 = 512;

/// A map material, as far as this path reads it.
#[derive(Debug)]
pub struct MapMaterial {
    /// Linear colour the texture is multiplied by.
    pub color: [f32; 4],
    pub albedo: Option<Arc<MapImage>>,
    /// Texture scale in xy, offset in zw.
    pub st: [f32; 4],
    /// Linear emitted colour, added after lighting, times `emission_map`.
    pub emission: [f32; 3],
    pub emission_map: Option<Arc<MapImage>>,
    pub blend: Blend,
    pub cutoff: f32,
    pub two_sided: bool,
    /// Lit by nothing: the colour shows as it is.
    pub unlit: bool,
    /// Unity's legacy particle shaders, which also paint maps' foliage:
    /// 2 * vertex colour * `color` * texture, alpha included, unlit.
    pub particle: bool,
    /// False for a material another pass draws, or none does yet: its
    /// submesh is left out.
    pub drawn: bool,
    /// Unity's render queue: blended surfaces are drawn in its order first.
    pub queue: i32,
    /// Set for Unity's Standard shaders, which are lit as Standard lights;
    /// without it a lit material is lit plainly.
    pub surface: Option<Surface>,
}

impl MapMaterial {
    /// A material whose submesh is not drawn.
    pub fn hidden() -> MapMaterial {
        MapMaterial {
            color: [0.0; 4],
            albedo: None,
            st: [1.0, 1.0, 0.0, 0.0],
            emission: [0.0; 3],
            emission_map: None,
            blend: Blend::Opaque,
            cutoff: 0.5,
            two_sided: false,
            unlit: false,
            particle: false,
            drawn: false,
            queue: 2000,
            surface: None,
        }
    }

    /// Every texture the material reads.
    pub fn images(&self) -> impl Iterator<Item = &Arc<MapImage>> {
        let surface = self.surface.iter().flat_map(|s| {
            let detail = s
                .detail
                .iter()
                .flat_map(|d| d.albedo.iter().chain(&d.normal).chain(&d.mask));
            s.gloss_map
                .iter()
                .chain(&s.normal_map)
                .chain(&s.occlusion_map)
                .chain(s.height.iter().map(|h| &h.0))
                .chain(detail)
        });
        self.albedo.iter().chain(&self.emission_map).chain(surface)
    }

    /// The material's numbers as the shader reads them.
    fn uniform(&self, receives: bool, sunlit: bool) -> MaterialUniform {
        let mut u = MaterialUniform {
            color: self.color,
            st: self.st,
            emission: [self.emission[0], self.emission[1], self.emission[2], 0.0],
            params: [
                if self.blend == Blend::Cutout { self.cutoff } else { -1.0 },
                if self.blend.kind() > 0 { 1.0 } else { 0.0 },
                // 0 lit, 1 unlit, 2 a particle shader.
                if self.particle {
                    2.0
                } else if self.unlit {
                    1.0
                } else {
                    0.0
                },
                // 1 when shadows fall on it, 2 more when the sun lights it
                // at all.
                f32::from(u8::from(receives)) + 2.0 * f32::from(u8::from(sunlit)),
            ],
            surface: [0.0, 0.5, 1.0, 1.0],
            detail_st: [1.0, 1.0, 0.0, 0.0],
            detail: [1.0, 0.0, 0.0, 0.0],
            spec_color: [0.0; 4],
            flags: [0; 4],
        };
        let Some(s) = self.surface.as_ref().filter(|_| !self.unlit && !self.particle) else {
            return u;
        };
        let mut flags = F_STANDARD;
        let mut set = |on: bool, flag: u32| {
            if on {
                flags |= flag;
            }
        };
        set(s.smoothness_from == SmoothnessFrom::AlbedoAlpha, F_SMOOTH_ALBEDO);
        // Without its texture a map reads Unity's default for it: white
        // for the metallic map, a flat normal map, no height.
        set(
            s.gloss_map.is_some() || s.smoothness_from == SmoothnessFrom::MapAlpha,
            F_GLOSS_MAP,
        );
        set(s.normal_map.is_some(), F_NORMAL_MAP);
        set(s.detail.is_some(), F_DETAIL);
        set(s.height.is_some(), F_PARALLAX);
        set(!s.highlights, F_NO_HIGHLIGHTS);
        set(!s.reflections, F_NO_REFLECTIONS);
        set(self.blend == Blend::Premultiplied, F_PREMULTIPLY);
        set(s.specular.is_some(), F_SPECULAR_SETUP);
        u.surface = [s.metallic, s.smoothness, s.normal_scale, s.occlusion_strength];
        if let Some(d) = &s.detail {
            u.detail_st = d.st;
            u.detail[0] = d.normal_scale;
            u.detail[1] = f32::from(u8::from(d.second_uv));
        }
        u.detail[2] = s.height.as_ref().map_or(0.0, |h| h.1);
        if let Some([r, g, b]) = s.specular {
            u.spec_color = [r, g, b, 0.0];
        }
        u.flags[0] = flags;
        u
    }
}

/// A mesh in world space.
#[derive(Debug, Default)]
pub struct MapMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    /// Where each vertex reads its object's lightmap, already scaled and
    /// moved into the object's part of it. Empty without a lightmap.
    pub uv2: Vec<[f32; 2]>,
    /// Vertex colours as stored; empty for white.
    pub colors: Vec<[f32; 4]>,
    /// Tangents in the world, with the sign of the binormal in w (turned
    /// for a mirrored object, as Unity's `unity_WorldTransformParams`
    /// does). Empty for a mesh without them: its normal maps stay flat.
    pub tangents: Vec<[f32; 4]>,
    /// The second UV set as stored, for detail maps that read it; empty
    /// when it is the first.
    pub uv1: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// First index and count of each submesh.
    pub submeshes: Vec<(u32, u32)>,
    /// Lowest and highest corner, in the world.
    pub bounds: ([f32; 3], [f32; 3]),
}

impl MapMesh {
    /// The box around the positions.
    pub fn compute_bounds(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for p in positions {
            let p = Vec3::from(*p);
            if p.is_finite() {
                min = min.min(p);
                max = max.max(p);
            }
        }
        (min.to_array(), max.to_array())
    }
}

/// Whether a box lies at least partly inside the view of `view_proj`: it
/// is out when all its corners are behind one of the view's planes.
fn in_view(view_proj: &Mat4, (min, max): ([f32; 3], [f32; 3])) -> bool {
    let rows = [view_proj.row(0), view_proj.row(1), view_proj.row(2), view_proj.row(3)];
    // Left, right, bottom, top, near (depth 0 to 1), far.
    let planes = [
        rows[3] + rows[0],
        rows[3] - rows[0],
        rows[3] + rows[1],
        rows[3] - rows[1],
        rows[2],
        rows[3] - rows[2],
    ];
    planes.iter().all(|p| {
        // The corner furthest along the plane's normal.
        let corner = glam::Vec4::new(
            if p.x >= 0.0 { max[0] } else { min[0] },
            if p.y >= 0.0 { max[1] } else { min[1] },
            if p.z >= 0.0 { max[2] } else { min[2] },
            1.0,
        );
        p.dot(corner) >= 0.0
    })
}

/// An object of the map: its mesh, placed, and a material per submesh.
#[derive(Debug)]
pub struct MapObject {
    pub name: String,
    pub mesh: MapMesh,
    pub materials: Vec<Arc<MapMaterial>>,
    /// The baked lightmap it reads, by its place in `MapScene::lightmaps`.
    pub lightmap: Option<usize>,
    /// Whether it casts the sun's shadow, and whether shadows fall on it.
    pub casts_shadows: bool,
    pub receives_shadows: bool,
    /// Casts its shadow without being seen (Unity's "shadows only").
    pub shadows_only: bool,
    /// Its Unity layer: the sun lights, and takes shadows from, only the
    /// layers of its culling mask.
    pub layer: u32,
    /// The volumes of `MapScene::vanish` that hide it while the camera is
    /// inside one.
    pub vanish: Vec<usize>,
    /// The reflection probe it reflects, by its place in
    /// `MapScene::probes`; without one, the scene's default reflection.
    pub probe: Option<usize>,
}

/// A cubemap to reflect, with Unity's decoding of its colour
/// (`unity_SpecCube0_HDR`: x scales it, the probe's intensity included).
#[derive(Debug, Clone, PartialEq)]
pub struct Reflection {
    /// Without a cubemap the reflection is black, Unity's default.
    pub cube: Option<Arc<crate::sky::MapCube>>,
    pub hdr: [f32; 4],
}

/// A reflection probe: its box in the world, which is never turned, and
/// what it reflects.
#[derive(Debug, Clone, PartialEq)]
pub struct ReflectionProbe {
    /// Where the probe stands.
    pub position: [f32; 3],
    /// The middle of its box and half its size.
    pub centre: [f32; 3],
    pub half: [f32; 3],
    /// Whether the cubemap is projected on the box.
    pub box_projection: bool,
    pub importance: i32,
    pub reflection: Reflection,
}

/// The probe an object reflects: among those whose box touches the
/// object's bounds, the most important, then the one sharing the most
/// volume with the bounds, then the smallest. Unity's rule for its forward
/// path, from its manual; where Unity would blend two probes of the same
/// importance, only the first is taken here.
pub fn pick_probe(bounds: ([f32; 3], [f32; 3]), probes: &[ReflectionProbe]) -> Option<usize> {
    let (lo, hi) = (Vec3::from(bounds.0), Vec3::from(bounds.1));
    let mut best: Option<(usize, i32, f32, f32)> = None;
    for (i, p) in probes.iter().enumerate() {
        let (centre, half) = (Vec3::from(p.centre), Vec3::from(p.half));
        let shared = (hi.min(centre + half) - lo.max(centre - half)).max(Vec3::ZERO);
        if hi.cmplt(centre - half).any() || lo.cmpgt(centre + half).any() {
            continue;
        }
        let (volume, size) = (shared.x * shared.y * shared.z, half.x * half.y * half.z);
        let better = best.is_none_or(|(_, importance, v, s)| (p.importance, volume, -size) > (importance, v, -s));
        if better {
            best = Some((i, p.importance, volume, size));
        }
    }
    best.map(|b| b.0)
}

/// A volume of a map's table of walls that hide (CharaStudio's vanish):
/// while the camera is inside it, the objects naming it are not drawn and
/// cast no shadow, so that a room shows from outside its walls.
#[derive(Debug, Clone, PartialEq)]
pub enum VanishVolume {
    /// A box: world to the box's own space, where it spans `centre` plus
    /// and minus `half`.
    Box {
        to_local: Mat4,
        centre: Vec3,
        half: Vec3,
    },
    Sphere {
        centre: Vec3,
        radius: f32,
    },
}

impl VanishVolume {
    pub fn contains(&self, p: Vec3) -> bool {
        match self {
            VanishVolume::Box { to_local, centre, half } => {
                let d = (to_local.transform_point3(p) - *centre).abs();
                d.x <= half.x && d.y <= half.y && d.z <= half.z
            }
            VanishVolume::Sphere { centre, radius } => p.distance(*centre) <= *radius,
        }
    }
}

/// A baked lightmap: the light, stored as RGBM, and, for directional
/// lightmaps, the main direction light came from.
#[derive(Debug)]
pub struct Lightmap {
    pub light: Arc<MapImage>,
    pub direction: Option<Arc<MapImage>>,
}

/// Unity 5.6's decoding of RGBM lightmaps in linear colour
/// (`DecodeLightmapRGBM` with the `unity_Lightmap_HDR` it sets there):
/// light = scale * a^exponent * rgb. The scale is 5^2.2: RGBM keeps up to
/// five times white, in gamma.
pub const RGBM_SCALE: f32 = 34.493_24;
pub const RGBM_EXPONENT: f32 = 2.2;

/// Linear fog.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fog {
    pub color: [f32; 3],
    pub start: f32,
    pub end: f32,
}

/// What lights a map.
#[derive(Debug, Clone, PartialEq)]
pub struct MapLight {
    /// Unit vector towards the sun.
    pub sun_dir: [f32; 3],
    /// Linear colour times intensity.
    pub sun_color: [f32; 3],
    /// The layers the sun lights, a bit each.
    pub sun_mask: u32,
    /// Ambient light as Unity's shaders read it (`unity_SHAr` to
    /// `unity_SHC`, see `modshader::sh_constants`), for what has no
    /// lightmap.
    pub ambient_sh: [[f32; 4]; 7],
    pub fog: Option<Fog>,
    /// The scene's sky, drawn behind everything as a camera that clears
    /// with the skybox does. Without one the picture is cleared with
    /// `background`.
    pub sky: Option<crate::sky::Sky>,
    /// The scene's own Unity skybox (`RenderSettings`), which CharaStudio's
    /// camera never draws; kept for drawing it on request.
    pub unity_sky: Option<crate::sky::Sky>,
    /// What shows where nothing is drawn.
    pub background: [f32; 3],
    /// A cutaway: nothing above this height is drawn. For looking into
    /// rooms from above.
    pub cut_above: Option<f32>,
    /// The sun's real-time shadows, when it casts them.
    pub shadows: Option<SunShadows>,
    /// What objects outside every reflection probe reflect: the cubemap
    /// Unity baked from the sky, or the scene's custom one.
    pub reflection: Reflection,
    /// Unity's `unity_IndirectSpecColor`: what a material with its
    /// reflections switched off reflects instead.
    pub indirect_specular: [f32; 3],
}

/// How the sun's shadows are drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunShadows {
    /// How dark a shadow is: 1 takes all the sun away.
    pub strength: f32,
    /// How far from the camera shadows reach, in metres.
    pub distance: f32,
}

/// A map ready to draw.
#[derive(Debug)]
pub struct MapScene {
    pub objects: Vec<MapObject>,
    pub lightmaps: Vec<Lightmap>,
    pub light: MapLight,
    /// The reflection probes objects name.
    pub probes: Vec<ReflectionProbe>,
    /// The volumes of the walls that hide.
    pub vanish: Vec<VanishVolume>,
    /// The layers the camera draws, a bit each. Objects on other layers
    /// are not seen, though they still cast the sun's shadow when they are
    /// on its layers, as in Unity.
    pub camera_mask: u32,
}

/// The layers CharaStudio's camera draws: Water (4), Chara (10), Map (11)
/// and 29 (Studio/Camera), as its captures record it.
pub const STUDIO_CAMERA_MASK: u32 = 536_874_000;

impl MapScene {
    /// Whether the camera sees `object`: it is on one of the camera's
    /// layers and not a shadow alone.
    pub fn seen(&self, object: &MapObject) -> bool {
        !object.shadows_only && object.layer < 32 && self.camera_mask & (1 << object.layer) != 0
    }

    /// The lowest and highest corner of everything drawn.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for (lo, hi) in self
            .objects
            .iter()
            .filter(|o| !o.mesh.positions.is_empty())
            .map(|o| o.mesh.bounds)
        {
            min = min.min(Vec3::from(lo));
            max = max.max(Vec3::from(hi));
        }
        if min.x > max.x {
            (Vec3::ZERO, Vec3::ONE)
        } else {
            (min, max)
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    uv2: [f32; 2],
    color: [f32; 4],
    tangent: [f32; 4],
    uv1: [f32; 2],
}

/// One probe as the shader reads it: Unity's `unity_SpecCube0_BoxMax`,
/// `_BoxMin`, `_ProbePosition` and `_HDR`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
struct ProbeUniform {
    box_max: [f32; 4],
    /// w: how much of this probe shows over the second, 1 for all.
    box_min: [f32; 4],
    /// w: 1 when the cubemap is projected on the box.
    position: [f32; 4],
    hdr: [f32; 4],
}

impl ProbeUniform {
    /// The scene's default reflection: no box.
    fn plain(r: &Reflection) -> ProbeUniform {
        ProbeUniform {
            box_max: [0.0; 4],
            box_min: [0.0, 0.0, 0.0, 1.0],
            position: [0.0; 4],
            hdr: r.hdr,
        }
    }

    fn of(p: &ReflectionProbe) -> ProbeUniform {
        let (c, h) = (Vec3::from(p.centre), Vec3::from(p.half));
        ProbeUniform {
            box_max: (c + h).extend(0.0).to_array(),
            box_min: (c - h).extend(1.0).to_array(),
            position: Vec3::from(p.position)
                .extend(f32::from(u8::from(p.box_projection)))
                .to_array(),
            hdr: p.reflection.hdr,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShadowUniform {
    to_atlas: [[f32; 16]; 4],
    ends: [f32; 4],
    params: [f32; 4],
    texel_world: [f32; 4],
    forward: [f32; 4],
}

/// One cascade of the sun's shadow map: world to the cascade's clip
/// space, and the world size of one texel. The cascade's sides fit its
/// slice of the view, snapped to whole texels so that shadows hold still
/// as the camera moves; its depth covers the whole map, so that a caster
/// outside the slice still shades it.
fn fit_cascade(slice: &[Vec3; 8], light: Mat4, map: (Vec3, Vec3), tile: u32) -> Option<(Mat4, f32)> {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for p in slice {
        let q = light.transform_point3(*p);
        lo = lo.min(q);
        hi = hi.max(q);
    }
    // A square around the slice's bounding sphere keeps the size steady
    // as the view turns.
    let centre_world = slice.iter().fold(Vec3::ZERO, |a, p| a + *p) / 8.0;
    let radius = slice.iter().map(|p| p.distance(centre_world)).fold(0.0, f32::max);
    let size = (radius * 2.0) * (1.0 + 20.0 / tile as f32);
    let texel = size / tile as f32;
    let centre = light.transform_point3(centre_world);
    let snap = |v: f32| (v / texel).floor() * texel;
    let (cx, cy) = (snap(centre.x), snap(centre.y));
    let half = size * 0.5;
    let (mut near, mut far) = (lo.z, hi.z);
    for i in 0..8 {
        let corner = Vec3::new(
            if i & 1 == 0 { map.0.x } else { map.1.x },
            if i & 2 == 0 { map.0.y } else { map.1.y },
            if i & 4 == 0 { map.0.z } else { map.1.z },
        );
        let z = light.transform_point3(corner).z;
        near = near.min(z);
        far = far.max(z);
    }
    if half <= 0.0 || far <= near {
        return None;
    }
    let proj = Mat4::orthographic_lh(cx - half, cx + half, cy - half, cy + half, near - 1.0, far + 1.0);
    Some((proj * light, texel))
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LightmapUniform {
    /// 1 when there is a lightmap, 1 when it is directional, then the RGBM
    /// scale and exponent.
    info: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [f32; 16],
    eye: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    sh: [[f32; 4]; 7],
    fog_color: [f32; 4],
    fog_range: [f32; 4],
    indirect_spec: [f32; 4],
    lamps: [[f32; 4]; 24],
    lamp_info: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
struct MaterialUniform {
    color: [f32; 4],
    st: [f32; 4],
    emission: [f32; 4],
    params: [f32; 4],
    /// Metallic, smoothness, the normal map's scale, the occlusion's
    /// strength.
    surface: [f32; 4],
    detail_st: [f32; 4],
    /// The detail normal's scale, 1 for the second UV set, the height
    /// map's scale.
    detail: [f32; 4],
    spec_color: [f32; 4],
    flags: [u32; 4],
}

/// The formats of what a map is drawn into: its own picture, or the
/// character frame's.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Target {
    pub color: wgpu::TextureFormat,
    pub depth: wgpu::TextureFormat,
}

impl Target {
    /// The map's own picture, drawn by `Renderer::draw_map`.
    const OWN: Target = Target {
        color: COLOR_FORMAT,
        depth: DEPTH,
    };
}

/// The vertex layout of `Vertex`.
const ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x2, 4 => Float32x4, 5 => Float32x4, 6 => Float32x2];

/// The map pass's pipelines, made the first time a map is set.
pub(crate) struct Pipelines {
    globals: wgpu::BindGroupLayout,
    material: wgpu::BindGroupLayout,
    lightmap: wgpu::BindGroupLayout,
    reflection: wgpu::BindGroupLayout,
    /// Reads reflection cubemaps: between texels and between levels.
    cube_sampler: wgpu::Sampler,
    /// What is reflected without a cubemap.
    black_cube: wgpu::TextureView,
    cascade: wgpu::BindGroupLayout,
    /// Shadow casters, by cut-out and by culling.
    casters: HashMap<(bool, bool), wgpu::RenderPipeline>,
    /// The same surfaces into the camera's depth texture.
    depth: HashMap<(bool, bool), wgpu::RenderPipeline>,
    compare: wgpu::Sampler,
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    /// By target, blending and whether back faces are culled; made for a
    /// target the first time it is drawn into.
    pipelines: HashMap<(Target, u8, bool), wgpu::RenderPipeline>,
    repeat: wgpu::Sampler,
    clamp: wgpu::Sampler,
    /// The textures of the map uploaded last, by the image's address. Each
    /// keeps its image, so that the address is not handed to another image
    /// while the texture is cached under it.
    images: HashMap<usize, (Arc<MapImage>, wgpu::TextureView)>,
    /// How many objects the last frame left out as outside the view.
    culled: std::cell::Cell<usize>,
    sky: crate::sky::SkyPass,
}

/// Uploads BC blocks with their mip levels.
fn upload_blocks(device: &wgpu::Device, queue: &wgpu::Queue, image: &MapImage) -> Option<wgpu::TextureView> {
    let MapImage::Compressed {
        name,
        width,
        height,
        bc3,
        levels,
        srgb,
        ..
    } = image
    else {
        return None;
    };
    let format = match (bc3, srgb) {
        (false, true) => wgpu::TextureFormat::Bc1RgbaUnormSrgb,
        (false, false) => wgpu::TextureFormat::Bc1RgbaUnorm,
        (true, true) => wgpu::TextureFormat::Bc3RgbaUnormSrgb,
        (true, false) => wgpu::TextureFormat::Bc3RgbaUnorm,
    };
    let block = if *bc3 { 16 } else { 8 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(name),
        size: wgpu::Extent3d {
            width: *width,
            height: *height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len().max(1) as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (i, data) in levels.iter().enumerate() {
        let (w, h) = ((width >> i).max(1), (height >> i).max(1));
        let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: i as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bw * block),
                rows_per_image: Some(bh),
            },
            // A level smaller than a block still takes a whole block.
            wgpu::Extent3d {
                width: bw * 4,
                height: bh * 4,
                depth_or_array_layers: 1,
            },
        );
    }
    Some(texture.create_view(&Default::default()))
}

impl Pipelines {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Pipelines {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("map"),
            source: wgpu::ShaderSource::Wgsl(include_str!("map.wgsl").into()),
        });
        let uniform = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let both = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let globals = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map globals"),
            entries: &[
                uniform(0, both),
                uniform(1, wgpu::ShaderStages::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let cascade = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map shadow cascade"),
            entries: &[uniform(0, wgpu::ShaderStages::VERTEX)],
        });
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let cube = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::Cube,
                multisampled: false,
            },
            count: None,
        };
        let filtering = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        // The uniform, the albedo and its sampler, the emission map, then
        // Standard's maps (metallic, normal, occlusion, detail albedo,
        // detail normal, detail mask, height) and the detail maps' sampler.
        let material = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map material"),
            entries: &[
                uniform(0, wgpu::ShaderStages::FRAGMENT),
                texture(1),
                filtering(2),
                texture(3),
                texture(4),
                texture(5),
                texture(6),
                texture(7),
                texture(8),
                texture(9),
                texture(10),
                filtering(11),
            ],
        });
        let reflection = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map reflection"),
            entries: &[uniform(0, wgpu::ShaderStages::FRAGMENT), cube(1), cube(2), filtering(3)],
        });
        let lightmap = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map lightmap"),
            entries: &[
                uniform(0, wgpu::ShaderStages::FRAGMENT),
                texture(1),
                texture(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map"),
            bind_group_layouts: &[Some(&globals), Some(&material), Some(&lightmap), Some(&reflection)],
            immediate_size: 0,
        });
        let sampler = |mode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: mode,
                address_mode_v: mode,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                anisotropy_clamp: 16,
                ..Default::default()
            })
        };
        // The sun's shadow casters: depth only, cut-out or solid, culled or
        // two-sided.
        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("map shadow"),
            source: wgpu::ShaderSource::Wgsl(include_str!("map_shadow.wgsl").into()),
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map shadow"),
            bind_group_layouts: &[Some(&cascade), Some(&material)],
            immediate_size: 0,
        });
        let mut casters = HashMap::new();
        let mut depth = HashMap::new();
        // The camera's depth texture is the same pass without the light's
        // bias.
        for (camera, cutout, cull) in [false, true].into_iter().flat_map(|a| {
            [false, true]
                .into_iter()
                .flat_map(move |b| [false, true].map(|c| (a, b, c)))
        }) {
            {
                let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("map shadow"),
                    layout: Some(&shadow_layout),
                    vertex: wgpu::VertexState {
                        module: &shadow_shader,
                        entry_point: Some("vs_main"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Vertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &ATTRIBUTES,
                        })],
                    },
                    fragment: cutout.then(|| wgpu::FragmentState {
                        module: &shadow_shader,
                        entry_point: Some("fs_cutout"),
                        compilation_options: Default::default(),
                        targets: &[],
                    }),
                    primitive: wgpu::PrimitiveState {
                        front_face: wgpu::FrontFace::Cw,
                        cull_mode: cull.then_some(wgpu::Face::Back),
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: SHADOW,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(wgpu::CompareFunction::LessEqual),
                        stencil: Default::default(),
                        bias: if camera {
                            wgpu::DepthBiasState::default()
                        } else {
                            wgpu::DepthBiasState {
                                constant: 2,
                                slope_scale: 2.0,
                                clamp: 0.0,
                            }
                        },
                    }),
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                });
                if camera {
                    depth.insert((cutout, cull), pipeline);
                } else {
                    casters.insert((cutout, cull), pipeline);
                }
            }
        }
        let compare = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("map shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let cube_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("map reflection"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let black_cube = crate::sky::upload_cube(
            device,
            queue,
            &crate::sky::MapCube {
                name: "black".into(),
                size: 1,
                faces: std::array::from_fn(|_| vec![0, 0, 0, 255]),
                srgb: false,
                bc6h: false,
                mips: Vec::new(),
            },
        );
        Pipelines {
            globals,
            material,
            lightmap,
            reflection,
            cube_sampler,
            black_cube,
            cascade,
            casters,
            depth,
            compare,
            shader,
            layout,
            pipelines: HashMap::new(),
            repeat: sampler(wgpu::AddressMode::Repeat),
            clamp: sampler(wgpu::AddressMode::ClampToEdge),
            images: HashMap::new(),
            culled: std::cell::Cell::new(0),
            sky: crate::sky::SkyPass::new(device, queue),
        }
    }
    /// Makes the pipelines for drawing into `target`, once. They neither
    /// test nor write stencil, so the characters' stencil in the same pass
    /// stays as theirs.
    pub(crate) fn prepare(&mut self, device: &wgpu::Device, target: Target) {
        self.sky.prepare(device, target.color, target.depth, SAMPLES);
        if self.pipelines.contains_key(&(target, 0, false)) {
            return;
        }
        for blend in [0u8, 1, 2, 3] {
            let blended = blend > 0;
            for cull in [false, true] {
                let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("map"),
                    layout: Some(&self.layout),
                    vertex: wgpu::VertexState {
                        module: &self.shader,
                        entry_point: Some("vs_main"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Vertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &ATTRIBUTES,
                        })],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &self.shader,
                        entry_point: Some("fs_main"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: target.color,
                            blend: match blend {
                                0 => None,
                                1 => Some(wgpu::BlendState::ALPHA_BLENDING),
                                // Unity's Blend One OneMinusSrcAlpha.
                                3 => Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                                // Unity's Blend SrcAlpha One.
                                _ => Some(wgpu::BlendState {
                                    color: wgpu::BlendComponent {
                                        src_factor: wgpu::BlendFactor::SrcAlpha,
                                        dst_factor: wgpu::BlendFactor::One,
                                        operation: wgpu::BlendOperation::Add,
                                    },
                                    alpha: wgpu::BlendComponent {
                                        src_factor: wgpu::BlendFactor::SrcAlpha,
                                        dst_factor: wgpu::BlendFactor::One,
                                        operation: wgpu::BlendOperation::Add,
                                    },
                                }),
                            },
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        // Unity's front faces wind clockwise; the data is
                        // left-handed and so is the projection.
                        front_face: wgpu::FrontFace::Cw,
                        cull_mode: cull.then_some(wgpu::Face::Back),
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: target.depth,
                        depth_write_enabled: Some(!blended),
                        depth_compare: Some(wgpu::CompareFunction::LessEqual),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: wgpu::MultisampleState {
                        count: SAMPLES,
                        ..Default::default()
                    },
                    multiview_mask: None,
                    cache: None,
                });
                self.pipelines.insert((target, blend, cull), pipeline);
            }
        }
    }
}

/// One submesh of an uploaded object.
struct Part {
    first: u32,
    count: u32,
    /// Its bind group in `MapGpu::materials`.
    material: usize,
    /// 0 opaque, 1 blended by alpha, 2 added, 3 already faded.
    blend: u8,
    cull: bool,
    queue: i32,
    /// Casts the sun's shadow, and whether it is cut out there.
    caster: bool,
    cutout: bool,
}

/// An object of an uploaded map, with what each frame decides about it.
struct ObjectGpu {
    /// Its vertex and index buffers in `MapGpu::buffers`; `None` for an
    /// object that is never seen and casts no shadow, which is not
    /// uploaded.
    buffers: Option<usize>,
    bounds: ([f32; 3], [f32; 3]),
    middle: Vec3,
    vanish: Vec<usize>,
    /// On a layer the camera draws, and not shadows only.
    on_camera: bool,
    /// Casts the sun's shadow from somewhere.
    casts: bool,
    /// The lightmap's bind group: 0 for none, then one per lightmap.
    lightmap: usize,
    /// The reflection's bind group: 0 for the scene's, then one per probe.
    reflection: usize,
    parts: Vec<Part>,
}

/// One submesh to draw this frame.
struct Draw {
    buffers: usize,
    lightmap: usize,
    reflection: usize,
    first: u32,
    count: u32,
    material: usize,
    /// Where the material first came up this frame: opaque surfaces are
    /// drawn in that order.
    rank: usize,
    blend: u8,
    cull: bool,
    cutout: bool,
    /// Distance from the eye to the object's middle, for sorting.
    distance: f32,
    queue: i32,
}

/// A submesh casting the sun's shadow this frame.
struct Caster {
    buffers: usize,
    first: u32,
    count: u32,
    material: usize,
    cutout: bool,
    cull: bool,
}

/// What one frame of an uploaded map draws, from `MapGpu::frame`.
pub(crate) struct MapFrame {
    draws: Vec<Draw>,
    casters: Vec<Caster>,
    /// Whether the sun's shadow atlas is drawn this frame.
    shadows: bool,
}

/// Where an uploaded map stands and how it shows, which may change every
/// frame without uploading the map again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapPlacement {
    /// The map's root: the map's own space to the world. It moves and
    /// turns the map, with its sun; a scale in it is not taken.
    pub root: Mat4,
    /// Walls hide while the camera is inside their volumes.
    pub hide_walls: bool,
    /// The map is drawn. While it is not, the characters are lit as if
    /// there were no map.
    pub shown: bool,
}

impl Default for MapPlacement {
    fn default() -> Self {
        MapPlacement {
            root: Mat4::IDENTITY,
            hide_walls: true,
            shown: true,
        }
    }
}

impl MapPlacement {
    /// The root as a turn and a move alone.
    pub fn rigid(&self) -> Mat4 {
        let (_, turn, at) = self.root.to_scale_rotation_translation();
        Mat4::from_rotation_translation(turn.normalize(), at)
    }

    /// A camera of the world as the map's own space sees it.
    fn camera_in_map(&self, camera: &Camera) -> Camera {
        let back = self.rigid().inverse();
        Camera {
            eye: back.transform_point3(camera.eye),
            target: back.transform_point3(camera.target),
            fov_y_deg: camera.fov_y_deg,
        }
    }
}

/// A map uploaded to the GPU: its buffers, textures and bind groups, made
/// once by `Renderer::set_map`. Each frame only writes the camera's
/// numbers into its uniform buffers and picks what to draw.
pub(crate) struct MapGpu {
    /// Where the map stands, set by `Renderer::place_map`.
    pub(crate) placement: MapPlacement,
    pub(crate) light: MapLight,
    /// The lowest and highest corner of everything drawn.
    bounds: (Vec3, Vec3),
    vanish: Vec<VanishVolume>,
    objects: Vec<ObjectGpu>,
    buffers: Vec<(wgpu::Buffer, wgpu::Buffer)>,
    materials: Vec<wgpu::BindGroup>,
    lightmaps: Vec<wgpu::BindGroup>,
    reflections: Vec<wgpu::BindGroup>,
    globals_buf: wgpu::Buffer,
    shadow_buf: wgpu::Buffer,
    /// One per cascade of the sun's shadow, with its bind group.
    cascades: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    /// The sun's shadow atlas; one texel when the sun casts none.
    atlas: wgpu::TextureView,
    globals_group: wgpu::BindGroup,
    /// The camera's view for the depth texture, with its bind group.
    depth_view: (wgpu::Buffer, wgpu::BindGroup),
    /// The sky's uniform buffer and bind group.
    sky: Option<(wgpu::Buffer, wgpu::BindGroup)>,
}

impl MapGpu {
    /// Uploads `map`. `views` holds the view of each of its images by the
    /// image's address, `sky_cube` the sky's cubemap and `cubes` the
    /// reflections' cubemaps, the scene's first.
    fn new(
        device: &wgpu::Device,
        pipes: &Pipelines,
        map: &MapScene,
        views: &HashMap<usize, wgpu::TextureView>,
        white: &wgpu::TextureView,
        sky_cube: Option<&wgpu::TextureView>,
        cubes: &[wgpu::TextureView],
    ) -> MapGpu {
        let l = &map.light;
        let view_of = |image: &Arc<MapImage>| views.get(&(Arc::as_ptr(image) as usize)).cloned();
        let uniform_buf = |label, size: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let globals_buf = uniform_buf("map globals", std::mem::size_of::<Globals>());
        let shadow_buf = uniform_buf("map shadow", std::mem::size_of::<ShadowUniform>());
        let shadowed = l.shadows.is_some();
        let cascades = if shadowed {
            (0..crate::shadow::CASCADES)
                .map(|_| {
                    let buf = uniform_buf("map shadow cascade", std::mem::size_of::<[f32; 16]>());
                    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("map shadow cascade"),
                        layout: &pipes.cascade,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buf.as_entire_binding(),
                        }],
                    });
                    (buf, group)
                })
                .collect()
        } else {
            Vec::new()
        };
        let depth_view = {
            let buf = uniform_buf("map camera depth", std::mem::size_of::<[f32; 16]>());
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("map camera depth"),
                layout: &pipes.cascade,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buf.as_entire_binding(),
                }],
            });
            (buf, group)
        };
        let atlas_side = if shadowed { SHADOW_ATLAS } else { 1 };
        let atlas = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("map shadow atlas"),
                size: wgpu::Extent3d {
                    width: atlas_side,
                    height: atlas_side,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SHADOW,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("map globals"),
            layout: &pipes.globals,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: shadow_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&atlas),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&pipes.compare),
                },
            ],
        });

        // The sky's textures; its numbers follow the camera.
        let sky = l.sky.as_ref().map(|sky| {
            let face = |i: usize| match sky {
                crate::sky::Sky::SixSided { faces, .. } => faces[i].image.as_ref().and_then(view_of),
                _ => None,
            };
            let faces: [Option<wgpu::TextureView>; 6] = std::array::from_fn(face);
            let buf = uniform_buf("map sky", std::mem::size_of::<crate::sky::SkyUniform>());
            let group = pipes.sky.bind(
                device,
                &buf,
                sky_cube,
                std::array::from_fn(|i| faces[i].as_ref().unwrap_or(white)),
                &pipes.clamp,
                &pipes.repeat,
            );
            (buf, group)
        });

        // The lightmaps' bind groups: the first for objects without one.
        let lightmap_group = |info: [f32; 4], light: &wgpu::TextureView, direction: &wgpu::TextureView| {
            let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("map lightmap"),
                contents: bytemuck::bytes_of(&LightmapUniform { info }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("map lightmap"),
                layout: &pipes.lightmap,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(light),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(direction),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&pipes.clamp),
                    },
                ],
            })
        };
        let mut lightmaps = vec![lightmap_group([0.0; 4], white, white)];
        for l in &map.lightmaps {
            let light = view_of(&l.light).unwrap_or_else(|| white.clone());
            let direction = l.direction.as_ref().and_then(view_of);
            let info = [1.0, f32::from(u8::from(direction.is_some())), RGBM_SCALE, RGBM_EXPONENT];
            lightmaps.push(lightmap_group(info, &light, direction.as_ref().unwrap_or(white)));
        }

        // The reflections' bind groups: the scene's default first. A second
        // probe is never blended in (see `pick_probe`), so the first one's
        // cubemap fills both places.
        let reflections: Vec<wgpu::BindGroup> = std::iter::once(ProbeUniform::plain(&l.reflection))
            .chain(map.probes.iter().map(ProbeUniform::of))
            .zip(cubes)
            .map(|(probe, cube)| {
                let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("map reflection"),
                    contents: bytemuck::cast_slice(&[probe, probe]),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("map reflection"),
                    layout: &pipes.reflection,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(cube),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(cube),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::Sampler(&pipes.cube_sampler),
                        },
                    ],
                })
            })
            .collect();

        // One bind group per material and whether shadows fall on it.
        let mut materials: Vec<wgpu::BindGroup> = Vec::new();
        let mut material_index: HashMap<(usize, bool, bool), usize> = HashMap::new();
        let mut bind_material = |m: &Arc<MapMaterial>, receives: bool, sunlit: bool| -> usize {
            *material_index
                .entry((Arc::as_ptr(m) as usize, receives, sunlit))
                .or_insert_with(|| {
                    let uniform = m.uniform(receives, sunlit);
                    let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("map material"),
                        contents: bytemuck::bytes_of(&uniform),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                    let view = |image: Option<&Arc<MapImage>>| {
                        image
                            .and_then(|i| views.get(&(Arc::as_ptr(i) as usize)))
                            .unwrap_or(white)
                    };
                    let sampler = |image: Option<&Arc<MapImage>>| {
                        if image.is_none_or(|i| i.repeat()) {
                            &pipes.repeat
                        } else {
                            &pipes.clamp
                        }
                    };
                    let surface = m.surface.as_ref();
                    let detail = surface.and_then(|s| s.detail.as_ref());
                    // Albedo, emission, then Standard's maps in the order
                    // of the layout. A map left out reads white, which is
                    // Unity's default for the metallic, occlusion and
                    // detail mask maps; the others are switched off by
                    // their flag.
                    let maps = [
                        (1, m.albedo.as_ref()),
                        (3, m.emission_map.as_ref()),
                        (4, surface.and_then(|s| s.gloss_map.as_ref())),
                        (5, surface.and_then(|s| s.normal_map.as_ref())),
                        (6, surface.and_then(|s| s.occlusion_map.as_ref())),
                        (7, detail.and_then(|d| d.albedo.as_ref())),
                        (8, detail.and_then(|d| d.normal.as_ref())),
                        (9, detail.and_then(|d| d.mask.as_ref())),
                        (10, surface.and_then(|s| s.height.as_ref()).map(|h| &h.0)),
                    ];
                    let mut entries = vec![
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(sampler(m.albedo.as_ref())),
                        },
                        wgpu::BindGroupEntry {
                            binding: 11,
                            resource: wgpu::BindingResource::Sampler(sampler(detail.and_then(|d| d.albedo.as_ref()))),
                        },
                    ];
                    entries.extend(maps.iter().map(|(binding, image)| wgpu::BindGroupEntry {
                        binding: *binding,
                        resource: wgpu::BindingResource::TextureView(view(*image)),
                    }));
                    materials.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("map material"),
                        layout: &pipes.material,
                        entries: &entries,
                    }));
                    materials.len() - 1
                })
        };

        // Every object that can be seen or casts the sun's shadow, whatever
        // the camera: which of them a frame draws, it decides itself.
        let mut buffers: Vec<(wgpu::Buffer, wgpu::Buffer)> = Vec::new();
        let mut objects = Vec::new();
        for o in &map.objects {
            let mesh = &o.mesh;
            if mesh.positions.is_empty() || mesh.indices.is_empty() {
                continue;
            }
            let on_camera = map.seen(o);
            let sunlit = o.layer < 32 && l.sun_mask & (1 << o.layer) != 0;
            let casts = shadowed && sunlit && o.casts_shadows;
            let mut object = ObjectGpu {
                buffers: None,
                bounds: mesh.bounds,
                middle: (Vec3::from(mesh.bounds.0) + Vec3::from(mesh.bounds.1)) * 0.5,
                vanish: o.vanish.clone(),
                on_camera,
                casts,
                lightmap: o.lightmap.filter(|i| *i < map.lightmaps.len()).map_or(0, |i| i + 1),
                reflection: o.probe.filter(|i| *i < map.probes.len()).map_or(0, |i| i + 1),
                parts: Vec::new(),
            };
            if !on_camera && !casts {
                objects.push(object);
                continue;
            }
            let verts: Vec<Vertex> = (0..mesh.positions.len())
                .map(|i| Vertex {
                    pos: mesh.positions[i],
                    normal: mesh.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]),
                    uv: mesh.uv.get(i).copied().unwrap_or([0.0, 0.0]),
                    uv2: mesh.uv2.get(i).copied().unwrap_or([0.0, 0.0]),
                    color: mesh.colors.get(i).copied().unwrap_or([1.0; 4]),
                    tangent: mesh.tangents.get(i).copied().unwrap_or([0.0; 4]),
                    uv1: mesh.uv1.get(i).or(mesh.uv.get(i)).copied().unwrap_or([0.0, 0.0]),
                })
                .collect();
            let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&o.name),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&o.name),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            buffers.push((vb, ib));
            object.buffers = Some(buffers.len() - 1);
            for (s, &(first, count)) in mesh.submeshes.iter().enumerate() {
                // Unity draws extra submeshes with the last material.
                let Some(m) = o.materials.get(s).or(o.materials.last()) else {
                    continue;
                };
                if !m.drawn || count == 0 {
                    continue;
                }
                object.parts.push(Part {
                    first,
                    count,
                    material: bind_material(m, o.receives_shadows && shadowed && sunlit, sunlit),
                    blend: m.blend.kind(),
                    cull: !m.two_sided,
                    queue: m.queue,
                    // Blended surfaces dither their shadow nearly away in
                    // Unity's Standard shader; they cast none here.
                    caster: casts && m.blend.kind() == 0 && !m.particle,
                    cutout: m.blend == Blend::Cutout,
                });
            }
            objects.push(object);
        }
        MapGpu {
            placement: MapPlacement::default(),
            light: l.clone(),
            bounds: map.bounds(),
            vanish: map.vanish.clone(),
            objects,
            buffers,
            materials,
            lightmaps,
            reflections,
            globals_buf,
            shadow_buf,
            cascades,
            depth_view,
            atlas,
            globals_group,
            sky,
        }
    }

    /// The unit vector towards the map's sun, in the world: the sun
    /// turns with the map.
    pub(crate) fn sun_dir(&self) -> [f32; 3] {
        let own = Vec3::from(self.light.sun_dir);
        self.placement
            .rigid()
            .transform_vector3(own)
            .normalize_or_zero()
            .to_array()
    }

    /// What shows where nothing is drawn, linear.
    pub(crate) fn background(&self) -> [f32; 3] {
        self.light.background
    }

    /// How far the camera must see to take in the whole map.
    pub(crate) fn far(&self, camera: &Camera) -> f32 {
        let camera = &self.placement.camera_in_map(camera);
        let (min, max) = self.bounds;
        // Near and far from the map's size: rooms and grounds alike.
        ((max - min).length() * 2.0).max(50.0) + camera.eye.distance((min + max) * 0.5)
    }

    /// Writes the camera's numbers for a frame seen through `view_proj`
    /// and picks what it draws: the walls that hide, the camera's layers,
    /// the view's bounds, blended surfaces sorted far to near.
    pub(crate) fn frame(
        &self,
        queue: &wgpu::Queue,
        pipes: &Pipelines,
        camera: &Camera,
        view_proj: Mat4,
        aspect: f32,
        lamps: &[crate::Lamp],
        fog: Option<Fog>,
    ) -> MapFrame {
        // Everything of the map is worked out in its own space: the
        // camera is brought into it, and its points are put in the world
        // by the view.
        let world_camera = camera;
        let camera = &self.placement.camera_in_map(world_camera);
        let view_proj = view_proj * self.placement.rigid();
        let l = &self.light;
        // The material fog the scene asks for, not the map's own: in
        // CharaStudio the Fog tick box switches it.
        let lit_fog = fog;
        let fog = lit_fog.unwrap_or(Fog {
            color: [0.0; 3],
            start: 0.0,
            end: 1.0,
        });
        let globals = Globals {
            view_proj: view_proj.to_cols_array(),
            eye: [
                camera.eye.x,
                camera.eye.y,
                camera.eye.z,
                l.cut_above.unwrap_or(f32::MAX),
            ],
            sun_dir: [l.sun_dir[0], l.sun_dir[1], l.sun_dir[2], 0.0],
            sun_color: [l.sun_color[0], l.sun_color[1], l.sun_color[2], 0.0],
            sh: l.ambient_sh,
            fog_color: [fog.color[0], fog.color[1], fog.color[2], 1.0],
            fog_range: [fog.start, fog.end, 0.0, if lit_fog.is_some() { 1.0 } else { 0.0 }],
            indirect_spec: [
                l.indirect_specular[0],
                l.indirect_specular[1],
                l.indirect_specular[2],
                0.0,
            ],
            lamps: [[0.0; 4]; 24],
            lamp_info: [0.0; 4],
        };
        // The lamps, brought into the map's space as the camera is.
        let back = self.placement.rigid().inverse();
        let local: Vec<crate::Lamp> = lamps
            .iter()
            .map(|l| crate::Lamp {
                at: l.at.map(|p| back.transform_point3(Vec3::from(p)).to_array()),
                dir: back.transform_vector3(Vec3::from(l.dir)).normalize_or_zero().to_array(),
                ..*l
            })
            .collect();
        let (lamp_rows, lamp_info) = crate::lamp_rows(&local, u32::MAX);
        let globals = Globals {
            lamps: lamp_rows,
            lamp_info,
            ..globals
        };
        queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));
        queue.write_buffer(&self.depth_view.0, 0, bytemuck::cast_slice(&view_proj.to_cols_array()));

        // The sun's shadow: four cascades over slices of the view, in a
        // square atlas, two by two.
        let forward = (camera.target - camera.eye).normalize_or_zero();
        let mut shadow = ShadowUniform::zeroed();
        let mut cascades: Vec<Mat4> = Vec::new();
        if let Some(set) = l.shadows {
            let view = crate::shadow::View {
                eye: camera.eye,
                forward,
                fov_y_deg: camera.fov_y_deg,
                aspect,
            };
            let splits = crate::shadow::ShadowSettings::default().cascade_splits;
            let ranges = crate::shadow::cascade_ranges(0.03, set.distance, splits);
            let light = crate::shadow::light_view(Vec3::from(l.sun_dir));
            for (i, r) in ranges.iter().enumerate() {
                let slice = view.slice_corners(r.0, r.1);
                if let Some((vp, texel)) = fit_cascade(&slice, light, self.bounds, SHADOW_ATLAS / 2) {
                    shadow.to_atlas[i] = crate::shadow::world_to_shadow(vp, i).to_cols_array();
                    shadow.texel_world[i] = texel;
                    cascades.push(vp);
                }
            }
            if cascades.len() == crate::shadow::CASCADES {
                shadow.ends = [ranges[0].1, ranges[1].1, ranges[2].1, ranges[3].1];
                shadow.params = [set.strength, 1.0, 1.0 / SHADOW_ATLAS as f32, 0.0];
                shadow.forward = [forward.x, forward.y, forward.z, 0.0];
            } else {
                cascades.clear();
            }
        }
        queue.write_buffer(&self.shadow_buf, 0, bytemuck::bytes_of(&shadow));
        for (vp, (buf, _)) in cascades.iter().zip(&self.cascades) {
            queue.write_buffer(buf, 0, bytemuck::cast_slice(&vp.to_cols_array()));
        }
        if let (Some(sky), Some((buf, _))) = (&l.sky, &self.sky) {
            // The sky stays with the world, as Unity's skybox does.
            let uniform = crate::sky::uniform(sky, self.sun_dir(), l.sun_color, world_camera, aspect);
            queue.write_buffer(buf, 0, bytemuck::bytes_of(&uniform));
        }

        let mut draws: Vec<Draw> = Vec::new();
        let mut casters: Vec<Caster> = Vec::new();
        // Where each material first came up, in the order of the objects.
        let mut rank = vec![usize::MAX; self.materials.len()];
        let mut ranked = 0;
        let mut culled = 0;
        // The volumes the camera stands in hide what names them.
        let hide = self.placement.hide_walls;
        let inside: Vec<bool> = self.vanish.iter().map(|v| hide && v.contains(camera.eye)).collect();
        for o in &self.objects {
            if o.vanish.iter().any(|v| inside.get(*v).copied().unwrap_or(false)) {
                continue;
            }
            let seen = o.on_camera && in_view(&view_proj, o.bounds);
            if !seen {
                culled += 1;
                if !o.casts {
                    continue;
                }
            }
            let Some(buffers) = o.buffers else { continue };
            let distance = o.middle.distance(camera.eye);
            for p in &o.parts {
                if rank[p.material] == usize::MAX {
                    rank[p.material] = ranked;
                    ranked += 1;
                }
                if p.caster {
                    casters.push(Caster {
                        buffers,
                        first: p.first,
                        count: p.count,
                        material: p.material,
                        cutout: p.cutout,
                        cull: p.cull,
                    });
                }
                if seen {
                    draws.push(Draw {
                        buffers,
                        lightmap: o.lightmap,
                        reflection: o.reflection,
                        first: p.first,
                        count: p.count,
                        material: p.material,
                        rank: rank[p.material],
                        blend: p.blend,
                        cull: p.cull,
                        cutout: p.cutout,
                        distance,
                        queue: p.queue,
                    });
                }
            }
        }
        pipes.culled.set(culled);
        // Opaque and cut-out first, then blended by queue, far to near.
        draws.sort_by(|a, b| {
            (a.blend > 0).cmp(&(b.blend > 0)).then_with(|| {
                if a.blend > 0 {
                    a.queue.cmp(&b.queue).then(b.distance.total_cmp(&a.distance))
                } else {
                    a.rank.cmp(&b.rank)
                }
            })
        });
        MapFrame {
            draws,
            casters,
            shadows: !cascades.is_empty(),
        }
    }

    /// Draws the sun's shadow atlas of `frame`, before the pass that reads
    /// it.
    pub(crate) fn shadow_pass(&self, pipes: &Pipelines, encoder: &mut wgpu::CommandEncoder, frame: &MapFrame) {
        if !frame.shadows {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("map shadow"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.atlas,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let tile = (SHADOW_ATLAS / 2) as f32;
        for (i, (_, group)) in self.cascades.iter().enumerate() {
            let [ox, oy] = crate::shadow::atlas_origin(i);
            let (x, y) = (ox * SHADOW_ATLAS as f32, oy * SHADOW_ATLAS as f32);
            pass.set_viewport(x, y, tile, tile, 0.0, 1.0);
            pass.set_scissor_rect(x as u32, y as u32, tile as u32, tile as u32);
            pass.set_bind_group(0, group, &[]);
            for c in &frame.casters {
                pass.set_pipeline(&pipes.casters[&(c.cutout, c.cull)]);
                pass.set_bind_group(1, &self.materials[c.material], &[]);
                let (v, i) = &self.buffers[c.buffers];
                pass.set_vertex_buffer(0, v.slice(..));
                pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(c.first..c.first + c.count, 0, 0..1);
            }
        }
    }

    /// Draws the opaque and cut-out surfaces of `frame` into the camera's
    /// depth texture, which the screen effects and the characters' shadow
    /// mask read: one camera, one depth, as Unity draws it.
    pub(crate) fn draw_depth(&self, pipes: &Pipelines, pass: &mut wgpu::RenderPass<'_>, frame: &MapFrame) {
        pass.set_bind_group(0, &self.depth_view.1, &[]);
        for d in frame.draws.iter().take_while(|d| d.blend == 0) {
            pass.set_pipeline(&pipes.depth[&(d.cutout, d.cull)]);
            pass.set_bind_group(1, &self.materials[d.material], &[]);
            let (v, i) = &self.buffers[d.buffers];
            pass.set_vertex_buffer(0, v.slice(..));
            pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(d.first..d.first + d.count, 0, 0..1);
        }
    }

    /// Draws the opaque and cut-out surfaces of `frame`, then the sky where
    /// the depth buffer is still open, as Unity orders a frame. Leaves the
    /// map's globals in bind group 0.
    pub(crate) fn draw_solid(
        &self,
        pipes: &Pipelines,
        target: Target,
        pass: &mut wgpu::RenderPass<'_>,
        frame: &MapFrame,
    ) {
        pass.set_bind_group(0, &self.globals_group, &[]);
        let solid = frame.draws.iter().take_while(|d| d.blend == 0);
        self.draw_list(pipes, target, pass, solid);
        if let Some((_, group)) = &self.sky {
            pipes.sky.draw(pass, group, target.color, target.depth);
            pass.set_bind_group(0, &self.globals_group, &[]);
        }
    }

    /// Draws the blended surfaces of `frame`, far to near.
    pub(crate) fn draw_blended(
        &self,
        pipes: &Pipelines,
        target: Target,
        pass: &mut wgpu::RenderPass<'_>,
        frame: &MapFrame,
    ) {
        pass.set_bind_group(0, &self.globals_group, &[]);
        let blended = frame.draws.iter().skip_while(|d| d.blend == 0);
        self.draw_list(pipes, target, pass, blended);
    }

    fn draw_list<'a>(
        &self,
        pipes: &Pipelines,
        target: Target,
        pass: &mut wgpu::RenderPass<'_>,
        draws: impl Iterator<Item = &'a Draw>,
    ) {
        for d in draws {
            pass.set_pipeline(&pipes.pipelines[&(target, d.blend, d.cull)]);
            pass.set_bind_group(1, &self.materials[d.material], &[]);
            pass.set_bind_group(2, &self.lightmaps[d.lightmap], &[]);
            pass.set_bind_group(3, &self.reflections[d.reflection], &[]);
            let (vb, ib) = &self.buffers[d.buffers];
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(d.first..d.first + d.count, 0, 0..1);
        }
    }
}

impl Renderer {
    /// How many objects the last map frame left out as outside the view.
    pub fn map_culled(&self) -> usize {
        self.map.as_ref().map_or(0, |p| p.culled.get())
    }

    /// Whether a map is set to draw with the characters.
    pub fn has_map(&self) -> bool {
        self.map_scene.is_some()
    }

    /// Uploads `map` to the GPU to be drawn behind and around the
    /// characters of every frame from now on, or takes the map away with
    /// `None`. Its buffers, textures and bind groups are made here once;
    /// a frame only writes the camera's numbers and picks what to draw.
    /// While a map is set, the characters are lit from its sun.
    pub fn set_map(&mut self, map: Option<&MapScene>) {
        // The old map's memory goes before the new one's comes; where it
        // stood stays for the next.
        let placement = self.map_scene.take().map(|m| m.placement).unwrap_or_default();
        let Some(map) = map else { return };
        if self.map.is_none() {
            self.map = Some(Pipelines::new(&self.device, &self.queue));
        }
        let l = &map.light;
        // Textures first. Those the last map shares with this one are kept,
        // the rest are let go once this map has its own.
        let pipes = self.map.as_mut().expect("made above");
        let mut old = std::mem::take(&mut pipes.images);
        let mut views: HashMap<usize, wgpu::TextureView> = HashMap::new();
        let images = map
            .objects
            .iter()
            .flat_map(|o| o.materials.iter().flat_map(|m| m.images()))
            .chain(
                map.lightmaps
                    .iter()
                    .flat_map(|l| std::iter::once(&l.light).chain(l.direction.as_ref())),
            )
            .chain(l.sky.iter().flat_map(|s| s.images()));
        for image in images {
            let key = Arc::as_ptr(image) as usize;
            if views.contains_key(&key) {
                continue;
            }
            let entry = old.remove(&key).unwrap_or_else(|| {
                let view = match image.as_ref() {
                    MapImage::Decoded(i) => {
                        let max = self.device.limits().max_texture_dimension_2d;
                        if i.width == 0 || i.height == 0 || i.width > max || i.height > max {
                            self.white.clone()
                        } else {
                            Renderer::upload(&self.device, &self.queue, i.width, i.height, &i.rgba, i.srgb)
                        }
                    }
                    MapImage::Compressed { .. } => {
                        upload_blocks(&self.device, &self.queue, image).unwrap_or_else(|| self.white.clone())
                    }
                };
                (image.clone(), view)
            });
            views.insert(key, entry.1.clone());
            pipes.images.insert(key, entry);
        }
        drop(old);
        let sky_cube = match &l.sky {
            Some(crate::sky::Sky::Cubemap { cube: Some(cube), .. }) => {
                Some(pipes.sky.cube(&self.device, &self.queue, cube))
            }
            _ => None,
        };
        // The reflections' cubemaps: the scene's, then each probe's.
        let cubes: Vec<wgpu::TextureView> = std::iter::once(&l.reflection)
            .chain(map.probes.iter().map(|p| &p.reflection))
            .map(|r| match &r.cube {
                Some(cube) => pipes.sky.cube(&self.device, &self.queue, cube),
                None => pipes.black_cube.clone(),
            })
            .collect();
        // The cubemaps of the last map that this one does not use go.
        let kept: Vec<usize> = match &l.sky {
            Some(crate::sky::Sky::Cubemap { cube: Some(cube), .. }) => Some(cube),
            _ => None,
        }
        .into_iter()
        .chain(
            std::iter::once(&l.reflection)
                .chain(map.probes.iter().map(|p| &p.reflection))
                .filter_map(|r| r.cube.as_ref()),
        )
        .map(|c| Arc::as_ptr(c) as usize)
        .collect();
        pipes.sky.keep_cubes(&kept);
        let mut gpu = MapGpu::new(&self.device, pipes, map, &views, &self.white, sky_cube.as_ref(), &cubes);
        gpu.placement = placement;
        self.map_scene = Some(gpu);
    }

    /// Puts the map that `set_map` uploaded where it stands in the world,
    /// and says how it shows. Cheap: nothing is uploaded again.
    pub fn place_map(&mut self, placement: MapPlacement) {
        if let Some(map) = self.map_scene.as_mut() {
            map.placement = placement;
        }
    }

    /// The map that is set and shown.
    pub(crate) fn shown_map(&self) -> Option<&MapGpu> {
        self.map_scene.as_ref().filter(|m| m.placement.shown)
    }

    /// Draws `map` alone from `camera` to an RGBA8 image, top row first.
    /// The map stays set afterwards, as `set_map` leaves it.
    pub fn render_map(&mut self, map: &MapScene, camera: &Camera, width: u32, height: u32) -> Result<Vec<u8>> {
        self.set_map(Some(map));
        self.draw_map(camera, width, height)
    }

    /// Draws the map `set_map` uploaded, alone, from `camera` to an RGBA8
    /// image, top row first.
    pub fn draw_map(&mut self, camera: &Camera, width: u32, height: u32) -> Result<Vec<u8>> {
        let device = &self.device;
        let pipes = self.map.as_mut().ok_or_else(|| anyhow::anyhow!("no map is set"))?;
        let gpu = self
            .map_scene
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no map is set"))?;
        pipes.prepare(device, Target::OWN);
        let pipes = &*pipes;
        let aspect = width as f32 / height as f32;
        let proj = Mat4::perspective_lh(camera.fov_y_deg.to_radians(), aspect, 0.03, gpu.far(camera));
        let view_proj = proj * Mat4::look_at_lh(camera.eye, camera.target, Vec3::Y);
        let frame = gpu.frame(&self.queue, pipes, camera, view_proj, aspect, &[], gpu.light.fog);

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let make = |format, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("map target"),
                    size,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("map picture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&Default::default());
        let msaa = make(COLOR_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let depth = make(DEPTH, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let mut encoder = device.create_command_encoder(&Default::default());
        gpu.shadow_pass(pipes, &mut encoder, &frame);
        {
            let bg = gpu.background();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("map"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &msaa,
                    depth_slice: None,
                    resolve_target: Some(&target_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0] as f64,
                            g: bg[1] as f64,
                            b: bg[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            gpu.draw_solid(pipes, Target::OWN, &mut pass, &frame);
            gpu.draw_blended(pipes, Target::OWN, &mut pass, &frame);
        }
        // Read back, honouring the row alignment wgpu requires.
        let row = width * 4;
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("map readback"),
            size: (padded * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            size,
        );
        self.queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| drop(tx.send(r)));
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;
        let data = slice.get_mapped_range()?;
        let mut out = Vec::with_capacity((row * height) as usize);
        for y in 0..height {
            let start = (y * padded) as usize;
            out.extend_from_slice(&data[start..start + row as usize]);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid(source: &str) {
        let module = wgpu::naga::front::wgsl::parse_str(source).expect("parses");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validates");
    }

    #[test]
    fn the_map_shaders_are_valid() {
        valid(include_str!("map.wgsl"));
        valid(include_str!("map_shadow.wgsl"));
    }

    #[test]
    fn the_uniforms_are_as_wide_as_the_shader_reads_them() {
        assert_eq!(std::mem::size_of::<MaterialUniform>(), 9 * 16);
        assert_eq!(std::mem::size_of::<ProbeUniform>(), 4 * 16);
        assert_eq!(std::mem::size_of::<Globals>(), 64 + 38 * 16);
        assert_eq!(std::mem::size_of::<Vertex>(), 80);
    }

    fn probe(centre: [f32; 3], half: f32, importance: i32) -> ReflectionProbe {
        ReflectionProbe {
            position: centre,
            centre,
            half: [half; 3],
            box_projection: false,
            importance,
            reflection: Reflection {
                cube: None,
                hdr: [1.0, 1.0, 0.0, 0.0],
            },
        }
    }

    #[test]
    fn an_object_reflects_the_probe_it_touches() {
        let object = ([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        assert_eq!(pick_probe(object, &[]), None);
        // Out of reach on one axis is out of reach.
        assert_eq!(pick_probe(object, &[probe([5.0, 0.5, 0.5], 1.0, 1)]), None);
        assert_eq!(
            pick_probe(
                object,
                &[probe([5.0, 0.5, 0.5], 1.0, 1), probe([1.5, 0.5, 0.5], 1.0, 1)]
            ),
            Some(1)
        );
    }

    #[test]
    fn the_more_important_probe_wins_then_the_one_sharing_more() {
        let object = ([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        // The second holds all of the object, the first half of it.
        let half = probe([-0.5, 0.5, 0.5], 1.0, 1);
        let whole = probe([0.5, 0.5, 0.5], 1.0, 1);
        assert_eq!(pick_probe(object, &[half.clone(), whole.clone()]), Some(1));
        let mut important = half.clone();
        important.importance = 2;
        assert_eq!(pick_probe(object, &[important, whole.clone()]), Some(0));
        // Both hold all of it: the smaller one is the nearer fit.
        let wide = probe([0.5, 0.5, 0.5], 9.0, 1);
        assert_eq!(pick_probe(object, &[wide, whole]), Some(1));
    }

    #[test]
    fn a_probe_hands_its_box_to_the_shader() {
        let mut p = probe([1.0, 2.0, 3.0], 2.0, 1);
        p.position = [1.0, 2.5, 3.0];
        p.box_projection = true;
        p.reflection.hdr = [0.5, 1.0, 0.0, 0.0];
        let u = ProbeUniform::of(&p);
        assert_eq!(u.box_max, [3.0, 4.0, 5.0, 0.0]);
        // All of this probe shows: no second one is blended in.
        assert_eq!(u.box_min, [-1.0, 0.0, 1.0, 1.0]);
        assert_eq!(u.position, [1.0, 2.5, 3.0, 1.0]);
        assert_eq!(u.hdr, [0.5, 1.0, 0.0, 0.0]);
        assert_eq!(ProbeUniform::plain(&p.reflection).position[3], 0.0);
    }

    fn image() -> Option<Arc<MapImage>> {
        Some(Arc::new(MapImage::Decoded(Arc::new(Image {
            name: "t".into(),
            width: 1,
            height: 1,
            rgba: vec![255; 4],
            srgb: false,
            repeat: true,
        }))))
    }

    #[test]
    fn only_standard_materials_are_lit_as_standard() {
        let mut m = MapMaterial::hidden();
        assert_eq!(m.uniform(true, true).flags[0], 0);
        m.surface = Some(Surface::default());
        let u = m.uniform(true, true);
        assert_eq!(u.flags[0], F_STANDARD);
        assert_eq!(u.surface, [0.0, 0.5, 1.0, 1.0]);
        assert_eq!(u.params[3], 3.0);
        // An unlit material stays unlit whatever else it holds.
        m.unlit = true;
        assert_eq!(m.uniform(false, false).flags[0], 0);
    }

    #[test]
    fn a_material_tells_the_shader_what_it_uses() {
        let mut m = MapMaterial::hidden();
        m.blend = Blend::Premultiplied;
        m.surface = Some(Surface {
            metallic: 0.25,
            smoothness: 0.75,
            smoothness_from: SmoothnessFrom::MapAlpha,
            normal_map: image(),
            normal_scale: 2.0,
            occlusion_strength: 0.5,
            detail: Some(Detail {
                albedo: image(),
                normal_scale: 0.5,
                st: [4.0, 4.0, 0.5, 0.5],
                second_uv: true,
                ..Default::default()
            }),
            height: image().map(|i| (i, 0.02)),
            highlights: false,
            reflections: false,
            ..Default::default()
        });
        let u = m.uniform(false, true);
        // The metallic map is read even when it is not set: it is white.
        let all = F_STANDARD
            | F_GLOSS_MAP
            | F_NORMAL_MAP
            | F_DETAIL
            | F_PARALLAX
            | F_NO_HIGHLIGHTS
            | F_NO_REFLECTIONS
            | F_PREMULTIPLY;
        assert_eq!(u.flags[0], all);
        assert_eq!(u.surface, [0.25, 0.75, 2.0, 0.5]);
        assert_eq!(u.detail_st, [4.0, 4.0, 0.5, 0.5]);
        assert_eq!(u.detail, [0.5, 1.0, 0.02, 0.0]);
        // Blended: the alpha is kept.
        assert_eq!(u.params[1], 1.0);
        assert_eq!(m.images().count(), 3);

        let s = m.surface.as_mut().expect("set above");
        s.smoothness_from = SmoothnessFrom::AlbedoAlpha;
        s.specular = Some([0.1, 0.2, 0.3]);
        let u = m.uniform(false, true);
        assert_eq!(u.flags[0], (all & !F_GLOSS_MAP) | F_SMOOTH_ALBEDO | F_SPECULAR_SETUP);
        assert_eq!(u.spec_color, [0.1, 0.2, 0.3, 0.0]);
    }
}
