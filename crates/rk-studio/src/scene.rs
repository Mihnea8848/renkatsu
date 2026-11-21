// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The scene model: what a scene holds, as plain data. Objects carry a
//! stable id, a name, the folder they sit in, their visibility, their
//! transform and what they are. Nothing here knows about the GPU or about
//! built characters; `live.rs` follows this model each frame.
//!
//! The model is serialisable as it stands, as a flat list of objects in
//! tree order, each naming its parent by id, so a scene file is readable
//! text and a change to one object touches a few lines.
//!
//! An object sits in a folder or is attached to another object, which it
//! then follows. Attached to a character it may also name one of the
//! character's bones. Where that bone is comes from the built character,
//! so `live.rs` hands it in as `Poses`.
//!
//! `Document` wraps a scene with the selection and the undo history. Undo
//! keeps whole snapshots: a scene is a short list of small values, so a
//! snapshot costs little and every operation is undone the same way.

use crate::kinds::Kind;
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Names one object for as long as the scene lives. Ids are never reused,
/// also not after an undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(pub u64);

/// Position, rotation as Unity's euler angles in degrees, and scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
        }
    }
}

impl Transform {
    /// The rotation as a quaternion: Unity applies Z, then X, then Y.
    pub fn quat(&self) -> Quat {
        let [x, y, z] = self.rotation;
        Quat::from_euler(glam::EulerRot::YXZ, y.to_radians(), x.to_radians(), z.to_radians())
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(Vec3::from(self.scale), self.quat(), Vec3::from(self.position))
    }

    /// The transform that makes a matrix, as far as one can: a matrix
    /// that shears has none.
    pub fn from_matrix(matrix: Mat4) -> Transform {
        let (scale, turn, position) = matrix.to_scale_rotation_translation();
        let (y, x, z) = turn.normalize().to_euler(glam::EulerRot::YXZ);
        Transform {
            position: clean(position),
            rotation: clean(Vec3::new(x, y, z) * (180.0 / std::f32::consts::PI)),
            scale: clean(scale),
        }
    }
}

/// Rounds away the noise of going through matrices, so that the numbers
/// in the Properties panel stay clean.
pub fn clean(v: Vec3) -> [f32; 3] {
    v.to_array().map(|x| (x * 1e4).round() / 1e4 + 0.0)
}

/// Where the bones are that objects are attached to: for each attached
/// object, its bone as a matrix in the space of the character's object.
/// It follows the built characters and is no part of what a scene is, so
/// two scenes are the same whatever theirs say.
#[derive(Debug, Clone, Default)]
pub struct Poses(HashMap<ObjectId, Mat4>);

impl Poses {
    pub fn set(&mut self, object: ObjectId, bone: Mat4) {
        self.0.insert(object, bone);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    fn of(&self, object: &Object) -> Mat4 {
        let known = object.bone.as_ref().and_then(|_| self.0.get(&object.id));
        known.copied().unwrap_or(Mat4::IDENTITY)
    }
}

impl PartialEq for Poses {
    fn eq(&self, _: &Poses) -> bool {
        true
    }
}

/// An animation on a character and how it is played. Some animations
/// hold one pose and some move; both are animations here, as in
/// CharaStudio's Anime menu.
///
/// The animation is one of the project's own files when `file` names
/// one. `bundle`, `controller` and `state` say where it came from, a state
/// of an animator controller in a bundle as CharaStudio's list names it,
/// and are what plays when the project has no file for it: scenes saved
/// before projects kept their own animations have only these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    /// The name shown.
    pub name: String,
    /// The animation file, relative to the project folder, with forward
    /// slashes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default)]
    pub bundle: String,
    #[serde(default)]
    pub controller: String,
    #[serde(default)]
    pub state: String,
    /// The controller whose clips replace the base's, for the H scenes'
    /// animations, which share one controller for a whole category.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub override_bundle: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub override_controller: String,
    /// How fast it plays; 1 is the animation's own speed.
    #[serde(default = "one")]
    pub speed: f32,
    /// Where it starts, in passes: 0 the first frame, 0.5 half way.
    #[serde(default)]
    pub start: f32,
    /// An animation that does not loop starts over at its end instead of
    /// holding its last frame (CharaStudio's option of that name).
    #[serde(default)]
    pub force_loop: bool,
    /// The two values CharaStudio's animation options hold while an H
    /// animation plays (`Studio.OICharInfo.animeOptionParam`): the body
    /// blend the clips are played at, in place of the character's own.
    /// None plays at the character's own, as every other animation does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<[f32; 2]>,
}

impl Animation {
    /// The pose the game's character maker starts every character in,
    /// played from what is installed.
    pub fn maker_pose() -> Animation {
        let (bundle, controller, state) = rk_chara::animations::MAKER_POSE;
        Animation {
            name: "Default pose".to_owned(),
            file: None,
            bundle: bundle.to_owned(),
            controller: controller.to_owned(),
            state: state.to_owned(),
            override_bundle: String::new(),
            override_controller: String::new(),
            speed: 1.0,
            start: 0.0,
            force_loop: false,
            options: None,
        }
    }

    /// Whether this plays the maker's pose.
    pub fn is_maker_pose(&self) -> bool {
        let (bundle, controller, state) = rk_chara::animations::MAKER_POSE;
        self.bundle == bundle && self.controller == controller && self.state == state
    }
}

fn one() -> f32 {
    1.0
}

impl Animation {
    /// An entry of the game's list, played as it is from where it is
    /// installed.
    pub fn of(pose: &rk_chara::animations::Entry) -> Animation {
        Animation {
            name: pose.name.clone(),
            file: None,
            bundle: pose.bundle.clone(),
            controller: pose.controller.clone(),
            state: pose.state.clone(),
            override_bundle: pose.override_bundle.clone(),
            override_controller: pose.override_controller.clone(),
            speed: 1.0,
            start: 0.0,
            force_loop: false,
            options: None,
        }
    }

    /// The controller whose clips replace the base's, when it names one.
    pub fn over(&self) -> rk_chara::motion::Override<'_> {
        (!self.override_bundle.is_empty()).then_some((self.override_bundle.as_str(), self.override_controller.as_str()))
    }
}

/// What an object is, with the values only that kind has.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Content {
    Folder,
    Character {
        /// The card the character is built from.
        card: PathBuf,
        /// Which of the card's outfits is worn.
        outfit: usize,
        outdoor_shoes: bool,
        /// Whether hair, skirts and the like move while the scene plays.
        simulate: bool,
        /// The state of each piece of clothing in the game's order (top,
        /// bottom, bra, underwear, gloves, pantyhose, socks, indoor shoes,
        /// outdoor shoes): 0 on, 1 and 2 half off, 3 off.
        #[serde(default)]
        clothes: [u8; 9],
        /// How much liquid lies on the skin, in the game's order: the
        /// face, then the body's front top, front bottom, back top and
        /// back bottom, each 0 to 2.
        #[serde(default)]
        liquid: [u8; rk_chara::siru::PARTS],
        /// The clothes take the body's levels too, as they do in the
        /// game.
        #[serde(default = "yes")]
        liquid_clothes: bool,
        /// The animation the character plays or the pose it holds; none
        /// leaves it as it was built. Scenes of Renkatsu 0.1.4 and older
        /// call it `pose`.
        #[serde(default, alias = "pose", skip_serializing_if = "Option::is_none")]
        animation: Option<Box<Animation>>,
        /// Who the character is, as changed in this scene; none keeps the
        /// card's. The card itself is never written.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<Box<crate::profile::Profile>>,
        /// The light group the character is in; none is the group every
        /// scene has.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        light_group: Option<String>,
    },
    Light {
        /// What the light lights and how it shines. Scenes of Renkatsu
        /// 0.2.1 and older have the character light only.
        #[serde(default)]
        light: LightKind,
        /// The light stays at the camera's upper left, as in the game's
        /// The character light keeps its angle to the view as the camera
        /// moves (CharaStudio's plugin lock), instead of staying fixed in
        /// the scene as CharaStudio's own does.
        from_camera: bool,
        /// The character light's two angles in degrees, CharaStudio's
        /// `charaLight.rot`: the first lowers it from 40 degrees above the
        /// scene's front, the second turns it around.
        #[serde(default)]
        angles: [f32; 2],
        intensity: f32,
        shadows: bool,
        #[serde(default = "white")]
        color: [f32; 3],
        /// How far a point or spot light reaches, in metres.
        #[serde(default = "reach")]
        range: f32,
        /// A spot light's cone, in degrees.
        #[serde(default = "cone")]
        spot_angle: f32,
        /// Whether a map light lights the map, and the characters.
        #[serde(default = "yes")]
        on_map: bool,
        #[serde(default = "yes")]
        on_characters: bool,
        /// The light groups whose characters it lights; none lights
        /// every group.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        groups: Option<Vec<String>>,
        /// A character light grows dim with the map's Light intensity, so
        /// that a dark room darkens its characters too.
        #[serde(default = "yes")]
        with_map: bool,
    },
    /// A post-processing effect on the whole picture.
    Effect {
        effect: Effect,
    },
    Camera {
        focal_length: f32,
        near: f32,
        far: f32,
    },
    Item,
    /// The map the scene stands in. A scene has one at most; its
    /// transform moves and turns the map's root, as CharaStudio's map
    /// position and rotation do.
    Map {
        /// The project's map, by its file relative to the project; empty
        /// for none. Scenes of Renkatsu 0.1.8 and older have none.
        #[serde(default)]
        map: PathBuf,
        #[serde(default)]
        time: MapTime,
        /// The walls between the camera and the scene hide while the
        /// camera is outside them, as in CharaStudio.
        #[serde(default = "yes")]
        hide_walls: bool,
        /// The map's own light, 0 to 1: how much of its baked and ambient
        /// light, reflections and unlit surfaces is kept. Lower makes the
        /// room darker, for lamps to light; the lamps keep their strength.
        #[serde(default = "whole")]
        light_intensity: f32,
    },
}

fn whole() -> f32 {
    1.0
}

/// A map's time of day: its sun, sky and light. A map has some of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapTime {
    #[default]
    Day,
    Evening,
    Night,
}

impl MapTime {
    pub const ALL: [MapTime; 3] = [MapTime::Day, MapTime::Evening, MapTime::Night];

    pub fn label(self) -> &'static str {
        match self {
            MapTime::Day => "Day",
            MapTime::Evening => "Evening",
            MapTime::Night => "Night",
        }
    }

    pub fn time_of_day(self) -> rk_map::unity::TimeOfDay {
        match self {
            MapTime::Day => rk_map::unity::TimeOfDay::Day,
            MapTime::Evening => rk_map::unity::TimeOfDay::Evening,
            MapTime::Night => rk_map::unity::TimeOfDay::Night,
        }
    }
}

impl Content {
    pub fn character(card: PathBuf) -> Content {
        Content::Character {
            card,
            outfit: 0,
            outdoor_shoes: false,
            simulate: true,
            clothes: [0; 9],
            liquid: [0; rk_chara::siru::PARTS],
            liquid_clothes: true,
            animation: None,
            profile: None,
            light_group: None,
        }
    }

    /// The key light a new scene starts with, a little softer than
    /// CharaStudio's 1.0.
    pub fn light() -> Content {
        Content::Light {
            light: LightKind::Character,
            from_camera: false,
            angles: [0.0; 2],
            intensity: 0.85,
            shadows: true,
            color: white(),
            range: reach(),
            spot_angle: cone(),
            on_map: true,
            on_characters: true,
            groups: None,
            with_map: true,
        }
    }

    /// A new light of a kind. Map lights shine on their own, and light
    /// the map and the characters.
    pub fn light_of(light: LightKind) -> Content {
        match Content::light() {
            Content::Light { intensity, .. } if light != LightKind::Character => Content::Light {
                light,
                from_camera: false,
                angles: [0.0; 2],
                intensity: if light == LightKind::Directional {
                    intensity
                } else {
                    1.0
                },
                shadows: light == LightKind::Directional,
                color: white(),
                range: reach(),
                spot_angle: cone(),
                on_map: true,
                on_characters: true,
                groups: None,
                with_map: false,
            },
            other => other,
        }
    }

    pub fn camera() -> Content {
        Content::Camera {
            focal_length: 35.0,
            near: 0.05,
            far: 100.0,
        }
    }

    /// A map of the project, by its file relative to the project, at
    /// the first time of day it has.
    pub fn map(map: PathBuf, time: MapTime) -> Content {
        Content::Map {
            map,
            time,
            hide_walls: true,
            light_intensity: 1.0,
        }
    }

    pub fn kind(&self) -> Kind {
        match self {
            Content::Folder => Kind::Folder,
            Content::Character { .. } => Kind::Character,
            Content::Light { .. } => Kind::Light,
            Content::Camera { .. } => Kind::Camera,
            Content::Item => Kind::Item,
            Content::Map { .. } => Kind::Map,
            Content::Effect { .. } => Kind::Effect,
        }
    }

