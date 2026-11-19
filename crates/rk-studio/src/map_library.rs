// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Maps library, the window that adds a map to the scene: the maps of
//! one source at a time, Koikatsu, a package or the mods, with the places
//! they stand in as a tree at the left and their pictures in a grid,
//! grouped by place while all of them show. Also the pictures of maps at
//! their times of day, which Properties shows too.
//!
//! The list is read on a thread of its own, so the window opens at once.

use crate::maps::ProjectMap;
use crate::scene::MapTime;
use crate::sources;
use crate::state::{Look, State};
use crate::widgets::{self, glyph, hair_v};
use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, Ui, Vec2};
use egui_phosphor::regular as icon;
use rk_chara::animations::Source;
use rk_vfs::index::norm;
use rk_vfs::Game;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Where a map is, for grouping: the game's maps by where they stand,
/// and the maps of mods the game does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    School,
    Outside,
    Other,
    Mods,
}

impl Place {
    pub const ALL: [Place; 4] = [Place::School, Place::Outside, Place::Other, Place::Mods];

    pub fn label(self) -> &'static str {
        match self {
            Place::School => "School",
            Place::Outside => "Outside",
            Place::Other => "Other",
            Place::Mods => "Mods",
        }
    }
}

/// The game's and kPlug's maps away from school.
const OUTSIDE: [&str; 8] = [
    "lovehotel",
    "karaoke",
    "church",
    "traincrowded",
    "train",
    "park",
    "thehotsprings",
    "thebeach",
];

/// The game's and kPlug's maps that are neither at school nor outside.
const OTHER: [&str; 3] = ["myroom", "thegirlroom", "livestage_s"];

/// Where the map whose scene bundle is `bundle` stands.
fn place_of(bundle: &str, from_mod: bool) -> Place {
    let stem = bundle.rsplit('/').next().unwrap_or(bundle).trim_end_matches(".unity3d");
    if rk_map::names::english(bundle).is_none() {
        return if from_mod { Place::Mods } else { Place::Other };
    }
    if OUTSIDE.contains(&stem) {
        Place::Outside
    } else if OTHER.contains(&stem) {
        Place::Other
    } else {
        Place::School
    }
}

/// A map the library offers.
#[derive(Debug, Clone)]
pub struct Item {
    pub name: String,
    /// Who brings it; `None` for a map of the project the game does not
    /// list.
    pub source: Option<Source>,
    pub place: Place,
    /// Its scene bundle.
    pub bundle: String,
    /// The game's row for it, to import it.
    pub entry: Option<rk_map::list::Entry>,
    /// The project's sidecar, for a map the project has.
    pub owned: Option<String>,
    /// The project's picture of it.
    pub picture: Option<PathBuf>,
    /// The times of day it has, as far as they are known.
    pub times: Vec<MapTime>,
    /// What importing it copies, as far as it is known before the copy.
    pub size: Option<u64>,
}

impl Item {
    /// The time it shows at in the library: day when it has it.
    fn first_time(&self) -> MapTime {
        if self.times.is_empty() || self.times.contains(&MapTime::Day) {
            MapTime::Day
        } else {
            self.times[0]
        }
    }
}

/// What a source of the dropdown shows.
#[derive(Debug, Clone, PartialEq)]
enum Shows {
    Project,
    One(Source),
    Mods,
}

#[derive(Debug, Clone)]
struct SourceRow {
    label: String,
    depth: usize,
    count: usize,
    shows: Shows,
}

/// What the thread that reads the maps sends: the maps, and where the
/// game's map list puts their pictures.
type Read = (Vec<Item>, Option<HashMap<String, (String, String)>>);

/// The Maps library.
pub struct Shelf {
    pub items: Vec<Item>,
    /// The thread that reads the maps, while it does.
    incoming: Option<Receiver<Read>>,
    game: Option<Arc<Game>>,
    rows: Vec<SourceRow>,
    /// The source that shows, of `rows`.
    source: usize,
    /// The place that shows; all of them when `None`.
    place: Option<Place>,
    pub search: String,
    pub chosen: Option<usize>,
    /// The chosen map goes in the scene; otherwise it is only imported
    /// into the project.
    to_scene: bool,
}

