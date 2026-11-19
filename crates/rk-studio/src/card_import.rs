// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The window that imports characters into the project: the cards of one
//! source at a time, an installed package or the user's Koikatsu, with
//! the source's folders as a tree at the left and its cards in a grid.
//!
//! A source is read on a thread of its own: its folders and files first,
//! then the characters' names, so that a game folder of thousands of
//! cards opens at once. Pictures load as their tiles come into view.

use crate::dialogs::Import;
use crate::sources;
use crate::state::{Look, State};
use crate::widgets::{self, glyph, hair_v};
use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, Ui, Vec2};
use egui_phosphor::regular as icon;
use rk_packages::Installed;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

/// A folder with cards in it or below it, as a row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub depth: usize,
    pub label: String,
    /// What it is folded and unfolded by: its path.
    pub key: String,
    /// The cards in it and below it.
    pub count: usize,
    /// One past the last folder below it, in the list of folders.
    pub end: usize,
}

/// A card of a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    /// The character's name, or the file's until the name is read and
    /// where the card has none.
    pub name: String,
    /// The folder it is in.
    pub folder: usize,
    /// The character is imported under the name shown here, which tells
    /// the game's own namesakes apart, instead of the card's own.
    pub renamed: bool,
    /// The card is another game's, named here, and cannot be imported.
    pub foreign: Option<&'static str>,
}

/// The folders of a source, parents before their children, and its cards.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub folders: Vec<Folder>,
    pub cards: Vec<Found>,
}

/// Folders deeper than this are not looked into, should links go round.
const DEEPEST: usize = 12;

/// The cards and the folders right in `dir`, each by name.
fn in_dir(dir: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut cards, mut dirs) = (Vec::new(), Vec::new());
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
            cards.push(path);
        }
    }
    let by_name = |p: &PathBuf| p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
    cards.sort_by_key(by_name);
    dirs.sort_by_key(by_name);
    (cards, dirs)
}

fn stem(path: &Path) -> String {
    path.file_stem().unwrap_or_default().to_string_lossy().into_owned()
}

impl Listing {
    /// Adds `dir` as a folder called `label`, with the folders in it
    /// below it. A folder without a card anywhere in it is left out.
    /// Returns how many cards it brought.
    pub fn add_folder(&mut self, dir: &Path, label: &str, depth: usize) -> usize {
        let (cards, dirs) = in_dir(dir);
        let at = self.folders.len();
        self.folders.push(Folder {
            depth,
            label: label.to_owned(),
            key: dir.to_string_lossy().into_owned(),
            count: 0,
            end: 0,
        });
        let mut count = cards.len();
        self.cards.extend(cards.into_iter().map(|path| Found {
            name: stem(&path),
            path,
            folder: at,
            renamed: false,
            foreign: None,
        }));
        if depth < DEEPEST {
            for sub in dirs {
                let label = sub.file_name().unwrap_or_default().to_string_lossy().into_owned();
                count += self.add_folder(&sub, &label, depth + 1);
            }
        }
        if count == 0 {
            self.folders.truncate(at);
        } else {
            self.folders[at].count = count;
            self.folders[at].end = self.folders.len();
        }
        count
    }

    /// Adds another listing after this one.
    pub fn append(&mut self, other: Listing) {
        let by = self.folders.len();
        self.folders
            .extend(other.folders.into_iter().map(|f| Folder { end: f.end + by, ..f }));
        self.cards.extend(other.cards.into_iter().map(|c| Found {
            folder: c.folder + by,
            ..c
        }));
    }

    /// The folders that show as rows: those whose parents are unfolded.
    pub fn rows(&self, unfolded: &HashSet<String>) -> Vec<usize> {
        let mut rows = Vec::new();
        let mut i = 0;
        while i < self.folders.len() {
            rows.push(i);
            let folder = &self.folders[i];
            i = if unfolded.contains(&folder.key) {
                i + 1
            } else {
                folder.end.max(i + 1)
            };
        }
        rows
    }

    /// The cards that show, as indices: those in `folder` and below it,
    /// or all, that the search leaves. The search looks at the names and
    /// at the files' names.
    pub fn matching(&self, folder: Option<usize>, search: &str) -> Vec<usize> {
        let search = search.trim().to_lowercase();
        let range = match folder.and_then(|f| self.folders.get(f).map(|folder| f..folder.end)) {
            Some(range) => range,
            None => 0..self.folders.len(),
        };
        (0..self.cards.len())
            .filter(|&i| {
                let card = &self.cards[i];
                range.contains(&card.folder)
                    && (search.is_empty()
                        || card.name.to_lowercase().contains(&search)
                        || stem(&card.path).to_lowercase().contains(&search))
            })
            .collect()
    }