    /// The icon of the object: its kind's, or its light's or effect's.
    pub fn glyph(&self) -> &'static str {
        match self {
            Content::Light { light, .. } => light.glyph(),
            Content::Effect { effect } => effect.glyph(),
            other => other.kind().glyph(),
        }
    }

    /// What the object is, as its header says.
    pub fn label(&self) -> &'static str {
        match self {
            Content::Light { light, .. } => light.label(),
            Content::Effect { effect } => effect.label(),
            other => other.kind().label(),
        }
    }

    /// What a new object of a kind starts as, for the kinds that need
    /// nothing else to exist. A character needs a card and a map a map.
    pub fn of_kind(kind: Kind) -> Option<Content> {
        match kind {
            Kind::Folder => Some(Content::Folder),
            Kind::Light => Some(Content::light()),
            Kind::Camera => Some(Content::camera()),
            Kind::Item => Some(Content::Item),
            _ => None,
        }
    }
}

fn yes() -> bool {
    true
}

fn white() -> [f32; 3] {
    [1.0; 3]
}

fn reach() -> f32 {
    10.0
}

fn cone() -> f32 {
    30.0
}

/// What a light lights. The character light is CharaStudio's: it lights
/// the characters only. The others are map lights, as a map's own sun
/// and lamps are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightKind {
    #[default]
    Character,
    Directional,
    Point,
    Spot,
}

impl LightKind {
    pub const ALL: [LightKind; 4] = [
        LightKind::Character,
        LightKind::Directional,
        LightKind::Point,
        LightKind::Spot,
    ];

    pub fn glyph(self) -> &'static str {
        use egui_phosphor::regular as icon;
        match self {
            LightKind::Character => icon::USER_FOCUS,
            LightKind::Directional => icon::SUN,
            LightKind::Point => icon::LIGHTBULB,
            LightKind::Spot => icon::FLASHLIGHT,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LightKind::Character => "Character light",
            LightKind::Directional => "Directional light",
            LightKind::Point => "Point light",
            LightKind::Spot => "Spot light",
        }
    }
}

/// The light group every scene has, which every character is in until
/// it is put in another.
pub const EVERYONE: &str = "Everyone";

/// CharaStudio's depth of field as a scene starts (`DOFInfo`).
fn dof_focal_size() -> f32 {
    0.95
}

fn dof_aperture() -> f32 {
    0.6
}

/// The sun shafts' threshold a scene starts with (128 of 255).
fn shaft_threshold() -> [f32; 3] {
    [128.0 / 255.0; 3]
}

/// The top of a new Fog, as CharaStudio's panel starts.
fn fog_height() -> f32 {
    1.0
}

/// CharaStudio's colour filters, in the order of its list (`Filter_00`).
pub const FILTERS: [&str; 17] = [
    "Day",
    "Evening",
    "Night",
    "Live stage",
    "Vivid",
    "Nostalgia",
    "Retro",
    "Flat colour",
    "Four colour",
    "Comic",
    "Mono",
    "Sepia",
    "Reverse mono",
    "Reverse colour",
    "Red accent",
    "Blue accent",
    "Green accent",
];

/// A post-processing effect, with its values. One object holds one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    /// `filter` is one of `FILTERS` by its number; none is the map's own
    /// filter for its time of day, or the first without a map.
    ColorFilter {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<usize>,
        blend: f32,
    },
    Bloom {
        intensity: f32,
        threshold: f32,
        spread: f32,
    },
    AmbientOcclusion {
        intensity: f32,
        radius: f32,
    },
    Vignette {
        intensity: f32,
        blur: f32,
    },
    /// CharaStudio's Fog: its colour (none for the map's own), the
    /// fog's top as a height, and how far from the camera it starts.
    Fog {
        #[serde(default)]
        color: Option<[f32; 3]>,
        #[serde(default = "fog_height")]
        height: f32,
        #[serde(default)]
        start_distance: f32,
    },
    /// CharaStudio's depth of field: how wide the sharp band is around
    /// where the camera turns, 0 to 2, and how strong the blur, 0 to 1.
    DepthOfField {
        #[serde(default = "dof_focal_size")]
        focal_size: f32,
        #[serde(default = "dof_aperture")]
        aperture: f32,
    },
    /// CharaStudio's sun shafts: how bright the sky must be for rays,
    /// and their colour (none for the map's own, with its sun).
    SunShafts {
        #[serde(default = "shaft_threshold")]
        threshold: [f32; 3],
        #[serde(default)]
        color: Option<[f32; 3]>,
    },
}

impl Effect {
    /// Every effect with the values a new one starts with: the
    /// renderer's for the four a scene has always had.
    pub fn all() -> [Effect; 7] {
        let bloom = rk_render::effects::Bloom::default();
        let ao = rk_render::effects::AmbientOcclusion::default();
        let vignette = rk_render::effects::Vignette::default();
        [
            // The map's own: Studio's first filter, none, while the scene
            // has no map.
            Effect::ColorFilter {
                filter: None,
                blend: 0.0,
            },
            Effect::Bloom {
                intensity: bloom.intensity,
                threshold: bloom.threshold,
                spread: bloom.blur_spread,
            },
            Effect::AmbientOcclusion {
                intensity: ao.intensity,
                radius: ao.radius,
            },
            Effect::Vignette {
                intensity: vignette.intensity,
                blur: vignette.blur,
            },
            Effect::Fog {
                color: Some([137.0 / 255.0, 193.0 / 255.0, 221.0 / 255.0]),
                height: fog_height(),
                start_distance: 0.0,
            },
            Effect::DepthOfField {
                focal_size: dof_focal_size(),
                aperture: dof_aperture(),
            },
            Effect::SunShafts {
                threshold: shaft_threshold(),
                color: Some([1.0; 3]),
            },
        ]
    }

    /// Whether the renderer draws the effect yet.
    pub fn drawn(&self) -> bool {
        true
    }

    pub fn glyph(&self) -> &'static str {
        use egui_phosphor::regular as icon;
        match self {
            Effect::ColorFilter { .. } => icon::SWATCHES,
            Effect::Bloom { .. } => icon::SPARKLE,
            Effect::AmbientOcclusion { .. } => icon::CIRCLE_HALF,
            Effect::Vignette { .. } => icon::VIGNETTE,
            Effect::Fog { .. } => icon::CLOUD_FOG,
            Effect::DepthOfField { .. } => icon::APERTURE,
            Effect::SunShafts { .. } => icon::SUN_HORIZON,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Effect::ColorFilter { .. } => "Colour filter",
            Effect::Bloom { .. } => "Bloom",
            Effect::AmbientOcclusion { .. } => "Ambient occlusion",
            Effect::Vignette { .. } => "Vignette",
            Effect::Fog { .. } => "Fog",
            Effect::DepthOfField { .. } => "Depth of field",
            Effect::SunShafts { .. } => "Sun shafts",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub id: ObjectId,
    pub name: String,
    /// The folder the object sits in or the object it is attached to;
    /// none at the top of the scene.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ObjectId>,
    /// The bone of the parent character the object follows, by its name
    /// in the character's skeleton; none follows the parent itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bone: Option<String>,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(flatten)]
    pub content: Content,
    #[serde(default)]
    pub transform: Transform,
}

impl Object {
    pub fn kind(&self) -> Kind {
        self.content.kind()
    }

    /// Unit vector from the scene towards a light with this transform: a
    /// light shines along its own forward axis.
    #[allow(dead_code)] // For map lights, which shine on their own.
    pub fn light_direction(&self) -> [f32; 3] {
        (-(self.transform.quat() * Vec3::Z)).to_array()
    }
}

/// The unit vector towards CharaStudio's character light: its rig turns
/// `Euler(40, 180, 0)` by `Euler(angles[0], angles[1], 0)` in the scene
/// (`CharaStudio_Data/level1`, Light Chara and Directional Chara). With
/// `follow`, the camera's eye and target, the light keeps the angle to
/// the view it has from Studio's first camera place.
pub fn chara_light_direction(angles: [f32; 2], follow: Option<(Vec3, Vec3)>) -> Vec3 {
    let up = 40.0f32.to_radians();
    let turn = Quat::from_euler(glam::EulerRot::YXZ, angles[1].to_radians(), angles[0].to_radians(), 0.0);
    let fixed = turn * Vec3::new(0.0, up.sin(), up.cos());
    let Some((eye, target)) = follow else { return fixed };
    // Studio's Main Camera as the scene starts.
    let first = Quat::from_xyzw(-1.618_124_7e-8, 0.995_055_6, -0.099_319_79, -1.621_151_3e-7).normalize();
    let forward = (target - eye).normalize_or_zero();
    let right = Vec3::Y.cross(forward).normalize_or_zero();
    if forward == Vec3::ZERO || right == Vec3::ZERO {
        return fixed;
    }
    let now = Quat::from_mat3(&glam::Mat3::from_cols(right, forward.cross(right), forward));
    now * first.inverse() * fixed
}

/// The rotation a new light gets, which puts it at the upper left of a
/// character seen from the front.
const LIGHT_ROTATION: [f32; 3] = [20.0, 158.0, 0.0];

/// Where an object goes when it is moved in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Last inside a folder, or last at the top of the scene.
    Into(Option<ObjectId>),
    /// Beside another object, in the same folder, before it.
    Before(ObjectId),
    /// Beside another object, in the same folder, after it.
    After(ObjectId),
}

/// A scene: its name and its objects. Siblings keep the order they have
/// in the list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
    next_id: u64,
    #[serde(default)]
    objects: Vec<Object>,
    /// The light groups characters can be put in, besides the one every
    /// scene has.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub light_groups: Vec<String>,
    #[serde(skip)]
    poses: Poses,
}

impl Scene {
    pub fn empty() -> Scene {
        Scene {
            name: "Untitled scene".to_owned(),
            next_id: 1,
            objects: Vec::new(),
            light_groups: Vec::new(),
            poses: Poses::default(),
        }
    }

    /// What a new scene starts with: the key light, and the effects the
    /// picture has always had.
    pub fn new() -> Scene {
        let mut scene = Scene::empty();
        let lights = scene.kind_folder(Kind::Light);
        scene.add("Key light", Content::light(), lights);
        scene.add_first_effects();
        scene
    }

    /// Whether a light lights a character's light group.
    pub fn light_reaches(&self, light: ObjectId, character: ObjectId) -> bool {
        let group = match self.get(character).map(|o| &o.content) {
            Some(Content::Character { light_group, .. }) => light_group.as_deref().unwrap_or(EVERYONE),
            _ => EVERYONE,
        };
        match self.get(light).map(|o| &o.content) {
            Some(Content::Light { groups, .. }) => groups.as_ref().is_none_or(|list| list.iter().any(|g| g == group)),
            _ => false,
        }
    }

    /// The shown lights that light characters on top of their character
    /// light, or with `map` those that light the map, as the renderer
    /// takes them, in the Outliner's order.
    pub fn lamps(&self, map: bool) -> Vec<(ObjectId, rk_render::Lamp)> {
        let mut out = Vec::new();
        for (_, id) in self.rows() {
            let Some(o) = self.get(id).filter(|o| self.shown(o.id)) else {
                continue;
            };
            let Content::Light {
                light,
                on_characters,
                on_map,
                color,
                intensity,
                range,
                spot_angle,
                ..
            } = &o.content
            else {
                continue;
            };
            if *light == LightKind::Character || !(if map { *on_map } else { *on_characters }) {
                continue;
            }
            let (_, turn, at) = self.world(id).to_scale_rotation_translation();
            // A light shines along its forward axis.
            let shines = (turn * Vec3::Z).normalize_or_zero();
            let (at, dir) = match light {
                LightKind::Directional => (None, -shines),
                _ => (Some(at.to_array()), shines),
            };
            out.push((
                id,
                rk_render::Lamp {
                    at,
                    dir: dir.to_array(),
                    color: *color,
                    intensity: *intensity,
                    range: *range,
                    spot_angle: (*light == LightKind::Spot).then_some(*spot_angle),
                },
            ));
        }
        out
    }

    /// The character light a character takes: of the shown character
    /// lights that light its light group, the brightest, as Unity picks
    /// a main light; none when no light reaches it.
    /// The shown map's Light intensity, or 1 without a map.
    pub fn map_light(&self) -> f32 {
        self.objects
            .iter()
            .find_map(|o| match &o.content {
                Content::Map { light_intensity, .. } if self.shown(o.id) => Some(*light_intensity),
                _ => None,
            })
            .unwrap_or(1.0)
    }

    /// How much of a character light's characters' own shading shows: the
    /// map's Light intensity where the light goes with the map, else all.
    pub fn chara_light_dim(&self, id: ObjectId) -> f32 {
        match self.get(id).map(|o| &o.content) {
            Some(Content::Light { with_map: true, .. }) => self.map_light(),
            _ => 1.0,
        }
    }

    /// How strong a character light shines: its intensity, as dim as the
    /// map makes its characters.
    pub fn chara_light_strength(&self, id: ObjectId) -> f32 {
        match self.get(id).map(|o| &o.content) {
            Some(Content::Light { intensity, .. }) => intensity * self.chara_light_dim(id),
            _ => 0.0,
        }
    }

    pub fn chara_light_of(&self, character: ObjectId) -> Option<ObjectId> {
        let group = match self.get(character).map(|o| &o.content) {
            Some(Content::Character { light_group, .. }) => light_group.as_deref().unwrap_or(EVERYONE),
            _ => EVERYONE,
        };
        let brightness = |o: &Object| match &o.content {
            Content::Light {
                light: LightKind::Character,
                groups,
                color,
                ..
            } if groups.as_ref().is_none_or(|list| list.iter().any(|g| g == group)) => {
                Some(self.chara_light_strength(o.id) * color.iter().copied().fold(0.0, f32::max))
            }
            _ => None,
        };
        let mut best: Option<(ObjectId, f32)> = None;
        for (_, id) in self.rows() {
            let Some(object) = self.get(id).filter(|o| self.shown(o.id)) else {
                continue;
            };
            if let Some(b) = brightness(object) {
                if best.is_none_or(|(_, top)| b > top) {
                    best = Some((id, b));
                }
            }
        }
        best.map(|(id, _)| id)
    }

