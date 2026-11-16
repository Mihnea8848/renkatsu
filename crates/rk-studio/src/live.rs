// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The live viewport: the scene's characters built from their cards and
//! drawn by Renkatsu's renderer on the window's own GPU device into a
//! texture that egui shows, every frame, through the camera of
//! `camera.rs`. The scene model says what exists; this follows it, building
//! a character when one appears, dropping it when it leaves, moving its
//! root to the object's transform and stepping its physics while it plays.

use crate::camera::{Focus, Orbit, ViewCamera};
use crate::scene::{Content, ObjectId, Scene, Transform};
use crate::state::{Log, Physics};
use anyhow::{Context, Result};
use eframe::egui_wgpu::{self, wgpu};
use glam::Vec3;
use rk_chara::scene::Trs;
use rk_render::{bounds, Camera, Environment, Renderer};
use std::path::PathBuf;

struct Target {
    texture: wgpu::Texture,
    id: egui::TextureId,
    size: [u32; 2],
}

/// What a character is built from: a change to any of it builds it again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Recipe {
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
}

/// What the Properties panel shows about a built character.
#[derive(Debug, Clone, Copy)]
pub struct Built {
    pub outfits: usize,
    pub warnings: usize,
}

pub struct Live {
    renderer: Renderer,
    render_state: egui_wgpu::RenderState,
    /// The game's files, opened when the first character is built.
    game: Option<rk_vfs::Game>,
    actors: Vec<Actor>,
    /// Characters that could not be built, with what was tried and why, so
    /// that they are not tried again every frame.
    failed: Vec<(ObjectId, Recipe, String)>,
    target: Option<Target>,
    pub cam: ViewCamera,
    /// The camera was framed on a character once.
    framed: bool,
}

impl Live {
    /// A renderer on the window's device, with nothing to draw yet.
    pub fn new(render_state: &egui_wgpu::RenderState) -> Result<Live> {
        let renderer = Renderer::with_device(render_state.device.clone(), render_state.queue.clone())?;
        // Until a character arrives, the view of a standing person.
        let empty = Camera::frame(Vec3::new(-0.4, 0.0, -0.2), Vec3::new(0.4, 1.5, 0.2), 16.0 / 9.0, 0.0);
        Ok(Live {
            renderer,
            render_state: render_state.clone(),
            game: None,
            actors: Vec::new(),
            failed: Vec::new(),
            target: None,
            cam: ViewCamera::new(Orbit::from_camera(&empty), None),
            framed: false,
        })
    }

    /// Whether any character is built.
    pub fn has_characters(&self) -> bool {
        !self.actors.is_empty()
    }

    /// What the camera can focus on for an object: a built character.
    pub fn subject(&self, id: ObjectId) -> Option<Focus> {
        self.actors.iter().find(|a| a.id == id).map(Actor::subject)
    }

    pub fn built(&self, id: ObjectId) -> Option<Built> {
        self.actors.iter().find(|a| a.id == id).map(|a| Built {
            outfits: a.outfits,
            warnings: a.chara.warnings.len(),
        })
    }

    /// Why a character could not be built, when it could not.
    pub fn failure(&self, id: ObjectId) -> Option<&str> {
        self.failed
            .iter()
            .find(|(failed, ..)| *failed == id)
            .map(|(.., why)| why.as_str())
    }

    /// Brings the built characters in line with the scene, and moves the
    /// physics on by `dt` seconds while it plays. `was` is the physics
    /// state of the frame before.
    pub fn follow(&mut self, scene: &Scene, was: Physics, physics: Physics, dt: f32, log: &mut Log) {
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
                        card: card.clone(),
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
            match self.build(id, &recipe) {
                Ok(actor) => {
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
                        self.cam = ViewCamera::new(home, Some(subject));
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

        let stopped = was != Physics::Stopped && physics == Physics::Stopped;
        let running = physics == Physics::Playing;
        for actor in &mut self.actors {
            let Some(object) = scene.get(actor.id) else { continue };
            if stopped {
                actor.back_to_rest();
            }
            if actor.place(&object.transform, running) {
                self.cam.follow(actor.subject());
            }
            let simulate = matches!(object.content, Content::Character { simulate: true, .. });
            if running && simulate {
                actor.chara.physics.step(&mut actor.chara.scene, dt);
            }
        }
    }

    fn build(&mut self, id: ObjectId, recipe: &Recipe) -> Result<Actor> {
        if self.game.is_none() {
            self.game = Some(rk_vfs::Game::open(&rk_core::Config::load()?)?);
        }
        let game = self.game.as_ref().expect("opened above");
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
        })
    }

    /// The light and the look of the scene for the renderer: the key light
    /// of the scene, or no light at all when the scene has none that shows.
    fn environment(&self, scene: &Scene) -> Environment {
        let mut env = Environment {
            ramp: self.actors.first().and_then(|a| a.chara.ramp.clone()),
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
        let env = self.environment(scene);
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
}
