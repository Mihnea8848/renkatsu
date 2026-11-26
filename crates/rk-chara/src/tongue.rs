// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The tongue put out by hand, Renkatsu's own. The game only ever shows
//! its tongue resting in the mouth (mouth pattern 21 swaps in a second
//! tongue mesh, still at rest), but both tongues are skinned to a chain of
//! five bones, `cf_j_tang_01` at the back to `cf_j_tang_05` at the tip,
//! each pointing along its own +Z. Posing that chain puts the tongue out:
//! the root slides forward past the lips and each bone further along
//! bends a little more, so the tongue hangs and curls down at the tip.

use glam::{Quat, Vec3};

use crate::scene::{Scene, Trs};

/// The tongue's bones, back to tip.
pub const CHAIN: [&str; 5] = [
    "cf_j_tang_01",
    "cf_j_tang_02",
    "cf_j_tang_03",
    "cf_j_tang_04",
    "cf_j_tang_05",
];

/// How far the root slides at full amount, in its own space: forward
/// past the lips and a little down onto the lower lip, in metres.
const SLIDE: Vec3 = Vec3::new(0.0, -0.004, 0.034);

/// How far each bone bends down at full amount, in degrees, back to tip:
/// the root tips the whole tongue down out of the mouth, the rest curl it.
const BEND: [f32; 5] = [22.0, 8.0, 12.0, 16.0, 20.0];

/// The chain's bones below `top`, back to tip.
pub fn find(scene: &Scene, top: usize) -> Option<[usize; 5]> {
    let mut out = [0; 5];
    for (slot, name) in out.iter_mut().zip(CHAIN) {
        *slot = scene.find(top, name)?;
    }
    Some(out)
}

/// Puts the tongue out by `amount` (0 to 1), over the chain's local
/// transforms `from` (the animation's, or the rest's).
pub fn put_out(scene: &mut Scene, chain: &[usize; 5], from: &[Trs; 5], amount: f32) {
    let a = amount.clamp(0.0, 1.0);
    for (i, (&node, base)) in chain.iter().zip(from).enumerate() {
        let mut local = *base;
        if i == 0 {
            local.pos += base.rot * (SLIDE * a);
        }
        local.rot = (base.rot * Quat::from_rotation_x((BEND[i] * a).to_radians())).normalize();
        scene.nodes[node].local = local;
    }
}

/// How far below a shaft's middle the tongue lies, in metres: below the
/// shaft's surface, so it shows under it rather than inside it.
const UNDER: f32 = 0.02;

/// How far the tip curls up against a shaft at full amount, in degrees.
const CURL: f32 = 28.0;

/// Puts the tongue out under a shaft by `amount`: out of the mouth along
/// `along` (from the mouth toward his body), on the line `UNDER` below the
/// shaft's middle at `entry`, `down` toward her chin. Each bone turns so
/// the next lands on that line, and the tip curls up against the shaft.
#[allow(clippy::too_many_arguments)]
pub fn lay_under(
    scene: &mut Scene,
    chain: &[usize; 5],
    from: &[Trs; 5],
    amount: f32,
    entry: Vec3,
    along: Vec3,
    down: Vec3,
) {
    let a = amount.clamp(0.0, 1.0);
    for (&node, base) in chain.iter().zip(from) {
        scene.nodes[node].local = *base;
    }
    let root = &mut scene.nodes[chain[0]].local;
    root.pos += from[0].rot * (SLIDE * a);
    let (Some(along), Some(down)) = (along.try_normalize(), down.try_normalize()) else {
        return;
    };
    let line = entry + down * UNDER;
    let world_rot = |scene: &Scene, n: usize| scene.world_of(n).to_scale_rotation_translation().1;
    for i in 0..4 {
        let (bone, child) = (chain[i], chain[i + 1]);
        let at = scene.world_of(bone).w_axis.truncate();
        let reach = scene.world_of(child).w_axis.truncate() - at;
        let length = reach.length();
        // Where on the line the next bone lands: as far along it as the
        // bone reaches, or the nearest point when it cannot reach it.
        let o = line - at;
        let b = o.dot(along);
        let disc = b * b - (o.length_squared() - length * length);
        let s = -b + disc.max(0.0).sqrt();
        let goal = line + along * s.max(-b);
        let turned = Quat::from_rotation_arc(reach.normalize_or_zero(), (goal - at).normalize_or_zero());
        // Only as much of the turn as the tongue is out.
        let turn = Quat::IDENTITY.slerp(turned, a);
        let rot = turn * world_rot(scene, bone);
        let parent = scene.nodes[bone].parent.map_or(Quat::IDENTITY, |p| world_rot(scene, p));
        scene.nodes[bone].local.rot = (parent.inverse() * rot).normalize();
    }
    let tip = &mut scene.nodes[chain[4]].local;
    tip.rot = (tip.rot * Quat::from_rotation_x((-CURL * a).to_radians())).normalize();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> (Scene, [usize; 5], [Trs; 5]) {
        let mut scene = Scene::default();
        let head = scene.add_node("cf_s_head", None, Trs::default());
        let mut parent = head;
        let mut nodes = [0; 5];
        for (i, name) in CHAIN.iter().enumerate() {
            let at = Trs {
                pos: Vec3::new(0.0, 0.0, 0.01),
                ..Default::default()
            };
            parent = scene.add_node(name, Some(parent), at);
            nodes[i] = parent;
        }
        let rest = nodes.map(|n| scene.nodes[n].local);
        (scene, nodes, rest)
    }

    #[test]
    fn the_tongue_comes_out_forward_and_hangs_down_at_the_tip() {
        let (mut scene, nodes, rest) = chain();
        let tip = |scene: &Scene| scene.world_of(nodes[4]).w_axis.truncate();
        let before = tip(&scene);
        put_out(&mut scene, &nodes, &rest, 1.0);
        let after = tip(&scene);
        assert!(after.z > before.z + 0.02, "out: {before} -> {after}");
        assert!(after.y < before.y - 0.01, "hangs: {before} -> {after}");
        // None puts it back as it was.
        put_out(&mut scene, &nodes, &rest, 0.0);
        assert!(tip(&scene).distance(before) < 1e-6);
    }

    #[test]
    fn under_a_shaft_the_tongue_lies_along_it() {
        let (mut scene, nodes, rest) = chain();
        // A shaft coming in level, its middle at the mouth, along -Z.
        let entry = Vec3::new(0.0, 0.0, 0.03);
        lay_under(&mut scene, &nodes, &rest, 1.0, entry, Vec3::Z, -Vec3::Y);
        let at = |i: usize| scene.world_of(nodes[i]).w_axis.truncate();
        // The middle bones lie on the line under the shaft.
        for i in 2..4 {
            assert!((at(i).y + UNDER).abs() < 0.004, "bone {i} at {}", at(i));
        }
        assert!(at(4).z > entry.z + 0.03, "reaches out past the mouth: {}", at(4));
    }
}
