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
use glam::{Mat4, Vec3};
use rk_chara::clothes_state::{self as cs, States};
use rk_chara::motion::{self, Bound, Motion};
use rk_chara::scene::{ShadowCasting, Trs};
use rk_chara::Image;
use rk_render::{bounds, Camera, Environment, Renderer, StudioEffects};
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
    recipe: Recipe,
    chara: rk_chara::Character,
    /// Every node's local transform as loaded, which a stop goes back to.
    rest: Vec<Trs>,
    /// The middle of the character as loaded and the distance that fits it.
    center: Vec3,
    distance: f32,
    /// Where the object is in the scene, as last given to the root.
    applied: Option<Mat4>,
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
    pose: Option<[String; 5]>,
    /// That animation, when it could be read.
    playing: Option<Playing>,
    /// Why it could not be read.
    animation_problem: Option<String>,
    /// The point of the animation the bones were last posed at.
    phase: Option<f32>,
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
    placed: Option<rk_render::map::MapPlacement>,
}

/// The meshes of a map the camera sees, in the map's own space, kept
/// after the upload for Snap to floor.
struct MapGround {
    pieces: Vec<MapPiece>,
}

struct MapPiece {
    positions: Vec<[f32; 3]>,
    indices: Vec<u32>,
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
                positions: std::mem::take(&mut mesh.positions),
                indices: std::mem::take(&mut mesh.indices),
                bounds: mesh.bounds,
            });
        }
        Self { pieces }
    }

    /// How far a ray in the map's own space goes before it meets the map.
    fn meet(&self, origin: Vec3, dir: Vec3) -> Option<f32> {
        let mut best: Option<f32> = None;
        for piece in &self.pieces {
            let reach = best.unwrap_or(f32::MAX);
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
                    if best.is_none_or(|nearest| t < nearest) {
                        best = Some(t);
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
    /// The colour table of Studio's first colour filter, when the build
    /// was asked to look for it.
    filter: Option<Option<Arc<Image>>>,
}

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
}

impl Clock {
    /// A scene that stands still.
    pub fn stopped() -> Clock {
        Clock {
            was: Playback::Stopped,
            now: Playback::Stopped,
            dt: 0.0,
            time: 0.0,
        }
    }
}

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
            self.chara.physics.reset(&self.chara.scene);
        }
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

    /// Puts the bones back as they were loaded.
    fn back_to_rest(&mut self) {
        for (node, rest) in self.chara.scene.nodes.iter_mut().zip(&self.rest) {
            node.local = *rest;
        }
        self.applied = None;
    }

    /// Brings the character in line with its object for one frame: the
    /// clothes states, the pose or animation, the place, and the physics.
    /// `game` reads an animation that is not loaded yet. Returns whether
    /// the character moved as a whole.
    fn update(
        &mut self,
        object: &crate::scene::Object,
        world: Mat4,
        game: &mut dyn FnMut() -> Result<Motion>,
        clock: &Clock,
        log: &mut Log,
    ) -> bool {
        let Content::Character {
            simulate,
            clothes,
            liquid,
            liquid_clothes,
            animation: pose,
            ..
        } = &object.content
        else {
            return false;
        };
        // What has to be posed again from the start.
        let mut again = clock.was != Playback::Stopped && clock.now == Playback::Stopped;

        // The states the scene asks for, as far as the outfit has them.
        let states = cs::fitted(clothes, &self.chara.clothes.worn);
        if states != self.clothes {
            self.chara.set_clothes(&states);
            self.clothes = states;
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
            ]
        });
        if key != self.pose {
            self.pose = key;
            self.playing = None;
            self.animation_problem = None;
            if let Some(pose) = pose {
                match game() {
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
            if let (Some(playing), Some(phase)) = (&self.playing, phase) {
                self.chara.animate(&playing.bound, phase, &states);
            }
            if *simulate {
                self.chara.physics.end_frame(&mut self.chara.scene, clock.dt);
            }
            self.phase = phase;
            return moved;
        }
        if !again && phase == self.phase {
            return self.place(world, false);
        }
        // Nothing plays: the character is posed once and holds still.
        self.back_to_rest();
        self.place(world, false);
        self.chara.refresh_heels(&states);
        if let (Some(playing), Some(phase)) = (&self.playing, phase) {
            self.chara.animate(&playing.bound, phase, &states);
            self.chara.physics.reset(&self.chara.scene);
            if *simulate {
                // Hair and skirts settle on the pose, as they settled on
                // the character when it was built.
                for _ in 0..rk_chara::physics::SETTLE_FRAMES {
                    self.chara.physics.begin_frame(&mut self.chara.scene);
                    self.chara.animate(&playing.bound, phase, &states);
                    self.chara
                        .physics
                        .end_frame(&mut self.chara.scene, rk_chara::physics::FRAME_TIME);
                }
            }
        }
        self.phase = phase;
        true
    }
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
}

pub struct Live {
    /// Cut-out edges are smoothed, from the settings.
    pub smooth_edges: bool,
    renderer: Renderer,
    render_state: egui_wgpu::RenderState,
    actors: Vec<Actor>,
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
    /// The camera was framed on a character once.
    framed: bool,
    /// The colour table of Studio's first colour filter, once it was looked
    /// for in the game; the inner `None` when the game has none.
    filter: Option<Option<Arc<Image>>>,
    /// Where the bones are that objects are attached to, as of the last
    /// frame followed.
    poses: Poses,
    /// The scene's map.
    map: MapState,
}

/// The colour filter a new CharaStudio scene uses (`SceneInfo.aceNo`).
const DEFAULT_COLOR_FILTER: i32 = 0;

