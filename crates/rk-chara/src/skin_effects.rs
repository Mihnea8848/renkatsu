// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Additional Skin Effects plugin (`KK_SkinEffects`, `Marco.SkinEffects`):
//! liquids, wetness, blush and tears drawn on the skin, at a level each.
//!
//! In Studio each effect has a level (`SkinEffectsGui.RegisterStudioControls`).
//! A level above 0 adds the plugin's picture for it to the skin overlays of
//! the body or the face (`SkinEffectsController.ApplyEffects`), as a
//! `KoiSkinOverlayX` additional texture with apply order 100 plus the
//! effect's number; KSOX lays the card's own overlay first and then the
//! additional textures by that order (`KoiSkinOverlayController`). The
//! pictures are the plugin's own, kept in `data/skin_effects` under GPL-3.0.

use crate::compose;
use crate::scene::Image;
use std::sync::{Arc, OnceLock};

/// An effect of the plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effect {
    /// Its name in the plugin (`SkinEffectKind`).
    pub key: &'static str,
    /// The name shown.
    pub name: &'static str,
    /// Drawn on the face rather than the body (`AffectsFaceAttribute`).
    pub face: bool,
    /// How many pictures, one a level.
    pub levels: u8,
}

/// The effects in the plugin's order, which is also their drawing order.
pub const EFFECTS: [Effect; 13] = [
    Effect {
        key: "VirginBloodBody",
        name: "Virgin blood",
        face: false,
        levels: 3,
    },
    Effect {
        key: "PussyJuiceBody",
        name: "Pussy juice",
        face: false,
        levels: 3,
    },
    Effect {
        key: "PussyBukkakeBody",
        name: "Pussy bukkake",
        face: false,
        levels: 3,
    },
    Effect {
        key: "AnalBukkakeBody",
        name: "Anal bukkake",
        face: false,
        levels: 3,
    },
    Effect {
        key: "WetBody",
        name: "Wet body",
        face: false,
        levels: 3,
    },
    Effect {
        key: "WetFace",
        name: "Wet face",
        face: true,
        levels: 3,
    },
    Effect {
        key: "BlushBody",
        name: "Blush body",
        face: false,
        levels: 3,
    },
    Effect {
        key: "BlushFace",
        name: "Blush face",
        face: true,
        levels: 3,
    },
    Effect {
        key: "ButtBlushBody",
        name: "Butt blush",
        face: false,
        levels: 3,
    },
    Effect {
        key: "TearFace",
        name: "Tears",
        face: true,
        levels: 3,
    },
    Effect {
        key: "DroolFace",
        name: "Drool",
        face: true,
        levels: 1,
    },
    Effect {
        key: "SalivaFace",
        name: "Saliva",
        face: true,
        levels: 3,
    },
    Effect {
        key: "CumInNoseFace",
        name: "Cum in nose",
        face: true,
        levels: 2,
    },
];

/// A level for each effect of `EFFECTS`, 0 for none.
pub type Levels = [u8; 13];

