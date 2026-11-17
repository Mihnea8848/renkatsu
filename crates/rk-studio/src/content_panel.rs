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
use egui::{Align, Align2, Color32, Layout, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
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
        project::CHARACTERS => (
            Kind::Character,
            "The project has no characters yet. Import a card to add one.",
        ),
        project::ANIMATIONS => (Kind::Animation, "This project has no animations yet"),
        project::POSES => (Kind::Pose, "Poses made by hand arrive with FK and IK posing"),
        "Maps" => (Kind::Map, "Maps arrive with their feature"),
        "Items" => (Kind::Item, "Items arrive with their feature"),
        project::SCENES => (Kind::Scene, "A project holds one scene for now"),
        "Renders" => (Kind::Camera, "Renders arrive with baking"),
        _ => (Kind::Folder, "Imported cards copy the mods they need here"),
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
    /// A file or folder to start renaming.
    rename: Option<String>,
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
            if widgets::menu_item(ui, k, icon::PACKAGE, "Import from a package", "", true) {
                asks.commands.push(Command::ImportFromPackage);
            }
            ui.separator();
        }
        project::ANIMATIONS => {
            if widgets::menu_item(ui, k, icon::DOWNLOAD_SIMPLE, "Import from file here", "", true) {
                asks.commands.push(Command::ImportAnimationFile);
            }
            if widgets::menu_item(ui, k, icon::PACKAGE, "Import from package here", "", true) {
                asks.commands.push(Command::ImportAnimations);
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
            asks.file(FileOp::Delete(folder.to_owned()));
        }
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
        asks.file(FileOp::Delete(rel.to_owned()));
    }
}

/// The text field of the file or folder being renamed, when that is
/// `rel`: drawn over `rect`. Returns whether it was drawn.
fn renaming(ui: &mut Ui, k: &Look, st: &mut State, rel: &str, rect: Rect, asks: &mut Asks) -> bool {
    let Some((target, text)) = &mut st.content_renaming else {
        return false;
    };
    if target != rel {
        return false;
    }
    if let Some(name) = widgets::rename_field(ui, k, rect, text) {
        if !name.trim().is_empty() {
            asks.file(FileOp::Rename(rel.to_owned(), name));
        }
        st.content_renaming = None;
    }
    true
}

/// What is being dragged over `rect`, when it can be moved into `folder`:
/// a file or a card of the same project folder.
fn drop_into(ui: &Ui, k: &Look, rect: Rect, folder: &str, asks: &mut Asks) {
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
        asks.file(FileOp::Move(rel, folder.to_owned()));
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
                if flat_button(ui, k, Some(icon::DOWNLOAD_SIMPLE), "Import card", false, false)
                    .on_hover_text("Pick a card and copy it into the project, with the mods it needs")
                    .clicked()
                {
                    asks.commands.push(Command::ImportCard);
                }
                if flat_button(ui, k, Some(icon::PACKAGE), "From a package", false, false)
                    .on_hover_text("Choose characters of the installed packages and copy their cards into the project")
                    .clicked()
                {
                    asks.commands.push(Command::ImportFromPackage);
                }
                let card = st.asset.clone().filter(|a| slashes(a).to_lowercase().ends_with(".png"));
                let why = if card.is_some() {
                    "Add the selected character to the scene"
                } else {
                    "Select a character first, or double click one"
                };
                if flat_button(ui, k, Some(icon::PLUS), "Add to scene", false, false)
                    .on_hover_text(why)
                    .clicked()
                {
                    if let Some(card) = card {
                        asks.commands.push(Command::AddCard(card));
                    }
                }
            }
            project::ANIMATIONS => animation_tools(ui, k, st, &mut asks),
            _ => {}
        }
        if icon_button(ui, k, icon::ARROWS_CLOCKWISE, false)
            .on_hover_text("Read the project's folders again")
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
            widgets::search(ui, k, &mut st.content_search, "Search this folder", 240.0);
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
    }
    ground.context_menu(|ui| folder_menu(ui, k, st, &open, &mut asks));
    let mut area = ui.new_child(UiBuilder::new().max_rect(grid));
    area.set_clip_rect(grid.intersect(ui.clip_rect()));
    match top {
        project::CHARACTERS => cards(&mut area, k, st, grid, &open, empty_note, &mut asks),
        project::ANIMATIONS => animations(&mut area, k, st, grid, &open, &mut asks),
        _ => files(&mut area, k, st, grid, &open, kind, empty_note, &mut asks),
    }
    ui.allocate_rect(body, Sense::hover());

    if let Some(folder) = asks.open.take() {
        st.open_folder(&folder);
        st.asset = None;
    }
    if let Some(rel) = asks.select.take() {
        st.asset = Some(PathBuf::from(rel));
    }
    if let Some(rel) = asks.rename.take() {
        st.content_renaming = Some((rel.clone(), content::stem(&rel).to_owned()));
    }
    for command in asks.commands {
        st.ask(command, ui.ctx());
    }
}