/// The name a source shows under.
fn source_name(source: &Source, known: &[rk_packages::index::Known]) -> String {
    match source {
        Source::Package(id) => known
            .iter()
            .find(|k| &k.id == id)
            .map_or_else(|| id.clone(), |k| k.name.clone()),
        other => other.label().to_owned(),
    }
}

/// The maps of the game, the packages and the mods, with the project's
/// own marked, then the project's maps the game does not list.
fn read(game: Option<&Game>, owned: &[ProjectMap], root: &Path) -> Read {
    let mut items = Vec::new();
    let mut used = vec![false; owned.len()];
    let table = game.map(rk_map::thumbnail::table);
    if let (Some(game), Some(table)) = (game, &table) {
        for entry in rk_map::list::read(game, false) {
            let bundle = norm(&entry.bundle);
            let own = owned.iter().position(|m| norm(m.map.scene_bundle()) == bundle);
            if let Some(i) = own {
                used[i] = true;
            }
            let own = own.map(|i| &owned[i]);
            // The times of a map not imported yet are those the game has
            // pictures of.
            let times = match (own, table.get(&bundle)) {
                (Some(m), _) => m.map.times.clone(),
                (None, Some(listed)) => MapTime::ALL
                    .into_iter()
                    .filter(|t| rk_map::thumbnail::has_time(game, listed, t.time_of_day()))
                    .collect(),
                (None, None) => Vec::new(),
            };
            let from_mod = matches!(entry.source, Source::Mod(_));
            items.push(Item {
                name: rk_map::names::shown(&entry),
                source: Some(entry.source.clone()),
                place: place_of(&bundle, from_mod),
                size: game.assets.bundle_size(&entry.bundle),
                owned: own.map(|m| m.file.clone()),
                picture: own.and_then(|m| project_picture(root, m)),
                bundle,
                times,
                entry: Some(entry),
            });
        }
    }
    for (m, _) in owned.iter().zip(&used).filter(|(_, used)| !**used) {
        items.push(Item {
            name: m.map.name.clone(),
            source: None,
            place: place_of(m.map.scene_bundle(), true),
            bundle: norm(m.map.scene_bundle()),
            entry: None,
            owned: Some(m.file.clone()),
            picture: project_picture(root, m),
            times: m.map.times.clone(),
            size: None,
        });
    }
    (items, table)
}

impl Shelf {
    /// Opens the library and starts reading the maps; a scripted run
    /// reads them at once.
    pub fn open(st: &mut State, to_scene: bool) -> Shelf {
        let owned = crate::maps::in_project(&st.project.root);
        let root = st.project.root.clone();
        let game = st.games.shared(&st.project.mods_dir()).ok();
        let mut shelf = Shelf {
            items: Vec::new(),
            incoming: None,
            game: game.clone(),
            rows: Vec::new(),
            source: 0,
            place: None,
            search: String::new(),
            chosen: None,
            to_scene,
        };
        if st.scripted {
            let read = read(game.as_deref(), &owned, &root);
            shelf.take(read, &mut st.map_pictures);
            return shelf;
        }
        let (send, receive) = channel();
        let spawned = std::thread::Builder::new().name("map library".into()).spawn(move || {
            let _ = send.send(read(game.as_deref(), &owned, &root));
        });
        shelf.incoming = spawned.is_ok().then_some(receive);
        shelf
    }

