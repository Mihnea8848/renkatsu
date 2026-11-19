// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The live viewport: the scene's characters built from their cards and
//! drawn by Renkatsu's renderer on the window's own GPU device into a
//! texture that egui shows, every frame, through the camera of
//! `camera.rs`. The scene model says what exists; this follows it, building
//! a character when one appears, dropping it when it leaves, moving its
//! root to the object's transform and stepping its physics while it plays.

use crate::camera::{Focus, Orbit, ViewCamera};
use crate::gizmo_pass::{GizmoPass, Mesh};
use crate::mods::Games;
use crate::project::Project;
use crate::scene::{Content, ObjectId, Scene};
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
    /// The clothes states the character was last brought to.
    clothes: States,
    /// The animation of the scene the actor last looked for, by its file
    /// in the project, bundle, controller and state.
    pose: Option<[String; 4]>,
    /// That animation, when it could be read.
    playing: Option<Playing>,
    /// Why it could not be read.
    animation_problem: Option<String>,
    /// The point of the animation the bones were last posed at.
    phase: Option<f32>,
}

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

        let key = pose.as_ref().map(|p| {
            [
                p.file.clone().unwrap_or_default(),
                p.bundle.clone(),
                p.controller.clone(),
                p.state.clone(),
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
    pub warnings: usize,
    /// What the worn clothes say about their states.
    pub worn: cs::Worn,
    /// The animation the character is in: whether it holds one pose,
    /// whether it loops and how long one pass is.
    pub animation: Option<(bool, bool, f32)>,
    /// Why the character's animation could not be read.
    pub animation_problem: Option<String>,
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
    target: Option<Target>,
    pub cam: ViewCamera,
    /// An object is being dragged in the viewport: the camera stays where
    /// it is instead of going with what it is focused on.
    pub hold_camera: bool,
    /// The layer the transform gizmo is drawn into.
    gizmo: GizmoPass,
    /// The camera was framed on a character once.
    framed: bool,
    /// The colour table of Studio's first colour filter, once it was looked
    /// for in the game; the inner `None` when the game has none.
    filter: Option<Option<Arc<Image>>>,
}

/// The colour filter a new CharaStudio scene uses (`SceneInfo.aceNo`).
const DEFAULT_COLOR_FILTER: i32 = 0;

impl Live {
    /// A renderer on the window's device, with nothing to draw yet.
    /// Gives back the picture egui was shown, when this renderer goes
    /// away before the window does.
    pub fn release(&mut self) {
        if let Some(target) = self.target.take() {
            self.render_state.renderer.write().free_texture(&target.id);
        }
        self.gizmo.release(&self.render_state);
    }

    /// Another renderer on the same device with a scene of its own, for a
    /// preview beside the viewport.
    pub fn sibling(&self) -> Result<Live> {
        Live::new(&self.render_state)
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
            target: None,
            cam: ViewCamera::new(Orbit::from_camera(&empty), None),
            hold_camera: false,
            gizmo: GizmoPass::new(&render_state.device),
            framed: false,
            filter: None,
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
        for focus in [self.cam.focus, self.cam.home_focus()].into_iter().flatten() {
            self.cam.forget(focus.object);
        }
        self.framed = false;
        self.filter = None;
    }

    /// Whether any character is built.
    pub fn has_characters(&self) -> bool {
        !self.actors.is_empty()
    }

    /// Gives the characters that could not be built another try, after
    /// the content they are built from changed.
    pub fn retry(&mut self) {
        self.failed.clear();
    }

    /// What the camera can focus on for an object: a built character.
    pub fn subject(&self, id: ObjectId) -> Option<Focus> {
        self.actors.iter().find(|a| a.id == id).map(Actor::subject)
    }

    pub fn built(&self, id: ObjectId) -> Option<Built> {
        self.actors.iter().find(|a| a.id == id).map(|a| Built {
            outfits: a.outfits,
            warnings: a.chara.warnings.len(),
            worn: a.chara.clothes.worn.clone(),
            animation: a
                .playing
                .as_ref()
                .map(|p| (p.is_static(), p.motion.loops(), p.motion.duration())),
            animation_problem: a.animation_problem.clone(),
        })
    }

    /// Why a character could not be built, when it could not.
    pub fn failure(&self, id: ObjectId) -> Option<&str> {
        self.failed
            .iter()
            .find(|(failed, ..)| *failed == id)
            .map(|(.., why)| why.as_str())
    }

    /// Brings the built characters in line with the scene, and moves
    /// their animations and physics on by the clock while the scene plays.
    /// The scene names its cards relative to the project, whose mods
    /// `games` opens the game with.
    pub fn follow(&mut self, scene: &Scene, project: &Project, games: &mut Games, clock: Clock, log: &mut Log) {
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

        // Characters that left the scene, or whose recipe changed.
        let before = self.actors.len();
        self.actors
            .retain(|a| wanted.iter().any(|(id, recipe)| *id == a.id && *recipe == a.recipe));
        if self.actors.len() != before {
            // The renderer keys what it uploaded by address, and a dropped
            // character's addresses can be handed out again.
            self.renderer.clear_textures();
        }
        self.failed
            .retain(|(id, recipe, _)| wanted.iter().any(|w| w.0 == *id && w.1 == *recipe));
        for focus in [self.cam.focus, self.cam.home_focus()].into_iter().flatten() {
            if !wanted.iter().any(|(id, _)| *id == focus.object) {
                self.cam.forget(focus.object);
            }
        }

        for (id, recipe) in wanted {
            let known = |other: ObjectId, tried: &Recipe| other == id && *tried == recipe;
            if self.actors.iter().any(|a| known(a.id, &a.recipe)) || self.failed.iter().any(|f| known(f.0, &f.1)) {
                continue;
            }
            let name = scene.get(id).map_or("", |o| o.name.as_str());
            let started = std::time::Instant::now();
            match Self::build(games, &project.mods_dir(), id, &recipe) {
                Ok(actor) => {
                    if self.filter.is_none() {
                        let game = games.get(&project.mods_dir()).ok();
                        self.filter = Some(game.and_then(|g| rk_chara::build::color_filter(g, DEFAULT_COLOR_FILTER)));
                    }
                    let file = recipe.card.file_name().unwrap_or_default().to_string_lossy();
                    log.info(format!(
                        "Loaded {name} from {file} in {:.1} s",
                        started.elapsed().as_secs_f32()
                    ));
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
                    self.actors.push(actor);
                }
                Err(e) => {
                    let why = format!("{e:#}");
                    log.error(format!("{name} could not be built: {why}"));
                    self.failed.push((id, recipe, why));
                }
            }
        }

        let mods = project.mods_dir();
        for actor in &mut self.actors {
            let Some(object) = scene.get(actor.id) else { continue };
            let parameters = actor.chara.parameters;
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
            if actor.update(object, scene.world(actor.id), &mut read, &clock, log) {
                self.cam.follow(actor.subject(), self.hold_camera);
            }
        }
    }

    fn build(games: &mut Games, mods: &Path, id: ObjectId, recipe: &Recipe) -> Result<Actor> {
        let game = games.get(mods)?;
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
            clothes: [cs::ON; cs::KINDS],
            pose: None,
            playing: None,
            animation_problem: None,
            phase: None,
        })
    }

    /// The light and the look of the scene for the renderer: the key light
    /// of the scene, or no light at all when the scene has none that shows.
    /// The Koikatsu pipeline draws with the screen effects a new CharaStudio
    /// scene has on. Without a map the effects stay on what is drawn, and
    /// the empty parts keep the plain `background`; a map takes the whole
    /// picture, as in CharaStudio.
    fn environment(&self, scene: &Scene, background: [f64; 4]) -> Environment {
        let map = scene
            .objects()
            .iter()
            .any(|o| matches!(o.content, Content::Map) && scene.shown(o.id));
        let mut effects = StudioEffects::studio(self.filter.clone().flatten());
        effects.backdrop = (!map).then_some(background);
        let mut env = Environment {
            ramp: self.actors.first().and_then(|a| a.chara.ramp.clone()),
            effects: Some(effects),
            alpha_to_coverage: self.smooth_edges,
            ..Default::default()
        };
        match scene.key_light() {
            Some(light) => {
                if let Content::Light {
                    from_camera,
                    intensity,
                    shadows,
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
        let target = self.target.as_ref().expect("made above");
        let env = self.environment(scene, background);
        let draws: Vec<_> = self
            .actors
            .iter()
            .filter(|a| scene.shown(a.id))
            .flat_map(|a| a.chara.scene.pose())
            .collect();
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

    /// The character a ray from `origin` along `dir` meets first, by the
    /// triangles of its meshes as they are posed, and how far along the
    /// ray. Worked out on the CPU, which a click can afford.
    pub fn pick(&self, scene: &Scene, origin: Vec3, dir: Vec3) -> Option<(ObjectId, f32)> {
        let mut best: Option<(ObjectId, f32)> = None;
        for actor in self.actors.iter().filter(|a| scene.shown(a.id)) {
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
