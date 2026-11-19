// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Face shots of the project's characters, for their tiles in the Content
//! panel. A card that has none is built, stood in the character maker's pose and drawn
//! from the front by a thread of its own, so the editor does not wait, and
//! the picture is kept in the project until the card changes.

use anyhow::{Context, Result};
use eframe::egui_wgpu::wgpu;
use glam::Vec3;
use rk_chara::scene::Part;
use rk_chara::{clothes_state, motion};
use rk_render::{bounds, Camera, Environment, Renderer, StudioEffects};
use rk_vfs::Game;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// The side of a face shot, in pixels: a few times the tile it shows in, so
/// that shrinking it smooths the edges.
pub const SIZE: u32 = 320;

/// How bright a shot is kept, so faces do not glare beside the panel.
const BRIGHTNESS: f32 = 0.85;

/// The pose a character takes for its shot: the first of the character
/// maker's poses (`cus_pose` in `custom/customscenelist.unity3d`), the one
/// the maker starts on. Starter Content: Animations & Poses carries it.
const POSE: (&str, &str, &str) = ("custom/custompose.unity3d", "custom_pose", "Stand_D_00");

/// Where a project keeps its face shots.
pub const FOLDER: &str = ".renkatsu/faces";

/// The face shot of the card `rel` (relative to the project) in a project.
pub fn shot_path(root: &Path, rel: &str) -> PathBuf {
    root.join(FOLDER)
        .join(format!("{}.png", rel.replace(['/', '\\'], "__")))
}

/// Whether a face shot is there and newer than its card.
pub fn fresh(shot: &Path, card: &Path) -> bool {
    let time = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    match (time(shot), time(card)) {
        (Some(shot), Some(card)) => shot >= card,
        _ => false,
    }
}

/// A card to take the face of.
struct Job {
    card: PathBuf,
    /// The content the card is built from, with the project's mods.
    game: Arc<Game>,
    shot: PathBuf,
    /// The colour around the face.
    backdrop: [u8; 3],
}

/// The thread that takes face shots, and what it was asked and finished.
pub struct Mugshots {
    jobs: Sender<Job>,
    done: Receiver<PathBuf>,
    asked: HashSet<PathBuf>,
}

impl Mugshots {
    /// Starts the thread, drawing on the window's device. `ctx` is asked to
    /// draw again when a shot is ready.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, ctx: eframe::egui::Context) -> Mugshots {
        let (jobs, inbox) = channel::<Job>();
        let (outbox, done) = channel();
        std::thread::Builder::new()
            .name("face shots".into())
            .spawn(move || {
                let mut renderer = None;
                for job in inbox {
                    match shoot(&mut renderer, &device, &queue, &job) {
                        Ok(()) => {
                            let _ = outbox.send(job.card);
                            ctx.request_repaint();
                        }
                        Err(e) => eprintln!("no face shot of {}: {e:#}", job.card.display()),
                    }
                }
            })
            .expect("a thread for face shots");
        Mugshots {
            jobs,
            done,
            asked: HashSet::new(),
        }
    }

    /// Asks for the face of `card` to be taken into `shot` on `backdrop`,
    /// once.
    pub fn ask(&mut self, card: &Path, game: Arc<Game>, shot: PathBuf, backdrop: [u8; 3]) {
        if self.asked.insert(card.to_owned()) {
            let _ = self.jobs.send(Job {
                card: card.to_owned(),
                game,
                shot,
                backdrop,
            });
        }
    }

    /// The cards whose face shots were finished since the last call.
    pub fn finished(&mut self) -> Vec<PathBuf> {
        let cards: Vec<PathBuf> = self.done.try_iter().collect();
        for card in &cards {
            // A card that changes again gets a new shot.
            self.asked.remove(card);
        }
        cards
    }
}

