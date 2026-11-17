// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! CharaStudio's list of animations, read as `Studio.Info` reads it.
//!
//! Every bundle under `studio/info/` may hold sheets: `AnimeGroup_<file>`
//! names the groups, `AnimeCategory_<n>_<group>` the categories of a
//! group, and `Anime_<n>_<group>_<category>` the animations: a number, the
//! group, the category, the name shown, the bundle, the controller in it
//! and the state to play. Bundles are read in name order and a later
//! sheet replaces an earlier one's entry. An animation whose bundle is not
//! installed is left out, as the game leaves it out.
//!
//! The `HAnime_` sheets, which list the H scenes' animations for Studio,
//! are never read, and an entry that points into `h/` is dropped.
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
pub const POSE_SECONDS: f32 = 0.2;

/// One animation of the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Pose {
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
    /// How long it is and whether it loops, once its clips were read.
    pub timing: Option<Timing>,
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
        !self.loops && self.seconds <= POSE_SECONDS
    }
}

impl Pose {
    /// Whether the animation holds one pose; `None` until its clips were
    /// read.
    pub fn is_static(&self) -> Option<bool> {
        self.timing.map(|t| t.is_static())
    }
}

/// The whole list: the animations with the names of their groups and
/// categories.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Library {
    pub poses: Vec<Pose>,
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

/// Content that is never listed: the H scenes' animations.
fn never(bundle: &str) -> bool {
    let bundle = bundle.replace('\\', "/").to_lowercase();
    bundle.starts_with("h/") || bundle.starts_with("sound/")
}

