// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The live viewport: the scene's characters built from their cards and
//! drawn by Renkatsu's renderer on the window's own GPU device into a
//! texture that egui shows, every frame, through the camera of
//! `camera.rs`. The scene model says what exists; this follows it, building
//! a character when one appears, dropping it when it leaves, moving its
//! root to the object's transform and stepping its physics while it plays.

use crate::camera::{Focus, Orbit, ViewCamera};
use crate::mods::Games;
use crate::project::Project;
use crate::scene::{Content, ObjectId, Scene, Transform};
use crate::state::{Log, Playback};
use anyhow::{Context, Result};
use eframe::egui_wgpu::{self, wgpu};
use glam::Vec3;
use rk_chara::clothes_state::{self as cs, States};
use rk_chara::motion::{self, Bound, Motion};
use rk_chara::scene::Trs;
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
    /// The object's transform as last given to the root.
    applied: Option<Transform>,
    /// How many outfits the card has.
    outfits: usize,
    /// The clothes states the character was last brought to.
    clothes: States,
    /// The animation of the scene the actor last looked for, by bundle,
    /// controller and state.
    pose: Option<[String; 3]>,
    /// That animation, when it could be read.
    playing: Option<Playing>,
    /// Why it could not be read.
    pose_problem: Option<String>,
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
        !self.motion.loops() && self.motion.duration() <= rk_chara::poses::POSE_SECONDS
    }

    /// The point of the animation that shows at the scene's time. A static
    /// pose shows its last frame, which is where the game leaves it.
    fn phase(&self, pose: &crate::scene::Pose, time: f32) -> f32 {
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
        let t = self.applied.unwrap_or_default();
        Focus {
            object: self.id,
            center: t.matrix().transform_point3(self.center),
            distance: self.distance * t.largest_scale().max(1e-3),
        }
    }

    /// Puts the character's root where the object's transform says. The
    /// physics is put back on the bones unless it is running, when the
    /// move is something for it to react to.
    fn place(&mut self, transform: &Transform, running: bool) -> bool {
        if self.applied == Some(*transform) {
            return false;
        }
        let top = self.chara.top;
        let (scale, rot, pos) = (transform.matrix() * self.rest[top].matrix()).to_scale_rotation_translation();
        self.chara.scene.nodes[top].local = Trs { pos, rot, scale };
        if !running {
            self.chara.physics.reset(&self.chara.scene);
        }
        self.applied = Some(*transform);
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
        game: &mut dyn FnMut() -> Result<Motion>,
        clock: &Clock,
        log: &mut Log,
    ) -> bool {
        let Content::Character {
            simulate,
            clothes,
            pose,
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

        let key = pose
            .as_ref()
            .map(|p| [p.bundle.clone(), p.controller.clone(), p.state.clone()]);
        if key != self.pose {
            self.pose = key;
            self.playing = None;
            self.pose_problem = None;
            if let Some(pose) = pose {
                match game() {
                    Ok(motion) => {
                        let bound = motion.bind(&motion::Skeleton::of(&self.chara.scene, self.chara.animator));
                        self.playing = Some(Playing { motion, bound });
                    }
                    Err(e) => {
                        let why = format!("{e:#}");
                        log.error(format!(
                            "{}: the pose {} could not be read: {why}",
                            object.name, pose.name
                        ));
                        self.pose_problem = Some(why);
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
            let moved = self.place(&object.transform, true);
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
            return self.place(&object.transform, false);
        }
        // Nothing plays: the character is posed once and holds still.
        self.back_to_rest();
        self.place(&object.transform, false);
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

/// What the Properties panel shows about a built character.
#[derive(Debug, Clone)]
pub struct Built {
    pub outfits: usize,
    pub warnings: usize,
    /// What the worn clothes say about their states.
    pub worn: cs::Worn,
    /// The animation the character is in: whether it holds one pose,
    /// whether it loops and how long one pass is.
    pub pose: Option<(bool, bool, f32)>,
    /// Why the character's pose could not be read.
    pub pose_problem: Option<String>,
}

pub struct Live {
    renderer: Renderer,
    render_state: egui_wgpu::RenderState,
    actors: Vec<Actor>,
    /// Characters that could not be built, with what was tried and why, so
    /// that they are not tried again every frame.
    failed: Vec<(ObjectId, Recipe, String)>,
    target: Option<Target>,
    pub cam: ViewCamera,
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
    pub fn new(render_state: &egui_wgpu::RenderState) -> Result<Live> {
        let renderer = Renderer::with_device(render_state.device.clone(), render_state.queue.clone())?;
        // Until a character arrives, the view of a standing person.
        let empty = Camera::frame(Vec3::new(-0.4, 0.0, -0.2), Vec3::new(0.4, 1.5, 0.2), 16.0 / 9.0, 0.0);
        Ok(Live {
            renderer,
            render_state: render_state.clone(),
            actors: Vec::new(),
            failed: Vec::new(),
            target: None,
            cam: ViewCamera::new(Orbit::from_camera(&empty), None),
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
            pose: a
                .playing
                .as_ref()
                .map(|p| (p.is_static(), p.motion.loops(), p.motion.duration())),
            pose_problem: a.pose_problem.clone(),
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
                let Content::Character { pose: Some(pose), .. } = &object.content else {
                    anyhow::bail!("the character has no pose");
                };
                motion::load(
                    games.get(&mods)?,
                    &pose.bundle,
                    &pose.controller,
                    &pose.state,
                    &parameters,
                )
            };
            if actor.update(object, &mut read, &clock, log) {
                self.cam.follow(actor.subject());
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
            pose_problem: None,
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
                    env.light_dir = (!from_camera).then(|| light.light_direction());
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
