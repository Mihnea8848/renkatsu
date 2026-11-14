// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Real-time shadows of the directional light, the way Unity 5.6 builds them
//! for the game: cascaded shadow maps, collected into a screen-sized mask
//! that the surface shaders read.
//!
//! Taken from the game's files: the light's settings (CharaStudio `level1`,
//! "Directional Chara"), the quality settings (`globalgamemanagers`, level
//! "Quality") and the collecting shader (`Hidden/Internal-ScreenSpaceShadows`
//! in `unity_builtin_extra`, see `shadow.wgsl`). How Unity's engine code fits
//! the cascades and scales the bias is not in those files; the functions
//! here follow Unity's documented behaviour and say so.

use glam::{Mat4, Vec3, Vec4};
use rk_chara::{ShadowCasting, ShadowPass};

/// Unity's last render queue for solid surfaces (`Geometry` + 500).
pub const LAST_SOLID_QUEUE: i32 = 2500;

/// Cascades the game's quality levels use.
pub const CASCADES: usize = 4;

/// Shadow settings of the scene. The defaults are CharaStudio's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowSettings {
    /// Whether the light casts shadows (Studio's "shadow" switch).
    pub enabled: bool,
    /// `Light.shadowStrength`.
    pub strength: f32,
    /// `Light.shadowBias`.
    pub bias: f32,
    /// `Light.shadowNormalBias`. Only shadow passes that a shader takes
    /// from its fallback use it; the game's own shaders do not.
    pub normal_bias: f32,
    /// `QualitySettings.shadowDistance`.
    pub distance: f32,
    /// `QualitySettings.shadowCascade4Split`.
    pub cascade_splits: [f32; 3],
    /// `QualitySettings.shadowNearPlaneOffset`.
    pub near_plane_offset: f32,
    /// Near plane of the game's camera, where the first cascade starts.
    pub camera_near: f32,
    /// Largest shadow map: 4096 at shadow resolution "Very High".
    pub max_resolution: u32,
}

impl Default for ShadowSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            strength: 0.8,
            bias: 0.01,
            normal_bias: 0.8,
            distance: 50.0,
            cascade_splits: [0.066_666_67, 0.2, 0.466_666_67],
            near_plane_offset: 2.0,
            camera_near: 0.06,
            max_resolution: 4096,
        }
    }
}

/// Side of the shadow map for a picture size. Unity's manual gives
/// `NextPowerOfTwo(pixel size * 1.9)` for a directional light at "Very High",
/// where the pixel size is the longer side of the screen.
pub fn atlas_size(width: u32, height: u32, max: u32) -> u32 {
    let pixels = width.max(height) as f32 * 1.9;
    (pixels.ceil() as u32).next_power_of_two().clamp(16, max.max(16))
}

/// View depth where each cascade starts and ends (`_LightSplitsNear` and
/// `_LightSplitsFar`): the splits are fractions of the way from the camera's
/// near plane to the shadow distance.
pub fn cascade_ranges(near: f32, distance: f32, splits: [f32; 3]) -> [(f32, f32); CASCADES] {
    let at = |f: f32| near + (distance - near) * f;
    [
        (near, at(splits[0])),
        (at(splits[0]), at(splits[1])),
        (at(splits[1]), at(splits[2])),
        (at(splits[2]), distance),
    ]
}

/// The camera as the cascades need it.
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub eye: Vec3,
    /// Unit vector the camera looks along.
    pub forward: Vec3,
    pub fov_y_deg: f32,
    pub aspect: f32,
}

impl View {
    /// Right and up of the camera, with the world's Y as up.
    fn axes(&self) -> (Vec3, Vec3) {
        let right = Vec3::Y.cross(self.forward).try_normalize().unwrap_or(Vec3::X);
        (right, self.forward.cross(right))
    }

