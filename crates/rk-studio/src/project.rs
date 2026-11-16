// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! A project on disk: a folder that owns everything its scenes use, so that
//! it can be moved, copied or handed to someone else as it stands.
//!
//! ```text
//! My project/
//!   My project.rkproj  the project's name, its scene and its characters
//!   Characters/*.png   the cards the project owns, copied in on import
//!   Scenes/*.toml      scenes, as the scene model writes itself
//!   Mods/*.zipmod      the zipmods those cards need, copied from the game
//!   Poses/ Maps/ Items/ Renders/   kept for the features that fill them
//! ```
//!
//! The project file is TOML named after the project, made safe for the
//! file system, and the folders are the ones the Content browser shows.
//!
//! Everything is readable text or the files as they came. Scenes name
//! their characters by a path relative to the project folder, with forward
//! slashes, never by an absolute path.

use crate::scene::Scene;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The version of the project and scene files this build writes.
pub const FORMAT: u32 = 1;

/// The extension of the project file.
pub const EXTENSION: &str = "rkproj";
const CHARACTERS: &str = "Characters";
const SCENES: &str = "Scenes";
const MODS: &str = "Mods";
/// Folders every project has, as the Content browser shows them: the ones
/// in use and the ones kept for later.
pub const FOLDERS: [&str; 7] = [CHARACTERS, "Poses", "Maps", "Items", SCENES, "Renders", MODS];

fn is_project_file(path: &Path) -> bool {
    path.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// The project files in a folder, by name.
fn project_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_project_file(p))
        .collect();
    files.sort_by(|a, b| by_name(a, b));
    files
}

fn yes() -> bool {
    true
}

/// A character the project owns: its card and the name it goes by here,
/// which need not be the name inside the card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Character {
    /// The card, relative to the project folder.
    pub file: String,
    pub name: String,
}

/// What the project file holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub format: u32,
    pub name: String,
    /// The scene that opens with the project, relative to its folder.
    pub scene: String,
    /// Ask before an import copies zipmods into the project.
    #[serde(default = "yes")]
    pub ask_before_copying_mods: bool,
    /// The picture the user gave the project, relative to its folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// The picture of the scene as it was last saved, relative to the
    /// project folder. The editor writes it; it may be deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub characters: Vec<Character>,
}

/// What a scene file holds: the format's version, then the scene.
#[derive(Serialize, Deserialize)]
struct SceneFile {
    format: u32,
    #[serde(flatten)]
    scene: Scene,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The project folder.
    pub root: PathBuf,
    pub file: ProjectFile,
}

/// A name as a file or folder name: without the characters file systems
/// refuse, and never empty.
pub fn file_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let clean = clean.trim().trim_end_matches('.').to_owned();
    if clean.is_empty() {
        "Untitled".to_owned()
    } else {
        clean
    }
}

/// A path below the project folder as scenes and the project file write it.
fn relative(folder: &str, file: &str) -> String {
    format!("{folder}/{file}")
}

/// Orders names as a file manager does: without regard to case.
fn by_name(a: &Path, b: &Path) -> std::cmp::Ordering {
    let key = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_lowercase());
    key(a).cmp(&key(b))
}

fn is_card(path: &Path) -> bool {
    path.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"))
}

/// Copies a folder with everything below it.
fn copy_folder(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).with_context(|| format!("creating {}", to.display()))?;
    for entry in std::fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).with_context(|| format!("copying {}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Whether a folder can take a new project: it is not there yet, or empty.
pub fn is_free(folder: &Path) -> bool {
    match std::fs::read_dir(folder) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => !folder.exists(),
    }
}

/// A folder below `home` that can take a new project: `base`, or `base`
/// with the first free number after it.
pub fn free_folder(home: &Path, base: &str) -> PathBuf {
    let first = home.join(base);
    if is_free(&first) {
        return first;
    }
    (2..)
        .map(|n| home.join(format!("{base} {n}")))
        .find(|p| is_free(p))
        .expect("some number is free")
}

/// What a new project starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Start {
    /// The default character and the key light.
    #[default]
    RenkatsuChan,
    /// The key light alone.
    Empty,
}

