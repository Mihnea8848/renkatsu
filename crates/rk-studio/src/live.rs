// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The live viewport: the scene's characters built from their cards and
//! drawn by Renkatsu's renderer on the window's own GPU device into a
//! texture that egui shows, every frame, through the camera of
//! `camera.rs`. The scene model says what exists; this follows it, building
//! a character when one appears, dropping it when it leaves, moving its
//! root to the object's transform and stepping its physics while it plays.
//! It also says where the bones are that objects are attached to, and
//! what of the characters lies under a point, for dropping things on them.
//!
//! A character is built in two halves. Reading its bundles, meshes and
//! textures is work for another thread, a job; what the job made is taken
//! in here, in the order the builds were asked for, so the scene draws as
//! if they had been built one after the other. Its textures then go to
//! the GPU a few a frame, and the character shows once they are all there.
//!
//! The scene's map is built the same way, from the project's own copy of
//! it, by a job, and set on the renderer once whole. Its object moves and
//! turns it, which costs nothing: the renderer takes the map's root each
//! frame.

use crate::camera::{Focus, Orbit, ViewCamera};
use crate::gizmo_pass::{GizmoPass, Mesh};
use crate::grid_pass::{Grid, GridPass};
use crate::jobs::{Handle, Jobs, Show};
use crate::mods::Games;
use crate::project::Project;
use crate::scene::{Content, MapTime, ObjectId, Poses, Scene};
use crate::state::{Log, Playback};
use anyhow::{Context, Result};
use eframe::egui_wgpu::{self, wgpu};
use glam::{Mat4, Quat, Vec3};
use rk_chara::bone_info::Part;
use rk_chara::clothes_state::{self as cs, States};
use rk_chara::motion::{self, Bound, Motion};
use rk_chara::pose::BoneEdit;
use rk_chara::scene::{ShadowCasting, Trs};
use rk_chara::Image;
use rk_render::{bounds, Camera, DrawLight, Environment, Renderer, StudioEffects};
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Target {
    texture: wgpu::Texture,
    id: egui::TextureId,
    size: [u32; 2],
}

/// What a character is built from: a change to any of it builds it again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Recipe {
    /// The card on disk.
    card: PathBuf,
    outfit: usize,
    outdoor_shoes: bool,
}

/// A character of the scene, built.
struct Actor {
    id: ObjectId,
    /// Small pictures of the character's patterns, by set, once asked
    /// for; and the sets asked for and not made yet.
    pictures: std::collections::HashMap<Pictures, Vec<(i32, Arc<egui::ColorImage>)>>,
    pictures_wanted: Vec<Pictures>,
    /// When the eyes' highlights began to shake, which they do on the real
    /// clock whether the scene plays or not, as in Studio.
    shake_since: std::time::Instant,
    /// BetterPenetration's aim of the shaft for this frame, and the one the
    /// character was last posed with.
    aim_job: Option<AimJob>,
    aim_posed: Option<AimJob>,
    /// Other characters' colliders this character's dynamic bones meet
    /// from its next update: a partner's shaft, the partner's body for the
    /// balls (`rk_chara::physics::Physics::set_guests`).
    guests_next: Option<rk_chara::physics::Guests>,
    /// The ones a partner's shaft gave last frame.
    received: rk_chara::physics::Guests,
    /// The vaginal or anal pull this character's shaft has on a partner,
    /// with the partner and the place; the end the shaft was last aimed
    /// at; and a pull on this character from a partner's shaft, the new
    /// one from the last frame first.
    hole_pull: Option<(
        ObjectId,
        rk_chara::better_penetration::Mode,
        rk_chara::better_penetration::MouthPull,
    )>,
    aimed_end: Option<Vec3>,
    hole_next: Option<rk_chara::better_penetration::HolePull>,
    hole_at: Option<rk_chara::better_penetration::HolePull>,
    /// The body has the pull bones or the inner bones a stroke moves.
    hole_bones: bool,
    /// The card is a male's.
    male: bool,
    /// A partner's shaft is in this character's mouth, which then holds
    /// it; as of this frame and the last one.
    mouth_held: bool,
    mouth_held_next: bool,
    /// The finger bones whose colliders push vaginas, and this character's
    /// vagina dynamic bones they push.
    fingers: Vec<usize>,
    vagina: Vec<usize>,
    /// The H position this character plays, as its male controller and
    /// state, when it plays one the game lists.
    h_position: Option<(String, String)>,
    /// The pull this character's shaft has on a partner's mouth, with the
    /// partner it is for.
    mouth_pull: Option<(ObjectId, rk_chara::better_penetration::MouthPull)>,
    /// Where a partner's shaft has pulled this character's mouth bone, in
    /// the bone's parent's space; and a new place from the last frame.
    mouth_at: Option<Vec3>,
    /// The new place, with the rate the mouth returns at once no shaft
    /// pulls it, from the partner's settings.
    mouth_next: Option<(Vec3, f32)>,
    mouth_return: f32,
    /// The size a partner's shaft gives this character's belly bulge from
    /// its next update, 1 for none.
    bulge_next: Option<f32>,
    recipe: Recipe,
    chara: rk_chara::Character,
    /// Every node's local transform as loaded, which a stop goes back to.
    rest: Vec<Trs>,
    /// The middle of the character as loaded and the distance that fits it.
    center: Vec3,
    distance: f32,
    /// Where the object is in the scene, as last given to the root.
    applied: Option<Mat4>,
    /// Where it was when the physics last saw it: hair and skirts are
    /// carried along from there when it is moved while nothing runs.
    physics_at: Option<Mat4>,
    /// How many outfits the card has.
    outfits: usize,
    /// The names More Outfits gives the card's outfits, by outfit number.
    outfit_names: std::collections::BTreeMap<usize, String>,
    /// The clothes states the character was last brought to.
    clothes: States,
    /// The liquid levels the skin was last laid with, and whether the
    /// clothes were laid with them too.
    liquid: (rk_chara::siru::Levels, bool),
    /// The animation of the scene the actor last looked for, by its file
    /// in the project, bundle, controller and state.
    pose: Option<[String; 6]>,
    /// The body height the animation plays at in place of the character's
    /// own: the female's, while it plays the male's half of her H position,
    /// as the main game's H scenes set it.
    paired_height: Option<f32>,
    /// That animation, when it could be read.
    playing: Option<Playing>,
    /// Why it could not be read.
    animation_problem: Option<String>,
    /// Hand patterns, by hand and number, that could not be read.
    hand_problems: Vec<(usize, i32)>,
    /// The point of the animation the bones were last posed at.
    phase: Option<f32>,
    /// Frames the cloth nets still have to settle for while the scene
    /// stands, which they do a few at a time as it is drawn.
    cloth_owed: u32,
    /// What the character is among changed since it last settled.
    among_changed: bool,
    /// The edits made by hand the bones were last posed with, and their
    /// bones' nodes.
    layer: Option<rk_chara::pose::Layer>,
    edits: Vec<(usize, rk_chara::pose::BoneEdit)>,
    /// The bones CharaStudio poses by hand (FK), as found on it.
    fk: Vec<rk_chara::bone_info::FkBone>,
    /// Its textures are on the GPU: it can be drawn without a wait.
    warm: bool,
    /// Seconds the build took.
    took: f32,
}

/// What a map is built from: a change to any of it builds it again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MapRecipe {
    /// The map's sidecar on disk.
    sidecar: PathBuf,
    time: MapTime,
}

/// The name of the colour table each time of day of a map takes.
type TimeFilters = Vec<TimeLook>;

/// What the lookup job reads from a map's bundle: each time of day's look,
/// and the mirrors.
type MapExtras = (
    TimeFilters,
    Vec<rk_render::mirror::MapMirror>,
    rk_map::motion::MapMotion,
);

/// What a map's time of day sets beside its light, read from its bundle:
/// the colour filter by its table's name, and the sun shafts' colour and
/// sun in the map's space.
#[derive(Debug, Clone, PartialEq)]
struct TimeLook {
    time: MapTime,
    filter: Option<String>,
    shaft_color: [f32; 3],
    caster: Option<[f32; 3]>,
}

/// What the shown map's time of day hands the screen effects.
#[derive(Debug, Clone, Copy, Default)]
struct MapLook {
    /// The colour filter's number among Studio's.
    filter: Option<i32>,
    /// The sun shafts' colour, and their sun in the scene.
    shafts: Option<([f32; 3], Option<Vec3>)>,
}

/// A map a job is building.
struct PendingMap {
    id: ObjectId,
    recipe: MapRecipe,
    name: String,
    started: std::time::Instant,
    handle: Handle<rk_render::map::MapScene>,
}

/// The scene's map, as far as the renderer has it.
#[derive(Default)]
struct MapState {
    /// The map set on the renderer: its object and what it was built from.
    set: Option<(ObjectId, MapRecipe)>,
    pending: Option<PendingMap>,
    /// A map that could not be built, so that it is not tried again every
    /// frame.
    failed: Option<(ObjectId, MapRecipe, String)>,
    /// Seconds the build of the map that is set took.
    took: f32,
    /// The map is set and its object shows.
    shown: bool,
    /// What of the set map can be stood on, and where it is while it
    /// shows.
    ground: Option<MapGround>,
    /// The colour filter of each of the set map's times of day, by the
    /// name of its table, read from the map's bundle; and the job reading
    /// it.
    filters: Option<(PathBuf, TimeFilters)>,
    reading_filters: Option<(PathBuf, Handle<MapExtras>)>,
    /// The set map's mirrors, read from its bundle beside its filters.
    mirrors: Option<(PathBuf, Vec<rk_render::mirror::MapMirror>)>,
    /// Where the set map's objects are, for its Animators to find them,
    /// and the Animators, bound to them; the time they were last posed at.
    places: Option<(PathBuf, Vec<rk_map::motion::Place>)>,
    motion: Option<(PathBuf, rk_map::motion::MapMotion)>,
    posed_at: Option<f32>,
    placed: Option<rk_render::map::MapPlacement>,
    /// The set map's movable pieces, and the matrices last given to the
    /// renderer for them.
    pieces: Vec<crate::map_pieces::Piece>,
    piece_matrices: Vec<(usize, glam::Mat4)>,
    /// Where the map's Animators last posed their objects.
    posed: Vec<(usize, glam::Mat4)>,
    /// The pieces drawn once more for a duplicate: the map's object, the
    /// matrix and the scene's object.
    copies: Vec<(usize, glam::Mat4, ObjectId)>,
    /// What each door was last sent to: open or not, and when its move
    /// began on the clock, by its Animator's path.
    doors_sent: std::collections::HashMap<String, (bool, Option<f32>)>,
}

/// The meshes of a map the camera sees, in the map's own space, kept
/// after the upload for Snap to floor.
struct MapGround {
    pieces: Vec<MapPiece>,
}

/// What `Live::obstacles` was made from.
#[derive(PartialEq)]
struct ObstaclesKey {
    ground: usize,
    root: glam::Mat4,
    moved: Vec<(usize, glam::Mat4)>,
    copies: Vec<(usize, glam::Mat4)>,
}

struct MapPiece {
    /// Its place in the map's list of objects.
    object: usize,
    /// Shared with the characters' skirts, which rest on the map.
    positions: std::sync::Arc<Vec<[f32; 3]>>,
    indices: std::sync::Arc<Vec<u32>>,
    bounds: ([f32; 3], [f32; 3]),
}

impl MapGround {
    /// Takes the seen meshes' positions and indices out of `map`, which
    /// the renderer no longer needs once it has uploaded them.
    fn take(map: &mut rk_render::map::MapScene) -> Self {
        let mut pieces = Vec::new();
        for i in 0..map.objects.len() {
            if !map.seen(&map.objects[i]) {
                continue;
            }
            let mesh = &mut map.objects[i].mesh;
            if mesh.indices.is_empty() {
                continue;
            }
            pieces.push(MapPiece {
                object: i,
                positions: std::sync::Arc::new(std::mem::take(&mut mesh.positions)),
                indices: std::sync::Arc::new(std::mem::take(&mut mesh.indices)),
                bounds: mesh.bounds,
            });
        }
        Self { pieces }
    }

    /// How far a ray in an object's baked space goes before it meets that
    /// object.
    fn hit_object(&self, object: usize, origin: Vec3, dir: Vec3) -> Option<f32> {
        let piece = self.pieces.iter().find(|p| p.object == object)?;
        let (lo, hi) = piece.bounds;
        if !ray_meets_box(origin, dir, [lo, hi], f32::MAX) {
            return None;
        }
        piece
            .indices
            .chunks_exact(3)
            .filter_map(|triangle| {
                let corner = |i: usize| piece.positions.get(triangle[i] as usize).copied().map(Vec3::from);
                ray_meets_triangle(origin, dir, corner(0)?, corner(1)?, corner(2)?)
            })
            .min_by(f32::total_cmp)
    }

    /// The object a ray in the map's own space meets first, and how far
    /// along. Objects in `moved` are where their matrix puts them (none
    /// at all when it has no size, as a hidden piece); those in `skip`
    /// are passed through.
    fn hit(&self, origin: Vec3, dir: Vec3, moved: &[(usize, glam::Mat4)], skip: &[usize]) -> Option<(usize, f32)> {
        let mut best: Option<(usize, f32)> = None;
        for piece in &self.pieces {
            if skip.contains(&piece.object) {
                continue;
            }
            // Into the piece's baked place: along the ray, distances stay.
            let (origin, dir) = match moved.iter().find(|(o, _)| *o == piece.object) {
                Some((_, m)) if m.determinant().abs() < 1e-9 => continue,
                Some((_, m)) => {
                    let back = m.inverse();
                    (back.transform_point3(origin), back.transform_vector3(dir))
                }
                None => (origin, dir),
            };
            let reach = best.map_or(f32::MAX, |(_, t)| t);
            let (lo, hi) = piece.bounds;
            if !ray_meets_box(origin, dir, [lo, hi], reach) {
                continue;
            }
            for triangle in piece.indices.chunks_exact(3) {
                let corner = |i: usize| piece.positions.get(triangle[i] as usize).copied().map(Vec3::from);
                let (Some(a), Some(b), Some(c)) = (corner(0), corner(1), corner(2)) else {
                    continue;
                };
                if let Some(t) = ray_meets_triangle(origin, dir, a, b, c) {
                    if best.is_none_or(|(_, nearest)| t < nearest) {
                        best = Some((piece.object, t));
                    }
                }
            }
        }
        best
    }
}

/// A character a job is building.
struct Pending {
    id: ObjectId,
    recipe: Recipe,
    name: String,
    handle: Handle<Prepared>,
}

/// What a build job hands back.
struct Prepared {
    actor: Actor,
    /// The colour tables of Studio's colour filters, when the build was
    /// asked to look for them.
    filters: Option<Filters>,
}

/// Studio's colour filters by their number (`SceneInfo.aceNo`).
type Filters = std::collections::BTreeMap<i32, Arc<Image>>;

/// How far a character of the scene is.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    /// Nothing was started for it yet.
    Waiting,
    Building,
    /// Built; its textures are going to the GPU.
    Uploading,
    /// Ready to draw, after a build of this many seconds.
    Ready(f32),
    Failed,
}

/// The time a frame may spend on sending a new character's textures to
/// the GPU.
const UPLOAD_BUDGET: std::time::Duration = std::time::Duration::from_millis(6);

/// An animation bound to an actor's bones.
struct Playing {
    motion: Motion,
    bound: Bound,
}

impl Playing {
    /// Whether the animation holds one pose: it does not loop and is as
    /// short as the game's fixed poses.
    fn is_static(&self) -> bool {
        !self.motion.loops() && self.motion.duration() <= rk_chara::animations::STATIC_SECONDS
    }

    /// The point of the animation that shows at the scene's time. A static
    /// pose shows its last frame, which is where the game leaves it.
    fn phase(&self, pose: &crate::scene::Animation, time: f32) -> f32 {
        if self.is_static() {
            return 1.0;
        }
        self.motion
            .phase(self.motion.normalized(pose.start, time, pose.speed), pose.force_loop)
    }
}

/// The scene's time for one frame: the transport before and now, the
/// seconds since the last frame and the seconds played since the stop.
#[derive(Debug, Clone, Copy)]
pub struct Clock {
    pub was: Playback,
    pub now: Playback,
    pub dt: f32,
    pub time: f32,
    /// A character whose keys play on a time of their own, as the
    /// Animator's solo play does, while the scene stands.
    pub solo: Option<(ObjectId, f32)>,
}

impl Clock {
    /// A scene that stands still.
    pub fn stopped() -> Clock {
        Clock {
            was: Playback::Stopped,
            now: Playback::Stopped,
            dt: 0.0,
            time: 0.0,
            solo: None,
        }
    }
}

/// Frames the cloth nets of one character settle for in each frame that
/// is drawn while the scene stands.
const CLOTH_FRAMES: u32 = 3;

impl Actor {
    /// The character as something to focus on, where its transform put it.
    fn subject(&self) -> Focus {
        let world = self.applied.unwrap_or_default();
        let (scale, ..) = world.to_scale_rotation_translation();
        Focus {
            object: self.id,
            center: world.transform_point3(self.center),
            distance: self.distance * scale.abs().max_element().max(1e-3),
        }
    }