/// How the viewport outlines the selected characters.
pub struct Outline {
    pub objects: Vec<ObjectId>,
    /// Linear RGBA.
    pub color: [f32; 4],
    /// In pixels of the picture.
    pub width: f32,
}

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
            smooth_edges: true,
            renderer,
            render_state: render_state.clone(),
            actors: Vec::new(),
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
            framed: false,
            filter: None,
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
        self.filter = None;
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
            let want_filter = self.filter.is_none();
            let theirs = recipe.clone();
            // A build that is waited for shows nowhere.
            let show = if self.patient { Show::Quiet } else { self.show_builds };
            let handle = jobs.start(&format!("Loading {name}"), show, true, move |_| {
                let started = std::time::Instant::now();
                let mut actor = Self::build(&game, id, &theirs)?;
                actor.took = started.elapsed().as_secs_f32();
                let filter = want_filter.then(|| rk_chara::build::color_filter(&game, DEFAULT_COLOR_FILTER));
                Ok(Prepared { actor, filter })
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
                Ok(Prepared { mut actor, filter }) => {
                    if self.filter.is_none() {
                        self.filter = filter;
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
        order.sort_by_key(|i| scene.depth(self.actors[*i].id));
        self.poses.clear();
        for at in order {
            let actor = &mut self.actors[at];
            let Some(object) = scene.get(actor.id) else { continue };
            let options = match &object.content {
                Content::Character { animation, .. } => animation.as_ref().and_then(|a| a.options),
                _ => None,
            };
            let parameters = actor.chara.parameters.with_options(options);
            let mut read = || -> Result<Motion> {
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
            if actor.update(object, world, &mut read, &clock, log) {
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
        self.follow_map(scene, project, jobs, log);
    }

    /// Brings the renderer's map in line with the scene's: builds the map
    /// the scene names when it is not the one set, takes it away when the
    /// scene has none, and puts it where its object stands. The map that
    /// is set stays until the one replacing it is built.
    fn follow_map(&mut self, scene: &Scene, project: &Project, jobs: &mut Jobs, log: &mut Log) {
        let wanted = scene.objects().iter().find_map(|o| match &o.content {
            Content::Map { map, time, hide_walls } if !map.as_os_str().is_empty() => Some((
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

    /// Whether a map is shown that things can be dropped on, so that the
    /// ground at height 0 is not.
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
        let rest = chara.scene.nodes.iter().map(|n| n.local).collect();
        let (min, max) = bounds(&chara.scene.pose());
        let home = Camera::frame(min, max, 16.0 / 9.0, 0.0);
        Ok(Actor {
            id,
            recipe: recipe.clone(),
            chara,
            rest,
            center: home.target,
            distance: (home.eye - home.target).length(),
            applied: None,
            outfits: card.coordinates.len(),
            outfit_names: rk_chara::more_outfits::names(&card.ext, card.coordinates.len()),
            clothes: [cs::ON; cs::KINDS],
            liquid: ([0; rk_chara::siru::PARTS], true),
            pose: None,
            playing: None,
            animation_problem: None,
            phase: None,
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
        let mut effects = scene_effects(scene, self.filter.clone().flatten());
        effects.backdrop = (!map).then_some(background);
        let mut env = Environment {
            ramp: self.actors.first().and_then(|a| a.chara.ramp.clone()),
            effects: Some(effects),
            camera_depth: self.keep_depth,
            alpha_to_coverage: self.smooth_edges,
            ..Default::default()
        };
        match scene.key_light() {
            Some(light) => {
                if let Content::Light {
                    from_camera,
                    intensity,
                    shadows,
                    ..
                } = light.content
                {
                    env.light_intensity = intensity;
                    env.shadows.enabled = shadows;
                    // A light shines along its own forward axis, wherever
                    // its folders turned it.
                    let turn = scene.parent_world(light.id).to_scale_rotation_translation().1;
                    let own = Vec3::from(light.light_direction());
                    env.light_dir = (!from_camera).then(|| (turn * own).to_array());
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
        for actor in self.actors.iter().filter(|a| a.warm && scene.shown(a.id)) {
            let from = draws.len();
            draws.extend(actor.chara.scene.pose());
            if self.outline.as_ref().is_some_and(|o| o.objects.contains(&actor.id)) {
                outlined.extend(from..draws.len());
            }
        }
        if let Some(o) = self.outline.as_ref().filter(|_| !outlined.is_empty()) {
            env.highlight = Some(rk_render::highlight::Highlight {
                draws: outlined,
                color: o.color,
                width: o.width,
            });
        }
        self.renderer
            .render_to(&target.texture, &draws, &self.cam.orbit.camera(), &env, background)?;
        Ok(target.id)
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
        let map = self.live.map_ground(&self.except).and_then(|(ground, placed)| {
            let back = placed.rigid().inverse();
            ground.meet(back.transform_point3(origin), back.transform_vector3(dir))
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
fn scene_effects(scene: &Scene, lut: Option<std::sync::Arc<rk_chara::Image>>) -> StudioEffects {
    use crate::scene::Effect;
    use rk_render::effects::{AmbientOcclusion, Bloom, ColorFilter, Vignette};
    let mut fx = StudioEffects::default();
    for object in scene.objects().iter().filter(|o| scene.shown(o.id)) {
        let Content::Effect { effect } = &object.content else {
            continue;
        };
        match *effect {
            Effect::ColorFilter { .. } if fx.color.is_none() => {
                fx.color = lut.clone().map(|lut| ColorFilter { lut, exposure: 1.0 });
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
            _ => {}
        }
    }
    fx
}