    /// Names the cards after their characters; namesakes and nameless
    /// cards keep their files' names, as the game's extras all carry the
    /// default character's name.
    fn name_all(&mut self) {
        let names: Vec<rk_card::Result<String>> = self.cards.iter().map(|c| rk_card::Card::name_of(&c.path)).collect();
        let read: Vec<(String, Option<String>)> = self
            .cards
            .iter()
            .zip(&names)
            .map(|(c, name)| (stem(&c.path), name.as_ref().ok().cloned()))
            .collect();
        for ((card, name), read) in self
            .cards
            .iter_mut()
            .zip(crate::packages::card_names(&read))
            .zip(&names)
        {
            card.name = name;
            card.renamed = true;
            card.foreign = foreign(read);
        }
    }
}

/// Where a source's cards are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// An installed package's folder.
    Package(PathBuf),
    /// The user's Koikatsu.
    Game(PathBuf),
}

/// What the thread that reads a source sends.
enum Message {
    /// The folders and cards, to show at once.
    Listed(Listing),
    /// A card's name, by its place in the listing.
    Name(usize, String),
    /// A card that is another game's, by its place in the listing.
    Foreign(usize, &'static str),
    /// Folders and cards that took longer, to go after the others.
    More(Listing),
}

/// A package's cards: the folders of its `cards` folder, `female` and
/// `male`, as the folders of the tree.
pub fn package_listing(dir: &Path) -> Listing {
    let cards = dir.join(rk_packages::manifest::folder::CARDS);
    let mut listing = Listing::default();
    for sub in in_dir(&cards).1 {
        let name = sub.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let mut label = name.clone();
        if let Some(first) = label.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        listing.add_folder(&sub, &label, 0);
    }
    listing
}

/// The user's own cards in a Koikatsu folder: the game's `female` and
/// `male` folders, with the folders the user keeps cards in below them.
pub fn game_listing(game: &Path) -> Listing {
    let chara = game.join("UserData").join("chara");
    let mut listing = Listing::default();
    listing.add_folder(&chara.join("female"), "Female", 0);
    listing.add_folder(&chara.join("male"), "Male", 0);
    listing
}

/// The cards a bundle of the game holds as text assets, by asset name.
fn bundle_cards(path: &Path) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    use rk_unityfs::{class, Bundle, SerializedFile};
    let bundle = Bundle::open(path)?;
    let mut cards = Vec::new();
    for node in bundle.nodes.iter().filter(|n| n.is_serialized()) {
        let file = SerializedFile::parse(bundle.read_node(node)?)?;
        for object in file.objects_of(class::TEXT_ASSET) {
            let value = file.read(object)?;
            let (Some(name), Some(bytes)) = (value.str("m_Name"), value.bytes("m_Script")) else {
                continue;
            };
            if rk_card::Card::parse(bytes).is_ok() {
                cards.push((name.to_owned(), bytes.to_vec()));
            }
        }
    }
    Ok(cards)
}

/// A file name for a card of the game's bundles, from its asset's name:
/// `ill_Default_Female` is `Default female.png`.
pub fn default_file(asset: &str) -> String {
    let asset = asset.strip_prefix("ill_").unwrap_or(asset);
    let mut clean: String = asset
        .chars()
        .map(|c| match c {
            c if c.is_ascii_alphanumeric() || c == '-' => c.to_ascii_lowercase(),
            _ => ' ',
        })
        .collect();
    if let Some(first) = clean.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    format!("{clean}.png")
}

