// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Content panel: the project's folders as they are on disk, and what
//! each holds. It shows the project and nothing else: what is installed
//! but not in the project is reached through the import windows.
//!
//! The tree at the left has the project's folders and, under the open
//! one, the folders the user made in it. Characters show as their cards,
//! animations as a list, and every other folder as its files. Everything
//! has a context menu, and files are dragged onto folders to move them.

use crate::content;
use crate::design::Tree;
use crate::kinds::Kind;
use crate::panels::{toolbar, DragCard};
use crate::project;
use crate::state::{Command, FileOp, Look, State};
use crate::widgets::{self, bar_divider, flat_button, glyph, hair_h, hair_v, icon_button, label};
use egui::{Align, Align2, Color32, Id, Layout, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use egui_phosphor::regular as icon;
use std::path::{Path, PathBuf};

/// A file of the project dragged in the Content panel, to move it into a
/// folder or, for an animation, onto a character.
#[derive(Debug, Clone)]
pub struct DragFile(pub String);

/// The kind each of the project's folders holds and what it says while
/// it is empty.
fn folder_kind(top: &str) -> (Kind, &'static str) {
    match top {
        project::CHARACTERS => (Kind::Character, "This project has no characters yet"),
        project::ANIMATIONS => (Kind::Animation, "This project has no animations yet"),
        project::POSES => (Kind::Pose, "This project has no poses yet"),
        crate::maps::MAPS => (Kind::Map, "This project has no maps yet"),
        "Items" => (Kind::Item, "Not available yet"),
        project::SCENES => (Kind::Scene, "This folder is empty"),
        "Renders" => (Kind::Camera, "This project has no renders yet"),
        _ => (Kind::Folder, "This project has no mods yet"),
    }
}

/// What this frame asks for, carried out once the panel is drawn.
#[derive(Default)]
struct Asks {
    commands: Vec<Command>,
    /// A folder to open.
    open: Option<String>,
    /// A file to select.
    select: Option<String>,
    /// The tiles in the order they are shown, for a Shift click's range.
    order: Vec<String>,
    /// A file or folder to start renaming.
    rename: Option<String>,
    /// The renaming was asked for in the folder tree, so its field shows
    /// there rather than on the tile.
    rename_in_tree: bool,
}

impl Asks {
    fn file(&mut self, op: FileOp) {
        self.commands.push(Command::File(op));
    }
}

fn slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Whether a file's folder is the open folder or lies below it.
fn in_open(folder: &str, open: &str) -> bool {
    folder == open || folder.strip_prefix(open).is_some_and(|rest| rest.starts_with('/'))
}

/// The import items of a folder's menus: what brings files into it.
fn import_items(ui: &mut Ui, k: &Look, top: &str, asks: &mut Asks) {
    match top {
        project::CHARACTERS => {
            if widgets::menu_item(ui, k, icon::DOWNLOAD_SIMPLE, "Import card here", "", true) {
                asks.commands.push(Command::ImportCard);
            }
            if widgets::menu_item(ui, k, icon::BOOKS, "Import from library", "", true) {
                asks.commands.push(Command::ImportFromPackage);
            }
            ui.separator();
        }
        project::ANIMATIONS => {
            if widgets::menu_item(ui, k, icon::FILE, "Import from file here", "", true) {
                asks.commands.push(Command::ImportAnimationFile);
            }
            if widgets::menu_item(ui, k, icon::BOOKS, "Import from library here", "", true) {
                asks.commands.push(Command::ImportAnimations);
            }
            ui.separator();
        }
        crate::maps::MAPS => {
            if widgets::menu_item(ui, k, icon::FILE, "Import from file", "", true) {
                asks.commands.push(Command::ImportMapFile);
            }
            if widgets::menu_item(ui, k, icon::BOOKS, "Import from library", "", true) {
                asks.commands.push(Command::MapLibrary);
            }
            ui.separator();
        }
        _ => {}
    }
}

/// The menu of a folder, or of the empty part of a folder's content.
fn folder_menu(ui: &mut Ui, k: &Look, st: &State, folder: &str, asks: &mut Asks) {
    let top = content::top(folder);
    import_items(ui, k, top, asks);
    if widgets::menu_item(ui, k, icon::FOLDER_SIMPLE_PLUS, "New folder", "", true) {
        asks.open = Some(folder.to_owned());
        asks.file(FileOp::NewFolder(folder.to_owned()));
    }
    let pastes = st
        .clipboard
        .as_ref()
        .is_some_and(|(rel, _)| content::top(rel) == top && rel != folder);
    if widgets::menu_item(ui, k, icon::CLIPBOARD, "Paste", "", pastes) {
        asks.file(FileOp::Paste(folder.to_owned()));
    }
    if content::changeable(folder) {
        ui.separator();
        if widgets::menu_item(ui, k, icon::PENCIL_SIMPLE, "Rename", "", true) {
            asks.rename = Some(folder.to_owned());
        }
        if widgets::menu_item(ui, k, icon::COPY, "Duplicate", "", true) {
            asks.file(FileOp::Duplicate(folder.to_owned()));
        }
        if widgets::menu_item(ui, k, icon::COPY, "Copy", "", true) {
            asks.file(FileOp::Copy(folder.to_owned()));
        }
        if widgets::menu_item(ui, k, icon::SCISSORS, "Cut", "", true) {
            asks.file(FileOp::Cut(folder.to_owned()));
        }
        move_menu(ui, k, st, folder, asks);
    }
    ui.separator();
    if widgets::menu_item(ui, k, icon::ARROW_SQUARE_OUT, "Show in file manager", "", true) {
        asks.file(FileOp::Reveal(folder.to_owned()));
    }
    if content::changeable(folder) {
        ui.separator();
        if widgets::menu_item(ui, k, icon::TRASH, "Delete", "", true) {
            for item in with_selection(st, folder) {
                asks.file(FileOp::Delete(item));
            }
        }
    }
}

/// Whether a tile is selected, noting it in the order the tiles show.
fn picked(st: &State, rel: &str, asks: &mut Asks) -> bool {
    asks.order.push(rel.to_owned());
    st.assets.iter().any(|a| a == rel) || st.asset.as_deref() == Some(Path::new(rel))
}

/// What an action on `rel` acts on: the whole selection when `rel` is
/// part of one, else `rel` alone.
fn with_selection(st: &State, rel: &str) -> Vec<String> {
    if st.assets.len() > 1 && st.assets.iter().any(|a| a == rel) {
        st.assets.clone()
    } else {
        vec![rel.to_owned()]
    }
}

/// The submenu that moves a file or a folder into another folder of the
/// same project folder.
fn move_menu(ui: &mut Ui, k: &Look, st: &State, rel: &str, asks: &mut Asks) {
    let top = content::top(rel).to_owned();
    widgets::menu_sub(ui, k, icon::FOLDER_SIMPLE, "Move into folder", |ui| {
        let mut targets = vec![top.clone()];
        targets.extend(content::folders(&st.project, &top));
        for target in targets {
            let inside = target == rel || target.starts_with(&format!("{rel}/"));
            let here = content::parent(rel) == target;
            let depth = target.matches('/').count();
            let name = format!("{}{}", "    ".repeat(depth), content::leaf(&target));
            let glyph_text = if depth == 0 {
                icon::FOLDER_OPEN
            } else {
                icon::FOLDER_SIMPLE
            };
            if widgets::menu_item(ui, k, glyph_text, &name, "", !inside && !here) {
                asks.file(FileOp::Move(rel.to_owned(), target));
            }
        }
        ui.separator();
        if widgets::menu_item(ui, k, icon::FOLDER_SIMPLE_PLUS, "New folder", "", true) {
            asks.file(FileOp::NewFolder(content::parent(rel).to_owned()));
        }
    });
}

/// The menu of a file.
fn file_menu(ui: &mut Ui, k: &Look, st: &State, rel: &str, asks: &mut Asks) {
    let top = content::top(rel);
    let character = st.doc.selection().is_some_and(|o| o.kind() == Kind::Character);
    let png = rel.to_lowercase().ends_with(".png");
    match top {
        project::CHARACTERS if png && widgets::menu_item(ui, k, icon::PLUS, "Add to scene", "", true) => {
            asks.commands.push(Command::AddCard(PathBuf::from(rel)));
        }
        project::CHARACTERS => {}
        project::POSES
            if is_pose(rel) && widgets::menu_item(ui, k, icon::PLAY, "Apply to selected character", "", character) =>
        {
            asks.commands.push(Command::LoadPose(rel.to_owned()));
        }
        project::ANIMATIONS
            if crate::sequence::is_keys(rel)
                && widgets::menu_item(ui, k, icon::PLAY, "Apply to selected character", "", character) =>
        {
            asks.commands.push(Command::LoadKeys(rel.to_owned()));
        }
        project::ANIMATIONS => {
            let item = st.animations.find(rel);
            if widgets::menu_item(
                ui,
                k,
                icon::PLAY,
                "Apply to selected character",
                "",
                character && item.is_some(),
            ) {
                asks.commands.push(Command::Animate(item.map(|i| i.reference())));
            }
            if item.is_some_and(|i| i.link.is_some())
                && widgets::menu_item(ui, k, icon::DOWNLOAD_SIMPLE, "Copy into the project", "", true)
            {
                asks.file(FileOp::CopyIn(rel.to_owned()));
            }
        }
        _ => {}
    }
    if png && widgets::menu_item(ui, k, icon::IMAGE, "Set as project icon", "", true) {
        asks.file(FileOp::SetIcon(rel.to_owned()));
    }
    ui.separator();
    if widgets::menu_item(ui, k, icon::PENCIL_SIMPLE, "Rename", "", true) {
        asks.rename = Some(rel.to_owned());
    }
    if widgets::menu_item(ui, k, icon::COPY, "Duplicate", "", true) {
        asks.file(FileOp::Duplicate(rel.to_owned()));
    }
    if widgets::menu_item(ui, k, icon::COPY, "Copy", "", true) {
        asks.file(FileOp::Copy(rel.to_owned()));
    }
    if widgets::menu_item(ui, k, icon::SCISSORS, "Cut", "", true) {
        asks.file(FileOp::Cut(rel.to_owned()));
    }
    let folder = content::parent(rel);
    let pastes = st.clipboard.as_ref().is_some_and(|(c, _)| content::top(c) == top);
    if widgets::menu_item(ui, k, icon::CLIPBOARD, "Paste", "", pastes) {
        asks.file(FileOp::Paste(folder.to_owned()));
    }
    move_menu(ui, k, st, rel, asks);
    ui.separator();
    if widgets::menu_item(ui, k, icon::ARROW_SQUARE_OUT, "Show in file manager", "", true) {
        asks.file(FileOp::Reveal(rel.to_owned()));
    }
    ui.separator();
    if widgets::menu_item(ui, k, icon::TRASH, "Delete", "", true) {
        for item in with_selection(st, rel) {
            asks.file(FileOp::Delete(item));
        }
    }
}

/// The text field of the file or folder being renamed, when that is
/// `rel`: drawn over `rect`. Returns whether it was drawn.
fn renaming(ui: &mut Ui, k: &Look, st: &mut State, rel: &str, rect: Rect, in_tree: bool, asks: &mut Asks) -> bool {
    // A folder shows in the tree and as a tile; its field shows in the
    // place the renaming started.
    if st.renaming_in_tree != in_tree {
        return false;
    }
    let Some((target, text)) = &mut st.content_renaming else {
        return false;
    };
    if target != rel {
        return false;
    }
    if let Some(name) = widgets::rename_field(ui, k, rect, Id::new(("content rename", rel)), text) {
        if !name.trim().is_empty() {
            asks.file(FileOp::Rename(rel.to_owned(), name));
        }
        st.content_renaming = None;
    }
    true
}

/// What is being dragged over `rect`, when it can be moved into `folder`:
/// a file or a card of the same project folder.
fn drop_into(ui: &Ui, k: &Look, rect: Rect, folder: &str, selected: &[String], asks: &mut Asks) {
    let dragged = if crate::panels::dragged_over::<DragFile>(ui, rect) {
        egui::DragAndDrop::payload::<DragFile>(ui.ctx()).map(|f| f.0.clone())
    } else if crate::panels::dragged_over::<DragCard>(ui, rect) {
        egui::DragAndDrop::payload::<DragCard>(ui.ctx()).map(|c| slashes(&c.0))
    } else {
        None
    };
    let Some(rel) = dragged else { return };
    if content::top(&rel) != content::top(folder) || content::parent(&rel) == folder {
        return;
    }
    ui.painter()
        .rect_stroke(rect, 0.0, Stroke::new(1.0, k.t.select_bright), StrokeKind::Inside);
    if ui.input(|i| i.pointer.any_released()) {
        // A tile of several selected brings the others along, those that
        // can go there: of the same project folder, not the folder itself
        // or one it lies in.
        let all: Vec<String> = if selected.contains(&rel) {
            selected.to_vec()
        } else {
            vec![rel]
        };
        for item in all {
            let fits = content::top(&item) == content::top(folder)
                && content::parent(&item) != folder
                && !in_open(folder, &item);
            if fits {
                asks.file(FileOp::Move(item, folder.to_owned()));
            }
        }
    }
}

pub fn show(ui: &mut Ui, k: &Look, st: &mut State) {
    let t = &k.t;
    st.library.begin_frame();
    st.folder = st.folder.min(project::FOLDERS.len() - 1);
    let top = project::FOLDERS[st.folder];
    if content::top(&st.subfolder) != top {
        st.subfolder = top.to_owned();
    }
    // The folder this frame draws; a click in the tree changes it for the
    // next frame.
    let open = st.subfolder.clone();
    let (kind, empty_note) = folder_kind(top);
    let mut asks = Asks::default();

    let bar = toolbar(ui, k, |ui| {
        match top {
            project::CHARACTERS => {
                if flat_button(ui, k, Some(icon::DOWNLOAD_SIMPLE), "Import card", false, false).clicked() {
                    asks.commands.push(Command::ImportCard);
                }
                if flat_button(ui, k, Some(icon::BOOKS), "From library", false, false).clicked() {
                    asks.commands.push(Command::ImportFromPackage);
                }
                let card = st.asset.clone().filter(|a| slashes(a).to_lowercase().ends_with(".png"));
                let add = flat_button(ui, k, Some(icon::PLUS), "Add to scene", false, false);
                let add = if card.is_none() {
                    add.on_hover_text("No character selected")
                } else {
                    add
                };
                if add.clicked() {
                    if let Some(card) = card {
                        asks.commands.push(Command::AddCard(card));
                    }
                }
            }
            project::ANIMATIONS => animation_tools(ui, k, st, &mut asks),
            crate::maps::MAPS => {
                if flat_button(ui, k, Some(icon::FILE), "Import from file", false, false).clicked() {
                    asks.commands.push(Command::ImportMapFile);
                }
                if flat_button(ui, k, Some(icon::BOOKS), "Import from library", false, false).clicked() {
                    asks.commands.push(Command::MapLibrary);
                }
                let map = st.asset.clone().filter(|a| is_sidecar(&slashes(a)));
                let add = flat_button(ui, k, Some(icon::PLUS), "Add to scene", false, false);
                let add = if map.is_none() {
                    add.on_hover_text("No map selected")
                } else {
                    add
                };
                if add.clicked() {
                    if let Some(map) = map {
                        asks.commands.push(Command::AddMap(map));
                    }
                }
            }
            _ => {}
        }
        if icon_button(ui, k, icon::ARROWS_CLOCKWISE, false)
            .on_hover_text("Refresh")
            .clicked()
        {
            st.refresh_content();
        }
        bar_divider(ui, k);
        ui.add_space(2.0);
        // The way to the open folder, each part a button.
        let mut trail = vec![(st.project.file.name.clone(), None)];
        let mut at = String::new();
        for part in open.split('/') {
            at = if at.is_empty() {
                part.to_owned()
            } else {
                format!("{at}/{part}")
            };
            trail.push((part.to_owned(), Some(at.clone())));
        }
        for (i, (part, target)) in trail.iter().enumerate() {
            let last = i + 1 == trail.len();
            let w = widgets::text_width(ui, part, k.font()).ceil();
            let sense = if target.is_some() && !last {
                Sense::click()
            } else {
                Sense::hover()
            };
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, k.field()), sense);
            let color = if last || resp.hovered() {
                t.text_bright
            } else {
                t.text_dim
            };
            label(ui.painter(), k, rect.left_center(), part, color);
            if resp.clicked() {
                asks.open = target.clone();
            }
            if !last {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(14.0, k.field()), Sense::hover());
                glyph(ui.painter(), k, rect.center(), icon::CARET_RIGHT, 10.0, t.text_dim);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            widgets::search(ui, k, &mut st.content_search, "Search", 240.0);
        });
    });
    hair_h(ui.painter(), bar.left(), bar.right(), bar.bottom() - 1.0, t.line);

    let body = ui.available_rect_before_wrap();
    let tree_w = 188.0;
    let tree = Rect::from_min_max(body.min, Pos2::new(body.left() + tree_w, body.bottom()));
    hair_v(ui.painter(), tree.right(), tree.top(), tree.bottom(), t.line);
    folder_tree(ui, k, st, tree, &open, &mut asks);

    let grid = Rect::from_min_max(Pos2::new(tree.right() + 1.0, body.top()), body.max);
    // The empty part of the folder: its menu, and a click that selects
    // nothing. Drawn first, so that what lies on it is clicked first.
    let ground = ui.interact(grid, ui.id().with("content ground"), Sense::click());
    if ground.clicked() {
        st.asset = None;
        st.assets.clear();
    }
    ground.context_menu(|ui| folder_menu(ui, k, st, &open, &mut asks));
    let mut area = ui.new_child(UiBuilder::new().max_rect(grid));
    area.set_clip_rect(grid.intersect(ui.clip_rect()));
    match top {
        project::CHARACTERS => cards(&mut area, k, st, grid, &open, empty_note, &mut asks),
        project::ANIMATIONS => animations(&mut area, k, st, grid, &open, &mut asks),
        crate::maps::MAPS => maps(&mut area, k, st, grid, &open, &mut asks),
        _ => files(&mut area, k, st, grid, &open, kind, empty_note, &mut asks),
    }
    ui.allocate_rect(body, Sense::hover());

    st.take_face_shots();
    // The mouse's back and forward buttons go through the folders opened.
    if ui.rect_contains_pointer(body) {
        let (back, forward) = ui.input(|i| {
            (
                i.pointer.button_pressed(egui::PointerButton::Extra1),
                i.pointer.button_pressed(egui::PointerButton::Extra2),
            )
        });
        if back || forward {
            st.folder_history(back);
        }
    }
    if let Some(folder) = asks.open.take() {
        st.go_to_folder(&folder);
        st.asset = None;
        st.assets.clear();
    }
    if let Some(rel) = asks.select.take() {
        let (ctrl, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
        let anchor = st.asset.as_deref().map(slashes);
        let at = |r: &str| asks.order.iter().position(|o| o == r);
        match (shift, anchor.as_deref().and_then(at), at(&rel)) {
            // Shift: everything from the last click to this one.
            (true, Some(a), Some(b)) => st.assets = asks.order[a.min(b)..=a.max(b)].to_vec(),
            _ if ctrl => {
                if st.assets.is_empty() {
                    st.assets.extend(anchor);
                }
                if let Some(i) = st.assets.iter().position(|o| *o == rel) {
                    st.assets.remove(i);
                } else {
                    st.assets.push(rel.clone());
                }
            }
            // A right click on a tile already selected keeps the others.
            _ if st.assets.contains(&rel) && ui.input(|i| i.pointer.secondary_released()) => {}
            _ => st.assets = vec![rel.clone()],
        }
        if !shift {
            st.asset = Some(PathBuf::from(rel));
        }
    }
    if let Some(rel) = asks.rename.take() {
        st.content_renaming = Some((rel.clone(), content::stem(&rel).to_owned()));
        st.renaming_in_tree = asks.rename_in_tree;
    }
    for command in asks.commands {
        st.ask(command, ui.ctx());
    }
}