    /// Takes the maps once they are read. Returns whether they are still
    /// being read.
    fn advance(&mut self, pictures: &mut Pictures) -> bool {
        let Some(incoming) = self.incoming.take() else {
            return false;
        };
        match incoming.try_recv() {
            Ok(read) => {
                self.take(read, pictures);
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                self.incoming = Some(incoming);
                true
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        }
    }

    fn take(&mut self, (items, table): Read, pictures: &mut Pictures) {
        if let Some(table) = table {
            pictures.table.get_or_insert(table);
        }
        self.rows = source_rows(&items);
        self.items = items;
        // Koikatsu first, where it is one.
        self.source = self
            .rows
            .iter()
            .position(|r| r.shows == Shows::One(Source::Game))
            .unwrap_or(0);
    }

    fn of_source(&self, item: &Item) -> bool {
        match self.rows.get(self.source).map(|r| &r.shows) {
            None => true,
            Some(Shows::Project) => item.source.is_none(),
            Some(Shows::Mods) => matches!(item.source, Some(Source::Mod(_))),
            Some(Shows::One(source)) => item.source.as_ref() == Some(source),
        }
    }

    fn matches(&self, item: &Item) -> bool {
        let search = self.search.trim().to_lowercase();
        search.is_empty() || item.name.to_lowercase().contains(&search)
    }

    /// The maps that show, as indices: of the source and place chosen,
    /// that the search leaves.
    fn shown(&self) -> Vec<usize> {
        (0..self.items.len())
            .filter(|&i| {
                let item = &self.items[i];
                self.of_source(item) && self.place.is_none_or(|p| p == item.place) && self.matches(item)
            })
            .collect()
    }
}

/// The project's picture of one of its maps.
fn project_picture(root: &Path, map: &ProjectMap) -> Option<PathBuf> {
    let file = map.map.thumbnail.as_ref()?;
    Some(root.join(&map.file).parent()?.join(file))
}

/// The sources of the dropdown: the project's own maps the game does not
/// list, the packages, Koikatsu and the mods, one by one under Mods when
/// there are several.
fn source_rows(items: &[Item]) -> Vec<SourceRow> {
    let known = rk_packages::index::known();
    let mut sources: Vec<(Option<&Source>, usize)> = Vec::new();
    for item in items {
        match sources.iter_mut().find(|(s, _)| *s == item.source.as_ref()) {
            Some((_, count)) => *count += 1,
            None => sources.push((item.source.as_ref(), 1)),
        }
    }
    let rank = |source: Option<&Source>| match source {
        None => 0,
        Some(Source::Package(_)) => 1,
        Some(Source::Game) => 2,
        Some(Source::Mod(_)) => 3,
    };
    let name = |source: Option<&Source>| source.map_or_else(|| "Project".to_owned(), |s| source_name(s, &known));
    sources.sort_by_key(|(source, _)| (rank(*source), name(*source).to_lowercase()));
    let mut rows = Vec::new();
    for (source, count) in sources.iter().filter(|(s, _)| !matches!(s, Some(Source::Mod(_)))) {
        rows.push(SourceRow {
            label: name(*source),
            depth: 0,
            count: *count,
            shows: source.map_or(Shows::Project, |s| Shows::One(s.clone())),
        });
    }
    let mods: Vec<_> = sources
        .iter()
        .filter(|(s, _)| matches!(s, Some(Source::Mod(_))))
        .collect();
    if mods.len() > 1 {
        rows.push(SourceRow {
            label: "Mods".to_owned(),
            depth: 0,
            count: mods.iter().map(|m| m.1).sum(),
            shows: Shows::Mods,
        });
    }
    for (source, count) in mods.iter() {
        rows.push(SourceRow {
            label: name(*source),
            depth: usize::from(mods.len() > 1),
            count: *count,
            shows: source.map_or(Shows::Project, |s| Shows::One(s.clone())),
        });
    }
    rows
}

/// Time spent loading pictures in one frame.
const BUDGET: Duration = Duration::from_millis(6);

/// The pictures of maps at their times of day, as the game's map list
/// names them, or the project's own picture of a map.
#[derive(Default)]
pub struct Pictures {
    table: Option<HashMap<String, (String, String)>>,
    textures: HashMap<(String, MapTime), Option<TextureHandle>>,
    /// The sidecars read, by file.
    sidecars: HashMap<PathBuf, Option<crate::maps::MapFile>>,
    /// The pass this frame's loading began in, and when.
    pass: u64,
    since: Option<Instant>,
}

impl Pictures {
    /// What the sidecar at `path` says, read once.
    pub fn sidecar(&mut self, path: &Path) -> Option<crate::maps::MapFile> {
        self.sidecars
            .entry(path.to_owned())
            .or_insert_with(|| crate::maps::MapFile::read(path).ok())
            .clone()
    }

    /// Where the game's map list puts each map's picture.
    pub fn table(&mut self, game: &Game) -> &HashMap<String, (String, String)> {
        self.table.get_or_insert_with(|| rk_map::thumbnail::table(game))
    }