    /// Puts the character's root where the object is in the scene. The
    /// physics is put back on the bones unless it is running, when the
    /// move is something for it to react to.
    fn place(&mut self, world: Mat4, running: bool) -> bool {
        if self.applied == Some(world) {
            return false;
        }
        let top = self.chara.top;
        let (scale, rot, pos) = (world * self.rest[top].matrix()).to_scale_rotation_translation();
        self.chara.scene.nodes[top].local = Trs { pos, rot, scale };
        if !running {
            // Hair and skirts move with the character and keep the shape
            // they settled in; CharaStudio does not put them back on their
            // rest shape, which in a sitting or lying pose is inside the
            // legs. The first time there is nothing to carry.
            match self.physics_at {
                Some(was) => self.chara.physics.carry(world * was.inverse()),
                None => self.chara.physics.reset(&self.chara.scene),
            }
        }
        self.physics_at = Some(world);
        self.applied = Some(world);
        true
    }

    /// Where a bone is, by its name, in the space of the character's
    /// object.
    fn bone(&self, name: &str) -> Option<Mat4> {
        let node = self.chara.scene.find(self.chara.top, name)?;
        let (world, _) = self.chara.scene.world();
        Some(self.applied.unwrap_or_default().inverse() * world[node])
    }

    /// Poses one frame: the animation at `phase` when there is one, and
    /// the edits made by hand on top. The edited bones start from how they
    /// were loaded, so that a bone the animation does not move takes its
    /// edit once, not once more every frame.
    fn pose_frame(&mut self, phase: Option<f32>, states: &States) {
        for &(node, _) in &self.edits {
            if let (Some(n), Some(rest)) = (self.chara.scene.nodes.get_mut(node), self.rest.get(node)) {
                n.local = *rest;
            }
        }
        let motion = self.playing.as_ref().zip(phase).map(|(p, phase)| (&p.bound, phase));
        self.chara.pose_frame(motion, &self.edits, self.layer.as_ref(), states);
        // BetterPenetration aims the shaft over the animation, FK and IK,
        // before the dynamic bones move.
        if let (Some(job), Some(shaft)) = (self.aim_job, self.chara.shaft.as_ref()) {
            let end = match (&job.body, shaft.points.first()) {
                (Some(body), Some(&root)) => {
                    let start = self.chara.scene.world().0[root].w_axis.truncate();
                    body.end(&job, start, shaft.base_length)
                }
                _ => job.end,
            };
            self.aimed_end = Some(end);
            shaft.aim(&mut self.chara.scene, job.entry, end, &job.settings, job.vaginal);
        }
        // A partner's shaft holds the mouth where it last pulled it.
        if let (Some(at), Some(node)) = (self.mouth_at, self.mouth_node()) {
            self.chara.scene.nodes[node].local.pos = self.rest[node].pos + at;
        }
        // And her vagina or anus where its stroke pulls them.
        if let Some(pull) = self.hole_at {
            rk_chara::better_penetration::apply_hole_pull(&mut self.chara.scene, self.chara.top, &pull);
        }
    }

    /// The face to show: the layer's, or with none the character's own, its
    /// mouth holding a partner's shaft while one is in it. None leaves the
    /// face as it was loaded.
    fn face_shown(&self) -> Option<rk_chara::face::Face> {
        let own = self.layer.as_ref().and_then(|l| l.face);
        if !self.mouth_held {
            return own;
        }
        let mut face = own.or_else(|| self.chara.face.as_ref().map(|r| r.start))?;
        face.mouth = rk_chara::face::HOLDING_MOUTH;
        face.mouth_open = 1.0;
        Some(face)
    }

    /// The bone of the mouth a shaft pulls.
    fn mouth_node(&self) -> Option<usize> {
        self.chara
            .scene
            .find(self.chara.top, rk_chara::better_penetration::MOUTH_BONE)
    }

    /// Puts the bones back as they were loaded.
    fn back_to_rest(&mut self) {
        for (node, rest) in self.chara.scene.nodes.iter_mut().zip(&self.rest) {
            node.local = *rest;
        }
        self.applied = None;
    }

    /// Brings the character in line with its object for one frame: the
    /// clothes states, the pose or animation, the place, and the physics.
    /// `game` reads an animation or a hand pattern that is not loaded
    /// yet. Returns whether the character moved as a whole.
    ///
    /// While the scene stands, a change settles the hair and the chains
    /// at once. The cloth nets of Advanced Skirt Physics cost far more:
    /// they settle over the next frames that are drawn (`cloth_owed`),
    /// unless `patient`, when they settle at once too.
    #[allow(clippy::too_many_arguments)]
    fn update(
        &mut self,
        object: &crate::scene::Object,
        world: Mat4,
        game: &mut dyn FnMut(Load) -> Result<Motion>,
        clock: &Clock,
        keys: Option<&rk_chara::keys::Keys>,
        patient: bool,
        log: &mut Log,
    ) -> bool {
        let Content::Character {
            simulate,
            clothes,
            liquid,
            liquid_clothes,
            game_skirts,
            animation: pose,
            pose_layer,
            ..
        } = &object.content
        else {
            return false;
        };
        // What has to be posed again from the start.
        let mut again = clock.was != Playback::Stopped && clock.now == Playback::Stopped;
        // A stop lets the mouth go back too.
        if again {
            self.mouth_pull = None;
            self.mouth_at = None;
            self.mouth_next = None;
        }
        // A new aim, or none where there was one, poses the shaft again.
        if !AimJob::near(self.aim_job.as_ref(), self.aim_posed.as_ref()) {
            self.aim_posed = self.aim_job;
            again = true;
        }
        // Another character's colliders, new or gone, settle the dynamic
        // bones again.
        if let Some(guests) = self.guests_next.take() {
            let guests = rk_chara::physics::merged(guests);
            if !rk_chara::physics::guests_near(&guests, self.chara.physics.guests(), STILL) {
                self.chara.physics.set_guests(guests);
                again = true;
            }
        }
        // A belly bulge of another size settles the belly again.
        if let Some(scale) = self.bulge_next.take() {
            if rk_chara::better_penetration::bulge(&mut self.chara.physics, &self.chara.scene, scale) {
                again = true;
            }
        }
        // A partner's stroke that pulls her vagina or anus poses her again;
        // with none, the bones go back to how they were loaded.
        match self.hole_next.take() {
            Some(pull) => {
                if self.hole_at != Some(pull) {
                    self.hole_at = Some(pull);
                    again |= clock.now != Playback::Playing;
                }
            }
            None => {
                if let Some(pull) = self.hole_at.take() {
                    let names = rk_chara::better_penetration::bones_by_name(&self.chara.scene, self.chara.top);
                    let inner = rk_chara::better_penetration::inner_bone(pull.mode);
                    let bones = rk_chara::better_penetration::pull_bones(pull.mode)
                        .iter()
                        .map(|(n, _)| *n);
                    for name in bones.chain(inner) {
                        if let Some(&node) = names.get(name) {
                            self.chara.scene.nodes[node].local = self.rest[node];
                        }
                    }
                    again = true;
                }
            }
        }
        // A partner's shaft that moved the mouth poses the face again.
        // With none pulling it, the mouth returns while the scene plays,
        // at the rate of the shaft that pulled it last.
        let playing = clock.now == Playback::Playing;
        if let Some((at, rate)) = self.mouth_next.take() {
            self.mouth_return = rate;
            if self.mouth_at.is_none_or(|was| was.distance(at) > STILL * 0.1) {
                self.mouth_at = Some(at);
                again |= !playing;
            }
        } else if let (Some(at), true) = (self.mouth_at, playing) {
            let left = at.length() - self.mouth_return * rk_chara::better_penetration::PULL_STEP;
            if left > 1e-6 && self.mouth_return > 0.0 {
                self.mouth_at = Some(at * (left / at.length()));
            } else if self.mouth_return > 0.0 {
                self.mouth_at = None;
                if let Some(node) = self.mouth_node() {
                    self.chara.scene.nodes[node].local.pos = self.rest[node].pos;
                }
            }
        }

        // The states the scene asks for, as far as the outfit has them.
        let states = cs::fitted(clothes, &self.chara.clothes.worn);
        if states != self.clothes {
            self.chara.set_clothes(&states);
            self.clothes = states;
            again = true;
        }

        // Advanced Skirt Physics goes by the outfit; a switch settles the
        // skirts again.
        let advanced = !game_skirts.contains(&self.recipe.outfit);
        if self.chara.physics.set_advanced(advanced) {
            again = true;
        }

        // The liquid on the skin is material values only: the bones and
        // the pose are untouched by it.
        if (*liquid, *liquid_clothes) != self.liquid {
            self.chara.set_siru(liquid, *liquid_clothes);
            self.liquid = (*liquid, *liquid_clothes);
        }

        // The H animation options pick the clips to blend, so a change
        // to them reads the motion again like a change of animation.
        let key = pose.as_ref().map(|p| {
            [
                p.file.clone().unwrap_or_default(),
                p.bundle.clone(),
                p.controller.clone(),
                p.state.clone(),
                p.options.map(|[a, b]| format!("{a} {b}")).unwrap_or_default(),
                self.paired_height.map(|h| h.to_string()).unwrap_or_default(),
            ]
        });
        if key != self.pose {
            self.pose = key;
            self.playing = None;
            self.animation_problem = None;
            if let Some(pose) = pose {
                match game(Load::Animation) {
                    Ok(motion) => {
                        let bound = motion.bind(&motion::Skeleton::of(&self.chara.scene, self.chara.animator));
                        self.playing = Some(Playing { motion, bound });
                    }
                    Err(e) => {
                        let why = format!("{e:#}");
                        log.error(format!(
                            "{}: the animation {} could not be read: {why}",
                            object.name, pose.name
                        ));
                        self.animation_problem = Some(why);
                    }
                }
            }
            // Stiletto's flags go by the animation.
            let named = pose.as_ref().filter(|_| self.playing.is_some());
            if let Some(heels) = &mut self.chara.heels {
                heels.animation(named.map(|p| p.controller.as_str()), named.map(|p| p.state.as_str()));
            }
            // The bones the last animation wrote go back to where they
            // were loaded.
            self.back_to_rest();
            self.chara.refresh_heels(&states);
            again = true;
        }

        // Edits made by hand: a change poses the character again from
        // the bones as loaded, so a bone no longer edited goes back.
        // The keys of the scene's sequence at the clock's time, over the
        // pose by hand: the bones' while FK is on, the IK targets' while IK
        // is, and the hands', the face's and the skin effects' always.
        let start = self.chara.face.as_ref().map(|f| f.start).unwrap_or_default();
        let keyed = keys.filter(|k| !k.is_empty()).map(|k| {
            let mut l = pose_layer.as_deref().cloned().unwrap_or_default();
            let time = match clock.solo {
                Some((solo, time)) if solo == self.id => time,
                _ => clock.time,
            };
            let (fk, ik) = (l.fk_on, l.ik_on);
            let mut at = rk_chara::keys::pose_at(k, time);
            at.retain(|name, _| {
                rk_chara::channels::Channel::of_key(name).is_some()
                    || if rk_chara::pose::ik_key_target(name).is_some() {
                        ik
                    } else {
                        fk
                    }
            });
            l.with_keys_from(at, &start);
            l
        });
        let layer = keyed.as_ref().or(pose_layer.as_deref()).filter(|l| !l.is_empty());
        // A new animation, other clothes or a stop put the body somewhere
        // else at once, and the cloth nets start again there. An edit by
        // hand moves it a little, and they follow.
        let fresh = again;
        if layer != self.layer.as_ref() {
            self.layer = layer.cloned();
            // A bone of CharaStudio's FK table takes its edit while one of
            // its parts is on.
            let fk = &self.fk;
            let parts_of = |name: &str| fk.iter().find(|b| b.info.bone == name).map(|b| b.info.parts());
            self.edits = layer.map_or_else(Vec::new, |l| {
                l.resolve_parts(&self.chara.scene, self.chara.top, parts_of)
            });
            self.back_to_rest();
            // The face is blend shapes, set once for the layer.
            let shown = self.face_shown();
            if let Some(face) = &self.chara.face {
                face.show(&mut self.chara.scene, shown.as_ref());
            }
            let skin = self.layer.as_ref().map(|l| l.skin).unwrap_or_default();
            self.chara.show_skin_effects(&skin);
            again = true;
        }
        // A shaft come into the mouth or gone from it shows the face again.
        let held = std::mem::take(&mut self.mouth_held_next);
        if held != self.mouth_held {
            self.mouth_held = held;
            let shown = self.face_shown();
            if let Some(face) = &self.chara.face {
                face.show(&mut self.chara.scene, shown.as_ref());
            }
            again |= clock.now != Playback::Playing;
        }
        // Shaking highlights take a new size every step of the real clock.
        if let (Some(rig), Some(face)) = (&self.chara.face, self.face_shown()) {
            if face.shaking {
                rig.show_at(
                    &mut self.chara.scene,
                    Some(&face),
                    self.shake_since.elapsed().as_secs_f32(),
                );
            }
        }
        // A hand pattern not loaded yet is read once.
        let hands = self.layer.as_ref().map_or([0, 0], |l| l.hands);
        for (side, pattern) in hands.into_iter().enumerate() {
            let loaded = self.chara.hands[side].as_ref().map(|c| c.pattern);
            if pattern == 0 || loaded == Some(pattern) || self.hand_problems.contains(&(side, pattern)) {
                continue;
            }
            let chara = &self.chara;
            let clip = game(Load::Hand(side, pattern)).and_then(|motion| {
                rk_chara::hand_pattern::HandClip::bind(pattern, &motion, &chara.scene, chara.top, side)
            });
            match clip {
                Ok(clip) => {
                    self.chara.hands[side] = Some(clip);
                    again = true;
                }
                Err(e) => {
                    log.error(format!(
                        "{}: the hand pattern {pattern} could not be read: {e:#}",
                        object.name
                    ));
                    self.hand_problems.push((side, pattern));
                }
            }
        }

        let phase = match (&self.playing, pose) {
            (Some(playing), Some(pose)) => Some(playing.phase(pose, clock.time)),
            _ => None,
        };
        let running = clock.now == Playback::Playing;
        if running {
            // A frame as the game runs it: the physics puts its bones
            // back, the animation writes, the physics simulates.
            let moved = self.place(world, true);
            if *simulate {
                self.chara.physics.begin_frame(&mut self.chara.scene);
            }
            self.pose_frame(phase, &states);
            if *simulate {
                self.chara.physics.end_frame(&mut self.chara.scene, clock.dt);
            }
            self.phase = phase;
            self.cloth_owed = 0;
            self.among_changed = false;
            return moved;
        }
        let lazy = !patient && *simulate && self.chara.physics.has_nets();
        let among = std::mem::take(&mut self.among_changed);
        if !again && phase == self.phase {
            let moved = self.place(world, false);
            // Moved onto a chair or a bed, or with a map that came or
            // changed around it, the skirts settle on it.
            let physics = &self.chara.physics;
            if (moved || among) && *simulate && physics.advanced() && physics.has_obstacles() {
                self.chara.physics.hold_cloth(lazy);
                for _ in 0..rk_chara::physics::SETTLE_FRAMES {
                    self.chara.physics.begin_frame(&mut self.chara.scene);
                    self.chara
                        .physics
                        .end_frame(&mut self.chara.scene, rk_chara::physics::FRAME_TIME);
                }
                self.chara.physics.hold_cloth(false);
                if lazy {
                    self.cloth_owed = rk_chara::physics::SETTLE_FRAMES;
                }
            }
            if !*simulate {
                self.cloth_owed = 0;
            }
            self.settle_cloth(CLOTH_FRAMES);
            return moved;
        }
        // Nothing plays: the character is posed once and holds still.
        // Advanced Skirt Physics moves into the new pose from this one.
        let mut from: Vec<Trs> = self.chara.scene.nodes.iter().map(|n| n.local).collect();
        self.back_to_rest();
        self.place(world, false);
        let top = self.chara.top;
        from[top] = self.chara.scene.nodes[top].local;
        self.chara.refresh_heels(&states);
        let motion = self.playing.is_some() && phase.is_some();
        let hands = self.layer.as_ref().is_some_and(|l| l.hands != [0, 0]);
        if motion || hands || !self.edits.is_empty() {
            // Hair and skirts settle from where they were, as CharaStudio
            // never puts them back on their rest shape for a new pose.
            self.pose_frame(phase, &states);
            if *simulate {
                // Hair and skirts settle on the pose, as they settled on
                // the character when it was built.
                self.chara.physics.hold_cloth(lazy);
                for frame in 0..rk_chara::physics::SETTLE_FRAMES {
                    self.chara.physics.begin_frame(&mut self.chara.scene);
                    self.pose_frame(phase, &states);
                    let amount = (frame + 1) as f32 / (rk_chara::physics::EASE_FRAMES + 1) as f32;
                    self.chara.physics.ease(&mut self.chara.scene, &from, amount);
                    // The cloth nets start again once a new pose is
                    // reached, as in `Character::settle_pose`.
                    if fresh && frame == rk_chara::physics::EASE_FRAMES {
                        self.chara.physics.restart_cloth();
                    }
                    self.chara
                        .physics
                        .end_frame(&mut self.chara.scene, rk_chara::physics::FRAME_TIME);
                }
                self.chara.physics.hold_cloth(false);
            }
        } else if *simulate {
            // Back in the pose it was loaded in: nothing settles but the
            // nets, which start again there.
            self.chara.physics.restart_cloth();
            if !lazy {
                self.chara.physics.settle_cloth(
                    &mut self.chara.scene,
                    rk_chara::physics::SETTLE_FRAMES,
                    rk_chara::physics::FRAME_TIME,
                );
            }
        }
        self.cloth_owed = if lazy { rk_chara::physics::SETTLE_FRAMES } else { 0 };
        self.settle_cloth(CLOTH_FRAMES);
        self.phase = phase;
        true
    }