impl Start {
    pub const ALL: [Start; 2] = [Start::RenkatsuChan, Start::Empty];

    pub fn label(self) -> &'static str {
        match self {
            Start::RenkatsuChan => "Renkatsu-Chan",
            Start::Empty => "Empty",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Start::RenkatsuChan => "The default character under a key light.",
            Start::Empty => "Only the key light.",
        }
    }
}

/// The environment variable that names another folder for new projects,
/// for tests and scripted runs that must leave the documents folder alone.
pub const HOME_VARIABLE: &str = "RENKATSU_PROJECTS";

/// Where a project keeps its icon: beside the project file.
pub const ICON: &str = "Icon.png";
/// Where the editor keeps the picture of the last saved scene: in a folder
/// of its own, apart from the user's renders.
pub const THUMBNAIL: &str = ".renkatsu/thumbnail.png";
/// The size of that picture, in pixels.
pub const THUMBNAIL_SIZE: [u32; 2] = [640, 360];

/// What the project selector shows of a project without opening it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub name: String,
    /// The user's icon, when the project has one that is still there.
    pub icon: Option<PathBuf>,
    /// The picture of the last saved scene, when there is one.
    pub thumbnail: Option<PathBuf>,
    pub scenes: usize,
    pub characters: usize,
}

/// Reads the summary of the project in a folder: its project file and the
/// names in two of its folders. `None` when the folder holds no project.
pub fn summary(root: &Path) -> Option<Summary> {
    let file = project_files(root).into_iter().next()?;
    let read = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| toml::from_str::<ProjectFile>(&text).ok());
    let picture = |relative: Option<&String>| relative.map(|r| root.join(r)).filter(|p| p.is_file());
    let scenes = std::fs::read_dir(root.join(SCENES))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("toml")))
        .count();
    Some(Summary {
        name: match &read {
            Some(f) => f.name.clone(),
            None => file.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
        },
        icon: picture(read.as_ref().and_then(|f| f.icon.as_ref())),
        thumbnail: picture(read.as_ref().and_then(|f| f.thumbnail.as_ref())),
        scenes,
        characters: cards_in(root).len(),
    })
}

/// Gives the project in a folder an icon, or takes it away, without the
/// project being open.
pub fn set_icon_in(root: &Path, source: Option<&Path>) -> Result<()> {
    let (mut project, ..) = Project::open(root)?;
    project.set_icon(source)
}

/// The cards in a project folder's characters folder, by file name.
pub fn cards_in(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root.join(CHARACTERS)) else {
        return Vec::new();
    };
    let mut cards: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| is_card(p)).collect();
    cards.sort_by(|a, b| by_name(a, b));
    cards
}

/// Where new projects go unless the user says otherwise: `Renkatsu` in the
/// user's documents folder.
pub fn home() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(HOME_VARIABLE).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let documents = if cfg!(windows) {
        PathBuf::from(std::env::var_os("USERPROFILE")?).join("Documents")
    } else {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let config = match std::env::var_os("XDG_CONFIG_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => home.join(".config"),
        };
        std::fs::read_to_string(config.join("user-dirs.dirs"))
            .ok()
            .and_then(|text| documents_of(&text, &home))
            .unwrap_or_else(|| home.join("Documents"))
    };
    Some(documents.join("Renkatsu"))
}

/// The documents folder named by the desktop's `user-dirs.dirs` file.
fn documents_of(text: &str, home: &Path) -> Option<PathBuf> {
    let value = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("XDG_DOCUMENTS_DIR="))?
        .trim()
        .trim_matches('"');
    let path = match value.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(value),
    };
    // The desktop writes the home folder itself when there is none.
    (path.is_absolute() && path != home).then_some(path)
}