/// The game's own characters, which it keeps in bundles: the maker's
/// defaults, the story's and the extras. They are written out as cards
/// into `out`, to be listed and imported like any other.
fn default_listing(game: &Path, out: &Path) -> Listing {
    let abdata = game.join("abdata");
    let mut kept: HashSet<PathBuf> = HashSet::new();
    for (folder, prefix, sub) in [
        ("custom", "presets_", ""),
        ("action/fixchara", "", "Story"),
        ("action/mobchara", "", "Extras"),
    ] {
        let mut bundles = Vec::new();
        rk_core::walk_files(&abdata.join(folder), "unity3d", &mut bundles);
        bundles.sort();
        let to = out.join(sub);
        for bundle in bundles.iter().filter(|b| stem(b).starts_with(prefix)) {
            for (asset, bytes) in bundle_cards(bundle).unwrap_or_default() {
                let file = to.join(default_file(&asset));
                let same = file.metadata().is_ok_and(|m| m.len() == bytes.len() as u64);
                if same || (std::fs::create_dir_all(&to).is_ok() && std::fs::write(&file, &bytes).is_ok()) {
                    kept.insert(file);
                }
            }
        }
    }
    // Cards of another game, written out earlier, go.
    let mut old = Vec::new();
    rk_core::walk_files(out, "png", &mut old);
    for file in old.into_iter().filter(|f| !kept.contains(f)) {
        let _ = std::fs::remove_file(file);
    }
    let mut listing = Listing::default();
    listing.add_folder(out, "Default", 0);
    listing.name_all();
    listing
}

/// The game a card is of, when reading it found it to be another's than
/// Koikatsu.
fn foreign(read: &rk_card::Result<String>) -> Option<&'static str> {
    match read {
        Err(rk_card::CardError::OtherGame(game)) => Some(game),
        _ => None,
    }
}

/// Reads a source and sends what it finds. It stops when nobody listens
/// any more.
fn read(place: Place, send: Sender<Message>) {
    match place {
        Place::Package(dir) => {
            let mut listing = package_listing(&dir);
            listing.name_all();
            let _ = send.send(Message::Listed(listing));
        }
        Place::Game(game) => {
            let listing = game_listing(&game);
            let paths: Vec<PathBuf> = listing.cards.iter().map(|c| c.path.clone()).collect();
            if send.send(Message::Listed(listing)).is_err() {
                return;
            }
            // A folder for each game, should the user have several.
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&game, &mut hasher);
            let out = std::env::temp_dir().join(format!(
                "renkatsu-game-cards-{:016x}",
                std::hash::Hasher::finish(&hasher)
            ));
            if send.send(Message::More(default_listing(&game, &out))).is_err() {
                return;
            }
            for (i, path) in paths.iter().enumerate() {
                let read = rk_card::Card::name_of(path);
                let message = match (&read, foreign(&read)) {
                    (_, Some(game)) => Message::Foreign(i, game),
                    (Ok(name), None) if !name.is_empty() => Message::Name(i, name.clone()),
                    _ => continue,
                };
                if send.send(message).is_err() {
                    return;
                }
            }
        }
    }
}

/// Something to import characters from.
pub struct Source {
    pub name: String,
    place: Place,
    pub listing: Listing,
    /// The thread that reads it, while it does.
    incoming: Option<Receiver<Message>>,
    started: bool,
    /// How many names arrived since the listing.
    named: usize,
    /// The folder that shows; all of them when `None`.
    pub folder: Option<usize>,
    /// The folders that show the folders in them, by key.
    unfolded: HashSet<String>,
}

impl Source {
    fn new(name: String, place: Place) -> Source {
        Source {
            name,
            place,
            listing: Listing::default(),
            incoming: None,
            started: false,
            named: 0,
            folder: None,
            unfolded: HashSet::new(),
        }
    }

    fn take(&mut self, message: Message) {
        match message {
            Message::Listed(listing) => {
                self.listing = listing;
                // The game's folders are many: it opens on the first.
                if let (Place::Game(_), Some(first)) = (&self.place, self.listing.folders.first()) {
                    self.folder = Some(0);
                    self.unfolded.insert(first.key.clone());
                }
            }
            Message::Name(i, name) => {
                if let Some(card) = self.listing.cards.get_mut(i) {
                    card.name = name;
                    self.named += 1;
                }
            }
            Message::Foreign(i, game) => {
                if let Some(card) = self.listing.cards.get_mut(i) {
                    card.foreign = Some(game);
                    self.named += 1;
                }
            }
            Message::More(listing) => self.listing.append(listing),
        }
    }

