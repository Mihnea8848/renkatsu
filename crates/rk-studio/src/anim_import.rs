// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The window that imports animations into the project: everything that
//! is installed (Koikatsu's own list, the packages', the mods'), browsed
//! by source, group and category, searched by name and filtered by
//! whether it moves, with a preview of the highlighted animation on a
//! character. What is chosen is copied into the project as animation
//! files.
//!
//! Groups and categories go by name, not by the numbers CharaStudio's
//! lists use: categories of the same name in groups of the same name are
//! one, whichever package or mod brought them, and each animation says
//! where it comes from.

use crate::animations::{self, From, Pick};
use crate::live::{Clock, Live};
use crate::mods::Games;
use crate::project::Project;
use crate::scene::{self, Content, ObjectId, Scene};
use crate::sources;
use crate::state::{Log, Look, Playback, State};
use crate::widgets::{self, glyph, hair_h, hair_v, label};
use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use egui_phosphor::regular as icon;
use rk_chara::animations::{same_name, Branch, Library, Source, Timing};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

/// How many more animation lengths have to arrive before the list is
/// worked out again.
const TIMED_STEP: usize = 256;

/// The lengths read so far while the app runs, by bundle, controller,
/// state and override, so that the window opens on them the next time.
static TIMINGS: std::sync::Mutex<Option<std::collections::HashMap<String, Timing>>> = std::sync::Mutex::new(None);

/// Reads the lengths of `entries` on a thread of its own, so the window
/// keeps its pace, and sends each with its place in the list. The thread
/// stops when nobody listens any more.
fn read_timings(
    game: std::sync::Arc<rk_vfs::Game>,
    entries: Vec<rk_chara::animations::Entry>,
) -> std::sync::mpsc::Receiver<(usize, Timing)> {
    let (send, receive) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("animation lengths".into())
        .spawn(move || {
            for (i, entry) in entries.iter().enumerate() {
                let key = format!(
                    "{}|{}|{}|{}|{}",
                    entry.bundle, entry.controller, entry.state, entry.override_bundle, entry.override_controller
                );
                let known = TIMINGS
                    .lock()
                    .ok()
                    .and_then(|map| map.as_ref().and_then(|m| m.get(&key).copied()));
                let timing = known.unwrap_or_else(|| {
                    let timing = rk_chara::animations::timing_of(&game, entry, &Library::TIMING_BODY);
                    if let Ok(mut map) = TIMINGS.lock() {
                        map.get_or_insert_with(Default::default).insert(key, timing);
                    }
                    timing
                });
                if send.send((i, timing)).is_err() {
                    return;
                }
            }
        });
    receive
}

/// One animation that can be imported.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The name in English, and the name as its list has it.
    pub name: String,
    pub original: String,
    pub group: String,
    pub category: String,
    pub source: Source,
    /// The source's name as it shows.
    pub source_name: String,
    /// The H scenes list it, rather than CharaStudio's own menu.
    pub h: bool,
    pub from: From,
    pub timing: Option<Timing>,
    /// Its place in CharaStudio's list, for the ones that come from it.
    listed: Option<usize>,
}

impl Entry {
    fn pick(&self) -> Pick {
        Pick {
            name: self.name.clone(),
            group: self.group.clone(),
            category: self.category.clone(),
            source: self.source_name.clone(),
            h: Some(self.h),
            from: self.from.clone(),
        }
    }

    /// The animation as a scene names it while it is not in the project.
    fn reference(&self) -> scene::Animation {
        let (file, bundle, controller, state, over) = match &self.from {
            From::Installed {
                bundle,
                controller,
                state,
                over,
            } => (None, bundle.clone(), controller.clone(), state.clone(), over.clone()),
            From::File(path) => (
                Some(path.to_string_lossy().into_owned()),
                String::new(),
                String::new(),
                String::new(),
                Default::default(),
            ),
        };
        let [override_bundle, override_controller] = over;
        scene::Animation {
            name: self.name.clone(),
            file,
            bundle,
            controller,
            state,
            override_bundle,
            override_controller,
            speed: 1.0,
            start: 0.0,
            force_loop: false,
            options: None,
        }
    }
}

/// The row the H scenes' groups sit under, so that they are one thing in
/// the tree instead of nine spread through CharaStudio's own.
pub const H_GROUPS: &str = "H Animations";

/// What of the list shows: a source, a group and a category, each or all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shown {
    /// One source by its name, or every mod at once.
    pub source: Option<SourceChoice>,
    /// One group by its name, or every group the H scenes list.
    pub group: Option<GroupChoice>,
    pub category: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupChoice {
    One(String),
    H,
}

