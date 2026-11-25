// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The kind of cloth a skirt is taken to be made of when nothing says:
//! a guess from what the game's files hold about the garment. Renkatsu's
//! own, not the game's.

use super::drape::Kind;

/// What is known of a skirt before it moves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Garment {
    /// The meshes that hang on the skirt's net, with how many of their
    /// vertices do: `o_bot_skirt01_a`.
    pub meshes: Vec<(String, usize)>,
    /// The game's chains round the waist and the most particles in one.
    pub chains: usize,
    pub particles: usize,
    /// The net's columns and rows.
    pub cols: usize,
    pub rows: usize,
    /// From the waist to the hem along the net, metres (mean of the
    /// columns).
    pub length: f32,
    /// How far the first and the last row are from the net's axis, metres
    /// (means).
    pub top: f32,
    pub hem: f32,
    /// The game's DynamicBone settings at the chains' ends (means).
    pub stiffness: f32,
    pub elasticity: f32,
    pub damping: f32,
}

impl Garment {
    /// How much the skirt widens per length down it.
    pub fn flare(&self) -> f32 {
        if self.length > 0.0 {
            (self.hem - self.top) / self.length
        } else {
            0.0
        }
    }

    /// One line for `rk render`.
    pub fn line(&self) -> String {
        let (kind, why) = guess(self);
        format!(
            "{}x{} chains, net {}x{}, {:.0} cm long, flare {:.2}, stiffness {:.2} elasticity {:.2} damping {:.2}: {} ({why})",
            self.chains,
            self.particles,
            self.cols,
            self.rows,
            self.length * 100.0,
            self.flare(),
            self.stiffness,
            self.elasticity,
            self.damping,
            kind.name(),
        )
    }
}

/// Words in a mesh's name that tell its kind, looked for in this
/// order. Names of the game's own clothes (`cf_o_bot_china_d`,
/// `o_bot_skirt_frea`, `cf_o_bot_cheerSK_d`) and what mods call theirs.
const WORDS: [(Kind, &[&str]); 4] = [
    (
        Kind::Fixed,
        &["tutu", "pannier", "petticoat", "crinoline", "balloon", "hoop"],
    ),
    (
        Kind::Tight,
        &["china", "qipao", "cheongsam", "tight", "pencil", "mermaid"],
    ),
    (
        Kind::Pleated,
        &["pleat", "cheer", "frea", "flare", "frill", "sailor", "seifuku"],
    ),
    (
        Kind::Flowing,
        &["dress", "onepiece", "one_piece", "gown", "yukata", "kimono"],
    ),
];
/// The game's DynamicBone stiffness (how much a chain keeps the way it
/// was posed, 0 to 1) from which a skirt is taken to hold its shape,
/// and from which to stand by itself. The game's school skirt, long
/// dress, cheer skirt and qipao have 0.06 to 0.09 at the hem; its flared
/// skirt has 0.6.
const HOLDS: f32 = 0.3;
const STANDS: f32 = 0.9;
/// From this length down the net a skirt reaches past the knee: the
/// game's short skirts are 27 to 29 cm, its long dress 50 cm.
const LONG: f32 = 0.45;

/// The kind of `garment` and what it was told by:
/// 1. a word in the name of one of its meshes, the mesh with the most
///    vertices on the net first;
/// 2. the game's own setting for how much the skirt keeps its shape;
/// 3. its length: long cloth flows, short cloth keeps its shape.
pub fn guess(garment: &Garment) -> (Kind, &'static str) {
    let mut meshes: Vec<&(String, usize)> = garment.meshes.iter().collect();
    meshes.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    for (name, _) in meshes {
        let name = name.to_lowercase();
        for (kind, words) in WORDS {
            if words.iter().any(|w| name.contains(w)) {
                return (kind, "its name");
            }
        }
    }
    if garment.stiffness >= STANDS {
        (Kind::Fixed, "the game keeps it in its shape")
    } else if garment.stiffness >= HOLDS {
        (Kind::Pleated, "the game holds it to its shape")
    } else if garment.length >= LONG {
        (Kind::Flowing, "it is long")
    } else {
        (Kind::Pleated, "it is short")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn garment(meshes: &[(&str, usize)], length: f32, stiffness: f32) -> Garment {
        Garment {
            meshes: meshes.iter().map(|(n, c)| (n.to_string(), *c)).collect(),
            chains: 8,
            particles: 4,
            cols: 24,
            rows: 7,
            length,
            top: 0.12,
            hem: 0.2,
            stiffness,
            elasticity: 0.01,
            damping: 0.15,
        }
    }

    #[test]
    fn a_name_tells_the_kind_first() {
        let kind = |name: &str| guess(&garment(&[(name, 100)], 0.6, 0.06)).0;
        assert_eq!(kind("cf_o_bot_china_d"), Kind::Tight);
        assert_eq!(kind("cf_o_bot_cheerSK_d"), Kind::Pleated);
        assert_eq!(kind("o_bot_skirt_frea"), Kind::Pleated);
        assert_eq!(kind("o_bot_tutu_a"), Kind::Fixed);
        assert_eq!(kind("o_top_onepiece_b"), Kind::Flowing);
        // The mesh with the most of the skirt decides, not a jacket's
        // hem on the same net.
        let mixed = garment(&[("o_top_gown_a", 90), ("o_bot_pleats", 600)], 0.3, 0.06);
        assert_eq!(guess(&mixed), (Kind::Pleated, "its name"));
    }

    #[test]
    fn without_a_name_the_game_settings_and_the_length_tell() {
        let kind = |length: f32, stiffness: f32| guess(&garment(&[("o_bot_skirt01_a", 675)], length, stiffness)).0;
        // The school skirt and the long dress of the game.
        assert_eq!(kind(0.28, 0.08), Kind::Pleated);
        assert_eq!(kind(0.50, 0.06), Kind::Flowing);
        // Long but held to its shape by the game, and one kept in it.
        assert_eq!(kind(0.50, 0.6), Kind::Pleated);
        assert_eq!(kind(0.28, 0.95), Kind::Fixed);
    }

    #[test]
    fn the_flare_is_the_widening_per_length() {
        let g = garment(&[], 0.4, 0.1);
        assert!((g.flare() - 0.2).abs() < 1e-6);
        assert_eq!(Garment::default().flare(), 0.0);
        assert!(g.line().contains("net 24x7"));
    }
}
