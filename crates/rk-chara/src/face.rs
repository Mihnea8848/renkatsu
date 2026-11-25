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
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

/// How often the shaking highlights take a new size, in seconds: the
/// game's `YureTime` of 0.3, on a timer that gains a frame's time once a
/// frame and once more for each of the two shaking highlights.
pub const SHAKE_STEP: f32 = 0.1;

/// The shaking highlights' size factor `seconds` after they began, across
/// and up (`YureAddScale`): 1 at first, then a new draw of 1 to 2 across
/// and 1 to 1.5 up each step. The game draws at random; here the draw
/// comes from the step and the eye, so a moment always looks the same.
pub fn shake_factor(seconds: f32, eye: usize) -> [f32; 2] {
    let step = (seconds.max(0.0) / SHAKE_STEP) as u64;
    if step == 0 {
        return [1.0, 1.0];
    }
    // SplitMix64.
    let mut z = (step * 2 + eye as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let unit = |bits: u64| (bits & 0xFF_FFFF) as f32 / 16_777_216.0;
    [1.0 + unit(z), 1.0 + 0.5 * unit(z >> 32)]
}

/// A highlight's placement while it shakes, from where the eye look put it
/// and where it put the iris (`EyeLookMaterialControll.Update` with
/// `isYure`): it follows the gaze 0.8 across and 0.5 up as far, and grows
/// by the iris's growth times `factor` about its middle.
pub fn shaken(place: [f32; 4], iris: [f32; 4], factor: [f32; 2]) -> [f32; 4] {
    let grow = [iris[0] - 1.0, iris[1] - 1.0];
    let moved = [iris[2] + grow[0] * 0.5, iris[3] + grow[1] * 0.5];
    // The highlight's own shift up or down, which the iris does not have.
    let shift = place[3] - iris[3];
    let grown = [grow[0] * factor[0], grow[1] * factor[1]];
    [
        1.0 + grown[0],
        1.0 + grown[1],
        moved[0] * 0.8 - grown[0] * 0.5,
        moved[1] * 0.5 - grown[1] * 0.5 + shift,
    ]
}

/// The iris textures the eye look moves (`EyeLookMaterialControll.texStates`).
const IRIS_TEXTURES: [&str; 3] = ["_MainTex", "_overtex1", "_overtex2"];

/// The mouth pattern that puts the tongue out.
pub const TONGUE_OUT: i32 = 21;

/// The eyes' blinking (`FBSBlinkControl`): open for a while, then closing,
/// shut for a few frames and opening again. The game's random numbers are
/// Unity's; here each face has its own seeded ones, so that faces do not
/// blink together and a run repeats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blink {
    /// `BlinkFrequency`: the open time is 0.2 s times a whole number below it.
    pub frequency: u8,
    /// `BaseSpeed`: how long closing or opening takes, plus up to 0.05 s.
    pub base_speed: f32,
    mode: i8,
    blink_time: f32,
    calc_speed: f32,
    /// Frames left shut, which the game counts in frames.
    count: i32,
    last_frame: f32,
    seed: u32,
}

/// The game counts the shut frames at its frame rate; Renkatsu at this one.
const BLINK_FRAME: f32 = 1.0 / 60.0;

impl Blink {
    /// A face that starts open and first blinks after a random wait.
    pub fn new(seed: u32) -> Blink {
        let mut b = Blink {
            frequency: 30,
            base_speed: 0.15,
            mode: -1,
            blink_time: 0.0,
            calc_speed: 0.15,
            count: 0,
            last_frame: 0.0,
            // Scrambled, as xorshift's first numbers from small seeds are alike.
            seed: seed.wrapping_mul(0x9E37_79B9).wrapping_add(0x7F4A_7C15) | 1,
        };
        b.blink_time = b.wait(0.0);
        b.mode = 0;
        b
    }