    /// Puts every character light at CharaStudio's start: fixed in the
    /// scene, at angles of 0.
    pub fn reset_chara_lights(&mut self) {
        for object in &mut self.objects {
            if let Content::Light {
                light: LightKind::Character,
                from_camera,
                angles,
                ..
            } = &mut object.content
            {
                *from_camera = false;
                *angles = [0.0; 2];
            }
        }
    }

    /// Gives a scene with a map the effects that follow it, in the Effects
    /// folder, where it has none of their kind: the colour filter, and the
    /// fog and sun shafts hidden, as a new CharaStudio scene leaves them.
    pub fn add_map_effects(&mut self) {
        let folder = self.kind_folder(Kind::Effect);
        let has = |scene: &Scene, same: &dyn Fn(&Effect) -> bool| {
            scene
                .objects
                .iter()
                .any(|o| matches!(&o.content, Content::Effect { effect } if same(effect)))
        };
        if !has(self, &|e| matches!(e, Effect::ColorFilter { .. })) {
            let effect = Effect::ColorFilter {
                filter: None,
                blend: 0.0,
            };
            self.add(effect.label(), Content::Effect { effect }, folder);
        }
        if !has(self, &|e| matches!(e, Effect::Fog { .. })) {
            let effect = Effect::Fog {
                color: None,
                height: 1.0,
                start_distance: 0.0,
            };
            let id = self.add(effect.label(), Content::Effect { effect }, folder);
            if let Some(o) = self.get_mut(id) {
                o.visible = false;
            }
        }
        if !has(self, &|e| matches!(e, Effect::SunShafts { .. })) {
            let effect = Effect::SunShafts {
                threshold: [128.0 / 255.0; 3],
                color: None,
            };
            let id = self.add(effect.label(), Content::Effect { effect }, folder);
            if let Some(o) = self.get_mut(id) {
                o.visible = false;
            }
        }
    }

    /// Moves the effects scenes of Renkatsu 0.2.1 and older kept under
    /// their map into the Effects folder. Where one moves next to a shown
    /// effect of its kind, the map's won the picture: the other is hidden,
    /// so that the scene looks as before.
    pub fn gather_effects(&mut self) {
        let under_map: Vec<ObjectId> = self
            .objects
            .iter()
            .filter(|o| matches!(o.content, Content::Effect { .. }))
            .filter(|o| {
                let mut at = o.parent;
                while let Some(p) = at.and_then(|p| self.get(p)) {
                    if matches!(p.content, Content::Map { .. }) {
                        return true;
                    }
                    at = p.parent;
                }
                false
            })
            .map(|o| o.id)
            .collect();
        if under_map.is_empty() {
            return;
        }
        let folder = self.kind_folder(Kind::Effect);
        for id in under_map {
            let shown = self.get(id).is_some_and(|o| o.visible);
            let kind = match self.get(id).map(|o| &o.content) {
                Some(Content::Effect { effect }) => std::mem::discriminant(effect),
                _ => continue,
            };
            if shown {
                // An untouched filter of the scene (Studio's first or the
                // map's own, unblended) goes; any other is kept, hidden.
                let untouched = |o: &Object| {
                    matches!(
                        o.content,
                        Content::Effect {
                            effect: Effect::ColorFilter {
                                filter: None | Some(0),
                                blend
                            }
                        } if blend == 0.0
                    )
                };
                let moved_filter = matches!(
                    self.get(id).map(|o| &o.content),
                    Some(Content::Effect {
                        effect: Effect::ColorFilter { .. }
                    })
                );
                if moved_filter {
                    self.objects.retain(|o| o.id == id || !untouched(o));
                }
                for other in &mut self.objects {
                    if other.id != id
                        && other.visible
                        && matches!(&other.content, Content::Effect { effect } if std::mem::discriminant(effect) == kind)
                    {
                        other.visible = false;
                    }
                }
            }
            if let Some(o) = self.get_mut(id) {
                o.parent = folder;
            }
        }
    }

    /// Adds the effects every scene had before they were objects, with
    /// the values the renderer had for them.
    pub fn add_first_effects(&mut self) {
        let folder = self.kind_folder(Kind::Effect);
        for effect in Effect::all().into_iter().take(4) {
            let name = effect.label();
            self.add(name, Content::Effect { effect }, folder);
        }
    }

    /// The top-level folder new objects of a kind go into, made when the
    /// scene has none.
    pub fn kind_folder(&mut self, kind: Kind) -> Option<ObjectId> {
        let name = kind.folder()?;
        let there = self
            .objects
            .iter()
            .find(|o| o.parent.is_none() && o.kind() == Kind::Folder && o.name == name)
            .map(|o| o.id);
        Some(there.unwrap_or_else(|| self.add(name, Content::Folder, None)))
    }

    /// Whether an object is one of the folders objects are sorted into.
    pub fn is_kind_folder(&self, id: ObjectId) -> bool {
        self.get(id).is_some_and(|o| {
            o.parent.is_none()
                && o.kind() == Kind::Folder
                && [
                    Kind::Character,
                    Kind::Item,
                    Kind::Light,
                    Kind::Camera,
                    Kind::Map,
                    Kind::Effect,
                ]
                .iter()
                .any(|k| k.folder() == Some(o.name.as_str()))
        })
    }

    pub fn objects(&self) -> &[Object] {
        &self.objects
    }

    pub fn get(&self, id: ObjectId) -> Option<&Object> {
        self.objects.iter().find(|o| o.id == id)
    }

    /// The matrix that carries an object's own transform into the scene:
    /// those of the objects it sits in, the outermost first, each with
    /// the bone the next one follows.
    pub fn parent_world(&self, id: ObjectId) -> Mat4 {
        self.parent_world_by(id, &self.poses)
    }

    /// `parent_world` with the bones where `poses` has them.
    pub fn parent_world_by(&self, id: ObjectId, poses: &Poses) -> Mat4 {
        let mut matrix = Mat4::IDENTITY;
        let mut child = self.get(id);
        // A tree deeper than the scene has objects is a loop in the file.
        for _ in 0..self.objects.len() {
            let Some(object) = child else { break };
            let Some(parent) = object.parent.and_then(|id| self.get(id)) else {
                break;
            };
            matrix = parent.transform.matrix() * poses.of(object) * matrix;
            child = Some(parent);
        }
        matrix
    }

    /// Where an object is in the scene, what it sits in counted in.
    pub fn world(&self, id: ObjectId) -> Mat4 {
        self.world_by(id, &self.poses)
    }

    /// `world` with the bones where `poses` has them.
    pub fn world_by(&self, id: ObjectId, poses: &Poses) -> Mat4 {
        let own = self.get(id).map_or(Mat4::IDENTITY, |o| o.transform.matrix());
        self.parent_world_by(id, poses) * own
    }

    /// Takes the bones from the built characters.
    pub fn set_poses(&mut self, poses: Poses) {
        self.poses = poses;
    }

    /// How deep an object sits: 0 at the top of the scene.
    pub fn depth(&self, id: ObjectId) -> usize {
        let mut at = self.get(id).and_then(|o| o.parent);
        let mut depth = 0;
        while let Some(parent) = at.filter(|_| depth < self.objects.len()) {
            depth += 1;
            at = self.get(parent).and_then(|o| o.parent);
        }
        depth
    }

    pub fn get_mut(&mut self, id: ObjectId) -> Option<&mut Object> {
        self.objects.iter_mut().find(|o| o.id == id)
    }

    fn index(&self, id: ObjectId) -> Option<usize> {
        self.objects.iter().position(|o| o.id == id)
    }

    /// The objects directly inside a folder, or at the top of the scene.
    pub fn children(&self, parent: Option<ObjectId>) -> Vec<ObjectId> {
        self.objects
            .iter()
            .filter(|o| o.parent == parent)
            .map(|o| o.id)
            .collect()
    }

    pub fn has_children(&self, id: ObjectId) -> bool {
        self.objects.iter().any(|o| o.parent == Some(id))
    }

    /// Every object in tree order with its depth, the top of the scene
    /// being depth 0.
    pub fn rows(&self) -> Vec<(usize, ObjectId)> {
        let mut rows = Vec::with_capacity(self.objects.len());
        self.walk(None, 0, &mut rows);
        rows
    }

    fn walk(&self, parent: Option<ObjectId>, depth: usize, rows: &mut Vec<(usize, ObjectId)>) {
        for id in self.children(parent) {
            rows.push((depth, id));
            self.walk(Some(id), depth + 1, rows);
        }
    }

    /// An object and everything below it, in tree order.
    pub fn subtree(&self, id: ObjectId) -> Vec<ObjectId> {
        let mut rows = vec![(0, id)];
        self.walk(Some(id), 1, &mut rows);
        rows.into_iter().map(|(_, id)| id).collect()
    }

    /// Whether `id` is `of` itself or one of the folders around it.
    pub fn contains(&self, id: ObjectId, of: ObjectId) -> bool {
        let mut at = Some(of);
        while let Some(o) = at {
            if o == id {
                return true;
            }
            at = self.get(o).and_then(|o| o.parent);
        }
        false
    }

    /// Whether an object shows: it and every folder around it are visible.
    pub fn shown(&self, id: ObjectId) -> bool {
        let mut at = self.get(id);
        while let Some(o) = at {
            if !o.visible {
                return false;
            }
            at = o.parent.and_then(|p| self.get(p));
        }
        true
    }

    /// The light that lights the scene: the first one that shows, in tree
    /// order. The renderer has one light.
    pub fn key_light(&self) -> Option<&Object> {
        self.rows().into_iter().filter_map(|(_, id)| self.get(id)).find(|o| {
            matches!(
                o.content,
                Content::Light {
                    light: LightKind::Character,
                    ..
                }
            ) && self.shown(o.id)
        })
    }

    /// A name no other object has: `base`, or `base` with the first free
    /// number after it.
    pub fn free_name(&self, base: &str) -> String {
        let taken = |name: &str| self.objects.iter().any(|o| o.name == name);
        if !taken(base) {
            return base.to_owned();
        }
        // "Chair 2" copies to "Chair 3", not to "Chair 2 2".
        let stem = match base.rsplit_once(' ') {
            Some((stem, number)) if number.parse::<u32>().is_ok() => stem,
            _ => base,
        };
        (2..)
            .map(|n| format!("{stem} {n}"))
            .find(|name| !taken(name))
            .expect("some number is free")
    }

