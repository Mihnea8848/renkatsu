// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The face as CharaStudio sets it: eyebrow, eye and mouth patterns, how
//! open the eyes and mouth are, and blinking.
//!
//! The head's `FaceBlendShape` has three `FBSBase` controllers. Every frame
//! (`FaceBlendShape.LateUpdate`) the eyebrows and eyes take the blink's
//! open rate, or keep the rate last forced while blinking is off, and the
//! mouth takes its fixed rate. `FBSBase.CalculateBlendShape` then sets
//! every shape of every pattern to 0 and the shown pattern's closed and
//! open shapes to `100 - n` and `n`, `n` the whole percent of
//! `Lerp(OpenMin, max, openRate)`; a fixed rate of 0 or more replaces that
//! lerp.
//!
//! Studio's eye pattern is a `cha_eyeset` item (`ChaControl.ChangeEyesPtn`):
//! its `EyesPtn` is the controller's pattern, `EyeBase` and `EyeHitomi`
//! show the eye white and the pupils, `EyeObjNo` one of the three gag eye
//! meshes, and `EpsTex` a picture in the irises (`_expression`, at
//! `_exppower` 1). Mouth pattern 21 puts the tongue out
//! (`ChangeMouthPtn` sets tongue state 1, and `UpdateVisible` then hides
//! the face's tongue and shows the body's).

use crate::expression::Controller;
use crate::scene::{Image, Scene, TexSlot};
use rk_card::Value;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

/// The mouth pattern that puts the tongue out.
pub const TONGUE_OUT: i32 = 21;

/// A `cha_eyeset` item.
#[derive(Debug, Clone, Default)]
pub struct EyeRow {
    /// The eye controller's pattern.
    pub pattern: i32,
    /// The eye white and the pupils show.
    pub base: bool,
    pub pupils: bool,
    /// The gag eye mesh shown, 1 to 3, or 0 for none.
    pub gag: i32,
    /// The picture in the irises.
    pub expression: Option<Arc<Image>>,
}

/// The game's eye patterns by the names given here, from their Japanese
/// ones.
pub fn eye_label(id: i32) -> Option<&'static str> {
    const NAMES: [&str; 39] = [
        "Default",
        "Closed",
        "Smile",
        "Smile, closed",
        "Slight smile",
        "Wink left",
        "Wink right",
        "Aching",
        "Shy",
        "Angry",
        "Serious",
        "Bored",
        "Pained",
        "Disgusted",
        "Thinking",
        "Sad",
        "Crying",
        "Flustered",
        "Dejected",
        "Troubled",
        "Smug",
        "Swirls 1",
        "Swirls 2",
        "Swirls 3",
        "Stars",
        "Hearts",
        "Heart eyes",
        "Fire in the eyes",
        "Cartoon wink",
        "Upright lines",
        "Cartoon closed",
        "Flat lines",
        "Cartoon crying",
        "Left shut tight",
        "Right shut tight",
        "Shut tight",
        "Left shut tight 2",
        "Right shut tight 2",
        "Shut tight 2",
    ];
    usize::try_from(id).ok().and_then(|i| NAMES.get(i)).copied()
}

/// The face's settings, as `ChaFileStatus` and Studio's face panel hold
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Face {
    /// The eyebrow controller's pattern.
    pub eyebrows: i32,
    /// A `cha_eyeset` item.
    pub eyes: i32,
    /// The mouth controller's pattern.
    pub mouth: i32,
    /// Studio's eye slider (`ChangeEyesOpenMax`), 0 to 1.
    pub eyes_open: f32,
    /// Whether the eyes blink; while they do not, they stay at the slider.
    pub blink: bool,
    /// Studio's mouth slider (`ChangeMouthOpen`, the mouth's fixed rate).
    pub mouth_open: f32,
}

impl Default for Face {
    /// A new status's face (`ChaFileStatus.MemberInit`) with Studio's mouth.
    fn default() -> Self {
        Face {
            eyebrows: 0,
            eyes: 0,
            mouth: 0,
            eyes_open: 1.0,
            blink: true,
            mouth_open: 0.0,
        }
    }
}