    /// Settles the cloth nets for up to `frames` of the frames they are
    /// still owed.
    fn settle_cloth(&mut self, frames: u32) {
        let frames = self.cloth_owed.min(frames);
        if frames > 0 {
            self.chara
                .physics
                .settle_cloth(&mut self.chara.scene, frames, rk_chara::physics::FRAME_TIME);
            self.cloth_owed -= frames;
        }
    }
}

/// CharaStudio's table of FK bones, read from the game once.
impl Live {
    /// A character's face controllers.
    pub fn face_rig(&self, id: ObjectId) -> Option<&rk_chara::face::Rig> {
        self.actors.iter().find(|a| a.id == id)?.chara.face.as_ref()
    }
}

/// How far a place may move, in metres, and still count as where it was:
/// re-posing one character moves the others' bones by rounding dust, and
/// reading that dust as a change would settle them again every frame.
const STILL: f32 = 1e-4;

/// What BetterPenetration aims a shaft with in a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct AimJob {
    entry: Vec3,
    end: Vec3,
    settings: rk_chara::better_penetration::Settings,
    vaginal: bool,
    /// Her body, when the shaft's end follows it rather than her waist.
    body: Option<BodyAim>,
}

impl AimJob {
    /// Whether two aims are the same but for rounding dust.
    fn near(a: Option<&AimJob>, b: Option<&AimJob>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.settings == b.settings
                    && a.vaginal == b.vaginal
                    && a.entry.distance(b.entry) <= STILL
                    && a.end.distance(b.end) <= STILL
                    && a.body.is_some() == b.body.is_some()
            }
            _ => false,
        }
    }
}

/// What of her body the main game's constraint reads: her front and back
/// rows of collision points, the place's up, and her inner target.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BodyAim {
    front: [rk_chara::body_collision::CollisionPoint; 3],
    back: [rk_chara::body_collision::CollisionPoint; 3],
    up: Vec3,
    inner: Vec3,
}

impl BodyAim {
    fn of(
        mode: rk_chara::better_penetration::Mode,
        world: &[Mat4],
        names: &std::collections::HashMap<String, usize>,
    ) -> Option<BodyAim> {
        use rk_chara::body_collision as bc;
        let points = bc::CollisionPoints::find(world, |n| names.get(n).copied(), &bc::BodyOptions::default())?;
        Some(BodyAim {
            front: points.front.try_into().ok()?,
            back: points.back.try_into().ok()?,
            up: world[*names.get(mode.entry_bone())?]
                .y_axis
                .truncate()
                .normalize_or_zero(),
            inner: world[*names.get(bc::INNER_TARGET)?].w_axis.truncate(),
        })
    }

    /// The shaft's end as the main game constrains it to her body, its
    /// root where it is posed now.
    fn end(&self, job: &AimJob, start: Vec3, base_length: f32) -> Vec3 {
        use rk_chara::better_penetration::Mode;
        use rk_chara::body_collision as bc;
        let points = bc::CollisionPoints {
            front: self.front.to_vec(),
            back: self.back.to_vec(),
        };
        let dan = bc::Dan {
            start,
            base_length,
            // A link is made only while the shaft is in her.
            penetration: true,
            bp_points: true,
            last_end: Vec3::ZERO,
            last_end_vector: Vec3::ZERO,
        };
        let partner = bc::Partner {
            reference_position: job.entry,
            reference_up: self.up,
            inner_position: self.inner,
            points: &points,
        };
        let mode = if job.vaginal { Mode::Vaginal } else { Mode::Anal };
        let options = bc::BodyOptions::default();
        bc::adjust_dan_to_target(
            mode,
            &dan,
            &partner,
            &job.settings,
            &options,
            rk_chara::better_penetration::PULL_STEP,
        )
        .end
    }
}

/// Where a character's shaft is aimed this frame: the partner, the place
/// on her, and BetterPenetration's two targets in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aim {
    pub partner: ObjectId,
    pub mode: rk_chara::better_penetration::Mode,
    pub entry: Vec3,
    pub end: Vec3,
}

impl Live {
    /// The aim of the shaft of the actor at `at`, from the others as they
    /// are posed now: the partner and place chosen in the scene, or the
    /// nearest the settings' auto-target finds, then the two targets on her
    /// bones. The actors with a shaft are updated after the others, so a
    /// partner is posed for this frame.
    /// BetterPenetration's settings of the actor at `at`: the scene's, or
    /// its card's.
    fn settings_of(&self, at: usize, scene: &Scene) -> Option<rk_chara::better_penetration::Settings> {
        let actor = &self.actors[at];
        actor.chara.shaft.as_ref()?;
        match scene.get(actor.id).map(|o| &o.content) {
            Some(Content::Character { better_penetration, .. }) => Some(
                better_penetration
                    .as_deref()
                    .copied()
                    .unwrap_or(actor.chara.better_penetration),
            ),
            _ => None,
        }
    }

    fn aim_of(&self, at: usize, scene: &Scene, held: Option<Aim>) -> Option<(Aim, AimJob)> {
        use rk_chara::better_penetration as bp;
        let actor = &self.actors[at];
        let shaft = actor.chara.shaft.as_ref()?;
        let Some(Content::Character {
            better_penetration,
            penetration_target,
            ..
        }) = scene.get(actor.id).map(|o| &o.content)
        else {
            return None;
        };
        let settings = better_penetration
            .as_deref()
            .copied()
            .unwrap_or(actor.chara.better_penetration);
        if !settings.enabled {
            return None;
        }
        // The partner chosen by hand, the plugin's auto-target, or
        // Renkatsu's own link.
        let (partner, mode) = match (penetration_target, bp::Mode::of_auto(settings.auto_target)) {
            (Some(t), _) => (Some(t.partner), t.mode),
            (None, Some(mode)) => (None, mode),
            (None, None) if settings.auto_link => {
                let (partner, mode) = self.auto_link(at, shaft, held)?;
                (Some(partner), mode)
            }
            (None, None) => return None,
        };
        let posed = |a: &Actor| (a.chara.scene.world().0, bp::bones_by_name(&a.chara.scene, a.chara.top));
        let partner = match partner {
            Some(p) => p,
            None => {
                let tip = shaft.tip(&actor.chara.scene.world().0);
                let candidates = self
                    .actors
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != at)
                    .filter_map(|(_, a)| {
                        let (world, names) = posed(a);
                        Some((a.id, world[*names.get(mode.entry_bone())?].w_axis.truncate()))
                    });
                bp::nearest(tip, shaft.base_length, candidates)?
            }
        };
        let her = self.actors.iter().find(|a| a.id == partner && a.id != actor.id)?;
        let (world, names) = posed(her);
        let (entry, end) = bp::targets(mode, &world, |n| names.get(n).copied())?;
        Some((
            Aim {
                partner,
                mode,
                entry,
                end,
            },
            AimJob {
                entry,
                end,
                settings,
                vaginal: mode == bp::Mode::Vaginal,
                body: (settings.body_collision && mode != bp::Mode::Oral)
                    .then(|| BodyAim::of(mode, &world, &names))
                    .flatten(),
            },
        ))
    }

    /// Renkatsu's own link for the shaft of the actor at `at`: where the
    /// game's H position he plays aims it in its present state, on the
    /// nearest character in reach; none when that state has him out of
    /// her; and for an animation the game does not know, the nearest of
    /// the three places in reach. The link of the last frame holds while
    /// its place stays in reach. Only a male's shown shaft links on its own.
    fn auto_link(
        &self,
        at: usize,
        shaft: &rk_chara::better_penetration::Shaft,
        held: Option<Aim>,
    ) -> Option<(ObjectId, rk_chara::better_penetration::Mode)> {
        use rk_chara::better_penetration as bp;
        let actor = &self.actors[at];
        // Only a male's shown shaft links on its own: a body may carry the
        // bones with CharaStudio's Son switch off, and a female's card says
        // on where Studio starts her off. A female's links by hand.
        if !actor.male || !actor.chara.clothes.son.switched_on {
            return None;
        }
        let wanted = actor
            .h_position
            .as_ref()
            .and_then(|(controller, state)| self.dan_aims.as_ref()?.aim(controller, state));
        let modes: Vec<bp::Mode> = match wanted {
            Some(None) => return None,
            Some(Some(mode)) => vec![mode],
            None => bp::Mode::ALL.to_vec(),
        };
        let mut places = Vec::new();
        for (i, other) in self.actors.iter().enumerate() {
            if i == at {
                continue;
            }
            let world = other.chara.scene.world().0;
            let names = bp::bones_by_name(&other.chara.scene, other.chara.top);
            for &mode in &modes {
                if let Some(&bone) = names.get(mode.entry_bone()) {
                    places.push((other.id, mode, world[bone].w_axis.truncate()));
                }
            }
        }
        let tip = shaft.tip(&actor.chara.scene.world().0);
        bp::nearest_place(tip, shaft.base_length, held.map(|h| (h.partner, h.mode)), &places)
    }

    /// The H position an animation is of, as the male's controller and
    /// the state: named in the scene for one played from the game, or
    /// found in CharaStudio's list by the group, category and state its
    /// file was taken from.
    fn h_position_of(&mut self, project: &Project, a: &crate::scene::Animation) -> Option<(String, String)> {
        if !a.override_controller.is_empty() {
            return Some((a.override_controller.clone(), a.state.clone()));
        }
        let file = own_file(project, a)?;
        if let Some(known) = self.h_positions.get(&file) {
            return known.clone();
        }
        let found = (|| {
            let meta = rk_chara::animation_file::read_meta(&file).ok()?;
            let library = self.h_library.as_ref()?;
            if !meta.h {
                return None;
            }
            library
                .entries
                .iter()
                .find(|e| {
                    e.h && e.state == meta.state
                        && e.controller == meta.controller
                        && library.group_name(e.group) == meta.group
                        && {
                            // The list names a category after its group.
                            let category = library.category_name(e.group, e.category);
                            category == meta.category || category == format!("{}: {}", meta.group, meta.category)
                        }
                })
                .map(|e| (e.override_controller.clone(), e.state.clone()))
        })();
        // Not cached before the list is read, so that it is tried again.
        if self.h_library.is_some() {
            self.h_positions.insert(file, found.clone());
        }
        found
    }

    /// The heights H pairs play at: a character playing the male's half of
    /// an H position plays it at the height of the one playing the female's
    /// half of the same position, as `HActionBase` sets the male's `height`
    /// to the female's body. CharaStudio leaves each at its own, so the two
    /// halves of a pair of different heights miss each other.
    fn pair_heights(&mut self, scene: &Scene, project: &Project, games: &mut Games, mods: &Path) {
        let halves: Vec<Option<(bool, String)>> = (0..self.actors.len())
            .map(|i| match scene.get(self.actors[i].id).map(|o| &o.content) {
                Some(Content::Character { animation: Some(a), .. }) => self.h_half_of(project, a, games, mods),
                _ => None,
            })
            .collect();
        for i in 0..self.actors.len() {
            let paired = match &halves[i] {
                Some((false, position)) => halves
                    .iter()
                    .position(|h| h.as_ref().is_some_and(|(female, p)| *female && p == position))
                    .map(|j| self.actors[j].chara.parameters.height),
                _ => None,
            };
            self.actors[i].paired_height = paired;
        }
    }

    /// Whether an animation is the female's half of an H position, and the
    /// position by its category's name: named in CharaStudio's list for one
    /// played from the game, or in its file.
    fn h_half_of(
        &mut self,
        project: &Project,
        a: &crate::scene::Animation,
        games: &mut Games,
        mods: &Path,
    ) -> Option<(bool, String)> {
        let file = own_file(project, a);
        if let Some(known) = file.as_ref().and_then(|f| self.h_halves.get(f)) {
            return known.clone();
        }
        if file.is_none() && a.override_controller.is_empty() {
            return None;
        }
        if self.h_library.is_none() {
            let game = games.get(mods).ok()?;
            self.h_library = Some(std::sync::Arc::new(rk_chara::animations::Library::read(game)));
        }
        let library = self.h_library.clone()?;
        let half = |group: &str, category: &str| {
            let category = category.rsplit(": ").next().unwrap_or(category).to_owned();
            (group.starts_with("Female"), category)
        };
        let found = match &file {
            Some(f) => rk_chara::animation_file::read_meta(f)
                .ok()
                .filter(|m| m.h)
                .map(|m| half(&m.group, &m.category)),
            None => library
                .entries
                .iter()
                .find(|e| e.h && e.override_controller == a.override_controller && e.state == a.state)
                .map(|e| {
                    half(
                        &library.group_name(e.group),
                        &library.category_name(e.group, e.category),
                    )
                }),
        };
        if let Some(f) = file {
            self.h_halves.insert(f, found.clone());
        }
        found
    }

    /// Where a character's shaft is aimed this frame, if anywhere.
    pub fn aim(&self, id: ObjectId) -> Option<Aim> {
        self.aims.iter().find(|(who, _)| *who == id).map(|(_, aim)| *aim)
    }
}

/// A set of pattern pictures of a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pictures {
    Eyebrows,
    Eyes,
    Mouth,
}

impl Pictures {
    /// The picture's size in pixels.
    pub fn size(self) -> [u32; 2] {
        let _ = self;
        [96, 48]
    }
}

/// The background of the pattern pictures.
const PICTURE_BACKGROUND: [f64; 4] = [0.16, 0.16, 0.17, 1.0];

impl Live {
    /// A character's pictures of a set, made on a coming frame when they
    /// are not yet.
    pub fn pictures(&mut self, id: ObjectId, set: Pictures) -> Option<&[(i32, Arc<egui::ColorImage>)]> {
        let actor = self.actors.iter_mut().find(|a| a.id == id)?;
        if !actor.pictures.contains_key(&set) && !actor.pictures_wanted.contains(&set) {
            actor.pictures_wanted.push(set);
        }
        actor.pictures.get(&set).map(Vec::as_slice)
    }

    /// Makes the pictures asked for: the character with each pattern in
    /// turn, seen up close, then as she was.
    fn make_pictures(&mut self, scene: &Scene) {
        for at in 0..self.actors.len() {
            let Some(set) = self.actors[at].pictures_wanted.pop() else {
                continue;
            };
            let mut env = self.environment(scene, PICTURE_BACKGROUND);
            let size = set.size();
            let actor = &mut self.actors[at];
            let mut made = Vec::new();
            let face_set = set;
            let (Some(rig), Some(camera)) = (actor.chara.face.clone(), face_camera(&actor.chara, face_set)) else {
                actor.pictures.insert(set, Vec::new());
                continue;
            };
            lift_light(&mut self.renderer, &actor.chara, &camera, &mut env, size);
            let now = actor.layer.as_ref().and_then(|l| l.face);
            let base = now.unwrap_or(rig.start);
            let (eyebrows, mouths) = rig.counts();
            let patterns: Vec<i32> = match face_set {
                Pictures::Eyebrows => (0..eyebrows as i32).collect(),
                Pictures::Eyes => rig.eye_choices(),
                _ => (0..mouths as i32).collect(),
            };
            for pattern in patterns {
                let mut face = base;
                match face_set {
                    Pictures::Eyebrows => face.eyebrows = pattern,
                    Pictures::Eyes => {
                        face.eyes = pattern;
                        face.blink = true;
                    }
                    _ => {
                        face.mouth = pattern;
                        // Open enough for the pattern's shape to show.
                        face.mouth_open = face.mouth_open.max(0.5);
                    }
                }
                rig.show(&mut actor.chara.scene, Some(&face));
                let draws = actor.chara.scene.pose();
                if let Ok(rgba) = self
                    .renderer
                    .render(&draws, &camera, &env, size[0], size[1], PICTURE_BACKGROUND)
                {
                    made.push((pattern, picture(size, &rgba)));
                }
            }
            rig.show(&mut actor.chara.scene, now.as_ref());
            actor.pictures.insert(set, made);
        }
    }
}

/// How bright a pattern picture should at least be, as the mean of its
/// pixels' lightness from 0 to 1.
const PICTURE_LIGHTNESS: f32 = 0.42;

/// Lights the pictures as the scene does, unless that leaves them too dark
/// to tell the patterns apart: then the light and the ambient go up until
/// a picture of the character as she is reaches `PICTURE_LIGHTNESS`, four
/// times at most.
fn lift_light(
    renderer: &mut Renderer,
    chara: &rk_chara::Character,
    camera: &Camera,
    env: &mut Environment,
    size: [u32; 2],
) {
    let draws = chara.scene.pose();
    let Ok(rgba) = renderer.render(&draws, camera, env, size[0], size[1], PICTURE_BACKGROUND) else {
        return;
    };
    let lightness = rgba
        .chunks_exact(4)
        .map(|p| (0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])) / 255.0)
        .sum::<f32>()
        / (rgba.len() / 4).max(1) as f32;
    if lightness < PICTURE_LIGHTNESS {
        let more = (PICTURE_LIGHTNESS / lightness.max(0.05)).min(4.0);
        env.light_intensity *= more;
        env.ambient *= more;
    }
}

/// RGBA bytes as an image for the interface.
fn picture(size: [u32; 2], rgba: &[u8]) -> Arc<egui::ColorImage> {
    Arc::new(egui::ColorImage::from_rgba_unmultiplied(
        [size[0] as usize, size[1] as usize],
        rgba,
    ))
}