    /// The picture of the map whose scene bundle is `bundle` at `time`:
    /// the game's, else `own`, the project's picture. Pictures load a few
    /// a frame unless `eager`; one not loaded yet is `None` and asks for
    /// another frame.
    pub fn get(
        &mut self,
        ctx: &egui::Context,
        game: Option<&Game>,
        bundle: &str,
        time: MapTime,
        own: Option<&Path>,
        eager: bool,
    ) -> Option<TextureHandle> {
        let key = (norm(bundle), time);
        if let Some(texture) = self.textures.get(&key) {
            return texture.clone();
        }
        let pass = ctx.cumulative_pass_nr();
        if self.pass != pass || self.since.is_none() {
            self.pass = pass;
            self.since = Some(Instant::now());
        }
        if !eager && self.since.is_some_and(|s| s.elapsed() > BUDGET) {
            ctx.request_repaint();
            return None;
        }
        let image = game
            .and_then(|game| {
                let listed = self.table(game).get(&key.0)?.clone();
                rk_map::thumbnail::picture_of(game, &listed, time.time_of_day())
            })
            .map(|p| egui::ColorImage::from_rgba_unmultiplied([p.width as usize, p.height as usize], &p.rgba))
            .or_else(|| own.and_then(crate::library::picture));
        let texture = image
            .map(|image| ctx.load_texture(format!("map {} {}", key.0, time.label()), image, crate::library::SHRUNK));
        self.textures.insert(key, texture.clone());
        texture
    }
}

/// The part of a picture a 16:9 frame shows: its middle.
fn wide_uv(size: [usize; 2]) -> Rect {
    let (w, h) = (size[0] as f32, size[1] as f32);
    let want = 16.0 / 9.0;
    if w / h > want {
        let side = (w - h * want) / 2.0 / w;
        Rect::from_min_max(Pos2::new(side, 0.0), Pos2::new(1.0 - side, 1.0))
    } else {
        let side = (h - w / want) / 2.0 / h;
        Rect::from_min_max(Pos2::new(0.0, side), Pos2::new(1.0, 1.0 - side))
    }
}

/// Draws a map's picture into `rect`, or its icon while there is none.
fn draw_picture(ui: &Ui, k: &Look, rect: Rect, texture: Option<&TextureHandle>, tint: Color32) {
    let p = ui.painter();
    p.rect_filled(rect, 0.0, k.t.input);
    match texture {
        Some(texture) => {
            p.image(texture.id(), rect, wide_uv(texture.size()), tint);
        }
        None => glyph(p, k, rect.center(), crate::kinds::Kind::Map.glyph(), 28.0, k.t.border),
    }
}

/// The picture of an item, loaded when it can be.
fn picture(ui: &Ui, st: &mut State, shelf: &Shelf, item: &Item, time: MapTime) -> Option<TextureHandle> {
    st.map_pictures.get(
        ui.ctx(),
        shelf.game.as_deref(),
        &item.bundle,
        time,
        item.picture.as_deref(),
        st.scripted,
    )
}

/// The window: a header with the search, the sources and places at the
/// left, the maps in a grid, and a foot with the chosen map and the
/// buttons.
pub fn show(ctx: &egui::Context, k: &Look, st: &mut State) {
    let Some(mut shelf) = st.map_library.take() else {
        return;
    };
    let t = k.t;
    let reading = shelf.advance(&mut st.map_pictures);
    if reading {
        ctx.request_repaint();
    }
    let typing = ctx.egui_wants_keyboard_input();
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !typing;
    let mut add = false;
    close |= crate::packages::veil(ctx, "map library");
    let window = crate::packages::placed(ctx, Vec2::new(1084.0, 640.0), Vec2::new(720.0, 420.0));
    egui::Area::new(egui::Id::new("map library"))
        .order(egui::Order::Foreground)
        .fixed_pos(window.min)
        .show(ctx, |ui| {
            ui.set_min_size(window.size());
            let title = if shelf.to_scene { "Add map" } else { "Import map" };
            let (frame, closed) = crate::packages::frame(ui, k, window, title, "");
            close |= closed;
            let (head, body, foot) = (frame.head, frame.body, frame.foot);
            let search = Rect::from_min_size(
                Pos2::new(head.right() - 52.0 - 260.0, head.center().y - k.field() / 2.0),
                Vec2::new(260.0, k.field()),
            );
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(search));
            widgets::search(&mut child, k, &mut shelf.search, "Search", 260.0);
            let tree = Rect::from_min_max(body.min, Pos2::new(body.left() + sources::COLUMN, body.bottom()));
            ui.painter().rect_filled(tree, 0.0, t.well);
            hair_v(ui.painter(), tree.right(), tree.top(), tree.bottom(), t.line);
            let grid_rect = Rect::from_min_max(Pos2::new(tree.right() + 1.0, body.top()), body.max);
            if shelf.items.is_empty() {
                let text = if reading { "Reading the maps" } else { "No maps" };
                ui.painter().text(
                    Pos2::new(grid_rect.center().x, grid_rect.top() + 58.0),
                    Align2::CENTER_CENTER,
                    text,
                    k.font(),
                    t.text_dim,
                );
            } else {
                tree_pane(ui, k, &mut shelf, tree);
                add |= grid(ui, k, st, &mut shelf, grid_rect);
            }

            // The foot: the chosen map and what importing it copies, and
            // the buttons.
            let (name, size) = foot_text(&shelf);
            let p = ui.painter();
            let end = p
                .text(
                    Pos2::new(foot.left() + 20.0, foot.center().y),
                    Align2::LEFT_CENTER,
                    &name,
                    k.font(),
                    if shelf.chosen.is_some() { t.text } else { t.text_dim },
                )
                .right();
            p.text(
                Pos2::new(end + 12.0, foot.center().y),
                Align2::LEFT_CENTER,
                &size,
                k.font(),
                t.text_dim,
            );
            let add_rect = Rect::from_min_size(
                Pos2::new(foot.right() - 20.0 - 104.0, foot.center().y - 15.0),
                Vec2::new(104.0, 30.0),
            );
            let cancel_rect = add_rect.translate(Vec2::new(-104.0 - 8.0, 0.0));
            if crate::selector::button(ui, k, cancel_rect, "Cancel", false, true) {
                close = true;
            }
            // A map the project has needs no import.
            let ready = shelf
                .chosen
                .and_then(|i| shelf.items.get(i))
                .is_some_and(|item| shelf.to_scene || item.owned.is_none());
            let caption = if shelf.to_scene { "Add" } else { "Import" };
            if crate::selector::button(ui, k, add_rect, caption, true, ready) {
                add = true;
            }
        });
    if add {
        accept(st, &shelf);
        return;
    }
    if !close {
        st.map_library = Some(shelf);
    }
}