    fn fresh_id(&mut self) -> ObjectId {
        let id = ObjectId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Adds an object last inside `parent`, which must be a folder, or at
    /// the top of the scene. The name is made unique.
    pub fn add(&mut self, name: &str, content: Content, parent: Option<ObjectId>) -> ObjectId {
        let parent = parent.filter(|p| self.get(*p).is_some_and(|o| o.kind() == Kind::Folder));
        let id = self.fresh_id();
        let mut transform = Transform::default();
        if content.kind() == Kind::Light {
            transform.rotation = LIGHT_ROTATION;
        }
        self.objects.push(Object {
            id,
            name: self.free_name(name),
            parent,
            bone: None,
            visible: true,
            content,
            transform,
        });
        id
    }

    /// Removes an object and everything below it. Returns what was removed.
    pub fn delete(&mut self, id: ObjectId) -> Vec<ObjectId> {
        if self.get(id).is_none() {
            return Vec::new();
        }
        let gone = self.subtree(id);
        self.objects.retain(|o| !gone.contains(&o.id));
        gone
    }

    /// Copies an object and everything below it, placing the copy right
    /// after the original. Returns the copy's id.
    pub fn duplicate(&mut self, id: ObjectId) -> Option<ObjectId> {
        let source = self.subtree(id);
        self.get(id)?;
        let mut copies: Vec<Object> = Vec::with_capacity(source.len());
        let mut renamed: Vec<(ObjectId, ObjectId)> = Vec::with_capacity(source.len());
        for old in &source {
            let mut copy = self.get(*old)?.clone();
            copy.id = self.fresh_id();
            renamed.push((*old, copy.id));
            if *old != id {
                // Inside the copy, parents are the copied folders.
                copy.parent = copy
                    .parent
                    .and_then(|p| renamed.iter().find(|(from, _)| *from == p))
                    .map(|(_, to)| *to);
            }
            copies.push(copy);
        }
        for mut copy in copies {
            copy.name = self.free_name(&copy.name);
            self.objects.push(copy);
        }
        let new = renamed[0].1;
        self.place(new, Place::After(id));
        Some(new)
    }

    /// Adds copies of objects that were copied out of a scene: the first
    /// is the head and goes last into `parent`, the rest sit below it as
    /// they did. Returns the head's new id.
    pub fn insert_copies(&mut self, objects: &[Object], parent: Option<ObjectId>) -> Option<ObjectId> {
        let head = objects.first()?.id;
        let parent = parent.filter(|p| self.get(*p).is_some_and(|o| o.kind() == Kind::Folder));
        let mut renamed: Vec<(ObjectId, ObjectId)> = Vec::with_capacity(objects.len());
        for object in objects {
            let mut copy = object.clone();
            copy.id = self.fresh_id();
            copy.parent = if object.id == head {
                parent
            } else {
                // An object whose folder was not copied with it joins the head.
                let found = object.parent.and_then(|p| renamed.iter().find(|(from, _)| *from == p));
                Some(found.map_or(renamed[0].1, |(_, to)| *to))
            };
            renamed.push((object.id, copy.id));
            copy.name = self.free_name(&copy.name);
            self.objects.push(copy);
        }
        Some(renamed[0].1)
    }

    /// Renames the project files the scene names: `to` gives the new path
    /// of a file that moved, relative to the project, or `None` for one
    /// that stays. Returns whether anything changed.
    pub fn retarget(&mut self, to: &dyn Fn(&str) -> Option<String>) -> bool {
        let mut changed = false;
        for object in &mut self.objects {
            if let Content::Character { card, animation, .. } = &mut object.content {
                let old = card.to_string_lossy().replace('\\', "/");
                if let Some(new) = to(&old) {
                    *card = PathBuf::from(new);
                    changed = true;
                }
                if let Some(file) = animation.as_mut().and_then(|a| a.file.as_mut()) {
                    if let Some(new) = to(file) {
                        *file = new;
                        changed = true;
                    }
                }
            }
            if let Content::Map { map, .. } = &mut object.content {
                let old = map.to_string_lossy().replace('\\', "/");
                if let Some(new) = to(&old) {
                    *map = PathBuf::from(new);
                    changed = true;
                }
            }
        }
        changed
    }

    /// How many objects name a project file, or a file below a folder.
    pub fn uses(&self, path: &str) -> usize {
        let below = format!("{path}/");
        let hit = |file: &str| file == path || file.starts_with(&below);
        self.objects
            .iter()
            .filter(|o| match &o.content {
                Content::Character { card, animation, .. } => {
                    hit(&card.to_string_lossy().replace('\\', "/"))
                        || animation.as_ref().and_then(|a| a.file.as_deref()).is_some_and(hit)
                }
                Content::Map { map, .. } => hit(&map.to_string_lossy().replace('\\', "/")),
                _ => false,
            })
            .count()
    }

    /// Whether an object may be moved to a place: the target must exist,
    /// and nothing can go inside itself.
    pub fn can_place(&self, id: ObjectId, place: Place) -> bool {
        if self.get(id).is_none() {
            return false;
        }
        let parent = match place {
            Place::Into(parent) => parent,
            Place::Before(other) | Place::After(other) => {
                if other == id {
                    return false;
                }
                match self.get(other) {
                    Some(o) => o.parent,
                    None => return false,
                }
            }
        };
        match parent {
            None => true,
            Some(p) => self.get(p).is_some() && !self.contains(id, p),
        }
    }

    /// Moves an object, with everything below it, to a place in the tree.
    /// It follows no bone there. Returns whether it moved.
    pub fn place(&mut self, id: ObjectId, place: Place) -> bool {
        if !self.can_place(id, place) {
            return false;
        }
        let (parent, before) = match place {
            Place::Into(parent) => (parent, None),
            Place::Before(other) => (self.get(other).and_then(|o| o.parent), Some(other)),
            Place::After(other) => {
                let parent = self.get(other).and_then(|o| o.parent);
                let siblings = self.children(parent);
                let next = siblings
                    .iter()
                    .skip_while(|s| **s != other)
                    .skip(1)
                    .find(|s| **s != id)
                    .copied();
                (parent, next)
            }
        };
        let Some(from) = self.index(id) else { return false };
        let mut object = self.objects.remove(from);
        object.parent = parent;
        object.bone = None;
        let to = before.and_then(|b| self.index(b)).unwrap_or(self.objects.len());
        self.objects.insert(to, object);
        true
    }
}

/// A scene and a selection as they were at one moment.
#[derive(Debug, Clone, PartialEq)]
struct Snapshot {
    scene: Scene,
    selected: Option<ObjectId>,
}

/// How many steps back the history goes.
const HISTORY: usize = 200;

/// The scene being edited, with the selection and the undo history. Every
/// change goes through here so that it can be undone.
#[derive(Debug, Clone)]
pub struct Document {
    scene: Scene,
    pub selected: Option<ObjectId>,
    /// More objects selected along with `selected`, and the selection they
    /// were added to: they count only while that is still the selection.
    others: Vec<ObjectId>,
    others_of: Option<ObjectId>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// A value is being dragged: its further changes belong to the step
    /// already recorded.
    gesture: bool,
}

impl Document {
    pub fn new(scene: Scene) -> Document {
        Document {
            scene,
            selected: None,
            others: Vec::new(),
            others_of: None,
            undo: Vec::new(),
            redo: Vec::new(),
            gesture: false,
        }
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Everything selected: the selection first, then what was added to
    /// it, without what is gone and without objects inside a folder that
    /// is selected too.
    pub fn selected_all(&self) -> Vec<ObjectId> {
        let Some(first) = self.selected.filter(|id| self.scene.get(*id).is_some()) else {
            return Vec::new();
        };
        let mut all = vec![first];
        if self.others_of == Some(first) {
            for id in &self.others {
                if self.scene.get(*id).is_some() && !all.contains(id) {
                    all.push(*id);
                }
            }
        }
        let inside = |id: ObjectId| all.iter().any(|other| *other != id && self.scene.contains(*other, id));
        all.iter().copied().filter(|id| !inside(*id)).collect()
    }

    /// Whether an object is selected, alone or with others.
    pub fn is_selected(&self, id: ObjectId) -> bool {
        self.selected == Some(id) || (self.others_of == self.selected && self.others.contains(&id))
    }

    /// Selects several objects: the first is the selection the panels
    /// show, the rest go along with it.
    pub fn select_many(&mut self, ids: &[ObjectId]) {
        self.selected = ids.first().copied();
        self.others = ids.iter().skip(1).copied().collect();
        self.others_of = self.selected;
    }

    /// Adds an object to the selection, or takes it out again.
    pub fn toggle_selected(&mut self, id: ObjectId) {
        let mut all: Vec<ObjectId> = match self.selected {
            Some(first) if self.others_of == Some(first) => {
                std::iter::once(first).chain(self.others.iter().copied()).collect()
            }
            Some(first) => vec![first],
            None => Vec::new(),
        };
        match all.iter().position(|other| *other == id) {
            Some(at) => {
                all.remove(at);
            }
            None => all.push(id),
        }
        self.select_many(&all);
    }

    /// Renames the project files the scene names, now and at every step
    /// of the history, so that undoing never brings back a path that is
    /// gone. It is no step of the history itself.
    pub fn retarget(&mut self, to: &dyn Fn(&str) -> Option<String>) -> bool {
        let mut changed = self.scene.retarget(to);
        for snapshot in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            changed |= snapshot.scene.retarget(to);
        }
        changed
    }

    /// An object with everything below it, as copies to paste.
    pub fn copy(&self, id: ObjectId) -> Vec<Object> {
        let ids = if self.scene.get(id).is_some() {
            self.scene.subtree(id)
        } else {
            Vec::new()
        };
        ids.into_iter().filter_map(|id| self.scene.get(id).cloned()).collect()
    }

    /// Pastes copied objects beside the selection and selects the copy.
    pub fn paste(&mut self, objects: &[Object]) -> Option<ObjectId> {
        if objects.is_empty() {
            return None;
        }
        self.checkpoint();
        let id = self.scene.insert_copies(objects, self.target_folder())?;
        self.selected = Some(id);
        if self.scene.get(id).is_some_and(|o| o.kind() == Kind::Character) {
            let position = self.free_spot();
            if let Some(object) = self.scene.get_mut(id) {
                object.transform.position = position;
            }
        }
        Some(id)
    }

    /// Adds an object last inside a folder, or at the top of the scene,
    /// and selects it.
    pub fn add_in(&mut self, name: &str, content: Content, parent: Option<ObjectId>) -> ObjectId {
        self.checkpoint();
        let id = self.scene.add(name, content, parent);
        self.selected = Some(id);
        id
    }

    /// Deletes several objects as one step of the history.
    pub fn delete_many(&mut self, ids: &[ObjectId]) -> bool {
        let ids: Vec<ObjectId> = ids.iter().copied().filter(|id| self.scene.get(*id).is_some()).collect();
        match ids[..] {
            [] => false,
            [one] => self.delete(one),
            _ => {
                self.checkpoint();
                for id in ids {
                    self.scene.delete(id);
                }
                self.selected = None;
                true
            }
        }
    }

    /// Shows or hides several objects as one step of the history.
    pub fn set_visible_many(&mut self, ids: &[ObjectId], visible: bool) -> bool {
        let change: Vec<ObjectId> = ids
            .iter()
            .copied()
            .filter(|id| self.scene.get(*id).is_some_and(|o| o.visible != visible))
            .collect();
        if change.is_empty() {
            return false;
        }
        self.checkpoint();
        for id in change {
            if let Some(object) = self.scene.get_mut(id) {
                object.visible = visible;
            }
        }
        true
    }

    /// Moves several objects to a place in the tree as one step of the
    /// history, in the order given. They stay where they are in the
    /// scene. Returns how many moved.
    pub fn place_many(&mut self, ids: &[ObjectId], place: Place) -> usize {
        let movable: Vec<ObjectId> = ids
            .iter()
            .copied()
            .filter(|id| self.scene.can_place(*id, place))
            .collect();
        if movable.is_empty() {
            return 0;
        }
        self.checkpoint();
        movable
            .iter()
            .filter(|id| self.keeping_place(**id, |scene| scene.place(**id, place)))
            .count()
    }

    /// Changes where an object sits in the tree and leaves it where it is
    /// in the scene: its transform is worked out again for what it sits
    /// in now.
    fn keeping_place(&mut self, id: ObjectId, change: impl FnOnce(&mut Scene) -> bool) -> bool {
        let sits = |scene: &Scene| scene.get(id).map(|o| (o.parent, o.bone.clone()));
        let (before, world) = (sits(&self.scene), self.scene.world(id));
        if !change(&mut self.scene) {
            return false;
        }
        if sits(&self.scene) != before {
            let own = self.scene.parent_world(id).inverse() * world;
            if let Some(object) = self.scene.get_mut(id).filter(|_| own.is_finite()) {
                object.transform = Transform::from_matrix(own);
            }
        }
        true
    }

    /// Attaches objects to another object as one step of the history: to
    /// the object itself, or to a bone of a character, which is at `pose`
    /// in the character's space. They stay where they are in the scene.
    /// Returns how many were attached.
    pub fn attach(&mut self, ids: &[ObjectId], to: ObjectId, bone: Option<(&str, Mat4)>) -> usize {
        let place = Place::Into(Some(to));
        let name = bone.map(|(name, _)| name);
        let new = |scene: &Scene, id: ObjectId| {
            scene
                .get(id)
                .is_some_and(|o| o.parent != Some(to) || o.bone.as_deref() != name)
        };
        let movable: Vec<ObjectId> = ids
            .iter()
            .copied()
            .filter(|id| self.scene.can_place(*id, place) && new(&self.scene, *id))
            .collect();
        if movable.is_empty() {
            return 0;
        }
        self.checkpoint();
        for id in &movable {
            self.keeping_place(*id, |scene| {
                // An object that is there already keeps its place among
                // the others.
                if scene.get(*id).is_some_and(|o| o.parent != Some(to)) {
                    scene.place(*id, place);
                }
                if let Some(object) = scene.get_mut(*id) {
                    object.bone = name.map(str::to_owned);
                }
                if let Some((_, pose)) = bone {
                    scene.poses.set(*id, pose);
                }
                true
            });
        }
        movable.len()
    }

    /// Snaps a character onto another so that an H animation lines the
    /// pair up, and returns whether it did.
    ///
    /// The game gives every character of an H scene the same world
    /// position and rotation (`HSceneProc.SetCharacterPositon`): there is
    /// no per-pair offset and no per-animation placement, because the
    /// relative placement is baked into the clips, which are authored in
    /// one space whose origin is the H point. So a pair is snapped by
    /// putting both characters at one transform. Renkatsu does that
    /// through the scene's own parent chain rather than by copying world
    /// matrices, so the pair then moves, turns and is posed as a unit.
    ///
    /// The H animation options go along: they are the body blend the
    /// clips are played at, so a pair holding different ones drifts
    /// apart. The character takes the one it is snapped onto.
    ///
    /// What the game runs on top of this is partner IK (`MotionIK`, a
    /// full-body biped solver whose hand and foot targets are worked out
    /// against the partner for every animation state). Renkatsu has
    /// none, so two bodies of different sizes line up but do not touch.
    pub fn snap(&mut self, id: ObjectId, to: ObjectId) -> bool {
        let character = |scene: &Scene, id: ObjectId| scene.get(id).is_some_and(|o| o.kind() == Kind::Character);
        let place = Place::Into(Some(to));
        if !character(&self.scene, id) || !character(&self.scene, to) || !self.scene.can_place(id, place) {
            return false;
        }
        let options = match self.scene.get(to).map(|o| &o.content) {
            Some(Content::Character { animation, .. }) => animation.as_ref().and_then(|a| a.options),
            _ => None,
        };
        // A pair that is snapped already is left alone, so that asking
        // for it again is no step of the history.
        let snapped = |o: &Object| {
            o.parent == Some(to)
                && o.bone.is_none()
                && o.transform == Transform::default()
                && match &o.content {
                    Content::Character { animation, .. } => animation.as_ref().and_then(|a| a.options) == options,
                    _ => false,
                }
        };
        if self.scene.get(id).is_some_and(snapped) {
            return false;
        }
        self.checkpoint();
        self.scene.place(id, place);
        let Some(object) = self.scene.get_mut(id) else {
            return false;
        };
        object.transform = Transform::default();
        if let Content::Character {
            animation: Some(animation),
            ..
        } = &mut object.content
        {
            animation.options = options;
        }
        true
    }

