// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The cards on disk, for the Content panel: the folders under the game's
//! `UserData/chara/female`, the cards of the folder that is open, and the
//! cards' own pictures as thumbnails, loaded as their tiles come into view.

use egui::TextureHandle;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A folder that holds cards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    /// How deep the folder lies below the card folder, which is depth 0.
    pub depth: usize,
    pub path: PathBuf,
}

/// How deep the folders are followed, and how many are listed at most:
/// mod packs can bring thousands.
const DEPTH: usize = 4;
const FOLDERS: usize = 400;

/// Time spent loading thumbnails in one frame.
const BUDGET: Duration = Duration::from_millis(6);

pub struct Library {
    /// The card folder first, then the folders below it in tree order.
    /// Empty when the game was not found.
    pub folders: Vec<Folder>,
    /// Why there are no folders, when there are none.
    pub problem: Option<String>,
    /// The cards of one folder, as last listed.
    listed: Option<(PathBuf, Vec<PathBuf>)>,
    /// Thumbnails by card; `None` for a card whose picture did not load.
    thumbs: HashMap<PathBuf, Option<TextureHandle>>,
    /// When this frame's thumbnail loading began.
    loading_since: Option<Instant>,
    /// Load every thumbnail asked for at once, for scripted pictures.
    pub eager: bool,
}

impl Library {
    pub fn new() -> Library {
        let mut library = Library {
            folders: Vec::new(),
            problem: None,
            listed: None,
            thumbs: HashMap::new(),
            loading_since: None,
            eager: false,
        };
        library.scan();
        library
    }

    /// Lists the card folders again, and forgets the cards and pictures.
    pub fn scan(&mut self) {
        self.folders.clear();
        self.listed = None;
        self.thumbs.clear();
        self.problem = None;
        match rk_core::Config::load() {
            Ok(config) => {
                let root = config.userdata().join("chara").join("female");
                if root.is_dir() {
                    self.folders = folders(&root);
                } else {
                    self.problem = Some(format!("The game has no card folder at {}", root.display()));
                }
            }
            Err(e) => self.problem = Some(format!("The game was not found: {e:#}")),
        }
    }

    /// The cards directly inside a folder, by name.
    pub fn cards(&mut self, folder: &Path) -> &[PathBuf] {
        if self.listed.as_ref().is_none_or(|(path, _)| path != folder) {
            // Another folder: its pictures replace the ones held.
            self.thumbs.clear();
            self.listed = Some((folder.to_owned(), cards(folder)));
        }
        &self.listed.as_ref().expect("listed above").1
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

/// Orders names as a file manager does: without regard to case.
fn by_name(a: &Path, b: &Path) -> std::cmp::Ordering {
    let key = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_lowercase());
    key(a).cmp(&key(b))
}

/// The card folder and the folders below it, in tree order.
fn folders(root: &Path) -> Vec<Folder> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<Folder>) {
        if depth > DEPTH {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut below: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        below.sort_by(|a, b| by_name(a, b));
        for path in below {
            if out.len() >= FOLDERS {
                return;
            }
            out.push(Folder {
                name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                depth,
                path: path.clone(),
            });
            walk(&path, depth + 1, out);
        }
    }
    let mut out = vec![Folder {
        name: "Characters".to_owned(),
        depth: 0,
        path: root.to_owned(),
    }];
    walk(root, 1, &mut out);
    out
}

/// The PNG files directly inside a folder, by name.
fn cards(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut cards: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
        .collect();
    cards.sort_by(|a, b| by_name(a, b));
    cards
}

/// A card's own picture: the PNG image the card file starts with.
fn picture(card: &Path) -> Option<egui::ColorImage> {
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

    /// A folder of its own under the system's temporary folder.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("renkatsu-library-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn folders_and_cards_are_listed_in_tree_order_by_name() {
        let root = scratch("tree");
        for dir in ["zeta", "Alpha", "Alpha/inner"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["b.png", "A.PNG", "notes.txt", "Alpha/c.png"] {
            std::fs::write(root.join(file), b"").unwrap();
        }
        let listed: Vec<(usize, String)> = folders(&root).into_iter().map(|f| (f.depth, f.name)).collect();
        assert_eq!(
            listed,
            [
                (0, "Characters".to_owned()),
                (1, "Alpha".to_owned()),
                (2, "inner".to_owned()),
                (1, "zeta".to_owned()),
            ]
        );
        let names = |dir: &Path| -> Vec<String> {
            cards(dir)
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(names(&root), ["A.PNG", "b.png"]);
        assert_eq!(names(&root.join("Alpha")), ["c.png"]);
        assert!(names(&root.join("missing")).is_empty());
        // A file that is no picture gives no thumbnail.
        assert!(picture(&root.join("b.png")).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
