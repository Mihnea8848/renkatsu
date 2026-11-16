// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The project's characters, for the Content panel: the cards the project
//! owns, under the names they were imported with, and the cards' own
//! pictures as thumbnails, loaded as their tiles come into view.

use crate::project::Project;
use egui::TextureHandle;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A character of the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The card relative to the project, as scenes name it.
    pub file: PathBuf,
    /// The name it shows under.
    pub name: String,
    /// The card on disk.
    pub path: PathBuf,
}

/// Time spent loading thumbnails in one frame.
const BUDGET: Duration = Duration::from_millis(6);

pub struct Library {
    /// The project's characters, by name.
    pub characters: Vec<Entry>,
    /// Thumbnails by card; `None` for a card whose picture did not load.
    thumbs: HashMap<PathBuf, Option<TextureHandle>>,
    /// When this frame's thumbnail loading began.
    loading_since: Option<Instant>,
    /// Load every thumbnail asked for at once, for scripted pictures.
    pub eager: bool,
}

impl Library {
    pub fn new(project: &Project) -> Library {
        let mut library = Library {
            characters: Vec::new(),
            thumbs: HashMap::new(),
            loading_since: None,
            eager: false,
        };
        library.refresh(project);
        library
    }

    /// Lists the project's characters again, and forgets the pictures of
    /// the cards that are gone.
    pub fn refresh(&mut self, project: &Project) {
        self.characters = project
            .characters()
            .into_iter()
            .map(|c| Entry {
                path: project.resolve(Path::new(&c.file)),
                file: PathBuf::from(c.file),
                name: c.name,
            })
            .collect();
        self.characters.sort_by_key(|c| c.name.to_lowercase());
        let characters = &self.characters;
        self.thumbs.retain(|path, _| characters.iter().any(|c| c.path == *path));
    }

    /// A new frame: the thumbnail loading starts over.
    pub fn begin_frame(&mut self) {
        self.loading_since = None;
    }

    /// The thumbnail of a card. Pictures load a few per frame, so a card
    /// may have none yet; the caller is told to draw again when so.
    pub fn thumb(&mut self, ctx: &egui::Context, card: &Path) -> Option<&TextureHandle> {
        if !self.thumbs.contains_key(card) {
            let since = *self.loading_since.get_or_insert_with(Instant::now);
            if !self.eager && since.elapsed() > BUDGET {
                ctx.request_repaint();
                return None;
            }
            let texture = picture(card)
                .map(|image| ctx.load_texture(format!("card {}", card.display()), image, egui::TextureOptions::LINEAR));
            self.thumbs.insert(card.to_owned(), texture);
        }
        self.thumbs.get(card).and_then(Option::as_ref)
    }
}

/// A card's own picture: the PNG image the card file starts with.
pub fn picture(card: &Path) -> Option<egui::ColorImage> {
    let mut decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(card).ok()?));
    // Palettes and 16-bit channels come out as plain 8-bit colour.
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let size = [info.width as usize, info.height as usize];
    let bytes = &buf[..info.buffer_size()];
    match info.color_type {
        png::ColorType::Rgba => Some(egui::ColorImage::from_rgba_unmultiplied(size, bytes)),
        png::ColorType::Rgb => Some(egui::ColorImage::from_rgb(size, bytes)),
        png::ColorType::Grayscale => Some(egui::ColorImage::from_gray(size, bytes)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_lists_the_projects_own_characters() {
        let home = std::env::temp_dir().join(format!("renkatsu-library-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let mut project = Project::create(&home.join("P"), "P").unwrap();
        let mut library = Library::new(&project);
        assert!(library.characters.is_empty());
        std::fs::write(home.join("Simple.png"), b"not a picture").unwrap();
        project.import_card(&home.join("Simple.png"), "Renkatsu-Chan").unwrap();
        library.refresh(&project);
        assert_eq!(
            library.characters,
            [Entry {
                file: PathBuf::from("Characters/Simple.png"),
                name: "Renkatsu-Chan".to_owned(),
                path: home.join("P/Characters/Simple.png"),
            }]
        );
        // A file that is no picture gives no thumbnail.
        assert!(picture(&library.characters[0].path).is_none());
        let _ = std::fs::remove_dir_all(&home);
    }
}