impl GroupChoice {
    fn takes(&self, entry: &Entry) -> bool {
        match self {
            GroupChoice::One(name) => same_name(name, &entry.group),
            GroupChoice::H => entry.h,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceChoice {
    One(Source),
    Mods,
}

impl SourceChoice {
    fn takes(&self, source: &Source) -> bool {
        match self {
            SourceChoice::One(one) => one == source,
            SourceChoice::Mods => matches!(source, Source::Mod(_)),
        }
    }
}

impl Shown {
    fn source_takes(&self, entry: &Entry) -> bool {
        self.source.as_ref().is_none_or(|s| s.takes(&entry.source))
    }

    pub fn takes(&self, entry: &Entry) -> bool {
        self.source_takes(entry)
            && self.group.as_ref().is_none_or(|g| g.takes(entry))
            && self.category.as_ref().is_none_or(|c| same_name(c, &entry.category))
    }
}

/// A row of the tree at the left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub depth: usize,
    pub label: String,
    pub count: usize,
    /// What choosing the row shows.
    pub shows: Shown,
    /// It has rows below it, shown or folded away.
    pub parent: bool,
    /// The key it is folded and unfolded by.
    pub key: String,
    /// It is a caption, not something to choose.
    pub caption: bool,
}

/// The sources the entries come from, as rows: all of them at once when
/// there are several, then the packages, Koikatsu, and the mods a step
/// further in under a row that shows them all.
pub fn source_rows(entries: &[Entry]) -> Vec<Node> {
    let row = |depth: usize, label: &str, choice: Option<SourceChoice>, count: usize, parent: bool| Node {
        depth,
        label: label.to_owned(),
        count,
        key: format!("source {label}"),
        shows: Shown {
            source: choice,
            group: None,
            category: None,
        },
        parent,
        caption: false,
    };
    let mut sources: Vec<(&Source, &str, usize)> = Vec::new();
    for entry in entries {
        match sources.iter_mut().find(|(s, ..)| *s == &entry.source) {
            Some((.., count)) => *count += 1,
            None => sources.push((&entry.source, &entry.source_name, 1)),
        }
    }
    let rank = |source: &Source| match source {
        Source::Package(_) => 0,
        Source::Game => 1,
        Source::Mod(_) => 2,
    };
    sources.sort_by_key(|(source, name, _)| (rank(source), name.to_lowercase()));
    let mut rows = Vec::new();
    if sources.len() > 1 {
        rows.push(row(0, "All sources", None, entries.len(), false));
    }
    let one = |source: &Source| Some(SourceChoice::One(source.clone()));
    for (source, name, count) in sources.iter().filter(|(s, ..)| !matches!(s, Source::Mod(_))) {
        rows.push(row(0, name, one(source), *count, false));
    }
    let mods: Vec<_> = sources.iter().filter(|(s, ..)| matches!(s, Source::Mod(_))).collect();
    match mods[..] {
        [] => {}
        // A lone mod needs no row above it.
        [(source, name, count)] => rows.push(row(0, name, one(source), *count, false)),
        _ => {
            let count = mods.iter().map(|m| m.2).sum();
            rows.push(row(0, "Mods", Some(SourceChoice::Mods), count, true));
            for (source, name, count) in mods {
                rows.push(row(1, name, one(source), *count, false));
            }
        }
    }
    rows
}

/// The tree: the groups and categories of the chosen source, merged by
/// name across everything that source covers. With `sources`, the
/// sources are rows above them instead of a dropdown of their own.
pub fn tree(entries: &[Entry], shown: &Shown, unfolded: &HashSet<String>, sources: bool) -> Vec<Node> {
    let caption = |label: &str| Node {
        depth: 0,
        label: label.to_owned(),
        count: 0,
        shows: Shown::default(),
        parent: false,
        key: String::new(),
        caption: true,
    };
    let mut rows = Vec::new();
    if sources {
        rows.push(caption("Source"));
        let mods_open = unfolded.contains("source Mods");
        rows.extend(source_rows(entries).into_iter().filter(|n| n.depth == 0 || mods_open));
        rows.push(caption("Category"));
    }
    let of_source: Vec<&Entry> = entries.iter().filter(|e| shown.source_takes(e)).collect();
    rows.push(Node {
        depth: 0,
        label: "All categories".to_owned(),
        count: of_source.len(),
        shows: Shown {
            source: shown.source.clone(),
            group: None,
            category: None,
        },
        parent: false,
        key: "all categories".to_owned(),
        caption: false,
    });
    // CharaStudio's own groups, each a row of the tree; the H scenes'
    // groups sit together under one row of their own instead, a step
    // further in, so that the list opens on the studio's own menu.
    let (h, own): (Vec<&Entry>, Vec<&Entry>) = of_source.iter().copied().partition(|e| e.h);
    let named = |of: &[&Entry]| {
        rk_chara::animations::tree_of(
            of.iter()
                .map(|e| (e.group.as_str(), e.category.as_str()))
                .collect::<Vec<_>>()
                .into_iter(),
        )
    };
    for branch in named(&own) {
        rows.extend(group_rows(&branch, 0, &shown.source, unfolded));
    }
    if !h.is_empty() {
        let key = "group h scenes".to_owned();
        let open = unfolded.contains(&key);
        rows.push(Node {
            depth: 0,
            label: H_GROUPS.to_owned(),
            count: h.len(),
            shows: Shown {
                source: shown.source.clone(),
                group: Some(GroupChoice::H),
                category: None,
            },
            parent: true,
            key,
            caption: false,
        });
        if open {
            for branch in named(&h) {
                rows.extend(group_rows(&branch, 1, &shown.source, unfolded));
            }
        }
    }
    rows
}

/// One group's rows: the group itself, and its categories a step
/// further in while it is unfolded.
fn group_rows(branch: &Branch, depth: usize, source: &Option<SourceChoice>, unfolded: &HashSet<String>) -> Vec<Node> {
    let key = format!("group {}", branch.name.to_lowercase());
    let group = Some(GroupChoice::One(branch.name.clone()));
    let mut rows = vec![Node {
        depth,
        label: branch.name.clone(),
        count: branch.count,
        shows: Shown {
            source: source.clone(),
            group: group.clone(),
            category: None,
        },
        parent: true,
        key: key.clone(),
        caption: false,
    }];
    if !unfolded.contains(&key) {
        return rows;
    }
    for (category, count) in &branch.categories {
        rows.push(Node {
            depth: depth + 1,
            key: format!("category {} {}", branch.name.to_lowercase(), category.to_lowercase()),
            shows: Shown {
                source: source.clone(),
                group: group.clone(),
                category: Some(category.clone()),
            },
            label: category.clone(),
            count: *count,
            parent: false,
            caption: false,
        });
    }
    rows
}

/// The entries that show, as indices: what the tree, the filter (all,
/// static, moving) and the search leave. The search looks at names in
/// English and as listed, and at group and category names.
pub fn matching(entries: &[Entry], shown: &Shown, filter: usize, search: &str) -> Vec<usize> {
    let search = search.trim().to_lowercase();
    (0..entries.len())
        .filter(|&i| {
            let e = &entries[i];
            let fits = match (filter, e.timing) {
                (0, _) => true,
                (_, None) => false,
                (1, Some(t)) => t.is_static(),
                (_, Some(t)) => !t.is_static(),
            };
            fits && shown.takes(e)
                && (search.is_empty()
                    || e.name.to_lowercase().contains(&search)
                    || e.original.to_lowercase().contains(&search)
                    || e.category.to_lowercase().contains(&search)
                    || e.group.to_lowercase().contains(&search))
        })
        .collect()
}

/// The entries of CharaStudio's list as read from what is installed, with
/// the sources' names as they show. `only` keeps one source's.
pub fn listed(library: &Library, only: Option<&Source>, name_of: &dyn Fn(&Source) -> String) -> Vec<Entry> {
    library
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| only.is_none_or(|o| *o == e.source))
        .map(|(i, e)| Entry {
            name: e.name.clone(),
            original: e.original.clone(),
            group: library.group_name(e.group),
            category: library.category_label(e.group, e.category),
            h: e.h,
            source_name: name_of(&e.source),
            source: e.source.clone(),
            from: From::Installed {
                bundle: e.bundle.clone(),
                controller: e.controller.clone(),
                state: e.state.clone(),
                over: [e.override_bundle.clone(), e.override_controller.clone()],
            },
            timing: e.timing,
            listed: Some(i),
        })
        .collect()
}

/// The animation files a package brings in its `animations` folder: the
/// folders around a file are its group and category, unless the file says
/// otherwise itself.
pub fn package_files(dir: &Path, id: &str, name: &str) -> Vec<Entry> {
    let or = |value: String, fallback: &str| {
        if value.trim().is_empty() {
            fallback.to_owned()
        } else {
            value
        }
    };
    rk_chara::animation_file::filed(&dir.join(rk_packages::manifest::folder::ANIMATIONS))
        .into_iter()
        .map(|file| Entry {
            name: file.name,
            original: file.meta.name.clone(),
            group: or(file.group, name),
            category: or(file.category, "Other"),
            h: file.meta.h,
            source: Source::Package(id.to_owned()),
            source_name: name.to_owned(),
            timing: Some(file.meta.timing()),
            from: From::File(file.path),
            listed: None,
        })
        .collect()
}

/// The preview: a small scene of its own with one character, drawn by a
/// renderer of its own.
impl Drop for Preview {
    fn drop(&mut self) {
        self.live.release();
    }
}

struct Preview {
    live: Live,
    scene: Scene,
    character: ObjectId,
    /// The entry that plays, and for how long it has.
    playing: Option<usize>,
    seconds: f32,
    log: Log,
}

pub struct Importer {
    /// The zipmod the user picked, when the window shows one mod's
    /// animations instead of everything installed.
    only: Option<PathBuf>,
    /// A stand-in project whose mods are the picked zipmod and the
    /// project's own, and the content opened with them.
    stand_in: Option<Project>,
    games: Games,
    library: Option<Library>,
    /// The lengths of the animations, as a thread reads them.
    timings: Option<std::sync::mpsc::Receiver<(usize, Timing)>>,
    pub entries: Vec<Entry>,
    problem: Option<String>,
    pub search: String,
    pub filter: usize,
    pub shown: Shown,
    unfolded: HashSet<String>,
    pub chosen: BTreeSet<usize>,
    pub highlighted: Option<usize>,
    /// Sort what is imported into folders by group and category.
    pub by_category: bool,
    preview: Option<Preview>,
    preview_problem: Option<String>,
    /// The tree and the list as last worked out, with what they were
    /// worked out from: both are the same from frame to frame until
    /// something is clicked, typed or read.
    view: Option<(ViewKey, Vec<Node>, Vec<usize>)>,
    /// The sources as the dropdown lists them, with how many entries
    /// they were worked out from.
    sources: Option<(usize, Vec<Node>)>,
}

/// What the tree and the list depend on.
#[derive(Debug, Clone, PartialEq)]
struct ViewKey {
    shown: Shown,
    filter: usize,
    search: String,
    unfolded: Vec<String>,
    entries: usize,
    timed: usize,
}

impl Importer {
    /// The rows of the tree and the entries of the list, worked out again
    /// only when what they depend on changed.
    fn view(&mut self) -> (Vec<Node>, Vec<usize>) {
        let mut unfolded: Vec<String> = self.unfolded.iter().cloned().collect();
        unfolded.sort();
        let key = ViewKey {
            shown: self.shown.clone(),
            filter: self.filter,
            search: self.search.clone(),
            unfolded,
            entries: self.entries.len(),
            // The list is worked out again as lengths arrive, but not for
            // every one of them.
            timed: self.library.as_ref().map_or(0, |l| {
                if l.timed() == l.entries.len() {
                    usize::MAX
                } else {
                    l.timed() / TIMED_STEP
                }
            }),
        };
        match &self.view {
            Some((old, rows, list)) if *old == key => (rows.clone(), list.clone()),
            _ => {
                // The sources are in the dropdown above the tree.
                let rows = tree(&self.entries, &self.shown, &self.unfolded, false);
                let list = matching(&self.entries, &self.shown, self.filter, &self.search);
                self.view = Some((key, rows.clone(), list.clone()));
                (rows, list)
            }
        }
    }
}

impl Importer {
    /// The window on everything installed, or on one zipmod.
    pub fn new(only: Option<PathBuf>) -> Importer {
        Importer {
            only,
            stand_in: None,
            games: Games::default(),
            library: None,
            timings: None,
            entries: Vec::new(),
            problem: None,
            search: String::new(),
            filter: 0,
            shown: Shown::default(),
            unfolded: HashSet::new(),
            chosen: BTreeSet::new(),
            highlighted: None,
            by_category: false,
            preview: None,
            preview_problem: None,
            view: None,
            sources: None,
        }
    }