/// The toolbar of the Animations folder.
fn animation_tools(ui: &mut Ui, k: &Look, st: &mut State, asks: &mut Asks) {
    if flat_button(ui, k, Some(icon::FILE), "Import from file", false, false).clicked() {
        asks.commands.push(Command::ImportAnimationFile);
    }
    if flat_button(ui, k, Some(icon::BOOKS), "Import from library", false, false).clicked() {
        asks.commands.push(Command::ImportAnimations);
    }
    let character = st.doc.selection().is_some_and(|o| o.kind() == Kind::Character);
    let chosen = st
        .asset
        .as_ref()
        .and_then(|a| st.animations.find(&slashes(a)))
        .map(|i| i.reference());
    // Keys saved as an animation apply as keys.
    let keys = st
        .asset
        .as_ref()
        .map(|a| slashes(a))
        .filter(|rel| crate::sequence::is_keys(rel));
    let why = match (character, chosen.is_some() || keys.is_some()) {
        (false, _) => Some("No character selected"),
        (true, false) => Some("No animation selected"),
        (true, true) => None,
    };
    ui.add_enabled_ui(why.is_none(), |ui| {
        let apply = flat_button(ui, k, Some(icon::PLAY), "Apply to selection", false, false);
        let apply = match why {
            Some(why) => apply.on_hover_text(why).on_disabled_hover_text(why),
            None => apply,
        };
        if apply.clicked() {
            asks.commands.push(match &keys {
                Some(rel) => Command::LoadKeys(rel.clone()),
                None => Command::Animate(chosen.clone()),
            });
        }
    });
    let (rect, _) = ui.allocate_exact_size(Vec2::new(210.0, k.field()), Sense::hover());
    widgets::segmented(
        ui,
        k,
        rect,
        ui.id().with("animation filter"),
        &mut st.animations.filter,
        &["All", "Static", "Moving"],
    );
}

