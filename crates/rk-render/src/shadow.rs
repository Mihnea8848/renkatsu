// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Real-time shadows of the directional light, the way Unity 5.6 builds them
//! for the game: cascaded shadow maps, collected into a screen-sized mask
//! that the surface shaders read.
//!
//! Taken from the game's files: the light's settings (CharaStudio `level1`,
//! "Directional Chara"), the quality settings (`globalgamemanagers`, level
//! "Quality"), the collecting shader (`Hidden/Internal-ScreenSpaceShadows`
//! in `unity_builtin_extra`, see `shadow.wgsl`) and the engine code in
//! `CharaStudio.exe` (Unity 5.6.2f1, read as x86-64 disassembly). The
//! functions that follow engine code name the address they were read at.

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
    /// Far plane of the game's camera. Only decides which renderers the
    /// camera sees.
    pub camera_far: f32,
    /// `QualitySettings.shadowResolution`: 0 low to 3 very high.
    pub resolution: u32,
    /// Largest shadow map Unity makes.
    pub max_resolution: u32,
    /// Whether the light's shadows are soft (`LightShadows.Soft`). Soft
    /// shadows take a larger normal bias; the mask is filtered either way.
    pub soft: bool,
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
            camera_far: 1000.0,
            resolution: 3,
            max_resolution: 4096,
            soft: true,
        }
    }
}

/// Side of the directional light's shadow map for a picture size
/// (`CharaStudio.exe` 0x1402089a0): the longer side of the camera's picture
/// times 3.8, cut to a whole number, rounded up to a power of two and halved
/// once per quality step below "Very High"; at least 32 and at most `max`.
/// Unity's own limit is 4096 (2048 below "Very High" on a graphics card
/// with less than 480 MB) or the largest texture of the device.
pub fn atlas_size(width: u32, height: u32, resolution: u32, max: u32) -> u32 {
    let pixels = (width.max(height) as f32 * 3.8) as u32;
    let side = pixels.max(1).next_power_of_two() >> 3u32.saturating_sub(resolution);
    side.clamp(32, max.max(32))
}

/// View depth where each cascade starts and ends (`_LightSplitsNear` and
/// `_LightSplitsFar`): the splits are fractions of the way from the camera's
/// near plane to the shadow distance (0x140208830 and 0x14020e1e1).
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
/// points from the scene to the light. The engine takes the three axes from
/// the light's transform; here the light has no roll, as a Studio light
/// turned by its two sliders has none.
pub fn light_view(to_light: Vec3) -> Mat4 {
    let rays = (-to_light).try_normalize().unwrap_or(Vec3::NEG_Y);
    let right = Vec3::Y.cross(rays).try_normalize().unwrap_or(Vec3::X);
    Mat4::look_to_lh(Vec3::ZERO, rays, rays.cross(right))
}

/// A box in the world by its lowest and highest corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    /// The box of a set of points; `None` for no points.
    pub fn of(points: impl IntoIterator<Item = Vec3>) -> Option<Self> {
        points.into_iter().fold(None, |b: Option<Aabb>, p| {
            Some(match b {
                Some(b) => Aabb {
                    min: b.min.min(p),
                    max: b.max.max(p),
                },
                None => Aabb { min: p, max: p },
            })
        })
    }

    fn union(self, o: Aabb) -> Aabb {
        Aabb {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }

    /// The largest value of `normal . p + d` over the box.
    fn reach(&self, normal: Vec3, d: f32) -> f32 {
        let center = (self.min + self.max) * 0.5;
        let half = (self.max - self.min) * 0.5;
        normal.dot(center) + d + normal.abs().dot(half)
    }

    /// The six faces as inward planes `(normal, d)`.
    fn planes(&self) -> [(Vec3, f32); 6] {
        [
            (Vec3::X, -self.min.x),
            (Vec3::NEG_X, self.max.x),
            (Vec3::Y, -self.min.y),
            (Vec3::NEG_Y, self.max.y),
            (Vec3::Z, -self.min.z),
            (Vec3::NEG_Z, self.max.z),
        ]
    }

    /// The six faces as corner loops.
    fn faces(&self) -> [[Vec3; 4]; 6] {
        let c = |i: usize| {
            Vec3::new(
                if i & 1 == 0 { self.min.x } else { self.max.x },
                if i & 2 == 0 { self.min.y } else { self.max.y },
                if i & 4 == 0 { self.min.z } else { self.max.z },
            )
        };
        BOX_FACES.map(|f| f.map(c))
    }
}