    /// What is installed changed: the list is read again.
    pub fn reset(&mut self) {
        self.library = None;
        self.timings = None;
        self.entries.clear();
        self.chosen.clear();
        self.highlighted = None;
        self.problem = None;
        self.view = None;
        self.sources = None;
        self.games.reset();
    }

    /// Reads the list when it is not read yet, and takes the animations'
    /// lengths read since the last frame. Returns whether more are to
    /// come.
    fn advance(&mut self, st: &mut State) -> bool {
        if self.problem.is_some() {
            return false;
        }
        if let (Some(file), None) = (&self.only, &self.stand_in) {
            match stand_in(&st.project, file) {
                Ok(project) => self.stand_in = Some(project),
                Err(e) => {
                    self.problem = Some(format!("{e:#}"));
                    return false;
                }
            }
        }
        let (project, games) = match &self.stand_in {
            Some(project) => (project, &mut self.games),
            None => (&st.project, &mut st.games),
        };
        let game = match games.shared(&project.mods_dir()) {
            Ok(game) => game,
            Err(e) => {
                self.problem = Some(format!("{e:#}"));
                return false;
            }
        };
        if self.library.is_none() {
            let library = Library::read(&game);
            let installed = &st.packages.installed;
            let name_of = |source: &Source| match source {
                Source::Package(id) => package_name(installed, id),
                other => other.label().to_owned(),
            };
            let only = self.only.as_ref().map(|file| {
                let info = rk_vfs::index::mod_info(file);
                Source::Mod(info.map_or_else(String::new, |m| if m.name.trim().is_empty() { m.guid } else { m.name }))
            });
            self.entries = listed(&library, only.as_ref(), &name_of);
            if self.only.is_none() {
                for row in installed {
                    let id = &row.package.manifest.id;
                    self.entries
                        .extend(package_files(&row.package.dir, id, &package_name(installed, id)));
                }
            }
            self.library = Some(library);
        }
        let library = self.library.as_mut().expect("read above");
        let before = library.timed();
        let done = if st.scripted {
            // A scripted picture waits for every length.
            library.time(&game, &|| false)
        } else {
            let timings = self
                .timings
                .get_or_insert_with(|| read_timings(game.clone(), library.entries.clone()));
            for (i, timing) in timings.try_iter() {
                library.record(i, timing);
            }
            library.timed() == library.entries.len()
        };
        if library.timed() != before {
            for entry in &mut self.entries {
                if let Some(i) = entry.listed {
                    entry.timing = library.entries[i].timing;
                }
            }
        }
        !done
    }