/// The toolbar of the Animations folder.
fn animation_tools(ui: &mut Ui, k: &Look, st: &mut State, asks: &mut Asks) {
    if flat_button(ui, k, Some(icon::DOWNLOAD_SIMPLE), "Import from file", false, false)
        .on_hover_text("Pick an animation file or a zipmod and copy animations from it into the project")
        .clicked()
    {
        asks.commands.push(Command::ImportAnimationFile);
    }
    if flat_button(ui, k, Some(icon::PACKAGE), "Import from package", false, false)
        .on_hover_text("Choose animations of the installed packages, of Koikatsu and of mods, and copy them in")
        .clicked()
    {
        asks.commands.push(Command::ImportAnimations);
    }
    let character = st.doc.selection().is_some_and(|o| o.kind() == Kind::Character);
    let chosen = st
        .asset
        .as_ref()
        .and_then(|a| st.animations.find(&slashes(a)))
        .map(|i| i.reference());
    let why = match (character, chosen.is_some()) {
        (false, _) => "Select a character in the scene first",
        (true, false) => "Select an animation first, or double click one",
        (true, true) => "Give the selected character the selected animation",
    };
    ui.add_enabled_ui(character && chosen.is_some(), |ui| {
        if flat_button(ui, k, Some(icon::PLAY), "Apply to selection", false, false)
            .on_hover_text(why)
            .on_disabled_hover_text(why)
            .clicked()
        {
            asks.commands.push(Command::Animate(chosen.clone()));
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

/// The tree at the left: the project, its folders and, under the open
/// one, the folders the user made.
fn folder_tree(ui: &mut Ui, k: &Look, st: &mut State, tree: Rect, open: &str, asks: &mut Asks) {
    let t = &k.t;
    let mut side = ui.new_child(UiBuilder::new().max_rect(tree));
    side.set_clip_rect(tree.intersect(ui.clip_rect()));
    egui::ScrollArea::vertical()
        .id_salt("folders")
        .auto_shrink(false)
        .show(&mut side, |ui| {
            ui.add_space(4.0);
            // The project folder holds the others, as on disk.
            let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row), Sense::hover());
            let p = ui.painter();
            let x = row.left() + 10.0;
            glyph(p, k, Pos2::new(x, row.center().y), icon::CARET_DOWN, 10.0, t.text_dim);
            glyph(
                p,
                k,
                Pos2::new(x + 14.0, row.center().y),
                icon::FOLDER_OPEN,
                15.0,
                t.text_dim,
            );
            let clip = p.with_clip_rect(row.with_max_x(row.right() - 4.0));
            label(
                &clip,
                k,
                Pos2::new(x + 28.0, row.center().y),
                &st.project.file.name,
                t.text_bright,
            );
            let open_top = content::top(open).to_owned();
            for top in project::FOLDERS {
                let (kind, _) = folder_kind(top);
                folder_row(ui, k, st, top, 0, kind, open, asks);
                if top != open_top {
                    continue;
                }
                for folder in content::folders(&st.project, top) {
                    let depth = folder.matches('/').count();
                    folder_row(ui, k, st, &folder, depth, kind, open, asks);
                }
            }
        });
}

/// One folder of the tree.
#[allow(clippy::too_many_arguments)]
fn folder_row(ui: &mut Ui, k: &Look, st: &mut State, rel: &str, depth: usize, kind: Kind, open: &str, asks: &mut Asks) {
    let t = &k.t;
    let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row), Sense::click());
    if resp.clicked() {
        asks.open = Some(rel.to_owned());
    }
    if resp.double_clicked() && content::changeable(rel) {
        asks.rename = Some(rel.to_owned());
    }
    let on = open == rel;
    {
        let p = ui.painter();
        if on {
            p.rect_filled(row, 0.0, t.control);
        } else if resp.hovered() {
            p.rect_filled(row, 0.0, t.row_hover);
        }
        if on && depth > 0 {
            p.rect_filled(
                Rect::from_min_size(row.left_top(), Vec2::new(2.0, row.height())),
                0.0,
                kind.color(),
            );
        }
    }
    let x = row.left() + 34.0 + depth as f32 * 14.0;
    let g = if on { icon::FOLDER_OPEN } else { icon::FOLDER_SIMPLE };
    let tint = if depth == 0 { kind.color() } else { Kind::Folder.color() };
    glyph(ui.painter(), k, Pos2::new(x, row.center().y), g, 15.0, tint);
    let name_rect = Rect::from_min_max(
        Pos2::new(x + 10.0, row.top() + 1.0),
        Pos2::new(row.right() - 4.0, row.bottom() - 1.0),
    );
    if !renaming(ui, k, st, rel, name_rect, asks) {
        let color = if on { t.text_bright } else { t.text };
        let clip = ui.painter().with_clip_rect(row.with_max_x(row.right() - 4.0));
        label(&clip, k, Pos2::new(x + 14.0, row.center().y), content::leaf(rel), color);
    }
    drop_into(ui, k, row, rel, asks);
    resp.context_menu(|ui| folder_menu(ui, k, st, rel, asks));
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

/// The project's characters in the open folder, as their cards.
fn cards(ui: &mut Ui, k: &Look, st: &mut State, grid: Rect, open: &str, empty: &str, asks: &mut Asks) {
    let t = &k.t;
    let search = st.content_search.trim().to_lowercase();
    let cards: Vec<crate::library::Entry> = st
        .library
        .characters
        .iter()
        .filter(|c| in_open(content::parent(&slashes(&c.file)), open))
        .filter(|c| {
            search.is_empty()
                || c.name.to_lowercase().contains(&search)
                || slashes(&c.file).to_lowercase().contains(&search)
        })
        .cloned()
        .collect();
    if cards.is_empty() && !search.is_empty() {
        note(ui, k, grid, "No character matches the search");
    } else if cards.is_empty() && open != project::CHARACTERS {
        note(
            ui,
            k,
            grid,
            "This folder has no characters. Drag cards onto it in the tree.",
        );
    } else if cards.is_empty() {
        note(ui, k, grid, empty);
    }
    // Cards are portrait pictures, 252 by 352.
    let thumb = Vec2::new((k.d.row * 2.5).round(), (k.d.row * 2.5 * 352.0 / 252.0).round());
    let caption = k.d.tree != Tree::Plain;
    let tile = Vec2::new(thumb.x + 12.0, thumb.y + if caption { 40.0 } else { 26.0 });
    let kind = Kind::Character;
    egui::ScrollArea::vertical()
        .id_salt("cards")
        .auto_shrink(false)
        .show(ui, |ui| {
            let columns = ((ui.available_width() - 12.0) / tile.x).floor().max(1.0) as usize;
            let rows = cards.len().div_ceil(columns);
            let (space, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), rows as f32 * tile.y + 12.0),
                Sense::hover(),
            );
            for (i, card) in cards.iter().enumerate() {
                let cell = Rect::from_min_size(
                    space.min
                        + Vec2::new(
                            12.0 + (i % columns) as f32 * tile.x,
                            10.0 + (i / columns) as f32 * tile.y,
                        ),
                    tile - Vec2::new(12.0, 0.0),
                );
                if !ui.is_rect_visible(cell) {
                    continue;
                }
                let rel = slashes(&card.file);
                let name = card.name.as_str();
                let resp = ui
                    .interact(cell, ui.id().with(("card", &rel)), Sense::click_and_drag())
                    .on_hover_text(format!(
                        "{name}\n{rel}\nDouble click or drag into the scene to add. Right click for more."
                    ));
                resp.dnd_set_drag_payload(DragCard(card.file.clone()));
                if resp.clicked() || resp.drag_started() || resp.secondary_clicked() {
                    asks.select = Some(rel.clone());
                }
                if resp.double_clicked() {
                    asks.commands.push(Command::AddCard(card.file.clone()));
                }
                let on = st.asset.as_ref() == Some(&card.file);
                let picture = Rect::from_min_size(cell.min, thumb);
                let texture = st.library.thumb(ui.ctx(), &card.path).map(|tex| tex.id());
                {
                    let p = ui.painter();
                    p.rect_filled(picture, 0.0, t.input);
                    match texture {
                        Some(id) => {
                            p.image(
                                id,
                                picture,
                                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        }
                        None => glyph(p, k, picture.center(), kind.glyph(), thumb.x * 0.42, t.hover),
                    }
                    // The type colour along the picture's foot, as Unreal's tiles.
                    p.rect_filled(
                        Rect::from_min_max(Pos2::new(picture.left(), picture.bottom() - 2.0), picture.max),
                        0.0,
                        kind.color(),
                    );
                    if on {
                        p.rect_stroke(picture, 0.0, Stroke::new(1.0, t.select), StrokeKind::Outside);
                        p.rect_stroke(picture, 0.0, Stroke::new(1.0, t.select), StrokeKind::Inside);
                    } else if resp.hovered() {
                        p.rect_stroke(picture, 0.0, Stroke::new(1.0, t.hover), StrokeKind::Inside);
                    }
                }
                let name_rect = Rect::from_min_size(
                    Pos2::new(cell.left(), picture.bottom() + 2.0),
                    Vec2::new(cell.width(), 20.0),
                );
                if !renaming(ui, k, st, &rel, name_rect, asks) {
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
                        name,
                        color,
                    );
                    if caption {
                        clip.text(
                            Pos2::new(cell.left() + 1.0, picture.bottom() + 27.0),
                            Align2::LEFT_CENTER,
                            kind.label(),
                            k.small(),
                            t.text_dim,
                        );
                    }
                }
                resp.context_menu(|ui| file_menu(ui, k, st, &rel, asks));
            }
        });
}

