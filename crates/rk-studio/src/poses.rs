// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The poses of the Content panel: CharaStudio's list of poses and
//! animations, read from the user's game or from an installed package,
//! and the poses the project saved for itself.
//!
//! The list keeps CharaStudio's own groups and categories, which its users
//! know, and adds what the list does not say: whether an entry holds one
//! pose (static) or moves (dynamic). That is read from the animation
//! itself, a few entries a frame, so the list shows at once.
//!
//! A pose of the project is a small text file in its `Poses` folder: the
//! animation it names and how it is played.

use crate::kinds::Kind;
use crate::project::Project;
use crate::scene;
use crate::state::{Command, Look, State};
use crate::widgets::{glyph, hair_h, label};
use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};
use rk_chara::poses::Library;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The extension of a pose saved in a project.
pub const EXTENSION: &str = "rkpose";

/// Time spent reading animations in one frame.
const BUDGET: Duration = Duration::from_millis(6);

/// The group of the tree that holds the project's own poses.
pub const PROJECT: i32 = -1;

/// A pose saved in the project.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    /// The file's name without its extension.
    pub name: String,
    pub pose: scene::Pose,
}

/// What the Content panel knows about poses.
#[derive(Default)]
pub struct Shelf {
    /// CharaStudio's list, once it was read.
    pub library: Option<Library>,
    /// Why the list could not be read.
    pub problem: Option<String>,
    /// The project's own poses.
    pub saved: Vec<Saved>,
    /// Which entries show: all, the static or the dynamic ones.
    pub filter: usize,
    /// The group shown: all of them, the project's poses, or one of
    /// CharaStudio's groups.
    pub group: Option<i32>,
    /// The pose selected in the list.
    pub selected: Option<scene::Pose>,
    /// Read every animation at once, for scripted pictures.
    pub eager: bool,
}

/// The poses saved in a project's folder, by name.
pub fn saved_in(dir: &Path) -> Vec<Saved> {
    let mut out: Vec<Saved> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == EXTENSION))
        .filter_map(|p| {
            let pose = toml::from_str::<scene::Pose>(&std::fs::read_to_string(&p).ok()?).ok()?;
            Some(Saved {
                name: p.file_stem()?.to_string_lossy().into_owned(),
                pose,
            })
        })
        .collect();
    out.sort_by_key(|s| s.name.to_lowercase());
    out
}

/// Saves a pose in a project's folder under a free name made from the
/// pose's own, and returns the file.
pub fn save_in(dir: &Path, pose: &scene::Pose) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let clean: String = pose
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_,".contains(c) {
                c
            } else {
                ' '
            }
        })
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let base = if clean.is_empty() { "Pose".to_owned() } else { clean };
    let file = (1..)
        .map(|n| match n {
            1 => dir.join(format!("{base}.{EXTENSION}")),
            n => dir.join(format!("{base} {n}.{EXTENSION}")),
        })
        .find(|p| !p.exists())
        .expect("some number is free");
    std::fs::write(&file, toml::to_string(pose)?)?;
    Ok(file)
}

impl Shelf {
    /// Lists the project's poses again and forgets the game's list, which
    /// is read again when next shown.
    pub fn refresh(&mut self, project: &Project) {
        self.saved = if project.is_none() {
            Vec::new()
        } else {
            saved_in(&project.folder("Poses"))
        };
        self.library = None;
        self.problem = None;
    }
}

/// Reads the game's list when it is not read yet, and moves the reading of
/// the animations' timing on by a frame's share. Returns whether more is
/// left to read.
fn advance(st: &mut State) -> bool {
    if st.project.is_none() {
        return false;
    }
    let game = match st.games.get(&st.project.mods_dir()) {
        Ok(game) => game,
        Err(e) => {
            st.poses.problem = Some(format!("{e:#}"));
            return false;
        }
    };
    let library = st.poses.library.get_or_insert_with(|| Library::read(game));
    let started = Instant::now();
    let eager = st.poses.eager;
    !library.time(game, &|| !eager && started.elapsed() > BUDGET)
}

/// One line of the list.
enum Line {
    /// The name of a category.
    Header(String, usize),
    Pose(scene::Pose, Option<rk_chara::poses::Timing>),
}

fn kind_text(timing: Option<rk_chara::poses::Timing>) -> String {
    match timing {
        None => String::new(),
        Some(t) if t.is_static() => "Static".to_owned(),
        Some(t) if t.loops => format!("Dynamic, loops every {:.1} s", t.seconds),
        Some(t) => format!("Dynamic, {:.1} s", t.seconds),
    }
}

/// The groups of the tree under Poses: what each shows and its name.
pub fn groups(st: &State) -> Vec<(Option<i32>, String)> {
    let mut out = vec![(None, "All".to_owned()), (Some(PROJECT), "Project".to_owned())];
    if let Some(library) = &st.poses.library {
        out.extend(library.groups().into_iter().map(|(id, name, _)| (Some(id), name)));
    }
    out
}

