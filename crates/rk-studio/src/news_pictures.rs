// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The pictures of the news: fetched and read away from the window the
//! first time a post that has them is opened, kept as textures after. A
//! moving picture, an animated PNG, plays its frames.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};

/// The most pixels a picture keeps across: twice the column it shows in.
const WIDEST: u32 = 1600;
/// The most bytes the frames of one moving picture keep; past it, frames
/// are skipped evenly and the others stay on for longer.
const FRAME_BYTES: usize = 96 << 20;
/// A frame that says it lasts no time lasts this long, as browsers do.
const SHORTEST: f32 = 0.1;

/// Frames read from a picture file, each with how long it shows.
type Frames = Vec<(egui::ColorImage, f32)>;

enum Slot {
    Loading(Receiver<Option<Frames>>),
    Ready(Shown),
    Failed,
}

/// A picture as the page draws it.
pub struct Shown {
    frames: Vec<(egui::TextureHandle, f32)>,
    /// Its size in pixels.
    pub size: [usize; 2],
    /// One pass of its frames, in seconds; zero for a still picture.
    total: f32,
}

impl Shown {
    /// The frame to show at `time` and the seconds until the next one.
    pub fn frame(&self, time: f64) -> (&egui::TextureHandle, Option<f32>) {
        if self.frames.len() < 2 || self.total <= 0.0 {
            return (&self.frames[0].0, None);
        }
        let mut at = (time % f64::from(self.total)) as f32;
        for (texture, lasts) in &self.frames {
            if at < *lasts {
                return (texture, Some(lasts - at));
            }
            at -= lasts;
        }
        (&self.frames[0].0, Some(SHORTEST))
    }
}

/// What a picture is at this frame.
pub enum State<'a> {
    Loading,
    Ready(&'a Shown),
    Failed,
}

#[derive(Default)]
pub struct Pictures {
    slots: HashMap<String, Slot>,
}

impl Pictures {
    /// The picture at `url`, asked for the first time it is wanted.
    pub fn get(&mut self, ctx: &egui::Context, url: &str) -> State<'_> {
        let slot = self.slots.entry(url.to_owned()).or_insert_with(|| {
            let (send, receive) = channel();
            let (url, ctx) = (url.to_owned(), ctx.clone());
            std::thread::spawn(move || {
                let frames = rk_packages::news::picture_file(&url)
                    .ok()
                    .and_then(|file| std::fs::read(file).ok())
                    .and_then(|bytes| read(&bytes));
                let _ = send.send(frames);
                ctx.request_repaint();
            });
            Slot::Loading(receive)
        });
        if let Slot::Loading(receive) = slot {
            match receive.try_recv() {
                Ok(Some(frames)) => *slot = Slot::Ready(upload(ctx, url, frames)),
                Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => *slot = Slot::Failed,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        match slot {
            Slot::Loading(_) => State::Loading,
            Slot::Ready(shown) => State::Ready(shown),
            Slot::Failed => State::Failed,
        }
    }
}

fn upload(ctx: &egui::Context, url: &str, frames: Frames) -> Shown {
    let size = frames[0].0.size;
    let total = if frames.len() > 1 {
        frames.iter().map(|(_, t)| t).sum()
    } else {
        0.0
    };
    let frames = frames
        .into_iter()
        .enumerate()
        .map(|(n, (image, lasts))| {
            let name = format!("news picture {url} {n}");
            (ctx.load_texture(name, image, egui::TextureOptions::LINEAR), lasts)
        })
        .collect();
    Shown { frames, size, total }
}

/// The frames of a PNG, still or moving, or of a JPEG, each no wider than
/// `WIDEST`.
fn read(bytes: &[u8]) -> Option<Frames> {
    use image::codecs::png::PngDecoder;
    use image::AnimationDecoder;
    let fit = |image: image::RgbaImage| {
        let image = if image.width() > WIDEST {
            let height = (u64::from(image.height()) * u64::from(WIDEST) / u64::from(image.width())).max(1) as u32;
            image::imageops::resize(&image, WIDEST, height, image::imageops::FilterType::Triangle)
        } else {
            image
        };
        let size = [image.width() as usize, image.height() as usize];
        egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw())
    };
    if bytes.starts_with(b"\x89PNG") {
        let decoder = PngDecoder::new(std::io::Cursor::new(bytes)).ok()?;
        if decoder.is_apng().ok()? {
            let mut frames: Frames = Vec::new();
            let mut kept = 0usize;
            for frame in decoder.apng().ok()?.into_frames() {
                let frame = frame.ok()?;
                let (num, den) = frame.delay().numer_denom_ms();
                let lasts = (num as f32 / den.max(1) as f32 / 1000.0).max(SHORTEST);
                let image = fit(frame.into_buffer());
                kept += image.pixels.len() * 4;
                frames.push((image, lasts));
            }
            return (!frames.is_empty()).then(|| thin(frames, kept));
        }
    }
    let image = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(vec![(fit(image), 0.0)])
}

/// Every n-th frame of a moving picture whose frames are more than
/// `FRAME_BYTES`, each kept for the time of the ones it stands for.
fn thin(frames: Frames, bytes: usize) -> Frames {
    let step = bytes.div_ceil(FRAME_BYTES).max(1);
    if step == 1 {
        return frames;
    }
    let mut out: Frames = Vec::new();
    for (n, (image, lasts)) in frames.into_iter().enumerate() {
        if n % step == 0 {
            out.push((image, lasts));
        } else if let Some(last) = out.last_mut() {
            last.1 += lasts;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An animated PNG of three frames of 4x2, a colour each.
    fn moving() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 4, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_animated(3, 0).unwrap();
            encoder.set_frame_delay(1, 10).unwrap();
            let mut writer = encoder.write_header().unwrap();
            for shade in [0u8, 120, 240] {
                let pixels: Vec<u8> = [shade, shade, shade, 255].repeat(4 * 2);
                writer.write_image_data(&pixels).unwrap();
            }
            writer.finish().unwrap();
        }
        bytes
    }

    #[test]
    fn a_moving_picture_plays_its_frames_in_turn() {
        let frames = read(&moving()).expect("an animated PNG reads");
        assert_eq!(frames.len(), 3);
        assert!(frames.iter().all(|(_, lasts)| (lasts - 0.1).abs() < 1e-6));
        assert_eq!(frames[1].0.pixels[0].r(), 120);
        let thinned = thin(frames, FRAME_BYTES * 2 + 1);
        assert_eq!(thinned.len(), 1);
        assert!((thinned[0].1 - 0.3).abs() < 1e-6);
    }
}
