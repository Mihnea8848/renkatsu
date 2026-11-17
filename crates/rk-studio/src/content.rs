// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! What the Content panel does to the project's files: rename, move,
//! duplicate, copy, delete and new folder, on the project folder on disk.
//!
//! Files are named by their path relative to the project, with forward
//! slashes, as scenes name them. Every change that moves a file says what
//! moved, so that the project file, the scenes on disk and the scene
//! being edited follow it: renaming a card never breaks a scene.
//!
//! The project's own folders (Characters, Animations and the others) stay
//! as they are, and a file stays inside the one it is in: a card does not
//! become an animation by being dragged.
//!
//! Deleting moves a file to the project's trash, `.renkatsu/trash`, from
//! where it can be taken back by hand.

use crate::project::{self, file_name, relative_to, Project};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Where deleted files go, relative to the project.
pub const TRASH: &str = ".renkatsu/trash";

/// The files and folders an operation moved: old path, new path.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Moved(pub Vec<(String, String)>);

impl Moved {
    /// Where a file is now, when it or a folder around it moved.
    pub fn target(&self, old: &str) -> Option<String> {
        self.0.iter().find_map(|(from, to)| {
            if old == from {
                Some(to.clone())
            } else {
                let rest = old.strip_prefix(from.as_str())?.strip_prefix('/')?;
                Some(format!("{to}/{rest}"))
            }
        })
    }
}

/// A key that orders names as people read them: without regard to case,
/// and numbers by their value, so that `Pose 2` comes before `Pose 10`.
pub fn natural(name: &str) -> Vec<(String, Option<u64>)> {
    let mut key = Vec::new();
    let mut text = String::new();
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        match c.to_digit(10) {
            Some(first) => {
                let mut number = u64::from(first);
                while let Some(digit) = chars.peek().and_then(|c| c.to_digit(10)) {
                    number = number.saturating_mul(10).saturating_add(u64::from(digit));
                    chars.next();
                }
                key.push((std::mem::take(&mut text), Some(number)));
            }
            None => text.extend(c.to_lowercase()),
        }
    }
    if !text.is_empty() {
        key.push((text, None));
    }
    key
}

/// The project folder a path is in: its first part.
pub fn top(rel: &str) -> &str {
    rel.split('/').next().unwrap_or(rel)
}

/// The folder a path is in, or nothing for a project folder itself.
pub fn parent(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// The last part of a path.
pub fn leaf(rel: &str) -> &str {
    rel.rsplit_once('/').map_or(rel, |(_, name)| name)
}

/// A file's name without its extension; a folder's name as it is.
pub fn stem(rel: &str) -> &str {
    let name = leaf(rel);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

fn extension(rel: &str) -> Option<&str> {
    let name = leaf(rel);
    name.rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(_, e)| e)
}

/// Whether a path names something the user may change: it lies inside one
/// of the project's folders, below the folder itself, and climbs nowhere.
pub fn changeable(rel: &str) -> bool {
    let mut parts = rel.split('/');
    let inside = parts.next().is_some_and(|first| project::FOLDERS.contains(&first));
    let rest: Vec<&str> = parts.collect();
    inside && !rest.is_empty() && rest.iter().all(|p| !p.is_empty() && *p != "." && *p != "..")
}

/// Whether a path names a folder things can be put into: one of the
/// project's folders or a folder below one.
fn is_folder(project: &Project, rel: &str) -> bool {
    project::FOLDERS.contains(&top(rel))
        && rel.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
        && project.root.join(rel).is_dir()
}

/// A name in `dir` that is free: `stem`, or `stem` with the first free
/// number, with the extension when there is one.
fn free_name(dir: &Path, stem: &str, extension: Option<&str>) -> String {
    let named = |n: u32| {
        let stem = if n == 1 { stem.to_owned() } else { format!("{stem} {n}") };
        match extension {
            Some(e) => format!("{stem}.{e}"),
            None => stem,
        }
    };
    (1..)
        .map(named)
        .find(|name| !dir.join(name).exists())
        .expect("some number is free")
}

fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    }
}

/// Brings the project in line with what moved.
fn follow(project: &mut Project, moved: &Moved) -> Result<()> {
    project.retarget(&|old| moved.target(old))
}