/// A camera close in front of the face: on the eyes, a little above them
/// for the eyebrows, below them for the mouth.
fn face_camera(chara: &rk_chara::Character, set: Pictures) -> Option<Camera> {
    let (world, _) = chara.scene.world();
    let eye = |name: &str| Some(world[chara.scene.find(chara.top, name)?].w_axis.truncate());
    let eyes = (eye("cf_J_Eye_tx_L")? + eye("cf_J_Eye_tx_R")?) * 0.5;
    let face = world[chara.scene.find(chara.top, "cf_J_FaceBase")?];
    let forward = face.z_axis.truncate().normalize_or_zero();
    let up = face.y_axis.truncate().normalize_or_zero();
    let target = match set {
        Pictures::Eyebrows => eyes + up * 0.018,
        Pictures::Mouth => eye("cf_J_MouthMove")?,
        _ => eyes,
    };
    // Both eyes across the picture; the mouth closer.
    let fov_y_deg = if set == Pictures::Mouth { 9.0 } else { 15.0 };
    // Seen in the face's own space, so that no pose or turn of the
    // character tips the picture over, with the same slight tilt in all.
    let tilted = Quat::from_axis_angle(forward, PICTURE_TILT.to_radians()) * up;
    Some(Camera {
        eye: target + forward * 0.3,
        target,
        fov_y_deg,
        up: tilted,
    })
}

/// How far the face pictures lean, in degrees.
const PICTURE_TILT: f32 = 8.0;

/// What an actor asks to have read from the game.
#[derive(Debug, Clone, Copy)]
pub enum Load {
    /// Its animation.
    Animation,
    /// A hand's pattern, by hand (0 left, 1 right) and number.
    Hand(usize, i32),
}

/// CharaStudio's hand patterns of the game and its mods, read once.
pub fn hand_lists(game: &rk_vfs::Game) -> &'static rk_chara::hand_pattern::Lists {
    HAND_LISTS.get_or_init(|| rk_chara::hand_pattern::read(game))
}

/// The hand patterns, once something has read them.
pub fn hand_lists_read() -> Option<&'static rk_chara::hand_pattern::Lists> {
    HAND_LISTS.get()
}

static HAND_LISTS: std::sync::OnceLock<rk_chara::hand_pattern::Lists> = std::sync::OnceLock::new();

fn fk_table(game: &rk_vfs::Game) -> &'static std::collections::BTreeMap<i32, rk_chara::bone_info::BoneInfo> {
    static TABLE: std::sync::OnceLock<std::collections::BTreeMap<i32, rk_chara::bone_info::BoneInfo>> =
        std::sync::OnceLock::new();
    TABLE.get_or_init(|| rk_chara::bone_info::read(game))
}

/// The animation file of the project that a scene's animation names,
/// when it names one that is there. A link of Renkatsu 0.1.4 is no such
/// file: it plays from what is installed.
pub fn own_file(project: &Project, animation: &crate::scene::Animation) -> Option<PathBuf> {
    let file = animation.file.as_deref()?;
    let path = project.resolve(Path::new(file));
    let ours = path
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case(rk_chara::animation_file::EXTENSION));
    (ours && path.is_file()).then_some(path)
}

/// What the Properties panel shows about a built character.
#[derive(Debug, Clone)]
pub struct Built {
    pub outfits: usize,
    /// The names More Outfits gives the outfits, by outfit number.
    pub outfit_names: std::collections::BTreeMap<usize, String>,
    pub warnings: usize,
    /// What the worn clothes say about their states.
    pub worn: cs::Worn,
    /// The animation the character is in: whether it holds one pose,
    /// whether it loops and how long one pass is.
    pub animation: Option<(bool, bool, f32)>,
    /// Why the character's animation could not be read.
    pub animation_problem: Option<String>,
    /// The body blend the character's own card gives its animations.
    pub parameters: rk_chara::motion::Parameters,
    /// The body carries the mask liquid levels are read against. The
    /// game's male body does not, so nothing of them shows on it.
    pub liquid_mask: bool,
    /// BetterPenetration's settings from the card, when the body has a
    /// shaft.
    pub better_penetration: Option<rk_chara::better_penetration::Settings>,
    /// The body has belly bones a BetterPenetration bulge pushes.
    pub belly: bool,
    /// The body has the bones a vaginal or anal pull moves.
    pub pull_bones: bool,
}

pub struct Live {
    /// Cut-out edges are smoothed, from the settings.
    pub smooth_edges: bool,
    renderer: Renderer,
    render_state: egui_wgpu::RenderState,
    actors: Vec<Actor>,
    /// Each shaft's aim this frame, by character.
    aims: Vec<(ObjectId, Aim)>,
    /// Where the game's H positions aim a shaft, once read, and
    /// CharaStudio's list, which tells the position an animation file was
    /// taken from; the position of each file, once found.
    dan_aims: Option<std::sync::Arc<rk_chara::dan_look::Aims>>,
    h_library: Option<std::sync::Arc<rk_chara::animations::Library>>,
    h_positions: std::collections::HashMap<std::path::PathBuf, Option<(String, String)>>,
    /// The half and the position of each animation file an H pair plays,
    /// once found.
    h_halves: std::collections::HashMap<std::path::PathBuf, Option<(bool, String)>>,
    /// The map as the characters' skirts rest on it, and what it was made
    /// from, so that it is made again only when the map moves.
    obstacles: Option<(ObstaclesKey, std::sync::Arc<rk_chara::physics::cloth::Obstacles>)>,
    /// Characters that could not be built, with what was tried and why, so
    /// that they are not tried again every frame.
    failed: Vec<(ObjectId, Recipe, String)>,
    /// The characters being built, in the order they were asked for.
    pending: Vec<Pending>,
    /// Wait for every build where it is asked for, as scripted pictures
    /// and the previews do, and upload as the draw needs it.
    pub patient: bool,
    /// How the builds show while they run.
    pub show_builds: Show,
    /// Textures of new characters that still wait for the GPU.
    uploads_left: usize,
    target: Option<Target>,
    pub cam: ViewCamera,
    /// An object is being dragged in the viewport: the camera stays where
    /// it is instead of going with what it is focused on.
    pub hold_camera: bool,
    /// The characters outlined as selected, set by the viewport each frame.
    pub outline: Option<Outline>,
    /// The layer the transform gizmo is drawn into.
    gizmo: GizmoPass,
    /// The layer the grid on the ground is drawn into.
    grid: GridPass,
    /// The scene's picture keeps its depth, which the grid is drawn
    /// behind.
    pub keep_depth: bool,
    /// The map's moving objects play (the Play menu's Map objects).
    pub map_motion: bool,
    /// The camera was framed on a character once.
    framed: bool,
    /// The first framing was looked at against the map's walls and
    /// furniture: it is turned to see past them once.
    pub view_checked: bool,
    /// The colour tables of Studio's colour filters, once they were looked
    /// for in the game; empty when the game has none.
    filters: Option<Filters>,
    /// Where the bones are that objects are attached to, as of the last
    /// frame followed.
    poses: Poses,
    /// The scene's map.
    map: MapState,
}

/// How the viewport outlines the selected characters.
pub struct Outline {
    pub objects: Vec<ObjectId>,
    /// Linear RGBA.
    pub color: [f32; 4],
    /// In pixels of the picture.
    pub width: f32,
}

/// Edits by bone name and the FK parts they need, from `Live::baked_edits`.
pub type Baked = (Vec<(String, BoneEdit)>, Vec<Part>);

impl Live {
    /// A renderer on the window's device, with nothing to draw yet.
    /// Gives back the picture egui was shown, when this renderer goes
    /// away before the window does.
    pub fn release(&mut self) {
        if let Some(target) = self.target.take() {
            self.render_state.renderer.write().free_texture(&target.id);
        }
        self.gizmo.release(&self.render_state);
        self.grid.release(&self.render_state);
    }

    /// Another renderer on the same device with a scene of its own, for a
    /// preview beside the viewport. It waits for its builds.
    pub fn sibling(&self) -> Result<Live> {
        let mut live = Live::new(&self.render_state)?;
        live.patient = true;
        Ok(live)
    }

    pub fn new(render_state: &egui_wgpu::RenderState) -> Result<Live> {
        let renderer = Renderer::with_device(render_state.device.clone(), render_state.queue.clone())?;
        // Until a character arrives, the view of a standing person.
        let empty = Camera::frame(Vec3::new(-0.4, 0.0, -0.2), Vec3::new(0.4, 1.5, 0.2), 16.0 / 9.0, 0.0);
        Ok(Live {
            aims: Vec::new(),
            dan_aims: None,
            h_library: None,
            h_positions: Default::default(),
            h_halves: Default::default(),
            smooth_edges: true,
            renderer,
            render_state: render_state.clone(),
            actors: Vec::new(),
            obstacles: None,
            failed: Vec::new(),
            pending: Vec::new(),
            patient: false,
            show_builds: Show::Notice,
            uploads_left: 0,
            target: None,
            cam: ViewCamera::new(Orbit::from_camera(&empty), None),
            hold_camera: false,
            outline: None,
            gizmo: GizmoPass::new(&render_state.device),
            grid: GridPass::new(&render_state.device),
            keep_depth: false,
            map_motion: true,
            framed: false,
            view_checked: true,
            filters: None,
            poses: Poses::default(),
            map: MapState::default(),
        })
    }

    /// Drops everything built, for another project: its scene's objects
    /// may carry the ids of the last one's, and its mods are its own.
    pub fn reset(&mut self) {
        if !self.actors.is_empty() {
            self.renderer.clear_textures();
        }
        self.actors.clear();
        self.failed.clear();
        for pending in self.pending.drain(..) {
            pending.handle.progress.cancel();
        }
        self.uploads_left = 0;
        for focus in [self.cam.focus, self.cam.home_focus()].into_iter().flatten() {
            self.cam.forget(focus.object);
        }
        self.framed = false;
        self.view_checked = false;
        self.filters = None;
        if let Some(pending) = self.map.pending.take() {
            pending.handle.progress.cancel();
        }
        if self.map.set.is_some() {
            self.renderer.set_map(None);
        }
        self.map = MapState::default();
    }

    /// Whether any character is built.
    pub fn has_characters(&self) -> bool {
        !self.actors.is_empty()
    }

    /// How far a character of the scene is, as it was last followed.
    pub fn progress(&self, id: ObjectId) -> Progress {
        if self.map.pending.as_ref().is_some_and(|p| p.id == id) {
            return Progress::Building;
        }
        if self.map.failed.as_ref().is_some_and(|f| f.0 == id) {
            return Progress::Failed;
        }
        if self.map.set.as_ref().is_some_and(|m| m.0 == id) {
            return Progress::Ready(self.map.took);
        }
        if self.pending.iter().any(|p| p.id == id) {
            return Progress::Building;
        }
        if self.failed.iter().any(|f| f.0 == id) {
            return Progress::Failed;
        }
        match self.actors.iter().find(|a| a.id == id) {
            Some(actor) if actor.warm => Progress::Ready(actor.took),
            Some(_) => Progress::Uploading,
            None => Progress::Waiting,
        }
    }

    /// How many textures of new characters still wait for the GPU.
    pub fn uploads_left(&self) -> usize {
        self.uploads_left
    }

    /// Gives the characters that could not be built another try, after
    /// the content they are built from changed.
    pub fn retry(&mut self) {
        self.failed.clear();
        self.map.failed = None;
    }

    /// What the camera can focus on for an object: a built character.
    pub fn subject(&self, id: ObjectId) -> Option<Focus> {
        self.actors.iter().find(|a| a.id == id).map(Actor::subject)
    }

    /// Where a built character's FK bones of the parts shown are in the
    /// world, by name: the handles the Pose tool shows.
    /// Each comes with the place of the nearest bone above it that shows
    /// too, for the line between them.
    pub fn fk_handles(&self, id: ObjectId) -> Option<Vec<(String, Vec3, Option<Vec3>)>> {
        let actor = self.actors.iter().find(|a| a.id == id)?;
        // Only while the character's FK is on, as CharaStudio shows them.
        let layer = actor.layer.as_ref().filter(|l| l.fk_on)?;
        let scene = &actor.chara.scene;
        let (world, active) = scene.world();
        let shown: Vec<&rk_chara::bone_info::FkBone> = actor
            .fk
            .iter()
            .filter(|b| active.get(b.node).copied().unwrap_or(false))
            // The bones of the parts switched on.
            .filter(|b| layer.part_on(&b.info.parts()))
            .collect();
        let at = |node: usize| world[node].w_axis.truncate();
        Some(
            shown
                .iter()
                .map(|b| {
                    let mut up = scene.nodes[b.node].parent;
                    while let Some(n) = up.filter(|n| !shown.iter().any(|s| s.node == *n)) {
                        up = scene.nodes[n].parent;
                    }
                    (b.info.bone.clone(), at(b.node), up.map(at))
                })
                .collect(),
        )
    }

    /// The edits that mirror a character's pose by hand: with `bone` the
    /// bone and the FK bones below it onto their twins on the other side,
    /// without it every FK bone of the parts that are on onto its twin (a
    /// bone in the middle onto itself). By bone name.
    pub fn mirror_edits(&self, id: ObjectId, bone: Option<&str>) -> Option<Vec<(String, Quat)>> {
        let actor = self.actors.iter().find(|a| a.id == id)?;
        let layer = actor.layer.as_ref()?;
        let scene = &actor.chara.scene;
        let top = actor.chara.top;
        let below = |node: usize, of: usize| {
            let mut at = Some(node);
            while let Some(n) = at {
                if n == of {
                    return true;
                }
                at = scene.nodes[n].parent;
            }
            false
        };
        let from = match bone {
            Some(name) => Some(scene.find(top, name)?),
            None => None,
        };
        let pairs: Vec<(usize, usize)> = actor
            .fk
            .iter()
            .filter(|b| layer.part_on(&b.info.parts()))
            .filter(|b| from.is_none_or(|f| below(b.node, f)))
            .filter_map(|b| {
                let twin = scene.find(top, &rk_chara::pose::twin_name(&b.info.bone))?;
                // A copy goes to the other side only.
                (from.is_none() || twin != b.node).then_some((b.node, twin))
            })
            .collect();
        let found = rk_chara::pose::mirrored(scene, &actor.rest, top, &actor.edits, &pairs);
        Some(
            found
                .into_iter()
                .map(|(n, rot)| (scene.nodes[n].name.clone(), rot))
                .collect(),
        )
    }

    /// Where a built character's animation was last posed, in passes.
    pub fn phase(&self, id: ObjectId) -> Option<f32> {
        self.actors.iter().find(|a| a.id == id)?.phase
    }

    /// A built character's own space, which her IK targets are in: the
    /// world of her top node.
    pub fn ik_space(&self, id: ObjectId) -> Option<Mat4> {
        let actor = self.actors.iter().find(|a| a.id == id)?;
        Some(actor.chara.scene.world().0[actor.chara.top])
    }

    /// A built character's IK targets in the world while IK is on: those of
    /// the groups that are on, each with its place and turn.
    pub fn ik_handles(&self, id: ObjectId) -> Option<Vec<(usize, Vec3, Quat)>> {
        let actor = self.actors.iter().find(|a| a.id == id)?;
        let layer = actor
            .layer
            .as_ref()
            .filter(|l| l.ik_on && l.ik.len() == rk_chara::pose::IK_TARGETS)?;
        let own = actor.chara.scene.world().0[actor.chara.top];
        let (_, turn, _) = own.to_scale_rotation_translation();
        Some(
            layer
                .ik
                .iter()
                .enumerate()
                .filter(|(i, _)| layer.ik_group_on(*i))
                .map(|(i, t)| (i, own.transform_point3(t.pos), (turn * t.rot).normalize()))
                .collect(),
        )
    }

    /// A built character's IK targets `which` mirrored onto their twins, as
    /// her pose shows them now, by the twin's place.
    pub fn ik_mirror(&self, id: ObjectId, which: &[usize]) -> Option<Vec<(usize, rk_chara::pose::IkTarget)>> {
        let actor = self.actors.iter().find(|a| a.id == id)?;
        let layer = actor
            .layer
            .as_ref()
            .filter(|l| l.ik_on && l.ik.len() == rk_chara::pose::IK_TARGETS)?;
        Some(actor.chara.ik_rig.as_ref()?.mirrored(&layer.ik, which))
    }

    /// A built character's IK targets on her bones as they stand, as
    /// Studio's copy puts them.
    pub fn ik_targets_on_bones(&self, id: ObjectId) -> Option<Vec<rk_chara::pose::IkTarget>> {
        let actor = self.actors.iter().find(|a| a.id == id)?;
        let rig = actor.chara.ik_rig.as_ref()?;
        Some(rig.targets_on_bones(&actor.chara.scene, actor.chara.top))
    }

