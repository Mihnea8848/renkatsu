// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The live viewport: a character built from a card, drawn by Renkatsu's
//! renderer on the window's own GPU device into a texture that egui shows,
//! every frame, through the camera of `camera.rs`, and the physics stepping while it plays.

use crate::camera::{Focus, Orbit, ViewCamera};
use anyhow::{Context, Result};
use eframe::egui_wgpu::{self, wgpu};
use rk_render::{bounds, Camera, Environment, Renderer};
use std::path::Path;

struct Target {
    texture: wgpu::Texture,
    id: egui::TextureId,
    size: [u32; 2],
}

pub struct Live {
    renderer: Renderer,
    render_state: egui_wgpu::RenderState,
    chara: rk_chara::Character,
    env: Environment,
    target: Option<Target>,
    /// The camera, focused on the character at the start.
    pub cam: ViewCamera,
    /// The character as something to focus on: its scene row, its middle
    /// and the distance that fits it.
    subject: Focus,
    /// Every node's local transform as loaded, which a stop goes back to.
    rest: Vec<rk_chara::scene::Trs>,
    pub name: String,
}

impl Live {
    /// Builds the card's character and a renderer on the window's device.
    /// `row` is the character's row in the scene.
    pub fn new(render_state: &egui_wgpu::RenderState, card: &Path, row: usize) -> Result<Live> {
        let game = rk_vfs::Game::open(&rk_core::Config::load()?)?;
        let card_data = rk_card::Card::load(card).with_context(|| format!("loading {}", card.display()))?;
        let chara = rk_chara::build(&game, &card_data, &rk_chara::BuildOptions::default())?;
        let renderer = Renderer::with_device(render_state.device.clone(), render_state.queue.clone())?;
        let env = Environment {
            ramp: chara.ramp.clone(),
            ..Default::default()
        };
        let rest = chara.scene.nodes.iter().map(|n| n.local).collect();
        let (min, max) = bounds(&chara.scene.pose());
        let home = Orbit::from_camera(&Camera::frame(min, max, 16.0 / 9.0, 0.0));
        let subject = Focus {
            row,
            center: home.target,
            distance: home.distance,
        };
        Ok(Live {
            renderer,
            render_state: render_state.clone(),
            chara,
            env,
            target: None,
            cam: ViewCamera::new(home, Some(subject)),
            subject,
            rest,
            name: card_data.full_name(),
        })
    }

    /// What the camera can focus on for a scene row: the character, when
    /// the row is the character's.
    pub fn subject(&self, row: usize) -> Option<Focus> {
        (row == self.subject.row).then_some(self.subject)
    }

    /// Advances the physics by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        self.chara.physics.step(&mut self.chara.scene, dt);
    }

    /// Puts the bones back as they were loaded and the physics on them.
    pub fn reset(&mut self) {
        for (node, rest) in self.chara.scene.nodes.iter_mut().zip(&self.rest) {
            node.local = *rest;
        }
        self.chara.physics.reset(&self.chara.scene);
    }

    /// Draws the character into a texture of `size` pixels and returns the
    /// id egui shows it under. The texture is made again only when the
    /// size changes.
    pub fn paint(&mut self, size: [u32; 2], background: [f64; 4]) -> Result<egui::TextureId> {
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
        let draws = self.chara.scene.pose();
        self.renderer
            .render_to(&target.texture, &draws, &self.cam.orbit.camera(), &self.env, background)?;
        Ok(target.id)
    }
}