    /// The eight corners of the view between two depths.
    pub fn slice_corners(&self, near: f32, far: f32) -> [Vec3; 8] {
        let (right, up) = self.axes();
        let tan = (self.fov_y_deg.to_radians() * 0.5).tan();
        let mut out = [Vec3::ZERO; 8];
        for (i, corner) in out.iter_mut().enumerate() {
            let depth = if i < 4 { near } else { far };
            let (h, w) = (depth * tan, depth * tan * self.aspect);
            let x = if i & 1 == 0 { -w } else { w };
            let y = if i & 2 == 0 { -h } else { h };
            *corner = self.eye + self.forward * depth + right * x + up * y;
        }
        out
    }
}

/// World to light space: X right, Y up, Z along the light's rays. `to_light`
/// points from the scene to the light. The light has no roll, as a Studio
/// light turned by its two sliders has none.
pub fn light_view(to_light: Vec3) -> Mat4 {
    let rays = (-to_light).try_normalize().unwrap_or(Vec3::NEG_Y);
    let right = Vec3::Y.cross(rays).try_normalize().unwrap_or(Vec3::X);
    Mat4::look_to_lh(Vec3::ZERO, rays, rays.cross(right))
}

/// One cascade of the shadow map.
#[derive(Debug, Clone, Copy)]
pub struct Cascade {
    /// World to the cascade's clip space, depth 0 at the near plane.
    pub view_proj: Mat4,
    /// World distance between the near and far plane.
    pub depth_range: f32,
    /// World size of one shadow map texel, across and up.
    pub texel: [f32; 2],
}

/// Fits a cascade around a slice of the view ("Close Fit" projection): the
/// box of the slice's corners in light space, with the near plane pulled
/// towards the light by the near plane offset. Casters in front of the near
/// plane are flattened onto it by the caster's vertex shader.
pub fn fit_cascade(view: &View, range: (f32, f32), light: Mat4, near_plane_offset: f32, resolution: u32) -> Cascade {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for c in view.slice_corners(range.0, range.1) {
        let p = light.transform_point3(c);
        min = min.min(p);
        max = max.max(p);
    }
    let near = min.z - near_plane_offset;
    let proj = Mat4::orthographic_lh(min.x, max.x, min.y, max.y, near, max.z);
    let res = resolution.max(1) as f32;
    Cascade {
        view_proj: proj * light,
        depth_range: max.z - near,
        texel: [(max.x - min.x) / res, (max.y - min.y) / res],
    }
}

/// Top left corner of a cascade's quarter of the atlas, in 0 to 1.
pub fn atlas_origin(cascade: usize) -> [f32; 2] {
    [(cascade % 2) as f32 * 0.5, (cascade / 2) as f32 * 0.5]
}

/// World to shadow map coordinates of a cascade (`unity_WorldToShadow`): xy
/// is the place in the atlas with y down, z the depth to compare.
pub fn world_to_shadow(cascade_view_proj: Mat4, cascade: usize) -> Mat4 {
    let [ox, oy] = atlas_origin(cascade);
    let to_atlas = Mat4::from_cols(
        Vec4::new(0.25, 0.0, 0.0, 0.0),
        Vec4::new(0.0, -0.25, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 1.0, 0.0),
        Vec4::new(0.25 + ox, 0.25 + oy, 0.0, 1.0),
    );
    to_atlas * cascade_view_proj
}

/// What the caster's vertex shader adds to clip depth
/// (`unity_LightShadowBias.x`). Unity scales the light's bias by the
/// cascade's depth range, which makes the bias a world distance along the
/// rays.
pub fn caster_bias(bias: f32, depth_range: f32) -> f32 {
    if depth_range > 0.0 {
        bias / depth_range
    } else {
        0.0
    }
}

/// How far a caster with normal bias moves each vertex inwards where the
/// light grazes it (`unity_LightShadowBias.z`): the light's normal bias in
/// shadow map texels.
pub fn caster_normal_bias(normal_bias: f32, texel: [f32; 2]) -> f32 {
    normal_bias * texel[0].max(texel[1])
}

/// How one material of a renderer takes part in the shadows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Role {
    /// Drawn into the shadow map; `Some(true)` leaves back faces out.
    pub shadow_map: Option<bool>,
    /// Drawn into the camera's depth texture, which the mask is built from.
    pub depth: Option<bool>,
}