impl Face {
    /// The face a card's status gives a character added to Studio
    /// (`AddObjectAssist`): its patterns, blink and eye slider; Studio's
    /// own mouth opening, which starts closed.
    pub fn of_status(status: &Value) -> Face {
        let int = |key: &str, default: i32| field(status, key).and_then(Value::as_i64).map_or(default, |v| v as i32);
        let float = |key: &str, default: f32| field(status, key).and_then(Value::as_f64).map_or(default, |v| v as f32);
        let start = Face::default();
        Face {
            eyebrows: int("eyebrowPtn", 0),
            eyes: int("eyesPtn", 0),
            mouth: int("mouthPtn", 0),
            eyes_open: float("eyesOpenMax", 1.0).clamp(0.0, 1.0),
            blink: field(status, "eyesBlink").and_then(Value::as_bool).unwrap_or(true),
            ..start
        }
    }
}

impl std::str::FromStr for Face {
    type Err = anyhow::Error;

    /// `EYEBROWS,EYES,MOUTH`, then optionally the eyes' opening, blink as
    /// 1 or 0, and the mouth's opening.
    fn from_str(spec: &str) -> anyhow::Result<Face> {
        let parts: Vec<&str> = spec.split(',').map(str::trim).collect();
        anyhow::ensure!(
            (3..=6).contains(&parts.len()),
            "a face is EYEBROWS,EYES,MOUTH[,EYES_OPEN[,BLINK[,MOUTH_OPEN]]]"
        );
        let mut face = Face {
            eyebrows: parts[0].parse()?,
            eyes: parts[1].parse()?,
            mouth: parts[2].parse()?,
            ..Face::default()
        };
        if let Some(v) = parts.get(3) {
            face.eyes_open = v.parse()?;
        }
        if let Some(v) = parts.get(4) {
            face.blink = *v != "0";
        }
        if let Some(v) = parts.get(5) {
            face.mouth_open = v.parse()?;
        }
        Ok(face)
    }
}

/// A map's value by its key.
fn field<'a>(map: &'a Value, key: &str) -> Option<&'a Value> {
    map.as_map()?
        .iter()
        .find(|(k, _)| k.as_str() == Some(key))
        .map(|(_, v)| v)
}

/// One controller bound to the character's meshes.
#[derive(Debug, Clone)]
pub struct Part {
    pub controller: Controller,
    /// The renderers each of its targets moves, by the target's place.
    pub renderers: Vec<Vec<usize>>,
}

impl Part {
    /// How many patterns the controller has (`FBSBase.GetMaxPtn`).
    pub fn patterns(&self) -> usize {
        self.controller.targets.first().map_or(0, |t| t.patterns.len())
    }

    /// Shows `pattern` at `rate`, as `CalculateBlendShape` with the blend
    /// done: every pattern's shapes to 0, then the pattern's closed and
    /// open ones. A pattern past the end is not changed to
    /// (`FBSBase.ChangePtn`), so it shows pattern 0.
    fn show(&self, scene: &mut Scene, pattern: i32, rate: f32) {
        let pattern = usize::try_from(pattern)
            .ok()
            .filter(|&p| p < self.patterns())
            .unwrap_or(0);
        let percent = (rate * 100.0).clamp(0.0, 100.0) as i64 as f32;
        for (target, renderers) in self.controller.targets.iter().zip(&self.renderers) {
            let mut weights: BTreeMap<i64, f32> = BTreeMap::new();
            for &(close, open) in &target.patterns {
                weights.insert(close, 0.0);
                weights.insert(open, 0.0);
            }
            if let Some(&(close, open)) = target.patterns.get(pattern) {
                *weights.entry(close).or_default() += 100.0 - percent;
                *weights.entry(open).or_default() += percent;
            }
            for &r in renderers {
                let rend = &mut scene.renderers[r];
                let count = rend.mesh.blend_shapes.len();
                rend.shape_weights.resize(count, 0.0);
                for (&shape, &w) in &weights {
                    if let Some(slot) = usize::try_from(shape).ok().and_then(|s| rend.shape_weights.get_mut(s)) {
                        *slot = w / 100.0;
                    }
                }
            }
        }
    }
}