/// How far each level of the tree is indented.
const INDENT: f32 = 14.0;

/// The tree at the left: the project, its folders and the folders the
/// user made in them. A folder that holds folders folds away.
fn folder_tree(ui: &mut Ui, k: &Look, st: &mut State, tree: Rect, open: &str, asks: &mut Asks) {
    let mut side = ui.new_child(UiBuilder::new().max_rect(tree));
    side.set_clip_rect(tree.intersect(ui.clip_rect()));
    egui::ScrollArea::vertical()
        .id_salt("folders")
        .auto_shrink(false)
        .show(&mut side, |ui| {
            ui.add_space(4.0);
            // The project folder holds the others, as on disk.
            let name = st.project.file.name.clone();
            folder_row(ui, k, st, "", &name, Kind::Folder, Some(open), asks);
            if st.content_folded.contains("") {
                return;
            }
            for top in project::FOLDERS {
                let (kind, _) = folder_kind(top);
                let below = tree_folders(st, top);
                folder_row(ui, k, st, top, top, kind, below.first().map(|_| open), asks);
                // What lies in a folded folder is passed over.
                let mut folded: Option<String> = st.content_folded.contains(top).then(|| top.to_owned());
                for (i, folder) in below.iter().enumerate() {
                    if folded.as_deref().is_some_and(|f| in_open(folder, f)) {
                        continue;
                    }
                    let parent = below.get(i + 1).is_some_and(|next| content::parent(next) == folder);
                    folder_row(
                        ui,
                        k,
                        st,
                        folder,
                        content::leaf(folder),
                        kind,
                        parent.then_some(open),
                        asks,
                    );
                    folded = st.content_folded.contains(folder).then(|| folder.clone());
                }
            }
        });
}