/// The toolbar of the Poses folder.
pub fn toolbar(ui: &mut Ui, k: &Look, st: &mut State) {
    use crate::widgets::{flat_button, segmented};
    use egui_phosphor::regular as icon;
    let character = st.doc.selection().is_some_and(|o| o.kind() == Kind::Character);
    let chosen = st.poses.selected.clone();
    let why = match (character, chosen.is_some()) {
        (false, _) => "Select a character in the scene first",
        (true, false) => "Select a pose first, or double click one",
        (true, true) => "Put the selected character in the selected pose",
    };
    ui.add_enabled_ui(character && chosen.is_some(), |ui| {
        if flat_button(
            ui,
            k,
            Some(icon::PERSON_ARMS_SPREAD),
            "Apply to selection",
            false,
            false,
        )
        .on_hover_text(why)
        .on_disabled_hover_text(why)
        .clicked()
        {
            st.run(Command::Pose(chosen));
        }
    });
    let posed = st
        .doc
        .selection()
        .is_some_and(|o| matches!(&o.content, scene::Content::Character { pose: Some(_), .. }));
    ui.add_enabled_ui(posed, |ui| {
        let why = "Take the pose off the selected character";
        if flat_button(ui, k, Some(icon::X), "Clear pose", false, false)
            .on_hover_text(why)
            .on_disabled_hover_text("The selected character has no pose")
            .clicked()
        {
            st.run(Command::Pose(None));
        }
    });
    let (rect, _) = ui.allocate_exact_size(Vec2::new(210.0, k.field()), Sense::hover());
    segmented(
        ui,
        k,
        rect,
        ui.id().with("pose filter"),
        &mut st.poses.filter,
        &["All", "Static", "Dynamic"],
    );
}

