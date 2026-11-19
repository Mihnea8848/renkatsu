// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! CharaStudio's list of animations (its Anime menu), read as
//! `Studio.Info` reads it. Some of them hold one pose and some move; a
//! pose in CharaStudio's own sense, a file of FK and IK rotations, is
//! something else and not read here.
//!
//! Every bundle under `studio/info/` may hold sheets: `AnimeGroup_<file>`
//! names the groups, `AnimeCategory_<n>_<group>` the categories of a
//! group, and `Anime_<n>_<group>_<category>` the animations: a number, the
//! group, the category, the name shown, the bundle, the controller in it
//! and the state to play. The H scenes' animations are listed in the same
//! bundles, as `HAnime_<n>_<group>_<category>`, in the same groups and
//! categories but in a sheet one column wider: a second bundle and file
//! sit in columns 6 and 7, so the state to play is column 8. The game
//! reads both into one list, its own sheets first, and so does this.
//! Bundles are read in name order and a later sheet replaces an earlier
//! one's entry. An animation whose bundle is not installed is left out,
//! as the game leaves it out.
//!
//! The game's names are Japanese; the names shown here are translations
//! made for Renkatsu, by the words the names are built from.

use rk_unityfs::{class, UObject, UValue};
use rk_vfs::Game;
use std::collections::BTreeMap;

/// The most any curve of an animation may change for it to count as
/// not moving at all: below what shows, above the noise of stored numbers.
pub const STILL: f32 = 1e-4;

/// The longest an animation that does not loop may be and still count as
/// a pose. The game stores its fixed poses as clips of six frames at
/// thirty a second, a sixth of a second, which play once and hold their
/// last frame; everything else in its list lasts 0.7 seconds or more.
pub const STATIC_SECONDS: f32 = 0.2;

/// Where an animation comes from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Source {
    /// The user's Koikatsu install.
    #[default]
    Game,
    /// An installed package, by the name of its folder, which is its id.
    Package(String),
    /// A zipmod, by its name.
    Mod(String),
}

impl Source {
    /// The name the source shows under.
    pub fn label(&self) -> &str {
        match self {
            Source::Game => "Koikatsu",
            Source::Package(id) => id,
            Source::Mod(name) => name,
        }
    }
}

/// A zipmod of the index as a source.
pub fn mod_source(index: &rk_vfs::index::Index, mod_idx: u32) -> Source {
    let info = index.mods.get(mod_idx as usize);
    let name = info.map(|m| {
        if m.name.trim().is_empty() {
            m.guid.clone()
        } else {
            m.name.clone()
        }
    });
    Source::Mod(name.unwrap_or_else(|| "Mod".to_owned()))
}

/// Where the first copy of a bundle lies: in a zipmod, in a package or in
/// the game. `None` when no source has it.
pub fn source_of(index: &rk_vfs::index::Index, bundle: &str) -> Option<Source> {
    let loc = index.bundles.get(&rk_vfs::index::norm(bundle))?.first()?;
    Some(match loc.mod_idx {
        Some(i) => mod_source(index, i),
        None => {
            let root = index.roots.get(loc.root as usize);
            let in_game =
                !index.game_dir.as_os_str().is_empty() && root.is_some_and(|r| r.starts_with(&index.game_dir));
            match root.and_then(|r| r.parent()).and_then(|p| p.file_name()) {
                Some(package) if !in_game => Source::Package(package.to_string_lossy().into_owned()),
                _ => Source::Game,
            }
        }
    })
}

/// One animation of the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Where CharaStudio has it: group, category and number.
    pub group: i32,
    pub category: i32,
    pub number: i32,
    /// The name the game shows.
    pub original: String,
    /// The name in English.
    pub name: String,
    pub bundle: String,
    /// The animator controller asset in the bundle.
    pub controller: String,
    /// The state of the controller.
    pub state: String,
    /// A second controller whose clips replace the base controller's by
    /// name, as the game layers it over them. The H scenes' animations
    /// share one controller for a whole category and name a different
    /// one of these each; the rest leave it empty.
    pub override_bundle: String,
    pub override_controller: String,
    /// How long it is and whether it loops, once its clips were read.
    pub timing: Option<Timing>,
    /// Who lists it: the game, a package or a mod. A mod may list clips
    /// of the game's own bundles under new names, and it is still the
    /// mod's animation.
    pub source: Source,
    /// The H scenes list it, in an `HAnime_` sheet, rather than
    /// CharaStudio's own `Anime_` sheets.
    pub h: bool,
}

/// What an animation's clips say about its time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    /// The length of one pass in seconds.
    pub seconds: f32,
    /// It starts over at its end.
    pub loops: bool,
}

impl Timing {
    /// Whether the animation holds one pose: it plays for a moment and
    /// stays on its last frame. The others move for as long as they play.
    pub fn is_static(&self) -> bool {
        !self.loops && self.seconds <= STATIC_SECONDS
    }
}

impl Entry {
    /// Whether the animation holds one pose; `None` until its clips were
    /// read.
    pub fn is_static(&self) -> Option<bool> {
        self.timing.map(|t| t.is_static())
    }

    /// The controller whose clips replace the base's, for the animations
    /// whose list names one.
    pub fn over(&self) -> crate::motion::Override<'_> {
        (!self.override_bundle.is_empty()).then_some((self.override_bundle.as_str(), self.override_controller.as_str()))
    }
}

/// The whole list: the animations with the names of their groups and
/// categories.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Library {
    pub entries: Vec<Entry>,
    /// How many of the animations, from the first, have their timing.
    timed: usize,
    groups: BTreeMap<i32, String>,
    categories: BTreeMap<(i32, i32), String>,
}

/// A category of a group that has animations, for a list of folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub group: i32,
    pub category: i32,
    /// `Group: Category` in English.
    pub name: String,
    pub count: usize,
}