/// One folder of the tree; the project's own is the empty path. `parent`
/// is given, with the open folder, for a folder that holds folders.
#[allow(clippy::too_many_arguments)]
fn folder_row(
    ui: &mut Ui,
    k: &Look,
    st: &mut State,
    rel: &str,
    name: &str,
    kind: Kind,
    parent: Option<&str>,
    asks: &mut Asks,
) {
    let t = &k.t;
    let project = rel.is_empty();
    let depth = if project { 0 } else { 1 + rel.matches('/').count() };
    let open = st.subfolder.as_str() == rel;
    let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row), Sense::click());
    let caret_x = row.left() + 10.0 + depth as f32 * INDENT;
    // The arrow that folds the folders inside away.
    let fold = Rect::from_center_size(Pos2::new(caret_x, row.center().y), Vec2::new(INDENT, row.height()));
    let fold_resp = parent.map(|_| ui.interact(fold, ui.id().with(("fold", rel)), Sense::click()));
    let folded = st.content_folded.contains(rel);
    if fold_resp.as_ref().is_some_and(|r| r.clicked()) || (project && resp.clicked()) {
        if !st.content_folded.remove(rel) {
            st.content_folded.insert(rel.to_owned());
            // The open folder stays in sight.
            if !project && parent.is_some_and(|open| in_open(open, rel) && open != rel) {
                asks.open = Some(rel.to_owned());
            }
        }
    } else if resp.clicked() && !project {
        asks.open = Some(rel.to_owned());
    }
    if resp.double_clicked() && content::changeable(rel) {
        asks.rename = Some(rel.to_owned());
        asks.rename_in_tree = true;
    }
    {
        let p = ui.painter();
        if open {
            p.rect_filled(row, 0.0, t.control);
        } else if resp.hovered() {
            p.rect_filled(row, 0.0, t.row_hover);
        }
        if open && depth > 1 {
            p.rect_filled(
                Rect::from_min_size(row.left_top(), Vec2::new(2.0, row.height())),
                0.0,
                kind.color(),
            );
        }
        // A line down from each folder this one lies in.
        for level in 0..depth {
            let x = (row.left() + 10.0 + level as f32 * INDENT).floor();
            hair_v(p, x, row.top(), row.bottom(), t.border);
        }
        if let Some(fold_resp) = &fold_resp {
            let g = if folded { icon::CARET_RIGHT } else { icon::CARET_DOWN };
            let color = if fold_resp.hovered() { t.text_bright } else { t.text_dim };
            glyph(p, k, fold.center(), g, 10.0, color);
        }
    }
    let x = caret_x + INDENT;
    let g = if open || (project && !folded) {
        icon::FOLDER_OPEN
    } else {
        icon::FOLDER_SIMPLE
    };
    let tint = match depth {
        0 => t.text_dim,
        1 => kind.color(),
        _ => Kind::Folder.color(),
    };
    glyph(ui.painter(), k, Pos2::new(x, row.center().y), g, 15.0, tint);
    let name_rect = Rect::from_min_max(
        Pos2::new(x + 10.0, row.top() + 1.0),
        Pos2::new(row.right() - 4.0, row.bottom() - 1.0),
    );
    if project || !renaming(ui, k, st, rel, name_rect, true, asks) {
        let color = if open || project { t.text_bright } else { t.text };
        let clip = ui.painter().with_clip_rect(row.with_max_x(row.right() - 4.0));
        label(&clip, k, Pos2::new(x + 14.0, row.center().y), name, color);
    }
    if !project {
        drop_into(ui, k, row, rel, &st.assets.clone(), asks);
        let asked = asks.rename.is_some();
        resp.context_menu(|ui| folder_menu(ui, k, st, rel, asks));
        if !asked && asks.rename.is_some() {
            asks.rename_in_tree = true;
        }
    }
}