/// Renames a file or a folder. A file keeps its extension. A card is
/// listed under its new name too.
pub fn rename(project: &mut Project, rel: &str, name: &str) -> Result<Moved> {
    if !changeable(rel) {
        bail!("{rel} cannot be renamed");
    }
    let from = project.root.join(rel);
    let is_file = from.is_file();
    let clean = file_name(name);
    let ext = extension(rel).filter(|_| is_file);
    // Typing the extension along is fine.
    let clean = match ext {
        Some(e) => clean
            .strip_suffix(&format!(".{e}"))
            .filter(|s| !s.is_empty())
            .unwrap_or(&clean)
            .to_owned(),
        None => clean,
    };
    let new_leaf = match ext {
        Some(e) => format!("{clean}.{e}"),
        None => clean.clone(),
    };
    let to_rel = join(parent(rel), &new_leaf);
    if to_rel == rel {
        return Ok(Moved::default());
    }
    let to = project.root.join(&to_rel);
    // On file systems that ignore case, a change of capitals is the same file.
    let same_file = to_rel.to_lowercase() == rel.to_lowercase();
    if to.exists() && !same_file {
        bail!("{new_leaf} already exists in this folder");
    }
    std::fs::rename(&from, &to).with_context(|| format!("renaming {}", from.display()))?;
    let moved = Moved(vec![(rel.to_owned(), to_rel.clone())]);
    follow(project, &moved)?;
    if is_file && top(rel) == project::CHARACTERS {
        project.name_card(&to_rel, &clean);
        project.save()?;
    }
    Ok(moved)
}

/// Moves a file or a folder into another folder of the same project
/// folder. A name that is taken there gets a number.
pub fn move_into(project: &mut Project, rel: &str, folder: &str) -> Result<Moved> {
    if !changeable(rel) {
        bail!("{rel} cannot be moved");
    }
    if !is_folder(project, folder) {
        bail!("{folder} is not a folder of the project");
    }
    if top(rel) != top(folder) {
        bail!("{} stays in {}", leaf(rel), top(rel));
    }
    if folder == rel || folder.starts_with(&format!("{rel}/")) {
        bail!("a folder cannot go inside itself");
    }
    if parent(rel) == folder {
        return Ok(Moved::default());
    }
    let from = project.root.join(rel);
    let dir = project.root.join(folder);
    let ext = extension(rel).filter(|_| from.is_file());
    let stem_of = if ext.is_some() { stem(rel) } else { leaf(rel) };
    let name = free_name(&dir, stem_of, ext);
    let to_rel = join(folder, &name);
    std::fs::rename(&from, project.root.join(&to_rel)).with_context(|| format!("moving {}", from.display()))?;
    let moved = Moved(vec![(rel.to_owned(), to_rel)]);
    follow(project, &moved)?;
    Ok(moved)
}

fn copy_all(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(from, to).with_context(|| format!("copying {}", from.display()))?;
    }
    Ok(())
}

/// Copies a file or a folder into a folder of the same project folder,
/// under a free name, and returns the copy's path. Copies of cards are
/// listed under the names of their originals.
pub fn copy_into(project: &mut Project, rel: &str, folder: &str) -> Result<String> {
    if !changeable(rel) {
        bail!("{rel} cannot be copied");
    }
    if !is_folder(project, folder) {
        bail!("{folder} is not a folder of the project");
    }
    if top(rel) != top(folder) {
        bail!("{} stays in {}", leaf(rel), top(rel));
    }
    if folder == rel || folder.starts_with(&format!("{rel}/")) {
        bail!("a folder cannot be copied into itself");
    }
    let from = project.root.join(rel);
    if !from.exists() {
        bail!("{rel} is gone");
    }
    let dir = project.root.join(folder);
    let ext = extension(rel).filter(|_| from.is_file());
    let stem_of = if ext.is_some() { stem(rel) } else { leaf(rel) };
    let to_rel = join(folder, &free_name(&dir, stem_of, ext));
    copy_all(&from, &project.root.join(&to_rel))?;
    if top(rel) == project::CHARACTERS {
        let copied = Moved(vec![(rel.to_owned(), to_rel.clone())]);
        let named: Vec<(String, String)> = project
            .file
            .characters
            .iter()
            .filter_map(|c| Some((copied.target(&c.file)?, c.name.clone())))
            .collect();
        for (file, name) in &named {
            project.name_card(file, name);
        }
        if !named.is_empty() {
            project.save()?;
        }
    }
    Ok(to_rel)
}

/// Copies a file or a folder beside itself.
pub fn duplicate(project: &mut Project, rel: &str) -> Result<String> {
    copy_into(project, rel, parent(rel))
}

/// Makes a folder inside a project folder or a folder below one, under a
/// free name, and returns its path.
pub fn new_folder(project: &Project, inside: &str, name: &str) -> Result<String> {
    if !is_folder(project, inside) {
        bail!("{inside} is not a folder of the project");
    }
    let dir = project.root.join(inside);
    let name = free_name(&dir, &file_name(name), None);
    std::fs::create_dir(dir.join(&name)).with_context(|| format!("making {name}"))?;
    Ok(join(inside, &name))
}