/// Corner loops of the six faces of a box whose corner index holds one bit
/// per axis; `View::slice_corners` numbers its corners the same way.
const BOX_FACES: [[usize; 4]; 6] = [
    [0, 1, 3, 2],
    [4, 5, 7, 6],
    [0, 2, 6, 4],
    [1, 3, 7, 5],
    [0, 1, 5, 4],
    [2, 3, 7, 6],
];

/// How far Unity grows the receivers' box before fitting (0x14021f1e5).
const RECEIVER_MARGIN: f32 = 0.01;

/// The box Unity fits the cascades to in Close Fit: the culling boxes of
/// the shadow receiving renderers of the solid queues that the camera sees
/// and that reach in front of the shadow distance, grown by a centimetre
/// (the forward render loop, 0x140235765, and 0x14021f1b0). `None` when
/// there is no such renderer; Unity then draws no shadows.
pub fn receiver_bounds(view: &View, set: &ShadowSettings, boxes: impl IntoIterator<Item = Aabb>) -> Option<Aabb> {
    let (right, up) = view.axes();
    let tan = (view.fov_y_deg.to_radians() * 0.5).tan();
    let (f, e) = (view.forward, view.eye);
    let sides = [
        f * (tan * view.aspect) - right,
        f * (tan * view.aspect) + right,
        f * tan - up,
        f * tan + up,
    ];
    let seen = |b: &Aabb| {
        sides.iter().all(|n| b.reach(*n, -n.dot(e)) >= 0.0)
            && b.reach(f, -f.dot(e) - set.camera_near) >= 0.0
            && b.reach(-f, f.dot(e) + set.camera_far) >= 0.0
            // The plane at the shadow distance.
            && b.reach(-f, f.dot(e) + set.distance) >= 0.0
    };
    let all = boxes.into_iter().filter(seen).reduce(Aabb::union)?;
    Some(Aabb {
        min: all.min - RECEIVER_MARGIN,
        max: all.max + RECEIVER_MARGIN,
    })
}

/// Cuts a convex polygon by a plane, keeping the side where
/// `normal . p + d` is not negative.
fn clip(polygon: &[Vec3], (normal, d): (Vec3, f32)) -> Vec<Vec3> {
    let mut out = Vec::with_capacity(polygon.len() + 1);
    for (i, &a) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()];
        let (da, db) = (normal.dot(a) + d, normal.dot(b) + d);
        if da >= 0.0 {
            out.push(a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            out.push(a + (b - a) * (da / (da - db)));
        }
    }
    out
}