/// Whether a project folder holds nothing at all: no file and no folder.
/// The folders right inside `open`.
fn subfolders(st: &State, open: &str) -> Vec<String> {
    tree_folders(st, content::top(open))
        .into_iter()
        .filter(|f| content::parent(f) == open)
        .collect()
}

/// Whether a file is a map's sidecar.
fn is_sidecar(rel: &str) -> bool {
    rel.to_lowercase().ends_with(&format!(".{}", crate::maps::EXTENSION))
}

/// The folders of a project folder that show as folders: all of them,
/// but for the maps' own folders, which show as their maps.
fn tree_folders(st: &State, top: &str) -> Vec<String> {
    let all = content::folders(&st.project, top);
    if top != crate::maps::MAPS {
        return all;
    }
    let maps: Vec<String> = all
        .iter()
        .filter(|f| content::files(&st.project, f).iter().any(|x| is_sidecar(x)))
        .cloned()
        .collect();
    all.into_iter()
        .filter(|f| !maps.iter().any(|m| in_open(f, m)))
        .collect()
}

fn bare(st: &State, top: &str) -> bool {
    content::folders(&st.project, top).is_empty() && content::files(&st.project, top).is_empty()
}

/// A line of text in the middle of an empty folder.
fn centred(ui: &Ui, k: &Look, area: Rect, text: &str) {
    ui.painter()
        .text(area.center(), Align2::CENTER_CENTER, text, k.font(), k.t.text_dim);
}

/// A line of text at the top left of an empty folder.
fn note(ui: &Ui, k: &Look, area: Rect, text: &str) {
    ui.painter().text(
        area.min + Vec2::new(14.0, 20.0),
        Align2::LEFT_CENTER,
        text,
        k.font(),
        k.t.text_dim,
    );
}

/// What an empty folder shows in its middle: what is missing, why
/// importing helps, and the two ways in, from a file and from the library.
fn empty_state(ui: &mut Ui, k: &Look, grid: Rect, title: &str, line: &str, ways: [Command; 2], asks: &mut Asks) {
    let t = &k.t;
    let c = grid.center();
    ui.painter().text(
        c - Vec2::new(0.0, 34.0),
        Align2::CENTER_CENTER,
        title,
        k.bold(k.d.text + 1.0),
        t.text_bright,
    );
    ui.painter().text(
        c - Vec2::new(0.0, 12.0),
        Align2::CENTER_CENTER,
        line,
        k.font(),
        t.text_dim,
    );
    // Each button is as wide as what it holds, so that the icon and the
    // text have the same room around them in both.
    let width = |text: &str| (14.0 + 16.0 + 7.0 + widgets::text_width(ui, text, k.font()) + 14.0).ceil();
    let (texts, gap) = (["Import from file", "Import from library"], 10.0);
    let widths = texts.map(width);
    let x = (c.x - (widths[0] + gap + widths[1]) / 2.0).round();
    let left = Rect::from_min_size(Pos2::new(x, c.y + 12.0), Vec2::new(widths[0], 30.0));
    let right = Rect::from_min_size(Pos2::new(left.right() + gap, c.y + 12.0), Vec2::new(widths[1], 30.0));
    let [file, package] = ways;
    if crate::selector::icon_button(ui, k, left, Some(icon::FILE), texts[0], false, true) {
        asks.commands.push(file);
    }
    if crate::selector::icon_button(ui, k, right, Some(icon::BOOKS), texts[1], true, true) {
        asks.commands.push(package);
    }
}

/// What a tile shows in its square.
enum Thumb {
    /// A part of a picture.
    Image(egui::TextureId, Rect),
    /// An icon in the kind's colour.
    Glyph(&'static str, Color32),
    /// A folder: its icon fills the tile, with no frame.
    Folder,
}

/// The side of a tile's square. Every kind shows at this size, as in
/// Unreal's content browser.
fn tile_side(k: &Look) -> f32 {
    (k.d.row * 3.25).round()
}

/// A tile: its square and the name under it.
fn tile_size(k: &Look) -> Vec2 {
    let side = tile_side(k);
    let caption = if k.d.tree != Tree::Plain { 38.0 } else { 24.0 };
    Vec2::new(side, side + caption)
}

/// The part of a picture a square shows: all of a square one, the middle
/// of a wide one, and the upper middle of a tall one, where a card's face
/// shot has the face.
fn square_uv(size: [usize; 2]) -> Rect {
    let (w, h) = (size[0] as f32, size[1] as f32);
    if h > w {
        let top = (h - w) * 0.3 / h;
        Rect::from_min_max(Pos2::new(0.0, top), Pos2::new(1.0, top + w / h))
    } else {
        let left = (w - h) / 2.0 / w;
        Rect::from_min_max(Pos2::new(left, 0.0), Pos2::new(left + h / w, 1.0))
    }
}

/// Lays `count` tiles out in rows as wide as the panel, in a scroll area,
/// and has `draw` draw each one in sight.
fn tile_grid(ui: &mut Ui, k: &Look, salt: &str, count: usize, mut draw: impl FnMut(&mut Ui, usize, Rect)) {
    let size = tile_size(k);
    let gap = 10.0;
    egui::ScrollArea::vertical()
        .id_salt(salt)
        .auto_shrink(false)
        .show(ui, |ui| {
            let columns = ((ui.available_width() - gap) / (size.x + gap)).floor().max(1.0) as usize;
            let rows = count.div_ceil(columns);
            let (space, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), rows as f32 * (size.y + gap) + gap),
                Sense::hover(),
            );
            for i in 0..count {
                let cell = Rect::from_min_size(
                    space.min
                        + Vec2::new(
                            gap + (i % columns) as f32 * (size.x + gap),
                            10.0 + (i / columns) as f32 * (size.y + gap),
                        ),
                    size,
                );
                if ui.is_rect_visible(cell) {
                    draw(ui, i, cell);
                }
            }
        });
}

/// The words of a tile: its file (for renaming), name and second line.
struct TileText<'a> {
    rel: &'a str,
    name: &'a str,
    caption: &'a str,
    caption_color: Option<Color32>,
}