/// Moves a file or a folder to the project's trash.
pub fn trash(project: &mut Project, rel: &str) -> Result<()> {
    if !changeable(rel) {
        bail!("{rel} cannot be deleted");
    }
    let from = project.root.join(rel);
    let bin = project.root.join(TRASH);
    let slot = (1..)
        .map(|n| bin.join(n.to_string()))
        .find(|p| !p.exists())
        .expect("some number is free");
    std::fs::create_dir_all(&slot).with_context(|| format!("making {}", slot.display()))?;
    std::fs::rename(&from, slot.join(leaf(rel))).with_context(|| format!("deleting {}", from.display()))?;
    if project.forget_missing_cards() {
        project.save()?;
    }
    Ok(())
}

/// The folders below a project folder, at any depth, as paths relative
/// to the project, in the order a tree shows them.
pub fn folders(project: &Project, top: &str) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        found.sort_by_key(|p| p.file_name().map(|n| natural(&n.to_string_lossy())));
        for path in found {
            if let Some(rel) = relative_to(root, &path) {
                out.push(rel);
            }
            walk(root, &path, out);
        }
    }
    let mut out = Vec::new();
    if !project.is_none() {
        walk(&project.root, &project.root.join(top), &mut out);
    }
    out
}

/// The files directly in a folder, by name, relative to the project.
pub fn files(project: &Project, folder: &str) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(project.root.join(folder))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter_map(|p| relative_to(&project.root, &p))
        .collect();
    found.sort_by_key(|f| natural(f));
    found
}