/// The left column: the sources, and the places of the one chosen.
fn tree_pane(ui: &mut Ui, k: &Look, shelf: &mut Shelf, column: Rect) {
    let items: Vec<sources::Item> = shelf
        .rows
        .iter()
        .map(|r| sources::Item {
            label: r.label.clone(),
            depth: r.depth,
            count: Some(r.count),
        })
        .collect();
    let (picked, area) = sources::dropdown(ui, k, column, ui.id().with("map source"), &items, shelf.source);
    if let Some(picked) = picked {
        shelf.source = picked;
        shelf.place = None;
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
    child.set_clip_rect(area);
    child.spacing_mut().item_spacing.y = 0.0;
    child.add_space(6.0);
    let of_source: Vec<&Item> = shelf.items.iter().filter(|i| shelf.of_source(i)).collect();
    let all = sources::Row {
        label: "All maps",
        depth: 0,
        count: Some(of_source.len()),
        on: shelf.place.is_none(),
        open: None,
    };
    if sources::row(&mut child, k, "all maps", all).row {
        shelf.place = None;
    }
    let places: Vec<(Place, usize)> = Place::ALL
        .into_iter()
        .map(|p| (p, of_source.iter().filter(|i| i.place == p).count()))
        .filter(|(_, n)| *n > 0)
        .collect();
    if places.len() > 1 {
        for (place, count) in places {
            let row = sources::Row {
                label: place.label(),
                depth: 0,
                count: Some(count),
                on: shelf.place == Some(place),
                open: None,
            };
            if sources::row(&mut child, k, place.label(), row).row {
                shelf.place = Some(place);
            }
        }
    }
}

/// What showing a map needs to know of how it is drawn.
struct Mark {
    on: bool,
    hovered: bool,
}

/// The outline and badge of a chosen or hovered picture.
fn outline(ui: &Ui, k: &Look, picture: Rect, mark: &Mark) {
    let p = ui.painter();
    let stroke = if mark.on {
        Some(Stroke::new(2.0, k.t.select))
    } else if mark.hovered {
        Some(Stroke::new(1.0, k.t.hover))
    } else {
        None
    };
    if let Some(stroke) = stroke {
        p.rect_stroke(picture, 0.0, stroke, StrokeKind::Outside);
    }
    if mark.on {
        let badge = Pos2::new(picture.right() - 13.0, picture.top() + 13.0);
        p.circle_filled(badge, 9.0, k.t.select);
        glyph(p, k, badge, icon::CHECK, 11.0, Color32::WHITE);
    }
}

/// A map as a tile: its picture, its name and, for the project's own, a
/// mark. Returns whether it was double clicked.
fn tile(ui: &mut Ui, k: &Look, st: &mut State, shelf: &mut Shelf, index: usize, rect: Rect) -> bool {
    let t = &k.t;
    let item = shelf.items[index].clone();
    let picture_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), (rect.width() * 9.0 / 16.0).round()));
    let resp = ui.interact(rect, ui.id().with(("map", &item.bundle)), Sense::click());
    if resp.clicked() {
        shelf.chosen = if shelf.chosen == Some(index) { None } else { Some(index) };
    }
    let double = resp.double_clicked();
    if double {
        shelf.chosen = Some(index);
    }
    let on = shelf.chosen == Some(index);
    let texture = if ui.is_rect_visible(rect) {
        picture(ui, st, shelf, &item, item.first_time())
    } else {
        None
    };
    let tint = if on || resp.hovered() {
        Color32::WHITE
    } else {
        Color32::from_gray(222)
    };
    draw_picture(ui, k, picture_rect, texture.as_ref(), tint);
    outline(
        ui,
        k,
        picture_rect,
        &Mark {
            on,
            hovered: resp.hovered(),
        },
    );
    crate::selector::line(
        ui,
        Pos2::new(rect.left(), picture_rect.bottom() + 13.0),
        rect.width(),
        &item.name,
        k.font(),
        if on { t.text_bright } else { t.text },
    );
    if item.owned.is_some() {
        crate::selector::line(
            ui,
            Pos2::new(rect.left(), picture_rect.bottom() + 30.0),
            rect.width(),
            "In project",
            k.small(),
            t.accent,
        );
    }
    resp.on_hover_text(&item.name);
    double
}