    /// The names of a built character's FK bones in `part`.
    pub fn part_bones(&self, id: ObjectId, part: Part) -> Vec<String> {
        self.actors
            .iter()
            .find(|a| a.id == id)
            .map(|a| {
                a.fk.iter()
                    .filter(|b| b.info.parts().contains(&part))
                    .map(|b| b.info.bone.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The edits that hold a character's pose of the frame once its
    /// animation is gone: each bone the animation writes or the hand
    /// edited, from the bones as loaded, by bone name (an edit that does
    /// nothing too, to clear one); and the FK parts the bones turned lie
    /// in, to switch on. Read with the heels off, as the
    /// frame is posed before they go on; the bones are left as they were.
    pub fn baked_edits(&mut self, id: ObjectId) -> Option<Baked> {
        let actor = self.actors.iter_mut().find(|a| a.id == id)?;
        let mut nodes: Vec<usize> = actor.playing.as_ref().map(|p| p.bound.nodes()).unwrap_or_default();
        nodes.extend(actor.edits.iter().map(|&(n, _)| n));
        nodes.sort_unstable();
        nodes.dedup();
        let chara = &mut actor.chara;
        let saved: Vec<Trs> = chara.scene.nodes.iter().map(|n| n.local).collect();
        if let Some(heels) = &chara.heels {
            heels.reset(&mut chara.scene);
        }
        for &(node, _) in &actor.edits {
            chara.scene.nodes[node].local = actor.rest[node];
        }
        if let (Some(playing), Some(phase)) = (&actor.playing, actor.phase) {
            playing.bound.apply(&mut chara.scene, phase);
        }
        rk_chara::pose::apply(&mut chara.scene, &actor.edits);
        let mut parts = Vec::new();
        let mut found = Vec::new();
        for node in nodes {
            let edit = BoneEdit::baked(actor.rest[node], chara.scene.nodes[node].local);
            let name = chara.scene.nodes[node].name.clone();
            if let Some(b) = actor.fk.iter().find(|b| b.node == node).filter(|_| !edit.is_identity()) {
                parts.extend(b.info.parts());
            }
            found.push((name, edit));
        }
        for (node, local) in chara.scene.nodes.iter_mut().zip(saved) {
            node.local = local;
        }
        parts.sort_unstable();
        parts.dedup();
        Some((found, parts))
    }

    /// Where a built character's bone is and how it is turned: its place
    /// and turn in the world, its parent's turn in the world and its own
    /// turn in its parent, for the Pose tool's gizmo.
    pub fn bone_turns(&self, id: ObjectId, name: &str) -> Option<(Vec3, Quat, Quat, Quat)> {
        let actor = self.actors.iter().find(|a| a.id == id)?;
        let scene = &actor.chara.scene;
        let node = scene.find(actor.chara.top, name)?;
        let (world, _) = scene.world();
        let (_, turn, at) = world[node].to_scale_rotation_translation();
        let parent = scene.nodes[node]
            .parent
            .map_or(Quat::IDENTITY, |p| world[p].to_scale_rotation_translation().1);
        Some((at, turn, parent, scene.nodes[node].local.rot))
    }

    /// A built character's joints, the bones a pose edits: the nodes named
    /// `cf_j_` below `cf_j_root`, in tree order, each with its depth among
    /// them.
    pub fn joints(&self, id: ObjectId) -> Option<Vec<(usize, String)>> {
        let actor = self.actors.iter().find(|a| a.id == id)?;
        let scene = &actor.chara.scene;
        let root = scene.find(actor.chara.animator, "cf_j_root")?;
        let mut out = Vec::new();
        let mut stack = vec![(root, 0usize)];
        while let Some((node, depth)) = stack.pop() {
            let n = &scene.nodes[node];
            let joint = n.name.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("cf_j_"));
            if joint {
                out.push((depth, n.name.clone()));
            }
            let below = depth + usize::from(joint);
            stack.extend(n.children.iter().rev().map(|&c| (c, below)));
        }
        Some(out)
    }

    pub fn built(&self, id: ObjectId) -> Option<Built> {
        self.actors.iter().find(|a| a.id == id).map(|a| Built {
            outfits: a.outfits,
            outfit_names: a.outfit_names.clone(),
            warnings: a.chara.warnings.len(),
            worn: a.chara.clothes.worn.clone(),
            animation: a
                .playing
                .as_ref()
                .map(|p| (p.is_static(), p.motion.loops(), p.motion.duration())),
            animation_problem: a.animation_problem.clone(),
            parameters: a.chara.parameters,
            liquid_mask: a.chara.liquid_mask(),
            better_penetration: a.chara.shaft.as_ref().map(|_| a.chara.better_penetration),
            belly: rk_chara::better_penetration::has_belly(&a.chara.physics, &a.chara.scene),
            pull_bones: rk_chara::better_penetration::has_pull_bones(&a.chara.scene, a.chara.top),
        })
    }

    /// Why a character could not be built, when it could not.
    pub fn failure(&self, id: ObjectId) -> Option<&str> {
        if let Some((_, _, why)) = self.map.failed.as_ref().filter(|f| f.0 == id) {
            return Some(why);
        }
        self.failed
            .iter()
            .find(|(failed, ..)| *failed == id)
            .map(|(.., why)| why.as_str())
    }

    /// Brings the built characters in line with the scene, and moves
    /// their animations and physics on by the clock while the scene plays.
    /// The scene names its cards relative to the project, whose mods
    /// `games` opens the game with.
    /// Holds the kinds of physics in `still` at their rest pose, on every
    /// character.
    pub fn hold_physics_still(&mut self, still: &[rk_chara::physics::Belongs]) {
        for actor in &mut self.actors {
            actor.chara.physics.hold_still(&actor.chara.scene, still);
        }
    }

    pub fn follow(
        &mut self,
        scene: &Scene,
        project: &Project,
        games: &mut Games,
        jobs: &mut Jobs,
        clock: Clock,
        log: &mut Log,
    ) {
        let wanted: Vec<(ObjectId, Recipe)> = scene
            .objects()
            .iter()
            .filter_map(|o| match &o.content {
                Content::Character {
                    card,
                    outfit,
                    outdoor_shoes,
                    ..
                } => Some((
                    o.id,
                    Recipe {
                        card: project.resolve(card),
                        outfit: *outfit,
                        outdoor_shoes: *outdoor_shoes,
                    },
                )),
                _ => None,
            })
            .collect();

        // Builds nobody waits for any more.
        self.pending.retain(|p| {
            let keep = wanted.iter().any(|(id, recipe)| *id == p.id && *recipe == p.recipe);
            if !keep {
                p.handle.progress.cancel();
            }
            keep
        });
        // Characters that left the scene. One whose recipe changed stays
        // as it is until the new build takes its place.
        let before = self.actors.len();
        let failed = &self.failed;
        self.actors.retain(|a| {
            wanted.iter().any(|(id, recipe)| {
                *id == a.id && (*recipe == a.recipe || !failed.iter().any(|f| f.0 == *id && f.1 == *recipe))
            })
        });
        if self.actors.len() != before {
            // The renderer keys what it uploaded by address, and a dropped
            // character's addresses can be handed out again.
            self.renderer.clear_textures();
        }
        self.failed
            .retain(|(id, recipe, _)| wanted.iter().any(|w| w.0 == *id && w.1 == *recipe));
        for focus in [self.cam.focus, self.cam.home_focus()].into_iter().flatten() {
            // Other objects than characters keep the focus while they are
            // in the scene.
            let gone = scene.get(focus.object).is_none_or(|o| {
                o.kind() == crate::kinds::Kind::Character && !wanted.iter().any(|(id, _)| *id == focus.object)
            });
            if gone {
                self.cam.forget(focus.object);
            }
        }

        let mods = project.mods_dir();
        for (id, recipe) in wanted {
            let known = |other: ObjectId, tried: &Recipe| other == id && *tried == recipe;
            if self.actors.iter().any(|a| known(a.id, &a.recipe))
                || self.failed.iter().any(|f| known(f.0, &f.1))
                || self.pending.iter().any(|p| known(p.id, &p.recipe))
            {
                continue;
            }
            let name = scene.get(id).map_or("", |o| o.name.as_str()).to_owned();
            // The content is opened by a job of its own; until it is in,
            // the character waits. A patient renderer opens it here.
            let game = if self.patient {
                match games.shared(&mods) {
                    Ok(game) => game,
                    Err(e) => {
                        let why = format!("{e:#}");
                        log.error(format!("{name} could not be built: {why}"));
                        self.failed.push((id, recipe, why));
                        continue;
                    }
                }
            } else {
                match games.ready(&mods) {
                    Some(game) => game,
                    None => continue,
                }
            };
            let want_filters = self.filters.is_none();
            let theirs = recipe.clone();
            // A build that is waited for shows nowhere.
            let show = if self.patient { Show::Quiet } else { self.show_builds };
            let handle = jobs.start(&format!("Loading {name}"), show, true, move |_| {
                let started = std::time::Instant::now();
                let mut actor = Self::build(&game, id, &theirs)?;
                actor.took = started.elapsed().as_secs_f32();
                let filters = want_filters.then(|| rk_chara::build::color_filters(&game));
                Ok(Prepared { actor, filters })
            });
            jobs.name_when_done(&format!("Loaded {name}"));
            self.pending.push(Pending {
                id,
                recipe,
                name,
                handle,
            });
        }

        // What the jobs built, in the order it was asked for: a build that
        // is done waits for the ones asked before it.
        while let Some(first) = self.pending.first() {
            let result = if self.patient {
                first.handle.wait()
            } else {
                match first.handle.try_take() {
                    Some(result) => result,
                    None => break,
                }
            };
            let Pending { id, recipe, name, .. } = self.pending.remove(0);
            match result {
                Ok(Prepared { mut actor, filters }) => {
                    if self.filters.is_none() {
                        self.filters = filters;
                    }
                    let file = recipe.card.file_name().unwrap_or_default().to_string_lossy();
                    log.info(format!("Loaded {name} from {file} in {:.1} s", actor.took));
                    for warning in &actor.chara.warnings {
                        log.warn(format!("{name}: {warning}"));
                    }
                    if !self.framed {
                        // The first character: the camera frames it and
                        // focuses on it, and that is the view to go back to.
                        let subject = actor.subject();
                        let mut home = self.cam.orbit;
                        home.target = subject.center;
                        home.distance = subject.distance;
                        self.cam.rehome(home, Some(subject));
                        self.framed = true;
                        self.view_checked = false;
                    }
                    // The build it replaces, of another recipe, goes now.
                    let before = self.actors.len();
                    self.actors.retain(|a| a.id != id);
                    if self.actors.len() != before {
                        self.renderer.clear_textures();
                    }
                    actor.warm = self.patient;
                    self.actors.push(actor);
                }
                Err(e) => {
                    let why = format!("{e:#}");
                    log.error(format!("{name} could not be built: {why}"));
                    self.failed.push((id, recipe, why));
                }
            }
        }

        // A character comes after what it is attached to, so that one on
        // a bone is put where the bone is in this frame.
        let mut order: Vec<usize> = (0..self.actors.len()).collect();
        // A character with a shaft after the others at its depth, so that
        // a partner is posed when it is aimed at her.
        order.sort_by_key(|i| (scene.depth(self.actors[*i].id), self.actors[*i].chara.shaft.is_some()));
        self.poses.clear();
        // Where the game's H positions aim, read once a shaft is here.
        if self.dan_aims.is_none() && self.actors.iter().any(|a| a.chara.shaft.is_some()) {
            if let Ok(game) = games.get(&mods) {
                self.dan_aims = Some(std::sync::Arc::new(rk_chara::dan_look::Aims::read(game)));
                self.h_library = Some(std::sync::Arc::new(rk_chara::animations::Library::read(game)));
            }
        }
        self.pair_heights(scene, project, games, &mods);
        for i in 0..self.actors.len() {
            if self.actors[i].chara.shaft.is_none() {
                continue;
            }
            let position = match scene.get(self.actors[i].id).map(|o| &o.content) {
                Some(Content::Character { animation: Some(a), .. }) => self.h_position_of(project, a),
                _ => None,
            };
            self.actors[i].h_position = position;
        }
        let held = std::mem::take(&mut self.aims);
        let obstacles = self.obstacles();
        for at in order {
            let id = self.actors[at].id;
            let aim = self.aim_of(at, scene, held.iter().find(|(who, _)| *who == id).map(|(_, a)| *a));
            if let Some((aim, _)) = aim {
                self.aims.push((self.actors[at].id, aim));
            }
            // The balls meet the partner's body while linked to her.
            let balls = aim
                .and_then(|(aim, _)| {
                    let shaft = self.actors[at].chara.shaft.as_ref()?;
                    let her = self.actors.iter().find(|a| a.id == aim.partner)?;
                    Some(shaft.ball_guests(&her.chara.physics, &her.chara.scene))
                })
                .unwrap_or_default();
            let settings = self.settings_of(at, scene);
            let actor = &mut self.actors[at];
            actor.aim_job = aim.map(|(_, job)| job);
            if let (Some(shaft), Some(settings)) = (actor.chara.shaft.as_ref(), settings) {
                shaft.size_colliders(&mut actor.chara.physics, &settings);
            }
            let mut guests = actor.received.clone();
            guests.extend(balls);
            actor.guests_next = Some(guests);
            // A map that came or changed has the skirts settle on it.
            actor.among_changed |= actor.chara.physics.set_obstacles(obstacles.clone());
            let Some(object) = scene.get(actor.id) else { continue };
            let options = match &object.content {
                Content::Character { animation, .. } => animation.as_ref().and_then(|a| a.options),
                _ => None,
            };
            let mut parameters = actor.chara.parameters.with_options(options);
            if let Some(height) = actor.paired_height {
                parameters.height = height;
            }
            let mut read = |what: Load| -> Result<Motion> {
                if let Load::Hand(side, pattern) = what {
                    let game = games.get(&mods)?;
                    let anime = hand_lists(game)[side]
                        .get(&pattern)
                        .with_context(|| format!("the game has no hand pattern {pattern}"))?;
                    return motion::load(
                        game,
                        &anime.bundle,
                        &anime.controller,
                        &anime.clip,
                        None,
                        &Default::default(),
                    );
                }
                let Content::Character {
                    animation: Some(animation),
                    ..
                } = &object.content
                else {
                    anyhow::bail!("the character has no animation");
                };
                // The project's own file first; what is installed plays
                // an animation the project never copied.
                if let Some(file) = own_file(project, animation) {
                    return rk_chara::animation_file::Stored::read(&file)?.motion(&parameters);
                }
                if animation.state.is_empty() {
                    anyhow::bail!(
                        "{} is gone from the project",
                        animation.file.as_deref().unwrap_or("its file")
                    );
                }
                motion::load(
                    games.get(&mods)?,
                    &animation.bundle,
                    &animation.controller,
                    &animation.state,
                    animation.over(),
                    &parameters,
                )
            };
            let world = scene.world_by(actor.id, &self.poses);
            let keys = scene.sequence.track(actor.id).map(|t| &t.keys);
            if actor.update(object, world, &mut read, &clock, keys, self.patient, log) {
                self.cam.follow(actor.subject(), self.hold_camera);
            }
            // The bones that objects follow, now that they are posed.
            let on_bones = |o: &&crate::scene::Object| o.parent == Some(actor.id) && o.bone.is_some();
            let attached: Vec<_> = scene.objects().iter().filter(on_bones).collect();
            if !attached.is_empty() {
                let (bones, _) = actor.chara.scene.world();
                let back = world.inverse();
                for object in attached {
                    let name = object.bone.as_deref().unwrap_or_default();
                    if let Some(node) = actor.chara.scene.find(actor.chara.top, name) {
                        self.poses.set(object.id, back * bones[node]);
                    }
                }
            }
        }
        // Each aimed shaft's colliders for its partner's dynamic bones,
        // which she meets from her next update.
        let mut given: Vec<(ObjectId, rk_chara::physics::Guests)> = Vec::new();
        for &(him, aim) in &self.aims {
            let Some(he) = self.actors.iter().find(|a| a.id == him) else {
                continue;
            };
            let Some(her) = self.actors.iter().find(|a| a.id == aim.partner) else {
                continue;
            };
            let (Some(shaft), Some(job)) = (he.chara.shaft.as_ref(), he.aim_job) else {
                continue;
            };
            let guests = shaft.partner_guests(
                &he.chara.physics,
                &he.chara.scene,
                &her.chara.physics,
                &her.chara.scene,
                aim.mode,
                &job.settings,
            );
            given.push((aim.partner, guests));
        }
        // Each shaft in a mouth pulls it with its strokes, measured from the
        // shaft's root as it is aimed now.
        let mut strokes = Vec::new();
        for &(him, aim) in &self.aims {
            let Some(he) = self.actors.iter().find(|a| a.id == him) else {
                continue;
            };
            let (Some(shaft), Some(job)) = (he.chara.shaft.as_ref(), he.aim_job) else {
                continue;
            };
            let Some(&root) = shaft.points.first() else {
                continue;
            };
            let start = he.chara.scene.world().0[root].w_axis.truncate();
            let oral = aim.mode == rk_chara::better_penetration::Mode::Oral;
            strokes.push((
                him,
                aim.partner,
                start,
                aim.entry,
                shaft.base_length,
                oral,
                job.settings,
            ));
        }
        // The belly bulge's size each partner takes, while a shaft with it
        // on is in her vagina.
        let bulges: Vec<(ObjectId, f32)> = self
            .aims
            .iter()
            .filter(|(_, aim)| aim.mode == rk_chara::better_penetration::Mode::Vaginal)
            .filter_map(|&(him, aim)| {
                let job = self.actors.iter().find(|a| a.id == him)?.aim_job?;
                job.settings
                    .belly_bulge
                    .then_some((aim.partner, job.settings.belly_bulge_scale))
            })
            .collect();
        for actor in &mut self.actors {
            actor.bulge_next = Some(bulges.iter().find(|(id, _)| *id == actor.id).map_or(1.0, |b| b.1));
        }
        let playing = clock.now == Playback::Playing;
        let mut pulled: Vec<(ObjectId, Vec3, f32)> = Vec::new();
        let mut holding: Vec<ObjectId> = Vec::new();
        for actor in &mut self.actors {
            let Some(&(_, partner, start, entry, base_length, oral, settings)) =
                strokes.iter().find(|s| s.0 == actor.id)
            else {
                actor.mouth_pull = None;
                continue;
            };
            let pull = match &mut actor.mouth_pull {
                Some((p, pull)) if *p == partner => pull,
                slot => {
                    &mut slot
                        .insert((partner, rk_chara::better_penetration::MouthPull::new(base_length)))
                        .1
                }
            };
            // A stroke pulls whenever the shaft moves; a return takes time,
            // so it runs while the scene plays.
            let stroking = oral && start.distance(entry) < base_length;
            if stroking && settings.oral_mouth {
                holding.push(partner);
            }
            if !stroking && !playing {
                pull.last_distance = base_length;
                continue;
            }
            if let Some(world) = pull.step(start, entry, base_length, oral, &settings) {
                pulled.push((partner, world, settings.return_rate));
            }
        }
        for actor in &mut self.actors {
            actor.mouth_held_next = holding.contains(&actor.id);
        }
        // The vaginal and anal pulls, by the same rule as the mouth's; with
        // them off, a vaginal stroke still turns her inner bone back each
        // frame, as Studio's `ReturnKokanBones` does.
        let mut holes: Vec<(ObjectId, rk_chara::better_penetration::HolePull)> = Vec::new();
        for &(him, aim) in &self.aims {
            use rk_chara::better_penetration as bp;
            let pulled_body = self.actors.iter().any(|a| a.id == aim.partner && a.hole_bones);
            if aim.mode == bp::Mode::Oral || !pulled_body {
                continue;
            }
            let Some(he) = self.actors.iter_mut().find(|a| a.id == him) else {
                continue;
            };
            let (Some(shaft), Some(job)) = (he.chara.shaft.as_ref(), he.aim_job) else {
                continue;
            };
            let Some(&root) = shaft.points.first() else {
                continue;
            };
            let start = he.chara.scene.world().0[root].w_axis.truncate();
            let base_length = shaft.base_length;
            let end = he.aimed_end.unwrap_or(job.end);
            let (direction, inside) = bp::hole_directions(start, aim.entry, end);
            let settings = job.settings;
            if !settings.pull_holes {
                he.hole_pull = None;
                holes.push((
                    aim.partner,
                    bp::HolePull {
                        mode: aim.mode,
                        amount: 0.0,
                        direction,
                        inside,
                        returning: true,
                    },
                ));
                continue;
            }
            let pull = match &mut he.hole_pull {
                Some((p, m, pull)) if *p == aim.partner && *m == aim.mode => pull,
                slot => &mut slot.insert((aim.partner, aim.mode, bp::MouthPull::new(base_length))).2,
            };
            let stroking = start.distance(aim.entry) < base_length;
            if stroking {
                pull.step(start, aim.entry, base_length, true, &settings);
            } else if playing {
                pull.last_distance = base_length;
                pull.release(&settings, bp::PULL_STEP);
            } else {
                pull.last_distance = base_length;
            }
            holes.push((
                aim.partner,
                bp::HolePull {
                    mode: aim.mode,
                    amount: pull.amount,
                    direction,
                    inside,
                    returning: !stroking,
                },
            ));
        }
        for actor in &mut self.actors {
            actor.hole_next = holes.iter().find(|(id, _)| *id == actor.id).map(|h| h.1);
        }
        for (partner, world, rate) in pulled {
            let Some(her) = self.actors.iter_mut().find(|a| a.id == partner) else {
                continue;
            };
            let Some(node) = her.mouth_node() else {
                continue;
            };
            let bone = her.chara.scene.world().0[node];
            her.mouth_next = Some((rk_chara::better_penetration::mouth_local(&bone, world), rate));
        }
        // Every character's fingers for every vagina, as posed this frame.
        let any_vagina = self.actors.iter().any(|a| !a.vagina.is_empty());
        let fingers: Vec<rk_chara::physics::Shape> = self
            .actors
            .iter()
            .filter(|a| any_vagina && !a.fingers.is_empty())
            .flat_map(|a| rk_chara::better_penetration::finger_shapes(&a.chara.scene, &a.fingers))
            .collect();
        for actor in &mut self.actors {
            actor.received = given
                .iter()
                .filter(|(id, _)| *id == actor.id)
                .flat_map(|(_, g)| g.iter().cloned())
                .chain(actor.vagina.iter().map(|&b| (b, fingers.clone())))
                .collect();
        }
        self.make_pictures(scene);
        self.follow_map(scene, project, jobs, log);
        self.follow_map_filters(project, games, jobs);
        self.send_doors(scene, &clock);
        // Map objects that do not move stand at their first frame.
        self.pose_map(if self.map_motion { clock.time } else { 0.0 });
        self.place_pieces(scene);
    }

    /// Reads which colour filter each time of day of the set map takes,
    /// from its bundle in the game or a package, once per map.
    fn follow_map_filters(&mut self, project: &Project, games: &mut Games, jobs: &mut Jobs) {
        let state = &mut self.map;
        let Some((_, recipe)) = state.set.as_ref() else { return };
        let sidecar = recipe.sidecar.clone();
        let known = state.filters.as_ref().is_some_and(|f| f.0 == sidecar)
            || state.reading_filters.as_ref().is_some_and(|r| r.0 == sidecar);
        if !known {
            let Some(game) = games.ready(&project.mods_dir()) else {
                return;
            };
            let path = sidecar.clone();
            let handle = jobs.start("Reading the map's filters", Show::Quiet, false, move |_| {
                let file = crate::maps::MapFile::read(&path)?;
                let bundle = file.bundles.first().map(|b| b.path.clone()).unwrap_or_default();
                let read = rk_map::scene::time_filters(&game, &bundle)?;
                let shafts = rk_map::scene::time_sun_shafts(&game, &bundle)?;
                let mirrors = rk_map::mirror::mirrors(&game, &bundle).unwrap_or_default();
                let motion = rk_map::motion::read(&game, &bundle).unwrap_or_default();
                let times = MapTime::ALL
                    .into_iter()
                    .map(|t| {
                        let name = read.iter().find(|(time, _)| *time == t.time_of_day());
                        let shaft = shafts.iter().find(|(time, ..)| *time == t.time_of_day());
                        TimeLook {
                            time: t,
                            filter: name.and_then(|(_, n)| n.clone()),
                            shaft_color: shaft.map_or([0.0; 3], |s| s.1),
                            caster: shaft.and_then(|s| s.2),
                        }
                    })
                    .collect();
                Ok((times, mirrors, motion))
            });
            state.reading_filters = Some((sidecar, handle));
        }
        let done = state
            .reading_filters
            .as_ref()
            .and_then(|(_, h)| if self.patient { Some(h.wait()) } else { h.try_take() });
        if let Some(result) = done {
            let (path, _) = state.reading_filters.take().expect("looked at above");
            let (result, mirrors, mut motion) = match result {
                Ok((times, mirrors, motion)) => (Ok(times), mirrors, motion),
                Err(e) => (Err(e), Vec::new(), rk_map::motion::MapMotion::default()),
            };
            if let Some((_, places)) = state.places.as_ref().filter(|(p, _)| *p == path) {
                motion.bind(places);
            }
            state.motion = Some((path.clone(), motion));
            state.posed_at = None;
            if state.set.as_ref().is_some_and(|(_, r)| r.sidecar == path) {
                self.renderer.set_map_mirrors(&mirrors);
            }
            state.mirrors = Some((path.clone(), mirrors));
            // A map whose bundle is not there keeps the first filter.
            state.filters = Some((path, result.unwrap_or_default()));
        }
    }

    /// Poses the set map's moving objects at the scene's time: they play
    /// while it plays, hold while it pauses, and stand at their first
    /// frame while it is stopped, as the game shows them from the start.
    /// The set map's doors, by their Animators' paths, and whether the
    /// game starts each open; once its bundle is read.
    pub fn map_doors(&self) -> Option<Vec<(String, bool)>> {
        let (_, recipe) = self.map.set.as_ref()?;
        let (_, motion) = self.map.motion.as_ref().filter(|(p, _)| *p == recipe.sidecar)?;
        Some(
            motion
                .openings()
                .into_iter()
                .map(|o| (o.object, o.starts_open))
                .collect(),
        )
    }

    /// Sends the map's doors where the scene has them: with their move
    /// while the scene plays and map objects move, at once otherwise, and
    /// back to where they rest when the clock goes back past a move.
    fn send_doors(&mut self, scene: &Scene, clock: &Clock) {
        let Some((map, recipe)) = self.map.set.as_ref() else {
            return;
        };
        let map = *map;
        let sidecar = recipe.sidecar.clone();
        let Some((_, motion)) = self.map.motion.as_mut().filter(|(p, _)| *p == sidecar) else {
            return;
        };
        let openings = motion.openings();
        let moving = self.map_motion && clock.now == crate::state::Playback::Playing;
        let mut changed = false;
        for id in scene.subtree(map) {
            let Some(Content::MapDoor { door, open }) = scene.get(id).map(|o| &o.content) else {
                continue;
            };
            let Some(opening) = openings.iter().find(|o| o.object == *door) else {
                continue;
            };
            let was = self.map.doors_sent.get(door).copied();
            let rewound = was.is_some_and(|(_, at)| at.is_some_and(|at| clock.time < at));
            if was.map(|(o, _)| o) == Some(*open) && !rewound {
                continue;
            }
            // A first sending, or a rewind, puts it where it rests.
            let at = (moving && was.is_some() && !rewound).then_some(clock.time);
            motion.set_open(opening, *open, at);
            self.map.doors_sent.insert(door.clone(), (*open, at));
            changed = true;
        }
        if changed {
            self.map.posed_at = None;
        }
    }

    /// The set map and its movable pieces, once it is built.
    pub fn map_pieces(&self) -> Option<(ObjectId, &[crate::map_pieces::Piece])> {
        let (id, _) = self.map.set.as_ref()?;
        Some((*id, &self.map.pieces))
    }

    /// Draws the map's pieces where the scene has them: moved, turned,
    /// sized, or hidden (drawn at no size, so that they cast nothing
    /// either); a piece taken out of the scene, hidden too. A piece the
    /// scene holds more than once (a duplicate) is drawn once more for
    /// each further one. The renderer takes matrices in the map's space
    /// from where the piece was baked.
    fn place_pieces(&mut self, scene: &Scene) {
        let Some((map, _)) = self.map.set.as_ref() else { return };
        let map = *map;
        let listed = matches!(
            scene.get(map).map(|o| &o.content),
            Some(Content::Map {
                pieces_listed: true,
                ..
            })
        );
        let back = scene.world(map).inverse();
        let mut matrices: Vec<(usize, glam::Mat4)> = Vec::new();
        let mut copies: Vec<(usize, glam::Mat4, ObjectId)> = Vec::new();
        let mut drawn: Vec<usize> = Vec::new();
        for id in scene.subtree(map) {
            let Some(object) = scene.get(id) else { continue };
            let Content::MapPiece { piece, nth, rest } = &object.content else {
                continue;
            };
            let Some(p) = self.map.pieces.iter().find(|p| p.name == *piece && p.nth == *nth) else {
                continue;
            };
            let shown = scene.shown(id);
            let matrix = if shown {
                let m = back * scene.world(id) * glam::Mat4::from_translation(-Vec3::from(*rest));
                // Where the map has it, exactly as baked.
                if m.abs_diff_eq(glam::Mat4::IDENTITY, 1e-5) {
                    glam::Mat4::IDENTITY
                } else {
                    m
                }
            } else {
                glam::Mat4::from_scale(Vec3::ZERO)
            };
            if drawn.contains(&p.index) {
                if shown {
                    copies.push((p.index, matrix, id));
                }
            } else {
                drawn.push(p.index);
                matrices.push((p.index, matrix));
            }
        }
        if listed {
            for p in &self.map.pieces {
                if !drawn.contains(&p.index) {
                    matrices.push((p.index, glam::Mat4::from_scale(Vec3::ZERO)));
                }
            }
        }
        if matrices != self.map.piece_matrices {
            self.renderer.set_map_object_matrices(&matrices);
            self.map.piece_matrices = matrices;
        }
        let same = copies.len() == self.map.copies.len()
            && copies
                .iter()
                .zip(&self.map.copies)
                .all(|(a, b)| a.0 == b.0 && a.1 == b.1);
        if !same {
            let list: Vec<(usize, glam::Mat4)> = copies.iter().map(|c| (c.0, c.1)).collect();
            self.renderer.set_map_copies(&list);
        }
        self.map.copies = copies;
    }

    fn pose_map(&mut self, seconds: f32) {
        let state = &mut self.map;
        let Some((_, recipe)) = state.set.as_ref() else { return };
        let Some((_, motion)) = state.motion.as_ref().filter(|(p, _)| *p == recipe.sidecar) else {
            return;
        };
        if motion.is_empty() || state.posed_at == Some(seconds) {
            return;
        }
        state.posed = motion.pose(seconds);
        self.renderer.set_map_object_matrices(&state.posed);
        state.posed_at = Some(seconds);
    }

    /// What the set map's time of day sets: its colour filter's number
    /// among Studio's, and its sun shafts, their sun put where the map's
    /// object places the map.
    fn map_look(&self, scene: &Scene) -> MapLook {
        let Some((id, recipe)) = self.map.set.as_ref() else {
            return MapLook::default();
        };
        let Some(look) = self
            .map
            .filters
            .as_ref()
            .filter(|(path, _)| *path == recipe.sidecar)
            .and_then(|(_, times)| times.iter().find(|t| t.time == recipe.time))
        else {
            return MapLook::default();
        };
        let filter = look.filter.as_ref().and_then(|name| {
            let filters = self.filters.as_ref()?;
            filters.iter().find(|(_, table)| table.name == *name).map(|(id, _)| *id)
        });
        let place = scene.world(*id);
        let caster = look.caster.map(|c| place.transform_point3(Vec3::from(c)));
        MapLook {
            filter,
            shafts: Some((look.shaft_color, caster)),
        }
    }

    /// Brings the renderer's map in line with the scene's: builds the map
    /// the scene names when it is not the one set, takes it away when the
    /// scene has none, and puts it where its object stands. The map that
    /// is set stays until the one replacing it is built.
    fn follow_map(&mut self, scene: &Scene, project: &Project, jobs: &mut Jobs, log: &mut Log) {
        let wanted = scene.objects().iter().find_map(|o| match &o.content {
            Content::Map {
                map, time, hide_walls, ..
            } if !map.as_os_str().is_empty() => Some((
                o,
                MapRecipe {
                    sidecar: project.resolve(map),
                    time: *time,
                },
                *hide_walls,
            )),
            _ => None,
        });
        let want = wanted.as_ref().map(|(o, recipe, _)| (o.id, recipe.clone()));
        let state = &mut self.map;
        if state
            .pending
            .as_ref()
            .is_some_and(|p| want.as_ref() != Some(&(p.id, p.recipe.clone())))
        {
            if let Some(p) = state.pending.take() {
                p.handle.progress.cancel();
            }
        }
        if state
            .failed
            .as_ref()
            .is_some_and(|f| want.as_ref() != Some(&(f.0, f.1.clone())))
        {
            state.failed = None;
        }
        if want.is_none() && state.set.is_some() {
            self.renderer.set_map(None);
            state.set = None;
            state.ground = None;
        }
        if let Some((id, recipe)) = want.clone() {
            let known =
                state.set.as_ref() == Some(&(id, recipe.clone())) || state.pending.is_some() || state.failed.is_some();
            if !known {
                let name = scene.get(id).map_or("the map", |o| o.name.as_str()).to_owned();
                let compressed = self.renderer.supports_bc();
                let show = if self.patient { Show::Quiet } else { self.show_builds };
                let (sidecar, time) = (recipe.sidecar.clone(), recipe.time);
                let handle = jobs.start(&format!("Loading {name}"), show, true, move |_| {
                    crate::maps::build(&sidecar, time, compressed)
                });
                jobs.name_when_done(&format!("Loaded {name}"));
                state.pending = Some(PendingMap {
                    id,
                    recipe,
                    name,
                    started: std::time::Instant::now(),
                    handle,
                });
            }
        }
        if let Some(pending) = state.pending.as_ref() {
            let result = if self.patient {
                Some(pending.handle.wait())
            } else {
                pending.handle.try_take()
            };
            if let Some(result) = result {
                let PendingMap {
                    id,
                    recipe,
                    name,
                    started,
                    ..
                } = state.pending.take().expect("looked at above");
                match result {
                    Ok(mut map) => {
                        self.renderer.set_map(Some(&map));
                        // The Animators find their objects again in this
                        // build of the map, and pose them anew.
                        let places = rk_map::motion::places(&map);
                        if let Some((_, motion)) = state.motion.as_mut().filter(|(p, _)| *p == recipe.sidecar) {
                            motion.bind(&places);
                        }
                        state.places = Some((recipe.sidecar.clone(), places));
                        state.posed_at = None;
                        // The map file has no mirrors; its bundle's are kept.
                        if let Some((_, mirrors)) = state.mirrors.as_ref().filter(|(p, _)| *p == recipe.sidecar) {
                            self.renderer.set_map_mirrors(mirrors);
                        }
                        state.pieces = crate::map_pieces::movable(&map);
                        state.piece_matrices.clear();
                        state.copies.clear();
                        state.doors_sent.clear();
                        state.ground = Some(MapGround::take(&mut map));
                        state.took = started.elapsed().as_secs_f32();
                        log.info(format!("Loaded {name} in {:.1} s", state.took));
                        state.set = Some((id, recipe));
                    }
                    Err(e) => {
                        let why = format!("{e:#}");
                        log.error(format!("{name} could not be built: {why}"));
                        state.failed = Some((id, recipe, why));
                    }
                }
            }
        }
        // Where the map stands, every frame: moving it uploads nothing.
        let placed = state.set.as_ref().and_then(|(id, _)| {
            let (object, _, hide_walls) = wanted.as_ref().filter(|w| w.0.id == *id)?;
            Some(rk_render::map::MapPlacement {
                root: scene.world_by(object.id, &self.poses),
                hide_walls: *hide_walls,
                shown: scene.shown(object.id),
            })
        });
        // A map being replaced by another object's keeps its own place.
        let placed = placed.or_else(|| {
            let (id, _) = state.set.as_ref()?;
            let object = scene.get(*id)?;
            Some(rk_render::map::MapPlacement {
                root: scene.world_by(object.id, &self.poses),
                hide_walls: true,
                shown: scene.shown(object.id),
            })
        });
        state.shown = placed.is_some_and(|p| p.shown);
        state.placed = placed.filter(|p| p.shown);
        if let Some(placement) = placed {
            self.renderer.place_map(placement);
        }
        self.renderer.set_map_sun(scene.map_sun());
    }

    /// The map's own sun of its time of day, as the unit vector towards it
    /// in the world; none until a map is drawn.
    pub fn map_sun(&self) -> Option<[f32; 3]> {
        self.renderer.map_sun()
    }

    /// Puts the camera at a view and makes it the one to go back to, at
    /// once, focused on nothing.
    pub fn place_camera(&mut self, camera: &Camera) {
        let glide = std::mem::replace(&mut self.cam.glide_time, 0.0);
        self.cam.rehome(Orbit::from_camera(camera), None);
        self.cam.release();
        self.cam.glide_time = glide;
        self.framed = true;
        self.view_checked = true;
    }

    /// Puts the camera at CharaStudio's starting view, which a new map is
    /// first seen from, and makes it the view to go back to. A character
    /// built after leaves it there.
    pub fn studio_camera(&mut self) {
        self.cam.rehome(Orbit::from_camera(&Self::studio_view()), None);
        self.framed = true;
    }

    /// The camera CharaStudio starts a scene with, which a map is first
    /// seen from: at (0, 2.09, 4.21), turned to face the scene's front and
    /// looking down a little, with a 23 degree lens
    /// (`Studio.CameraControl`, as its captures record it).
    pub fn studio_view() -> Camera {
        let eye = Vec3::new(0.0, 2.09, 4.21);
        let turn = glam::Quat::from_xyzw(-1.618_124_7e-8, 0.995_055_6, -0.099_319_79, -1.621_151_3e-7);
        let forward = turn * Vec3::Z;
        // The point it looks at is where its view crosses the scene's
        // middle plane, the one through the origin facing the camera.
        let reach = if forward.z.abs() > 1e-4 {
            -eye.z / forward.z
        } else {
            4.0
        };
        Camera {
            eye,
            target: eye + forward * reach.max(0.5),
            fov_y_deg: 23.0,
            up: glam::Vec3::Y,
        }
    }

    /// Where the bones are that objects are attached to, for the scene.
    pub fn poses(&self) -> &Poses {
        &self.poses
    }

    /// Where a bone of a built character is, in the space of the
    /// character's object.
    pub fn bone(&self, id: ObjectId, name: &str) -> Option<Mat4> {
        self.actors.iter().find(|a| a.id == id)?.bone(name)
    }

    /// The names of a built character's bones and of the points the game
    /// hangs accessories on, in the order of its skeleton.
    pub fn bone_names(&self, id: ObjectId) -> Vec<String> {
        let Some(actor) = self.actors.iter().find(|a| a.id == id) else {
            return Vec::new();
        };
        let scene = &actor.chara.scene;
        let mut names: Vec<String> = Vec::new();
        for node in scene.subtree(actor.chara.top) {
            let name = &scene.nodes[node].name;
            let bone = ["cf_j_", "cm_j_", "a_n_"].iter().any(|start| name.starts_with(start));
            if bone && !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
    }

    /// The lowest point of a built character as it is posed: the middle
    /// of its bounds, at their bottom.
    pub fn lowest(&self, id: ObjectId) -> Option<Vec3> {
        let actor = self.actors.iter().find(|a| a.id == id)?;
        let (min, max) = bounds(&actor.chara.scene.pose());
        Some(Vec3::new((min.x + max.x) * 0.5, min.y, (min.z + max.z) * 0.5))
    }

    /// The map to stand on, while it shows and is not one of `except`.
    fn map_ground(&self, except: &[ObjectId]) -> Option<(&MapGround, rk_render::map::MapPlacement)> {
        let (id, _) = self.map.set.as_ref()?;
        if except.contains(id) {
            return None;
        }
        self.map.ground.as_ref().zip(self.map.placed)
    }

    /// The map as Advanced Skirt Physics sees it: every mesh the camera
    /// sees, where it is in the world now, duplicates included. It is
    /// made again only when the map, its place or its pieces changed.
    fn obstacles(&mut self) -> Option<std::sync::Arc<rk_chara::physics::cloth::Obstacles>> {
        let Some((ground, placed)) = self.map_ground(&[]) else {
            self.obstacles = None;
            return None;
        };
        let key = ObstaclesKey {
            // The map's own meshes: a map read again has new ones.
            ground: ground
                .pieces
                .first()
                .map_or(0, |p| std::sync::Arc::as_ptr(&p.positions) as usize),
            root: placed.rigid(),
            moved: self.map_moved(),
            copies: self.map.copies.iter().map(|(o, m, _)| (*o, *m)).collect(),
        };
        if let Some((was, made)) = &self.obstacles {
            if *was == key {
                return Some(made.clone());
            }
        }
        let made = self.make_obstacles()?;
        self.obstacles = Some((key, made.clone()));
        Some(made)
    }

    fn make_obstacles(&self) -> Option<std::sync::Arc<rk_chara::physics::cloth::Obstacles>> {
        use rk_chara::physics::cloth::{Obstacle, Obstacles};
        let (ground, placed) = self.map_ground(&[])?;
        let root = placed.rigid();
        let moved = self.map_moved();
        let mut pieces = Vec::with_capacity(ground.pieces.len() + self.map.copies.len());
        let obstacle = |piece: &MapPiece, m: glam::Mat4| Obstacle {
            positions: piece.positions.clone(),
            indices: piece.indices.clone(),
            bounds: piece.bounds,
            world: root * m,
        };
        for piece in &ground.pieces {
            let m = moved
                .iter()
                .find(|(o, _)| *o == piece.object)
                .map_or(glam::Mat4::IDENTITY, |(_, m)| *m);
            pieces.push(obstacle(piece, m));
        }
        for (object, m, _) in &self.map.copies {
            if let Some(piece) = ground.pieces.iter().find(|p| p.object == *object) {
                pieces.push(obstacle(piece, *m));
            }
        }
        Some(std::sync::Arc::new(Obstacles { pieces }))
    }

    /// Where the map's objects stand now: posed by their Animators, then
    /// moved as pieces of the scene.
    fn map_moved(&self) -> Vec<(usize, glam::Mat4)> {
        let mut all = self.map.posed.clone();
        for (object, m) in &self.map.piece_matrices {
            all.retain(|(o, _)| o != object);
            all.push((*object, *m));
        }
        all
    }

    /// The map's objects that scene objects in `ids` are: pieces and the
    /// parts of doors.
    fn map_objects_of(&self, scene: &Scene, ids: &[ObjectId]) -> Vec<usize> {
        let mut out = Vec::new();
        for id in ids {
            match scene.get(*id).map(|o| &o.content) {
                Some(Content::MapPiece { piece, nth, .. }) => {
                    if let Some(p) = self.map.pieces.iter().find(|p| p.name == *piece && p.nth == *nth) {
                        out.push(p.index);
                    }
                }
                Some(Content::MapDoor { door, .. }) => out.extend(self.door_objects(door)),
                _ => {}
            }
        }
        out
    }

    /// The map's objects and the copies of them that the scene objects in
    /// `ids` are drawn as: a piece's first scene object is its object, the
    /// others its copies; a door is the objects its Animator moves.
    fn outlined_map(&self, scene: &Scene, ids: &[ObjectId]) -> (Vec<usize>, Vec<usize>) {
        let mut objects = Vec::new();
        let mut copies = Vec::new();
        for id in ids {
            if let Some(c) = self.map.copies.iter().position(|(_, _, owner)| owner == id) {
                copies.push(c);
                continue;
            }
            objects.extend(self.map_objects_of(scene, &[*id]));
        }
        (objects, copies)
    }

    /// The map's objects a door's Animator moves.
    fn door_objects(&self, door: &str) -> Vec<usize> {
        let Some((_, motion)) = self.map.motion.as_ref() else {
            return Vec::new();
        };
        let Some(opening) = motion.openings().into_iter().find(|o| o.object == door) else {
            return Vec::new();
        };
        opening
            .parts
            .iter()
            .filter_map(|p| motion.parts.get(*p)?.object)
            .collect()
    }

    /// The map's object a ray meets first, if the map is nearer than
    /// `nearer`, and the scene's object it is: a piece, a door, or none
    /// for the map itself (a wall, the floor).
    pub fn pick_map(&self, scene: &Scene, origin: Vec3, dir: Vec3) -> Option<(Option<ObjectId>, f32)> {
        let (map, _) = self.map.set.as_ref()?;
        let (ground, placed) = self.map_ground(&[])?;
        let back = placed.rigid().inverse();
        let moved = self.map_moved();
        let (origin, dir) = (back.transform_point3(origin), back.transform_vector3(dir));
        let hit = ground.hit(origin, dir, &moved, &[]);
        // A duplicate is its piece's mesh where its own matrix puts it.
        let copy = self
            .map
            .copies
            .iter()
            .filter_map(|(object, m, id)| {
                let to = m.inverse();
                let t = ground.hit_object(*object, to.transform_point3(origin), to.transform_vector3(dir))?;
                Some((*id, t))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, t)) = copy {
            if hit.is_none_or(|(_, far)| t < far) {
                return Some((Some(id), t));
            }
        }
        let (object, t) = hit?;
        let owner = scene
            .subtree(*map)
            .into_iter()
            .find(|id| match scene.get(*id).map(|o| &o.content) {
                Some(Content::MapPiece { piece, nth, .. }) => self
                    .map
                    .pieces
                    .iter()
                    .any(|p| p.index == object && p.name == *piece && p.nth == *nth),
                Some(Content::MapDoor { door, .. }) => self.door_objects(door).contains(&object),
                _ => false,
            });
        Some((owner, t))
    }

    /// Whether a character's cloth still settles while the scene stands,
    /// or her eyes' highlights shake, so that frames keep being drawn.
    pub fn settling(&self) -> bool {
        self.actors
            .iter()
            .any(|a| a.cloth_owed > 0 || a.layer.as_ref().and_then(|l| l.face).is_some_and(|f| f.shaking))
    }

    /// Whether a map is shown that things can be dropped on, so that the
    /// ground at height 0 is not.
    /// Whether the camera has framed a character since the scene opened.
    pub fn framed(&self) -> bool {
        self.framed
    }

    pub fn on_map(&self) -> bool {
        self.map_ground(&[]).is_some()
    }

    /// The characters and the map as a surface to drop things on,
    /// without `except`.
    pub fn surface<'a>(&'a self, scene: &'a Scene, except: Vec<ObjectId>) -> impl crate::surface::Surface + 'a {
        Meshes {
            live: self,
            scene,
            except,
        }
    }

    /// The half of a build that needs no GPU: the card, its bundles,
    /// meshes and textures. It runs on a job's thread.
    fn build(game: &rk_vfs::Game, id: ObjectId, recipe: &Recipe) -> Result<Actor> {
        let card = rk_card::Card::load(&recipe.card).with_context(|| format!("loading {}", recipe.card.display()))?;
        let options = rk_chara::BuildOptions {
            outfit: recipe.outfit.min(card.coordinates.len().saturating_sub(1)),
            outdoor_shoes: recipe.outdoor_shoes,
            ..Default::default()
        };
        let chara = rk_chara::build(game, &card, &options)?;
        let fk =
            rk_chara::bone_info::on_character(fk_table(game), &chara.scene, chara.animator, chara.top, card.sex() == 1);
        hand_lists(game);
        let rest = chara.scene.nodes.iter().map(|n| n.local).collect();
        let (min, max) = bounds(&chara.scene.pose());
        let home = Camera::frame(min, max, 16.0 / 9.0, 0.0);
        // Fingers push vaginas, this character's own and others'.
        let fingers = rk_chara::better_penetration::finger_nodes(&chara.scene, chara.top);
        let hole_bones = rk_chara::better_penetration::has_pull_bones(&chara.scene, chara.top)
            || ["cf_J_Vagina_Inner", "cf_J_Ana_Root"]
                .iter()
                .any(|n| chara.scene.find(chara.top, n).is_some());
        let vagina = rk_chara::better_penetration::vagina_bones(&chara.physics, &chara.scene);
        Ok(Actor {
            male: card.sex() == 0,
            mouth_held: false,
            mouth_held_next: false,
            hole_bones,
            fingers,
            vagina,
            hole_pull: None,
            aimed_end: None,
            hole_next: None,
            hole_at: None,
            id,
            recipe: recipe.clone(),
            chara,
            rest,
            center: home.target,
            distance: (home.eye - home.target).length(),
            applied: None,
            physics_at: None,
            cloth_owed: 0,
            among_changed: false,
            outfits: card.coordinates.len(),
            outfit_names: rk_chara::more_outfits::names(&card.ext, card.coordinates.len()),
            clothes: [cs::ON; cs::KINDS],
            liquid: ([0; rk_chara::siru::PARTS], true),
            pose: None,
            paired_height: None,
            layer: None,
            edits: Vec::new(),
            playing: None,
            animation_problem: None,
            hand_problems: Vec::new(),
            pictures: Default::default(),
            pictures_wanted: Vec::new(),
            shake_since: std::time::Instant::now(),
            aim_job: None,
            aim_posed: None,
            guests_next: None,
            received: Vec::new(),
            h_position: None,
            mouth_pull: None,
            mouth_at: None,
            mouth_next: None,
            mouth_return: 0.0,
            bulge_next: None,
            phase: None,
            fk,
            warm: false,
            took: 0.0,
        })
    }

    /// The light and the look of the scene for the renderer: the key light
    /// of the scene, or no light at all when the scene has none that shows.
    /// The Koikatsu pipeline draws with the screen effects a new CharaStudio
    /// scene has on. Without a map the effects stay on what is drawn, and
    /// the empty parts keep the plain `background`; a map takes the whole
    /// picture, as in CharaStudio.
    fn environment(&self, scene: &Scene, background: [f64; 4]) -> Environment {
        let map = self.map.shown;
        let mut effects = scene_effects(scene, self.filters.as_ref(), self.map_look(scene));
        effects.backdrop = (!map).then_some(background);
        let mut env = Environment {
            ramp: self.actors.first().and_then(|a| a.chara.ramp.clone()),
            effects: Some(effects),
            camera_depth: self.keep_depth,
            alpha_to_coverage: self.smooth_edges,
            ..Default::default()
        };
        env.map_light_intensity = scene.map_light();
        match scene.key_light() {
            Some(light) => {
                if let Content::Light {
                    from_camera,
                    angles,
                    intensity,
                    shadows,
                    color,
                    ..
                } = light.content
                {
                    env.light_intensity = intensity;
                    env.light_dim = scene.chara_light_dim(light.id);
                    env.light_color = color;
                    env.shadows.enabled = shadows;
                    // CharaStudio's rig, fixed in the scene or kept to
                    // the view.
                    let camera = self.cam.orbit.camera();
                    let follow = from_camera.then_some((camera.eye, camera.target));
                    env.light_dir = Some(crate::scene::chara_light_direction(angles, follow).to_array());
                }
            }
            None => {
                env.light_intensity = 0.0;
                env.shadows.enabled = false;
            }
        }
        env
    }

    /// Draws the scene's characters that show into a texture of `size`
    /// pixels and returns the id egui shows it under. The texture is made
    /// again only when the size changes.
    pub fn paint(&mut self, scene: &Scene, size: [u32; 2], background: [f64; 4]) -> Result<egui::TextureId> {
        let size = [size[0].max(1), size[1].max(1)];
        if self.target.as_ref().is_none_or(|t| t.size != size) {
            let device = &self.render_state.device;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("viewport"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: Renderer::TARGET_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
            });
            // egui takes what it samples as display-ready colour, so it gets
            // the stored sRGB bytes as they are, through a plain view; the
            // renderer still writes through the sRGB format.
            let view = texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                ..Default::default()
            });
            let mut egui_renderer = self.render_state.renderer.write();
            let id = match &self.target {
                Some(old) => {
                    egui_renderer.update_egui_texture_from_wgpu_texture(
                        device,
                        &view,
                        wgpu::FilterMode::Linear,
                        old.id,
                    );
                    old.id
                }
                None => egui_renderer.register_native_texture(device, &view, wgpu::FilterMode::Linear),
            };
            self.target = Some(Target { texture, id, size });
        }
        // A new character's textures go to the GPU a few a frame; it is
        // drawn from the frame they are all there.
        self.uploads_left = 0;
        if let Some(actor) = self.actors.iter_mut().find(|a| !a.warm) {
            let draws = actor.chara.scene.pose();
            self.uploads_left = self.renderer.warm(&draws, UPLOAD_BUDGET);
            actor.warm = self.uploads_left == 0;
        }
        let target = self.target.as_ref().expect("made above");
        let env = self.environment(scene, background);
        let mut env = env;
        let mut draws = Vec::new();
        let mut outlined = Vec::new();
        // The character lights besides the main one, as handed over.
        let mut others: Vec<ObjectId> = Vec::new();
        // The lamps: the lights that are not character lights and light
        // the characters too.
        let lamps = scene.lamps(false);
        env.lamps = lamps.iter().map(|(_, lamp)| *lamp).collect();
        env.map_lamps = scene.lamps(true).into_iter().map(|(_, lamp)| lamp).collect();
        for actor in self.actors.iter().filter(|a| a.warm && scene.shown(a.id)) {
            let from = draws.len();
            draws.extend(actor.chara.scene.pose());
            if self.outline.as_ref().is_some_and(|o| o.objects.contains(&actor.id)) {
                outlined.extend(from..draws.len());
            }
            // The character light of the character's light group; none
            // leaves it to the ambient light.
            let main = scene.key_light().map(|l| l.id);
            let light = match scene.chara_light_of(actor.id) {
                Some(id) if Some(id) == main => DrawLight::Main,
                Some(id) => {
                    let at = match others.iter().position(|o| *o == id) {
                        Some(at) => at,
                        None => {
                            others.push(id);
                            env.other_lights.push(self.other_light(scene, id));
                            others.len() - 1
                        }
                    };
                    DrawLight::Other(at)
                }
                None => DrawLight::Dark,
            };
            env.draw_lights.resize(from, DrawLight::Main);
            env.draw_lights.resize(draws.len(), light);
            let mask = lamps
                .iter()
                .enumerate()
                .filter(|(_, (id, _))| scene.light_reaches(*id, actor.id))
                .fold(0u32, |m, (i, _)| if i < 32 { m | 1 << i } else { m });
            env.draw_lamps.resize(from, u32::MAX);
            env.draw_lamps.resize(draws.len(), mask);
        }
        // Selected map pieces and doors are outlined too: the map's objects
        // and the copies drawn for duplicates.
        let (map_objects, map_copies) = self
            .outline
            .as_ref()
            .map(|o| self.outlined_map(scene, &o.objects))
            .unwrap_or_default();
        let any = !outlined.is_empty() || !map_objects.is_empty() || !map_copies.is_empty();
        if let Some(o) = self.outline.as_ref().filter(|_| any) {
            env.highlight = Some(rk_render::highlight::Highlight {
                draws: outlined,
                map_objects,
                map_copies,
                color: o.color,
                width: o.width,
            });
        }
        self.renderer
            .render_to(&target.texture, &draws, &self.cam.orbit.camera(), &env, background)?;
        Ok(target.id)
    }

    /// A character light besides the main one, as the renderer takes it.
    fn other_light(&self, scene: &Scene, id: ObjectId) -> rk_render::OtherLight {
        let (angles, follows, color, intensity) = match scene.get(id).map(|o| &o.content) {
            Some(Content::Light {
                angles,
                from_camera,
                color,
                intensity,
                ..
            }) => (*angles, *from_camera, *color, *intensity),
            _ => ([0.0; 2], false, [1.0; 3], 0.0),
        };
        let camera = self.cam.orbit.camera();
        let follow = follows.then_some((camera.eye, camera.target));
        rk_render::OtherLight {
            dir: crate::scene::chara_light_direction(angles, follow).to_array(),
            color,
            intensity,
            dim: scene.chara_light_dim(id),
        }
    }

    /// Draws a transform gizmo into its layer of `size` pixels, as the
    /// viewport's camera sees it, and returns the id egui shows it under.
    pub fn paint_gizmo(&mut self, size: [u32; 2], view_proj: Mat4, mesh: &Mesh) -> egui::TextureId {
        let size = [size[0].max(1), size[1].max(1)];
        let eye = self.cam.orbit.camera().eye;
        self.gizmo.paint(&self.render_state, size, view_proj, eye, mesh)
    }

    /// Draws the grid on the ground into its layer of `size` pixels, under
    /// what the scene's picture of that size shows, and returns the id
    /// egui shows it under. `None` before the scene was drawn at that
    /// size.
    pub fn paint_grid(&mut self, size: [u32; 2], view_proj: Mat4, grid: &Grid) -> Option<egui::TextureId> {
        let size = [size[0].max(1), size[1].max(1)];
        if self.target.as_ref().is_none_or(|t| t.size != size) {
            return None;
        }
        let depth = self.renderer.camera_depth()?.clone();
        let eye = self.cam.orbit.camera().eye;
        Some(self.grid.paint(&self.render_state, size, view_proj, eye, &depth, grid))
    }

    /// The character a ray from `origin` along `dir` meets first, by the
    /// triangles of its meshes as they are posed, and how far along the
    /// ray. Worked out on the CPU, which a click can afford.
    pub fn pick(&self, scene: &Scene, origin: Vec3, dir: Vec3) -> Option<(ObjectId, f32)> {
        self.cast(scene, origin, dir, &[])
    }

    /// `pick` among the characters that are not in `except`.
    fn cast(&self, scene: &Scene, origin: Vec3, dir: Vec3, except: &[ObjectId]) -> Option<(ObjectId, f32)> {
        let mut best: Option<(ObjectId, f32)> = None;
        let wanted = |a: &&Actor| scene.shown(a.id) && !except.contains(&a.id);
        for actor in self.actors.iter().filter(wanted) {
            for draw in actor.chara.scene.pose() {
                if draw.renderer.cast_shadows == ShadowCasting::ShadowsOnly {
                    continue;
                }
                let reach = best.map_or(f32::MAX, |(_, t)| t);
                if draw.bounds.is_some_and(|b| !ray_meets_box(origin, dir, b, reach)) {
                    continue;
                }
                let positions = draw.positions();
                let mesh = &draw.renderer.mesh;
                for sub in mesh.submeshes.iter().filter(|s| s.topology == 0) {
                    let first = sub.first_index as usize;
                    let Some(indices) = mesh.indices.get(first..first + sub.index_count as usize) else {
                        continue;
                    };
                    for triangle in indices.chunks_exact(3) {
                        let corner = |i: usize| positions.get(triangle[i] as usize).copied().map(Vec3::from);
                        let (Some(a), Some(b), Some(c)) = (corner(0), corner(1), corner(2)) else {
                            continue;
                        };
                        if let Some(t) = ray_meets_triangle(origin, dir, a, b, c) {
                            if best.is_none_or(|(_, nearest)| t < nearest) {
                                best = Some((actor.id, t));
                            }
                        }
                    }
                }
            }
        }
        best
    }

    /// The shown characters and where their bones are in the world, for
    /// telling which of them a box on the screen touches.
    pub fn bones(&self, scene: &Scene) -> Vec<(ObjectId, Vec<Vec3>)> {
        self.actors
            .iter()
            .filter(|a| scene.shown(a.id))
            .map(|a| {
                let (world, active) = a.chara.scene.world();
                let points = world
                    .iter()
                    .zip(&active)
                    .filter(|(_, active)| **active)
                    .map(|(m, _)| m.w_axis.truncate())
                    .collect();
                (a.id, points)
            })
            .collect()
    }

    /// A small picture of the scene as the viewport's camera will see it
    /// once it is at rest, as RGBA bytes of `size` pixels, for the project
    /// selector. `None` when the scene shows no character: a picture of
    /// nothing says nothing.
    pub fn snapshot(&mut self, scene: &Scene, size: [u32; 2], background: [f64; 4]) -> Result<Option<Vec<u8>>> {
        let draws: Vec<_> = self
            .actors
            .iter()
            .filter(|a| scene.shown(a.id))
            .flat_map(|a| a.chara.scene.pose())
            .collect();
        if draws.is_empty() {
            return Ok(None);
        }
        let env = self.environment(scene, background);
        let camera = self.cam.destination().camera();
        self.renderer
            .render(&draws, &camera, &env, size[0], size[1], background)
            .map(Some)
    }
}

/// The posed meshes of the scene's characters as a surface.
struct Meshes<'a> {
    live: &'a Live,
    scene: &'a Scene,
    except: Vec<ObjectId>,
}