/// One line of the list of animations.
enum Line {
    /// A folder's name and how many of its animations show.
    Header(String, usize),
    Item(usize),
}

/// The project's animations in the open folder, as a list by folder.
fn animations(ui: &mut Ui, k: &Look, st: &mut State, grid: Rect, open: &str, asks: &mut Asks) {
    let t = &k.t;
    let search = st.content_search.trim().to_lowercase();
    let filter = st.animations.filter;
    let here: Vec<usize> = (0..st.animations.items.len())
        .filter(|&i| in_open(st.animations.items[i].folder(), open))
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
            fits && (search.is_empty()
                || item.name.to_lowercase().contains(&search)
                || item.category().to_lowercase().contains(&search))
        })
        .collect();
    if here.is_empty() {
        // Nothing here yet: say so in the middle, with the two ways in.
        let c = grid.center();
        let text = if st.animations.items.is_empty() {
            "This project has no animations yet"
        } else {
            "This folder has no animations yet"
        };
        ui.painter().text(
            c - Vec2::new(0.0, 34.0),
            Align2::CENTER_CENTER,
            text,
            k.bold(k.d.text + 1.0),
            t.text_bright,
        );
        ui.painter().text(
            c - Vec2::new(0.0, 12.0),
            Align2::CENTER_CENTER,
            "Imported animations are copied into the project, so it plays them anywhere.",
            k.font(),
            t.text_dim,
        );
        let size = Vec2::new(170.0, 30.0);
        let left = Rect::from_min_size(Pos2::new(c.x - size.x - 5.0, c.y + 12.0), size);
        let right = Rect::from_min_size(Pos2::new(c.x + 5.0, c.y + 12.0), size);
        if crate::selector::button(ui, k, left, "Import from file", false, true) {
            asks.commands.push(Command::ImportAnimationFile);
        }
        if crate::selector::button(ui, k, right, "Import from package", true, true) {
            asks.commands.push(Command::ImportAnimations);
        }
        return;
    }
    if shown.is_empty() {
        note(ui, k, grid, "No animation matches");
        return;
    }
    // A header for every folder, unless everything sits in the open one.
    let several = shown.iter().any(|&i| st.animations.items[i].folder() != open);
    let mut lines: Vec<Line> = Vec::new();
    let mut last: Option<&str> = None;
    for &i in &shown {
        let folder = st.animations.items[i].folder();
        if several && last != Some(folder) {
            let count = shown
                .iter()
                .filter(|&&j| st.animations.items[j].folder() == folder)
                .count();
            let name = match folder.strip_prefix(open).map(|f| f.trim_start_matches('/')) {
                Some("") | None => content::leaf(open).to_owned(),
                Some(rest) => rest.replace('/', " / "),
            };
            lines.push(Line::Header(name, count));
            last = Some(folder);
        }
        lines.push(Line::Item(i));
    }
    let worn = st.doc.selection().and_then(|o| match &o.content {
        crate::scene::Content::Character { animation, .. } => animation.clone(),
        _ => None,
    });
    let worn_rel = worn.as_ref().and_then(|a| st.animations.of(a)).map(|i| i.rel.clone());
    egui::ScrollArea::vertical()
        .id_salt("animations")
        .auto_shrink(false)
        .show_rows(ui, k.d.row, lines.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for line in &lines[range] {
                let (row, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row), Sense::click_and_drag());
                let y = row.center().y;
                let i = match line {
                    Line::Header(name, count) => {
                        let p = ui.painter();
                        p.rect_filled(row, 0.0, t.header);
                        hair_h(p, row.left(), row.right(), row.bottom() - 1.0, t.line);
                        glyph(
                            p,
                            k,
                            Pos2::new(row.left() + 18.0, y),
                            icon::FOLDER_SIMPLE,
                            13.0,
                            Kind::Folder.color(),
                        );
                        let x = label(p, k, Pos2::new(row.left() + 32.0, y), name, t.text_bright);
                        p.text(
                            Pos2::new(x + 8.0, y),
                            Align2::LEFT_CENTER,
                            count.to_string(),
                            k.small(),
                            t.text_dim,
                        );
                        continue;
                    }
                    Line::Item(i) => *i,
                };
                let item = st.animations.items[i].clone();
                let on = st.asset.as_deref() == Some(Path::new(&item.rel));
                {
                    let p = ui.painter();
                    if on {
                        p.rect_filled(row, 0.0, t.select);
                    } else if resp.hovered() {
                        p.rect_filled(row, 0.0, t.row_hover);
                    }
                    let fixed = item.is_static();
                    let g = match (fixed, item.link.is_some()) {
                        (_, true) => icon::LINK_SIMPLE,
                        (Some(true), _) => Kind::Pose.glyph(),
                        _ => Kind::Animation.glyph(),
                    };
                    let tint = if on { t.text_bright } else { Kind::Animation.color() };
                    glyph(p, k, Pos2::new(row.left() + 22.0, y), g, 14.0, tint);
                }
                let dim = if on { t.text_bright } else { t.text_dim };
                let name_rect = Rect::from_min_max(
                    Pos2::new(row.left() + 34.0, row.top() + 1.0),
                    Pos2::new(row.right() - 430.0, row.bottom() - 1.0).max(Pos2::new(row.left() + 160.0, 0.0)),
                );
                if !renaming(ui, k, st, &item.rel, name_rect, asks) {
                    let p = ui.painter();
                    let color = if on { t.text_bright } else { t.text };
                    let clip = p.with_clip_rect(row.with_max_x(row.right() - 440.0));
                    let x = label(&clip, k, Pos2::new(row.left() + 38.0, y), &item.name, color);
                    if worn_rel.as_deref() == Some(item.rel.as_str()) {
                        clip.text(
                            Pos2::new(x + 10.0, y),
                            Align2::LEFT_CENTER,
                            "on the selection",
                            k.small(),
                            if on { t.text_bright } else { t.accent },
                        );
                    }
                }
                let p = ui.painter();
                let clip = p.with_clip_rect(Rect::from_min_max(
                    Pos2::new(row.right() - 430.0, row.top()),
                    Pos2::new(row.right() - 200.0, row.bottom()),
                ));
                clip.text(
                    Pos2::new(row.right() - 430.0, y),
                    Align2::LEFT_CENTER,
                    item.category(),
                    k.small(),
                    dim,
                );
                p.text(
                    Pos2::new(row.right() - 16.0, y),
                    Align2::RIGHT_CENTER,
                    item.kind_text(),
                    k.small(),
                    dim,
                );
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
                    "{}\n{}{from}\nDouble click to give it to the selected character. Right click for more.",
                    item.name, item.rel
                ));
            }
        });
}

