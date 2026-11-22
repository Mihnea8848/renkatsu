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

/// Somewhere a character can stand near `point`, on any map, the game's
/// or a mod's, and be seen from `to_camera` (the way to the camera).
///
/// A spot is a floor with head room over it (not under a table), room
/// around it at the knees and the chest (not in a wall or a locker), and
/// nothing between it and the camera as far as it frames her. Of the spots
/// found going round `point`, a quarter metre a ring, the nearest wins,
/// a step up or down counting double: a floor beside a table wins over
/// the table's top.
pub fn stand_spot(point: Vec3, to_camera: Vec3, surface: &dyn Surface) -> Option<Vec3> {
    // Near and fine; a map whose floor is far from its origin, or whose
    // near floors are walled in, gets a coarser look further out and down.
    let near = search(point, to_camera, surface, 0.25, 12.0, 20.0);
    if near.is_some_and(|(score, _)| score == 0.0) {
        return near.map(|(_, spot)| spot);
    }
    let far = search(point, to_camera, surface, 2.0, 60.0, 200.0);
    [near, far]
        .into_iter()
        .flatten()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, spot)| spot)
}

fn search(
    point: Vec3,
    to_camera: Vec3,
    surface: &dyn Surface,
    step: f32,
    reach: f32,
    depth: f32,
) -> Option<(f32, Vec3)> {
    let mut best: Option<(f32, Vec3)> = None;
    let mut ring = 0u32;
    loop {
        let r = ring as f32 * step;
        // Nothing further out can be nearer than the best found.
        if r > reach || best.is_some_and(|(score, _)| r > score) {
            break;
        }
        let steps = if ring == 0 {
            1
        } else {
            (r * std::f32::consts::TAU / step).ceil() as u32
        };
        for k in 0..steps {
            // Round from the camera's side, so ties go to the front.
            let side = Vec3::new(to_camera.x, 0.0, to_camera.z).normalize_or(Vec3::Z);
            let angle = k as f32 / steps as f32 * std::f32::consts::TAU;
            let (sin, cos) = angle.sin_cos();
            let across = Vec3::new(side.z, 0.0, -side.x);
            let column = point + (side * cos + across * sin) * r;
            for floor in floors(column, depth, surface) {
                // A metre down, or a metre short of a full view, weighs as
                // three metres further out; a metre up as eight, so that a
                // walled-in floor beats the roof over it.
                let rise = floor.y - point.y;
                let score = r + if rise > 0.0 { 8.0 * rise } else { -3.0 * rise };
                if best.is_some_and(|(b, _)| score >= b) {
                    continue;
                }
                if let Some(short) = room_at(floor, to_camera, surface) {
                    let score = score + 3.0 * short;
                    if best.is_none_or(|(b, _)| score < b) {
                        best = Some((score, floor));
                    }
                }
            }
        }
        ring += 1;
    }
    best
}

/// The floors of a column through `at`, top down, from ten metres above it
/// to `depth` below.
fn floors(at: Vec3, depth: f32, surface: &dyn Surface) -> Vec<Vec3> {
    let mut found = Vec::new();
    let mut from = at + Vec3::Y * 10.0;
    let bottom = at.y - depth;
    while let Some(far) = surface.meet(from, Vec3::NEG_Y) {
        let hit = from - Vec3::Y * far;
        if hit.y < bottom || found.len() > 16 {
            break;
        }
        found.push(hit);
        from = hit - Vec3::Y * 0.02;
    }
    found
}

