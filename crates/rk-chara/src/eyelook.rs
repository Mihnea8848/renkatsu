// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Size and place of the iris on the eye. The game does not move the eye
//! mesh: a script on each eye scales and shifts the iris texture, from the
//! card's pupil sliders and from where the eyes look.

use rk_unityfs::{UObject, UValue};

/// Values the game stores on each eye's `EyeLookMaterialControll` script.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EyeLook {
    pub inside_wait: f32,
    pub outside_wait: f32,
    pub up_wait: f32,
    pub down_wait: f32,
    pub inside_limit: f32,
    pub outside_limit: f32,
    pub up_limit: f32,
    pub down_limit: f32,
    pub power: f32,
}

impl Default for EyeLook {
    /// What the script's own initialiser sets.
    fn default() -> Self {
        Self {
            inside_wait: -100.0,
            outside_wait: 100.0,
            up_wait: -100.0,
            down_wait: 100.0,
            inside_limit: -100.0,
            outside_limit: 100.0,
            up_limit: -80.0,
            down_limit: 80.0,
            power: 0.001,
        }
    }
}

impl EyeLook {
    /// Reads the script. Returns which eye it is for (0 left, 1 right) with
    /// its values, or `None` when the object is another kind of script.
    pub fn read(o: &UObject) -> Option<(usize, Self)> {
        let side = o.i64("eyeLR")? as usize;
        let d = Self::default();
        let num = |name: &str, default: f32| o.get(name).and_then(UValue::as_f32).unwrap_or(default);
        o.get("InsideWait")?;
        Some((
            side,
            Self {
                inside_wait: num("InsideWait", d.inside_wait),
                outside_wait: num("OutsideWait", d.outside_wait),
                up_wait: num("UpWait", d.up_wait),
                down_wait: num("DownWait", d.down_wait),
                inside_limit: num("InsideLimit", d.inside_limit),
                outside_limit: num("OutsideLimit", d.outside_limit),
                up_limit: num("UpLimit", d.up_limit),
                down_limit: num("DownLimit", d.down_limit),
                power: num("power", d.power),
            },
        ))
    }
}

/// The card's pupil sliders, each 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pupil {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Texture scale and offset for the iris of one eye.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}

// Unity's Mathf.Lerp limits the weight to 0..1.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn inverse_lerp(a: f32, b: f32, v: f32) -> f32 {
    ((v - a) / (b - a)).clamp(0.0, 1.0)
}

/// Where the iris texture sits. `gaze` is how far the eye is turned sideways
/// and up, each -1 to 1; zero looks straight ahead.
pub fn placement(look: &EyeLook, right: bool, pupil: Pupil, gaze: [f32; 2]) -> Placement {
    let mut shift_x = lerp(0.2, -0.6, pupil.x);
    if right {
        shift_x = -shift_x;
    }
    let shift_y = lerp(-0.5, 0.5, pupil.y);
    let grow = [lerp(1.8, -0.2, pupil.width), lerp(1.8, -0.2, pupil.height)];

    let mut v = [gaze[0] + shift_x, gaze[1] + shift_y];
    let len = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if len > 1.0 {
        v = [v[0] / len, v[1] / len];
    }
    let side = lerp(look.inside_wait, look.outside_wait, inverse_lerp(-1.0, 1.0, v[0]));
    let up = lerp(look.down_wait, look.up_wait, inverse_lerp(-1.0, 1.0, v[1]));
    let moved = [
        (side * look.power * lerp(1.0, 5.0, grow[0])).clamp(look.inside_limit, look.outside_limit),
        (up * look.power * lerp(1.0, 5.0, grow[1])).clamp(look.up_limit, look.down_limit),
    ];
    Placement {
        scale: [1.0 + grow[0], 1.0 + grow[1]],
        offset: [moved[0] - grow[0] * 0.5, moved[1] - grow[1] * 0.5],
    }
}

/// How far a highlight sits above or below the iris, from its slider.
pub fn highlight_shift(slider: f32) -> f32 {
    lerp(0.1, -0.1, slider)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5
    }

    #[test]
    fn sliders_scale_and_shift_the_iris() {
        let look = EyeLook::default();
        // Width and height at 0.9 leave the texture at its own size, and a
        // centred pupil does not move it.
        let centred = Pupil {
            x: 0.25,
            y: 0.5,
            width: 0.9,
            height: 0.9,
        };
        let p = placement(&look, false, centred, [0.0, 0.0]);
        assert!(near(p.scale, [1.0, 1.0]), "{p:?}");
        assert!(near(p.offset, [0.0, 0.0]), "{p:?}");

        // A smaller slider value tiles the texture more, so the iris shrinks,
        // and the offset keeps it centred.
        let small = Pupil {
            width: 0.4,
            height: 0.65,
            ..centred
        };
        let p = placement(&look, false, small, [0.0, 0.0]);
        assert!(near(p.scale, [2.0, 1.5]), "{p:?}");
        assert!(near(p.offset, [-0.5, -0.25]), "{p:?}");
    }

    #[test]
    fn position_and_gaze_move_it() {
        let look = EyeLook::default();
        let pupil = Pupil {
            x: 0.0,
            y: 0.5,
            width: 0.9,
            height: 0.9,
        };
        // Pupil X at 0 shifts by 0.2: one fifth of the way to the outside.
        let left = placement(&look, false, pupil, [0.0, 0.0]);
        assert!(near(left.offset, [0.02, 0.0]), "{left:?}");
        // The right eye mirrors it.
        let right = placement(&look, true, pupil, [0.0, 0.0]);
        assert!(near(right.offset, [-0.02, 0.0]), "{right:?}");
        // Looking up moves the texture the other way; the sum is cut at length 1.
        let up = placement(&look, false, pupil, [0.0, 5.0]);
        assert!(up.offset[1] < -0.09 && up.offset[1] >= -0.1, "{up:?}");
    }

    #[test]
    fn highlight_slider_is_centred_at_half() {
        assert_eq!(highlight_shift(0.5), 0.0);
        assert!((highlight_shift(0.0) - 0.1).abs() < 1e-6);
        assert!((highlight_shift(2.0) + 0.1).abs() < 1e-6);
    }
}
