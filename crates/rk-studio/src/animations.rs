// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The project's own animations: the files in its `Animations` folder and
//! the folders the user made there, and how animations get in.
//!
//! An animation of the project is a Renkatsu animation file (`.rkanim`):
//! one animation copied out of where it was installed, so the project
//! plays it on a machine without that package, mod or Koikatsu. Importing
//! writes such a file for every animation chosen; nothing else is copied,
//! and the bundle it came from (often tens of megabytes shared by a
//! hundred animations) stays where it is.
//!
//! Renkatsu 0.1.4 kept links instead (`.rkpose`: the bundle, controller
//! and state to play). They still list and still play from what is
//! installed, marked as linked, and can be copied in for good.

use crate::content;
use crate::project::{self, Project};
use crate::scene;
use anyhow::{Context, Result};
use rk_chara::animation_file::{self, Meta, Stored};
use std::path::{Path, PathBuf};

/// One animation of the project.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// The file relative to the project, as scenes name it.
    pub rel: String,
    /// The file's name without its extension, which is the name shown.
    pub name: String,
    /// What the file says about itself; none for a link and for a file
    /// that cannot be read.
    pub meta: Option<Meta>,
    /// What a link of Renkatsu 0.1.4 points at.
    pub link: Option<scene::Animation>,
}

impl Item {
    /// The folder the file is in, relative to the project.
    pub fn folder(&self) -> &str {
        content::parent(&self.rel)
    }

    /// Whether it holds one pose; `None` when that is not known.
    pub fn is_static(&self) -> Option<bool> {
        self.meta.as_ref().map(Meta::is_static)
    }

    /// The animation as a scene names it, played as it is.
    pub fn reference(&self) -> scene::Animation {
        let origin = |pick: fn(&Meta) -> &String, linked: fn(&scene::Animation) -> &String| {
            self.meta
                .as_ref()
                .map(pick)
                .or(self.link.as_ref().map(linked))
                .cloned()
                .unwrap_or_default()
        };
        scene::Animation {
            name: self.name.clone(),
            file: Some(self.rel.clone()),
            bundle: origin(|m| &m.bundle, |l| &l.bundle),
            controller: origin(|m| &m.controller, |l| &l.controller),
            state: origin(|m| &m.state, |l| &l.state),
            speed: self.link.as_ref().map_or(1.0, |l| l.speed),
            start: self.link.as_ref().map_or(0.0, |l| l.start),
            force_loop: self.link.as_ref().is_some_and(|l| l.force_loop),
        }
    }

    /// Where it was listed before it was imported: `Group: Category`.
    pub fn category(&self) -> String {
        match &self.meta {
            Some(m) if !m.group.is_empty() && !m.category.is_empty() => format!("{}: {}", m.group, m.category),
            Some(m) => format!("{}{}", m.group, m.category),
            None => String::new(),
        }
    }

    /// Static, or how it moves.
    pub fn kind_text(&self) -> String {
        match (&self.meta, &self.link) {
            (Some(m), _) => kind_text(m.seconds, m.loops),
            (None, Some(_)) => "Linked, not copied".to_owned(),
            (None, None) => "Cannot be read".to_owned(),
        }
    }
}

/// How an animation of some length reads in a list.
pub fn kind_text(seconds: f32, loops: bool) -> String {
    if !loops && seconds <= rk_chara::animations::STATIC_SECONDS {
        "Static".to_owned()
    } else if loops {
        format!("Moving, loops every {seconds:.1} s")
    } else {
        format!("Moving, {seconds:.1} s")
    }
}

/// What the Content panel knows about the project's animations.
#[derive(Default)]
pub struct Shelf {
    /// Every animation of the project, by folder and name.
    pub items: Vec<Item>,
    /// Which show: all, the static or the moving ones.
    pub filter: usize,
}

impl Shelf {
    /// Lists the project's animations again.
    pub fn refresh(&mut self, project: &Project) {
        self.items = if project.is_none() { Vec::new() } else { scan(project) };
    }

    pub fn find(&self, rel: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.rel == rel)
    }

    /// The item a scene's animation names: by its file, or for a scene
    /// that names none, by where the animation came from.
    pub fn of(&self, animation: &scene::Animation) -> Option<&Item> {
        match &animation.file {
            Some(file) => self.find(file),
            None => self.items.iter().find(|i| {
                let r = i.reference();
                !r.state.is_empty()
                    && (&r.bundle, &r.controller, &r.state)
                        == (&animation.bundle, &animation.controller, &animation.state)
            }),
        }
    }
}