/// Draws a tile: its picture, the kind's colour along the picture's foot,
/// the name, and in the designs that have them a second line.
#[allow(clippy::too_many_arguments)]
fn draw_tile(
    ui: &mut Ui,
    k: &Look,
    st: &mut State,
    cell: Rect,
    resp: &egui::Response,
    on: bool,
    thumb: Thumb,
    color: Color32,
    text: TileText,
    asks: &mut Asks,
) {
    let t = &k.t;
    let picture = Rect::from_min_size(cell.min, Vec2::splat(cell.width()));
    {
        let p = ui.painter();
        match thumb {
            Thumb::Folder => {
                if on || resp.hovered() {
                    p.rect_filled(picture, k.radius(), if on { t.control } else { t.row_hover });
                }
                glyph(
                    p,
                    k,
                    picture.center(),
                    icon::FOLDER_SIMPLE,
                    picture.width() * 0.72,
                    color,
                );
            }
            Thumb::Image(id, uv) => {
                p.rect_filled(picture, 0.0, t.input);
                p.image(id, picture, uv, Color32::WHITE);
            }
            Thumb::Glyph(g, tint) => {
                p.rect_filled(picture, 0.0, t.input);
                glyph(p, k, picture.center(), g, picture.width() * 0.42, tint);
            }
        }
        if !matches!(thumb, Thumb::Folder) {
            // The type colour along the picture's foot, as Unreal's tiles.
            p.rect_filled(
                Rect::from_min_max(Pos2::new(picture.left(), picture.bottom() - 2.0), picture.max),
                0.0,
                color,
            );
            if on {
                p.rect_stroke(picture, 0.0, Stroke::new(1.0, t.select), StrokeKind::Outside);
                p.rect_stroke(picture, 0.0, Stroke::new(1.0, t.select), StrokeKind::Inside);
            } else if resp.hovered() {
                p.rect_stroke(picture, 0.0, Stroke::new(1.0, t.hover), StrokeKind::Inside);
            }
        }
    }
    let name_rect = Rect::from_min_size(
        Pos2::new(cell.left(), picture.bottom() + 2.0),
        Vec2::new(cell.width(), 20.0),
    );
    if !renaming(ui, k, st, text.rel, name_rect, false, asks) {
        let p = ui.painter();
        let clip = p.with_clip_rect(Rect::from_min_max(
            Pos2::new(cell.left(), picture.bottom()),
            Pos2::new(cell.right(), cell.bottom()),
        ));
        let color = if on { t.text_bright } else { t.text };
        label(
            &clip,
            k,
            Pos2::new(cell.left() + 1.0, picture.bottom() + 12.0),
            text.name,
            color,
        );
        if k.d.tree != Tree::Plain {
            clip.text(
                Pos2::new(cell.left() + 1.0, picture.bottom() + 27.0),
                Align2::LEFT_CENTER,
                text.caption,
                k.small(),
                text.caption_color.unwrap_or(t.text_dim),
            );
        }
    }
}

/// A folder inside the open one, as a tile. A double click opens it, and
/// files dropped on it go into it.
fn folder_tile(ui: &mut Ui, k: &Look, st: &mut State, cell: Rect, rel: &str, asks: &mut Asks) {
    let resp = ui.interact(cell, ui.id().with(("folder tile", rel)), Sense::click_and_drag());
    if content::changeable(rel) {
        resp.dnd_set_drag_payload(DragFile(rel.to_owned()));
    }
    if resp.clicked() || resp.secondary_clicked() {
        asks.select = Some(rel.to_owned());
    }
    if resp.double_clicked() {
        asks.open = Some(rel.to_owned());
    }
    let on = picked(st, rel, asks);
    drop_into(ui, k, cell, rel, &st.assets.clone(), asks);
    let text = TileText {
        rel,
        name: content::leaf(rel),
        caption: "Folder",
        caption_color: None,
    };
    draw_tile(
        ui,
        k,
        st,
        cell,
        &resp,
        on,
        Thumb::Folder,
        Kind::Folder.color(),
        text,
        asks,
    );
    resp.context_menu(|ui| folder_menu(ui, k, st, rel, asks));
}

/// What lies in the open folder: right inside it, or anywhere below it
/// while searching.
fn shown_in(folder: &str, open: &str, searching: bool) -> bool {
    if searching {
        in_open(folder, open)
    } else {
        folder == open
    }
}

/// The project's characters in the open folder, as tiles of their faces.
fn cards(ui: &mut Ui, k: &Look, st: &mut State, grid: Rect, open: &str, empty: &str, asks: &mut Asks) {
    let search = st.content_search.trim().to_lowercase();
    let searching = !search.is_empty();
    let inside = if searching { Vec::new() } else { subfolders(st, open) };
    let cards: Vec<crate::library::Entry> = st
        .library
        .characters
        .iter()
        .filter(|c| shown_in(content::parent(&slashes(&c.file)), open, searching))
        .filter(|c| {
            !searching || c.name.to_lowercase().contains(&search) || slashes(&c.file).to_lowercase().contains(&search)
        })
        .cloned()
        .collect();
    if cards.is_empty() && inside.is_empty() {
        if searching {
            note(ui, k, grid, "No character matches the search");
        } else if !bare(st, project::CHARACTERS) {
            centred(ui, k, grid, "This folder is empty");
        } else {
            empty_state(
                ui,
                k,
                grid,
                empty,
                "Characters can be imported from a file or the library.",
                [Command::ImportCard, Command::ImportFromPackage],
                asks,
            );
        }
        return;
    }
    let kind = Kind::Character;
    // Face shots are taken a little darker than the panel they sit on.
    let panel = k.t.panel;
    st.library.backdrop = [panel.r(), panel.g(), panel.b()].map(|c| (f32::from(c) * 0.85).round() as u8);
    tile_grid(ui, k, "cards", inside.len() + cards.len(), |ui, i, cell| {
        if let Some(rel) = inside.get(i) {
            folder_tile(ui, k, st, cell, rel, asks);
            return;
        }
        let card = &cards[i - inside.len()];
        let rel = slashes(&card.file);
        let name = card.name.as_str();
        let resp = ui
            .interact(cell, ui.id().with(("card", &rel)), Sense::click_and_drag())
            .on_hover_text(format!("{name}\n{rel}"));
        resp.dnd_set_drag_payload(DragCard(card.file.clone()));
        if resp.clicked() || resp.drag_started() || resp.secondary_clicked() {
            asks.select = Some(rel.clone());
        }
        if resp.double_clicked() {
            asks.commands.push(Command::AddCard(card.file.clone()));
        }
        let on = picked(st, &rel, asks);
        let thumb = match st.library.thumb(ui.ctx(), &card.path) {
            Some(tex) => Thumb::Image(tex.id(), square_uv(tex.size())),
            None => Thumb::Glyph(kind.glyph(), k.t.hover),
        };
        let text = TileText {
            rel: &rel,
            name,
            caption: kind.label(),
            caption_color: None,
        };
        let color = if card.male { Kind::MALE } else { kind.color() };
        draw_tile(ui, k, st, cell, &resp, on, thumb, color, text, asks);
        resp.context_menu(|ui| file_menu(ui, k, st, &rel, asks));
    });
}