/// The files of any other folder, one a row.
#[allow(clippy::too_many_arguments)]
fn files(ui: &mut Ui, k: &Look, st: &mut State, grid: Rect, open: &str, kind: Kind, empty: &str, asks: &mut Asks) {
    let t = &k.t;
    let search = st.content_search.trim().to_lowercase();
    let mut folders = vec![open.to_owned()];
    folders.extend(
        content::folders(&st.project, content::top(open))
            .into_iter()
            .filter(|f| in_open(f, open) && f != open),
    );
    let found: Vec<String> = folders
        .iter()
        .flat_map(|f| content::files(&st.project, f))
        .filter(|f| search.is_empty() || content::leaf(f).to_lowercase().contains(&search))
        .collect();
    if found.is_empty() {
        note(
            ui,
            k,
            grid,
            if search.is_empty() {
                empty
            } else {
                "No file matches the search"
            },
        );
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("files")
        .auto_shrink(false)
        .show_rows(ui, k.d.row, found.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for rel in &found[range] {
                let (row, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row), Sense::click_and_drag());
                let y = row.center().y;
                let on = st.asset.as_deref() == Some(Path::new(rel));
                let lower = rel.to_lowercase();
                let g = if lower.ends_with(".png") {
                    icon::IMAGE
                } else if lower.ends_with(".zipmod") {
                    icon::PACKAGE
                } else if lower.ends_with(".toml") {
                    Kind::Scene.glyph()
                } else {
                    icon::FILE
                };
                {
                    let p = ui.painter();
                    if on {
                        p.rect_filled(row, 0.0, t.select);
                    } else if resp.hovered() {
                        p.rect_filled(row, 0.0, t.row_hover);
                    }
                    let tint = if on { t.text_bright } else { kind.color() };
                    glyph(p, k, Pos2::new(row.left() + 22.0, y), g, 14.0, tint);
                }
                let name_rect = Rect::from_min_max(
                    Pos2::new(row.left() + 34.0, row.top() + 1.0),
                    Pos2::new(row.right() - 120.0, row.bottom() - 1.0),
                );
                if !renaming(ui, k, st, rel, name_rect, asks) {
                    let p = ui.painter();
                    let color = if on { t.text_bright } else { t.text };
                    let clip = p.with_clip_rect(row.with_max_x(row.right() - 120.0));
                    let shown = rel
                        .strip_prefix(open)
                        .map_or(rel.as_str(), |r| r.trim_start_matches('/'));
                    label(&clip, k, Pos2::new(row.left() + 38.0, y), shown, color);
                }
                let size = std::fs::metadata(st.project.root.join(rel)).map_or(0, |m| m.len());
                ui.painter().text(
                    Pos2::new(row.right() - 16.0, y),
                    Align2::RIGHT_CENTER,
                    crate::mods::size_text(size),
                    k.small(),
                    if on { t.text_bright } else { t.text_dim },
                );
                resp.dnd_set_drag_payload(DragFile(rel.clone()));
                if resp.clicked() || resp.drag_started() || resp.secondary_clicked() {
                    asks.select = Some(rel.clone());
                }
                resp.context_menu(|ui| file_menu(ui, k, st, rel, asks));
            }
        });
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
    fn every_project_folder_has_a_kind() {
        for folder in project::FOLDERS {
            assert!(!folder_kind(folder).1.is_empty(), "{folder}");
        }
        assert_eq!(folder_kind(project::ANIMATIONS).0, Kind::Animation);
        assert_eq!(folder_kind(project::POSES).0, Kind::Pose);
    }
}
