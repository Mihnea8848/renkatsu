// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Eye Control plugin (`KK_EyeControl`): a card stores how far the
//! eyes open at most and whether they blink.
//!
//! In Studio the plugin acts once, when the character is added to the
//! scene (`EyeControlCharaController.OnCharacterAddedToScene`): it calls
//! `ChaControl.ChangeEyesOpenMax(EyeOpenMax)` and, with blinking disabled,
//! `ChangeEyesBlinkFlag(false)`.
//!
//! What the game makes of that:
//! - `ChangeEyesOpenMax` stores the value clamped to 0..1 in the status and
//!   sets `eyesCtrl.OpenMax` to it clamped to 0..0.92.
//! - Every frame `FaceBlendShape.LateUpdate` replaces the limit with
//!   `min(1 - EyeLookUpCorrect, OpenMax)` plus a term that follows where
//!   the eyes look, and `FBSBase.CalculateBlendShape` opens the eyes to
//!   `Lerp(OpenMin, limit, openRate)`.
//! - `openRate` is 1 between blinks, and `ChangeEyesBlinkFlag(false)`
//!   forces it to 1, so a still picture has it at 1 either way.

use rk_card::{ExtData, Value};

pub const KEY: &str = "com.deathweasel.bepinex.eyecontrol";

/// Upper clamp of `eyesCtrl.OpenMax` in `ChaControl.ChangeEyesOpenMax`.
pub const OPEN_MAX_LIMIT: f32 = 0.92;
/// `FaceBlendShape.EyeLookUpCorrect` when the prefab does not store it.
pub const EYE_LOOK_UP_CORRECT: f32 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub eye_open_max: f32,
    pub disable_blinking: bool,
}

/// `LoadExtendedSaveData` starts from these; they are also the game's own
/// status defaults, so a card without the data behaves the same.
impl Default for Settings {
    fn default() -> Self {
        Self {
            eye_open_max: 1.0,
            disable_blinking: false,
        }
    }
}

pub fn read(ext: &ExtData) -> Settings {
    let mut s = Settings::default();
    let Some(p) = ext.get(KEY) else { return s };
    if let Some(v) = p.data.get("EyeOpenMax").and_then(Value::as_f64) {
        s.eye_open_max = v as f32;
    }
    if let Some(v) = p.data.get("DisableBlinking").and_then(Value::as_bool) {
        s.disable_blinking = v;
    }
    s
}

/// The limit the eyes open to for a status value `eyes_open_max`, with the
/// eyes looking straight ahead (the look term is then 0).
/// `up_correct` is the head's `FaceBlendShape.EyeLookUpCorrect`.
pub fn open_limit(eyes_open_max: f32, up_correct: f32) -> f32 {
    let open_max = eyes_open_max.clamp(0.0, OPEN_MAX_LIMIT);
    (1.0 - up_correct).min(open_max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_card::PluginData;

    #[test]
    fn reads_card_values() {
        let mut p = PluginData::default();
        p.data.insert("EyeOpenMax".into(), Value::F32(0.6));
        p.data.insert("DisableBlinking".into(), Value::Boolean(true));
        let mut ext = ExtData::new();
        assert_eq!(read(&ext), Settings::default());
        ext.insert(KEY.into(), p);
        let s = read(&ext);
        assert!((s.eye_open_max - 0.6).abs() < 1e-6);
        assert!(s.disable_blinking);
    }

    #[test]
    fn limit_is_clamped_twice() {
        // Fully open: the look correction is the tighter limit.
        assert!((open_limit(1.0, 0.1) - 0.9).abs() < 1e-6);
        // Without a correction the game's own clamp remains.
        assert!((open_limit(1.0, 0.0) - 0.92).abs() < 1e-6);
        assert!((open_limit(0.6, 0.1) - 0.6).abs() < 1e-6);
        assert_eq!(open_limit(-1.0, 0.1), 0.0);
    }
}
