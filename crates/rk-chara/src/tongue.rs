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
}