/// Builds the character of a card, stands it as the maker does, draws its face from
/// the front and saves the picture.
fn shoot(renderer: &mut Option<Renderer>, device: &wgpu::Device, queue: &wgpu::Queue, job: &Job) -> Result<()> {
    let game = &*job.game;
    let card = rk_card::Card::load(&job.card).with_context(|| format!("loading {}", job.card.display()))?;
    let mut chara = rk_chara::build(game, &card, &rk_chara::BuildOptions::default())?;
    // Without the pose in the game or Starter Content, the shot keeps the
    // T pose.
    if let Ok(motion) = motion::load(game, POSE.0, POSE.1, POSE.2, None, &chara.parameters) {
        let bound = motion.bind(&motion::Skeleton::of(&chara.scene, chara.animator));
        chara.animate(&bound, 0.0, &[clothes_state::ON; clothes_state::KINDS]);
    }
    let draws = chara.scene.pose();
    // The face alone frames the shot; hair may reach far below it.
    let head: Vec<_> = chara
        .scene
        .pose()
        .into_iter()
        .filter(|d| d.renderer.part == Part::Head)
        .collect();
    anyhow::ensure!(!head.is_empty(), "the character has no head");
    let camera = face_camera(bounds(&head));
    // The look of the viewport: CharaStudio's screen effects and its first
    // colour filter. The whole picture is dimmed after, so the backdrop is
    // drawn that much lighter to come out as asked.
    let backdrop = job
        .backdrop
        .map(|c| (f64::from(c) / 255.0 / f64::from(BRIGHTNESS)).min(1.0));
    let backdrop = [backdrop[0], backdrop[1], backdrop[2], 1.0];
    let mut effects = StudioEffects::studio(rk_chara::build::color_filter(game, 0));
    effects.backdrop = Some(backdrop);
    let env = Environment {
        ramp: chara.ramp.clone(),
        effects: Some(effects),
        // Shrunk into a tile, smoothed edges read cleaner.
        alpha_to_coverage: true,
        ..Default::default()
    };
    if renderer.is_none() {
        *renderer = Some(Renderer::with_device(device.clone(), queue.clone())?);
    }
    let renderer = renderer.as_mut().expect("made above");
    let mut rgba = renderer.render(&draws, &camera, &env, SIZE, SIZE, backdrop)?;
    dim(&mut rgba, BRIGHTNESS);
    if let Some(dir) = job.shot.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::File::create(&job.shot).with_context(|| format!("writing {}", job.shot.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), SIZE, SIZE);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&rgba)?;
    Ok(())
}

/// Darkens RGBA pixels by `amount`, leaving their alpha.
fn dim(rgba: &mut [u8], amount: f32) {
    for pixel in rgba.chunks_exact_mut(4) {
        for c in &mut pixel[..3] {
            *c = (f32::from(*c) * amount).round() as u8;
        }
    }
}

/// A camera in front of the head (the character faces +Z) that shows the
/// head with a little room around it, as the game's maker shoots faces.
fn face_camera((min, max): (Vec3, Vec3)) -> Camera {
    let fov_y_deg = 23.0f32;
    let size = max - min;
    // The middle of the face, a touch low so the chin has room.
    let target = (min + max) * 0.5 - Vec3::Y * size.y * 0.05;
    let half = size.y.max(size.x) * 0.85;
    let distance = half / (fov_y_deg.to_radians() * 0.5).tan();
    Camera {
        eye: target + Vec3::Z * distance,
        target,
        fov_y_deg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shot_is_named_after_its_card() {
        let root = Path::new("/p");
        assert_eq!(
            shot_path(root, "Characters/Cast/Anna.png"),
            Path::new("/p/.renkatsu/faces/Characters__Cast__Anna.png.png")
        );
    }

    #[test]
    fn dimming_keeps_alpha_and_hits_the_backdrop() {
        // The backdrop is drawn that much lighter, so dimming lands on it.
        let light = (30.0 / BRIGHTNESS).round() as u8;
        let mut rgba = [light, light, light, 255, 200, 100, 0, 128];
        dim(&mut rgba, BRIGHTNESS);
        assert_eq!(rgba, [30, 30, 30, 255, 170, 85, 0, 128]);
    }

    #[test]
    fn the_camera_looks_at_the_face_from_the_front() {
        let camera = face_camera((Vec3::new(-0.1, 1.4, -0.1), Vec3::new(0.1, 1.65, 0.1)));
        assert!(camera.eye.z > camera.target.z, "in front of the face");
        assert_eq!(camera.eye.x, camera.target.x);
        assert!((camera.target.y - 1.5125).abs() < 1e-4);
    }
}