/// A group of the browsing tree with its categories and how many
/// animations each holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub count: usize,
    pub categories: Vec<(String, usize)>,
}

fn lowered(s: &str) -> impl Iterator<Item = char> + '_ {
    s.trim().chars().flat_map(char::to_lowercase)
}

/// The same name, whatever its capitals and the spaces around it.
pub fn same_name(a: &str, b: &str) -> bool {
    lowered(a).eq(lowered(b))
}

/// Builds the browsing tree from the group and category names of some
/// animations, in the order they first appear.
pub fn tree_of<'a>(named: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<Branch> {
    let mut out: Vec<Branch> = Vec::new();
    for (group, category) in named {
        let at = match out.iter().position(|b| same_name(&b.name, group)) {
            Some(at) => at,
            None => {
                out.push(Branch {
                    name: group.trim().to_owned(),
                    count: 0,
                    categories: Vec::new(),
                });
                out.len() - 1
            }
        };
        let branch = &mut out[at];
        branch.count += 1;
        match branch.categories.iter_mut().find(|(name, _)| same_name(name, category)) {
            Some((_, count)) => *count += 1,
            None => branch.categories.push((category.trim().to_owned(), 1)),
        }
    }
    out
}

/// The rows of a sheet, each a row of cells.
pub type Rows = Vec<Vec<String>>;

/// The rows of a sheet as the game stores it in a bundle (`ExcelData`).
pub fn sheet_rows(sheet: &UObject) -> Rows {
    let cells = |row: &UValue| -> Vec<String> {
        row.as_object()
            .and_then(|r| r.array("list"))
            .unwrap_or_default()
            .iter()
            .map(|c| c.as_str().unwrap_or_default().to_owned())
            .collect()
    };
    sheet.array("list").unwrap_or_default().iter().map(cells).collect()
}

fn rows(sheet: &Rows) -> impl Iterator<Item = Vec<&str>> {
    sheet.iter().map(|row| row.iter().map(String::as_str).collect())
}

/// The numbers in a sheet's name after `prefix`: `Anime_00_07_22` gives
/// 0, 7 and 22 for `Anime_`.
fn sheet_numbers(name: &str, prefix: &str) -> Option<Vec<i32>> {
    name.get(..prefix.len()).filter(|p| p.eq_ignore_ascii_case(prefix))?;
    name[prefix.len()..].split('_').map(|n| n.parse().ok()).collect()
}

/// The two shapes an animation sheet comes in. Both name the animation
/// in column 3, its bundle in 4 and its controller in 5, and the game
/// reads them into one list; they part after that, the H scenes' sheets
/// carrying a second bundle and file (the override) in 6 and 7 before
/// the clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Layout {
    /// `Anime_<n>_<group>_<category>`: the clip is column 6.
    Studio,
    /// `HAnime_<n>_<group>_<category>`: the clip is column 8.
    HScene,
}

impl Layout {
    fn prefix(self) -> &'static str {
        match self {
            Layout::Studio => "Anime_",
            Layout::HScene => "HAnime_",
        }
    }

    /// The column holding the state the controller plays.
    fn state(self) -> usize {
        match self {
            Layout::Studio => 6,
            Layout::HScene => 8,
        }
    }

    /// The columns holding the bundle and the file of the controller
    /// whose clips replace the base's. CharaStudio's own sheets have
    /// none, so they are pointed at an empty cell.
    fn over(self) -> (usize, usize) {
        match self {
            Layout::Studio => (usize::MAX, usize::MAX),
            Layout::HScene => (6, 7),
        }
    }
}

impl Library {
    /// Adds the sheets of one `studio/info` bundle, or of one folder of
    /// list files of a mod. `sheets` are the sheets by name, `listed` is
    /// who brings them and `installed` says whether a bundle exists. A bundle's sheet of
    /// groups is the one named after the bundle, `AnimeGroup_<stem>`; a
    /// mod's folder has no such name to go by (`stem` is `None`) and every
    /// `AnimeGroup_` sheet in it counts, as for Sideloader.
    pub fn add_bundle(
        &mut self,
        stem: Option<&str>,
        sheets: &[(String, Rows)],
        listed: &Source,
        installed: &dyn Fn(&str) -> bool,
    ) {
        let is_groups = |name: &str| match stem {
            Some(stem) => name.eq_ignore_ascii_case(&format!("AnimeGroup_{stem}")),
            None => name.get(..11).is_some_and(|p| p.eq_ignore_ascii_case("AnimeGroup_")),
        };
        for (_, sheet) in sheets.iter().filter(|(n, _)| is_groups(n)) {
            for row in rows(sheet).skip(1) {
                if let (Some(Ok(group)), Some(name)) = (row.first().map(|c| c.trim().parse()), row.get(1)) {
                    self.groups.insert(group, (*name).to_owned());
                }
            }
        }
        // Category sheets by group, then by their own number.
        let mut ordered: Vec<(Vec<i32>, &Rows)> = sheets
            .iter()
            .filter_map(|(n, s)| Some((sheet_numbers(n, "AnimeCategory_").filter(|v| v.len() == 2)?, s)))
            .collect();
        ordered.sort_by_key(|(n, _)| (n[1], n[0]));
        for (numbers, sheet) in ordered {
            let group = numbers[1];
            if !self.groups.contains_key(&group) {
                continue;
            }
            for row in rows(sheet).skip(1) {
                if let (Some(Ok(category)), Some(name)) = (row.first().map(|c| c.trim().parse::<i32>()), row.get(1)) {
                    self.categories.insert((group, category), (*name).to_owned());
                }
            }
        }
        // Animation sheets by group, category, then number: CharaStudio's
        // own first, then the H scenes', as the game reads them, so that a
        // clash of numbers ends the way it does there.
        let mut ordered: Vec<(Layout, Vec<i32>, &Rows)> = Vec::new();
        for (name, sheet) in sheets {
            for layout in [Layout::Studio, Layout::HScene] {
                if let Some(numbers) = sheet_numbers(name, layout.prefix()).filter(|v| v.len() == 3) {
                    ordered.push((layout, numbers, sheet));
                    break;
                }
            }
        }
        ordered.sort_by_key(|(layout, n, _)| (*layout, n[1], n[2], n[0]));
        for (layout, _, sheet) in ordered {
            for row in rows(sheet).skip(2) {
                let cell = |i: usize| row.get(i).copied().unwrap_or_default();
                let number = |i: usize| cell(i).trim().parse::<i32>().ok();
                let (Some(no), Some(group), Some(category)) = (number(0), number(1), number(2)) else {
                    continue;
                };
                let bundle = cell(4);
                if !installed(bundle) {
                    continue;
                }
                let pose = Entry {
                    group,
                    category,
                    number: no,
                    original: cell(3).to_owned(),
                    name: english(cell(3)),
                    bundle: bundle.to_owned(),
                    controller: cell(5).to_owned(),
                    state: cell(layout.state()).to_owned(),
                    override_bundle: cell(layout.over().0).to_owned(),
                    override_controller: cell(layout.over().1).to_owned(),
                    timing: None,
                    source: listed.clone(),
                    h: layout == Layout::HScene,
                };
                match self
                    .entries
                    .iter_mut()
                    .find(|p| (p.group, p.category, p.number) == (group, category, no))
                {
                    Some(old) => *old = pose,
                    None => self.entries.push(pose),
                }
            }
        }
    }