/// Shows a file in the system's file manager: its folder opens. Nothing
/// waits for the file manager.
pub fn reveal(path: &Path) -> Result<()> {
    let folder = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(folder)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("starting {program}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Content, Scene};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("renkatsu-content-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A project with Anna's card in a scene that is saved.
    fn project(home: &Path) -> Project {
        let mut project = Project::create(&home.join("P"), "P").unwrap();
        std::fs::write(home.join("anna.png"), b"card").unwrap();
        let card = project.import_card(&home.join("anna.png"), "Anna").unwrap();
        let mut scene = Scene::new();
        scene.add("Anna", Content::character(PathBuf::from(card)), None);
        project.save_scene(&scene).unwrap();
        project
    }

    fn card_in_scene(project: &Project) -> String {
        let (_, scene, _) = Project::open(&project.root).unwrap();
        let Content::Character { card, .. } = &scene.objects().last().unwrap().content else {
            panic!("no character");
        };
        card.to_string_lossy().into_owned()
    }

    #[test]
    fn renaming_and_moving_a_card_keeps_the_scene_on_it() {
        let home = scratch("rename");
        let mut p = project(&home);
        let moved = rename(&mut p, "Characters/anna.png", "Anna Kato").unwrap();
        assert_eq!(moved.target("Characters/anna.png").unwrap(), "Characters/Anna Kato.png");
        assert!(p.root.join("Characters/Anna Kato.png").is_file());
        assert_eq!(card_in_scene(&p), "Characters/Anna Kato.png");
        assert_eq!(p.character_name("Characters/Anna Kato.png"), Some("Anna Kato"));
        // The same name again, with the extension typed along, changes nothing.
        assert_eq!(
            rename(&mut p, "Characters/Anna Kato.png", "Anna Kato.png").unwrap(),
            Moved::default()
        );

        let folder = new_folder(&p, "Characters", "Cast").unwrap();
        assert_eq!(folder, "Characters/Cast");
        assert_eq!(new_folder(&p, "Characters", "Cast").unwrap(), "Characters/Cast 2");
        let moved = move_into(&mut p, "Characters/Anna Kato.png", &folder).unwrap();
        assert_eq!(moved.0[0].1, "Characters/Cast/Anna Kato.png");
        assert_eq!(card_in_scene(&p), "Characters/Cast/Anna Kato.png");
        // The folder is renamed around her, and moved into another.
        rename(&mut p, "Characters/Cast", "Main cast").unwrap();
        assert_eq!(card_in_scene(&p), "Characters/Main cast/Anna Kato.png");
        move_into(&mut p, "Characters/Main cast", "Characters/Cast 2").unwrap();
        assert_eq!(card_in_scene(&p), "Characters/Cast 2/Main cast/Anna Kato.png");
        assert_eq!(
            p.character_name("Characters/Cast 2/Main cast/Anna Kato.png"),
            Some("Anna Kato")
        );
        assert_eq!(
            folders(&p, "Characters"),
            ["Characters/Cast 2", "Characters/Cast 2/Main cast"]
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn what_cannot_be_done_is_refused() {
        let home = scratch("refuse");
        let mut p = project(&home);
        // The project's own folders stay, and nothing leaves the project.
        assert!(rename(&mut p, "Characters", "People").is_err());
        assert!(trash(&mut p, "Animations").is_err());
        assert!(rename(&mut p, "Characters/../P.rkproj", "x").is_err());
        assert!(trash(&mut p, "P.rkproj").is_err());
        assert!(new_folder(&p, "Elsewhere", "x").is_err());
        assert!(new_folder(&p, "Characters/..", "x").is_err());
        // A card stays among the characters.
        assert!(move_into(&mut p, "Characters/anna.png", "Animations").is_err());
        assert!(copy_into(&mut p, "Characters/anna.png", "Animations").is_err());
        // A name that is taken is refused; a folder does not go into itself.
        std::fs::write(p.root.join("Characters/bea.png"), b"bea").unwrap();
        assert!(rename(&mut p, "Characters/bea.png", "anna").is_err());
        let folder = new_folder(&p, "Characters", "Cast").unwrap();
        assert!(move_into(&mut p, &folder, &folder).is_err());
        // Signs a file name cannot hold are replaced.
        let moved = rename(&mut p, "Characters/bea.png", "Bea: take 2").unwrap();
        assert_eq!(moved.0[0].1, "Characters/Bea_ take 2.png");
        assert_eq!(card_in_scene(&p), "Characters/anna.png");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn copies_get_free_names_and_deleted_files_go_to_the_trash() {
        let home = scratch("copy");
        let mut p = project(&home);
        assert_eq!(
            duplicate(&mut p, "Characters/anna.png").unwrap(),
            "Characters/anna 2.png"
        );
        assert_eq!(
            duplicate(&mut p, "Characters/anna.png").unwrap(),
            "Characters/anna 3.png"
        );
        assert_eq!(p.character_name("Characters/anna 2.png"), Some("Anna"));
        let folder = new_folder(&p, "Characters", "Cast").unwrap();
        assert_eq!(
            copy_into(&mut p, "Characters/anna.png", &folder).unwrap(),
            "Characters/Cast/anna.png"
        );
        // A folder is copied with what it holds.
        assert_eq!(duplicate(&mut p, &folder).unwrap(), "Characters/Cast 2");
        assert!(p.root.join("Characters/Cast 2/anna.png").is_file());
        assert_eq!(
            files(&p, "Characters"),
            ["Characters/anna 2.png", "Characters/anna 3.png", "Characters/anna.png"]
        );
        // Numbers in names count by their value.
        let mut names = vec!["Pose 10", "pose 2", "Pose", "Pose 1", "Bow"];
        names.sort_by_key(|n| natural(n));
        assert_eq!(names, ["Bow", "Pose", "Pose 1", "pose 2", "Pose 10"]);

        trash(&mut p, "Characters/anna 2.png").unwrap();
        trash(&mut p, "Characters/Cast 2").unwrap();
        assert!(!p.root.join("Characters/anna 2.png").exists());
        assert!(p.root.join(".renkatsu/trash/1/anna 2.png").is_file());
        assert!(p.root.join(".renkatsu/trash/2/Cast 2/anna.png").is_file());
        assert_eq!(p.character_name("Characters/anna 2.png"), None);
        // The scene's own card was never touched.
        assert_eq!(card_in_scene(&p), "Characters/anna.png");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn paths_are_taken_apart_as_scenes_write_them() {
        assert_eq!(top("Animations/Sitting/Sit.rkanim"), "Animations");
        assert_eq!(parent("Animations/Sitting/Sit.rkanim"), "Animations/Sitting");
        assert_eq!(parent("Animations"), "");
        assert_eq!(leaf("Animations/Sitting/Sit.rkanim"), "Sit.rkanim");
        assert_eq!(stem("Animations/Sitting/Sit 1.5.rkanim"), "Sit 1.5");
        assert_eq!(stem("Animations/Sitting"), "Sitting");
        assert!(changeable("Animations/a.rkanim") && !changeable("Animations") && !changeable("x/a"));
        let moved = Moved(vec![("Animations/A".into(), "Animations/B".into())]);
        assert_eq!(moved.target("Animations/A/x.rkanim").unwrap(), "Animations/B/x.rkanim");
        assert_eq!(moved.target("Animations/A"), Some("Animations/B".into()));
        assert_eq!(moved.target("Animations/Ab/x.rkanim"), None);
    }
}