/// Whether a character fits on `floor` with the camera's side open, and
/// how many metres short of a full view that side is: a small room's
/// floor still beats the roof over it, a roomy spot beats both.
fn room_at(floor: Vec3, to_camera: Vec3, surface: &dyn Surface) -> Option<f32> {
    const HEAD: f32 = 1.8;
    const ELBOWS: f32 = 0.25;
    // CharaStudio's narrow lens frames a character from about five metres.
    const SIGHT: f32 = 6.0;
    // Less open than this, the camera would stand at her nose.
    const NEAREST: f32 = 1.0;
    let open = |from: Vec3, dir: Vec3, reach: f32| surface.meet(from, dir).is_none_or(|far| far > reach);
    let up = floor + Vec3::Y * 0.05;
    if !open(up, Vec3::Y, HEAD) {
        return None;
    }
    // Nothing between the chest and the feet: not at a table's edge.
    let chest = floor + Vec3::Y * 1.2;
    let down = surface.meet(chest, Vec3::NEG_Y);
    if down.is_none_or(|far| (chest.y - far - floor.y).abs() > 0.02) {
        return None;
    }
    let round = [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z];
    for height in [0.3, 1.2] {
        let at = floor + Vec3::Y * height;
        if !round.iter().all(|dir| open(at, *dir, ELBOWS)) {
            return None;
        }
    }
    let to_camera = to_camera.normalize_or(Vec3::Z);
    // From the chest and both shoulders: half of her behind a corner is
    // not a view.
    let across = Vec3::new(to_camera.z, 0.0, -to_camera.x).normalize_or(Vec3::X) * 0.3;
    let sight = [Vec3::ZERO, across, -across]
        .iter()
        .map(|side| surface.meet(chest + *side, to_camera).unwrap_or(SIGHT))
        .fold(SIGHT, f32::min);
    (sight >= NEAREST).then_some(SIGHT - sight)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Boxes, each from its low corner to its high one, as a surface.
    struct Boxes(Vec<(Vec3, Vec3)>);

    impl Surface for Boxes {
        fn meet(&self, origin: Vec3, dir: Vec3) -> Option<f32> {
            self.0
                .iter()
                .filter_map(|(lo, hi)| {
                    let inv = dir.recip();
                    let (a, b) = ((*lo - origin) * inv, (*hi - origin) * inv);
                    let near = a.min(b).max_element();
                    let far = a.max(b).min_element();
                    // From outside, the near face; from inside, the far.
                    (far >= near && far > 1e-4).then_some(if near > 1e-4 { near } else { far })
                })
                .min_by(f32::total_cmp)
        }
    }

    fn floor() -> (Vec3, Vec3) {
        (Vec3::new(-20.0, -0.1, -20.0), Vec3::new(20.0, 0.0, 20.0))
    }

    #[test]
    fn a_character_stands_beside_a_table_not_on_it_nor_in_it() {
        let table = (Vec3::new(-0.55, 0.0, -0.55), Vec3::new(0.55, 0.7, 0.55));
        let room = Boxes(vec![floor(), table]);
        let spot = stand_spot(Vec3::ZERO, Vec3::Z, &room).unwrap();
        assert!(spot.y.abs() < 1e-3, "{spot}");
        // Out of the table, on the camera's side.
        assert!(spot.z > 0.55 && spot.x.abs() < 0.3, "{spot}");
    }

    #[test]
    fn a_character_in_a_closet_comes_out_where_the_camera_sees_her() {
        // A closet of 0.6 m round the origin, open nowhere.
        let walls = vec![
            floor(),
            (Vec3::new(-0.35, 0.0, -0.35), Vec3::new(-0.3, 2.0, 0.35)),
            (Vec3::new(0.3, 0.0, -0.35), Vec3::new(0.35, 2.0, 0.35)),
            (Vec3::new(-0.35, 0.0, -0.35), Vec3::new(0.35, 2.0, -0.3)),
            (Vec3::new(-0.35, 0.0, 0.3), Vec3::new(0.35, 2.0, 0.35)),
        ];
        let spot = stand_spot(Vec3::ZERO, Vec3::Z, &Boxes(walls)).unwrap();
        assert!(spot.z > 0.6, "{spot}");
    }

    #[test]
    fn a_character_in_a_small_room_stands_on_its_floor_not_its_roof() {
        // A room of 4 by 3 m, 3 m high, with a flat roof on top and no
        // floor outside: nowhere inside is six metres open toward the
        // camera, the roof is.
        let walls = vec![
            (Vec3::new(-2.0, -0.1, -1.5), Vec3::new(2.0, 0.0, 1.5)),
            (Vec3::new(-2.0, 0.0, -1.5), Vec3::new(-1.9, 3.0, 1.5)),
            (Vec3::new(1.9, 0.0, -1.5), Vec3::new(2.0, 3.0, 1.5)),
            (Vec3::new(-2.0, 0.0, -1.5), Vec3::new(2.0, 3.0, -1.4)),
            (Vec3::new(-2.0, 0.0, 1.4), Vec3::new(2.0, 3.0, 1.5)),
            (Vec3::new(-2.0, 3.0, -1.5), Vec3::new(2.0, 3.5, 1.5)),
        ];
        let spot = stand_spot(Vec3::ZERO, Vec3::Z, &Boxes(walls)).unwrap();
        assert!(
            spot.y.abs() < 1e-3 && spot.x.abs() < 1.9 && spot.z.abs() < 1.4,
            "{spot}"
        );
    }

    #[test]
    fn a_character_over_nothing_lands_on_the_floor_below() {
        let low = Boxes(vec![(Vec3::new(-20.0, -5.1, -20.0), Vec3::new(20.0, -5.0, 20.0))]);
        let spot = stand_spot(Vec3::ZERO, Vec3::Z, &low).unwrap();
        assert!((spot.y + 5.0).abs() < 1e-3, "{spot}");
    }

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