    /// A random number in `0.0..1.0` (xorshift).
    fn random(&mut self) -> f32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    /// When an open eye next closes: 0.2 s times a whole number below the
    /// frequency (`Random.Range(0, BlinkFrequency)`).
    fn wait(&mut self, now: f32) -> f32 {
        let n = (self.random() * f32::from(self.frequency)).floor();
        now + 0.2 * n
    }

    /// Whether a blink is under way: closing, shut or opening.
    pub fn busy(&self) -> bool {
        self.mode != 0
    }

    /// Seconds from `now` until the open eye next starts to close.
    pub fn until_next(&self, now: f32) -> f32 {
        (self.blink_time - now).max(0.0)
    }

    /// The open rate at `now` seconds of the real clock (`CalcBlink`).
    pub fn rate(&mut self, now: f32) -> f32 {
        let left = (self.blink_time - now).max(0.0);
        let rate = match self.mode {
            0 => 1.0,
            1 => (left / self.calc_speed).clamp(0.0, 1.0),
            _ => (1.0 - left / self.calc_speed).clamp(0.0, 1.0),
        };
        if now > self.blink_time {
            match self.mode {
                0 => {
                    // `SetForceClose`.
                    self.calc_speed = self.base_speed + self.random() * 0.05;
                    self.blink_time = now + self.calc_speed;
                    self.count = (self.random() * 3.0).floor() as i32 + 1;
                    self.mode = 1;
                    self.last_frame = now;
                }
                1 => {
                    // Shut for `count` frames, then `SetForceOpen`.
                    while self.count > 0 && now - self.last_frame >= BLINK_FRAME {
                        self.count -= 1;
                        self.last_frame += BLINK_FRAME;
                    }
                    if self.count <= 0 {
                        self.calc_speed = self.base_speed + self.random() * 0.05;
                        self.blink_time = now + self.calc_speed;
                        self.mode = -1;
                    }
                }
                _ => {
                    self.blink_time = self.wait(now);
                    self.mode = 0;
                }
            }
        }
        rate
    }
}

/// The mouth the game's H scenes give a face with a shaft in its mouth,
/// "Holding in the mouth" (咥え): the `HFaceList` faces of oral actions use
/// it, or 21 for licking, with the mouth's least opening at 1.
pub const HOLDING_MOUTH: i32 = 22;

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

/// The game's eyebrow patterns by the names given here, from their
/// Japanese ones.
pub fn eyebrow_label(pattern: i32) -> Option<&'static str> {
    const NAMES: [&str; 17] = [
        "Default",
        "Angry",
        "Troubled",
        "Bored",
        "Doubtful left",
        "Doubtful right",
        "Thinking left",
        "Thinking right",
        "Cross left",
        "Cross right",
        "Serious",
        "Uneasy",
        "Surprised",
        "Dejected",
        "Smug",
        "Wink left",
        "Wink right",
    ];
    usize::try_from(pattern).ok().and_then(|i| NAMES.get(i)).copied()
}