impl Library {
    /// Adds the sheets of one `studio/info` bundle, or of one folder of
    /// list files of a mod. `sheets` are the sheets by name and
    /// `installed` says whether a bundle exists. A bundle's sheet of
    /// groups is the one named after the bundle, `AnimeGroup_<stem>`; a
    /// mod's folder has no such name to go by (`stem` is `None`) and every
    /// `AnimeGroup_` sheet in it counts, as for Sideloader.
    pub fn add_bundle(&mut self, stem: Option<&str>, sheets: &[(String, Rows)], installed: &dyn Fn(&str) -> bool) {
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
        // Animation sheets by group, category, then number.
        let mut ordered: Vec<(Vec<i32>, &Rows)> = sheets
            .iter()
            .filter_map(|(n, s)| Some((sheet_numbers(n, "Anime_").filter(|v| v.len() == 3)?, s)))
            .collect();
        ordered.sort_by_key(|(n, _)| (n[1], n[2], n[0]));
        for (_, sheet) in ordered {
            for row in rows(sheet).skip(2) {
                let cell = |i: usize| row.get(i).copied().unwrap_or_default();
                let number = |i: usize| cell(i).trim().parse::<i32>().ok();
                let (Some(no), Some(group), Some(category)) = (number(0), number(1), number(2)) else {
                    continue;
                };
                let bundle = cell(4);
                if never(bundle) || !installed(bundle) {
                    continue;
                }
                let pose = Pose {
                    group,
                    category,
                    number: no,
                    original: cell(3).to_owned(),
                    name: english(cell(3)),
                    bundle: bundle.to_owned(),
                    controller: cell(5).to_owned(),
                    state: cell(6).to_owned(),
                    timing: None,
                };
                match self
                    .poses
                    .iter_mut()
                    .find(|p| (p.group, p.category, p.number) == (group, category, no))
                {
                    Some(old) => *old = pose,
                    None => self.poses.push(pose),
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
                        if !name.to_ascii_lowercase().starts_with("anime") {
                            continue;
                        }
                        if let Ok(sheet) = file.sf.read(info) {
                            sheets.push((name, sheet_rows(&sheet)));
                        }
                    }
                }
            }
            library.add_bundle(Some(&stem), &sheets, &|bundle| {
                game.assets.index.bundles.contains_key(&rk_vfs::index::norm(bundle))
            });
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
            library.add_bundle(None, &sheets, &|bundle| {
                game.assets.index.bundles.contains_key(&rk_vfs::index::norm(bundle))
            });
        }
        library.poses.sort_by_key(|p| (p.group, p.category, p.number));
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
        // A middling body, where blends by height sit between their clips.
        let parameters = crate::motion::Parameters {
            height: 0.5,
            breast: 0.5,
        };
        while self.timed < self.poses.len() {
            if enough() {
                return false;
            }
            let pose = &mut self.poses[self.timed];
            let timing = crate::motion::load(game, &pose.bundle, &pose.controller, &pose.state, &parameters)
                .map(|m| Timing {
                    seconds: m.duration(),
                    loops: m.loops(),
                })
                .unwrap_or(Timing {
                    seconds: 0.0,
                    loops: false,
                });
            pose.timing = Some(timing);
            self.timed += 1;
        }
        true
    }

    /// The categories that have animations, in the game's order.
    pub fn categories(&self) -> Vec<Category> {
        let mut out: Vec<Category> = Vec::new();
        for pose in &self.poses {
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

    /// The animation CharaStudio has under these numbers.
    pub fn find(&self, group: i32, category: i32, number: i32) -> Option<&Pose> {
        self.poses
            .iter()
            .find(|p| (p.group, p.category, p.number) == (group, category, number))
    }
}

impl Library {
    /// The groups that have animations, in the game's order: the group's
    /// number, its name in English and how many animations it has.
    pub fn groups(&self) -> Vec<(i32, String, usize)> {
        let mut out: Vec<(i32, String, usize)> = Vec::new();
        for pose in &self.poses {
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
    ("初期ポーズ", "Default pose"),
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
    ("男", "Male"),
    ("女", "Female"),
];

/// A name of the game's lists in English: the words it is built from are
/// translated and what is left (numbers, letters) is kept, with full
/// width digits and letters made plain. A name with a word that is not
/// known stays as the game has it.
pub fn english(name: &str) -> String {
    let plain: String = name
        .chars()
        .map(|c| match c {
            '０'..='９' | 'Ａ'..='Ｚ' | 'ａ'..='ｚ' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
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
        if c.is_ascii_alphanumeric() && after_word && !out.ends_with(' ') {
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
                    &["3", "0", "3", "歩く3", "h/anim/female/00.unity3d", "walk_run", "never"],
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
            // The H scenes' sheets are not read at all.
            (
                "HAnime_00_01_100".to_owned(),
                sheet(&[&[], &[], &["0", "1", "100", "x", "studio/anime/00.unity3d", "h", "h"]]),
            ),
        ];
        let mut library = Library::default();
        library.add_bundle(Some("00"), &sheets, &|b| b == "studio/anime/00.unity3d");
        library
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
        library.add_bundle(None, &sheets, &|_| true);
        let own = library.find(6549, 65490, 0).unwrap();
        assert_eq!((own.name.as_str(), own.state.as_str()), ("Masculine 01", "Mstand_00"));
        assert_eq!(library.category_name(6549, 65490), "MASCULINE: Masculine");
        // An entry with the numbers of one of the game's categories joins it.
        assert_eq!(library.find(0, 4, 1).unwrap().name, "Stand 9");
        library.poses.sort_by_key(|p| (p.group, p.category, p.number));
        assert_eq!(
            library.groups(),
            [(0, "Character".to_owned(), 4), (6549, "MASCULINE".to_owned(), 1)]
        );
        // A bundle's own sheet of groups must carry the bundle's name.
        let mut strict = Library::default();
        strict.add_bundle(Some("00"), &sheets, &|_| true);
        assert!(strict.groups.is_empty());
    }

    #[test]
    fn the_list_is_read_from_the_sheets() {
        let library = library();
        let names: Vec<&str> = library.poses.iter().map(|p| p.name.as_str()).collect();
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
        library.add_bundle(Some("01"), &sheets, &|_| true);
        assert_eq!(library.poses.len(), 3);
        assert_eq!(library.find(0, 3, 1).unwrap().state, "f_hasiru_new");
    }

    #[test]
    fn h_content_is_never_listed() {
        assert!(never("h/anim/female/02_00_00.unity3d"));
        assert!(never("H\\anim\\male\\01.unity3d"));
        assert!(!never("studio/anime/00.unity3d"));
    }
}