    /// Copies the chosen animations, or the highlighted one when none is
    /// chosen, into the project.
    fn import(&mut self, st: &mut State) {
        let wanted: Vec<usize> = if self.chosen.is_empty() {
            self.highlighted.into_iter().collect()
        } else {
            self.chosen.iter().copied().collect()
        };
        let picks: Vec<Pick> = wanted
            .iter()
            .filter_map(|&i| self.entries.get(i))
            .map(Entry::pick)
            .collect();
        match &self.stand_in {
            // A picked zipmod is read through the window's own content.
            Some(project) => {
                let folder = st.animation_folder();
                let game = self.games.get(&project.mods_dir()).ok();
                let done = animations::import(&st.project, game, &picks, &folder, self.by_category);
                st.imported_animations(done, &folder);
            }
            None => st.import_animations(&picks, self.by_category),
        }
    }
}

/// The name a package shows under: the one in the list Renkatsu ships,
/// else its own, else its id.
fn package_name(installed: &[crate::packages::Row], id: &str) -> String {
    rk_packages::index::known()
        .into_iter()
        .find(|k| k.id == id)
        .map(|k| k.name)
        .or_else(|| {
            installed
                .iter()
                .find(|r| r.package.manifest.id == id)
                .map(|r| r.package.manifest.name.clone())
        })
        .unwrap_or_else(|| id.to_owned())
}

/// A stand-in project in the system's temporary folder whose `Mods` hold
/// links to a picked zipmod and to the project's own mods, so that the
/// content opens with them and nothing is copied.
fn stand_in(project: &Project, zipmod: &Path) -> anyhow::Result<Project> {
    use anyhow::Context;
    let root = std::env::temp_dir().join(format!("renkatsu-import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mods = root.join("Mods");
    std::fs::create_dir_all(&mods).with_context(|| format!("creating {}", mods.display()))?;
    let mut files = vec![zipmod.to_owned()];
    files.extend(crate::mods::in_folder(&project.mods_dir()).into_iter().map(|m| m.path));
    for file in files {
        let target = mods.join(file.file_name().unwrap_or_default());
        if target.exists() {
            continue;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&file, &target).with_context(|| format!("linking {}", file.display()))?;
        #[cfg(not(unix))]
        std::fs::copy(&file, &target).with_context(|| format!("copying {}", file.display()))?;
    }
    let mut stand_in = Project::none();
    stand_in.root = root;
    Ok(stand_in)
}

/// The card the preview shows: the selected character's, else the first
/// character's of the scene, else the default card, with the clothes the
/// character wears in the scene.
fn preview_content(st: &State) -> Option<Content> {
    let character = |o: &scene::Object| match &o.content {
        Content::Character {
            card,
            outfit,
            outdoor_shoes,
            clothes,
            liquid,
            ..
        } => Some(Content::Character {
            card: st.project.resolve(card),
            outfit: *outfit,
            outdoor_shoes: *outdoor_shoes,
            simulate: true,
            clothes: *clothes,
            liquid: *liquid,
            animation: None,
            profile: None,
        }),
        _ => None,
    };
    st.doc
        .selection()
        .and_then(character)
        .or_else(|| st.default_card().map(Content::character))
        .or_else(|| st.doc.scene().objects().iter().find_map(character))
        .or_else(|| {
            let first = st.library.characters.first()?;
            Some(Content::character(first.path.clone()))
        })
}

impl Importer {
    /// Draws the preview into `rect`: the character playing the
    /// highlighted animation.
    fn preview(&mut self, ui: &mut Ui, k: &Look, st: &mut State, rect: Rect) {
        let t = &k.t;
        ui.painter().rect_filled(rect, 0.0, t.input);
        let say = |ui: &Ui, text: &str| {
            let galley = ui
                .painter()
                .layout(text.to_owned(), k.font(), t.text_dim, rect.width() - 32.0);
            let at = rect.center() - galley.size() / 2.0;
            ui.painter().galley(at, galley, t.text_dim);
        };
        if self.preview.is_none() && self.preview_problem.is_none() {
            let made = (|| -> anyhow::Result<Preview> {
                use anyhow::Context;
                let live = st.live.as_ref().context("The preview needs the viewport")?.sibling()?;
                let content = preview_content(st).context("No character to preview with")?;
                let mut scene = Scene::new();
                let character = scene.add("Preview", content, None);
                Ok(Preview {
                    live,
                    scene,
                    character,
                    playing: None,
                    seconds: 0.0,
                    log: Log::new(),
                })
            })();
            match made {
                Ok(preview) => self.preview = Some(preview),
                Err(e) => self.preview_problem = Some(format!("{e:#}")),
            }
        }
        let Some(preview) = &mut self.preview else {
            say(ui, self.preview_problem.as_deref().unwrap_or_default());
            return;
        };
        let dt = ui.input(|i| i.stable_dt).min(1.0 / 30.0);
        if preview.playing != self.highlighted {
            preview.playing = self.highlighted;
            preview.seconds = 0.0;
            let animation = self.highlighted.and_then(|i| self.entries.get(i)).map(Entry::reference);
            if let Some(Content::Character { animation: worn, .. }) =
                preview.scene.get_mut(preview.character).map(|o| &mut o.content)
            {
                *worn = animation.map(Box::new);
            }
        } else {
            preview.seconds += dt;
        }
        let (project, games) = match &self.stand_in {
            Some(project) => (project, &mut self.games),
            None => (&st.project, &mut st.games),
        };
        preview.live.follow(
            &preview.scene,
            project,
            games,
            &mut st.jobs,
            Clock {
                was: Playback::Playing,
                now: Playback::Playing,
                dt,
                time: preview.seconds,
            },
            &mut preview.log,
        );
        preview.live.cam.advance(dt);
        let ppp = ui.ctx().pixels_per_point();
        let size = [
            (rect.width() * ppp).round() as u32,
            (rect.height() * ppp).round() as u32,
        ];
        match preview.live.paint(&preview.scene, size, [0.16, 0.16, 0.17, 1.0]) {
            Ok(id) => {
                ui.painter().image(
                    id,
                    rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            Err(e) => say(ui, &format!("The preview could not be drawn: {e:#}")),
        }
        if let Some(why) = preview.live.failure(preview.character) {
            say(ui, &format!("The character could not be built: {why}"));
        } else if let Some(why) = preview.live.built(preview.character).and_then(|b| b.animation_problem) {
            let text = format!("This animation could not be read: {why}");
            let band = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 44.0), rect.max);
            ui.painter().rect_filled(band, 0.0, Color32::from_black_alpha(170));
            crate::selector::line(
                ui,
                Pos2::new(band.left() + 10.0, band.center().y),
                band.width() - 20.0,
                &text,
                k.small(),
                t.stop,
            );
        }
        ui.ctx().request_repaint();
    }
}

/// The window, drawn like the one that imports characters: a header with
/// the search, the tree, the list and the preview, and a foot with what
/// is chosen and the buttons.
pub fn show(ctx: &egui::Context, k: &Look, st: &mut State) {
    let Some(mut w) = st.importer.take() else { return };
    let t = k.t;
    if st.project.is_none() {
        return;
    }
    let reading = w.advance(st);
    if reading {
        ctx.request_repaint();
    }
    let typing = ctx.egui_wants_keyboard_input();
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !typing;
    let mut import = false;
    let mut to_packages = false;
    if crate::packages::veil(ctx, "animation importer") {
        close = true;
    }
    let window = crate::packages::placed(ctx, Vec2::new(1180.0, 700.0), Vec2::new(860.0, 460.0));
    egui::Area::new(egui::Id::new("animation importer"))
        .order(egui::Order::Foreground)
        .fixed_pos(window.min)
        .show(ctx, |ui| {
            ui.set_min_size(window.size());
            let (frame, closed) = crate::packages::frame(ui, k, window, "Import animations", "");
            close |= closed;
            let (head, body, foot) = (frame.head, frame.body, frame.foot);
            if !w.entries.is_empty() {
                let search = Rect::from_min_size(
                    Pos2::new(head.right() - 52.0 - 260.0, head.center().y - k.field() / 2.0),
                    Vec2::new(260.0, k.field()),
                );
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(search));
                widgets::search(&mut child, k, &mut w.search, "Search", 260.0);
            }
            if w.entries.is_empty() {
                let c = body.center();
                let (title, text) = match (&w.problem, &w.only) {
                    (Some(problem), _) => ("The animations could not be read", Some(problem.clone())),
                    (None, Some(_)) => ("This zipmod lists no animations", None),
                    (None, None) => ("Nothing with animations is installed", None),
                };
                let lift = if text.is_some() { 30.0 } else { 14.0 };
                ui.painter().text(
                    c - Vec2::new(0.0, lift),
                    Align2::CENTER_CENTER,
                    title,
                    k.bold(k.d.text),
                    t.text_bright,
                );
                if let Some(text) = text {
                    ui.painter().text(
                        c - Vec2::new(0.0, 8.0),
                        Align2::CENTER_CENTER,
                        text,
                        k.font(),
                        t.text_dim,
                    );
                }
                if w.only.is_none() && w.problem.is_none() {
                    let open = Rect::from_center_size(c + Vec2::new(0.0, 28.0), Vec2::new(150.0, 30.0));
                    if crate::selector::button(ui, k, open, "Open Packages", true, true) {
                        to_packages = true;
                    }
                }
            } else {
                let tree_w = crate::sources::COLUMN;
                let preview_w = (body.width() * 0.28).clamp(240.0, 340.0);
                let tree = Rect::from_min_max(body.min, Pos2::new(body.left() + tree_w, body.bottom()));
                let side = Rect::from_min_max(Pos2::new(body.right() - preview_w, body.top()), body.max);
                let list = Rect::from_min_max(tree.right_top(), side.left_bottom());
                ui.painter().rect_filled(tree, 0.0, t.well);
                hair_v(ui.painter(), tree.right(), tree.top(), tree.bottom(), t.line);
                hair_v(ui.painter(), side.left(), side.top(), side.bottom(), t.line);
                tree_pane(ui, k, &mut w, tree);
                import |= list_pane(ui, k, &mut w, list, reading, typing);
                side_pane(ui, k, st, &mut w, side.with_min_x(side.left() + 1.0));
            }
            // The foot: what is chosen, and the buttons.
            let n = if w.chosen.is_empty() {
                usize::from(w.highlighted.is_some())
            } else {
                w.chosen.len()
            };
            let text = match (w.chosen.len(), n) {
                (_, 0) if w.entries.is_empty() => String::new(),
                (_, 0) => "No animation chosen".to_owned(),
                (0, _) => "1 animation highlighted".to_owned(),
                (1, _) => "1 animation chosen".to_owned(),
                (many, _) => format!("{many} animations chosen"),
            };
            ui.painter().text(
                Pos2::new(foot.left() + 20.0, foot.center().y),
                Align2::LEFT_CENTER,
                text,
                k.font(),
                t.text_dim,
            );
            let import_rect = Rect::from_min_size(
                Pos2::new(foot.right() - 20.0 - 104.0, foot.center().y - 15.0),
                Vec2::new(104.0, 30.0),
            );
            let cancel_rect = import_rect.translate(Vec2::new(-104.0 - 8.0, 0.0));
            if !w.entries.is_empty() {
                let check = Rect::from_min_size(
                    Pos2::new(cancel_rect.left() - 250.0, foot.center().y - k.field() / 2.0),
                    Vec2::new(236.0, k.field()),
                );
                widgets::checkbox(
                    ui,
                    k,
                    check,
                    ui.id().with("by category"),
                    &mut w.by_category,
                    "Folders by group and category",
                );
            }
            if crate::selector::button(ui, k, cancel_rect, "Cancel", false, true) {
                close = true;
            }
            let caption = if n > 1 {
                format!("Import {n}")
            } else {
                "Import".to_owned()
            };
            if crate::selector::button(ui, k, import_rect, &caption, true, n > 0) {
                import = true;
            }
        });
    if to_packages {
        st.packages.open = true;
        return;
    }
    if import && (w.highlighted.is_some() || !w.chosen.is_empty()) {
        w.import(st);
        return;
    }
    if !close {
        st.importer = Some(w);
    }
}

/// The left column: the dropdown of sources, and the tree of groups and
/// categories.
fn tree_pane(ui: &mut Ui, k: &Look, w: &mut Importer, column: Rect) {
    let (rows, _) = w.view();
    let area = {
        let count = w.entries.len();
        if w.sources.as_ref().is_none_or(|(of, _)| *of != count) {
            w.sources = Some((count, source_rows(&w.entries)));
        }
        let nodes = w.sources.as_ref().map_or(&[][..], |(_, nodes)| nodes);
        let items: Vec<sources::Item> = nodes
            .iter()
            .map(|n| sources::Item {
                label: n.label.clone(),
                depth: n.depth,
                count: Some(n.count),
            })
            .collect();
        let chosen = nodes.iter().position(|n| n.shows.source == w.shown.source).unwrap_or(0);
        let id = ui.id().with("animation source");
        let (picked, rest) = sources::dropdown(ui, k, column, id, &items, chosen);
        if let Some(node) = picked.and_then(|i| nodes.get(i)) {
            w.shown = node.shows.clone();
        }
        rest
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
    child.set_clip_rect(area);
    egui::ScrollArea::vertical()
        .id_salt("animation tree")
        .auto_shrink(false)
        .max_height(area.height())
        .show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(6.0);
            for node in &rows {
                if node.caption {
                    sources::caption(ui, k, &node.label);
                    continue;
                }
                // A source row is on when it is the source shown; a
                // category row when it is the group and category shown.
                let is_source = node.key.starts_with("source ");
                let on = if is_source {
                    node.shows.source == w.shown.source
                } else {
                    node.shows == w.shown
                };
                let of = sources::Row {
                    label: &node.label,
                    depth: node.depth,
                    count: Some(node.count),
                    on,
                    open: node.parent.then(|| w.unfolded.contains(&node.key)),
                };
                let clicked = sources::row(ui, k, &node.key, of);
                if clicked.row {
                    w.shown = node.shows.clone();
                }
                // Choosing a group opens it; choosing it again folds it.
                if (clicked.fold || (clicked.row && node.parent)) && !w.unfolded.remove(&node.key) {
                    w.unfolded.insert(node.key.clone());
                }
            }
            ui.add_space(8.0);
        });
}

/// The filter row and the list. Returns whether an animation was double
/// clicked, to import it at once.
fn list_pane(ui: &mut Ui, k: &Look, w: &mut Importer, area: Rect, reading: bool, typing: bool) -> bool {
    let t = &k.t;
    let mut import = false;
    let bar = Rect::from_min_size(area.min, Vec2::new(area.width(), k.d.row + 12.0));
    hair_h(ui.painter(), bar.left() + 1.0, bar.right(), bar.bottom() - 1.0, t.line);
    let seg = Rect::from_min_size(
        Pos2::new(bar.left() + 12.0, bar.center().y - k.field() / 2.0),
        Vec2::new(210.0, k.field()),
    );
    widgets::segmented(
        ui,
        k,
        seg,
        ui.id().with("importer filter"),
        &mut w.filter,
        &["All", "Static", "Moving"],
    );
    let (_, shown) = w.view();
    let all_chosen = !shown.is_empty() && shown.iter().all(|i| w.chosen.contains(i));
    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(seg.right() + 8.0, bar.top()),
                Pos2::new(bar.right() - 8.0, bar.bottom()),
            ))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    let caption = if all_chosen { "Choose none" } else { "Choose all shown" };
    if widgets::flat_button(&mut right, k, Some(icon::CHECKS), caption, false, false).clicked() {
        for i in &shown {
            if all_chosen {
                w.chosen.remove(i);
            } else {
                w.chosen.insert(*i);
            }
        }
    }
    let counted = if reading && w.filter != 0 {
        format!("{} shown, reading lengths", shown.len())
    } else {
        format!("{} shown", shown.len())
    };
    let width = widgets::text_width(&right, &counted, k.small()).ceil() + 8.0;
    let (rect, _) = right.allocate_exact_size(Vec2::new(width, k.field()), Sense::hover());
    right.painter().text(
        rect.right_center() - Vec2::new(4.0, 0.0),
        Align2::RIGHT_CENTER,
        counted,
        k.small(),
        t.text_dim,
    );

    // The arrow keys move the highlight, for looking through a category.
    if !typing && !shown.is_empty() {
        let step =
            ui.input(|i| i32::from(i.key_pressed(egui::Key::ArrowDown)) - i32::from(i.key_pressed(egui::Key::ArrowUp)));
        if step != 0 {
            let at = w.highlighted.and_then(|h| shown.iter().position(|i| *i == h));
            let next = match at {
                Some(at) => (at as i32 + step).clamp(0, shown.len() as i32 - 1) as usize,
                None => 0,
            };
            w.highlighted = Some(shown[next]);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Space)) {
            if let Some(h) = w.highlighted {
                if !w.chosen.remove(&h) {
                    w.chosen.insert(h);
                }
            }
        }
    }

    let rows_area = Rect::from_min_max(Pos2::new(area.left() + 1.0, bar.bottom()), area.max);
    if shown.is_empty() {
        let text = if w.filter != 0 && reading {
            "Reading the animations"
        } else {
            "No animation matches"
        };
        ui.painter().text(
            Pos2::new(rows_area.center().x, rows_area.top() + 40.0),
            Align2::CENTER_CENTER,
            text,
            k.font(),
            t.text_dim,
        );
        return false;
    }
    let with_category = w.shown.category.is_none();
    let with_source = w.shown.source.as_ref().is_none_or(|s| *s == SourceChoice::Mods);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rows_area));
    child.set_clip_rect(rows_area);
    egui::ScrollArea::vertical()
        .id_salt("animation list")
        .auto_shrink(false)
        .max_height(rows_area.height())
        .show_rows(&mut child, k.d.row, shown.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for &i in &shown[range] {
                let entry = &w.entries[i];
                let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row), Sense::click());
                let y = row.center().y;
                let lit = w.highlighted == Some(i);
                let ticked = w.chosen.contains(&i);
                let tick = Rect::from_center_size(Pos2::new(row.left() + 20.0, y), Vec2::splat(row.height()));
                let tick_resp = ui.interact(tick, ui.id().with(("tick", i)), Sense::click());
                let p = ui.painter();
                if lit {
                    p.rect_filled(row, 0.0, t.select);
                } else if resp.hovered() || tick_resp.hovered() {
                    p.rect_filled(row, 0.0, t.row_hover);
                }
                // The tick box.
                let mark = Rect::from_center_size(tick.center(), Vec2::splat(14.0));
                let (fill, edge) = match (ticked, lit) {
                    (true, false) => (t.select, t.select),
                    (true, true) => (Color32::WHITE, Color32::WHITE),
                    (false, true) => (Color32::TRANSPARENT, Color32::WHITE),
                    (false, false) => (t.input, if tick_resp.hovered() { t.text } else { t.border }),
                };
                p.rect_filled(mark, 2.0, fill);
                p.rect_stroke(mark, 2.0, Stroke::new(1.0, edge), StrokeKind::Inside);
                if ticked {
                    let color = if lit { t.select } else { Color32::WHITE };
                    glyph(p, k, mark.center(), icon::CHECK, 10.0, color);
                }
                let fixed = entry.timing.map(|timing| timing.is_static());
                let g = if fixed == Some(true) {
                    crate::kinds::Kind::Pose.glyph()
                } else {
                    crate::kinds::Kind::Animation.glyph()
                };
                let bright = if lit { Color32::WHITE } else { t.text };
                let dim = if lit { Color32::WHITE } else { t.text_dim };
                let tint = if lit {
                    Color32::WHITE
                } else {
                    crate::kinds::Kind::Animation.color()
                };
                glyph(p, k, Pos2::new(row.left() + 44.0, y), g, 14.0, tint);
                let kind_w = 170.0;
                let source_w = if with_source { 150.0 } else { 0.0 };
                let category_w = if with_category {
                    ((row.width() - kind_w - source_w - 60.0) * 0.42).max(0.0)
                } else {
                    0.0
                };
                let name_end = row.right() - kind_w - source_w - category_w - 8.0;
                label(
                    &p.with_clip_rect(row.with_max_x(name_end)),
                    k,
                    Pos2::new(row.left() + 58.0, y),
                    &entry.name,
                    bright,
                );
                let mut x = name_end + 8.0;
                if with_category {
                    let text = format!("{}: {}", entry.group, entry.category);
                    p.with_clip_rect(row.with_max_x(x + category_w - 8.0)).text(
                        Pos2::new(x, y),
                        Align2::LEFT_CENTER,
                        text,
                        k.small(),
                        dim,
                    );
                    x += category_w;
                }
                if with_source {
                    p.with_clip_rect(row.with_max_x(x + source_w - 8.0)).text(
                        Pos2::new(x, y),
                        Align2::LEFT_CENTER,
                        &entry.source_name,
                        k.small(),
                        dim,
                    );
                }
                let kind = match entry.timing {
                    Some(timing) => animations::kind_text(timing.seconds, timing.loops),
                    None => String::new(),
                };
                p.text(
                    Pos2::new(row.right() - 14.0, y),
                    Align2::RIGHT_CENTER,
                    kind,
                    k.small(),
                    dim,
                );
                if tick_resp.clicked() || (resp.clicked() && ui.input(|i| i.modifiers.command)) {
                    if !w.chosen.remove(&i) {
                        w.chosen.insert(i);
                    }
                } else if resp.clicked() {
                    w.highlighted = Some(i);
                }
                if resp.double_clicked() {
                    w.highlighted = Some(i);
                    if w.chosen.is_empty() {
                        import = true;
                    } else {
                        w.chosen.insert(i);
                    }
                }
                let entry = &w.entries[i];
                let listed = if entry.original != entry.name && !entry.original.is_empty() {
                    format!("\nListed as {}", entry.original)
                } else {
                    String::new()
                };
                resp.on_hover_text(format!(
                    "{}{listed}\n{}: {}\nFrom {}",
                    entry.name, entry.group, entry.category, entry.source_name
                ));
            }
        });
    import
}

