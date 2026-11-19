// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The liquid that lies on the skin: sweat and the rest, which the game
//! calls `siru` and keeps as a level per part of the body.
//!
//! `ChaControl.UpdateSiru` holds five levels, `ChaFileDefine.SiruParts`:
//! the face, then the body's four quarters. The face's level goes to
//! `_liquidface` on the face material, the four others to `_liquidftop`,
//! `_liquidfbot`, `_liquidbtop` and `_liquidbbot` on the body's, as plain
//! floats. `main_skin` reads them against its liquid mask, whose colour
//! says which level a place answers to, and lays the amount texture and a
//! ripple normal map over the skin where the level asks for them.
//!
//! The levels are not the card's: CharaStudio keeps them per character in
//! the scene (`Studio.OICharInfo.siru`, five bytes) and hands them to the
//! character on load (`AddObjectAssist`), which is where ours live too.
//!
//! The game also passes the body's four to the clothes of kinds 0, 1, 2, 3
//! and 5 (`ChaControl.UpdateClothesSiru`), whose shaders carry the same
//! properties. Renkatsu's clothes shaders leave the liquid layers out, so
//! that is not done here.

use crate::scene::Scene;

/// `ChaFileDefine.SiruParts`: the face, then the body's four quarters.
pub const PARTS: usize = 5;

/// How much liquid lies on each part, 0 to [`MAX`].
pub type Levels = [u8; PARTS];

/// The most a part takes. `main_skin` reads the amount texture's red
/// channel at level one and its green channel as well at level two;
/// nothing is drawn for a level above that.
pub const MAX: u8 = 2;

/// What each part is called, in the order the game keeps them.
pub const NAMES: [&str; PARTS] = ["Face", "Front top", "Front bottom", "Back top", "Back bottom"];

/// `ChaControl.customMatFace`: the renderer the face's skin is drawn with.
pub const FACE: &str = "cf_O_face";

/// `ChaControl.customMatBody`.
pub const BODY: &str = "o_body_a";

/// The face's level.
const FACE_LEVEL: &str = "_liquidface";

/// The body's four, in `SiruParts` order after the face.
const BODY_LEVELS: [&str; 4] = ["_liquidftop", "_liquidfbot", "_liquidbtop", "_liquidbbot"];

/// The mask that says which level a place on the skin answers to. A body
/// without one shows no liquid whatever the levels say, which is how the
/// game's male body comes.
const MASK: &str = "_liquidmask";

/// Lays the levels on the face and the body, as `UpdateSiru` does.
pub fn apply(scene: &mut Scene, levels: &Levels) {
    for r in &mut scene.renderers {
        let name = r.name.clone();
        let Some(m) = r.materials.first_mut() else {
            continue;
        };
        if name == FACE {
            m.set_float(FACE_LEVEL, f32::from(levels[0]));
        } else if name == BODY {
            for (property, &level) in BODY_LEVELS.iter().zip(&levels[1..]) {
                m.set_float(property, f32::from(level));
            }
        }
    }
}

/// Whether the body carries the mask the levels are read against, which
/// is what decides whether any of this shows.
pub fn masked(scene: &Scene) -> bool {
    scene
        .renderers
        .iter()
        .any(|r| r.name == BODY && r.materials.first().is_some_and(|m| m.textures.contains_key(MASK)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{test_renderer, MaterialInst, RendererInst};

    fn renderer(name: &str) -> RendererInst {
        let mut r = test_renderer(name, 0);
        r.materials.push(MaterialInst::default());
        r
    }

    #[test]
    fn the_face_takes_its_level_and_the_body_the_other_four() {
        let mut scene = Scene::default();
        scene.renderers.push(renderer(FACE));
        scene.renderers.push(renderer(BODY));
        apply(&mut scene, &[2, 1, 0, 2, 1]);
        let face = &scene.renderers[0].materials[0].floats;
        assert_eq!(face.get(FACE_LEVEL), Some(&2.0));
        // The body's own level for the face is left as it was.
        let body = &scene.renderers[1].materials[0].floats;
        assert_eq!(body.get(FACE_LEVEL), None);
        assert_eq!(body.get("_liquidftop"), Some(&1.0));
        assert_eq!(body.get("_liquidfbot"), Some(&0.0));
        assert_eq!(body.get("_liquidbtop"), Some(&2.0));
        assert_eq!(body.get("_liquidbbot"), Some(&1.0));
    }

    #[test]
    fn a_body_without_the_mask_shows_none_of_it() {
        let mut scene = Scene::default();
        scene.renderers.push(renderer(BODY));
        assert!(!masked(&scene));
        let slot = crate::scene::TexSlot {
            image: std::sync::Arc::new(crate::scene::Image {
                name: MASK.into(),
                width: 1,
                height: 1,
                rgba: vec![0; 4],
                srgb: false,
                repeat: false,
            }),
            scale: [1.0, 1.0],
            offset: [0.0, 0.0],
        };
        scene.renderers[0].materials[0].textures.insert(MASK.into(), slot);
        assert!(masked(&scene));
    }
}