impl crate::surface::Surface for Meshes<'_> {
    fn meet(&self, origin: Vec3, dir: Vec3) -> Option<f32> {
        let characters = self
            .live
            .cast(self.scene, origin, dir, &self.except)
            .map(|(_, far)| far);
        // The map is drawn turned and moved by its root; the ray goes
        // into the map's space the same way, which keeps its lengths.
        let skip = self.live.map_objects_of(self.scene, &self.except);
        let map = self.live.map_ground(&self.except).and_then(|(ground, placed)| {
            let back = placed.rigid().inverse();
            let moved = self.live.map_moved();
            ground
                .hit(
                    back.transform_point3(origin),
                    back.transform_vector3(dir),
                    &moved,
                    &skip,
                )
                .map(|(_, t)| t)
        });
        characters.into_iter().chain(map).min_by(f32::total_cmp)
    }
}

/// Whether a ray passes through a box before it has gone `reach` far.
fn ray_meets_box(origin: Vec3, dir: Vec3, bounds: [[f32; 3]; 2], reach: f32) -> bool {
    let (lo, hi) = (Vec3::from(bounds[0]), Vec3::from(bounds[1]));
    let inv = dir.recip();
    let (a, b) = ((lo - origin) * inv, (hi - origin) * inv);
    let near = a.min(b).max_element();
    let far = a.max(b).min_element();
    far >= near.max(0.0) && near <= reach
}