/// The preview with what it shows written under it.
fn side_pane(ui: &mut Ui, k: &Look, st: &mut State, w: &mut Importer, area: Rect) {
    let t = &k.t;
    let caption_h = 76.0;
    let picture = Rect::from_min_max(area.min, Pos2::new(area.right(), area.bottom() - caption_h));
    w.preview(ui, k, st, picture);
    let caption = Rect::from_min_max(picture.left_bottom(), area.max);
    hair_h(ui.painter(), caption.left(), caption.right(), caption.top(), t.line);
    let x = caption.left() + 14.0;
    let width = caption.width() - 28.0;
    match w.highlighted.and_then(|i| w.entries.get(i)) {
        Some(entry) => {
            crate::selector::line(
                ui,
                Pos2::new(x, caption.top() + 18.0),
                width,
                &entry.name,
                k.bold(k.d.text),
                t.text_bright,
            );
            crate::selector::line(
                ui,
                Pos2::new(x, caption.top() + 38.0),
                width,
                &format!("{}: {}", entry.group, entry.category),
                k.small(),
                t.text_dim,
            );
            let kind = entry.timing.map_or_else(String::new, |timing| {
                animations::kind_text(timing.seconds, timing.loops)
            });
            let text = if kind.is_empty() {
                format!("From {}", entry.source_name)
            } else {
                format!("{kind}. From {}", entry.source_name)
            };
            crate::selector::line(
                ui,
                Pos2::new(x, caption.top() + 56.0),
                width,
                &text,
                k.small(),
                t.text_dim,
            );
        }
        None => {
            crate::selector::line(
                ui,
                Pos2::new(x, caption.top() + 28.0),
                width,
                "No animation shown",
                k.font(),
                t.text_dim,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, group: &str, category: &str, source: Source, seconds: f32) -> Entry {
        Entry {
            name: name.to_owned(),
            original: format!("{name} (listed)"),
            group: group.to_owned(),
            category: category.to_owned(),
            source_name: match &source {
                Source::Package(_) => "Dance pack".to_owned(),
                other => other.label().to_owned(),
            },
            source,
            from: From::Installed {
                bundle: "b".into(),
                controller: "c".into(),
                state: name.to_lowercase(),
                over: Default::default(),
            },
            timing: Some(Timing { seconds, loops: false }),
            listed: None,
            h: false,
        }
    }

    /// The same, listed by the H scenes' sheets.
    fn h_entry(name: &str, group: &str, category: &str, seconds: f32) -> Entry {
        Entry {
            h: true,
            ..entry(name, group, category, Source::Game, seconds)
        }
    }

    fn entries() -> Vec<Entry> {
        let pack = || Source::Package("dance-pack".into());
        vec![
            entry("Stand 1", "Character", "Stand", Source::Game, 0.1),
            entry("Walk 1", "Character", "Walk and run", Source::Game, 1.2),
            entry("Waltz", "Studio", "Dance", Source::Game, 8.0),
            // A package adds to an existing category by its name, and
            // brings a category of its own.
            entry("Tango", "studio", " DANCE ", pack(), 9.0),
            entry("Bow", "Studio", "Greetings", pack(), 0.15),
            entry("Lean", "Men", "Stand", Source::Mod("Masculine".into()), 0.1),
            entry("Sit", "Men", "Sit", Source::Mod("Another mod".into()), 0.1),
        ]
    }

    fn labels(rows: &[Node]) -> Vec<(usize, &str, usize)> {
        rows.iter()
            .filter(|n| !n.caption)
            .map(|n| (n.depth, n.label.as_str(), n.count))
            .collect()
    }

    #[test]
    fn categories_of_the_same_name_merge_across_sources() {
        let entries = entries();
        let mut unfolded = HashSet::new();
        let rows = tree(&entries, &Shown::default(), &unfolded, true);
        assert_eq!(
            labels(&rows),
            [
                (0, "All sources", 7),
                (0, "Dance pack", 2),
                (0, "Koikatsu", 3),
                (0, "Mods", 2),
                (0, "All categories", 7),
                (0, "Character", 2),
                (0, "Studio", 3),
                (0, "Men", 2),
            ]
        );
        // Unfolded, Studio's Dance holds the game's and the package's.
        unfolded.insert("group studio".to_owned());
        unfolded.insert("source Mods".to_owned());
        let rows = tree(&entries, &Shown::default(), &unfolded, true);
        assert_eq!(
            labels(&rows)[3..],
            [
                (0, "Mods", 2),
                (1, "Another mod", 1),
                (1, "Masculine", 1),
                (0, "All categories", 7),
                (0, "Character", 2),
                (0, "Studio", 3),
                (1, "Dance", 2),
                (1, "Greetings", 1),
                (0, "Men", 2),
            ]
        );
        let dance = rows.iter().find(|n| n.label == "Dance").unwrap();
        let names = |shown: &Shown, filter: usize, search: &str| -> Vec<&str> {
            matching(&entries, shown, filter, search)
                .into_iter()
                .map(|i| entries[i].name.as_str())
                .collect()
        };
        assert_eq!(names(&dance.shows, 0, ""), ["Waltz", "Tango"]);

        // One source chosen: the tree shows what that source has.
        let pack = rows.iter().find(|n| n.label == "Dance pack").unwrap();
        let rows = tree(&entries, &pack.shows, &unfolded, true);
        assert_eq!(
            labels(&rows)[6..],
            [
                (0, "All categories", 2),
                (0, "studio", 2),
                (1, "DANCE", 1),
                (1, "Greetings", 1)
            ]
        );
        assert_eq!(names(&pack.shows, 0, ""), ["Tango", "Bow"]);
        let mods = Shown {
            source: Some(SourceChoice::Mods),
            ..Default::default()
        };
        assert_eq!(names(&mods, 0, ""), ["Lean", "Sit"]);

        // Static or moving, and the search over names and categories.
        assert_eq!(names(&Shown::default(), 1, ""), ["Stand 1", "Bow", "Lean", "Sit"]);
        assert_eq!(names(&Shown::default(), 2, ""), ["Walk 1", "Waltz", "Tango"]);
        assert_eq!(names(&Shown::default(), 0, "dance"), ["Waltz", "Tango"]);
        assert_eq!(names(&Shown::default(), 0, "tango (listed)"), ["Tango"]);
        assert_eq!(names(&Shown::default(), 2, "stand"), Vec::<&str>::new());
    }

    #[test]
    fn the_sources_are_a_list_of_their_own() {
        let entries = entries();
        // Packages, then the game, then the mods under a row for all.
        assert_eq!(
            labels(&source_rows(&entries)),
            [
                (0, "All sources", 7),
                (0, "Dance pack", 2),
                (0, "Koikatsu", 3),
                (0, "Mods", 2),
                (1, "Another mod", 1),
                (1, "Masculine", 1),
            ]
        );
        // With the sources in a dropdown, the tree is the categories alone.
        let rows = tree(&entries, &Shown::default(), &HashSet::new(), false);
        assert!(rows.iter().all(|n| !n.caption));
        assert_eq!(
            labels(&rows),
            [
                (0, "All categories", 7),
                (0, "Character", 2),
                (0, "Studio", 3),
                (0, "Men", 2)
            ]
        );
        // One source needs no row for all, and one mod none for the mods.
        let mut some: Vec<Entry> = entries.iter().filter(|e| e.source == Source::Game).cloned().collect();
        assert_eq!(labels(&source_rows(&some)), [(0, "Koikatsu", 3)]);
        some.push(entries[5].clone());
        assert_eq!(
            labels(&source_rows(&some)),
            [(0, "All sources", 4), (0, "Koikatsu", 3), (0, "Masculine", 1)]
        );
        // Without Koikatsu, it is not a source.
        let without: Vec<Entry> = entries.iter().filter(|e| e.source != Source::Game).cloned().collect();
        assert!(source_rows(&without).iter().all(|n| n.label != "Koikatsu"));
    }

    #[test]
    fn the_h_scenes_groups_sit_under_one_row() {
        let mut all = entries();
        all.push(h_entry("Stand 1", "H foreplay", "Stand", 2.0));
        all.push(h_entry("Chair 1", "H foreplay", "Chair sit", 2.0));
        all.push(h_entry("Lying 1", "Female H insertion", "Missionary", 2.0));
        let mut unfolded = HashSet::new();
        let rows = tree(&all, &Shown::default(), &unfolded, true);
        // CharaStudio's groups stay where they were; the H scenes' three
        // animations are one row, after them, with nothing of theirs
        // standing beside the studio's own groups.
        assert_eq!(
            &labels(&rows)[4..],
            [
                (0, "All categories", 10),
                (0, "Character", 2),
                (0, "Studio", 3),
                (0, "Men", 2),
                (0, H_GROUPS, 3),
            ]
        );
        // Unfolded it holds its groups, and those their categories.
        unfolded.insert("group h scenes".to_owned());
        unfolded.insert("group h foreplay".to_owned());
        let rows = tree(&all, &Shown::default(), &unfolded, true);
        assert_eq!(
            &labels(&rows)[7..],
            [
                (0, "Men", 2),
                (0, H_GROUPS, 3),
                (1, "H foreplay", 2),
                (2, "Stand", 1),
                (2, "Chair sit", 1),
                (1, "Female H insertion", 1),
            ]
        );
        let names = |shown: &Shown| -> Vec<&str> {
            matching(&all, shown, 0, "")
                .into_iter()
                .map(|i| all[i].name.as_str())
                .collect()
        };
        let h = rows.iter().find(|n| n.label == H_GROUPS).unwrap();
        assert_eq!(names(&h.shows), ["Stand 1", "Chair 1", "Lying 1"]);
        // The row for one of its groups shows that group alone, and the
        // studio's own Character group keeps its own Stand 1.
        let foreplay = rows.iter().find(|n| n.label == "H foreplay").unwrap();
        assert_eq!(names(&foreplay.shows), ["Stand 1", "Chair 1"]);
        let character = rows.iter().find(|n| n.label == "Character").unwrap();
        assert_eq!(names(&character.shows), ["Stand 1", "Walk 1"]);
    }

    #[test]
    fn a_packages_animation_files_take_their_folders_as_group_and_category() {
        let dir = std::env::temp_dir().join(format!("renkatsu-importer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let deep = dir.join("animations/Studio/Dance");
        std::fs::create_dir_all(&deep).unwrap();
        rk_chara::animation_file::sample("Spin", 3.0, true)
            .write(&deep.join("Spin.rkanim"))
            .unwrap();
        // At the top of the folder, the file's own group and category count.
        rk_chara::animation_file::sample("Walk 1", 1.5, true)
            .write(&dir.join("animations/Walk.rkanim"))
            .unwrap();
        std::fs::write(dir.join("animations/readme.txt"), b"").unwrap();
        std::fs::write(deep.join("broken.rkanim"), b"no").unwrap();
        let found = package_files(&dir, "dance-pack", "Dance pack");
        let seen: Vec<(&str, &str, &str)> = found
            .iter()
            .map(|e| (e.name.as_str(), e.group.as_str(), e.category.as_str()))
            .collect();
        assert_eq!(
            seen,
            [("Spin", "Studio", "Dance"), ("Walk", "Character", "Walk and run")]
        );
        assert_eq!(found[0].source, Source::Package("dance-pack".into()));
        assert_eq!(found[0].timing.unwrap().seconds, 3.0);
        assert!(matches!(found[0].from, From::File(_)));
        // What it names to a scene is the file itself.
        assert!(found[0].reference().file.unwrap().ends_with("Spin.rkanim"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