    /// Reads the list from everything that is installed: the sheets in
    /// the `studio/info` bundles of the game, of the packages and of mods,
    /// then the list files mods bring as CSV, each mod's folders in turn.
    /// A mod or a package so adds groups and categories of its own, or
    /// puts animations into the ones that exist, by the numbers its sheets
    /// give.
    pub fn read(game: &Game) -> Library {
        Library::read_with(game, false)
    }

    /// As [`Library::read`]; with `missing`, animations whose bundle is
    /// not among the content are listed too, for looking at what a
    /// package lists by itself, without the game it adds to.
    pub fn read_with(game: &Game, missing: bool) -> Library {
        let installed = |bundle: &str| missing || game.assets.index.bundles.contains_key(&rk_vfs::index::norm(bundle));
        let mut paths: Vec<&String> = game
            .assets
            .index
            .bundles
            .keys()
            .filter(|p| p.starts_with("studio/info/"))
            .collect();
        paths.sort();
        let mut library = Library::default();
        for path in paths {
            let stem = path
                .rsplit('/')
                .next()
                .unwrap_or(path)
                .trim_end_matches(".unity3d")
                .to_owned();
            let mut sheets = Vec::new();
            // The game's own bundle first, then the mods' of the same name,
            // whose entries replace the game's.
            for bundle in game.assets.bundles(path).into_iter().rev() {
                for file in &bundle.files {
                    for info in file.sf.objects_of(class::MONO_BEHAVIOUR) {
                        let Some(name) = file.sf.name(info) else { continue };
                        let lower = name.to_ascii_lowercase();
                        if !lower.starts_with("anime") && !lower.starts_with("hanime") {
                            continue;
                        }
                        if let Ok(sheet) = file.sf.read(info) {
                            sheets.push((name, sheet_rows(&sheet)));
                        }
                    }
                }
            }
            let listed = source_of(&game.assets.index, path).unwrap_or_default();
            library.add_bundle(Some(&stem), &sheets, &listed, &installed);
        }
        // The list files of mods, a folder of a mod at a time.
        let mut folders: Vec<(Option<u32>, &str)> = Vec::new();
        for sheet in &game.assets.index.studio_sheets {
            if !folders.contains(&(sheet.mod_idx, sheet.folder.as_str())) {
                folders.push((sheet.mod_idx, &sheet.folder));
            }
        }
        for (mod_idx, folder) in folders {
            let sheets: Vec<(String, Rows)> = game
                .assets
                .index
                .studio_sheets
                .iter()
                .filter(|s| (s.mod_idx, s.folder.as_str()) == (mod_idx, folder))
                .map(|s| (s.name.clone(), s.rows.clone()))
                .collect();
            let listed = mod_idx.map_or(Source::Game, |i| mod_source(&game.assets.index, i));
            library.add_bundle(None, &sheets, &listed, &installed);
        }
        if installed(MAKER_POSE.0) {
            library.entries.push(maker_pose(
                source_of(&game.assets.index, MAKER_POSE.0).unwrap_or_default(),
            ));
        }
        library.entries.sort_by_key(|p| (p.group, p.category, p.number));
        library
    }

    /// How many animations have their timing.
    pub fn timed(&self) -> usize {
        self.timed
    }

    /// Reads the timing of the animations that lack it, from their clips,
    /// until `enough` says to stop; true once every animation has it. An
    /// animation's length is in its clips alone (the controller only says
    /// whether a state loops), so this reads them all, a few at a time
    /// when the caller has a frame to draw. An animation that cannot be
    /// read counts as one without length.
    pub fn time(&mut self, game: &Game, enough: &dyn Fn() -> bool) -> bool {
        let parameters = Self::TIMING_BODY;
        while self.timed < self.entries.len() {
            if enough() {
                return false;
            }
            let pose = &mut self.entries[self.timed];
            pose.timing = Some(timing_of(game, pose, &parameters));
            self.timed += 1;
        }
        true
    }

