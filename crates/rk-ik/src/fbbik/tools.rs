// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The helpers of `RootMotion.V3Tools` and `RootMotion.QuaTools` that the
//! full body solver calls. Port of RootMotion FinalIK.

use crate::unity;
use glam::{Quat, Vec3};

/// `V3Tools.ExtractVertical`.
pub fn extract_vertical(v: Vec3, vertical_axis: Vec3, weight: f32) -> Vec3 {
    if weight == 0.0 {
        return Vec3::ZERO;
    }
    unity::project(v, vertical_axis) * weight
}

/// `V3Tools.ExtractHorizontal`.
pub fn extract_horizontal(v: Vec3, normal: Vec3, weight: f32) -> Vec3 {
    if weight == 0.0 {
        return Vec3::ZERO;
    }
    let mut normal = normal;
    let mut tangent = v;
    unity::ortho_normalize(&mut normal, &mut tangent);
    unity::project(v, tangent) * weight
}

/// `V3Tools.ClampDirection`: the direction and whether it was changed.
pub fn clamp_direction(
    direction: Vec3,
    normal_direction: Vec3,
    clamp_weight: f32,
    clamp_smoothing: i32,
) -> (Vec3, bool) {
    if clamp_weight <= 0.0 {
        return (direction, false);
    }
    if clamp_weight >= 1.0 {
        return (normal_direction, true);
    }
    let num = unity::angle(normal_direction, direction);
    let num2 = 1.0 - num / 180.0;
    if num2 > clamp_weight {
        return (direction, false);
    }
    let num3 = if clamp_weight > 0.0 {
        unity::clamp(1.0 - (clamp_weight - num2) / (1.0 - num2), 0.0, 1.0)
    } else {
        1.0
    };
    let mut num4 = if clamp_weight > 0.0 {
        unity::clamp(num2 / clamp_weight, 0.0, 1.0)
    } else {
        1.0
    };
    for _ in 0..clamp_smoothing {
        let f = num4 * std::f32::consts::PI * 0.5;
        num4 = unity::sin(f);
    }
    (unity::vec_slerp(normal_direction, direction, num4 * num3), true)
}

/// `QuaTools.RotationToLocalSpace`.
pub fn rotation_to_local_space(space: Quat, rotation: Quat) -> Quat {
    unity::inverse(unity::mul(unity::inverse(space), rotation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_and_horizontal_parts_add_up() {
        let v = Vec3::new(0.3, 0.8, -0.2);
        let up = Vec3::Y;
        let sum = extract_vertical(v, up, 1.0) + extract_horizontal(v, up, 1.0);
        assert!((sum - v).length() < 1e-6);
        assert_eq!(extract_vertical(v, up, 0.0), Vec3::ZERO);
        assert_eq!(extract_horizontal(v, up, 0.0), Vec3::ZERO);
    }

    #[test]
    fn clamp_leaves_near_directions_and_pulls_far_ones() {
        let normal = Vec3::X;
        // 45 degrees off: inside a clamp of 0.505 (89.1 degrees).
        let near = Vec3::new(1.0, 1.0, 0.0);
        assert_eq!(clamp_direction(near, normal, 0.505, 0), (near, false));
        // 135 degrees off: pulled back towards the normal.
        let far = Vec3::new(-1.0, 1.0, 0.0);
        let (clamped, changed) = clamp_direction(far, normal, 0.505, 0);
        assert!(changed);
        assert!(unity::angle(normal, clamped) < unity::angle(normal, far));
        assert_eq!(clamp_direction(far, normal, 1.0, 0), (normal, true));
        assert_eq!(clamp_direction(far, normal, 0.0, 0), (far, false));
    }
}