/// The maps of the source and place chosen as a grid of tiles; while
/// all places show, under a caption for each place.
fn grid(ui: &mut Ui, k: &Look, st: &mut State, shelf: &mut Shelf, body: Rect) -> bool {
    let t = &k.t;
    let shown = shelf.shown();
    if shown.is_empty() {
        ui.painter().text(
            Pos2::new(body.center().x, body.top() + 58.0),
            Align2::CENTER_CENTER,
            "No map matches the search",
            k.font(),
            t.text_dim,
        );
        return false;
    }
    const GAP: f32 = 14.0;
    let inner = body.shrink2(Vec2::new(20.0, 0.0));
    let want = 176.0;
    let columns = ((inner.width() + GAP) / (want + GAP)).floor().max(1.0) as usize;
    let width = ((inner.width() - (columns - 1) as f32 * GAP) / columns as f32).floor();
    let cell = Vec2::new(width, (width * 9.0 / 16.0).round() + 42.0);
    let used = columns as f32 * cell.x + (columns - 1) as f32 * GAP;
    let left = inner.left() + ((inner.width() - used) / 2.0).floor();
    // The groups, each a caption and its tiles; one group without a
    // caption for a place, or for maps that all stand in one.
    let mut groups: Vec<(Option<Place>, Vec<usize>)> = if shelf.place.is_none() {
        Place::ALL
            .into_iter()
            .map(|p| {
                (
                    Some(p),
                    shown.iter().copied().filter(|&i| shelf.items[i].place == p).collect(),
                )
            })
            .filter(|(_, items): &(Option<Place>, Vec<usize>)| !items.is_empty())
            .collect()
    } else {
        vec![(None, shown)]
    };
    if groups.len() == 1 {
        groups[0].0 = None;
    }
    let grouped = groups[0].0.is_some();
    const CAPTION: f32 = 34.0;
    let height: f32 = 18.0
        + groups
            .iter()
            .map(|(place, items)| {
                let caption = if place.is_some() { CAPTION } else { 0.0 };
                caption + items.len().div_ceil(columns) as f32 * (cell.y + GAP)
            })
            .sum::<f32>();
    let mut double = false;
    let mut area = ui.new_child(egui::UiBuilder::new().max_rect(body));
    area.set_clip_rect(body);
    let scroll = egui::ScrollArea::vertical()
        .id_salt(("map tiles", shelf.source, shelf.place.map(Place::label)))
        .auto_shrink(false)
        .max_height(body.height());
    let scroll = match st.map_scroll {
        Some(offset) => scroll.vertical_scroll_offset(offset),
        None => scroll,
    };
    scroll.show(&mut area, |ui| {
        let (space, _) = ui.allocate_exact_size(Vec2::new(body.width(), height), Sense::hover());
        let mut y = space.top() + if grouped { 6.0 } else { 18.0 };
        for (place, items) in &groups {
            if let Some(place) = place {
                ui.painter().text(
                    Pos2::new(left, y + CAPTION / 2.0 + 2.0),
                    Align2::LEFT_CENTER,
                    place.label().to_uppercase(),
                    k.small(),
                    t.text_dim,
                );
                y += CAPTION;
            }
            for (n, &index) in items.iter().enumerate() {
                let min = Pos2::new(
                    left + (n % columns) as f32 * (cell.x + GAP),
                    y + (n / columns) as f32 * (cell.y + GAP),
                );
                double |= tile(ui, k, st, shelf, index, Rect::from_min_size(min, cell));
            }
            y += items.len().div_ceil(columns) as f32 * (cell.y + GAP);
        }
    });
    double
}