    /// Takes the timing of the animation at `index`, read elsewhere.
    /// Animations are counted as timed in order, up to the first without.
    pub fn record(&mut self, index: usize, timing: Timing) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.timing = Some(timing);
        }
        while self.entries.get(self.timed).is_some_and(|e| e.timing.is_some()) {
            self.timed += 1;
        }
    }

    /// The body timings are read for: a middling one, where blends by
    /// height sit between their clips.
    pub const TIMING_BODY: crate::motion::Parameters = crate::motion::Parameters {
        height: 0.5,
        breast: 0.5,
        options: None,
    };

    /// The categories that have animations, in the game's order.
    pub fn categories(&self) -> Vec<Category> {
        let mut out: Vec<Category> = Vec::new();
        for pose in &self.entries {
            match out.last_mut() {
                Some(last) if (last.group, last.category) == (pose.group, pose.category) => last.count += 1,
                _ => out.push(Category {
                    group: pose.group,
                    category: pose.category,
                    name: self.category_name(pose.group, pose.category),
                    count: 1,
                }),
            }
        }
        out
    }

    /// `Group: Category` in English.
    pub fn category_name(&self, group: i32, category: i32) -> String {
        let group_name = self.groups.get(&group).map_or_else(String::new, |g| english(g));
        let name = self
            .categories
            .get(&(group, category))
            .map_or_else(|| format!("Category {category}"), |c| english(c));
        if group_name.is_empty() {
            name
        } else {
            format!("{group_name}: {name}")
        }
    }

    /// The name of a group in English, or `Group <n>` for one no sheet
    /// names.
    pub fn group_name(&self, group: i32) -> String {
        self.groups
            .get(&group)
            .map_or_else(|| format!("Group {group}"), |g| english(g))
    }

    /// The name of a category in English, without its group.
    pub fn category_label(&self, group: i32, category: i32) -> String {
        self.categories
            .get(&(group, category))
            .map_or_else(|| format!("Category {category}"), |c| english(c))
    }

    /// The groups and their categories for browsing, in the list's order.
    /// They are told apart by name, not by number: categories of the same
    /// name in groups of the same name are one, whichever source brought
    /// them and whatever numbers it used.
    pub fn tree(&self) -> Vec<Branch> {
        let named: Vec<(String, String)> = self
            .entries
            .iter()
            .map(|e| (self.group_name(e.group), self.category_label(e.group, e.category)))
            .collect();
        tree_of(named.iter().map(|(g, c)| (g.as_str(), c.as_str())))
    }

    /// The animation CharaStudio has under these numbers.
    pub fn find(&self, group: i32, category: i32, number: i32) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|p| (p.group, p.category, p.number) == (group, category, number))
    }
}

impl Library {
    /// The groups that have animations, in the game's order: the group's
    /// number, its name in English and how many animations it has.
    pub fn groups(&self) -> Vec<(i32, String, usize)> {
        let mut out: Vec<(i32, String, usize)> = Vec::new();
        for pose in &self.entries {
            match out.iter_mut().find(|g| g.0 == pose.group) {
                Some(group) => group.2 += 1,
                None => {
                    let name = self
                        .groups
                        .get(&pose.group)
                        .map_or_else(|| format!("Group {}", pose.group), |g| english(g));
                    out.push((pose.group, name, 1));
                }
            }
        }
        out
    }
}

/// A sheet from rows of cells, for tests.
pub fn sheet(rows: &[&[&str]]) -> Rows {
    rows.iter()
        .map(|cells| cells.iter().map(|c| (*c).to_owned()).collect())
        .collect()
}