    /// Takes the bones from the built characters. No step of the history.
    pub fn set_poses(&mut self, poses: Poses) {
        self.scene.set_poses(poses);
    }

    pub fn selection(&self) -> Option<&Object> {
        self.scene.get(self.selected?)
    }

    /// Records the scene as it is, before a change.
    fn checkpoint(&mut self) {
        self.undo.push(Snapshot {
            scene: self.scene.clone(),
            selected: self.selected,
        });
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.gesture = false;
    }

    /// Forgets the history: the scene as it is becomes where undo stops.
    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.gesture = false;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn restore(&mut self, snapshot: Snapshot) -> Snapshot {
        let now = Snapshot {
            scene: std::mem::replace(&mut self.scene, snapshot.scene),
            selected: self.selected,
        };
        // The bones are where they are now, not where they were then.
        self.scene.poses = now.scene.poses.clone();
        // Ids handed out since the snapshot stay used.
        self.scene.next_id = self.scene.next_id.max(now.scene.next_id);
        self.selected = snapshot.selected;
        self.gesture = false;
        now
    }

    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else { return false };
        let now = self.restore(snapshot);
        self.redo.push(now);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop() else { return false };
        let now = self.restore(snapshot);
        self.undo.push(now);
        true
    }

    /// The folder a new object goes into: the selected folder, or the
    /// folder the selection sits in.
    fn target_folder(&self) -> Option<ObjectId> {
        let selected = self.selection()?;
        if selected.kind() == Kind::Folder {
            Some(selected.id)
        } else {
            selected.parent
        }
    }

    /// Moves an object in the same step of the history as the one before.
    pub fn move_now(&mut self, id: ObjectId, position: [f32; 3]) {
        if let Some(object) = self.scene.get_mut(id) {
            object.transform.position = position;
        }
    }

    /// Gives the scene the effects that follow its map, in the same step of
    /// the history as the one before.
    pub fn add_map_effects(&mut self) {
        self.scene.add_map_effects();
    }

    /// Adds an object and selects it. A folder goes beside the selection;
    /// anything else into the selected folder, or else into its kind's
    /// folder.
    pub fn add(&mut self, name: &str, content: Content) -> ObjectId {
        self.checkpoint();
        let kind = content.kind();
        let parent = match self.selection() {
            _ if kind == Kind::Folder => self.target_folder(),
            Some(s) if s.kind() == Kind::Folder && !self.scene.is_kind_folder(s.id) => Some(s.id),
            _ => self.scene.kind_folder(kind),
        };
        let id = self.scene.add(name, content, parent);
        self.selected = Some(id);
        id
    }

    /// Adds a character built from a card, standing clear of the others,
    /// and selects it.
    pub fn add_character(&mut self, name: &str, card: PathBuf) -> ObjectId {
        let id = self.add(name, Content::character(card));
        let position = self.free_spot();
        if let Some(object) = self.scene.get_mut(id) {
            object.transform.position = position;
        }
        id
    }

    /// A place on the ground no character stands on: the origin, or the
    /// first free step to its side.
    fn free_spot(&self) -> [f32; 3] {
        const STEP: f32 = 1.0;
        let taken = |x: f32| {
            self.scene.objects.iter().any(|o| {
                let [ox, _, oz] = o.transform.position;
                o.kind() == Kind::Character && Some(o.id) != self.selected && (ox - x).abs() < 0.3 && oz.abs() < 0.3
            })
        };
        // 0, one step right, one step left, two steps right, and so on.
        (0..)
            .map(|n: i32| match n {
                0 => 0.0,
                n if n % 2 == 1 => ((n + 1) / 2) as f32 * STEP,
                n => -(n / 2) as f32 * STEP,
            })
            .find(|x| !taken(*x))
            .map_or([0.0; 3], |x| [x, 0.0, 0.0])
    }

    /// Deletes an object with everything below it. The selection moves to
    /// the next sibling, else the previous one, else the folder around it.
    pub fn delete(&mut self, id: ObjectId) -> bool {
        let Some(object) = self.scene.get(id) else { return false };
        let parent = object.parent;
        let siblings = self.scene.children(parent);
        let at = siblings.iter().position(|s| *s == id).unwrap_or(0);
        let neighbour = siblings
            .get(at + 1)
            .or_else(|| at.checked_sub(1).and_then(|i| siblings.get(i)))
            .copied()
            .or(parent);
        self.checkpoint();
        let gone = self.scene.delete(id);
        if self.selected.is_some_and(|s| gone.contains(&s)) {
            self.selected = neighbour;
        }
        true
    }

    /// Copies an object with everything below it and selects the copy.
    pub fn duplicate(&mut self, id: ObjectId) -> Option<ObjectId> {
        self.scene.get(id)?;
        self.checkpoint();
        let copy = self.scene.duplicate(id)?;
        self.selected = Some(copy);
        if self.scene.get(copy).is_some_and(|o| o.kind() == Kind::Character) {
            let position = self.free_spot();
            if let Some(object) = self.scene.get_mut(copy) {
                object.transform.position = position;
            }
        }
        Some(copy)
    }

    /// Moves an object in the tree.
    #[cfg(test)]
    pub fn place(&mut self, id: ObjectId, place: Place) -> bool {
        if !self.scene.can_place(id, place) {
            return false;
        }
        self.checkpoint();
        self.scene.place(id, place)
    }

    /// Renames an object. An empty name is refused.
    pub fn rename(&mut self, id: ObjectId, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || self.scene.get(id).is_none_or(|o| o.name == name) {
            return false;
        }
        self.checkpoint();
        if let Some(object) = self.scene.get_mut(id) {
            object.name = name.to_owned();
        }
        true
    }

    /// Adds a light group with the first free name, and returns it.
    pub fn add_light_group(&mut self) -> String {
        self.checkpoint();
        let groups = &mut self.scene.light_groups;
        let name = (2..)
            .map(|n| format!("Group {n}"))
            .find(|name| !groups.contains(name))
            .unwrap_or_default();
        groups.push(name.clone());
        name
    }

    /// Shows or hides an object.
    pub fn set_visible(&mut self, id: ObjectId, visible: bool) -> bool {
        if self.scene.get(id).is_none_or(|o| o.visible == visible) {
            return false;
        }
        self.checkpoint();
        if let Some(object) = self.scene.get_mut(id) {
            object.visible = visible;
        }
        true
    }

    /// Replaces an object's values with edited ones. `held` says the
    /// pointer is down, as while a number is dragged: the changes of one
    /// drag make one step of the history.
    pub fn edit(&mut self, edited: Object, held: bool) -> bool {
        let Some(at) = self.scene.index(edited.id) else {
            return false;
        };
        if self.scene.objects[at] == edited {
            return false;
        }
        if !self.gesture {
            self.checkpoint();
        }
        self.gesture = held;
        // The tree is not edited this way.
        let (parent, bone) = (self.scene.objects[at].parent, self.scene.objects[at].bone.clone());
        self.scene.objects[at] = Object { parent, bone, ..edited };
        true
    }

    /// The pointer was let go: the next edit starts a new step.
    pub fn end_gesture(&mut self) {
        self.gesture = false;
    }

