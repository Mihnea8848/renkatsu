// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The live viewport: a character built from a card, drawn by Renkatsu's
//! renderer on the window's own GPU device into a texture that egui shows,
//! every frame, with an orbit camera and the physics stepping while it plays.

use anyhow::{Context, Result};
use eframe::egui_wgpu::{self, wgpu};
use glam::Vec3;
use rk_render::{bounds, Camera, Environment, Renderer};
use std::path::Path;

/// Where the camera looks from: angles around a point, at a distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    pub target: Vec3,
    /// Around the vertical axis, radians.
    pub yaw: f32,
    /// Above the horizon, radians.
    pub pitch: f32,
    pub distance: f32,
    pub fov_y_deg: f32,
}

impl Orbit {
    /// The orbit that sees a camera's view: the same eye, target and lens.
    pub fn from_camera(c: &Camera) -> Orbit {
        let v = c.eye - c.target;
        let distance = v.length().max(1e-3);
        Orbit {
            target: c.target,
            yaw: v.x.atan2(-v.z),
            pitch: (v.y / distance).clamp(-1.0, 1.0).asin(),
            distance,
            fov_y_deg: c.fov_y_deg,
        }
    }

    pub fn camera(&self) -> Camera {
        let dir = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            -self.yaw.cos() * self.pitch.cos(),
        );
        Camera {
            eye: self.target + dir * self.distance,
            target: self.target,
            fov_y_deg: self.fov_y_deg,
        }
    }

    /// Turns around the target, by a drag in points: dragging right turns
    /// the view right and dragging down tilts it down, as in Unity.
    pub fn rotate(&mut self, drag: egui::Vec2) {
        self.turn(drag, 0.008);
    }

    fn turn(&mut self, drag: egui::Vec2, rate: f32) {
        self.yaw -= drag.x * rate;
        self.pitch = (self.pitch + drag.y * rate).clamp(-1.5, 1.5);
    }

    /// Turns the camera where it stands, by a drag in points, as holding
    /// the right button does in Unity: the eye stays and the target moves.
    pub fn look(&mut self, drag: egui::Vec2) {
        let eye = self.camera().eye;
        self.turn(drag, 0.004);
        let to_eye = self.camera().eye - self.target;
        self.target = eye - to_eye;
    }

    /// Flies the camera and its target together. `along` is right, up and
    /// forward, each -1 to 1; the speed follows the distance to the target
    /// so it suits a close-up and a whole room alike.
    pub fn fly(&mut self, along: Vec3, dt: f32, fast: bool) {
        let cam = self.camera();
        let forward = (cam.target - cam.eye).normalize_or_zero();
        let right = Vec3::Y.cross(forward).normalize_or_zero();
        let speed = self.distance.max(0.5) * if fast { 3.0 } else { 1.0 };
        self.target += (right * along.x + Vec3::Y * along.y + forward * along.z) * speed * dt;
    }

    /// Moves the target across the view, by a drag in points.
    pub fn pan(&mut self, drag: egui::Vec2, view_height: f32) {
        let cam = self.camera();
        let forward = (cam.target - cam.eye).normalize_or_zero();
        let right = Vec3::Y.cross(forward).normalize_or_zero();
        let up = forward.cross(right);
        // The distance a point covers on screen at the target's depth.
        let scale = 2.0 * self.distance * (self.fov_y_deg.to_radians() / 2.0).tan() / view_height.max(1.0);
        self.target += (-right * drag.x + up * drag.y) * scale;
    }

    /// Moves closer for a positive scroll, farther for a negative one.
    pub fn zoom(&mut self, scroll: f32) {
        self.distance = (self.distance * (-scroll * 0.002).exp()).clamp(0.05, 50.0);
    }
}

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
    pub orbit: Orbit,
    /// The framing the character was loaded with, for F.
    home: Orbit,
    /// Every node's local transform as loaded, which a stop goes back to.
    rest: Vec<rk_chara::scene::Trs>,
    pub name: String,
}

impl Live {
    /// Builds the card's character and a renderer on the window's device.
    pub fn new(render_state: &egui_wgpu::RenderState, card: &Path) -> Result<Live> {
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
        Ok(Live {
            renderer,
            render_state: render_state.clone(),
            chara,
            env,
            target: None,
            orbit: home,
            home,
            rest,
            name: card_data.full_name(),
        })
    }

    /// Back to the framing the character was loaded with.
    pub fn frame(&mut self) {
        self.orbit = self.home;
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
            .render_to(&target.texture, &draws, &self.orbit.camera(), &self.env, background)?;
        Ok(target.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_orbit_keeps_the_camera_it_was_made_from() {
        let c = Camera {
            eye: Vec3::new(0.3, 1.4, -2.5),
            target: Vec3::new(0.0, 0.9, 0.0),
            fov_y_deg: 30.0,
        };
        let back = Orbit::from_camera(&c).camera();
        assert!((back.eye - c.eye).length() < 1e-4, "{:?}", back.eye);
        assert_eq!(back.target, c.target);
    }

    #[test]
    fn zoom_and_pitch_stay_in_range() {
        let mut o = Orbit {
            target: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            distance: 2.0,
            fov_y_deg: 30.0,
        };
        o.zoom(1.0e6);
        assert!(o.distance >= 0.05);
        o.rotate(egui::Vec2::new(0.0, -1.0e4));
        assert!(o.pitch <= 1.5);
        o.rotate(egui::Vec2::new(0.0, 1.0e4));
        assert!(o.pitch >= -1.5);
    }

    #[test]
    fn looking_keeps_the_eye_and_flying_moves_both() {
        let mut o = Orbit {
            target: Vec3::new(0.0, 1.0, 0.0),
            yaw: 0.3,
            pitch: 0.2,
            distance: 2.0,
            fov_y_deg: 30.0,
        };
        let eye = o.camera().eye;
        o.look(egui::Vec2::new(40.0, -25.0));
        assert!((o.camera().eye - eye).length() < 1e-4);
        let (eye, target) = (o.camera().eye, o.target);
        o.fly(Vec3::new(0.0, 0.0, 1.0), 0.5, false);
        let moved = o.target - target;
        assert!(moved.length() > 0.1);
        assert!(((o.camera().eye - eye) - moved).length() < 1e-4);
    }
}