impl Project {
    /// No project: what the editor holds while the project selector shows.
    /// It has no folder, and nothing reads or writes it.
    pub fn none() -> Project {
        Project {
            root: PathBuf::new(),
            file: ProjectFile {
                format: FORMAT,
                name: String::new(),
                scene: String::new(),
                ask_before_copying_mods: true,
                icon: None,
                thumbnail: None,
                characters: Vec::new(),
            },
        }
    }

    /// Whether this is the stand-in for no project.
    pub fn is_none(&self) -> bool {
        self.root.as_os_str().is_empty()
    }

    /// Makes a new project in `root`, which must be free, with its folders
    /// and its project file. The scene is written by the first save.
    pub fn create(root: &Path, name: &str) -> Result<Project> {
        if !is_free(root) {
            bail!("{} already holds files", root.display());
        }
        for folder in FOLDERS {
            let dir = root.join(folder);
            std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let project = Project {
            root: root.to_owned(),
            file: ProjectFile {
                format: FORMAT,
                name: name.to_owned(),
                scene: String::new(),
                ask_before_copying_mods: true,
                icon: None,
                thumbnail: None,
                characters: Vec::new(),
            },
        };
        project.save()?;
        Ok(project)
    }

    /// Whether a folder holds a project.
    pub fn exists(root: &Path) -> bool {
        !project_files(root).is_empty()
    }

    /// The project file: the project's name, safe for the file system, with
    /// the `.rkproj` extension.
    pub fn path(&self) -> PathBuf {
        self.root.join(format!("{}.{EXTENSION}", file_name(&self.file.name)))
    }

    /// Opens a project from its project file, or from the folder that holds
    /// one, with its scene. A project whose scene file is gone opens with a
    /// new scene; the second value says so.
    pub fn open(path: &Path) -> Result<(Project, Scene, Option<String>)> {
        let (root, file) = if is_project_file(path) {
            (path.parent().unwrap_or(path), path.to_owned())
        } else {
            let file = project_files(path)
                .into_iter()
                .next()
                .with_context(|| format!("{} holds no .{EXTENSION} project file", path.display()))?;
            (path, file)
        };
        let text = std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
        let file: ProjectFile = toml::from_str(&text).with_context(|| format!("reading {}", file.display()))?;
        if file.format > FORMAT {
            bail!(
                "{} was saved by a newer Renkatsu (format {})",
                root.display(),
                file.format
            );
        }
        let project = Project {
            root: root.to_owned(),
            file,
        };
        let (scene, note) = match project.load_scene() {
            Ok(scene) => (scene, None),
            Err(e) => (Scene::new(), Some(format!("The project opens with a new scene: {e:#}"))),
        };
        Ok((project, scene, note))
    }

    fn load_scene(&self) -> Result<Scene> {
        if self.file.scene.is_empty() {
            bail!("the project names no scene");
        }
        let path = self.root.join(&self.file.scene);
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let file: SceneFile = toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
        Ok(file.scene)
    }

    /// Writes the project file, and makes sure every folder is there. A
    /// project file left under another name, by a rename or a copy, goes.
    pub fn save(&self) -> Result<()> {
        if self.is_none() {
            bail!("no project is open");
        }
        for folder in FOLDERS {
            let dir = self.root.join(folder);
            std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let path = self.path();
        std::fs::write(&path, toml::to_string(&self.file)?).with_context(|| format!("writing {}", path.display()))?;
        for old in project_files(&self.root) {
            if old.file_name() != path.file_name() {
                std::fs::remove_file(&old).with_context(|| format!("removing {}", old.display()))?;
            }
        }
        Ok(())
    }

    /// The folder of the Content browser's folder `name`.
    pub fn folder(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// Writes the scene to its file, named after the scene the first time,
    /// and the project file with it.
    pub fn save_scene(&mut self, scene: &Scene) -> Result<()> {
        if self.file.scene.is_empty() {
            self.file.scene = relative(SCENES, &format!("{}.toml", file_name(&scene.name)));
        }
        let path = self.root.join(&self.file.scene);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string(&SceneFile {
            format: FORMAT,
            scene: scene.clone(),
        })?;
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        self.save()
    }

    /// Copies the whole project to another folder, which must be free, and
    /// returns the copy under its new name. Scenes refer to their
    /// characters relative to the project, so they need no change.
    pub fn copy_to(&self, root: &Path, name: &str) -> Result<Project> {
        if !is_free(root) {
            bail!("{} already holds files", root.display());
        }
        if root.starts_with(&self.root) {
            bail!("a project cannot be saved inside itself");
        }
        copy_folder(&self.root, root)?;
        let copy = Project {
            root: root.to_owned(),
            file: ProjectFile {
                name: name.to_owned(),
                ..self.file.clone()
            },
        };
        copy.save()?;
        Ok(copy)
    }

    /// Makes a PNG picture the project's icon: it is copied into the
    /// project folder and named in the project file. `None` takes the icon
    /// away, with its file.
    pub fn set_icon(&mut self, source: Option<&Path>) -> Result<()> {
        match source {
            Some(source) => {
                let bytes = std::fs::read(source).with_context(|| format!("reading {}", source.display()))?;
                png::Decoder::new(std::io::Cursor::new(&bytes))
                    .read_info()
                    .with_context(|| format!("{} is not a PNG picture", source.display()))?;
                let target = self.root.join(ICON);
                std::fs::write(&target, bytes).with_context(|| format!("writing {}", target.display()))?;
                self.file.icon = Some(ICON.to_owned());
            }
            None => {
                if let Some(old) = self.file.icon.take() {
                    let _ = std::fs::remove_file(self.root.join(old));
                }
            }
        }
        self.save()
    }

    /// Writes the picture of the saved scene, from RGBA bytes of
    /// `THUMBNAIL_SIZE`, and names it in the project file. `None` removes
    /// the picture: the scene shows nothing to take one of.
    pub fn set_thumbnail(&mut self, rgba: Option<&[u8]>) -> Result<()> {
        let path = self.root.join(THUMBNAIL);
        let named = match rgba {
            Some(rgba) => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let file = std::fs::File::create(&path).with_context(|| format!("writing {}", path.display()))?;
                let mut encoder =
                    png::Encoder::new(std::io::BufWriter::new(file), THUMBNAIL_SIZE[0], THUMBNAIL_SIZE[1]);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.write_header()?.write_image_data(rgba)?;
                Some(THUMBNAIL.to_owned())
            }
            None => {
                let _ = std::fs::remove_file(&path);
                None
            }
        };
        if self.file.thumbnail != named {
            self.file.thumbnail = named;
            self.save()?;
        }
        Ok(())
    }

    pub fn mods_dir(&self) -> PathBuf {
        self.root.join(MODS)
    }

    /// Where a path a scene names lies on disk.
    pub fn resolve(&self, relative: &Path) -> PathBuf {
        self.root.join(relative)
    }

    /// Copies a card into the project and lists it under `name`. A card
    /// that is already there, byte for byte, is not copied again; another
    /// card with the same file name gets a number. Returns the card's path
    /// relative to the project. The project file is not written.
    pub fn import_card(&mut self, source: &Path, name: &str) -> Result<String> {
        let dir = self.root.join(CHARACTERS);
        std::fs::create_dir_all(&dir)?;
        let bytes = std::fs::read(source).with_context(|| format!("reading {}", source.display()))?;
        let stem = source.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let stem = file_name(&stem);
        let mut file = format!("{stem}.png");
        let mut number = 1;
        loop {
            let target = dir.join(&file);
            match std::fs::read(&target) {
                Ok(there) if there == bytes => break,
                Ok(_) => {
                    number += 1;
                    file = format!("{stem} {number}.png");
                }
                Err(_) => {
                    std::fs::write(&target, &bytes).with_context(|| format!("writing {}", target.display()))?;
                    break;
                }
            }
        }
        let file = relative(CHARACTERS, &file);
        if !self.file.characters.iter().any(|c| c.file == file) {
            self.file.characters.push(Character {
                file: file.clone(),
                name: name.to_owned(),
            });
        }
        Ok(file)
    }

    /// The characters the project owns: every card in its characters
    /// folder, by file name, under the name it was imported with or, for a
    /// card that was put there by hand, its file name.
    pub fn characters(&self) -> Vec<Character> {
        if self.is_none() {
            return Vec::new();
        }
        cards_in(&self.root)
            .iter()
            .map(|card| {
                let file = relative(CHARACTERS, &card.file_name().unwrap_or_default().to_string_lossy());
                let name = self.character_name(&file).map(str::to_owned);
                let stem = card.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                Character {
                    name: name.unwrap_or(stem),
                    file,
                }
            })
            .collect()
    }

    /// The name a card of the project was imported under.
    pub fn character_name(&self, file: &str) -> Option<&str> {
        self.file
            .characters
            .iter()
            .find(|c| c.file == file)
            .map(|c| c.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Content;

    /// A folder of its own under the system's temporary folder.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("renkatsu-project-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_project_is_readable_text_and_reads_back() {
        let home = scratch("round-trip");
        let root = home.join("First");
        let mut project = Project::create(&root, "First").unwrap();
        for folder in FOLDERS {
            assert!(root.join(folder).is_dir(), "{folder}");
        }
        std::fs::write(home.join("Simple.png"), b"card one").unwrap();
        let card = project.import_card(&home.join("Simple.png"), "Renkatsu-Chan").unwrap();
        assert_eq!(card, "Characters/Simple.png");

        let mut scene = Scene::new();
        scene.name = "Main: take 1".to_owned();
        scene.add("Renkatsu-Chan", Content::character(PathBuf::from(&card)), None);
        project.save_scene(&scene).unwrap();
        assert_eq!(project.file.scene, "Scenes/Main_ take 1.toml");

        let text = std::fs::read_to_string(project.path()).unwrap();
        assert!(text.contains("name = \"First\""), "{text}");
        assert!(text.contains("file = \"Characters/Simple.png\""), "{text}");
        let text = std::fs::read_to_string(root.join(&project.file.scene)).unwrap();
        assert!(text.contains("card = \"Characters/Simple.png\""), "{text}");
        assert!(!text.contains(&*home.to_string_lossy()), "{text}");

        let (read, read_scene, note) = Project::open(&root).unwrap();
        assert_eq!(read, project);
        assert_eq!(read_scene, scene);
        assert_eq!(note, None);
        // The project file itself opens the project too.
        assert_eq!(Project::open(&project.path()).unwrap().0, project);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn scenes_follow_the_project_to_another_folder() {
        let home = scratch("copy");
        let mut project = Project::create(&home.join("A"), "A").unwrap();
        std::fs::write(home.join("Card.png"), b"card").unwrap();
        let card = project.import_card(&home.join("Card.png"), "Card").unwrap();
        std::fs::write(project.mods_dir().join("x.zipmod"), b"zip").unwrap();
        let mut scene = Scene::new();
        scene.add("Card", Content::character(PathBuf::from(&card)), None);
        project.save_scene(&scene).unwrap();

        let copy = project.copy_to(&home.join("B"), "B").unwrap();
        assert_eq!(copy.file.name, "B");
        let (opened, opened_scene, _) = Project::open(&home.join("B")).unwrap();
        // The copy's project file carries the new name, and only that one.
        assert_eq!(project_files(&home.join("B")), vec![opened.path()]);
        assert_eq!(opened_scene, scene);
        // The scene's card is the copy's own file.
        let Content::Character { card, .. } = &opened_scene.objects().last().unwrap().content else {
            panic!("no character");
        };
        assert_eq!(opened.resolve(card), home.join("B/Characters/Card.png"));
        assert!(opened.resolve(card).is_file());
        assert!(opened.mods_dir().join("x.zipmod").is_file());
        // A folder in use is refused, and so is the project's own inside.
        assert!(project.copy_to(&home.join("B"), "C").is_err());
        assert!(project.copy_to(&home.join("A/inner"), "C").is_err());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_card_is_imported_once_and_namesakes_get_a_number() {
        let home = scratch("import");
        let mut project = Project::create(&home.join("P"), "P").unwrap();
        std::fs::create_dir_all(home.join("other")).unwrap();
        std::fs::write(home.join("Aoi.png"), b"one").unwrap();
        std::fs::write(home.join("other/Aoi.png"), b"two").unwrap();
        assert_eq!(
            project.import_card(&home.join("Aoi.png"), "Aoi").unwrap(),
            "Characters/Aoi.png"
        );
        assert_eq!(
            project.import_card(&home.join("Aoi.png"), "Again").unwrap(),
            "Characters/Aoi.png"
        );
        assert_eq!(
            project.import_card(&home.join("other/Aoi.png"), "Other Aoi").unwrap(),
            "Characters/Aoi 2.png"
        );
        // A card put in the folder by hand shows under its file name.
        std::fs::write(home.join("P/Characters/by hand.png"), b"three").unwrap();
        std::fs::write(home.join("P/Characters/notes.txt"), b"").unwrap();
        let listed: Vec<(String, String)> = project.characters().into_iter().map(|c| (c.file, c.name)).collect();
        assert_eq!(
            listed,
            [
                ("Characters/Aoi 2.png".to_owned(), "Other Aoi".to_owned()),
                ("Characters/Aoi.png".to_owned(), "Aoi".to_owned()),
                ("Characters/by hand.png".to_owned(), "by hand".to_owned()),
            ]
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_missing_scene_opens_as_a_new_one() {
        let home = scratch("missing-scene");
        let mut project = Project::create(&home.join("P"), "P").unwrap();
        project.save_scene(&Scene::new()).unwrap();
        std::fs::remove_file(project.root.join(&project.file.scene)).unwrap();
        let (_, scene, note) = Project::open(&project.root).unwrap();
        assert_eq!(scene, Scene::new());
        assert!(note.is_some());
        assert!(Project::open(&home.join("nowhere")).is_err());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn new_projects_get_free_folders_and_clean_names() {
        let home = scratch("free");
        assert_eq!(free_folder(&home, "Untitled project"), home.join("Untitled project"));
        Project::create(&home.join("Untitled project"), "Untitled project").unwrap();
        // An empty folder is free; one with a project in it is not.
        std::fs::create_dir_all(home.join("Untitled project 2")).unwrap();
        assert_eq!(free_folder(&home, "Untitled project"), home.join("Untitled project 2"));
        Project::create(&home.join("Untitled project 2"), "Untitled project 2").unwrap();
        assert_eq!(free_folder(&home, "Untitled project"), home.join("Untitled project 3"));
        assert!(Project::create(&home.join("Untitled project"), "Again").is_err());
        assert_eq!(file_name("a/b: c?"), "a_b_ c_");
        assert_eq!(file_name("  . "), "Untitled");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn no_project_reads_and_writes_nothing() {
        let none = Project::none();
        assert!(none.is_none());
        assert!(none.characters().is_empty());
        assert!(none.save().is_err());
        let home = scratch("none");
        let project = Project::create(&home.join("P"), "P: one").unwrap();
        assert!(!project.is_none());
        assert_eq!(summary(&home.join("P")).map(|s| s.name).as_deref(), Some("P: one"));
        assert_eq!(summary(&home.join("nowhere")), None);
        std::fs::write(home.join("P/Characters/b.png"), b"").unwrap();
        std::fs::write(home.join("P/Characters/A.png"), b"").unwrap();
        assert_eq!(
            cards_in(&home.join("P")),
            [home.join("P/Characters/A.png"), home.join("P/Characters/b.png")]
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A PNG picture of one grey pixel.
    fn tiny_png(path: &Path) {
        let mut encoder = png::Encoder::new(std::fs::File::create(path).unwrap(), 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[90, 90, 90, 255])
            .unwrap();
    }

    #[test]
    fn a_project_names_its_icon_and_its_scene_picture_relative_to_itself() {
        let home = scratch("pictures");
        let root = home.join("P");
        let mut project = Project::create(&root, "P").unwrap();
        let none = summary(&root).unwrap();
        assert_eq!(
            (none.icon, none.thumbnail, none.scenes, none.characters),
            (None, None, 0, 0)
        );
        assert!(!std::fs::read_to_string(project.path()).unwrap().contains("icon"));

        // The scene's picture goes to a folder of its own, not the renders.
        let pixels = vec![40u8; (THUMBNAIL_SIZE[0] * THUMBNAIL_SIZE[1] * 4) as usize];
        project.set_thumbnail(Some(&pixels)).unwrap();
        assert_eq!(project.file.thumbnail.as_deref(), Some(".renkatsu/thumbnail.png"));
        assert!(root.join(".renkatsu/thumbnail.png").is_file());
        assert!(std::fs::read_dir(root.join("Renders")).unwrap().next().is_none());

        // An icon is copied in; what is no picture is refused.
        tiny_png(&home.join("logo.png"));
        std::fs::write(home.join("notes.png"), b"not a picture").unwrap();
        assert!(project.set_icon(Some(&home.join("notes.png"))).is_err());
        assert_eq!(project.file.icon, None);
        set_icon_in(&root, Some(&home.join("logo.png"))).unwrap();
        let text = std::fs::read_to_string(project.path()).unwrap();
        assert!(text.contains("icon = \"Icon.png\""), "{text}");
        assert!(text.contains("thumbnail = \".renkatsu/thumbnail.png\""), "{text}");
        assert!(!text.contains(&*home.to_string_lossy()), "{text}");

        // Scenes and characters are counted from their folders.
        let (mut project, ..) = Project::open(&root).unwrap();
        project.save_scene(&Scene::new()).unwrap();
        std::fs::write(root.join("Scenes/Second.toml"), b"").unwrap();
        std::fs::write(root.join("Scenes/notes.txt"), b"").unwrap();
        std::fs::write(root.join("Characters/a.png"), b"").unwrap();
        let both = summary(&root).unwrap();
        assert_eq!(both.icon, Some(root.join("Icon.png")));
        assert_eq!(both.thumbnail, Some(root.join(".renkatsu/thumbnail.png")));
        assert_eq!((both.scenes, both.characters), (2, 1));

        // Both follow the project to a copy, and both can go again.
        let copy = project.copy_to(&home.join("Q"), "Q").unwrap();
        assert_eq!(summary(&copy.root).unwrap().icon, Some(home.join("Q/Icon.png")));
        project.set_icon(None).unwrap();
        project.set_thumbnail(None).unwrap();
        let gone = summary(&root).unwrap();
        assert_eq!((gone.icon, gone.thumbnail), (None, None));
        assert!(!root.join("Icon.png").exists() && !root.join(".renkatsu/thumbnail.png").exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    // `user-dirs.dirs` is a Linux desktop file with Linux paths, which
    // Windows does not take as absolute.
    #[cfg(not(windows))]
    #[test]
    fn the_documents_folder_comes_from_the_desktop() {
        let home = Path::new("/home/someone");
        let text = "# comment\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_DOCUMENTS_DIR=\"$HOME/Documente\"\n";
        assert_eq!(documents_of(text, home), Some(home.join("Documente")));
        assert_eq!(
            documents_of("XDG_DOCUMENTS_DIR=\"/data/docs\"", home),
            Some(PathBuf::from("/data/docs"))
        );
        assert_eq!(documents_of("XDG_DOCUMENTS_DIR=\"$HOME/\"", home), None);
        assert_eq!(documents_of("", home), None);
    }
}