fn is_animation(path: &Path) -> bool {
    path.extension()
        .is_some_and(|x| x.eq_ignore_ascii_case(animation_file::EXTENSION) || x.eq_ignore_ascii_case(project::OLD_LINK))
}

/// The animations in a project's folder, at any depth, by folder and
/// name.
pub fn scan(project: &Project) -> Vec<Item> {
    let mut files = Vec::new();
    project::files_below(&project.folder(project::ANIMATIONS), &mut files);
    let mut items: Vec<Item> = files
        .into_iter()
        .filter(|p| is_animation(p))
        .filter_map(|path| {
            let rel = project::relative_to(&project.root, &path)?;
            let old = path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case(project::OLD_LINK));
            let (meta, link) = if old {
                let text = std::fs::read_to_string(&path).ok();
                (None, text.and_then(|t| toml::from_str::<scene::Animation>(&t).ok()))
            } else {
                (animation_file::read_meta(&path).ok(), None)
            };
            Some(Item {
                name: content::stem(&rel).to_owned(),
                rel,
                meta,
                link,
            })
        })
        .collect();
    items.sort_by_key(|i| (content::natural(i.folder()), content::natural(&i.name)));
    items
}

/// Where an animation that is being imported comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum From {
    /// A state of a controller in a bundle that is installed.
    Installed {
        bundle: String,
        controller: String,
        state: String,
    },
    /// An animation file, of a package or picked by the user.
    File(PathBuf),
}

/// An animation chosen for import, with what its list says about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub name: String,
    pub group: String,
    pub category: String,
    /// The name of the game, package or mod that lists it.
    pub source: String,
    /// The H scenes list it, rather than CharaStudio's own menu;
    /// `None` keeps what the animation file says itself.
    pub h: Option<bool>,
    pub from: From,
}

/// What an import did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Imported {
    /// The files that are in the project now, relative to it.
    pub files: Vec<String>,
    /// Bytes written; files that were there already count nothing.
    pub bytes: u64,
    /// What could not be imported, and why.
    pub problems: Vec<String>,
}

/// Reads what a pick names, ready to be written as a file.
fn stored(game: Option<&rk_vfs::Game>, pick: &Pick) -> Result<Stored> {
    let mut stored = match &pick.from {
        From::Installed {
            bundle,
            controller,
            state,
        } => {
            let game = game.context("the content it is installed in could not be opened")?;
            Stored::extract(game, bundle, controller, state)?
        }
        From::File(path) => Stored::read(path)?,
    };
    // What the list said wins over what the file carried.
    let fill = |to: &mut String, from: &str| {
        if !from.is_empty() {
            *to = from.to_owned();
        }
    };
    fill(&mut stored.meta.name, &pick.name);
    fill(&mut stored.meta.group, &pick.group);
    fill(&mut stored.meta.category, &pick.category);
    fill(&mut stored.meta.source, &pick.source);
    if let Some(h) = pick.h {
        stored.meta.h = h;
    }
    Ok(stored)
}

/// Copies animations into a folder of the project, each as its own file
/// named after the animation. With `by_category` each goes into a folder
/// named after its group and category. An animation that is there
/// already, byte for byte, is not written again; another of the same name
/// gets a number.
pub fn import(
    project: &Project,
    game: Option<&rk_vfs::Game>,
    picks: &[Pick],
    folder: &str,
    by_category: bool,
) -> Imported {
    let mut done = Imported::default();
    for pick in picks {
        let written = stored(game, pick).and_then(|stored| {
            let mut dir = folder.to_owned();
            if by_category {
                for part in [&stored.meta.group, &stored.meta.category] {
                    if !part.trim().is_empty() {
                        dir = format!("{dir}/{}", project::file_name(part));
                    }
                }
            }
            write_in(project, &dir, &stored)
        });
        match written {
            Ok((rel, bytes)) => {
                done.bytes += bytes;
                if !done.files.contains(&rel) {
                    done.files.push(rel);
                }
            }
            Err(e) => done.problems.push(format!("{}: {e:#}", pick.name)),
        }
    }
    done
}