/// Whether a material reads the shadow mask. The mask holds the shadows of
/// what the depth texture holds, the solid queues up to 2500; Unity draws
/// later queues with the shader variants that have no shadows.
pub fn receives(renderer_receives: bool, queue: i32) -> bool {
    renderer_receives && queue <= LAST_SOLID_QUEUE
}

/// Unity draws the shadow map from renderers that cast, and the camera's
/// depth texture from visible renderers in the solid queues (up to 2500),
/// both with the shader's shadow caster pass.
pub fn role(mode: ShadowCasting, pass: ShadowPass, queue: i32) -> Role {
    let cull_back = match pass {
        ShadowPass::None => {
            return Role {
                shadow_map: None,
                depth: None,
            }
        }
        ShadowPass::TwoSided => false,
        ShadowPass::CullBack | ShadowPass::Fallback => true,
    };
    Role {
        shadow_map: match mode {
            ShadowCasting::Off => None,
            ShadowCasting::TwoSided => Some(false),
            ShadowCasting::On | ShadowCasting::ShadowsOnly => Some(cull_back),
        },
        depth: (mode != ShadowCasting::ShadowsOnly && queue <= LAST_SOLID_QUEUE).then_some(cull_back),
    }
}

/// `_LightShadowData.x`: what the mask holds in full shadow.
pub fn shadow_floor(strength: f32) -> f32 {
    1.0 - strength.clamp(0.0, 1.0)
}