/// How far along a ray it meets a triangle, from either side.
fn ray_meets_triangle(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (ab, ac) = (b - a, c - a);
    let p = dir.cross(ac);
    let det = ab.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let s = origin - a;
    let u = s.dot(p) / det;
    let q = s.cross(ab);
    let v = dir.dot(q) / det;
    if u < 0.0 || v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = ac.dot(q) / det;
    (t > 0.0).then_some(t)
}

/// The effects the scene's objects ask for: the first of each that shows.
/// An effect no object asks for is off.
/// Renkatsu's own fog on GlobalFog's maths: exponential squared distance
/// fog, which keeps what is near clear and closes in towards
/// `visibility` metres, where it hides 95%; and under `height` a ground
/// mist in which one sees `mist` metres at the floor. GlobalFog's height
/// fog thickens with the depth under its top, by its height density per
/// metre.
fn own_fog(
    color: Option<[f32; 3]>,
    height: f32,
    visibility: Option<f32>,
    mist: Option<f32>,
) -> rk_render::effects::Fog {
    // exp(-(density * way)^2) is 5% left where density * way is the root
    // of 3.
    let lost = 3.0f32.sqrt();
    let first = rk_render::effects::Fog::default();
    let top = height.max(0.05);
    let see = visibility.map(|v| v.max(0.1));
    let mist = mist.map(|m| m.max(0.1));
    // At the floor a metre of the way counts 1 for the distance fog and
    // height density times the top for the mist.
    let (density, height_density) = match (see, mist) {
        (Some(v), Some(m)) if m < v => (lost / v, (v / m - 1.0) / top),
        (Some(v), _) => (lost / v, 0.0),
        (None, Some(m)) => (lost / m, 1.0 / top),
        (None, None) => (0.0, 0.0),
    };
    rk_render::effects::Fog {
        color: color.unwrap_or(first.color),
        color_from_map: color.is_none(),
        height: top,
        start_distance: 0.0,
        height_density,
        height_fog: height_density > 0.0,
        distance_fog: see.is_some(),
        radial: true,
        mode: rk_render::effects::FogMode::ExponentialSquared { density },
        own: true,
        ..first
    }
}