    /// A drag was called off: the scene goes back to how it was before
    /// the drag, and the drag leaves no step in the history.
    pub fn cancel_gesture(&mut self) {
        if self.gesture {
            if let Some(snapshot) = self.undo.pop() {
                self.restore(snapshot);
            }
        }
        self.gesture = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_character_takes_the_brightest_light_of_its_group_or_none() {
        let mut scene = Scene::empty();
        let key = scene.add("Key light", Content::light(), None);
        let anna = scene.add("Anna", Content::character("anna.png".into()), None);
        let bea = scene.add("Bea", Content::character("bea.png".into()), None);
        let set =
            |scene: &mut Scene, id: ObjectId, f: &dyn Fn(&mut Content)| f(&mut scene.get_mut(id).unwrap().content);
        set(&mut scene, bea, &|c| {
            if let Content::Character { light_group, .. } = c {
                *light_group = Some("Shade".into());
            }
        });
        // Every group by default.
        assert_eq!(
            (scene.chara_light_of(anna), scene.chara_light_of(bea)),
            (Some(key), Some(key))
        );
        // The key light for Everyone alone: Bea has none.
        set(&mut scene, key, &|c| {
            if let Content::Light { groups, .. } = c {
                *groups = Some(vec![EVERYONE.into()]);
            }
        });
        assert_eq!(
            (scene.chara_light_of(anna), scene.chara_light_of(bea)),
            (Some(key), None)
        );
        // A light of her own; the brighter wins where two reach.
        let shade = scene.add("Shade light", Content::light(), None);
        set(&mut scene, shade, &|c| {
            if let Content::Light { groups, intensity, .. } = c {
                *groups = Some(vec!["Shade".into(), EVERYONE.into()]);
                *intensity = 0.4;
            }
        });
        assert_eq!(
            (scene.chara_light_of(anna), scene.chara_light_of(bea)),
            (Some(key), Some(shade))
        );
        scene.get_mut(shade).unwrap().visible = false;
        assert_eq!(scene.chara_light_of(bea), None);
    }

    #[test]
    fn the_character_light_is_charastudios_rig() {
        let near = |a: Vec3, b: Vec3| a.distance(b) < 1e-4;
        let up = 40.0f32.to_radians();
        // 40 degrees above the scene's front, as Studio starts.
        assert!(near(
            chara_light_direction([0.0; 2], None),
            Vec3::new(0.0, up.sin(), up.cos())
        ));
        // The first angle lowers it, the second turns it around.
        assert!(near(chara_light_direction([40.0, 0.0], None), Vec3::Z));
        assert!(near(
            chara_light_direction([0.0, 90.0], None),
            Vec3::new(up.cos(), up.sin(), 0.0)
        ));
        // Following Studio's first camera, it is where the fixed one is.
        let view = crate::live::Live::studio_view();
        let follow = chara_light_direction([0.0; 2], Some((view.eye, view.target)));
        assert!(near(follow, chara_light_direction([0.0; 2], None)), "{follow}");
        // From behind, it comes from behind the camera too.
        let behind = chara_light_direction([0.0; 2], Some((Vec3::new(0.0, 1.0, -4.0), Vec3::new(0.0, 1.0, 0.0))));
        assert!(behind.z < -0.5, "{behind}");
    }

    fn effects_in(scene: &Scene) -> Vec<(String, Option<String>, bool)> {
        scene
            .objects()
            .iter()
            .filter(|o| matches!(o.content, Content::Effect { .. }))
            .map(|o| {
                let parent = o.parent.and_then(|p| scene.get(p)).map(|p| p.name.clone());
                (o.name.clone(), parent, o.visible)
            })
            .collect()
    }

    #[test]
    fn a_map_brings_its_effects_into_the_effects_folder() {
        let mut scene = Scene::new();
        scene.add(
            "1F Hallway",
            Content::map(PathBuf::from("Maps/1F Hallway.rkmap"), MapTime::Day),
            None,
        );
        scene.add_map_effects();
        let effects = effects_in(&scene);
        assert!(
            effects
                .iter()
                .all(|(_, parent, _)| parent.as_deref() == Some("Effects")),
            "{effects:?}"
        );
        // One colour filter, the map's own; fog and shafts hidden.
        let filters: Vec<_> = scene
            .objects()
            .iter()
            .filter_map(|o| match o.content {
                Content::Effect {
                    effect: Effect::ColorFilter { filter, .. },
                } => Some(filter),
                _ => None,
            })
            .collect();
        assert_eq!(filters, vec![None]);
        assert!(effects.contains(&("Fog".to_owned(), Some("Effects".to_owned()), false)));
        assert!(effects.contains(&("Sun shafts".to_owned(), Some("Effects".to_owned()), false)));
        // A second time adds nothing.
        let count = effects.len();
        scene.add_map_effects();
        assert_eq!(effects_in(&scene).len(), count);
    }

    #[test]
    fn older_map_effects_move_to_the_effects_folder_and_keep_the_picture() {
        let mut scene = Scene::new();
        let map = scene.add(
            "1F Hallway",
            Content::map(PathBuf::from("Maps/1F Hallway.rkmap"), MapTime::Day),
            None,
        );
        let own = Effect::ColorFilter {
            filter: Some(3),
            blend: 0.5,
        };
        let own = scene.add("Colour filter", Content::Effect { effect: own }, None);
        scene.get_mut(own).unwrap().parent = Some(map);
        let fog = Effect::Fog {
            color: None,
            height: 1.0,
            start_distance: 0.0,
        };
        let fog = scene.add("Fog", Content::Effect { effect: fog }, None);
        scene.get_mut(fog).unwrap().parent = Some(map);
        scene.get_mut(fog).unwrap().visible = false;
        scene.gather_effects();
        let effects = effects_in(&scene);
        assert!(
            effects
                .iter()
                .all(|(_, parent, _)| parent.as_deref() == Some("Effects")),
            "{effects:?}"
        );
        // The map's shown filter won before: it stays shown, the scene's
        // own is hidden.
        let filters: Vec<_> = scene
            .objects()
            .iter()
            .filter_map(|o| match o.content {
                Content::Effect {
                    effect: Effect::ColorFilter { filter, .. },
                } => Some((filter, o.visible)),
                _ => None,
            })
            .collect();
        assert_eq!(filters, vec![(Some(3), true)]);
        // A filter someone changed is kept, hidden.
        let mut scene = Scene::new();
        if let Some(Content::Effect {
            effect: Effect::ColorFilter { filter, .. },
        }) = scene.objects.iter_mut().map(|o| &mut o.content).find(|c| {
            matches!(
                c,
                Content::Effect {
                    effect: Effect::ColorFilter { .. }
                }
            )
        }) {
            *filter = Some(5);
        }
        let map = scene.add(
            "1F Hallway",
            Content::map(PathBuf::from("Maps/1F Hallway.rkmap"), MapTime::Day),
            None,
        );
        let own = Effect::ColorFilter {
            filter: None,
            blend: 0.0,
        };
        let own = scene.add("Colour filter", Content::Effect { effect: own }, None);
        scene.get_mut(own).unwrap().parent = Some(map);
        scene.gather_effects();
        let filters: Vec<_> = scene
            .objects()
            .iter()
            .filter_map(|o| match o.content {
                Content::Effect {
                    effect: Effect::ColorFilter { filter, .. },
                } => Some((filter, o.visible)),
                _ => None,
            })
            .collect();
        assert_eq!(filters, vec![(Some(5), false), (None, true)]);
        assert!(effects.contains(&("Fog".to_owned(), Some("Effects".to_owned()), false)));
    }

    #[test]
    fn character_lights_dim_with_the_map_unless_unlinked() {
        let mut scene = Scene::new();
        let key = scene.key_light().unwrap().id;
        assert_eq!(scene.chara_light_strength(key), 0.85);
        let map = scene.add(
            "Hallway",
            Content::map(PathBuf::from("Maps/a.rkmap"), MapTime::Night),
            None,
        );
        if let Content::Map { light_intensity, .. } = &mut scene.get_mut(map).unwrap().content {
            *light_intensity = 0.2;
        }
        assert!((scene.chara_light_strength(key) - 0.17).abs() < 1e-6);
        if let Content::Light { with_map, .. } = &mut scene.get_mut(key).unwrap().content {
            *with_map = false;
        }
        assert_eq!(scene.chara_light_strength(key), 0.85);
        // A lamp is the light of a dark room: it does not follow.
        assert!(matches!(
            Content::light_of(LightKind::Point),
            Content::Light { with_map: false, .. }
        ));
    }

    #[test]
    fn older_character_lights_become_charastudios() {
        let mut scene = Scene::empty();
        let id = scene.add("Key light", Content::light(), None);
        if let Content::Light {
            from_camera, angles, ..
        } = &mut scene.get_mut(id).unwrap().content
        {
            *from_camera = true;
            *angles = [10.0, 20.0];
        }
        scene.reset_chara_lights();
        assert!(matches!(
            scene.get(id).unwrap().content,
            Content::Light {
                from_camera: false,
                angles: [0.0, 0.0],
                ..
            }
        ));
    }

    fn card(name: &str) -> PathBuf {
        PathBuf::from(format!("/cards/{name}.png"))
    }

    fn names(scene: &Scene) -> Vec<(usize, &str)> {
        scene
            .rows()
            .into_iter()
            .map(|(depth, id)| (depth, scene.get(id).unwrap().name.as_str()))
            .collect()
    }

    /// Cast (folder) holding Anna and Bea, then a light and a camera.
    fn cast() -> (Document, [ObjectId; 5]) {
        let mut doc = Document::new(Scene::empty());
        let folder = doc.add("Cast", Content::Folder);
        let anna = doc.add_character("Anna", card("anna"));
        doc.selected = Some(folder);
        let bea = doc.add_character("Bea", card("bea"));
        let light = doc.add_in("Key light", Content::light(), None);
        let camera = doc.add_in("Camera", Content::camera(), None);
        (doc, [folder, anna, bea, light, camera])
    }

    /// Gives a character an H animation holding the given options.
    fn with_options(doc: &mut Document, id: ObjectId, options: Option<[f32; 2]>) {
        let mut edited = doc.scene().get(id).unwrap().clone();
        if let Content::Character { animation, .. } = &mut edited.content {
            *animation = Some(Box::new(Animation {
                name: "Missionary".into(),
                options,
                ..Animation::maker_pose()
            }));
        }
        doc.edit(edited, false);
    }

    #[test]
    fn snapping_puts_a_pair_at_one_transform_through_the_parent_chain() {
        let (mut doc, [_, anna, bea, ..]) = cast();
        // The two stand apart, as new characters do.
        assert_ne!(
            doc.scene().get(anna).unwrap().transform.position,
            doc.scene().get(bea).unwrap().transform.position
        );
        assert!(doc.snap(bea, anna));
        let snapped = doc.scene().get(bea).unwrap();
        assert_eq!(snapped.parent, Some(anna), "it follows the one it is snapped onto");
        assert_eq!(snapped.bone, None, "the whole character, not a bone");
        assert_eq!(snapped.transform, Transform::default(), "at that one's origin");
        assert_eq!(doc.scene().world(bea), doc.scene().world(anna), "one transform");
        // Moving the pair moves both, because it goes through the chain.
        let mut moved = doc.scene().get(anna).unwrap().clone();
        moved.transform.position = [2.0, 0.0, 1.0];
        doc.edit(moved, false);
        assert_eq!(doc.scene().world(bea), doc.scene().world(anna));
    }

    #[test]
    fn snapping_takes_the_other_character_s_animation_options_and_is_undone() {
        let (mut doc, [_, anna, bea, ..]) = cast();
        with_options(&mut doc, anna, Some([0.25, 0.75]));
        with_options(&mut doc, bea, Some([1.0, 1.0]));
        let was = doc.scene().get(bea).unwrap().clone();
        assert!(doc.snap(bea, anna));
        let Content::Character { animation, .. } = &doc.scene().get(bea).unwrap().content else {
            panic!("a character");
        };
        assert_eq!(animation.as_ref().unwrap().options, Some([0.25, 0.75]));
        // Asking again changes nothing and leaves no step of the history.
        assert!(!doc.snap(bea, anna));
        assert!(doc.undo());
        assert_eq!(doc.scene().get(bea).unwrap(), &was);
    }

    #[test]
    fn a_character_is_snapped_only_onto_another_character_outside_itself() {
        let (mut doc, [folder, anna, bea, light, _]) = cast();
        assert!(!doc.snap(anna, anna), "not onto itself");
        assert!(!doc.snap(anna, light), "not onto a light");
        assert!(!doc.snap(light, anna), "a light is not snapped");
        assert!(!doc.snap(anna, folder), "a folder is not a character");
        assert!(doc.snap(bea, anna));
        assert!(!doc.snap(anna, bea), "not inside what already follows it");
    }

    #[test]
    fn objects_get_ids_that_are_never_reused() {
        let (mut doc, ids) = cast();
        let mut sorted = ids.to_vec();
        sorted.dedup();
        assert_eq!(sorted.len(), 5, "ids differ");
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "ids grow");

        // An id handed out before an undo is not handed out again.
        doc.selected = None;
        let chair = doc.add("Chair", Content::Item);
        assert!(doc.undo());
        assert!(doc.scene().get(chair).is_none());
        let desk = doc.add("Desk", Content::Item);
        assert_ne!(desk, chair);
        assert!(desk > chair);
    }

    #[test]
    fn a_new_object_goes_into_its_kinds_folder() {
        let rows = |doc: &Document| -> Vec<(usize, String)> {
            let scene = doc.scene();
            scene
                .rows()
                .into_iter()
                .map(|(depth, id)| (depth, scene.get(id).unwrap().name.clone()))
                .collect()
        };
        let row = |depth: usize, name: &str| (depth, name.to_owned());
        let mut doc = Document::new(Scene::new());
        assert_eq!(
            rows(&doc),
            [
                row(0, "Lights"),
                row(1, "Key light"),
                row(0, "Effects"),
                row(1, "Colour filter"),
                row(1, "Bloom"),
                row(1, "Ambient occlusion"),
                row(1, "Vignette"),
            ]
        );
        // Beside another light, wherever the selection is.
        doc.add("Lamp", Content::light_of(LightKind::Point));
        // A folder of the user's that is selected takes it; another
        // kind's folder does not.
        doc.selected = None;
        let props = doc.add("Props", Content::Folder);
        doc.add("Cup", Content::Item);
        let lights = doc.scene().objects()[0].id;
        doc.selected = Some(lights);
        doc.add("Desk", Content::Item);
        assert_eq!(rows(&doc)[..3], [row(0, "Lights"), row(1, "Key light"), row(1, "Lamp")]);
        assert_eq!(
            rows(&doc)[8..],
            [row(0, "Props"), row(1, "Cup"), row(0, "Items"), row(1, "Desk")]
        );
        assert!(doc.scene().is_kind_folder(lights) && !doc.scene().is_kind_folder(props));
    }

    #[test]
    fn adding_goes_into_the_selected_folder_and_selects() {
        let (doc, [folder, anna, bea, light, camera]) = cast();
        assert_eq!(
            names(doc.scene()),
            [(0, "Cast"), (1, "Anna"), (1, "Bea"), (0, "Key light"), (0, "Camera")]
        );
        assert_eq!(doc.scene().get(anna).unwrap().parent, Some(folder));
        assert_eq!(doc.scene().get(bea).unwrap().parent, Some(folder));
        assert_eq!(doc.scene().get(light).unwrap().parent, None);
        assert_eq!(doc.selected, Some(camera));
        assert_eq!(doc.scene().children(Some(folder)), [anna, bea]);
        assert!(doc.scene().has_children(folder));
        assert!(!doc.scene().has_children(anna));
    }

    #[test]
    fn new_objects_go_into_folders_only() {
        let mut scene = Scene::empty();
        let light = scene.add("Light", Content::light(), None);
        let item = scene.add("Chair", Content::Item, Some(light));
        assert_eq!(scene.get(item).unwrap().parent, None);
        assert!(!scene.place(item, Place::Into(Some(ObjectId(99)))));
        // Attached it can be to anything but itself and what it holds.
        assert!(scene.can_place(item, Place::Into(Some(light))));
        assert!(scene.place(light, Place::Into(Some(item))));
        assert!(!scene.can_place(item, Place::Into(Some(light))));
        assert!(!scene.can_place(item, Place::Into(Some(item))));
    }

    /// A character that is turned and moved, with a light and an item
    /// beside it.
    fn stage() -> (Document, [ObjectId; 3]) {
        let mut doc = Document::new(Scene::empty());
        let anna = doc.add_character("Anna", card("anna"));
        let light = doc.add_in("Light", Content::light(), None);
        let cup = doc.add_in("Cup", Content::Item, None);
        let mut turned = doc.scene().get(anna).unwrap().clone();
        turned.transform = Transform {
            position: [1.0, 0.0, 2.0],
            rotation: [0.0, 90.0, 0.0],
            scale: [1.0; 3],
        };
        doc.edit(turned, false);
        let mut held = doc.scene().get(cup).unwrap().clone();
        held.transform.position = [0.3, 1.1, -0.4];
        held.transform.rotation = [10.0, 20.0, 30.0];
        doc.edit(held, false);
        doc.clear_history();
        (doc, [anna, light, cup])
    }

    fn near(a: Mat4, b: Mat4) -> bool {
        a.abs_diff_eq(b, 2e-3)
    }

    /// A bone that stands off the character's origin, turned and tilted.
    fn hand(at: f32) -> Mat4 {
        Mat4::from_rotation_translation(
            Quat::from_euler(glam::EulerRot::YXZ, at, 0.4, -0.2),
            Vec3::new(0.4, 1.0 + at, 0.1),
        )
    }

    #[test]
    fn attaching_and_detaching_keep_the_place_in_the_scene() {
        let (mut doc, [anna, light, cup]) = stage();
        let (cup_before, light_before) = (doc.scene().world(cup), doc.scene().world(light));
        // To the character itself, as one step for both.
        assert_eq!(doc.attach(&[cup, light], anna, None), 2);
        assert_eq!(doc.scene().get(cup).unwrap().parent, Some(anna));
        assert_ne!(
            doc.scene().get(cup).unwrap().transform.position,
            [0.3, 1.1, -0.4],
            "the transform is the character's now"
        );
        assert!(near(doc.scene().world(cup), cup_before));
        assert!(near(doc.scene().world(light), light_before));
        // On to a bone of it.
        assert_eq!(doc.attach(&[cup], anna, Some(("cf_j_hand_L", hand(0.0)))), 1);
        assert_eq!(doc.scene().get(cup).unwrap().bone.as_deref(), Some("cf_j_hand_L"));
        assert!(near(doc.scene().world(cup), cup_before));
        // The same again changes nothing and leaves no step.
        assert_eq!(doc.attach(&[cup], anna, Some(("cf_j_hand_L", hand(0.0)))), 0);
        // Off again, to the top of the scene.
        assert_eq!(doc.place_many(&[cup], Place::Into(None)), 1);
        let free = doc.scene().get(cup).unwrap();
        assert_eq!((free.parent, free.bone.clone()), (None, None));
        assert!(near(doc.scene().world(cup), cup_before));
        // Each was one step.
        assert!(doc.undo() && doc.undo() && doc.undo() && !doc.undo());
        assert_eq!(doc.scene().get(light).unwrap().parent, None);
        assert!(near(doc.scene().world(cup), cup_before));
    }