/// The words the game's names of groups, categories and animations are
/// built from, longest first where one starts another.
const WORDS: &[(&str, &str)] = &[
    // Groups.
    ("キャラ", "Character"),
    ("スタジオ", "Studio"),
    ("バトル", "Battle"),
    ("アウトドア", "Outdoor"),
    ("サマー", "Summer"),
    ("アフタースクール", "After school"),
    // Categories.
    ("基本", "Basic"),
    ("感情", "Emotion"),
    ("歩く・走る", "Walk and run"),
    ("会話", "Talk"),
    ("椅子・机", "Chair and desk"),
    ("床", "Floor"),
    ("食べる", "Eat"),
    ("アクション", "Action"),
    ("リアクション", "Reaction"),
    ("ライブ", "Live show"),
    ("追加", "Extra"),
    ("和", "Japanese style"),
    ("メカ", "Mecha"),
    ("ファンタジー", "Fantasy"),
    ("グラビア", "Photo shoot"),
    ("休日", "Day off"),
    ("ハロウィン", "Halloween"),
    ("クリスマス", "Christmas"),
    ("カットイン", "Cut-in"),
    // Animations.
    ("あくび", "Yawn"),
    ("お姉さん座り", "Sit with legs to the side"),
    ("お茶をたてる待機", "Make tea, wait"),
    ("お茶をたてる", "Make tea"),
    ("お茶を飲む待機", "Drink tea, wait"),
    ("お茶を飲む", "Drink tea"),
    ("くしゃみ", "Sneeze"),
    ("しゃがみ歩き", "Crouch walk"),
    ("しゃがみ", "Crouch"),
    ("しんきゃく", "Stomp"),
    ("アニメ", "Animation"),
    ("ウクレレ", "Ukulele"),
    ("クロールで泳ぐ", "Swim crawl"),
    ("ゲームをする", "Play a game"),
    ("シャドーボクシング", "Shadow boxing"),
    ("シューティング", "Shooting"),
    ("シーン", "Scene"),
    ("スイング", "Swing"),
    ("スクワット", "Squat"),
    ("ストレッチ", "Stretch"),
    ("スマホを眺める", "Look at phone"),
    ("ソファーでまったり", "Relax on sofa"),
    ("ダイブ", "Dive"),
    ("ダンス練習待機", "Dance practice, wait"),
    ("ダンス練習", "Dance practice"),
    ("ダンス", "Dance"),
    ("チアダンス待機", "Cheer dance, wait"),
    ("チアダンス練習", "Cheer dance practice"),
    ("チラシを配る", "Hand out flyers"),
    ("パタパタ", "Fan oneself"),
    ("ビーチボール", "Beach ball"),
    ("フリック操作", "Swipe on phone"),
    ("フル", "Full"),
    ("プール座り", "Sit by the pool"),
    ("ベンチで寝る", "Sleep on bench"),
    ("ポスターを貼る", "Put up poster"),
    ("立ちポーズ", "Standing pose"),
    // The game's first animation holds the T pose; "Default pose" is the
    // character maker's standing pose, listed before it.
    ("初期ポーズ", "T-pose"),
    ("ポールダンス", "Pole dance"),
    ("ポーズ", "Pose"),
    ("ライフル", "Rifle"),
    ("リップクリームを塗る待機", "Lip balm, wait"),
    ("リップクリームを塗る", "Put on lip balm"),
    ("両手剣", "Two-handed sword"),
    ("中腰", "Half crouch"),
    ("二丁拳銃", "Two pistols"),
    ("伸び", "Stretch up"),
    ("倒れる", "Fall down"),
    ("勉強をする待機", "Study, wait"),
    ("勉強をする", "Study"),
    ("動き回る", "Move around"),
    ("呼び出し", "Call over"),
    ("驚恥他", "Surprise, shame and others"),
    ("哀", "Sad"),
    ("喜", "Happy"),
    ("怒", "Angry"),
    ("壁立ち", "Stand at wall"),
    ("外を眺める", "Look outside"),
    ("水を掛けられる", "Get splashed"),
    ("水を掛ける", "Splash water"),
    ("準備体操", "Warm-up"),
    ("女の子座り", "Sit on heels apart"),
    ("姿勢を変える", "Change posture"),
    ("容姿を確認する待機", "Check looks, wait"),
    ("容姿を確認する", "Check looks"),
    ("寝たふり", "Pretend to sleep"),
    ("寝返り待機", "Turn over, wait"),
    ("寝返り", "Turn over"),
    ("添い寝", "Lie alongside"),
    ("机で寝る", "Sleep at desk"),
    ("椅子で寝る", "Sleep on chair"),
    ("寝バック", "Lying from behind"),
    ("寝位", "Lying position"),
    ("寝", "Lie down"),
    ("小太刀二刀", "Two short swords"),
    ("小太刀", "Short sword"),
    ("横座り", "Sit sideways"),
    ("正座", "Kneel formally"),
    ("座り", "Sit"),
    ("弓矢", "Bow and arrow"),
    ("箒持ち待機さぼり", "Hold broom, slack off"),
    ("箒持ち待機", "Hold broom, wait"),
    ("足を組み待機", "Legs crossed, wait"),
    ("足を組み換える", "Cross legs the other way"),
    ("恥ずかしがる", "Embarrassed"),
    ("手を振る", "Wave"),
    ("手を触れられ嫌がられる", "Hand touched, dislikes it"),
    ("手を触れられ恥ずかしがる", "Hand touched, embarrassed"),
    ("胸を見られ恥ずかしがる", "Looked at, embarrassed"),
    ("胸を触られ隠す", "Covers chest"),
    ("頬を触られ嫌がられる", "Cheek touched, dislikes it"),
    ("顔を見られ隠す", "Hides face"),
    ("隠すループ", "Cover up, loop"),
    ("隠す", "Cover up"),
    ("投擲", "Throw"),
    ("拳銃", "Pistol"),
    ("掃く", "Sweep"),
    ("攻撃", "Attack"),
    ("防御", "Guard"),
    ("暑がる", "Feel hot"),
    ("本を探す", "Look for a book"),
    ("本を見る", "Look at a book"),
    ("本を読む待機", "Read a book, wait"),
    ("本を読む", "Read a book"),
    ("机によりかかる", "Lean on desk"),
    ("机に座る", "Sit on desk"),
    ("机を拭く", "Wipe desk"),
    ("窓を拭く", "Wipe window"),
    ("格闘", "Martial arts"),
    ("棒・槍", "Staff and spear"),
    ("槌・鎌", "Hammer and scythe"),
    ("盾と剣", "Shield and sword"),
    ("片手剣", "One-handed sword"),
    ("片足上げ抱え", "Hold one leg up"),
    ("気弾", "Energy blast"),
    ("魔法本", "Spell book"),
    ("魔法杖", "Magic staff"),
    ("着替え", "Change clothes"),
    ("絵を描く待機", "Draw, wait"),
    ("絵を描く", "Draw"),
    ("腰振り", "Sway hips"),
    ("自撮り待機", "Selfie, wait"),
    ("自撮り", "Selfie"),
    ("音楽を聴く", "Listen to music"),
    ("髪を直す", "Fix hair"),
    ("食事", "Meal"),
    ("飲む待機", "Drink, wait"),
    ("飲む", "Drink"),
    ("歩く", "Walk"),
    ("走り", "Run"),
    ("走る", "Run"),
    ("話す", "Talk"),
    ("立ち", "Stand"),
    ("椅子", "Chair"),
    ("待機", "Wait"),
    // The H scenes' groups.
    ("ダークネス", "Darkness"),
    ("複数", "Multiple"),
    ("愛撫", "Foreplay"),
    ("特殊", "Special"),
    ("レズ", "Lesbian"),
    ("奉仕", "Service"),
    ("挿入前", "Before insertion"),
    ("挿入", "Insertion"),
    // Where they happen.
    ("本棚", "Bookshelf"),
    ("ソファ", "Sofa"),
    ("ベンチ", "Bench"),
    ("フェンス", "Fence"),
    ("プール", "Pool"),
    ("トイレ", "Toilet"),
    ("シャワー", "Shower"),
    ("ハードル", "Hurdle"),
    ("跳び箱", "Vaulting box"),
    // The positions.
    ("開脚", "Spread legs"),
    ("正常位", "Missionary"),
    ("マングリ", "Piledriver"),
    ("騎乗位", "Cowgirl"),
    ("後背位", "Doggy style"),
    ("側位", "Side position"),
    ("立位", "Standing position"),
    ("駅弁", "Standing carry"),
    ("種付けプレス", "Mating press"),
    ("四つん這い", "On all fours"),
    ("仰向け", "On her back"),
    ("対面", "Facing"),
    ("背面", "Facing away"),
    ("バック", "From behind"),
    ("アナル", "Anal"),
    ("片足上げ", "One leg up"),
    ("押さえ", "Held down"),
    ("押し付け", "Pressed against"),
    ("腕引っ張り", "Arms pulled"),
    ("腕はさみ", "Arms squeezed"),
    ("密着", "Pressed close"),
    ("またがり", "Straddling"),
    ("掴まり", "Holding on"),
    // What is done.
    ("パイズリ", "Breast job"),
    ("Wフェラ", "Double blowjob"),
    ("フェラ", "Blowjob"),
    ("イラマチオ", "Deep throat"),
    ("クンニ", "Cunnilingus"),
    ("手マン", "Fingering"),
    ("オナニー", "Masturbation"),
    ("オナホ", "Onahole"),
    ("両手フェラ", "Two-handed blowjob"),
    ("片手フェラ", "One-handed blowjob"),
    ("両手コキ", "Two-handed handjob"),
    ("片手コキ", "One-handed handjob"),
    ("手コキ", "Handjob"),
    ("ノーハンド", "No hands"),
    ("亀頭", "Glans"),
    ("咥え", "with mouth"),
    ("口内", "In mouth"),
    ("舐め", "Licking"),
    ("いじり", "Teasing"),
    ("いたずら", "Mischief"),
    ("覗き", "Peeping"),
    ("互い", "Each other"),
    ("弄り", "Touching"),
    ("具合わせ", "Rubbing together"),
    ("揉まれ", "Fondled"),
    ("触り", "Touched"),
    ("反応", "Reaction"),
    ("キス", "Kiss"),
    ("嫌がり", "Dislikes it"),
    ("股間", "Crotch"),
    // How one goes.
    ("絶頂前", "Before climax"),
    ("絶頂", "Climax"),
    ("同時", "Together"),
    ("外出し", "Outside finish"),
    ("中出し", "Finish inside"),
    ("汁垂れ", "Dripping"),
    ("抜く", "Withdraw"),
    ("ループ", "Loop"),
    ("開始", "Start"),
    ("事後", "After"),
    ("のむ", "Swallow"),
    ("はく", "Spit out"),
    ("弱", "Weak"),
    ("強", "Strong"),
    ("両手", "Both hands"),
    ("片手", "One hand"),
    ("胸", "Breasts"),
    ("尻", "Buttocks"),
    ("玉", "Balls"),
    ("竿", "Shaft"),
    ("先", "Tip"),
    ("机", "Desk"),
    ("壁", "Wall"),
    ("角", "Corner"),
    ("中", "Inside"),
    ("前", "Front"),
    ("後", "Rear"),
    ("男", "Male"),
    ("女", "Female"),
];