/// The game's mouth patterns by the names given here, from their Japanese
/// ones; the vowels are the mouth shapes of speech.
pub fn mouth_label(pattern: i32) -> Option<&'static str> {
    const NAMES: [&str; 40] = [
        "Default",
        "Smile",
        "Happy",
        "Happy, small",
        "Happy, smaller",
        "Excited",
        "Excited, small",
        "Excited, smaller",
        "Angry",
        "Angry 2",
        "Serious",
        "Serious 2",
        "Disgusted",
        "Lonely",
        "Flustered",
        "Displeased",
        "Fed up",
        "Surprised",
        "Surprised, small",
        "Smug",
        "Tongue out",
        "Licking",
        "Holding in the mouth",
        "Kiss",
        "Open, tongue out",
        "A, small",
        "A, large",
        "I, small",
        "I, large",
        "U, small",
        "U, large",
        "E, small",
        "E, large",
        "O, small",
        "O, large",
        "N, small",
        "N, large",
        "Cat mouth",
        "Triangle",
        "Cartoon grin",
    ];
    usize::try_from(pattern).ok().and_then(|i| NAMES.get(i)).copied()
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
    /// The tears, 0 to 3 (`ChaFileStatus.tearsLv`): none, small, middle,
    /// large.
    #[serde(default)]
    pub tears: u8,
    /// The blush, 0 to 1 (`ChaFileStatus.hohoAkaRate`).
    #[serde(default)]
    pub blush: f32,
    /// The eyes' highlights hidden (`ChaControl.HideEyeHighlight`).
    #[serde(default)]
    pub hide_highlight: bool,
    /// KK_ExpressionControl's eye roll, 0 to 0.5: the irises move up by
    /// this much of their texture.
    #[serde(default)]
    pub eye_y: f32,
    /// KK_ExpressionControl's smaller irises, 0 to 1: their textures grow
    /// by this much about their middle.
    #[serde(default)]
    pub eye_small: f32,
    /// The eyes' highlights shake (`ChaControl.ChangeEyesShaking`, which
    /// KK_EyeShaking switches in Studio).
    #[serde(default)]
    pub shaking: bool,
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
            tears: 0,
            blush: 0.0,
            hide_highlight: false,
            eye_y: 0.0,
            eye_small: 0.0,
            shaking: false,
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
            (3..=12).contains(&parts.len()),
            "a face is EYEBROWS,EYES,MOUTH[,EYES_OPEN[,BLINK[,MOUTH_OPEN[,TEARS[,BLUSH[,HIDE_HIGHLIGHT[,EYE_Y[,EYE_SMALL[,SHAKING]]]]]]]]]"
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
        if let Some(v) = parts.get(6) {
            face.tears = v.parse()?;
        }
        if let Some(v) = parts.get(7) {
            face.blush = v.parse()?;
        }
        if let Some(v) = parts.get(8) {
            face.hide_highlight = *v != "0";
        }
        if let Some(v) = parts.get(9) {
            face.eye_y = v.parse()?;
        }
        if let Some(v) = parts.get(10) {
            face.eye_small = v.parse()?;
        }
        if let Some(v) = parts.get(11) {
            face.shaking = *v != "0";
        }
        Ok(face)
    }
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
    /// The small, middle and large tears' nodes (`cf_O_namida_S/M/L`).
    pub tears: [Option<usize>; 3],
    /// The face's renderer, whose material carries the blush.
    pub face: Option<usize>,
    /// The irises' textures as the eye look places them, by renderer:
    /// `_MainTex`, `_overtex1` and `_overtex2`'s scale and offset.
    pub iris_places: Vec<(usize, [Option<[f32; 4]>; 3])>,
    /// Each eye's look script and the card's pupil sliders, which place
    /// the irises for a gaze (`crate::eyelook::placement`).
    pub gaze_place: Option<([crate::eyelook::EyeLook; 2], crate::eyelook::Pupil)>,
    /// The face the character was built with.
    pub start: Face,
    /// The eyes' open rate it was built with: EyeControl turns blinking
    /// off after setting the slider, which forces the rate to 1.
    pub start_rate: f32,
}

impl Rig {
    /// Shows `face`, or the face the character was built with.
    pub fn show(&self, scene: &mut Scene, face: Option<&Face>) {
        self.show_at(scene, face, 0.0);
    }

    /// As `show`, with shaking highlights where they are `seconds` after
    /// they began.
    pub fn show_at(&self, scene: &mut Scene, face: Option<&Face>, seconds: f32) {
        self.show_blinking(scene, face, seconds, 1.0);
    }

    /// As `show_at`, with the eyes and eyebrows where a blink has them:
    /// `blink` is its open rate, 1 between blinks; it counts only while
    /// the face blinks.
    pub fn show_blinking(&self, scene: &mut Scene, face: Option<&Face>, seconds: f32, blink: f32) {
        self.show_gazing(scene, face, seconds, blink, None);
    }

