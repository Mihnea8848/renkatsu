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
    /// Whether the card is of a man, which colours its icons blue.
    pub male: bool,
}

/// How thumbnails are filtered: they show much smaller than they are, so
/// smaller copies smooth their edges instead of skipping pixels.
const SHRUNK: egui::TextureOptions = egui::TextureOptions {
    mipmap_mode: Some(egui::TextureFilter::Linear),
    ..egui::TextureOptions::LINEAR
};

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
    /// The project's folder, for the face shots.
    root: PathBuf,
    /// The thread that takes face shots, once there is a device to draw on.
    pub mugshots: Option<crate::mugshot::Mugshots>,
    /// The colour face shots are taken on, a little darker than the panel.
    pub backdrop: [u8; 3],
    /// Cards whose face shots are to be taken, with where they go.
    pub wanted: Vec<(PathBuf, PathBuf)>,
}

impl Library {
    /// The colour of a scene object's icon: its kind's, blue for a man.
    pub fn color_of(&self, content: &crate::scene::Content) -> egui::Color32 {
        match content {
            crate::scene::Content::Character { card, .. }
                if self.characters.iter().any(|c| c.file == *card && c.male) =>
            {
                crate::kinds::Kind::MALE
            }
            _ => content.kind().color(),
        }
    }

    pub fn new(project: &Project) -> Library {
        let mut library = Library {
            characters: Vec::new(),
            thumbs: HashMap::new(),
            loading_since: None,
            eager: false,
            root: PathBuf::new(),
            mugshots: None,
            backdrop: [30, 30, 30],
            wanted: Vec::new(),
        };
        library.refresh(project);
        library
    }

    /// Lists the project's characters again, and forgets the pictures of
    /// the cards that are gone.
    pub fn refresh(&mut self, project: &Project) {
        self.root = project.root.clone();
        self.characters = project
            .characters()
            .into_iter()
            .map(|c| {
                let path = project.resolve(Path::new(&c.file));
                Entry {
                    male: rk_card::Card::male_of(&path).unwrap_or(false),
                    path,
                    file: PathBuf::from(c.file),
                    name: c.name,
                }
            })
            .collect();
        self.characters.sort_by_key(|c| crate::content::natural(&c.name));
        let characters = &self.characters;
        self.thumbs.retain(|path, _| characters.iter().any(|c| c.path == *path));
    }

    /// A new frame: the thumbnail loading starts over, and the cards whose
    /// face shots are now taken load them.
    pub fn begin_frame(&mut self) {
        self.loading_since = None;
        if let Some(mugshots) = self.mugshots.as_mut() {
            for card in mugshots.finished() {
                self.thumbs.remove(&card);
            }
        }
    }

    /// The thumbnail of a card: the face shot Renkatsu took of it, else the
    /// card's own picture while the shot is taken. Pictures load a few per
    /// frame, so a card may have none yet; the caller is told to draw again
    /// when so.
    pub fn thumb(&mut self, ctx: &egui::Context, card: &Path) -> Option<&TextureHandle> {
        if !self.thumbs.contains_key(card) {
            let since = *self.loading_since.get_or_insert_with(Instant::now);
            if !self.eager && since.elapsed() > BUDGET {
                ctx.request_repaint();
                return None;
            }
            // The face shot Renkatsu took of the character, else the card's
            // own picture until it is taken.
            let shot = self
                .characters
                .iter()
                .find(|c| c.path == card)
                .map(|c| crate::mugshot::shot_path(&self.root, &c.file.to_string_lossy()));
            let image = match &shot {
                Some(shot) if crate::mugshot::fresh(shot, card) => std::fs::File::open(shot)
                    .ok()
                    .and_then(|f| decode(std::io::BufReader::new(f))),
                Some(shot) => {
                    self.wanted.push((card.to_owned(), shot.clone()));
                    None
                }
                None => None,
            };
            let texture = image
                .or_else(|| picture(card))
                .map(|image| ctx.load_texture(format!("card {}", card.display()), image, SHRUNK));
            self.thumbs.insert(card.to_owned(), texture);
        }
        self.thumbs.get(card).and_then(Option::as_ref)
    }
}

/// A card's own picture: the PNG image the card file starts with.
pub fn picture(card: &Path) -> Option<egui::ColorImage> {
    decode(std::io::BufReader::new(std::fs::File::open(card).ok()?)).or_else(|| {
        // A card saved without its picture still holds the face the game
        // shows in its lists.
        let face = rk_card::Card::load(card).ok()?.face_png;
        decode(std::io::Cursor::new(face))
    })
}

/// A PNG image as plain colour.
fn decode(file: impl std::io::BufRead + std::io::Seek) -> Option<egui::ColorImage> {
    let mut decoder = png::Decoder::new(file);
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
                male: false,
            }]
        );
        // A file that is no picture gives no thumbnail.
        assert!(picture(&library.characters[0].path).is_none());
        let _ = std::fs::remove_dir_all(&home);
    }
}