/// A name of the game's lists in English: the words it is built from are
/// translated and what is left (numbers, letters, the `+` and `&` the
/// names join parts with) is kept, with full width forms made plain. A
/// name with a word that is not known stays as the game has it.
pub fn english(name: &str) -> String {
    let plain: String = name
        .chars()
        .map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '\u{3000}' => ' ',
            c => c,
        })
        .collect();
    let mut out = String::new();
    let mut rest = plain.trim();
    // The last thing written was a translated word.
    let mut after_word = false;
    while !rest.is_empty() {
        if let Some((word, english)) = WORDS.iter().find(|(w, _)| rest.starts_with(w)) {
            after_word = true;
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            // Only the first word keeps its capital.
            if out.is_empty() {
                out.push_str(english);
            } else {
                out.push_str(&english.to_lowercase());
            }
            rest = &rest[word.len()..];
            continue;
        }
        let c = rest.chars().next().unwrap_or(' ');
        if !c.is_ascii() {
            return name.to_owned();
        }
        if (c.is_ascii_alphanumeric() || c == '+' || c == '&') && after_word && !out.ends_with(' ') {
            out.push(' ');
        }
        after_word = false;
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.is_empty() {
        name.to_owned()
    } else {
        out
    }
}

/// The pose the game's character maker starts every character in: the
/// first of its poses (`cus_pose` in `custom/customscenelist.unity3d`).
pub const MAKER_POSE: (&str, &str, &str) = ("custom/custompose.unity3d", "custom_pose", "Stand_D_00");

/// The maker's pose as the first entry of Character: Basic, before the
/// game's own T pose. CharaStudio does not list it; Renkatsu does, as the
/// default pose.
pub fn maker_pose(source: Source) -> Entry {
    Entry {
        group: 0,
        category: 0,
        number: -1,
        original: "基本".to_owned(),
        name: "Default pose".to_owned(),
        bundle: MAKER_POSE.0.to_owned(),
        controller: MAKER_POSE.1.to_owned(),
        state: MAKER_POSE.2.to_owned(),
        override_bundle: String::new(),
        override_controller: String::new(),
        timing: None,
        source,
        h: false,
    }
}