    /// As `show_blinking`, with the irises where a gaze puts them: each
    /// eye's sideways rate and the upward rate (`crate::gaze`); none looks
    /// straight ahead.
    pub fn show_gazing(
        &self,
        scene: &mut Scene,
        face: Option<&Face>,
        seconds: f32,
        blink: f32,
        gaze: Option<([f32; 2], f32)>,
    ) {
        let (face, eyes_rate) = match face {
            Some(f) => (f, if f.blink { blink } else { Rig::eyes_rate(f) }),
            None if self.start.blink => (&self.start, blink.min(self.start_rate)),
            None => (&self.start, self.start_rate),
        };
        // Eyebrows follow the blink (`SyncBlink`); with blinking off at 1.
        let brows_rate = if face.blink { blink } else { 1.0 };
        if let Some(part) = &self.eyebrows {
            let c = &part.controller;
            part.show(scene, face.eyebrows, lerp(c.open_min, c.open_max, brows_rate));
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
        self.show_irises(scene, face, seconds, self.gaze_shift(gaze));
        if let Some(part) = &self.mouth {
            // Studio fixes the mouth at its slider (`FixedRate`).
            part.show(scene, face.mouth, face.mouth_open.clamp(0.0, 1.0));
        }
        // `UpdateVisible`: tear level n shows the n-th tears.
        for (i, node) in self.tears.iter().enumerate() {
            if let Some(n) = node {
                scene.nodes[*n].active = usize::from(face.tears) == i + 1;
            }
        }
        // `ChangeHohoAkaRate`: the face's second overlay colour takes
        // 0.2 of alpha at the most.
        if let Some(m) = self.face.and_then(|r| scene.renderers[r].materials.first_mut()) {
            let mut color = m.colors.get("_overcolor2").copied().unwrap_or([1.0; 4]);
            color[3] = 0.2 * face.blush.clamp(0.0, 1.0);
            m.set_color("_overcolor2", color);
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

    /// Keeps where the eye look put the irises' textures, for the eye
    /// roll and size to start from.
    pub fn keep_iris_places(&mut self, scene: &Scene) {
        self.iris_places = self
            .irises
            .iter()
            .map(|&r| {
                let m = scene.renderers[r].materials.first();
                let place = |name: &str| {
                    m.and_then(|m| m.textures.get(name))
                        .map(|t| [t.scale[0], t.scale[1], t.offset[0], t.offset[1]])
                };
                (r, IRIS_TEXTURES.map(place))
            })
            .collect();
    }

    /// The highlights, and KK_ExpressionControl's eye roll and size, which
    /// it lays every frame over the eye look's placement
    /// (`ProcEyeY`, `ProcEyeSmall`): the scale grows by the size, the
    /// offset moves back by half of it, and up by the roll. The picture in
    /// the irises rolls along, as KK_Ahegao rolls it.
    /// How far a gaze moves each iris's textures from where looking
    /// straight ahead puts them.
    fn gaze_shift(&self, gaze: Option<([f32; 2], f32)>) -> [[f32; 2]; 2] {
        let (Some((looks, pupil)), Some((h, v))) = (self.gaze_place, gaze) else {
            return [[0.0; 2]; 2];
        };
        [0, 1].map(|side| {
            let at = crate::eyelook::placement(&looks[side], side == 1, pupil, [h[side], v]);
            let ahead = crate::eyelook::placement(&looks[side], side == 1, pupil, [0.0, 0.0]);
            [at.offset[0] - ahead.offset[0], at.offset[1] - ahead.offset[1]]
        })
    }

    fn show_irises(&self, scene: &mut Scene, face: &Face, seconds: f32, shift: [[f32; 2]; 2]) {
        let small = face.eye_small.clamp(0.0, 1.0);
        let roll = face.eye_y.clamp(0.0, 0.5);
        for (eye, (r, places)) in self.iris_places.iter().enumerate() {
            let Some(m) = scene.renderers[*r].materials.first_mut() else {
                continue;
            };
            m.set_float("_isHighLight", if face.hide_highlight { 0.0 } else { 1.0 });
            let shake = face.shaking.then(|| shake_factor(seconds, eye));
            for (i, (name, place)) in IRIS_TEXTURES.iter().zip(places).enumerate() {
                let (Some(mut place), Some(slot)) = (*place, m.textures.get_mut(*name)) else {
                    continue;
                };
                if let (Some(r), Some(iris), true) = (shake, places[0], i > 0) {
                    place = shaken(place, iris, r);
                }
                slot.scale = [place[0] + small, place[1] + small];
                let moved = shift.get(eye).copied().unwrap_or_default();
                slot.offset = [
                    place[2] - small / 2.0 + moved[0],
                    place[3] - small / 2.0 - roll + moved[1],
                ];
            }
            // The picture in the irises rolls with them, as KK_Ahegao moves
            // it (`_expression` to the material's offset, up by twice the
            // roll); KK_ExpressionControl leaves it behind.
            let st = m.tex_st.get("_expression").copied().unwrap_or([1.0, 1.0, 0.0, 0.0]);
            if let Some(slot) = m.textures.get_mut("_expression") {
                slot.offset = [st[2], st[3] - roll * 2.0];
            }
        }
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
    fn shaking_highlights_start_still_and_draw_in_the_games_ranges() {
        assert_eq!(shake_factor(0.05, 0), [1.0, 1.0]);
        for step in 1..200 {
            let f = shake_factor(step as f32 * SHAKE_STEP + 0.01, step % 2);
            assert!((1.0..=2.0).contains(&f[0]) && (1.0..=1.5).contains(&f[1]), "{f:?}");
        }
        assert_eq!(
            shake_factor(1.23, 1),
            shake_factor(1.23, 1),
            "a moment always looks the same"
        );
        assert_ne!(
            shake_factor(1.23, 0),
            shake_factor(1.23, 1),
            "each eye shakes on its own"
        );
        // An iris grown by 0.4 and moved, its highlight 0.1 higher: still,
        // the highlight follows less of the move; shaking, it grows more.
        let iris = [1.4, 1.4, -0.2 + 0.1, -0.2 - 0.04];
        let place = [1.4, 1.4, iris[2], iris[3] + 0.1];
        let still = shaken(place, iris, [1.0, 1.0]);
        assert!((still[0] - 1.4).abs() < 1e-6);
        assert!((still[2] - (0.1 * 0.8 - 0.2)).abs() < 1e-6);
        assert!((still[3] - (-0.04 * 0.5 - 0.2 + 0.1)).abs() < 1e-6);
        assert!(shaken(place, iris, [2.0, 1.5])[0] > 1.7);
    }

    #[test]
    fn a_blink_closes_and_opens_within_the_games_times() {
        let mut b = Blink::new(7);
        let mut t = 0.0;
        // Open until the first blink, at most 0.2 s times 29.
        while b.rate(t) >= 1.0 {
            t += BLINK_FRAME;
            assert!(t < 6.0, "a blink comes within 5.8 s");
        }
        let started = t;
        let mut shut = false;
        while t < started + 1.0 {
            let r = b.rate(t);
            shut |= r <= 0.0;
            if shut && r >= 1.0 {
                break;
            }
            t += BLINK_FRAME;
        }
        assert!(shut, "the eyes shut");
        // Closing and opening each 0.15 to 0.2 s, shut 1 to 3 frames.
        let took = t - started;
        assert!((0.3..0.5).contains(&took), "{took}");
        // Faces with other seeds blink at other times.
        let first = |seed| {
            let mut b = Blink::new(seed);
            (0..600).position(|i| b.rate(i as f32 * BLINK_FRAME) < 1.0)
        };
        assert_ne!(first(1), first(2));
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