    #[test]
    fn an_attached_object_follows_its_bone() {
        let (mut doc, [anna, _, cup]) = stage();
        doc.attach(&[cup], anna, Some(("cf_j_hand_L", hand(0.0))));
        let own = doc.scene().get(cup).unwrap().transform.matrix();
        // The animation moves the hand on.
        let mut poses = Poses::default();
        poses.set(cup, hand(0.5));
        doc.set_poses(poses);
        let character = doc.scene().world(anna);
        assert!(near(doc.scene().world(cup), character * hand(0.5) * own));
        // It also goes along when the character moves as a whole.
        let mut moved = doc.scene().get(anna).unwrap().clone();
        moved.transform.position = [-2.0, 0.0, 0.5];
        doc.edit(moved, false);
        let character = doc.scene().world(anna);
        assert!(near(doc.scene().world(cup), character * hand(0.5) * own));
        // An undo leaves the bone where it is now.
        doc.undo();
        let character = doc.scene().world(anna);
        assert!(near(doc.scene().world(cup), character * hand(0.5) * own));
        // A bone nobody knows, as of a character that is not built, is at
        // the character's origin.
        doc.set_poses(Poses::default());
        assert!(near(doc.scene().world(cup), character * own));
    }

    #[test]
    fn the_scene_file_names_the_bone() {
        let (mut doc, [anna, light, cup]) = stage();
        doc.attach(&[cup], anna, Some(("cf_j_hand_L", hand(0.0))));
        doc.attach(&[light], anna, None);
        let text = toml::to_string(doc.scene()).unwrap();
        assert!(text.contains("bone = \"cf_j_hand_L\""), "{text}");
        assert_eq!(text.matches("bone = ").count(), 1, "{text}");
        let back: Scene = toml::from_str(&text).unwrap();
        assert_eq!(&back, doc.scene());
        let read = back.get(cup).unwrap();
        assert_eq!((read.parent, read.bone.as_deref()), (Some(anna), Some("cf_j_hand_L")));
        assert_eq!(back.get(light).unwrap().bone, None);
    }

    #[test]
    fn names_stay_unique() {
        let mut scene = Scene::empty();
        scene.add("Chair", Content::Item, None);
        let second = scene.add("Chair", Content::Item, None);
        assert_eq!(scene.get(second).unwrap().name, "Chair 2");
        let third = scene.duplicate(second).unwrap();
        assert_eq!(scene.get(third).unwrap().name, "Chair 3");
    }

    #[test]
    fn new_characters_stand_clear_of_each_other() {
        let (mut doc, [_, anna, bea, ..]) = cast();
        let x = |doc: &Document, id| doc.scene().get(id).unwrap().transform.position[0];
        assert_eq!(x(&doc, anna), 0.0);
        assert!(x(&doc, bea).abs() > 0.5);
        let cleo = doc.add_character("Cleo", card("cleo"));
        assert!((x(&doc, cleo) - x(&doc, bea)).abs() > 0.5);
        assert!(x(&doc, cleo).abs() > 0.5);
    }

    #[test]
    fn deleting_a_folder_takes_what_it_holds() {
        let (mut doc, [folder, anna, bea, light, _]) = cast();
        doc.selected = Some(anna);
        assert!(doc.delete(folder));
        assert!(doc.scene().get(anna).is_none());
        assert!(doc.scene().get(bea).is_none());
        assert_eq!(names(doc.scene()), [(0, "Key light"), (0, "Camera")]);
        // The selection was inside: it moves to the folder's next sibling.
        assert_eq!(doc.selected, Some(light));
        assert!(!doc.delete(folder), "already gone");
    }

    #[test]
    fn the_selection_after_a_delete_is_a_neighbour() {
        let (mut doc, [folder, anna, bea, light, camera]) = cast();
        doc.selected = Some(anna);
        doc.delete(anna);
        assert_eq!(doc.selected, Some(bea), "the next sibling");
        doc.delete(bea);
        assert_eq!(doc.selected, Some(folder), "the folder, once it is empty");
        doc.selected = Some(camera);
        doc.delete(camera);
        assert_eq!(doc.selected, Some(light), "the previous sibling at the end");
        // Deleting something else leaves the selection alone.
        doc.delete(folder);
        assert_eq!(doc.selected, Some(light));
        doc.delete(light);
        assert_eq!(doc.selected, None);
        assert!(doc.scene().objects().is_empty());
    }

    #[test]
    fn objects_move_between_folders_and_places() {
        let (mut doc, [folder, anna, bea, light, camera]) = cast();
        assert!(doc.place(bea, Place::Into(None)));
        assert_eq!(
            names(doc.scene()),
            [(0, "Cast"), (1, "Anna"), (0, "Key light"), (0, "Camera"), (0, "Bea")]
        );
        assert!(doc.place(bea, Place::Before(light)));
        assert_eq!(doc.scene().children(None), [folder, bea, light, camera]);
        assert!(doc.place(camera, Place::After(anna)));
        assert_eq!(doc.scene().children(Some(folder)), [anna, camera]);
        assert!(doc.place(camera, Place::Before(anna)));
        assert_eq!(doc.scene().children(Some(folder)), [camera, anna]);
        assert!(doc.place(light, Place::After(folder)));
        assert_eq!(doc.scene().children(None), [folder, light, bea]);
        // After the last sibling.
        assert!(doc.place(folder, Place::After(bea)));
        assert_eq!(doc.scene().children(None), [light, bea, folder]);
        assert_eq!(
            doc.scene().children(Some(folder)),
            [camera, anna],
            "the folder kept its content"
        );
    }

    #[test]
    fn a_folder_cannot_go_inside_itself() {
        let (mut doc, [folder, anna, ..]) = cast();
        doc.selected = Some(folder);
        let inner = doc.add("Inner", Content::Folder);
        assert_eq!(doc.scene().get(inner).unwrap().parent, Some(folder));
        let before = doc.scene().clone();
        assert!(!doc.place(folder, Place::Into(Some(inner))));
        assert!(!doc.place(folder, Place::Into(Some(folder))));
        assert!(!doc.place(folder, Place::Before(anna)));
        assert!(!doc.place(anna, Place::Before(anna)));
        assert_eq!(doc.scene(), &before);
        assert!(doc.scene().contains(folder, inner));
        assert!(!doc.scene().contains(inner, folder));
    }

    #[test]
    fn a_copy_gets_new_ids_and_sits_after_the_original() {
        let (mut doc, [folder, anna, bea, ..]) = cast();
        let copy = doc.duplicate(folder).unwrap();
        assert_eq!(doc.selected, Some(copy));
        assert_eq!(
            names(doc.scene()),
            [
                (0, "Cast"),
                (1, "Anna"),
                (1, "Bea"),
                (0, "Cast 2"),
                (1, "Anna 2"),
                (1, "Bea 2"),
                (0, "Key light"),
                (0, "Camera"),
            ]
        );
        let inside = doc.scene().children(Some(copy));
        assert_eq!(inside.len(), 2);
        assert!(inside.iter().all(|id| *id != anna && *id != bea));
        let content = |id| doc.scene().get(id).unwrap().content.clone();
        assert_eq!(content(inside[0]), content(anna));
        assert!(doc.duplicate(ObjectId(99)).is_none());
    }

    #[test]
    fn every_operation_is_undone_and_redone() {
        let (mut doc, [folder, anna, bea, light, _]) = cast();
        let start = doc.scene().clone();
        doc.selected = Some(anna);

        doc.delete(folder);
        doc.rename(light, "Sun");
        doc.set_visible(light, false);
        let chair = doc.add("Chair", Content::Item);
        doc.duplicate(chair);
        doc.place(chair, Place::Into(None));
        let end = doc.scene().clone();
        assert_ne!(end, start);

        let mut steps = 0;
        while doc.scene().objects() != start.objects() {
            assert!(doc.undo(), "ran out of history before the start");
            steps += 1;
        }
        assert_eq!(steps, 6);
        assert_eq!(doc.selected, Some(anna), "the selection comes back too");
        assert!(doc.scene().get(bea).is_some());
        assert!(doc.can_redo());
        for _ in 0..steps {
            assert!(doc.redo());
        }
        assert!(!doc.redo());
        assert_eq!(doc.scene().objects(), end.objects());

        // A new change drops what could be redone.
        doc.undo();
        doc.rename(light, "Moon");
        assert!(!doc.can_redo());
    }

    #[test]
    fn changes_that_change_nothing_leave_no_step() {
        let (mut doc, [_, anna, _, light, _]) = cast();
        let steps = |doc: &Document| doc.undo.len();
        let before = steps(&doc);
        assert!(!doc.rename(anna, "Anna"));
        assert!(!doc.rename(anna, "   "));
        assert!(!doc.set_visible(light, true));
        assert!(!doc.edit(doc.scene().get(anna).unwrap().clone(), false));
        assert!(!doc.delete(ObjectId(99)));
        assert_eq!(steps(&doc), before);
    }

    #[test]
    fn a_cancelled_drag_leaves_no_step() {
        let (mut doc, [_, anna, ..]) = cast();
        let start = doc.scene().clone();
        let before = (doc.undo.len(), doc.redo.len());
        for x in [0.1, 0.2] {
            let mut edited = doc.scene().get(anna).unwrap().clone();
            edited.transform.position[0] = x;
            assert!(doc.edit(edited, true));
        }
        doc.cancel_gesture();
        assert_eq!(doc.scene().objects(), start.objects());
        assert_eq!((doc.undo.len(), doc.redo.len()), before);
        // With no drag under way there is nothing to call off.
        doc.cancel_gesture();
        assert_eq!((doc.undo.len(), doc.redo.len()), before);
    }

    #[test]
    fn an_object_is_where_its_folders_put_it() {
        let (mut doc, [cast, anna, ..]) = cast();
        let mut folder = doc.scene().get(cast).unwrap().clone();
        folder.transform.position = [2.0, 0.0, 0.0];
        folder.transform.rotation = [0.0, 90.0, 0.0];
        doc.edit(folder, false);
        let mut inside = doc.scene().get(anna).unwrap().clone();
        inside.transform.position = [0.0, 0.0, 1.0];
        doc.edit(inside, false);
        // The folder's quarter turn around Y carries +Z to +X.
        let place = doc.scene().world(anna).w_axis.truncate();
        assert!((place - Vec3::new(3.0, 0.0, 0.0)).length() < 1e-5, "{place:?}");
        assert_eq!(doc.scene().parent_world(cast), Mat4::IDENTITY);
    }

    #[test]
    fn one_drag_is_one_step() {
        let (mut doc, [_, anna, ..]) = cast();
        let start = doc.scene().clone();
        for x in [0.1, 0.2, 0.3] {
            let mut edited = doc.scene().get(anna).unwrap().clone();
            edited.transform.position[0] = x;
            assert!(doc.edit(edited, true));
        }
        doc.end_gesture();
        let mut edited = doc.scene().get(anna).unwrap().clone();
        edited.transform.rotation[1] = 90.0;
        assert!(doc.edit(edited, false));

        assert!(doc.undo());
        let anna_now = doc.scene().get(anna).unwrap();
        assert_eq!(anna_now.transform.position[0], 0.3);
        assert_eq!(anna_now.transform.rotation[1], 0.0);
        assert!(doc.undo());
        assert_eq!(doc.scene().objects(), start.objects());
    }

    #[test]
    fn hiding_a_folder_hides_what_it_holds() {
        let (mut doc, [folder, anna, _, light, _]) = cast();
        assert!(doc.scene().shown(anna));
        doc.set_visible(folder, false);
        assert!(!doc.scene().shown(anna));
        assert!(doc.scene().get(anna).unwrap().visible, "its own switch is untouched");
        assert!(doc.scene().shown(light));
    }

    #[test]
    fn the_key_light_is_the_first_light_that_shows() {
        let (mut doc, [.., light, _]) = cast();
        doc.selected = None;
        let second = doc.add("Fill", Content::light());
        assert_eq!(doc.scene().key_light().map(|o| o.id), Some(light));
        doc.set_visible(light, false);
        assert_eq!(doc.scene().key_light().map(|o| o.id), Some(second));
        doc.set_visible(second, false);
        assert!(doc.scene().key_light().is_none());
    }

    #[test]
    fn a_light_shines_along_its_rotation() {
        let mut scene = Scene::empty();
        let id = scene.add("Light", Content::light(), None);
        let light = scene.get_mut(id).unwrap();
        // Unturned, it shines along +Z, so it stands towards -Z.
        light.transform.rotation = [0.0; 3];
        let d = light.light_direction();
        assert!((Vec3::from(d) - Vec3::NEG_Z).length() < 1e-5);
        // Tilted down by 90 degrees it shines down, so it stands above.
        light.transform.rotation = [90.0, 0.0, 0.0];
        let d = light.light_direction();
        assert!((Vec3::from(d) - Vec3::Y).length() < 1e-5, "{d:?}");
        // A new light stands in front of the character, up and to a side.
        let fresh = Scene::new();
        let d = fresh.key_light().unwrap().light_direction();
        assert!(d[2] > 0.5 && d[1] > 0.2, "{d:?}");
    }