/// Writes an animation into a folder of the project and returns its path
/// and the bytes written.
fn write_in(project: &Project, folder: &str, stored: &Stored) -> Result<(String, u64)> {
    if content::top(folder) != project::ANIMATIONS || folder.split('/').any(|p| p.is_empty() || p == "..") {
        anyhow::bail!("{folder} is not a folder of the project's animations");
    }
    let dir = project.root.join(folder);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let bytes = stored.to_bytes();
    let stem = project::file_name(&stored.meta.name);
    for n in 1.. {
        let name = match n {
            1 => format!("{stem}.{}", animation_file::EXTENSION),
            n => format!("{stem} {n}.{}", animation_file::EXTENSION),
        };
        let target = dir.join(&name);
        match std::fs::read(&target) {
            Ok(there) if there == bytes => return Ok((format!("{folder}/{name}"), 0)),
            Ok(_) => continue,
            Err(_) => {
                std::fs::write(&target, &bytes).with_context(|| format!("writing {}", target.display()))?;
                return Ok((format!("{folder}/{name}"), bytes.len() as u64));
            }
        }
    }
    unreachable!("some number is free")
}

/// Copies a linked animation into the project for good: the animation
/// file is written beside the link and the link goes to the trash.
/// Returns the old and the new path.
pub fn copy_in(project: &mut Project, game: &rk_vfs::Game, item: &Item) -> Result<(String, String)> {
    let link = item.link.as_ref().context("it is in the project already")?;
    let pick = Pick {
        name: item.name.clone(),
        group: String::new(),
        category: String::new(),
        source: String::new(),
        h: None,
        from: From::Installed {
            bundle: link.bundle.clone(),
            controller: link.controller.clone(),
            state: link.state.clone(),
        },
    };
    let (rel, _) = write_in(project, item.folder(), &stored(Some(game), &pick)?)?;
    content::trash(project, &item.rel)?;
    Ok((item.rel.clone(), rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("renkatsu-animations-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A package's animation file on disk, to import.
    fn file(home: &Path, name: &str, seconds: f32, loops: bool) -> Pick {
        let path = home.join(format!("{name}.rkanim"));
        animation_file::sample(name, seconds, loops).write(&path).unwrap();
        Pick {
            name: name.to_owned(),
            group: "Studio".into(),
            category: "Dance".into(),
            source: "Dance pack".into(),
            h: None,
            from: From::File(path),
        }
    }

    #[test]
    fn importing_copies_animations_into_the_project_and_lists_them() {
        let home = scratch("import");
        let project = Project::create(&home.join("P"), "P").unwrap();
        assert!(scan(&project).is_empty(), "a new project has no animations");
        let picks = [file(&home, "Spin", 3.0, true), file(&home, "Bow: deep", 0.15, false)];
        let done = import(&project, None, &picks, "Animations", false);
        assert_eq!(done.problems, Vec::<String>::new());
        assert_eq!(done.files, ["Animations/Spin.rkanim", "Animations/Bow_ deep.rkanim"]);
        assert!(done.bytes > 0);

        // The project's files are all it needs: the sources may go.
        std::fs::remove_file(home.join("Spin.rkanim")).unwrap();
        std::fs::remove_file(home.join("Bow: deep.rkanim")).unwrap();
        let items = scan(&project);
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["Bow_ deep", "Spin"]);
        let spin = &items[1];
        assert_eq!(spin.category(), "Studio: Dance");
        assert_eq!(spin.meta.as_ref().unwrap().source, "Dance pack");
        assert_eq!(spin.kind_text(), "Moving, loops every 3.0 s");
        assert_eq!(
            (items[0].kind_text().as_str(), items[0].is_static()),
            ("Static", Some(true))
        );
        let named = spin.reference();
        assert_eq!(named.file.as_deref(), Some("Animations/Spin.rkanim"));
        assert_eq!((named.name.as_str(), named.speed), ("Spin", 1.0));
        // And it plays from the project's file.
        let stored = Stored::read(&project.root.join(&spin.rel)).unwrap();
        let parameters = rk_chara::motion::Parameters {
            height: 0.5,
            breast: 0.5,
        };
        assert_eq!(stored.motion(&parameters).unwrap().duration(), 3.0);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_same_animation_is_imported_once_and_namesakes_get_a_number() {
        let home = scratch("twice");
        let project = Project::create(&home.join("P"), "P").unwrap();
        let spin = file(&home, "Spin", 3.0, true);
        let first = import(&project, None, std::slice::from_ref(&spin), "Animations", false);
        let again = import(&project, None, std::slice::from_ref(&spin), "Animations", false);
        assert_eq!(again.files, first.files);
        assert_eq!(again.bytes, 0);
        // Another animation that is called the same.
        let other = file(&home, "Spin", 5.0, false);
        let third = import(&project, None, &[other], "Animations", false);
        assert_eq!(third.files, ["Animations/Spin 2.rkanim"]);

        // By category, into folders named after the group and category.
        let sorted = import(&project, None, &[spin], "Animations/Mine", true);
        assert_eq!(sorted.files, ["Animations/Mine/Studio/Dance/Spin.rkanim"]);
        let folders = content::folders(&project, "Animations");
        assert_eq!(
            folders,
            [
                "Animations/Mine",
                "Animations/Mine/Studio",
                "Animations/Mine/Studio/Dance"
            ]
        );
        // A file that is no animation, a folder outside the animations and
        // an installed animation without the content it is in are refused.
        // Its own name: on Windows, broken.rkanim and the Broken.rkanim
        // that `file` writes are the same file.
        std::fs::write(home.join("not an animation.rkanim"), b"not an animation").unwrap();
        let broken = Pick {
            from: From::File(home.join("not an animation.rkanim")),
            ..file(&home, "Broken", 1.0, false)
        };
        let installed = Pick {
            from: From::Installed {
                bundle: "studio/anime/00.unity3d".into(),
                controller: "tachi".into(),
                state: "f_tachi_00".into(),
            },
            ..file(&home, "Stand", 1.0, false)
        };
        let done = import(&project, None, &[broken, installed], "Animations", false);
        assert!(done.files.is_empty());
        assert_eq!(done.problems.len(), 2, "{:?}", done.problems);
        let outside = import(&project, None, &[file(&home, "Spin", 3.0, true)], "Characters", false);
        assert_eq!((outside.files.len(), outside.problems.len()), (0, 1));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn links_of_the_old_poses_folder_list_as_linked_animations() {
        let home = scratch("links");
        let root = home.join("P");
        Project::create(&root, "P").unwrap();
        let link = "name = \"Stand 1\"\nbundle = \"studio/anime/00.unity3d\"\ncontroller = \"tachi\"\n\
                    state = \"f_tachi_00\"\nspeed = 0.5\nstart = 0.25\nforce_loop = true\n";
        std::fs::write(root.join("Poses/Stand 1.rkpose"), link).unwrap();
        // Opening the project moves the link to where animations are.
        let (project, ..) = Project::open(&root).unwrap();
        let items = scan(&project);
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.rel, "Animations/Stand 1.rkpose");
        assert_eq!(item.kind_text(), "Linked, not copied");
        let named = item.reference();
        assert_eq!(named.file.as_deref(), Some("Animations/Stand 1.rkpose"));
        assert_eq!(
            (named.bundle.as_str(), named.controller.as_str(), named.state.as_str()),
            ("studio/anime/00.unity3d", "tachi", "f_tachi_00")
        );
        assert_eq!((named.speed, named.start, named.force_loop), (0.5, 0.25, true));
        // A scene of that version names the animation without a file, and
        // still finds the item.
        let mut shelf = Shelf::default();
        shelf.refresh(&project);
        let old = scene::Animation { file: None, ..named };
        assert_eq!(
            shelf.of(&old).map(|i| i.rel.as_str()),
            Some("Animations/Stand 1.rkpose")
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_animation_moved_into_a_folder_keeps_its_scene() {
        use crate::scene::{Content, Scene};
        let home = scratch("move");
        let mut project = Project::create(&home.join("P"), "P").unwrap();
        let done = import(&project, None, &[file(&home, "Spin", 3.0, true)], "Animations", false);
        let item = scan(&project).remove(0);
        let mut scene = Scene::new();
        let mut content = Content::character(PathBuf::from("Characters/anna.png"));
        if let Content::Character { animation, .. } = &mut content {
            *animation = Some(item.reference());
        }
        scene.add("Anna", content, None);
        project.save_scene(&scene).unwrap();

        let folder = content::new_folder(&project, "Animations", "Dances").unwrap();
        content::move_into(&mut project, &done.files[0], &folder).unwrap();
        content::rename(&mut project, "Animations/Dances/Spin.rkanim", "Twirl").unwrap();
        let (project, scene, _) = Project::open(&project.root).unwrap();
        let Content::Character { animation, .. } = &scene.objects().last().unwrap().content else {
            panic!("no character");
        };
        let file = animation.as_ref().unwrap().file.clone().unwrap();
        assert_eq!(file, "Animations/Dances/Twirl.rkanim");
        assert!(project.root.join(&file).is_file());
        assert_eq!(scan(&project)[0].name, "Twirl");
        let _ = std::fs::remove_dir_all(&home);
    }
}