/// The pictures by `<key>_<level>`.
const PICTURES: &[(&str, &[u8])] = &[
    (
        "VirginBloodBody_01",
        include_bytes!("../data/skin_effects/VirginBloodBody_01.png"),
    ),
    (
        "VirginBloodBody_02",
        include_bytes!("../data/skin_effects/VirginBloodBody_02.png"),
    ),
    (
        "VirginBloodBody_03",
        include_bytes!("../data/skin_effects/VirginBloodBody_03.png"),
    ),
    (
        "PussyJuiceBody_01",
        include_bytes!("../data/skin_effects/PussyJuiceBody_01.png"),
    ),
    (
        "PussyJuiceBody_02",
        include_bytes!("../data/skin_effects/PussyJuiceBody_02.png"),
    ),
    (
        "PussyJuiceBody_03",
        include_bytes!("../data/skin_effects/PussyJuiceBody_03.png"),
    ),
    (
        "PussyBukkakeBody_01",
        include_bytes!("../data/skin_effects/PussyBukkakeBody_01.png"),
    ),
    (
        "PussyBukkakeBody_02",
        include_bytes!("../data/skin_effects/PussyBukkakeBody_02.png"),
    ),
    (
        "PussyBukkakeBody_03",
        include_bytes!("../data/skin_effects/PussyBukkakeBody_03.png"),
    ),
    (
        "AnalBukkakeBody_01",
        include_bytes!("../data/skin_effects/AnalBukkakeBody_01.png"),
    ),
    (
        "AnalBukkakeBody_02",
        include_bytes!("../data/skin_effects/AnalBukkakeBody_02.png"),
    ),
    (
        "AnalBukkakeBody_03",
        include_bytes!("../data/skin_effects/AnalBukkakeBody_03.png"),
    ),
    ("WetBody_01", include_bytes!("../data/skin_effects/WetBody_01.png")),
    ("WetBody_02", include_bytes!("../data/skin_effects/WetBody_02.png")),
    ("WetBody_03", include_bytes!("../data/skin_effects/WetBody_03.png")),
    ("WetFace_01", include_bytes!("../data/skin_effects/WetFace_01.png")),
    ("WetFace_02", include_bytes!("../data/skin_effects/WetFace_02.png")),
    ("WetFace_03", include_bytes!("../data/skin_effects/WetFace_03.png")),
    ("BlushBody_01", include_bytes!("../data/skin_effects/BlushBody_01.png")),
    ("BlushBody_02", include_bytes!("../data/skin_effects/BlushBody_02.png")),
    ("BlushBody_03", include_bytes!("../data/skin_effects/BlushBody_03.png")),
    ("BlushFace_01", include_bytes!("../data/skin_effects/BlushFace_01.png")),
    ("BlushFace_02", include_bytes!("../data/skin_effects/BlushFace_02.png")),
    ("BlushFace_03", include_bytes!("../data/skin_effects/BlushFace_03.png")),
    (
        "ButtBlushBody_01",
        include_bytes!("../data/skin_effects/ButtBlushBody_01.png"),
    ),
    (
        "ButtBlushBody_02",
        include_bytes!("../data/skin_effects/ButtBlushBody_02.png"),
    ),
    (
        "ButtBlushBody_03",
        include_bytes!("../data/skin_effects/ButtBlushBody_03.png"),
    ),
    ("TearFace_01", include_bytes!("../data/skin_effects/TearFace_01.png")),
    ("TearFace_02", include_bytes!("../data/skin_effects/TearFace_02.png")),
    ("TearFace_03", include_bytes!("../data/skin_effects/TearFace_03.png")),
    ("DroolFace_01", include_bytes!("../data/skin_effects/DroolFace_01.png")),
    (
        "SalivaFace_01",
        include_bytes!("../data/skin_effects/SalivaFace_01.png"),
    ),
    (
        "SalivaFace_02",
        include_bytes!("../data/skin_effects/SalivaFace_02.png"),
    ),
    (
        "SalivaFace_03",
        include_bytes!("../data/skin_effects/SalivaFace_03.png"),
    ),
    (
        "CumInNoseFace_01",
        include_bytes!("../data/skin_effects/CumInNoseFace_01.png"),
    ),
    (
        "CumInNoseFace_02",
        include_bytes!("../data/skin_effects/CumInNoseFace_02.png"),
    ),
];

/// An effect's picture at a level from 1, decoded once.
pub fn picture(effect: usize, level: u8) -> Option<Arc<Image>> {
    static DECODED: OnceLock<Vec<OnceLock<Option<Arc<Image>>>>> = OnceLock::new();
    let effect = EFFECTS.get(effect)?;
    if level == 0 || level > effect.levels {
        return None;
    }
    let name = format!("{}_{level:02}", effect.key);
    let at = PICTURES.iter().position(|(n, _)| *n == name)?;
    let cells = DECODED.get_or_init(|| PICTURES.iter().map(|_| OnceLock::new()).collect());
    cells[at]
        .get_or_init(|| crate::material_editor::decode(PICTURES[at].1, name).map(Arc::new))
        .clone()
}

/// Whether any effect of the face (or else the body) is on.
pub fn any(levels: &Levels, face: bool) -> bool {
    EFFECTS.iter().zip(levels).any(|(e, &l)| e.face == face && l > 0)
}

/// Lays the effects of the face (or else the body) over a skin texture
/// that already has the card's overlay, in the plugin's order; `None`
/// while none of them is on.
pub fn paint(base: &Image, levels: &Levels, face: bool) -> Option<Image> {
    let mut out: Option<Image> = None;
    for (i, (effect, &level)) in EFFECTS.iter().zip(levels).enumerate() {
        if effect.face != face {
            continue;
        }
        if let Some(over) = picture(i, level) {
            out = Some(compose::blend(out.as_ref().unwrap_or(base), &over));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_level_has_its_picture() {
        for (i, effect) in EFFECTS.iter().enumerate() {
            for level in 1..=effect.levels {
                assert!(picture(i, level).is_some(), "{} {level}", effect.key);
            }
            assert!(picture(i, effect.levels + 1).is_none());
            assert!(picture(i, 0).is_none());
        }
    }

    #[test]
    fn effects_land_on_their_own_part() {
        let mut levels = Levels::default();
        levels[9] = 2;
        assert!(any(&levels, true) && !any(&levels, false), "tears are on the face");
        let base = compose::clear(8, 8, "face");
        assert!(paint(&base, &Levels::default(), true).is_none());
        assert!(paint(&base, &levels, false).is_none(), "no body effect is on");
        assert_eq!(
            paint(&base, &levels, true).map(|i| i.width),
            Some(1024),
            "the picture's detail is kept"
        );
    }
}