/// A character's face controllers, ready to show a face.
#[derive(Debug, Clone, Default)]
pub struct Rig {
    pub eyebrows: Option<Part>,
    pub eyes: Option<Part>,
    pub mouth: Option<Part>,
    /// `FaceBlendShape.EyeLookUpCorrect`.
    pub up_correct: f32,
    /// The `cha_eyeset` items, by number.
    pub eyesets: BTreeMap<i32, EyeRow>,
    /// The eye white's and the pupils' nodes (`n_eyebase`, `n_hitomi`).
    pub eye_base: Option<usize>,
    pub pupils: Option<usize>,
    /// The irises' renderers (`rendEye`).
    pub irises: Vec<usize>,
    /// The face's tongue and the body's, by renderer.
    pub tongues: [Vec<usize>; 2],
    /// The face the character was built with.
    pub start: Face,
    /// The eyes' open rate it was built with: EyeControl turns blinking
    /// off after setting the slider, which forces the rate to 1.
    pub start_rate: f32,
}

impl Rig {
    /// Shows `face`, or the face the character was built with.
    pub fn show(&self, scene: &mut Scene, face: Option<&Face>) {
        let (face, eyes_rate) = match face {
            Some(f) => (f, Rig::eyes_rate(f)),
            None => (&self.start, self.start_rate),
        };
        // Eyebrows follow the blink (`SyncBlink`), between blinks and
        // with blinking off both at 1.
        if let Some(part) = &self.eyebrows {
            let c = &part.controller;
            part.show(scene, face.eyebrows, lerp(c.open_min, c.open_max, 1.0));
        }
        if let Some(part) = &self.eyes {
            let c = &part.controller;
            // `ChangeEyesOpenMax` clamps the limit to 0.92, and every
            // frame the limit is at most `1 - EyeLookUpCorrect`, with the
            // eyes looking straight ahead.
            let limit = crate::eye_control::open_limit(face.eyes_open, self.up_correct);
            let pattern = self.eye_row(face.eyes).map_or(0, |r| r.pattern);
            part.show(scene, pattern, lerp(c.open_min, limit, eyes_rate));
        }
        self.show_eye_row(scene, face.eyes);
        if let Some(part) = &self.mouth {
            // Studio fixes the mouth at its slider (`FixedRate`).
            part.show(scene, face.mouth, face.mouth_open.clamp(0.0, 1.0));
        }
        let out = face.mouth == TONGUE_OUT;
        for (tongue, shown) in self.tongues.iter().zip([!out, out]) {
            for &r in tongue {
                scene.renderers[r].enabled = shown;
            }
        }
    }

    /// An eye pattern's item; one missing from the list shows item 0's
    /// (`ChangeEyesPtn`).
    pub fn eye_row(&self, id: i32) -> Option<&EyeRow> {
        self.eyesets.get(&id).or_else(|| self.eyesets.get(&0))
    }

    /// The eye white, the pupils and the irises' picture of an eye
    /// pattern.
    fn show_eye_row(&self, scene: &mut Scene, id: i32) {
        let Some(row) = self.eye_row(id) else { return };
        for (node, shown) in [(self.eye_base, row.base), (self.pupils, row.pupils)] {
            if let Some(n) = node {
                scene.nodes[n].active = shown;
            }
        }
        for &r in &self.irises {
            for m in &mut scene.renderers[r].materials {
                match &row.expression {
                    Some(image) => {
                        // `SetTexture` keeps the placement the material
                        // stores: on the game's irises a third of the size,
                        // over the pupil.
                        let st = m.tex_st.get("_expression").copied().unwrap_or([1.0, 1.0, 0.0, 0.0]);
                        m.set_texture(
                            "_expression",
                            TexSlot {
                                image: image.clone(),
                                scale: [st[0], st[1]],
                                offset: [st[2], st[3]],
                            },
                        );
                        m.set_float("_exppower", 1.0);
                    }
                    None => {
                        m.textures.remove("_expression");
                        m.set_float("_exppower", 0.0);
                    }
                }
            }
        }
    }

    /// The eye patterns that can be shown: the gag eyes' meshes are not
    /// drawn yet.
    pub fn eye_choices(&self) -> Vec<i32> {
        self.eyesets
            .iter()
            .filter(|(_, r)| r.gag == 0)
            .map(|(id, _)| *id)
            .collect()
    }

    /// The eyes' open rate for a face set in Studio's panel, which sets
    /// blinking before the slider (`AddObjectAssist`): 1 between blinks,
    /// or the slider while they do not blink.
    pub fn eyes_rate(face: &Face) -> f32 {
        if face.blink {
            1.0
        } else {
            face.eyes_open.clamp(0.0, 1.0)
        }
    }