    /// Starts reading the source when that is not done yet, and takes
    /// what was read since the last frame; all of it, waiting, when
    /// `wait` is set. Returns whether more is to come.
    pub fn advance(&mut self, wait: bool) -> bool {
        if !self.started {
            self.started = true;
            let (send, receive) = channel();
            let place = self.place.clone();
            let spawned = std::thread::Builder::new()
                .name("character source".into())
                .spawn(move || read(place, send));
            self.incoming = spawned.is_ok().then_some(receive);
        }
        let Some(incoming) = self.incoming.take() else {
            return false;
        };
        if wait {
            while let Ok(message) = incoming.recv() {
                self.take(message);
            }
            return false;
        }
        // No more than this a frame, so the frame stays short.
        for _ in 0..512 {
            match incoming.try_recv() {
                Ok(message) => self.take(message),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
        self.incoming = Some(incoming);
        true
    }
}

/// The sources there are: the installed packages that bring cards, each
/// under the name the Packages window shows it by, and Koikatsu when its
/// folder is set.
pub fn sources(packages: &[Installed], game: Option<&rk_core::Config>) -> Vec<Source> {
    let known = rk_packages::index::known();
    let mut sources: Vec<Source> = packages
        .iter()
        .filter(|p| !p.cards().is_empty())
        .map(|p| {
            let name = known
                .iter()
                .find(|k| k.id == p.manifest.id)
                .map_or_else(|| p.manifest.name.clone(), |k| k.name.clone());
            Source::new(name, Place::Package(p.dir.clone()))
        })
        .collect();
    if let Some(game) = game {
        sources.push(Source::new("Koikatsu".to_owned(), Place::Game(game.game_dir.clone())));
    }
    sources
}

/// What the grid depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ViewKey {
    source: usize,
    folder: Option<usize>,
    search: String,
    cards: usize,
    named: usize,
}

/// How many more names have to arrive before a search is run again.
const NAMED_STEP: usize = 128;

/// Time spent loading pictures in one frame.
const BUDGET: Duration = Duration::from_millis(6);

/// Pictures kept at most; past it, the ones out of view go.
const PICTURES: usize = 400;

/// The window that imports characters.
pub struct Browser {
    pub sources: Vec<Source>,
    /// The source that shows.
    pub source: usize,
    pub search: String,
    /// The chosen cards, with the names they are imported under where
    /// those are not the cards' own.
    pub chosen: BTreeMap<PathBuf, Option<String>>,
    /// Pictures by card, with the frame they last showed in; `None` for
    /// a card whose picture did not load.
    thumbs: HashMap<PathBuf, (Option<TextureHandle>, u64)>,
    frame: u64,
    /// The cards of the grid as last worked out.
    view: Option<(ViewKey, Vec<usize>)>,
}

impl Browser {
    pub fn new(packages: &[Installed], game: Option<&rk_core::Config>) -> Browser {
        Browser {
            sources: sources(packages, game),
            source: 0,
            search: String::new(),
            chosen: BTreeMap::new(),
            thumbs: HashMap::new(),
            frame: 0,
            view: None,
        }
    }