/// The project's animations in the open folder, as tiles.
fn animations(ui: &mut Ui, k: &Look, st: &mut State, grid: Rect, open: &str, asks: &mut Asks) {
    let t = &k.t;
    let search = st.content_search.trim().to_lowercase();
    let searching = !search.is_empty();
    let filter = st.animations.filter;
    let inside = if searching { Vec::new() } else { subfolders(st, open) };
    let here: Vec<usize> = (0..st.animations.items.len())
        .filter(|&i| shown_in(st.animations.items[i].folder(), open, searching))
        .collect();
    let shown: Vec<usize> = here
        .iter()
        .copied()
        .filter(|&i| {
            let item = &st.animations.items[i];
            let fits = match (filter, item.is_static()) {
                (0, _) => true,
                (_, None) => false,
                (1, Some(fixed)) => fixed,
                (_, Some(fixed)) => !fixed,
            };
            fits && (!searching
                || item.name.to_lowercase().contains(&search)
                || item.category().to_lowercase().contains(&search))
        })
        .collect();
    // A character's keys saved as animations, which move.
    let keyed: Vec<String> = if filter == 1 {
        Vec::new()
    } else {
        let mut folders = vec![open.to_owned()];
        if searching {
            folders.extend(
                content::folders(&st.project, content::top(open))
                    .into_iter()
                    .filter(|f| in_open(f, open) && f != open),
            );
        }
        folders
            .iter()
            .flat_map(|f| content::files(&st.project, f))
            .filter(|f| crate::sequence::is_keys(f))
            .filter(|f| !searching || content::stem(f).to_lowercase().contains(&search))
            .collect()
    };
    if here.is_empty() && inside.is_empty() && keyed.is_empty() {
        if searching {
            note(ui, k, grid, "No animation matches");
        } else if !bare(st, project::ANIMATIONS) {
            centred(ui, k, grid, "This folder is empty");
        } else {
            empty_state(
                ui,
                k,
                grid,
                "This project has no animations yet",
                "Animations can be imported from a file or the library.",
                [Command::ImportAnimationFile, Command::ImportAnimations],
                asks,
            );
        }
        return;
    }
    if shown.is_empty() && inside.is_empty() && keyed.is_empty() {
        note(ui, k, grid, "No animation matches");
        return;
    }
    let worn = st.doc.selection().and_then(|o| match &o.content {
        crate::scene::Content::Character { animation, .. } => animation.clone(),
        _ => None,
    });
    let worn_rel = worn.as_ref().and_then(|a| st.animations.of(a)).map(|i| i.rel.clone());
    tile_grid(
        ui,
        k,
        "animations",
        inside.len() + keyed.len() + shown.len(),
        |ui, n, cell| {
            if let Some(rel) = inside.get(n) {
                folder_tile(ui, k, st, cell, rel, asks);
                return;
            }
            if let Some(rel) = keyed.get(n - inside.len()) {
                keys_tile(ui, k, st, cell, rel, asks);
                return;
            }
            let item = st.animations.items[shown[n - inside.len() - keyed.len()]].clone();
            let resp = ui.interact(cell, ui.id().with(("animation", &item.rel)), Sense::click_and_drag());
            let on = picked(st, &item.rel, asks);
            let g = match (item.is_static(), item.link.is_some()) {
                (_, true) => icon::LINK_SIMPLE,
                (Some(true), _) => Kind::Pose.glyph(),
                _ => Kind::Animation.glyph(),
            };
            let worn_here = worn_rel.as_deref() == Some(item.rel.as_str());
            let kind_text = match item.is_static() {
                Some(true) => "Static",
                Some(false) => "Moving",
                None => "Animation",
            };
            let text = TileText {
                rel: &item.rel,
                name: &item.name,
                caption: if worn_here { "On the selection" } else { kind_text },
                caption_color: worn_here.then_some(t.accent),
            };
            let thumb = Thumb::Glyph(g, Kind::Animation.color());
            draw_tile(ui, k, st, cell, &resp, on, thumb, Kind::Animation.color(), text, asks);
            resp.dnd_set_drag_payload(DragFile(item.rel.clone()));
            if resp.clicked() || resp.drag_started() || resp.secondary_clicked() {
                asks.select = Some(item.rel.clone());
            }
            if resp.double_clicked() {
                asks.commands.push(Command::Animate(Some(item.reference())));
            }
            resp.context_menu(|ui| file_menu(ui, k, st, &item.rel, asks));
            let from = item.meta.as_ref().map_or(String::new(), |m| {
                if m.source.is_empty() {
                    String::new()
                } else {
                    format!("\nFrom {}", m.source)
                }
            });
            resp.on_hover_text(format!(
                "{}\n{}, {}\n{}{from}",
                item.name,
                item.category(),
                item.kind_text(),
                item.rel
            ));
        },
    );
}

/// The project's maps in the open folder, as tiles of their pictures. A
/// map's own folder shows as the map.
fn maps(ui: &mut Ui, k: &Look, st: &mut State, grid: Rect, open: &str, asks: &mut Asks) {
    let t = &k.t;
    let search = st.content_search.trim().to_lowercase();
    let searching = !search.is_empty();
    let inside = if searching { Vec::new() } else { subfolders(st, open) };
    let maps: Vec<crate::maps::ProjectMap> = crate::maps::in_project(&st.project.root)
        .into_iter()
        .filter(|m| shown_in(m.folder(), open, searching))
        .filter(|m| !searching || m.map.name.to_lowercase().contains(&search))
        .collect();
    if maps.is_empty() && inside.is_empty() {
        if searching {
            note(ui, k, grid, "No map matches the search");
        } else if !bare(st, crate::maps::MAPS) {
            centred(ui, k, grid, "This folder is empty");
        } else {
            empty_state(
                ui,
                k,
                grid,
                "This project has no maps yet",
                "Maps can be imported from a file or the library.",
                [Command::ImportMapFile, Command::MapLibrary],
                asks,
            );
        }
        return;
    }
    let kind = Kind::Map;
    let in_scene = st.doc.scene().objects().iter().find_map(|o| match &o.content {
        crate::scene::Content::Map { map, .. } => Some(slashes(map)),
        _ => None,
    });
    tile_grid(ui, k, "maps", inside.len() + maps.len(), |ui, i, cell| {
        if let Some(rel) = inside.get(i) {
            folder_tile(ui, k, st, cell, rel, asks);
            return;
        }
        let map = &maps[i - inside.len()];
        let rel = map.file.clone();
        let folder = content::parent(&rel).to_owned();
        let resp = ui
            .interact(cell, ui.id().with(("map", &rel)), Sense::click())
            .on_hover_text(format!("{}\n{folder}", map.map.name));
        if resp.clicked() || resp.secondary_clicked() {
            asks.select = Some(rel.clone());
        }
        if resp.double_clicked() {
            asks.commands.push(Command::AddMap(PathBuf::from(&rel)));
        }
        let on = picked(st, &rel, asks);
        let picture = map
            .map
            .thumbnail
            .as_ref()
            .map(|p| st.project.root.join(&folder).join(p));
        let thumb = match picture.and_then(|p| st.library.thumb(ui.ctx(), &p).cloned()) {
            Some(tex) => Thumb::Image(tex.id(), square_uv(tex.size())),
            None => Thumb::Glyph(kind.glyph(), k.t.hover),
        };
        let shown = in_scene.as_deref() == Some(rel.as_str());
        let text = TileText {
            rel: &folder,
            name: &map.map.name,
            caption: if shown { "In the scene" } else { kind.label() },
            caption_color: shown.then_some(t.accent),
        };
        draw_tile(ui, k, st, cell, &resp, on, thumb, kind.color(), text, asks);
        resp.context_menu(|ui| {
            if widgets::menu_item(ui, k, icon::PLUS, "Add to scene", "", true) {
                asks.commands.push(Command::AddMap(PathBuf::from(&rel)));
            }
            ui.separator();
            if widgets::menu_item(ui, k, icon::ARROW_SQUARE_OUT, "Show in file manager", "", true) {
                asks.file(FileOp::Reveal(folder.clone()));
            }
            ui.separator();
            if widgets::menu_item(ui, k, icon::TRASH, "Delete", "", true) {
                asks.file(FileOp::Delete(folder.clone()));
            }
        });
    });
}