/// How long an animation is and whether it loops, from its clips. One that
/// cannot be read counts as one without length.
pub fn timing_of(game: &Game, entry: &Entry, parameters: &crate::motion::Parameters) -> Timing {
    crate::motion::load(
        game,
        &entry.bundle,
        &entry.controller,
        &entry.state,
        entry.over(),
        parameters,
    )
    .map(|m| Timing {
        seconds: m.duration(),
        loops: m.loops(),
    })
    .unwrap_or(Timing {
        seconds: 0.0,
        loops: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_translated_by_their_words() {
        assert_eq!(english("歩く1"), "Walk 1");
        assert_eq!(english("立ち １２"), "Stand 12");
        assert_eq!(english("女 格闘A"), "Female martial arts A");
        assert_eq!(english("お茶を飲む待機"), "Drink tea, wait");
        assert_eq!(english("歩く・走る"), "Walk and run");
        assert_eq!(english("ポーズ"), "Pose");
        // The H scenes' names: their own words, and the `+` and `&` that
        // join parts, full width or not.
        assert_eq!(english("H愛撫"), "H foreplay");
        assert_eq!(english("女H挿入"), "Female H insertion");
        assert_eq!(english("椅子パイズリ+咥え"), "Chair breast job + with mouth");
        assert_eq!(english("先舐め＋竿舐め"), "Tip licking + shaft licking");
        assert_eq!(english("女 バック＆フェラ"), "Female from behind & blowjob");
        assert_eq!(english("同時絶頂弱ループ"), "Together climax weak loop");
        assert_eq!(english("挿入前待機"), "Before insertion wait");
        // A longer word wins: 寝位 is not 寝 and a stray 位.
        assert_eq!(english("机寝位"), "Desk lying position");
        // A word that is not known leaves the name alone.
        assert_eq!(english("謎の踊り2"), "謎の踊り2");
        assert_eq!(english("Idle"), "Idle");
        assert_eq!(english("EC"), "EC");
    }

    fn library() -> Library {
        let sheets = vec![
            (
                "AnimeGroup_00".to_owned(),
                sheet(&[&["id", "name"], &["0", "キャラ"], &["7", "スタジオ"]]),
            ),
            (
                "AnimeCategory_00_00".to_owned(),
                sheet(&[&["id", "name"], &["3", "歩く・走る"], &["4", "立ち"]]),
            ),
            // A category sheet of a group no sheet names is ignored.
            (
                "AnimeCategory_00_05".to_owned(),
                sheet(&[&["id", "name"], &["9", "椅子"]]),
            ),
            (
                "Anime_00_00_03".to_owned(),
                sheet(&[
                    &[],
                    &["no", "group", "category", "name", "bundle", "file", "clip"],
                    &[
                        "0",
                        "0",
                        "3",
                        "歩く1",
                        "studio/anime/00.unity3d",
                        "walk_run",
                        "f_aruki_00",
                    ],
                    &[
                        "1",
                        "0",
                        "3",
                        "走る1",
                        "studio/anime/00.unity3d",
                        "walk_run",
                        "f_hasiru_00",
                    ],
                    &["x", "0", "3", "broken"],
                    &["2", "0", "3", "歩く2", "studio/anime/77.unity3d", "walk_run", "missing"],
                ]),
            ),
            (
                "Anime_00_00_04".to_owned(),
                sheet(&[
                    &[],
                    &[],
                    &["0", "0", "4", "立ち1", "studio/anime/00.unity3d", "tachi", "f_tachi_00"],
                ]),
            ),
        ];
        let mut library = Library::default();
        library.add_bundle(Some("00"), &sheets, &Source::Game, &|b| b == "studio/anime/00.unity3d");
        library
    }

    /// The H scenes' sheets carry a second bundle and file before the
    /// clip, so the state is column 8, and the game reads them after its
    /// own sheets: a row of theirs under numbers already taken wins.
    #[test]
    fn the_h_scenes_sheets_have_their_own_columns_and_come_last() {
        let header = &[
            "no", "group", "category", "name", "bundle", "file", "bundle", "file", "clip",
        ];
        let sheets = vec![
            ("AnimeGroup_00".to_owned(), sheet(&[&["id", "name"], &["1", "H愛撫"]])),
            (
                "AnimeCategory_00_01".to_owned(),
                sheet(&[&["id", "name"], &["100", "立ち"]]),
            ),
            // Read first, although its name sorts later.
            (
                "Anime_00_01_100".to_owned(),
                sheet(&[
                    &[],
                    &["no", "group", "category", "name", "bundle", "file", "clip"],
                    &[
                        "0",
                        "1",
                        "100",
                        "歩く1",
                        "studio/anime/00.unity3d",
                        "walk_run",
                        "f_aruki_00",
                    ],
                ]),
            ),
            (
                "HAnime_00_01_100".to_owned(),
                sheet(&[
                    &[],
                    header,
                    &[
                        "0",
                        "1",
                        "100",
                        "待機",
                        "h/anim/female/00_00_00.unity3d",
                        "kha_f_base",
                        "h/anim/female/00_00_00.unity3d",
                        "kha_f_00",
                        "Idle",
                    ],
                    // Its bundle is not installed.
                    &[
                        "1",
                        "1",
                        "100",
                        "胸触り",
                        "h/anim/female/99.unity3d",
                        "kha_f_base",
                        "",
                        "",
                        "M_Idle",
                    ],
                ]),
            ),
        ];
        let mut library = Library::default();
        library.add_bundle(Some("00"), &sheets, &Source::Game, &|b| b != "h/anim/female/99.unity3d");
        assert_eq!(library.entries.len(), 1);
        let pose = &library.entries[0];
        assert_eq!(pose.original, "待機");
        assert_eq!(pose.bundle, "h/anim/female/00_00_00.unity3d");
        assert_eq!(pose.controller, "kha_f_base");
        // Column 8, not the second bundle in 6.
        assert_eq!(pose.state, "Idle");
        // Columns 6 and 7 name the controller whose clips replace the
        // base's. A whole category of the H scenes shares one base
        // controller and one state name, so this is the only thing that
        // tells its animations apart.
        assert_eq!(pose.override_bundle, "h/anim/female/00_00_00.unity3d");
        assert_eq!(pose.override_controller, "kha_f_00");
        assert_eq!(pose.over(), Some(("h/anim/female/00_00_00.unity3d", "kha_f_00")));
        // The groups and categories are the game's own, shared with Studio.
        assert_eq!(library.category_name(1, 100), "H foreplay: Stand");
    }

    #[test]
    fn charastudios_own_sheets_name_no_second_controller() {
        let library = library();
        let pose = &library.entries[0];
        assert_eq!(pose.state, "f_aruki_00");
        assert_eq!(pose.over(), None, "column 6 is the clip, not a bundle");
    }

    #[test]
    fn a_mod_adds_a_group_of_its_own() {
        let mut library = library();
        // The files of a mod: any sheet of groups counts, and the
        // animations may be clips of the game played under a new name.
        let sheets = vec![
            ("AnimeGroup_MASCULINE".to_owned(), sheet(&[&[], &["6549", "MASCULINE"]])),
            (
                "AnimeCategory_00_6549".to_owned(),
                sheet(&[&[], &["65490", "Masculine"]]),
            ),
            (
                "Anime_00_6549_65490".to_owned(),
                sheet(&[
                    &[],
                    &[],
                    &[
                        "0",
                        "6549",
                        "65490",
                        "Masculine 01",
                        "custom/custompose.unity3d",
                        "custom_pose",
                        "Mstand_00",
                    ],
                    &["1", "0", "4", "立ち9", "studio/anime/00.unity3d", "tachi", "extra"],
                ]),
            ),
        ];
        library.add_bundle(None, &sheets, &Source::Mod("Masculine".into()), &|_| true);
        let own = library.find(6549, 65490, 0).unwrap();
        assert_eq!((own.name.as_str(), own.state.as_str()), ("Masculine 01", "Mstand_00"));
        assert_eq!(library.category_name(6549, 65490), "MASCULINE: Masculine");
        // An entry with the numbers of one of the game's categories joins it.
        assert_eq!(library.find(0, 4, 1).unwrap().name, "Stand 9");
        library.entries.sort_by_key(|p| (p.group, p.category, p.number));
        assert_eq!(
            library.groups(),
            [(0, "Character".to_owned(), 4), (6549, "MASCULINE".to_owned(), 1)]
        );
        assert_eq!(
            library.find(6549, 65490, 0).unwrap().source,
            Source::Mod("Masculine".into())
        );
        // The tree for browsing goes by names.
        assert_eq!(
            library.tree(),
            [
                Branch {
                    name: "Character".into(),
                    count: 4,
                    categories: vec![("Walk and run".into(), 2), ("Stand".into(), 2)],
                },
                Branch {
                    name: "MASCULINE".into(),
                    count: 1,
                    categories: vec![("Masculine".into(), 1)],
                },
            ]
        );
        // A bundle's own sheet of groups must carry the bundle's name.
        let mut strict = Library::default();
        strict.add_bundle(Some("00"), &sheets, &Source::Game, &|_| true);
        assert!(strict.groups.is_empty());
    }

    #[test]
    fn the_list_is_read_from_the_sheets() {
        let library = library();
        let names: Vec<&str> = library.entries.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Walk 1", "Run 1", "Stand 1"]);
        let walk = library.find(0, 3, 0).unwrap();
        assert_eq!(
            (walk.bundle.as_str(), walk.controller.as_str(), walk.state.as_str()),
            ("studio/anime/00.unity3d", "walk_run", "f_aruki_00")
        );
        assert_eq!(walk.original, "歩く1");
        assert_eq!(
            library.categories(),
            [
                Category {
                    group: 0,
                    category: 3,
                    name: "Character: Walk and run".into(),
                    count: 2
                },
                Category {
                    group: 0,
                    category: 4,
                    name: "Character: Stand".into(),
                    count: 1
                },
            ]
        );
        assert_eq!(library.category_name(0, 9), "Character: Category 9");
    }

    #[test]
    fn a_later_bundle_replaces_an_entry() {
        let mut library = library();
        let sheets = vec![(
            "Anime_01_00_03".to_owned(),
            sheet(&[
                &[],
                &[],
                &[
                    "1",
                    "0",
                    "3",
                    "走る2",
                    "studio/anime/00.unity3d",
                    "walk_run",
                    "f_hasiru_new",
                ],
            ]),
        )];
        library.add_bundle(Some("01"), &sheets, &Source::Game, &|_| true);
        assert_eq!(library.entries.len(), 3);
        assert_eq!(library.find(0, 3, 1).unwrap().state, "f_hasiru_new");
    }

    #[test]
    fn categories_of_the_same_name_are_one_whatever_their_numbers() {
        // Two packs: one adds to the game's group 0 under a new category
        // number but an existing name, the other brings a group of its own.
        let mut library = library();
        let sheets = vec![
            ("AnimeGroup_x".to_owned(), sheet(&[&[], &["900", "character"]])),
            (
                "AnimeCategory_00_900".to_owned(),
                sheet(&[&[], &["1", " STAND "], &["2", "Dance"]]),
            ),
            (
                "Anime_00_900_1".to_owned(),
                sheet(&[&[], &[], &["0", "900", "1", "Lean", "pack/a.unity3d", "c", "lean"]]),
            ),
            (
                "Anime_00_900_2".to_owned(),
                sheet(&[&[], &[], &["0", "900", "2", "Spin", "pack/a.unity3d", "c", "spin"]]),
            ),
        ];
        library.add_bundle(None, &sheets, &Source::Package("dance-pack".into()), &|_| true);
        assert_eq!(
            library.tree(),
            [Branch {
                name: "Character".into(),
                count: 5,
                categories: vec![("Walk and run".into(), 2), ("Stand".into(), 2), ("Dance".into(), 1)],
            }]
        );
        assert_eq!(library.find(900, 1, 0).unwrap().source.label(), "dance-pack");
    }
}
