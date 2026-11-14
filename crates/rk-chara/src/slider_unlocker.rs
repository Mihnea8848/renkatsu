// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Slider values outside 0 to 1, as the Slider Unlocker plugin computes
//! them (`SliderMath`): position and scale run on a straight line from the
//! first key to the last, rotation does the same after unwrapping by the
//! direction of the first segment, and a few bones keep the value at the
//! nearest end of the range.

/// One key: position, rotation in degrees, scale.
pub type Key = [f32; 9];

/// Bones whose rotation the plugin leaves at the clamped value.
fn keeps_rotation(name: &str) -> bool {
    name.contains("cf_s_Mune")
        || name.contains("cf_s_Mouth")
        || name.contains("cf_s_LegLow")
        || name.contains("cf_s_MayuTip")
        || (name.contains("thigh") && name.contains("01"))
        || (name.starts_with("cf_a_bust") && name.ends_with("_size"))
}

/// The value of `name` at `rate` outside 0 to 1. `clamped` is the value at
/// the nearest end, which the excluded rotations keep.
pub fn outside(keys: &[Key], name: &str, rate: f32, clamped: Key) -> Key {
    let (first, last) = (keys[0], keys[keys.len() - 1]);
    let mut out = clamped;
    for k in (0..3).chain(6..9) {
        out[k] = first[k] + (last[k] - first[k]) * rate;
    }
    if !keeps_rotation(name) {
        let second = keys.get(1).copied().unwrap_or(last);
        for k in 3..6 {
            let rising = second[k] - first[k] >= 0.0;
            let mut span = last[k] - first[k];
            if span > 0.0 && !rising {
                span -= 360.0;
            } else if span < 0.0 && rising {
                span += 360.0;
            }
            out[k] = if rate < 0.0 {
                first[k] - span * rate.abs()
            } else {
                last[k] + span * (rate - 1.0).abs()
            };
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(pos: f32, rot: f32, scl: f32) -> Key {
        [pos, 0.0, 0.0, rot, 0.0, 0.0, scl, 1.0, 1.0]
    }

    #[test]
    fn position_and_scale_follow_the_first_and_last_keys() {
        // A bent curve: the middle key is off the straight line.
        let keys = [key(0.0, 0.0, 1.0), key(0.9, 0.0, 1.0), key(1.0, 0.0, 2.0)];
        let v = outside(&keys, "cf_s_ear", 1.5, keys[2]);
        assert!((v[0] - 1.5).abs() < 1e-6);
        assert!((v[6] - 2.5).abs() < 1e-6);
        let v = outside(&keys, "cf_s_ear", -0.5, keys[0]);
        assert!((v[0] + 0.5).abs() < 1e-6);
        assert!((v[6] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn rotation_extends_the_whole_span() {
        let keys = [key(0.0, 0.0, 1.0), key(0.0, 10.0, 1.0), key(0.0, 20.0, 1.0)];
        assert!((outside(&keys, "cf_s_ear", 1.5, keys[2])[3] - 30.0).abs() < 1e-4);
        assert!((outside(&keys, "cf_s_ear", -0.5, keys[0])[3] + 10.0).abs() < 1e-4);
    }

    #[test]
    fn rotation_span_takes_the_first_segments_direction() {
        // Rising first segment (10 -> 20) but a last key below the first:
        // the plugin goes the long way round, a span of 350 degrees.
        let keys = [key(0.0, 10.0, 1.0), key(0.0, 20.0, 1.0), key(0.0, 0.0, 1.0)];
        assert!((outside(&keys, "cf_s_ear", 1.5, keys[2])[3] - 175.0).abs() < 1e-4);
        // The stored angles are compared as they are: 350 -> 0 counts as falling.
        let keys = [key(0.0, 350.0, 1.0), key(0.0, 0.0, 1.0), key(0.0, 10.0, 1.0)];
        assert!((outside(&keys, "cf_s_ear", 1.5, keys[2])[3] + 160.0).abs() < 1e-4);
    }

    #[test]
    fn some_rotations_keep_the_clamped_value() {
        let keys = [key(0.0, 0.0, 1.0), key(0.0, 30.0, 1.0)];
        let clamped = keys[1];
        let v = outside(&keys, "cf_s_Mouth_02", 2.0, clamped);
        assert_eq!(v[3], 30.0);
        // Position still extends.
        let keys = [key(0.0, 0.0, 1.0), key(1.0, 30.0, 1.0)];
        assert!((outside(&keys, "cf_s_Mouth_02", 2.0, keys[1])[0] - 2.0).abs() < 1e-6);
    }
}