fn scene_effects(scene: &Scene, filters: Option<&Filters>, map: MapLook) -> StudioEffects {
    let map_filter = map.filter;
    use crate::scene::Effect;
    use rk_render::effects::{AmbientOcclusion, Bloom, ColorFilter, Vignette};
    let mut fx = StudioEffects::default();
    // Of two shown effects of a kind, the first in the scene draws.
    let objects: Vec<_> = scene.objects().iter().filter(|o| scene.shown(o.id)).collect();
    for object in objects {
        let Content::Effect { effect } = &object.content else {
            continue;
        };
        match *effect {
            Effect::ColorFilter { filter, blend } if fx.color.is_none() => {
                let number = filter.map_or(map_filter.unwrap_or(0), |f| f as i32);
                let lut = filters.and_then(|f| f.get(&number)).cloned();
                // The filters were made for a lit room: in a dark one their
                // contrast turns colours deep, so the filter fades as the
                // map's light goes, as far as the map's Filter decay says.
                let blend = blend.max(scene.filter_fade());
                fx.color = lut.map(|lut| ColorFilter {
                    lut,
                    exposure: 1.0,
                    blend,
                });
            }
            Effect::Bloom {
                intensity,
                threshold,
                spread,
            } if fx.bloom.is_none() => {
                fx.bloom = Some(Bloom {
                    intensity,
                    threshold,
                    blur_spread: spread,
                    ..Bloom::default()
                });
            }
            Effect::AmbientOcclusion { intensity, radius } if fx.ao.is_none() => {
                fx.ao = Some(AmbientOcclusion {
                    intensity,
                    radius,
                    ..AmbientOcclusion::default()
                });
            }
            Effect::Vignette { intensity, blur } if fx.vignette.is_none() => {
                fx.vignette = Some(Vignette {
                    intensity,
                    blur,
                    ..Vignette::default()
                });
            }
            Effect::DepthOfField { focal_size, aperture } if fx.dof.is_none() => {
                // It focuses where the camera turns around, as the
                // Studio's CameraTarget does.
                fx.dof = Some(rk_render::effects::DepthOfField {
                    focal_size,
                    aperture,
                    ..rk_render::effects::DepthOfField::default()
                });
            }
            Effect::SunShafts { threshold, color } if fx.shafts.is_none() => {
                // A map's own colour and sun for its time of day; without a
                // map, the scene's colour from the middle of the picture.
                let first = rk_render::effects::SunShafts::default();
                let (own, caster) = match (color, map.shafts) {
                    (None, Some((c, caster))) => (c, caster),
                    (Some(c), Some((_, caster))) => (c, caster),
                    (c, None) => (c.unwrap_or(first.color), None),
                };
                fx.shafts = Some(rk_render::effects::SunShafts {
                    threshold,
                    color: own,
                    caster,
                    ..first
                });
            }
            Effect::Fog {
                color,
                height,
                start_distance,
                visibility,
                mist,
            } if fx.fog.is_none() => {
                let first = rk_render::effects::Fog::default();
                fx.fog = Some(if visibility.is_none() && mist.is_none() {
                    // Without a map, the Studio scene's own fog mode; with a
                    // map that has fog, the renderer takes the map's.
                    rk_render::effects::Fog {
                        color: color.unwrap_or(first.color),
                        color_from_map: color.is_none(),
                        height,
                        start_distance,
                        ..first
                    }
                } else {
                    own_fog(color, height, visibility, mist)
                });
            }
            _ => {}
        }
    }
    fx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fog_that_shows_turns_the_studios_fog_on() {
        let mut scene = Scene::empty();
        assert!(scene_effects(&scene, None, MapLook::default()).fog.is_none());
        let fog = scene.add(
            "Fog",
            Content::Effect {
                effect: Effect::Fog {
                    color: None,
                    height: 5.0,
                    start_distance: 2.0,
                    visibility: None,
                    mist: None,
                },
            },
            None,
        );
        let fx = scene_effects(&scene, None, MapLook::default()).fog.expect("on");
        assert!(fx.color_from_map);
        assert_eq!((fx.height, fx.start_distance), (5.0, 2.0));
        // Hidden, it is off, as a map's own starts.
        scene.get_mut(fog).unwrap().visible = false;
        assert!(scene_effects(&scene, None, MapLook::default()).fog.is_none());
    }
    use crate::scene::Effect;

    fn table(name: &str) -> Arc<Image> {
        Arc::new(Image {
            name: name.to_owned(),
            width: 1,
            height: 1,
            rgba: vec![0; 4],
            srgb: false,
            repeat: false,
        })
    }

    #[test]
    fn own_fog_hides_95_percent_at_its_visibility() {
        use rk_render::effects::fog_factor;
        let kept = |fog: &rk_render::effects::Fog, eye_y: f32, way: [f32; 3]| fog_factor(fog, eye_y, way, way[2]);
        let fog = own_fog(None, 1.0, Some(12.0), None);
        assert!((kept(&fog, 1.5, [0.0, 0.0, 12.0]) - 0.05).abs() < 0.01);
        // Clearer than plain exponential fog half way.
        assert!(kept(&fog, 1.5, [0.0, 0.0, 6.0]) > 0.4);
        // In ground mist, one sees its own distance at the floor.
        let mist = own_fog(None, 1.0, Some(200.0), Some(30.0));
        let at_floor = kept(&mist, 0.0, [0.0, 0.0, 30.0]);
        assert!((at_floor - 0.05).abs() < 0.02, "{at_floor}");
        // Above the mist, far clearer.
        assert!(kept(&mist, 3.0, [0.0, 0.0, 30.0]) > 0.9);
    }

    #[test]
    fn the_scenes_filter_is_the_maps_own_and_follows_its_time() {
        let filters: Filters = [(0, table("Lut_Amount")), (1, table("Lut_TimeSunset1"))].into();
        let mut scene = Scene::new();
        let lut = |scene: &Scene, map: Option<i32>| {
            scene_effects(
                scene,
                Some(&filters),
                MapLook {
                    filter: map,
                    shafts: None,
                },
            )
            .color
            .map(|c| c.lut.name.clone())
        };
        // A new scene's filter is the map's own: Studio's first without a
        // map, the time's with one.
        assert_eq!(lut(&scene, None).as_deref(), Some("Lut_Amount"));
        assert_eq!(lut(&scene, Some(1)).as_deref(), Some("Lut_TimeSunset1"));
        // Of two shown filters, the first in the scene draws.
        let own = scene.add(
            "Colour filter 2",
            Content::Effect {
                effect: Effect::ColorFilter {
                    filter: Some(0),
                    blend: 0.0,
                },
            },
            None,
        );
        assert_eq!(lut(&scene, Some(1)).as_deref(), Some("Lut_TimeSunset1"));
        // With the first hidden, the second.
        let first = scene
            .objects()
            .iter()
            .find(|o| {
                o.id != own
                    && matches!(
                        o.content,
                        Content::Effect {
                            effect: Effect::ColorFilter { .. }
                        }
                    )
            })
            .unwrap()
            .id;
        scene.get_mut(first).unwrap().visible = false;
        assert_eq!(lut(&scene, Some(1)).as_deref(), Some("Lut_Amount"));
    }
}
