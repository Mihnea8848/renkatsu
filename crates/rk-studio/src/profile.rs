// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Who a character is besides looks: name, personality, birthday, club,
//! traits and likes, as the game's character maker sets them. A card
//! brings them, and a character in a scene may change them for itself;
//! the card is never written. The numbers a card stores are named from
//! the game's own lists (`etcetra/list/config` for personalities,
//! `action/list/clubinfo` for clubs), translated to English.

use rk_card::{map_get, Value};
use serde::{Deserialize, Serialize};

/// Personalities by the number a card stores, in the game's order.
pub const PERSONALITIES: [&str; 39] = [
    "Sexy",
    "Ojousama",
    "Haughty",
    "Kouhai",
    "Mysterious",
    "Weirdo",
    "Yamato Nadeshiko",
    "Tomboy",
    "Pure",
    "Simple",
    "Delusional",
    "Motherly",
    "Big sister",
    "Gyaru",
    "Delinquent",
    "Wild",
    "Wannabe",
    "Contrary",
    "Unlucky",
    "Bookish",
    "Timid",
    "Classic heroine",
    "Trendy",
    "Otaku",
    "Yandere",
    "Lazy",
    "Quiet",
    "Stubborn",
    "Old-fashioned",
    "Frank and cool",
    "Easygoing",
    "Strong-willed",
    "Sincere",
    "Glamorous",
    "Returnee",
    "Dialect speaker",
    "Teasing",
    "Emotionless",
    "Meticulous",
];

pub const BLOOD_TYPES: [&str; 4] = ["A", "B", "O", "AB"];

/// Clubs by number; male cards are always in the Koikatsu club.
pub const CLUBS: [&str; 6] = [
    "Going-home club",
    "Swimming club",
    "Manga club",
    "Cheerleading club",
    "Tea ceremony club",
    "Track club",
];

pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The traits shown, by their key in the card, in the maker's order.
pub const TRAITS: [(&str, &str); 12] = [
    ("harapeko", "Always hungry"),
    ("donkan", "Oblivious"),
    ("dokusyo", "Bookworm"),
    ("ongaku", "Music lover"),
    ("kappatu", "Lively"),
    ("friendly", "Friendly"),
    ("kireizuki", "Neat freak"),
    ("taida", "Lazy"),
    ("sinsyutu", "Elusive"),
    ("hitori", "Loner"),
    ("undo", "Athletic"),
    ("majime", "Serious"),
];

/// The traits the maker keeps for its H page, by their key in the
/// card's `attribute` block, in the maker's order. The game lists them
/// beside [`TRAITS`], in one list; they are kept apart here so that what
/// a character is like and what it is like in bed are two pages.
pub const H_TRAITS: [(&str, &str); 6] = [
    ("hinnyo", "Weak bladder"),
    ("choroi", "Easily swayed"),
    ("bitch", "Promiscuous"),
    ("mutturi", "Secretly lewd"),
    ("ukemi", "Passive"),
    ("likeGirls", "Likes girls"),
];

/// What the character will not do, by their key in the card's `denial`
/// block, in the maker's order (`ChaCustom.CvsH`).
pub const DENIAL: [(&str, &str); 5] = [
    ("kiss", "Kissing"),
    ("aibu", "Caressing"),
    ("anal", "Anal"),
    ("massage", "Massage"),
    ("notCondom", "Without a condom"),
];

/// Where the character is weakest, by the number the card stores; -1,
/// shown as Not set, is none. The game's touch areas in the order
/// `HandCtrl` maps them, the breasts twice: the breasts themselves, and
/// the nipples, which it tells apart by the object on the area.
pub const WEAK_POINTS: [&str; 6] = ["Mouth", "Breasts", "Crotch", "Anus", "Bottom", "Nipples"];

/// The maker's questions, by their key in the card (the game spells the
/// block `awnser`).
pub const LIKES: [(&str, &str); 9] = [
    ("animal", "Animals"),
    ("eat", "Eating"),
    ("cook", "Cooking"),
    ("exercise", "Exercise"),
    ("study", "Studying"),
    ("fashionable", "Fashion"),
    ("blackCoffee", "Black coffee"),
    ("spicy", "Spicy food"),
    ("sweet", "Sweets"),
];