/// The 5x5 tent filter of the collecting shader for one axis: weights of its
/// three bilinear taps and where they sit, in texels from
/// `floor(t + 0.5) - 0.5`, where `t` is the sample's place in texels and
/// `frac` the fraction of `t + 0.5`. Mirrors `pcf_axis` in `shadow.wgsl`; the
/// weights of both axes multiply and the sum is divided by 144.
pub fn pcf_axis(frac: f32) -> ([f32; 3], [f32; 3]) {
    let w = [4.0 - 3.0 * frac, 7.0, 1.0 + 3.0 * frac];
    let at = [(3.0 - 2.0 * frac) / w[0] - 2.0, (3.0 + frac) / w[1], frac / w[2] + 2.0];
    (w, at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn defaults_are_the_studio_settings() {
        let s = ShadowSettings::default();
        assert!(s.enabled);
        assert_eq!((s.strength, s.bias, s.normal_bias, s.distance), (0.8, 0.01, 0.8, 50.0));
        assert_eq!(
            (s.near_plane_offset, s.camera_near, s.max_resolution),
            (2.0, 0.06, 4096)
        );
        assert!(near(shadow_floor(s.strength), 0.2));
        assert_eq!(shadow_floor(3.0), 0.0);
    }

    #[test]
    fn atlas_size_follows_the_picture() {
        // 1400 * 1.9 = 2660, the next power of two is 4096.
        assert_eq!(atlas_size(900, 1400, 4096), 4096);
        assert_eq!(atlas_size(512, 512, 4096), 1024);
        assert_eq!(atlas_size(539, 300, 4096), 2048);
        assert_eq!(atlas_size(4000, 4000, 4096), 4096);
        assert_eq!(atlas_size(0, 0, 4096), 16);
    }

    #[test]
    fn cascades_split_the_shadow_distance() {
        let s = ShadowSettings::default();
        let r = cascade_ranges(s.camera_near, s.distance, s.cascade_splits);
        assert!(near(r[0].0, 0.06));
        assert!(near(r[0].1, 0.06 + 49.94 / 15.0));
        assert!(near(r[1].1, 0.06 + 49.94 * 0.2));
        assert!(near(r[2].1, 0.06 + 49.94 * 7.0 / 15.0));
        assert_eq!(r[3].1, 50.0);
        // Each cascade starts where the one before ends.
        for i in 1..CASCADES {
            assert_eq!(r[i].0, r[i - 1].1);
        }
    }

    #[test]
    fn light_space_looks_along_the_rays() {
        // Light above and in front: the rays go down and back.
        let to_light = Vec3::new(0.0, 0.642_787_6, 0.766_044_4);
        let m = light_view(to_light);
        let z = m.transform_vector3(-to_light);
        assert!(near(z.x, 0.0) && near(z.y, 0.0) && near(z.z, 1.0));
        // No roll: the world's up has no sideways part.
        assert!(near(m.transform_vector3(Vec3::Y).x, 0.0));
        // A rotation only.
        assert!(near(m.determinant(), 1.0));
        // A light straight above still has a basis.
        let m = light_view(Vec3::Y);
        assert!(near(m.transform_vector3(Vec3::NEG_Y).z, 1.0));
        assert!(m.is_finite());
    }

    fn view() -> View {
        View {
            eye: Vec3::new(0.0, 1.4, 1.3),
            forward: Vec3::NEG_Z,
            fov_y_deg: 23.0,
            aspect: 900.0 / 1400.0,
        }
    }

    #[test]
    fn slice_corners_lie_on_the_two_planes() {
        let v = view();
        let c = v.slice_corners(0.5, 2.0);
        let tan = (11.5f32).to_radians().tan();
        for (i, p) in c.iter().enumerate() {
            let depth = if i < 4 { 0.5 } else { 2.0 };
            let rel = *p - v.eye;
            assert!(near(rel.dot(v.forward), depth));
            assert!(near(rel.y.abs(), depth * tan));
            assert!(near(rel.x.abs(), depth * tan * v.aspect));
        }
    }

    #[test]
    fn cascade_holds_its_slice() {
        let v = view();
        let light = light_view(Vec3::new(0.0, 0.642_787_6, 0.766_044_4));
        let range = (0.06, 3.39);
        let c = fit_cascade(&v, range, light, 2.0, 2048);
        let mut touched = [false; 4];
        for p in v.slice_corners(range.0, range.1) {
            let ndc = c.view_proj.project_point3(p);
            assert!(ndc.x.abs() <= 1.0001 && ndc.y.abs() <= 1.0001, "{ndc}");
            assert!((0.0..=1.0001).contains(&ndc.z), "{ndc}");
            touched[0] |= near(ndc.x, -1.0);
            touched[1] |= near(ndc.x, 1.0);
            touched[2] |= near(ndc.y, -1.0);
            touched[3] |= near(ndc.y, 1.0);
        }
        // The fit is tight on all four sides.
        assert_eq!(touched, [true; 4]);
        // The near plane sits two units towards the light from the slice.
        let nearest = v
            .slice_corners(range.0, range.1)
            .iter()
            .map(|p| c.view_proj.project_point3(*p).z)
            .fold(f32::MAX, f32::min);
        assert!(near(nearest * c.depth_range, 2.0));
        // Texel size is the box over the resolution.
        let width = 2.0 * 3.39 * (11.5f32).to_radians().tan() * v.aspect;
        assert!(near(c.texel[0], width / 2048.0));
        // Depth grows along the rays.
        let a = c.view_proj.project_point3(Vec3::new(0.0, 1.5, 0.0)).z;
        let b = c.view_proj.project_point3(Vec3::new(0.0, 1.0, -0.6)).z;
        assert!(b > a);
    }

    #[test]
    fn shadow_coordinates_land_in_the_cascade_quarter() {
        let v = view();
        let light = light_view(Vec3::new(0.3, 0.6, 0.7));
        let c = fit_cascade(&v, (0.06, 3.39), light, 2.0, 2048);
        for i in 0..CASCADES {
            let m = world_to_shadow(c.view_proj, i);
            let [ox, oy] = atlas_origin(i);
            for p in v.slice_corners(0.06, 3.39) {
                let s = m.project_point3(p);
                assert!(s.x >= ox - 1e-4 && s.x <= ox + 0.5 + 1e-4, "{s}");
                assert!(s.y >= oy - 1e-4 && s.y <= oy + 0.5 + 1e-4, "{s}");
                // Depth is the cascade's own.
                assert!(near(s.z, c.view_proj.project_point3(p).z));
            }
        }
        assert_eq!(
            [0, 1, 2, 3].map(atlas_origin),
            [[0.0, 0.0], [0.5, 0.0], [0.0, 0.5], [0.5, 0.5]]
        );
        // Up in light space is up in the atlas, where y points down.
        let m = world_to_shadow(c.view_proj, 0);
        let up = light.inverse().transform_vector3(Vec3::Y);
        let p = v.eye + v.forward;
        assert!(m.project_point3(p + up * 0.1).y < m.project_point3(p).y);
    }

    #[test]
    fn bias_is_a_world_distance() {
        // A centimetre in a cascade ten units deep.
        assert!(near(caster_bias(0.01, 10.0), 0.001));
        assert_eq!(caster_bias(0.01, 0.0), 0.0);
        // Moving a point by the bias along the rays changes its clip depth
        // by the same amount.
        let v = view();
        let to_light = Vec3::new(0.0, 0.642_787_6, 0.766_044_4);
        let c = fit_cascade(&v, (0.06, 3.39), light_view(to_light), 2.0, 2048);
        let p = Vec3::new(0.1, 1.4, 0.0);
        let moved = c.view_proj.project_point3(p - to_light * 0.01).z - c.view_proj.project_point3(p).z;
        assert!((moved - caster_bias(0.01, c.depth_range)).abs() < 1e-6);
    }

    #[test]
    fn normal_bias_is_in_texels() {
        assert!(near(caster_normal_bias(0.8, [0.001, 0.002]), 0.0016));
    }

    #[test]
    fn roles_follow_renderer_and_shader() {
        let none = Role {
            shadow_map: None,
            depth: None,
        };
        // No shadow pass: nothing, whatever the renderer says.
        assert_eq!(role(ShadowCasting::On, ShadowPass::None, 2000), none);
        // Skin: casts with both faces and is in the depth texture.
        let skin = role(ShadowCasting::On, ShadowPass::TwoSided, 2350);
        assert_eq!((skin.shadow_map, skin.depth), (Some(false), Some(false)));
        // Front hair draws late: it casts but is not in the depth texture.
        let fringe = role(ShadowCasting::On, ShadowPass::TwoSided, 3040);
        assert_eq!((fringe.shadow_map, fringe.depth), (Some(false), None));
        // A renderer that does not cast still shows in the depth texture.
        let teeth = role(ShadowCasting::Off, ShadowPass::TwoSided, 2450);
        assert_eq!((teeth.shadow_map, teeth.depth), (None, Some(false)));
        // The body's shadow mesh casts through its fallback and is not seen.
        let caster = role(ShadowCasting::ShadowsOnly, ShadowPass::Fallback, 2000);
        assert_eq!((caster.shadow_map, caster.depth), (Some(true), None));
        // Layers drawn after the solid queues never show shadows.
        assert!(receives(true, 2450));
        assert!(!receives(true, 3040));
        assert!(!receives(false, 2450));
        // Two-sided casting overrides the pass in the shadow map only.
        let both = role(ShadowCasting::TwoSided, ShadowPass::CullBack, 2450);
        assert_eq!((both.shadow_map, both.depth), (Some(false), Some(true)));
    }

    #[test]
    fn tent_filter_weights() {
        for frac in [0.0f32, 0.25, 0.5, 0.99] {
            let (w, at) = pcf_axis(frac);
            // 12 per axis, 144 for both: the shader's 1/144.
            assert!(near(w.iter().sum::<f32>(), 12.0));
            // Each tap lies between the two texels it blends.
            assert!((-2.0..=-1.0).contains(&at[0]), "{at:?}");
            assert!((0.0..=1.0).contains(&at[1]), "{at:?}");
            assert!((2.0..=3.0).contains(&at[2]), "{at:?}");
        }
        // With no fraction the filter is the symmetric 5 wide tent
        // 1 3 4 3 1 over 12, here as bilinear pairs.
        let (w, at) = pcf_axis(0.0);
        assert_eq!(w, [4.0, 7.0, 1.0]);
        assert!(near(at[0], -1.25) && near(at[1], 3.0 / 7.0) && near(at[2], 2.0));
    }
}