/// What the foot says of the chosen map: its name and, for a map to
/// import, what importing copies.
fn foot_text(shelf: &Shelf) -> (String, String) {
    match shelf.chosen.and_then(|i| shelf.items.get(i)) {
        None => ("No map chosen".to_owned(), String::new()),
        Some(item) => {
            let size = match (&item.owned, item.size) {
                (None, Some(bytes)) => crate::mods::size_text(bytes),
                _ => String::new(),
            };
            (item.name.clone(), size)
        }
    }
}

/// Puts the chosen map in the scene, the project's own as it is and
/// another imported first; or, opened to import, imports it alone.
fn accept(st: &mut State, shelf: &Shelf) {
    let Some(item) = shelf.chosen.and_then(|i| shelf.items.get(i)) else {
        return;
    };
    match (&item.owned, shelf.to_scene) {
        (Some(file), true) => st.add_map(PathBuf::from(file)),
        (Some(_), false) => {}
        (None, _) => {
            if let Some(entry) = item.entry.clone() {
                st.import_map(entry, shelf.to_scene);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_stand_in_their_places() {
        assert_eq!(place_of("map/scene/rooftop.unity3d", false), Place::School);
        assert_eq!(place_of("map/scene/thebeach.unity3d", true), Place::Outside);
        assert_eq!(place_of("map/scene/myroom.unity3d", false), Place::Other);
        assert_eq!(place_of("studio/map/bushitu.unity3d", false), Place::School);
        assert_eq!(place_of("map/someone/else.unity3d", true), Place::Mods);
        assert_eq!(place_of("map/someone/else.unity3d", false), Place::Other);
    }

    #[test]
    fn a_wide_picture_shows_its_middle() {
        let uv = wide_uv([320, 180]);
        assert!((uv.min.x).abs() < 1e-6 && (uv.max.x - 1.0).abs() < 1e-6);
        let square = wide_uv([100, 100]);
        assert!((square.height() - 0.5625).abs() < 1e-4);
    }
}