/// The profile, with the card's numbers and keys, so that a value the
/// lists do not know survives an edit of another one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub last_name: String,
    pub first_name: String,
    pub nickname: String,
    /// Male cards have no personality, traits or likes in the game, and
    /// are always in the Koikatsu club. Always the card's.
    #[serde(skip)]
    pub male: bool,
    /// The number in [`PERSONALITIES`].
    pub personality: i64,
    /// Voice pitch, 0 to 100.
    pub pitch: f32,
    /// 1 to 12, or 0 when not set.
    pub birth_month: i64,
    pub birth_day: i64,
    /// The number in [`BLOOD_TYPES`].
    pub blood_type: i64,
    /// The number in [`CLUBS`].
    pub club: i64,
    /// The keys of [`TRAITS`] the character has. Traits Renkatsu does not
    /// show are kept as they are.
    pub traits: Vec<String>,
    /// The keys of [`H_TRAITS`] it has, out of the same block as
    /// `traits`.
    pub h_traits: Vec<String>,
    /// The keys of [`DENIAL`] it will not do.
    pub denial: Vec<String>,
    /// The number in [`WEAK_POINTS`], or -1 for none.
    pub weak_point: i64,
    /// The keys of [`LIKES`] the character likes.
    pub likes: Vec<String>,
    /// Traits added for this character, beyond the game's.
    pub custom_traits: Vec<Custom>,
    /// H traits added for this character, beyond the game's.
    pub custom_h_traits: Vec<Custom>,
    /// Likes added for this character, beyond the game's.
    pub custom_likes: Vec<Custom>,
    /// A male card's aggressiveness, diligence and kindness, -5 to 5.
    pub stats: [i64; 3],
}

impl Default for Profile {
    fn default() -> Profile {
        Profile {
            last_name: String::new(),
            first_name: String::new(),
            nickname: String::new(),
            male: false,
            personality: 0,
            pitch: 50.0,
            birth_month: 0,
            birth_day: 0,
            blood_type: 0,
            club: 0,
            traits: Vec::new(),
            h_traits: Vec::new(),
            denial: Vec::new(),
            weak_point: -1,
            likes: Vec::new(),
            custom_traits: Vec::new(),
            custom_h_traits: Vec::new(),
            custom_likes: Vec::new(),
            stats: [0; 3],
        }
    }
}

impl Profile {
    /// Reads a card's Parameter block.
    pub fn read(param: &Value) -> Profile {
        let int = |key| map_get(param, key).and_then(Value::as_i64).unwrap_or(0);
        let text = |key| {
            map_get(param, key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_owned()
        };
        let keys = |block| -> Vec<String> {
            match map_get(param, block) {
                Some(Value::Map(pairs)) => pairs
                    .iter()
                    .filter(|(_, v)| v.as_bool() == Some(true))
                    .filter_map(|(k, _)| k.as_str().map(str::to_owned))
                    .collect(),
                _ => Vec::new(),
            }
        };
        let pitch = map_get(param, "voiceRate").and_then(|v| v.as_f64()).unwrap_or(0.5);
        // The H traits sit in the same block as the others; the pages
        // part them, so the card's block is read in two.
        let attribute = keys("attribute");
        let h_trait = |key: &String| H_TRAITS.iter().any(|(k, _)| k == key);
        Profile {
            last_name: text("lastname"),
            first_name: text("firstname"),
            nickname: text("nickname"),
            male: map_get(param, "sex").and_then(Value::as_i64) == Some(0),
            personality: int("personality"),
            pitch: (pitch.clamp(0.0, 1.0) * 100.0).round() as f32,
            birth_month: int("birthMonth"),
            birth_day: int("birthDay"),
            blood_type: int("bloodType"),
            club: int("clubActivities"),
            traits: attribute.iter().filter(|k| !h_trait(k)).cloned().collect(),
            h_traits: attribute.iter().filter(|k| h_trait(k)).cloned().collect(),
            denial: keys("denial"),
            weak_point: map_get(param, "weakPoint").and_then(Value::as_i64).unwrap_or(-1),
            likes: keys("awnser"),
            custom_traits: Vec::new(),
            custom_h_traits: Vec::new(),
            custom_likes: Vec::new(),
            stats: [int("aggressive"), int("diligence"), int("kindness")],
        }
    }

    /// Adds or takes away `key` in a list of keys.
    pub fn set(list: &mut Vec<String>, key: &str, on: bool) {
        list.retain(|k| k != key);
        if on {
            list.push(key.to_owned());
        }
    }

    /// The days the birth month has, 31 when no month is set; February
    /// has 29, as the game's maker allows.
    pub fn days(&self) -> i64 {
        match self.birth_month {
            2 => 29,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        }
    }
}

/// A trait or a like the game does not have, named by the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Custom {
    pub name: String,
    /// Whether the character has the trait, or likes the thing.
    pub on: bool,
}

/// Adds `name` to `custom` unless it is empty or already there, in the
/// game's names or the added ones, whatever the capitals. Returns whether
/// it was added.
pub fn add_custom(custom: &mut Vec<Custom>, game: &[(&str, &str)], name: &str, on: bool) -> bool {
    let name = name.trim();
    let taken = |other: &str| other.trim().eq_ignore_ascii_case(name);
    if name.is_empty() || game.iter().any(|(_, g)| taken(g)) || custom.iter().any(|c| taken(&c.name)) {
        return false;
    }
    custom.push(Custom {
        name: name.to_owned(),
        on,
    });
    true
}

