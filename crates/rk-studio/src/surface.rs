// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! What an object can be dropped on. Snap to floor asks every surface of
//! the scene what lies under a point and takes the first it meets. The
//! characters and the map answer with their meshes (`live.rs`). Without a
//! map, under them all is the ground, at height 0.

use glam::Vec3;

/// Something a ray can meet.
pub trait Surface {
    /// How far a ray from `origin` along `dir` goes before it meets the
    /// surface.
    fn meet(&self, origin: Vec3, dir: Vec3) -> Option<f32>;
}

/// How far above a point the ray down starts, so that a surface the
/// point already rests on is still met.
const LIFT: f32 = 1e-3;

/// The height of what lies under a point: the first of the surfaces
/// straight below it, or the ground when there is one. A point under the
/// ground comes back up to it. Nothing below and no ground gives `None`.
pub fn floor_below(point: Vec3, surfaces: &[&dyn Surface], ground: Option<f32>) -> Option<f32> {
    let from = point + Vec3::Y * LIFT;
    let nearest = surfaces
        .iter()
        .filter_map(|surface| surface.meet(from, Vec3::NEG_Y))
        .min_by(f32::total_cmp)
        .map(|far| from.y - far);
    match (nearest, ground) {
        (Some(floor), Some(ground)) => Some(floor.max(ground)),
        (floor, ground) => floor.or(ground),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A level plate of one metre around the origin at a height.
    struct Plate(f32);

    impl Surface for Plate {
        fn meet(&self, origin: Vec3, dir: Vec3) -> Option<f32> {
            let far = (self.0 - origin.y) / dir.y;
            let at = origin + dir * far;
            (far > 0.0 && at.x.abs() <= 0.5 && at.z.abs() <= 0.5).then_some(far)
        }
    }

    #[test]
    fn a_point_falls_on_the_first_surface_below_it() {
        let (low, high) = (Plate(0.4), Plate(1.0));
        let surfaces: [&dyn Surface; 2] = [&low, &high];
        assert!((floor_below(Vec3::new(0.0, 2.0, 0.0), &surfaces, Some(0.0)).unwrap() - 1.0).abs() < 1e-5);
        // Between the two it falls on the lower one.
        assert!((floor_below(Vec3::new(0.0, 0.7, 0.0), &surfaces, Some(0.0)).unwrap() - 0.4).abs() < 1e-5);
        // What rests on a surface stays on it.
        assert!((floor_below(Vec3::new(0.0, 1.0, 0.0), &surfaces, Some(0.0)).unwrap() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_ground_is_under_everything() {
        let plate = Plate(0.4);
        // Beside the plate there is only the ground.
        assert_eq!(floor_below(Vec3::new(3.0, 2.0, 0.0), &[&plate], Some(0.0)), Some(0.0));
        // A point under the ground comes back up.
        assert_eq!(floor_below(Vec3::new(0.0, -1.5, 0.0), &[], Some(0.0)), Some(0.0));
        // A surface under the ground is not fallen through to.
        assert_eq!(
            floor_below(Vec3::new(0.0, 2.0, 0.0), &[&Plate(-1.0)], Some(0.0)),
            Some(0.0)
        );
    }

    #[test]
    fn a_map_has_no_ground_under_it() {
        // A map's floor may lie under height 0.
        let floor = floor_below(Vec3::new(0.0, 2.0, 0.0), &[&Plate(-1.0)], None).unwrap();
        assert!((floor + 1.0).abs() < 1e-5);
        // Beside the map there is nothing to fall to.
        assert_eq!(floor_below(Vec3::new(3.0, 2.0, 0.0), &[&Plate(-1.0)], None), None);
    }
}