    #[test]
    fn a_transform_matches_unity() {
        let t = Transform {
            position: [1.0, 2.0, 3.0],
            rotation: [0.0, 90.0, 0.0],
            scale: [2.0, 2.0, 2.0],
        };
        // Turned 90 degrees around Y, forward (+Z) points along +X.
        let p = t.matrix().transform_point3(Vec3::Z);
        assert!((p - Vec3::new(3.0, 2.0, 3.0)).length() < 1e-5, "{p:?}");
    }

    #[test]
    fn a_scene_is_readable_text_and_reads_back() {
        let (mut doc, [_, anna, ..]) = cast();
        let mut edited = doc.scene().get(anna).unwrap().clone();
        edited.transform.rotation = [0.0, 45.0, 0.0];
        edited.visible = false;
        if let Content::Character { animation, .. } = &mut edited.content {
            *animation = Some(Box::new(Animation {
                options: Some([0.25, 0.75]),
                ..Animation::maker_pose()
            }));
        }
        doc.edit(edited, false);
        let text = toml::to_string(doc.scene()).unwrap();
        assert!(text.contains("name = \"Anna\""), "{text}");
        assert!(text.contains("kind = \"character\""), "{text}");
        assert!(text.contains("card = \"/cards/anna.png\""), "{text}");
        assert!(text.contains("options = [0.25, 0.75]"), "{text}");
        let back: Scene = toml::from_str(&text).unwrap();
        assert_eq!(&back, doc.scene());
    }

    #[test]
    fn the_maker_pose_is_known_by_what_it_plays() {
        let pose = Animation::maker_pose();
        assert!(pose.is_maker_pose() && pose.file.is_none());
        let other = Animation {
            state: "Stand_00_00".into(),
            ..pose.clone()
        };
        assert!(!other.is_maker_pose());
    }

    #[test]
    fn a_changed_profile_is_saved_with_the_character() {
        use crate::profile::{self, Profile};
        let (mut doc, [_, anna, ..]) = cast();
        let mut edited = doc.scene().get(anna).unwrap().clone();
        let Content::Character { profile: own, .. } = &mut edited.content else {
            panic!("Anna is a character");
        };
        let mut p = Profile {
            nickname: "Annie".into(),
            ..Profile::default()
        };
        profile::add_custom(&mut p.custom_likes, &profile::LIKES, "Jazz", true);
        profile::add_custom(&mut p.custom_traits, &profile::TRAITS, "Early riser", false);
        *own = Some(Box::new(p));
        assert!(doc.edit(edited.clone(), false));
        let text = toml::to_string(doc.scene()).unwrap();
        assert!(text.contains("nickname = \"Annie\""), "{text}");
        assert!(text.contains("name = \"Jazz\""), "{text}");
        let back: Scene = toml::from_str(&text).unwrap();
        assert_eq!(back.get(anna).unwrap().content, edited.content);
    }

    #[test]
    fn clothes_states_and_the_pose_are_saved_and_undone() {
        let (mut doc, [_, anna, ..]) = cast();
        let pose = Animation {
            name: "Stand 1".into(),
            file: Some("Animations/Stand 1.rkanim".into()),
            bundle: "studio/anime/00.unity3d".into(),
            controller: "tachi".into(),
            state: "f_tachi_00".into(),
            override_bundle: String::new(),
            override_controller: String::new(),
            speed: 0.5,
            start: 0.25,
            options: None,
            force_loop: true,
        };
        let mut edited = doc.scene().get(anna).unwrap().clone();
        let Content::Character {
            clothes,
            animation: worn,
            ..
        } = &mut edited.content
        else {
            panic!("Anna is a character");
        };
        *clothes = [1, 0, 0, 2, 3, 0, 0, 3, 3];
        *worn = Some(Box::new(pose.clone()));
        assert!(doc.edit(edited.clone(), false));
        let text = toml::to_string(doc.scene()).unwrap();
        assert!(text.contains("clothes = [1, 0, 0, 2, 3, 0, 0, 3, 3]"), "{text}");
        assert!(text.contains("state = \"f_tachi_00\""), "{text}");
        let back: Scene = toml::from_str(&text).unwrap();
        assert_eq!(&back, doc.scene());
        assert_eq!(back.get(anna).unwrap().content, edited.content);

        // One step back: dressed and as built again.
        assert!(doc.undo());
        let Content::Character { clothes, animation, .. } = &doc.scene().get(anna).unwrap().content else {
            panic!("Anna is a character");
        };
        assert_eq!((*clothes, animation), ([0; 9], &None));
        assert!(doc.redo());
        assert_eq!(doc.scene().get(anna).unwrap().content, edited.content);

        // A scene saved before clothes states and poses existed reads as
        // fully dressed and unposed, and a pose with only its animation
        // named plays at its own speed from its start.
        let old = text
            .lines()
            .filter(|l| !l.starts_with("clothes") && !l.starts_with("speed") && !l.starts_with("start"))
            .filter(|l| !l.starts_with("force_loop"))
            .collect::<Vec<_>>()
            .join("\n");
        let read: Scene = toml::from_str(&old).unwrap();
        let Content::Character { clothes, animation, .. } = &read.get(anna).unwrap().content else {
            panic!("Anna is a character");
        };
        assert_eq!(*clothes, [0; 9]);
        let pose = animation.as_ref().unwrap();
        assert_eq!((pose.speed, pose.start, pose.force_loop), (1.0, 0.0, false));
    }

    #[test]
    fn a_scene_that_says_pose_still_loads_its_animation() {
        // As Renkatsu 0.1.4 wrote a posed character: the table is called
        // pose and names no file of the project.
        let old = r#"
name = "Old"
next_id = 2

[[objects]]
id = 1
name = "Anna"
kind = "character"
card = "Characters/anna.png"
outfit = 0
outdoor_shoes = false
simulate = true

[objects.pose]
name = "Stand 1"
bundle = "studio/anime/00.unity3d"
controller = "tachi"
state = "f_tachi_00"
speed = 0.5
"#;
        let scene: Scene = toml::from_str(old).unwrap();
        let Content::Character { animation, .. } = &scene.objects()[0].content else {
            panic!("Anna is a character");
        };
        let animation = animation.as_ref().unwrap();
        assert_eq!((animation.name.as_str(), animation.speed), ("Stand 1", 0.5));
        assert_eq!(animation.file, None);
        assert_eq!(animation.state, "f_tachi_00");
        // It is written back under the new name.
        let text = toml::to_string(&scene).unwrap();
        assert!(text.contains("[objects.animation]"), "{text}");
        assert!(!text.contains("pose"), "{text}");
    }

    #[test]
    fn a_map_is_saved_with_its_place_and_time() {
        let mut doc = Document::new(Scene::new());
        let id = doc.add(
            "Rooftop",
            Content::map(PathBuf::from("Maps/Rooftop/Rooftop.rkmap"), MapTime::Day),
        );
        let mut edited = doc.scene().get(id).unwrap().clone();
        edited.transform.position = [1.5, 0.0, -2.0];
        edited.transform.rotation = [0.0, 90.0, 0.0];
        if let Content::Map { time, hide_walls, .. } = &mut edited.content {
            *time = MapTime::Evening;
            *hide_walls = false;
        }
        doc.edit(edited, false);
        let text = toml::to_string(doc.scene()).unwrap();
        assert!(text.contains("kind = \"map\""), "{text}");
        assert!(text.contains("map = \"Maps/Rooftop/Rooftop.rkmap\""), "{text}");
        assert!(text.contains("time = \"evening\""), "{text}");
        let back: Scene = toml::from_str(&text).unwrap();
        assert_eq!(&back, doc.scene());
        assert_eq!(back.uses("Maps/Rooftop"), 1);
        // A move of the map's folder is followed.
        let mut scene = back;
        assert!(scene
            .retarget(&|old| (old == "Maps/Rooftop/Rooftop.rkmap").then(|| "Maps/School/Rooftop.rkmap".to_owned())));
        assert_eq!(scene.uses("Maps/School/Rooftop.rkmap"), 1);
    }

    #[test]
    fn a_map_saved_before_maps_were_kept_still_loads() {
        // Scenes of Renkatsu 0.1.8 could hold a map object with nothing in
        // it.
        let old = r#"
name = "Old"
next_id = 2

[[objects]]
id = 1
name = "Map"
kind = "map"
"#;
        let scene: Scene = toml::from_str(old).unwrap();
        assert_eq!(
            scene.objects()[0].content,
            Content::Map {
                map: PathBuf::new(),
                time: MapTime::Day,
                hide_walls: true,
                light_intensity: 1.0,
            }
        );
    }

    #[test]
    fn files_that_move_are_followed_through_the_scene_and_its_history() {
        let (mut doc, [_, anna, ..]) = cast();
        let mut edited = doc.scene().get(anna).unwrap().clone();
        if let Content::Character { card, animation, .. } = &mut edited.content {
            *card = PathBuf::from("Characters/anna.png");
            *animation = Some(Box::new(Animation {
                name: "Sit".into(),
                file: Some("Animations/Sit.rkanim".into()),
                bundle: String::new(),
                controller: String::new(),
                state: String::new(),
                override_bundle: String::new(),
                override_controller: String::new(),
                speed: 1.0,
                start: 0.0,
                options: None,
                force_loop: false,
            }));
        }
        doc.edit(edited, false);
        doc.rename(anna, "Anna B");
        assert_eq!(doc.scene().uses("Characters/anna.png"), 1);
        assert_eq!(doc.scene().uses("Animations"), 1);
        assert_eq!(doc.scene().uses("Animations/Si"), 0);
        let moved = |old: &str| match old {
            "Characters/anna.png" => Some("Characters/Cast/Anna.png".to_owned()),
            "Animations/Sit.rkanim" => Some("Animations/Sitting/Sit.rkanim".to_owned()),
            _ => None,
        };
        assert!(doc.retarget(&moved));
        assert!(!doc.retarget(&moved), "nothing names the old paths any more");
        let paths = |doc: &Document| match &doc.scene().get(anna).unwrap().content {
            Content::Character { card, animation, .. } => (
                card.to_string_lossy().into_owned(),
                animation.as_ref().and_then(|a| a.file.clone()),
            ),
            _ => panic!("Anna is a character"),
        };
        let new = (
            "Characters/Cast/Anna.png".to_owned(),
            Some("Animations/Sitting/Sit.rkanim".to_owned()),
        );
        assert_eq!(paths(&doc), new);
        // The step before the rename names the files where they are now.
        assert!(doc.undo());
        assert_eq!(paths(&doc), new);
        assert!(doc.redo());
        assert_eq!(paths(&doc), new);
    }

    #[test]
    fn several_objects_are_selected_deleted_and_pasted_together() {
        let (mut doc, [folder, anna, bea, light, camera]) = cast();
        doc.select_many(&[light, camera]);
        assert!(doc.is_selected(light) && doc.is_selected(camera) && !doc.is_selected(anna));
        doc.toggle_selected(anna);
        doc.toggle_selected(camera);
        assert_eq!(doc.selected_all(), [light, anna]);
        // Selecting something else the plain way drops the rest.
        doc.selected = Some(bea);
        assert_eq!(doc.selected_all(), [bea]);
        assert!(!doc.is_selected(anna));
        // A folder and something inside it count as the folder.
        doc.select_many(&[anna, folder, light]);
        assert_eq!(doc.selected_all(), [folder, light]);
        let all = doc.selected_all();
        assert!(doc.delete_many(&all));
        assert_eq!(names(doc.scene()), [(0, "Camera")]);
        assert!(doc.undo());
        assert_eq!(doc.scene().objects().len(), 5);

        // Copy the folder with its characters and paste it at the top.
        let copied = doc.copy(folder);
        assert_eq!(copied.len(), 3);
        doc.selected = None;
        let pasted = doc.paste(&copied).unwrap();
        assert_eq!(doc.selected, Some(pasted));
        assert_eq!(
            names(doc.scene()),
            [
                (0, "Cast"),
                (1, "Anna"),
                (1, "Bea"),
                (0, "Key light"),
                (0, "Camera"),
                (0, "Cast 2"),
                (1, "Anna 2"),
                (1, "Bea 2"),
            ]
        );
        assert!(doc.undo());
        assert_eq!(doc.scene().objects().len(), 5);

        // Hide two at once, take both out of the folder at once.
        assert!(doc.set_visible_many(&[anna, bea], false));
        assert!(!doc.scene().get(anna).unwrap().visible && !doc.scene().get(bea).unwrap().visible);
        assert!(doc.undo());
        assert_eq!(doc.place_many(&[anna, bea], Place::Into(None)), 2);
        assert_eq!(names(doc.scene())[3..], [(0, "Anna"), (0, "Bea")]);
        assert!(doc.undo());
        assert_eq!(doc.scene().get(anna).unwrap().parent, Some(folder));
        let child = doc.add_in("Props", Content::Folder, Some(folder));
        assert_eq!(doc.scene().get(child).unwrap().parent, Some(folder));
    }
}