/// The files of any other folder, as tiles.
#[allow(clippy::too_many_arguments)]
fn files(ui: &mut Ui, k: &Look, st: &mut State, grid: Rect, open: &str, kind: Kind, empty: &str, asks: &mut Asks) {
    let search = st.content_search.trim().to_lowercase();
    let searching = !search.is_empty();
    let inside = if searching { Vec::new() } else { subfolders(st, open) };
    let found: Vec<String> = if searching {
        let mut folders = vec![open.to_owned()];
        folders.extend(
            content::folders(&st.project, content::top(open))
                .into_iter()
                .filter(|f| in_open(f, open) && f != open),
        );
        folders
            .iter()
            .flat_map(|f| content::files(&st.project, f))
            .filter(|f| content::leaf(f).to_lowercase().contains(&search))
            .collect()
    } else {
        content::files(&st.project, open)
    };
    if found.is_empty() && inside.is_empty() {
        if searching {
            note(ui, k, grid, "No file matches the search");
        } else if !bare(st, content::top(open)) {
            centred(ui, k, grid, "This folder is empty");
        } else {
            note(ui, k, grid, empty);
        }
        return;
    }
    tile_grid(ui, k, "files", inside.len() + found.len(), |ui, n, cell| {
        if let Some(rel) = inside.get(n) {
            folder_tile(ui, k, st, cell, rel, asks);
            return;
        }
        let rel = &found[n - inside.len()];
        let resp = ui.interact(cell, ui.id().with(("file", rel)), Sense::click_and_drag());
        let on = picked(st, rel, asks);
        let lower = rel.to_lowercase();
        let g = if lower.ends_with(".png") {
            icon::IMAGE
        } else if lower.ends_with(".zipmod") {
            icon::PACKAGE
        } else if lower.ends_with(".rkscene") || lower.ends_with(".toml") {
            Kind::Scene.glyph()
        } else if lower.ends_with(".rkpose") {
            Kind::Pose.glyph()
        } else {
            icon::FILE
        };
        // A pose shows by its name and kind, as an animation does; other
        // files by their file name and size.
        let pose = is_pose(rel);
        let caption = if pose {
            "Pose".to_owned()
        } else {
            crate::mods::size_text(std::fs::metadata(st.project.root.join(rel)).map_or(0, |m| m.len()))
        };
        let text = TileText {
            rel,
            name: if pose { content::stem(rel) } else { content::leaf(rel) },
            caption: &caption,
            caption_color: None,
        };
        draw_tile(
            ui,
            k,
            st,
            cell,
            &resp,
            on,
            Thumb::Glyph(g, kind.color()),
            kind.color(),
            text,
            asks,
        );
        resp.dnd_set_drag_payload(DragFile(rel.clone()));
        if resp.clicked() || resp.drag_started() || resp.secondary_clicked() {
            asks.select = Some(rel.clone());
        }
        if resp.double_clicked() && is_pose(rel) && character_chosen(st) {
            asks.commands.push(Command::LoadPose(rel.clone()));
        }
        resp.context_menu(|ui| file_menu(ui, k, st, rel, asks));
        resp.on_hover_text(rel.as_str());
    });
}

/// A character's keys saved as an animation: a click selects it, a
/// double click gives it to the selected character, and it drags onto a
/// character in the Outliner.
fn keys_tile(ui: &mut Ui, k: &Look, st: &mut State, cell: Rect, rel: &str, asks: &mut Asks) {
    let resp = ui.interact(cell, ui.id().with(("keys", rel)), Sense::click_and_drag());
    let on = picked(st, rel, asks);
    let text = TileText {
        rel,
        name: content::stem(rel),
        caption: "Keys",
        caption_color: None,
    };
    draw_tile(
        ui,
        k,
        st,
        cell,
        &resp,
        on,
        Thumb::Glyph(icon::DIAMOND, Kind::Animation.color()),
        Kind::Animation.color(),
        text,
        asks,
    );
    resp.dnd_set_drag_payload(DragFile(rel.to_owned()));
    if resp.clicked() || resp.drag_started() || resp.secondary_clicked() {
        asks.select = Some(rel.to_owned());
    }
    if resp.double_clicked() && character_chosen(st) {
        asks.commands.push(Command::LoadKeys(rel.to_owned()));
    }
    resp.context_menu(|ui| file_menu(ui, k, st, rel, asks));
}

fn is_pose(rel: &str) -> bool {
    content::top(rel) == project::POSES && rel.to_lowercase().ends_with(".rkpose")
}

fn character_chosen(st: &State) -> bool {
    st.doc.selection().is_some_and(|o| o.kind() == Kind::Character)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open_folder_shows_what_lies_in_it_and_below() {
        assert!(in_open("Animations", "Animations"));
        assert!(in_open("Animations/Sitting/Chairs", "Animations/Sitting"));
        assert!(!in_open("Animations/Sit", "Animations/Sitting"));
        assert!(!in_open("Animations/Sitting 2", "Animations/Sitting"));
        assert!(!in_open("Animations", "Animations/Sitting"));
    }

    #[test]
    fn a_tile_shows_a_square_of_any_picture() {
        // A card's 240 by 320 face shot: its full width, from a little
        // below the top, where the face is.
        let face = square_uv([240, 320]);
        assert_eq!((face.min.x, face.max.x), (0.0, 1.0));
        assert!((face.min.y - 0.075).abs() < 1e-6 && (face.height() - 0.75).abs() < 1e-6);
        // A wide picture: its middle.
        let wide = square_uv([400, 200]);
        assert_eq!((wide.min.x, wide.max.x, wide.min.y, wide.max.y), (0.25, 0.75, 0.0, 1.0));
        assert_eq!(square_uv([64, 64]), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)));
    }

    #[test]
    fn searching_reaches_below_the_open_folder() {
        assert!(shown_in("Animations", "Animations", false));
        assert!(!shown_in("Animations/Dances", "Animations", false));
        assert!(shown_in("Animations/Dances", "Animations", true));
    }

    #[test]
    fn every_project_folder_has_a_kind() {
        for folder in project::FOLDERS {
            assert!(!folder_kind(folder).1.is_empty(), "{folder}");
        }
        assert_eq!(folder_kind(project::ANIMATIONS).0, Kind::Animation);
        assert_eq!(folder_kind(project::POSES).0, Kind::Pose);
    }
}