/// The names of a list for a dropdown, and the place of `n` in it. A
/// number the list does not have gets a place of its own at the end, so
/// that it shows and is kept until something else is chosen.
pub fn options(list: &[&str], n: i64) -> (Vec<String>, usize) {
    let mut names: Vec<String> = list.iter().map(|s| (*s).to_owned()).collect();
    match usize::try_from(n).ok().filter(|&i| i < list.len()) {
        Some(i) => (names, i),
        None => {
            names.push(format!("Number {n}"));
            (names, list.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_custom_name_is_added_once() {
        let mut custom = Vec::new();
        assert!(add_custom(&mut custom, &LIKES, " Jazz ", true));
        assert_eq!(custom[0].name, "Jazz");
        assert!(!add_custom(&mut custom, &LIKES, "jazz", true), "the same name twice");
        assert!(!add_custom(&mut custom, &LIKES, "sweets", true), "a name the game has");
        assert!(!add_custom(&mut custom, &LIKES, "  ", true), "no name");
        assert_eq!(custom.len(), 1);
    }

    #[test]
    fn the_lists_follow_the_game() {
        assert_eq!(PERSONALITIES.len(), 39);
        assert_eq!(PERSONALITIES[8], "Pure");
        assert_eq!(PERSONALITIES[38], "Meticulous");
        assert_eq!(BLOOD_TYPES[2], "O");
        // The maker shows all eighteen attributes in one list; Renkatsu
        // shows the same eighteen over two pages, none twice.
        assert_eq!(TRAITS.len() + H_TRAITS.len(), 18);
        assert!(!TRAITS.iter().any(|(k, _)| H_TRAITS.iter().any(|(h, _)| h == k)));
        assert_eq!(WEAK_POINTS.len(), 6);
    }

    #[test]
    fn reads_a_parameter_block() {
        let map = |pairs: Vec<(&str, Value)>| Value::Map(pairs.into_iter().map(|(k, v)| (Value::from(k), v)).collect());
        let param = map(vec![
            ("lastname", Value::from("春野")),
            ("firstname", Value::from("千佳")),
            ("nickname", Value::from("ちかりん")),
            ("sex", Value::from(1)),
            ("personality", Value::from(8)),
            ("bloodType", Value::from(2)),
            ("birthMonth", Value::from(5)),
            ("birthDay", Value::from(17)),
            ("clubActivities", Value::from(9)),
            ("voiceRate", Value::from(0.5)),
            (
                "attribute",
                map(vec![
                    ("dokusyo", Value::from(true)),
                    ("majime", Value::from(true)),
                    ("undo", Value::from(false)),
                    ("mutturi", Value::from(true)),
                    ("bitch", Value::from(false)),
                ]),
            ),
            (
                "denial",
                map(vec![("kiss", Value::from(true)), ("anal", Value::from(false))]),
            ),
            ("weakPoint", Value::from(2)),
            ("awnser", map(vec![("sweet", Value::from(true))])),
        ]);
        let p = Profile::read(&param);
        assert_eq!((p.last_name.as_str(), p.first_name.as_str()), ("春野", "千佳"));
        assert!(!p.male);
        assert_eq!((p.personality, p.blood_type, p.birth_month, p.birth_day), (8, 2, 5, 17));
        assert_eq!(p.pitch, 50.0);
        // One block, two pages: the H traits are kept apart from the rest.
        assert_eq!(p.traits, ["dokusyo", "majime"]);
        assert_eq!(p.h_traits, ["mutturi"]);
        assert_eq!(p.denial, ["kiss"]);
        assert_eq!(p.weak_point, 2);
        assert_eq!(p.likes, ["sweet"]);
        // A card that says nothing about a weak point has none.
        assert_eq!(Profile::read(&Value::Map(Vec::new())).weak_point, -1);
    }

    #[test]
    fn a_number_the_list_lacks_is_kept() {
        let (names, at) = options(&CLUBS, 9);
        assert_eq!(names.len(), CLUBS.len() + 1);
        assert_eq!(names[at], "Number 9");
        assert_eq!(options(&CLUBS, 2).1, 2);
    }

    #[test]
    fn edits_survive_a_scene_file() {
        let mut p = Profile {
            first_name: "Yukie".into(),
            club: 3,
            ..Profile::default()
        };
        Profile::set(&mut p.traits, "undo", true);
        Profile::set(&mut p.h_traits, "ukemi", true);
        Profile::set(&mut p.denial, "notCondom", true);
        p.weak_point = 5;
        assert!(add_custom(&mut p.custom_likes, &LIKES, "Jazz", true));
        assert!(add_custom(&mut p.custom_traits, &TRAITS, "Early riser", false));
        let text = toml::to_string(&p).unwrap();
        let back: Profile = toml::from_str(&text).unwrap();
        assert_eq!(back, p);
    }
}