    /// How many patterns the eyebrows and the mouth have.
    pub fn counts(&self) -> (usize, usize) {
        (
            self.eyebrows.as_ref().map_or(0, Part::patterns),
            self.mouth.as_ref().map_or(0, Part::patterns),
        )
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_gives_its_face_and_studio_closes_the_mouth() {
        let status = Value::Map(vec![
            (Value::from("eyebrowPtn"), Value::from(3)),
            (Value::from("eyesPtn"), Value::from(4)),
            (Value::from("mouthPtn"), Value::from(1)),
            (Value::from("eyesOpenMax"), Value::from(0.625)),
            (Value::from("eyesBlink"), Value::from(false)),
            (Value::from("mouthOpenMax"), Value::from(1.0)),
        ]);
        let face = Face::of_status(&status);
        assert_eq!((face.eyebrows, face.eyes, face.mouth), (3, 4, 1));
        assert_eq!(face.eyes_open, 0.625);
        assert!(!face.blink);
        assert_eq!(face.mouth_open, 0.0);
        assert_eq!(Face::of_status(&Value::Nil), Face::default());
    }

    #[test]
    fn a_face_reads_from_its_numbers() {
        let face: Face = "3,4,1,0.5,0,0.8".parse().unwrap();
        assert_eq!((face.eyebrows, face.eyes, face.mouth), (3, 4, 1));
        assert_eq!((face.eyes_open, face.blink, face.mouth_open), (0.5, false, 0.8));
        assert_eq!("2,0,5".parse::<Face>().unwrap().eyes_open, 1.0);
        assert!("1,2".parse::<Face>().is_err());
    }

    #[test]
    fn eye_patterns_switch_the_eyes_and_mouth_21_swaps_the_tongues() {
        let Ok(game) = rk_vfs::Game::open_user(&[]) else {
            eprintln!("skipped: no game");
            return;
        };
        let Some(dir) = rk_core::Config::load().ok().map(|c| c.game_dir) else {
            return;
        };
        let Ok(card) = rk_card::Card::load(dir.join("UserData/chara/female/Simple.png")) else {
            eprintln!("skipped: no Simple.png");
            return;
        };
        let options = crate::BuildOptions {
            clothes: false,
            accessories: false,
            physics: false,
            ..Default::default()
        };
        let mut chara = crate::build(&game, &card, &options).expect("Simple builds");
        let rig = chara.face.clone().expect("the head has its face controllers");
        assert_eq!(rig.irises.len(), 2);
        assert!(
            rig.tongues.iter().all(|t| !t.is_empty()),
            "a tongue in the face and one in the body"
        );
        assert!(rig.eyesets[&25].expression.is_some(), "hearts in the eyes");
        assert!(rig.eye_choices().contains(&25) && !rig.eye_choices().contains(&21));
        let base = rig.eye_base.expect("n_eyebase");

        rig.show(
            &mut chara.scene,
            Some(&Face {
                eyes: 21,
                mouth: TONGUE_OUT,
                ..Face::default()
            }),
        );
        assert!(!chara.scene.nodes[base].active, "a gag eye hides the eye white");
        assert!(!chara.scene.renderers[rig.tongues[0][0]].enabled);
        assert!(chara.scene.renderers[rig.tongues[1][0]].enabled);

        rig.show(
            &mut chara.scene,
            Some(&Face {
                eyes: 25,
                ..Face::default()
            }),
        );
        assert!(chara.scene.nodes[base].active);
        assert!(chara.scene.renderers[rig.tongues[0][0]].enabled);
        let iris = &chara.scene.renderers[rig.irises[0]].materials[0];
        let picture = &iris.textures["_expression"];
        assert_eq!(
            (picture.scale, picture.offset),
            ([3.0, 3.0], [-1.0, -0.8]),
            "the material's placement"
        );
        assert_eq!(iris.floats.get("_exppower"), Some(&1.0));
    }

    #[test]
    fn eyes_stay_at_the_slider_only_while_they_do_not_blink() {
        let face = Face {
            eyes_open: 0.4,
            ..Face::default()
        };
        assert_eq!(Rig::eyes_rate(&face), 1.0);
        assert_eq!(Rig::eyes_rate(&Face { blink: false, ..face }), 0.4);
    }
}