    /// The cards of the grid, worked out again only when the source, the
    /// folder, the search or what is read changed.
    fn shown(&mut self) -> Vec<usize> {
        let Some(source) = self.sources.get(self.source) else {
            return Vec::new();
        };
        let key = ViewKey {
            source: self.source,
            folder: source.folder,
            search: self.search.clone(),
            cards: source.listing.cards.len(),
            // Names matter to a search alone, and not each of them.
            named: match (self.search.trim().is_empty(), &source.incoming) {
                (true, _) => 0,
                (false, None) => usize::MAX,
                (false, Some(_)) => source.named / NAMED_STEP,
            },
        };
        match &self.view {
            Some((old, shown)) if *old == key => shown.clone(),
            _ => {
                let shown = source.listing.matching(source.folder, &self.search);
                self.view = Some((key, shown.clone()));
                shown
            }
        }
    }
}

/// The window, drawn like the one that imports animations: a header with
/// the search, the sources and folders at the left, the cards in a grid,
/// and a foot with what is chosen and the buttons.
pub fn show(ctx: &egui::Context, k: &Look, st: &mut State) {
    let Some(mut w) = st.packages.browser.take() else {
        return;
    };
    let t = k.t;
    w.frame += 1;
    let reading = match w.sources.get_mut(w.source) {
        Some(source) => source.advance(st.scripted),
        None => false,
    };
    if reading {
        ctx.request_repaint();
    }
    let typing = ctx.egui_wants_keyboard_input();
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !typing;
    let mut import = false;
    let mut to_packages = false;
    close |= crate::packages::veil(ctx, "character importer");
    let window = crate::packages::placed(ctx, Vec2::new(1084.0, 640.0), Vec2::new(720.0, 420.0));
    egui::Area::new(egui::Id::new("character importer"))
        .order(egui::Order::Foreground)
        .fixed_pos(window.min)
        .show(ctx, |ui| {
            ui.set_min_size(window.size());
            let (frame, closed) = crate::packages::frame(ui, k, window, "Import characters", "");
            close |= closed;
            let (head, body, foot) = (frame.head, frame.body, frame.foot);
            if w.sources.is_empty() {
                let c = body.center();
                ui.painter().text(
                    c - Vec2::new(0.0, 22.0),
                    Align2::CENTER_CENTER,
                    "No installed package has characters",
                    k.bold(k.d.text),
                    t.text_bright,
                );
                let open = Rect::from_center_size(c + Vec2::new(0.0, 14.0), Vec2::new(150.0, 30.0));
                if crate::selector::button(ui, k, open, "Open Packages", true, true) {
                    to_packages = true;
                }
            } else {
                let search = Rect::from_min_size(
                    Pos2::new(head.right() - 52.0 - 260.0, head.center().y - k.field() / 2.0),
                    Vec2::new(260.0, k.field()),
                );
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(search));
                widgets::search(&mut child, k, &mut w.search, "Search", 260.0);
                let tree = Rect::from_min_max(body.min, Pos2::new(body.left() + sources::COLUMN, body.bottom()));
                let grid = Rect::from_min_max(tree.right_top(), body.max);
                ui.painter().rect_filled(tree, 0.0, t.well);
                hair_v(ui.painter(), tree.right(), tree.top(), tree.bottom(), t.line);
                tree_pane(ui, k, &mut w, tree);
                import |= grid_pane(ui, k, &mut w, grid.with_min_x(grid.left() + 1.0), reading, st.scripted);
            }

            // The foot: what is chosen, and the buttons.
            let n = w.chosen.len();
            let text = match n {
                0 if w.sources.is_empty() => String::new(),
                0 => "No character chosen".to_owned(),
                1 => "1 character chosen".to_owned(),
                n => format!("{n} characters chosen"),
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
    if import && !w.chosen.is_empty() {
        // Copied into the project as a card from disk is, with the
        // question about the mods it needs.
        for (path, name) in std::mem::take(&mut w.chosen) {
            st.imports.push_back(Import {
                path,
                place: false,
                ask: true,
                name,
                first: false,
            });
        }
        return;
    }
    if !close {
        st.packages.browser = Some(w);
    }
}

/// The left column: where the cards come from, and that source's
/// folders.
fn tree_pane(ui: &mut Ui, k: &Look, w: &mut Browser, column: Rect) {
    let items: Vec<sources::Item> = w
        .sources
        .iter()
        .map(|s| sources::Item {
            label: s.name.clone(),
            depth: 0,
            count: None,
        })
        .collect();
    let (picked, area) = sources::dropdown(ui, k, column, ui.id().with("character source"), &items, w.source);
    if let Some(picked) = picked {
        w.source = picked;
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
    child.set_clip_rect(area);
    egui::ScrollArea::vertical()
        .id_salt("character folders")
        .auto_shrink(false)
        .max_height(area.height())
        .show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(6.0);
            let Some(source) = w.sources.get_mut(w.source) else {
                return;
            };
            let all = sources::Row {
                label: "All characters",
                depth: 0,
                count: Some(source.listing.cards.len()),
                on: source.folder.is_none(),
                open: None,
            };
            if sources::row(ui, k, "all characters", all).row {
                source.folder = None;
            }
            for i in source.listing.rows(&source.unfolded) {
                let folder = &source.listing.folders[i];
                let parent = folder.end > i + 1;
                let of = sources::Row {
                    label: &folder.label,
                    depth: folder.depth,
                    count: Some(folder.count),
                    on: source.folder == Some(i),
                    open: parent.then(|| source.unfolded.contains(&folder.key)),
                };
                let clicked = sources::row(ui, k, &folder.key, of);
                if clicked.row {
                    source.folder = Some(i);
                }
                // Choosing a folder opens it; choosing it again folds it.
                if (clicked.fold || (clicked.row && parent)) && !source.unfolded.remove(&folder.key) {
                    source.unfolded.insert(folder.key.clone());
                }
            }
            ui.add_space(8.0);
        });
}

/// The cards of the folder that shows, evenly spaced in as many columns
/// as fit. Returns whether a card was double clicked, to import it at
/// once.
fn grid_pane(ui: &mut Ui, k: &Look, w: &mut Browser, body: Rect, reading: bool, eager: bool) -> bool {
    let t = &k.t;
    let shown = w.shown();
    let Some(source) = w.sources.get(w.source) else {
        return false;
    };
    let listing = &source.listing;
    if shown.is_empty() {
        let text = match (listing.cards.is_empty(), reading) {
            (true, true) => "Reading the characters",
            (true, false) => "No characters",
            (false, _) => "No character matches the search",
        };
        ui.painter().text(
            Pos2::new(body.center().x, body.top() + 58.0),
            Align2::CENTER_CENTER,
            text,
            k.font(),
            t.text_dim,
        );
        return false;
    }
    const GAP: f32 = 14.0;
    let thumb = Vec2::new(116.0, (116.0 * 352.0 / 252.0_f32).round());
    let tile = Vec2::new(thumb.x, thumb.y + 42.0);
    let inner = body.shrink2(Vec2::new(20.0, 0.0));
    let columns = ((inner.width() + GAP) / (tile.x + GAP)).floor().max(1.0) as usize;
    let used = columns as f32 * tile.x + (columns - 1) as f32 * GAP;
    let left = inner.left() + ((inner.width() - used) / 2.0).floor();
    let rows = shown.len().div_ceil(columns);
    let step = tile.y + GAP;
    let mut import = false;
    let since = Instant::now();
    let frame = w.frame;
    let mut area = ui.new_child(egui::UiBuilder::new().max_rect(body));
    area.set_clip_rect(body);
    egui::ScrollArea::vertical()
        // Each source and folder scrolls from its own top.
        .id_salt(("character cards", w.source, source.folder))
        .auto_shrink(false)
        .max_height(body.height())
        .show_viewport(&mut area, |ui, view| {
            let height = 18.0 + rows as f32 * step;
            let (space, _) = ui.allocate_exact_size(Vec2::new(body.width(), height), Sense::hover());
            // Only the rows in view are drawn.
            let first = ((view.top() - 18.0) / step).floor().max(0.0) as usize;
            let last = (((view.bottom() - 18.0) / step).ceil().max(0.0) as usize).min(rows);
            let in_view = shown.iter().enumerate().take(last * columns).skip(first * columns);
            for (n, &index) in in_view {
                let card = &listing.cards[index];
                let min = Pos2::new(
                    left + (n % columns) as f32 * (tile.x + GAP),
                    space.top() + 18.0 + (n / columns) as f32 * step,
                );
                let rect = Rect::from_min_size(min, tile);
                // Another game's card shows, dimmed, but cannot be chosen.
                let sense = if card.foreign.is_some() {
                    Sense::hover()
                } else {
                    Sense::click()
                };
                let resp = ui.interact(rect, ui.id().with(("card", &card.path)), sense);
                let name = card.renamed.then(|| card.name.clone());
                if resp.clicked() && w.chosen.remove(&card.path).is_none() {
                    w.chosen.insert(card.path.clone(), name.clone());
                }
                if resp.double_clicked() {
                    w.chosen.insert(card.path.clone(), name);
                    import = true;
                }
                // Pictures load a few a frame, so the window keeps its pace.
                match w.thumbs.get_mut(&card.path) {
                    Some(thumb) => thumb.1 = frame,
                    None if eager || since.elapsed() < BUDGET => {
                        let texture = crate::library::picture(&card.path).map(|image| {
                            ui.ctx().load_texture(
                                format!("card {}", card.path.display()),
                                image,
                                egui::TextureOptions::LINEAR,
                            )
                        });
                        w.thumbs.insert(card.path.clone(), (texture, frame));
                    }
                    None => ui.ctx().request_repaint(),
                }
                let on = w.chosen.contains_key(&card.path);
                let picture = Rect::from_min_size(rect.min, thumb);
                let p = ui.painter();
                p.rect_filled(picture, 0.0, t.input);
                match w.thumbs.get(&card.path) {
                    Some((Some(texture), _)) => {
                        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                        let tint = if card.foreign.is_some() {
                            Color32::from_gray(90)
                        } else if on || resp.hovered() {
                            Color32::WHITE
                        } else {
                            Color32::from_gray(222)
                        };
                        p.image(texture.id(), picture, uv, tint);
                    }
                    _ => glyph(p, k, picture.center(), icon::USER, 32.0, t.border),
                }
                let outline = if on {
                    Some(Stroke::new(2.0, t.select))
                } else if resp.hovered() {
                    Some(Stroke::new(1.0, t.hover))
                } else {
                    None
                };
                if let Some(stroke) = outline {
                    p.rect_stroke(picture, 0.0, stroke, StrokeKind::Outside);
                }
                if on {
                    let badge = Pos2::new(picture.right() - 13.0, picture.top() + 13.0);
                    p.circle_filled(badge, 9.0, t.select);
                    glyph(p, k, badge, icon::CHECK, 11.0, Color32::WHITE);
                }
                crate::selector::line(
                    ui,
                    Pos2::new(rect.left(), picture.bottom() + 13.0),
                    tile.x,
                    &card.name,
                    k.font(),
                    if on { t.text_bright } else { t.text },
                );
                // Where it is: its folder when that is below the one that
                // shows, else the game's or the package's folder above it.
                let own = (source.folder != Some(card.folder)).then_some(card.folder);
                let top = || {
                    (0..=card.folder)
                        .rev()
                        .find(|&i| listing.folders.get(i).is_some_and(|f| f.depth == 0))
                };
                let folder = own
                    .or_else(top)
                    .and_then(|i| listing.folders.get(i))
                    .map_or("", |f| f.label.as_str());
                let folder = card.foreign.unwrap_or(folder);
                crate::selector::line(
                    ui,
                    Pos2::new(rect.left(), picture.bottom() + 30.0),
                    tile.x,
                    folder,
                    k.small(),
                    t.text_dim,
                );
                match card.foreign {
                    Some(game) => resp.on_hover_text(format!("A card of {game}, not of Koikatsu")),
                    None => resp.on_hover_text(&card.name),
                };
            }
        });
    // A long folder scrolled through leaves its pictures behind.
    if w.thumbs.len() > PICTURES {
        w.thumbs.retain(|_, thumb| thumb.1 == frame);
    }
    import
}

#[cfg(test)]
mod tests {

    #[test]
    fn another_games_card_is_named_by_its_game() {
        assert_eq!(
            foreign(&Err(rk_card::CardError::OtherGame("Artificial Academy 2"))),
            Some("Artificial Academy 2")
        );
        assert_eq!(foreign(&Ok("Hu Tao".to_owned())), None);
        assert_eq!(foreign(&Err(rk_card::CardError::NoTrailer)), None);
    }
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("renkatsu-card-import-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(file: &Path) {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, b"not a card").unwrap();
    }

    /// A Koikatsu folder with cards as a user keeps them.
    fn game(name: &str) -> PathBuf {
        let game = temp(name);
        let chara = game.join("UserData/chara");
        for file in [
            "female/Simple.png",
            "female/bookish.PNG",
            "female/notes.txt",
            "female/[Mihnea8848]/Koikatsu Girls Academy/Aoi.png",
            "female/[Mihnea8848]/Koikatsu Girls Academy/Rin.png",
            "female/[Mihnea8848]/Sketches/Draft.png",
            "female/Downloaded/Yuki.png",
            "female/Empty/readme.txt",
            "male/Player.png",
        ] {
            write(&chara.join(file));
        }
        std::fs::create_dir_all(game.join("abdata")).unwrap();
        game
    }

    fn folders(listing: &Listing, rows: &[usize]) -> Vec<(usize, String, usize)> {
        rows.iter()
            .map(|&i| &listing.folders[i])
            .map(|f| (f.depth, f.label.clone(), f.count))
            .collect()
    }

    fn names(listing: &Listing, shown: &[usize]) -> Vec<String> {
        shown.iter().map(|&i| listing.cards[i].name.clone()).collect()
    }

    #[test]
    fn the_games_card_folders_are_the_tree() {
        let game = game("tree");
        let listing = game_listing(&game);
        let row = |depth: usize, label: &str, count: usize| (depth, label.to_owned(), count);
        // Folded, the two folders of the game alone; a folder without
        // cards is not a row, nor is a file that is no card.
        let mut unfolded = HashSet::new();
        assert_eq!(
            folders(&listing, &listing.rows(&unfolded)),
            [row(0, "Female", 6), row(0, "Male", 1)]
        );
        unfolded.insert(listing.folders[0].key.clone());
        assert_eq!(
            folders(&listing, &listing.rows(&unfolded)),
            [
                row(0, "Female", 6),
                row(1, "[Mihnea8848]", 3),
                row(1, "Downloaded", 1),
                row(0, "Male", 1)
            ]
        );
        unfolded.insert(listing.folders[1].key.clone());
        assert_eq!(
            folders(&listing, &listing.rows(&unfolded))[1..4],
            [
                row(1, "[Mihnea8848]", 3),
                row(2, "Koikatsu Girls Academy", 2),
                row(2, "Sketches", 1)
            ]
        );

        // A folder shows its cards and those below it, by file name.
        assert_eq!(listing.matching(None, "").len(), 7);
        assert_eq!(
            names(&listing, &listing.matching(Some(0), "")),
            ["bookish", "Simple", "Aoi", "Rin", "Draft", "Yuki"]
        );
        assert_eq!(names(&listing, &listing.matching(Some(1), "")), ["Aoi", "Rin", "Draft"]);
        assert_eq!(names(&listing, &listing.matching(Some(2), "")), ["Aoi", "Rin"]);
        let male = listing.folders.iter().position(|f| f.label == "Male").unwrap();
        assert_eq!(names(&listing, &listing.matching(Some(male), "")), ["Player"]);
        // The search stays within the folder.
        assert_eq!(names(&listing, &listing.matching(Some(0), " RI")), ["Rin"]);
        assert!(listing.matching(Some(male), "rin").is_empty());
        let _ = std::fs::remove_dir_all(&game);
    }

    #[test]
    fn a_listing_goes_after_another() {
        let game = game("append");
        let mut listing = game_listing(&game);
        let extra = temp("append-extra");
        write(&extra.join("Default female.png"));
        write(&extra.join("Story/c-8.png"));
        let mut more = Listing::default();
        more.add_folder(&extra, "Default", 0);
        let before = listing.folders.len();
        listing.append(more);
        let all: HashSet<String> = listing.folders.iter().map(|f| f.key.clone()).collect();
        assert_eq!(
            folders(&listing, &listing.rows(&all))[before..],
            [(0, "Default".to_owned(), 2), (1, "Story".to_owned(), 1)]
        );
        assert_eq!(
            names(&listing, &listing.matching(Some(before), "")),
            ["Default female", "c-8"]
        );
        assert_eq!(names(&listing, &listing.matching(Some(before + 1), "")), ["c-8"]);
        assert_eq!(listing.matching(None, "").len(), 9);
        let _ = std::fs::remove_dir_all(&game);
        let _ = std::fs::remove_dir_all(&extra);
    }

    #[test]
    fn koikatsu_is_a_source_only_when_its_folder_is_set() {
        let game = game("sources");
        assert!(sources(&[], None).is_empty());
        let config = rk_core::Config { game_dir: game.clone() };
        let mut found = sources(&[], Some(&config));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Koikatsu");
        // Read to the end, it opens on the game's first folder; cards
        // that do not read keep their files' names.
        assert!(!found[0].advance(true));
        assert_eq!(found[0].folder, Some(0));
        let listing = &found[0].listing;
        assert_eq!(listing.folders[0].label, "Female");
        assert_eq!(listing.cards.len(), 7);
        assert!(listing.cards.iter().all(|c| !c.renamed));
        let _ = std::fs::remove_dir_all(&game);
    }

    #[test]
    fn a_packages_card_folders_are_its_tree() {
        let dir = temp("package");
        write(&dir.join("cards/female/Default female.png"));
        write(&dir.join("cards/female/Story c-8.png"));
        write(&dir.join("cards/male/Story player.png"));
        write(&dir.join("abdata/chara/thumb.png"));
        let mut source = Source::new("A package".to_owned(), Place::Package(dir.clone()));
        // Frame by frame, as the window takes it, without waiting.
        while source.advance(false) {
            std::thread::yield_now();
        }
        // A package opens on all its cards, which go by the names shown.
        assert_eq!(source.folder, None);
        let listing = &source.listing;
        let all: Vec<usize> = (0..listing.folders.len()).collect();
        assert_eq!(
            folders(listing, &all),
            [(0, "Female".to_owned(), 2), (0, "Male".to_owned(), 1)]
        );
        assert_eq!(
            names(listing, &listing.matching(None, "")),
            ["Default female", "Story c-8", "Story player"]
        );
        assert!(listing.cards.iter().all(|c| c.renamed));
        assert_eq!(names(listing, &listing.matching(None, "PLAYER")), ["Story player"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_games_own_cards_get_readable_file_names() {
        assert_eq!(default_file("ill_Default_Female"), "Default female.png");
        assert_eq!(default_file("c-8"), "C-8.png");
    }
}