/// The list of poses in `area`.
pub fn list(ui: &mut Ui, k: &Look, st: &mut State, area: Rect) {
    let t = &k.t;
    let reading = advance(st);
    if reading {
        ui.ctx().request_repaint();
    }
    let note = |ui: &Ui, text: &str| {
        ui.painter().text(
            area.min + Vec2::new(14.0, 20.0),
            Align2::LEFT_CENTER,
            text,
            k.font(),
            t.text_dim,
        );
    };
    if let Some(problem) = &st.poses.problem {
        note(ui, &format!("The poses could not be read: {problem}"));
        return;
    }
    let search = st.content_search.trim().to_lowercase();
    let matches = |name: &str| search.is_empty() || name.to_lowercase().contains(&search);
    let (filter, group) = (st.poses.filter, st.poses.group);
    let fits = |timing: Option<rk_chara::poses::Timing>| match (filter, timing) {
        (0, _) => true,
        (_, None) => false,
        (1, Some(t)) => t.is_static(),
        (_, Some(t)) => !t.is_static(),
    };
    let mut lines: Vec<Line> = Vec::new();
    if group.is_none_or(|g| g == PROJECT) && filter == 0 {
        let saved: Vec<&Saved> = st.poses.saved.iter().filter(|s| matches(&s.name)).collect();
        if !saved.is_empty() {
            lines.push(Line::Header("Project".to_owned(), saved.len()));
        }
        for s in saved {
            let mut pose = s.pose.clone();
            pose.name = s.name.clone();
            lines.push(Line::Pose(pose, None));
        }
    }
    let (mut total, mut timed) = (0, 0);
    if let Some(library) = &st.poses.library {
        (total, timed) = (library.poses.len(), library.timed());
        for category in library.categories() {
            if group.is_some_and(|g| g != category.group) {
                continue;
            }
            let poses: Vec<&rk_chara::poses::Pose> = library
                .poses
                .iter()
                .filter(|p| (p.group, p.category) == (category.group, category.category))
                .filter(|p| fits(p.timing) && (matches(&p.name) || matches(&category.name)))
                .collect();
            if poses.is_empty() {
                continue;
            }
            lines.push(Line::Header(category.name.clone(), poses.len()));
            lines.extend(poses.into_iter().map(|p| Line::Pose(scene::Pose::of(p), p.timing)));
        }
    }
    if lines.is_empty() {
        let text = if total == 0 && group != Some(PROJECT) {
            "No poses are installed. Starter Content: Poses in File, Packages brings CharaStudio's, \
             and so does choosing your Koikatsu folder."
        } else if group == Some(PROJECT) && st.poses.saved.is_empty() {
            "The project has no poses of its own yet. Save one from a posed character's Properties."
        } else if filter != 0 && timed < total {
            "Reading the animations"
        } else {
            "No pose matches"
        };
        note(ui, text);
        return;
    }

    let applied = st.doc.selection().and_then(|o| match &o.content {
        scene::Content::Character { pose, .. } => pose.clone(),
        _ => None,
    });
    let same = |a: &scene::Pose, b: &scene::Pose| {
        (&a.bundle, &a.controller, &a.state, &a.name) == (&b.bundle, &b.controller, &b.state, &b.name)
    };
    let mut select = None;
    let mut apply = None;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
    child.set_clip_rect(area.intersect(ui.clip_rect()));
    egui::ScrollArea::vertical()
        .id_salt("poses")
        .auto_shrink(false)
        .show_rows(&mut child, k.d.row, lines.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for i in range {
                let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row), Sense::click());
                let p = ui.painter();
                let y = row.center().y;
                match &lines[i] {
                    Line::Header(name, count) => {
                        p.rect_filled(row, 0.0, t.header);
                        hair_h(p, row.left(), row.right(), row.bottom() - 1.0, t.line);
                        let x = label(p, k, Pos2::new(row.left() + 12.0, y), name, t.text_bright);
                        p.text(
                            Pos2::new(x + 8.0, y),
                            Align2::LEFT_CENTER,
                            count.to_string(),
                            k.small(),
                            t.text_dim,
                        );
                    }
                    Line::Pose(pose, timing) => {
                        let on = st.poses.selected.as_ref().is_some_and(|s| same(s, pose));
                        let worn = applied.as_ref().is_some_and(|a| {
                            (&a.bundle, &a.controller, &a.state) == (&pose.bundle, &pose.controller, &pose.state)
                        });
                        if on {
                            p.rect_filled(row, 0.0, t.select);
                        } else if resp.hovered() {
                            p.rect_filled(row, 0.0, t.row_hover);
                        }
                        let tint = if on { t.text_bright } else { Kind::Pose.color() };
                        glyph(p, k, Pos2::new(row.left() + 22.0, y), Kind::Pose.glyph(), 14.0, tint);
                        let color = if on { t.text_bright } else { t.text };
                        let clip = p.with_clip_rect(row.with_max_x(row.right() - 230.0));
                        let x = label(&clip, k, Pos2::new(row.left() + 38.0, y), &pose.name, color);
                        if worn {
                            p.text(
                                Pos2::new(x + 10.0, y),
                                Align2::LEFT_CENTER,
                                "on the selection",
                                k.small(),
                                if on { t.text_bright } else { t.accent },
                            );
                        }
                        p.text(
                            Pos2::new(row.right() - 16.0, y),
                            Align2::RIGHT_CENTER,
                            kind_text(*timing),
                            k.small(),
                            if on { t.text_bright } else { t.text_dim },
                        );
                        if resp.clicked() {
                            select = Some(pose.clone());
                        }
                        if resp.double_clicked() {
                            apply = Some(pose.clone());
                        }
                        resp.on_hover_text(format!(
                            "{}\n{} in {}\nDouble click to put the selected character in it",
                            pose.name, pose.state, pose.bundle
                        ));
                    }
                }
            }
        });
    if let Some(pose) = select {
        st.poses.selected = Some(pose);
    }
    if let Some(pose) = apply {
        st.poses.selected = Some(pose.clone());
        st.run(Command::Pose(Some(pose)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(name: &str) -> scene::Pose {
        scene::Pose {
            name: name.to_owned(),
            bundle: "studio/anime/00.unity3d".into(),
            controller: "tachi".into(),
            state: "f_tachi_00".into(),
            speed: 0.5,
            start: 0.25,
            force_loop: true,
        }
    }

    #[test]
    fn a_project_keeps_poses_as_files() {
        let dir = std::env::temp_dir().join(format!("renkatsu-poses-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(saved_in(&dir).is_empty());
        let first = save_in(&dir, &pose("Stand 1")).unwrap();
        assert_eq!(first.file_name().unwrap(), "Stand 1.rkpose");
        // The same name again takes a number; signs that do not belong in
        // a file name go.
        let second = save_in(&dir, &pose("Stand 1")).unwrap();
        assert_eq!(second.file_name().unwrap(), "Stand 1 2.rkpose");
        let third = save_in(&dir, &pose("a/b: c")).unwrap();
        assert_eq!(third.file_name().unwrap(), "a b c.rkpose");
        std::fs::write(dir.join("broken.rkpose"), "not a pose").unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        let saved = saved_in(&dir);
        let names: Vec<&str> = saved.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["a b c", "Stand 1", "Stand 1 2"]);
        // Everything about how it plays comes back.
        assert_eq!(saved[1].pose, pose("Stand 1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_kind_says_whether_a_pose_moves() {
        use rk_chara::poses::Timing;
        let timing = |seconds, loops| Some(Timing { seconds, loops });
        assert_eq!(kind_text(None), "");
        assert_eq!(kind_text(timing(1.0 / 6.0, false)), "Static");
        assert_eq!(kind_text(timing(1.333, true)), "Dynamic, loops every 1.3 s");
        assert_eq!(kind_text(timing(5.2, false)), "Dynamic, 5.2 s");
    }
}