/// The points Unity fits a Close Fit cascade around
/// (`CalculateFocusedLightHull`, 0x1402af000): the corners of the part of
/// the view slice that lies inside the receivers' box, and for each corner
/// the point where a ray from it towards the light leaves the box
/// (0x1402ac010). `corners` are numbered as `View::slice_corners` gives
/// them. Empty when the slice misses the box.
pub fn focused_hull(corners: &[Vec3; 8], to_light: Vec3, receivers: &Aabb) -> Vec<Vec3> {
    // The corners of the common part of two convex solids are where the
    // faces of each, cut by the faces of the other, have their corners.
    let slice_faces = BOX_FACES.map(|f| f.map(|i| corners[i]));
    let inside = corners.iter().sum::<Vec3>() / 8.0;
    let slice_planes = slice_faces.map(|f| {
        let n = (f[1] - f[0]).cross(f[2] - f[0]).normalize_or_zero();
        let n = if n.dot(inside - f[0]) < 0.0 { -n } else { n };
        (n, -n.dot(f[0]))
    });
    let mut points = Vec::new();
    let cut = |faces: &[[Vec3; 4]; 6], planes: &[(Vec3, f32); 6], points: &mut Vec<Vec3>| {
        for face in faces {
            let mut polygon = face.to_vec();
            for plane in planes {
                polygon = clip(&polygon, *plane);
            }
            points.extend(polygon);
        }
    };
    cut(&slice_faces, &receivers.planes(), &mut points);
    cut(&receivers.faces(), &slice_planes, &mut points);

    let dir = to_light.normalize_or_zero();
    let mut out = Vec::with_capacity(points.len() * 2);
    for p in points {
        out.push(p);
        // Where the ray leaves the box.
        let mut exit = f32::MAX;
        for axis in 0..3 {
            if dir[axis].abs() > 1e-10 {
                let wall = if dir[axis] > 0.0 {
                    receivers.max[axis]
                } else {
                    receivers.min[axis]
                };
                exit = exit.min((wall - p[axis]) / dir[axis]);
            }
        }
        if (0.0..f32::MAX).contains(&exit) {
            out.push(p + dir * exit);
        }
    }
    out
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

/// Texels Unity adds to the width and height of a cascade's box when there
/// are several cascades.
const BORDER_TEXELS: f32 = 10.0;

/// Fits a cascade's projection around the points of `focused_hull` (Close
/// Fit, 0x14020e87c to 0x14020ec2e): their box in light space, widened by
/// ten texels, with its centre moved onto a whole texel. With `h` half the
/// box's depth, the planes lie `1.1 h` in front of and behind the centre,
/// and the near plane is pulled ten times the near plane offset further
/// towards the light. Casters in front of the near plane are flattened onto
/// it by the caster's vertex shader. `tile` is the cascade's part of the
/// shadow map in texels. `None` for no points.
pub fn fit_cascade(
    points: &[Vec3],
    light: Mat4,
    near_plane_offset: f32,
    tile: [u32; 2],
    several_cascades: bool,
) -> Option<Cascade> {
    let b = Aabb::of(points.iter().map(|p| light.transform_point3(*p)))?;
    let res = [tile[0].max(1) as f32, tile[1].max(1) as f32];
    let mut size = b.max - b.min;
    if several_cascades {
        size.x += size.x / res[0] * BORDER_TEXELS;
        size.y += size.y / res[1] * BORDER_TEXELS;
    }
    let texel = [size.x / res[0], size.y / res[1]];
    let mut center = (b.min + b.max) * 0.5;
    // The engine does this step in double precision, with C's fmod.
    let snap = |at: f32, step: f32| {
        if step > 0.0 {
            (at as f64 - (at as f64 % step as f64)) as f32
        } else {
            at
        }
    };
    center.x = snap(center.x, texel[0]);
    center.y = snap(center.y, texel[1]);
    let half = size * 0.5;
    let origin = center.z - half.z * 1.2;
    let near = origin + half.z * 0.1 - near_plane_offset.max(0.0) * 10.0;
    let far = origin + half.z * 2.3;
    if half.x <= 0.0 || half.y <= 0.0 || far <= near {
        return None;
    }
    let proj = Mat4::orthographic_lh(
        center.x - half.x,
        center.x + half.x,
        center.y - half.y,
        center.y + half.y,
        near,
        far,
    );
    Some(Cascade {
        view_proj: proj * light,
        depth_range: far - near,
        texel,
    })
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
/// (`unity_LightShadowBias.x`). The engine multiplies the light's bias by the
/// cascade projection's depth scale (0x140214938), which makes the bias a
/// world distance along the rays.
pub fn caster_bias(bias: f32, depth_range: f32) -> f32 {
    if depth_range > 0.0 {
        bias / depth_range
    } else {
        0.0
    }
}

/// How far a caster with normal bias moves each vertex inwards where the
/// light grazes it (`unity_LightShadowBias.z`, 0x140214938): the light's
/// normal bias times the larger side of a shadow map texel, and times 2.5
/// when the shadows are soft.
pub fn caster_normal_bias(normal_bias: f32, soft: bool, texel: [f32; 2]) -> f32 {
    normal_bias * if soft { 2.5 } else { 1.0 } * texel[0].max(texel[1])
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
        assert!(s.enabled && s.soft);
        assert_eq!((s.strength, s.bias, s.normal_bias, s.distance), (0.8, 0.01, 0.8, 50.0));
        assert_eq!((s.near_plane_offset, s.camera_near, s.camera_far), (2.0, 0.06, 1000.0));
        assert_eq!((s.resolution, s.max_resolution), (3, 4096));
        assert!(near(shadow_floor(s.strength), 0.2));
        assert_eq!(shadow_floor(3.0), 0.0);
    }

    #[test]
    fn atlas_size_follows_the_picture() {
        // 1400 * 3.8 = 5320, the next power of two is 8192, capped.
        assert_eq!(atlas_size(900, 1400, 3, 4096), 4096);
        // 512 * 3.8 = 1945.6, cut to 1945.
        assert_eq!(atlas_size(512, 512, 3, 4096), 2048);
        // 269 * 3.8 = 1022.2 stays under 1024; 270 * 3.8 = 1026 does not.
        assert_eq!(atlas_size(269, 100, 3, 4096), 1024);
        assert_eq!(atlas_size(100, 270, 3, 4096), 2048);
        // Each quality step below "Very High" halves the side.
        assert_eq!(atlas_size(512, 512, 2, 4096), 1024);
        assert_eq!(atlas_size(512, 512, 0, 4096), 256);
        assert_eq!(atlas_size(0, 0, 3, 4096), 32);
        assert_eq!(atlas_size(64, 64, 0, 4096), 32);
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
        let to_light = TO_LIGHT;
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

    const TO_LIGHT: Vec3 = Vec3::new(0.0, 0.642_787_6, 0.766_044_4);

    fn cube(center: Vec3, half: f32) -> Aabb {
        Aabb {
            min: center - half,
            max: center + half,
        }
    }

    #[test]
    fn receivers_are_what_the_camera_sees() {
        let v = view();
        let set = ShadowSettings::default();
        // The camera at z 1.3 looks down -Z.
        let seen = cube(Vec3::new(0.0, 1.4, 0.0), 0.1);
        let behind = cube(Vec3::new(0.0, 1.4, 3.0), 0.1);
        let beside = cube(Vec3::new(4.0, 1.4, 0.0), 0.1);
        let beyond = cube(Vec3::new(0.0, 1.4, -60.0), 0.1);
        // Mostly outside the picture, but reaching into it.
        let reaching = cube(Vec3::new(1.0, 1.4, 0.0), 0.9);
        let b = receiver_bounds(&v, &set, [seen, behind, beside, beyond, reaching]).unwrap();
        // The seen boxes whole, grown by a centimetre.
        assert!(b.min.abs_diff_eq(Vec3::new(-0.11, 0.49, -0.91), 1e-5), "{b:?}");
        assert!(b.max.abs_diff_eq(Vec3::new(1.91, 2.31, 0.91), 1e-5), "{b:?}");
        assert!(receiver_bounds(&v, &set, [behind, beside, beyond]).is_none());
        assert!(receiver_bounds(&v, &set, []).is_none());
    }

    #[test]
    fn hull_is_the_slice_inside_the_receivers() {
        let v = view();
        let corners = v.slice_corners(0.5, 2.0);
        let inside = |b: &Aabb, p: Vec3| p.cmpge(b.min - 1e-4).all() && p.cmple(b.max + 1e-4).all();

        // A box around the whole slice: its eight corners, and where each
        // one's ray towards the light leaves the box.
        let big = cube(Vec3::new(0.0, 1.4, 0.3), 5.0);
        let hull = focused_hull(&corners, TO_LIGHT, &big);
        for c in corners {
            assert!(hull.iter().any(|p| p.abs_diff_eq(c, 1e-4)), "{c}");
            let exit = hull
                .iter()
                .find(|p| (**p - c).length() > 1e-3 && (**p - c).normalize().abs_diff_eq(TO_LIGHT, 1e-3))
                .expect("a point towards the light");
            // On a wall of the box.
            let on_wall = (0..3).any(|a| near(exit[a], big.min[a]) || near(exit[a], big.max[a]));
            assert!(on_wall && inside(&big, *exit), "{exit}");
        }

        // A small box inside the slice: nothing outside it remains.
        let small = cube(v.eye + v.forward * 1.2, 0.05);
        let hull = focused_hull(&corners, TO_LIGHT, &small);
        assert!(!hull.is_empty());
        assert!(hull.iter().all(|p| inside(&small, *p)));
        let b = Aabb::of(hull.iter().copied()).unwrap();
        assert!(b.min.abs_diff_eq(small.min, 1e-4) && b.max.abs_diff_eq(small.max, 1e-4));

        // A box cut by the slice's far plane ends there.
        let cut = cube(v.eye + v.forward * 2.0, 0.05);
        let hull = focused_hull(&corners, TO_LIGHT, &cut);
        let deepest = hull
            .iter()
            .map(|p| (*p - v.eye).dot(v.forward))
            .fold(f32::MIN, f32::max);
        // The rays towards the light climb back towards the camera here.
        assert!(near(deepest, 2.0), "{deepest}");

        // A box the slice misses gives nothing, and so no cascade.
        let away = cube(v.eye + v.forward * 3.0, 0.05);
        assert!(focused_hull(&corners, TO_LIGHT, &away).is_empty());
        assert!(fit_cascade(&[], light_view(TO_LIGHT), 2.0, [2048, 2048], true).is_none());
    }

    #[test]
    fn cascade_fits_its_points() {
        let light = light_view(TO_LIGHT);
        // A box of points given in light space: 2 wide, 1 high, 4 deep.
        let back = light.inverse();
        let points: Vec<Vec3> = [(-1.0, 3.0, 10.0), (1.0, 4.0, 14.0), (0.0, 3.5, 12.0)]
            .map(|(x, y, z)| back.transform_point3(Vec3::new(x, y, z)))
            .to_vec();
        let c = fit_cascade(&points, light, 2.0, [2048, 1024], true).unwrap();
        // Ten texels wider than the points on each axis.
        assert!(near(c.texel[0], 2.0 * (1.0 + 10.0 / 2048.0) / 2048.0));
        assert!(near(c.texel[1], 1.0 * (1.0 + 10.0 / 1024.0) / 1024.0));
        // With one cascade the box is the points' own.
        let single = fit_cascade(&points, light, 2.0, [2048, 1024], false).unwrap();
        assert!(near(single.texel[0], 2.0 / 2048.0) && near(single.texel[1], 1.0 / 1024.0));
        // Half the depth is 2: the planes lie 2.2 either side of the middle
        // and the near plane another 10 * 2 towards the light.
        assert!(near(c.depth_range, 4.4 + 20.0));
        let depth = |z: f32| {
            c.view_proj
                .project_point3(back.transform_point3(Vec3::new(0.0, 3.5, z)))
                .z
        };
        assert!(near(depth(12.0 - 2.2 - 20.0), 0.0) && near(depth(12.0 + 2.2), 1.0));
        // Every point is inside, away from the border.
        for p in &points {
            let ndc = c.view_proj.project_point3(*p);
            assert!(
                ndc.x.abs() < 1.0 && ndc.y.abs() < 1.0 && ndc.z > 0.0 && ndc.z < 1.0,
                "{ndc}"
            );
        }
        // The middle of the box sits on a whole texel, at most one texel
        // from the middle of the points.
        let mid = light.transform_point3(c.view_proj.inverse().project_point3(Vec3::new(0.0, 0.0, 0.5)));
        for (axis, want) in [(0, 0.0f32), (1, 3.5)] {
            let texels = mid[axis] / c.texel[axis];
            assert!((texels - texels.round()).abs() < 2e-2, "{texels}");
            assert!((mid[axis] - want).abs() <= c.texel[axis] * 1.01);
        }
        // A negative near plane offset counts as none.
        let c = fit_cascade(&points, light, -1.0, [2048, 1024], true).unwrap();
        assert!(near(c.depth_range, 4.4));
        // Depth grows along the rays.
        let a = c.view_proj.project_point3(points[0]).z;
        assert!(c.view_proj.project_point3(points[0] - TO_LIGHT).z > a);
    }

    #[test]
    fn close_fit_follows_the_receivers() {
        // The game's box around a character's hips, seen by the face camera.
        let v = View {
            eye: Vec3::new(0.0, 1.42, 1.91),
            ..view()
        };
        let set = ShadowSettings::default();
        let chara = Aabb {
            min: Vec3::new(-0.9, -0.06, -0.9),
            max: Vec3::new(0.9, 1.74, 0.9),
        };
        let bounds = receiver_bounds(&v, &set, [chara]).unwrap();
        let ranges = cascade_ranges(set.camera_near, set.distance, set.cascade_splits);
        let light = light_view(TO_LIGHT);
        let fit = |r: (f32, f32)| {
            let hull = focused_hull(&v.slice_corners(r.0, r.1), TO_LIGHT, &bounds);
            fit_cascade(&hull, light, set.near_plane_offset, [2048, 2048], true)
        };
        // The character is 1 to 2.8 units away: all in the first cascade.
        let first = fit(ranges[0]).unwrap();
        assert!(ranges[1..].iter().all(|r| fit(*r).is_none()));
        // The box is the part of the view inside the character's box, not
        // the whole slice: well under half a millimetre a texel across.
        assert!(first.texel[0] < 0.0005 && first.texel[1] < 0.001, "{:?}", first.texel);
        // A point of the face lands inside it.
        let ndc = first.view_proj.project_point3(Vec3::new(0.05, 1.45, 0.08));
        assert!(
            ndc.x.abs() < 1.0 && ndc.y.abs() < 1.0 && (0.0..1.0).contains(&ndc.z),
            "{ndc}"
        );
    }

    #[test]
    fn shadow_coordinates_land_in_the_cascade_quarter() {
        let v = view();
        let light = light_view(Vec3::new(0.3, 0.6, 0.7));
        let corners = v.slice_corners(0.06, 3.39);
        let c = fit_cascade(&corners, light, 2.0, [2048, 2048], true).unwrap();
        for i in 0..CASCADES {
            let m = world_to_shadow(c.view_proj, i);
            let [ox, oy] = atlas_origin(i);
            for p in corners {
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
        let c = fit_cascade(
            &v.slice_corners(0.06, 3.39),
            light_view(TO_LIGHT),
            2.0,
            [2048, 2048],
            true,
        )
        .unwrap();
        let p = Vec3::new(0.1, 1.4, 0.0);
        let moved = c.view_proj.project_point3(p - TO_LIGHT * 0.01).z - c.view_proj.project_point3(p).z;
        assert!((moved - caster_bias(0.01, c.depth_range)).abs() < 1e-6);
    }

    #[test]
    fn normal_bias_is_in_texels() {
        // The larger side of a texel; soft shadows take 2.5 times as much.
        assert!(near(caster_normal_bias(0.8, false, [0.001, 0.002]), 0.0016));
        assert!(near(caster_normal_bias(0.8, true, [0.001, 0.002]), 0.004));
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
