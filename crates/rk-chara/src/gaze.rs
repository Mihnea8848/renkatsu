// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Where the eyes look (`EyeLookCalc.EyeUpdateCalc`). The head's script
//! turns each eye toward a target within its pattern's limits, eased by the
//! pattern's speed, and hands the turn on as two rates from -1 to 1, which
//! the irises' scripts turn into the texture's place (`crate::eyelook`).
//! Studio's eye patterns are the script's `eyeTypeStates`: front (a point
//! ahead of the face), toward the camera, a controlled target, looking
//! away, and the same four for H. The game reads the camera as the target.

use glam::{Mat4, Quat, Vec3};
use rk_ik::unity;
use rk_unityfs::UValue;

/// How a pattern looks (`EYE_LOOK_TYPE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookType {
    NoLook,
    Target,
    Away,
    Forward,
    Control,
}

impl LookType {
    fn of(n: i64) -> LookType {
        match n {
            1 => LookType::Target,
            2 => LookType::Away,
            3 => LookType::Forward,
            4 => LookType::Control,
            _ => LookType::NoLook,
        }
    }
}

/// One pattern (`EyeTypeState`), angles in degrees.
#[derive(Debug, Clone, PartialEq)]
pub struct EyeTypeState {
    pub name: String,
    pub threshold_angle_difference: f32,
    pub bending_multiplier: f32,
    pub max_angle_difference: f32,
    pub up_bending_angle: f32,
    pub down_bending_angle: f32,
    pub min_bending_angle: f32,
    pub max_bending_angle: f32,
    pub leap_speed: f32,
    pub front_target_distance: f32,
    pub near_distance: f32,
    pub h_angle_limit: f32,
    pub v_angle_limit: f32,
    pub look_type: LookType,
}

impl EyeTypeState {
    pub fn read(v: &UValue) -> Option<EyeTypeState> {
        let f = |name: &str| v.get(name).and_then(UValue::as_f32).unwrap_or(0.0);
        Some(EyeTypeState {
            name: v.get("comment").and_then(UValue::as_str).unwrap_or_default().to_owned(),
            threshold_angle_difference: f("thresholdAngleDifference"),
            bending_multiplier: f("bendingMultiplier"),
            max_angle_difference: f("maxAngleDifference"),
            up_bending_angle: f("upBendingAngle"),
            down_bending_angle: f("downBendingAngle"),
            min_bending_angle: f("minBendingAngle"),
            max_bending_angle: f("maxBendingAngle"),
            leap_speed: f("leapSpeed"),
            front_target_distance: f("forntTagDis"),
            near_distance: f("nearDis"),
            h_angle_limit: f("hAngleLimit"),
            v_angle_limit: f("vAngleLimit"),
            look_type: LookType::of(v.get("lookType")?.as_i64()?),
        })
    }
}

/// One eye (`EyeObject`).
#[derive(Debug, Clone, PartialEq)]
struct Eye {
    node: usize,
    right: bool,
    reference_look: Vec3,
    reference_up: Vec3,
    angle_h: f32,
    angle_v: f32,
}

/// The head's look script, set up as `Init` leaves it.
#[derive(Debug, Clone, PartialEq)]
pub struct EyeLook {
    root: usize,
    center: Option<usize>,
    center_eye_length: f32,
    correct: bool,
    sorasi_rate: f32,
    pub states: Vec<EyeTypeState>,
    eyes: Vec<Eye>,
}

pub(crate) fn rotation(m: &Mat4) -> Quat {
    m.to_scale_rotation_translation().1
}

fn position(m: &Mat4) -> Vec3 {
    m.w_axis.truncate()
}

/// `Mathf.Lerp`, its rate clamped.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// `Mathf.InverseLerp`.
pub(crate) fn inverse_lerp(a: f32, b: f32, v: f32) -> f32 {
    if a == b {
        0.0
    } else {
        ((v - a) / (b - a)).clamp(0.0, 1.0)
    }
}

/// `Mathf.Sign`: 1 for zero.
fn sign(v: f32) -> f32 {
    if v >= 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// `EyeLookCalc.AngleAroundAxis`.
pub(crate) fn angle_around_axis(a: Vec3, b: Vec3, axis: Vec3) -> f32 {
    let a = a - unity::project(a, axis);
    let b = b - unity::project(b, axis);
    let angle = unity::angle(a, b);
    if unity::dot(axis, unity::cross(a, b)) < 0.0 {
        -angle
    } else {
        angle
    }
}

impl EyeLook {
    /// From the script's values and the world matrices at the pose it is
    /// set up in (`Init`): each eye's reference look and up in its
    /// parent's space.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        world: &[Mat4],
        parents: &dyn Fn(usize) -> Option<usize>,
        root: usize,
        center: Option<usize>,
        center_eye_length: f32,
        correct: bool,
        sorasi_rate: f32,
        head_look: Vec3,
        head_up: Vec3,
        eyes: &[(usize, bool)],
        states: Vec<EyeTypeState>,
    ) -> Option<EyeLook> {
        let root_turn = rotation(&world[root]);
        let eyes = eyes
            .iter()
            .map(|&(node, right)| {
                let parent = rotation(&world[parents(node)?]);
                let back = unity::inverse(parent);
                Some(Eye {
                    node,
                    right,
                    reference_look: unity::rotate(unity::mul(back, root_turn), unity::normalize(head_look)),
                    reference_up: unity::rotate(unity::mul(back, root_turn), unity::normalize(head_up)),
                    angle_h: 0.0,
                    angle_v: 0.0,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        (eyes.len() == 2 && !states.is_empty()).then_some(EyeLook {
            root,
            center,
            center_eye_length,
            correct,
            sorasi_rate,
            states,
            eyes,
        })
    }

    /// One frame looking at `target` in pattern `ptn` (`EyeUpdateCalc`),
    /// `dt` seconds after the last. Returns each eye's sideways rate and
    /// the upward rate, as the irises' scripts read them. The eyes go by
    /// their place, as the game gives each its target by the eye's index.
    #[allow(clippy::needless_range_loop)]
    pub fn update(
        &mut self,
        world: &[Mat4],
        parents: &dyn Fn(usize) -> Option<usize>,
        target: Vec3,
        ptn: usize,
        dt: f32,
    ) -> ([f32; 2], f32) {
        let ptn = ptn.min(self.states.len() - 1);
        if dt <= 0.0 {
            return self.rates(ptn);
        }
        let state = self.states[ptn].clone();
        let mut look = state.look_type;
        // Studio's controlled target is an object of its own, not ported.
        if look == LookType::Control {
            look = LookType::Target;
        }
        if look == LookType::NoLook {
            return self.rates(ptn);
        }
        let root = world[self.root];
        let root_at = position(&root);
        let root_forward = unity::rotate(rotation(&root), Vec3::Z);
        let mut target = target;
        let mut to = target - root_at;
        if look != LookType::Target && unity::distance(target, root_at) < state.near_distance {
            to = unity::normalize(to) * state.near_distance;
            target = root_at + to;
        }
        let h = unity::angle(Vec3::new(to.x, root_forward.y, to.z), root_forward);
        let v = unity::angle(Vec3::new(root_forward.x, to.y, to.z), root_forward);
        if h > state.h_angle_limit || v > state.v_angle_limit {
            look = LookType::Forward;
        }
        if look == LookType::Forward {
            // `n_cam_front`: under the root, turned 5 degrees down.
            let front = unity::mul(rotation(&root), Quat::from_rotation_x(5f32.to_radians()));
            target = root_at + unity::rotate(front, Vec3::Z) * state.front_target_distance;
        }
        // Each eye at its own point either side of the target, so that the
        // two converge on it (`n_cam_tl`, `n_cam_tr`).
        let mut aims = [target, target];
        if let (true, Some(center), LookType::Target | LookType::Forward) = (self.correct, self.center, look) {
            let c = world[center];
            let mut local = c.inverse().transform_point3(target);
            local.z = local.z.max(0.5);
            let at = c.transform_point3(local);
            let toward = unity::normalize(at - position(&c));
            let side = unity::normalize(unity::cross(Vec3::Y, toward));
            let ahead = unity::normalize(unity::cross(side, Vec3::Y));
            let turn = unity::look_rotation(ahead, Vec3::Y);
            let scale = c.to_scale_rotation_translation().0.x.abs();
            for (i, x) in [-self.center_eye_length, self.center_eye_length]
                .into_iter()
                .enumerate()
            {
                aims[i] = at + unity::rotate(turn, Vec3::new(x * scale, 0.0, 0.0));
            }
        }
        let mut away_pick = -1.0f32;
        for i in 0..self.eyes.len() {
            let eye = &self.eyes[i];
            let Some(parent) = parents(eye.node) else { continue };
            let back = unity::inverse(rotation(&world[parent]));
            let goal = match look {
                LookType::Target | LookType::Forward => aims[i],
                _ => target,
            };
            let dir = unity::rotate(back, unity::normalize(goal - position(&world[eye.node])));
            let mut f = angle_around_axis(eye.reference_look, dir, eye.reference_up);
            let axis = unity::cross(eye.reference_up, dir);
            let flat = dir - unity::project(dir, eye.reference_up);
            let mut f2 = angle_around_axis(flat, dir, axis);
            let f3 = (f.abs() - state.threshold_angle_difference).max(0.0) * sign(f);
            let f4 = (f2.abs() - state.threshold_angle_difference).max(0.0) * sign(f2);
            let bend = state.bending_multiplier;
            f = (f3.abs() * bend.abs()).max(f.abs() - state.max_angle_difference) * sign(f) * sign(bend);
            f2 = (f4.abs() * bend.abs()).max(f2.abs() - state.max_angle_difference) * sign(f2) * sign(bend);
            let (min, max) = if eye.right {
                (-state.max_bending_angle, -state.min_bending_angle)
            } else {
                (state.min_bending_angle, state.max_bending_angle)
            };
            f = f.clamp(min, max);
            f2 = f2.clamp(state.up_bending_angle, state.down_bending_angle);
            if look == LookType::Away {
                let (lo, hi) = (-state.max_bending_angle, -state.min_bending_angle);
                if away_pick == -1.0 {
                    let mut now = lerp(-1.0, 1.0, inverse_lerp(lo, hi, eye.angle_h));
                    let wanted = lerp(-1.0, 1.0, inverse_lerp(lo, hi, f));
                    let apart = now - wanted;
                    let r = self.sorasi_rate;
                    if apart.abs() < r {
                        now = if apart < 0.0 {
                            if wanted < -r {
                                wanted + r
                            } else {
                                wanted - r
                            }
                        } else if apart > 0.0 {
                            if wanted > r {
                                wanted - r
                            } else {
                                wanted + r
                            }
                        } else {
                            wanted + r
                        };
                        away_pick = inverse_lerp(-1.0, 1.0, now);
                        f = lerp(lo, hi, away_pick);
                    } else {
                        away_pick = inverse_lerp(-1.0, 1.0, now);
                        f = eye.angle_h;
                    }
                } else {
                    f = lerp(lo, hi, away_pick);
                }
                f2 = -f2;
            }
            let t = dt * state.leap_speed;
            let eye = &mut self.eyes[i];
            eye.angle_h = lerp(eye.angle_h, f, t);
            eye.angle_v = lerp(eye.angle_v, f2, t);
        }
        self.rates(ptn)
    }

    /// `AngleHRateCalc` and `AngleVRateCalc`.
    fn rates(&self, ptn: usize) -> ([f32; 2], f32) {
        let s = &self.states[ptn.min(self.states.len() - 1)];
        let mut h = [0.0; 2];
        for (i, eye) in self.eyes.iter().enumerate().take(2) {
            h[i] = if eye.right {
                lerp(
                    -1.0,
                    1.0,
                    inverse_lerp(-s.max_bending_angle, -s.min_bending_angle, eye.angle_h),
                )
            } else {
                lerp(
                    -1.0,
                    1.0,
                    inverse_lerp(s.min_bending_angle, s.max_bending_angle, eye.angle_h),
                )
            };
        }
        let a = self.eyes[0].angle_v;
        let v = if s.down_bending_angle <= s.up_bending_angle {
            if a >= 0.0 {
                -inverse_lerp(0.0, s.up_bending_angle, a)
            } else {
                inverse_lerp(0.0, s.down_bending_angle, a)
            }
        } else if a >= 0.0 {
            -inverse_lerp(0.0, s.down_bending_angle, a)
        } else {
            inverse_lerp(0.0, s.up_bending_angle, a)
        };
        // Rates for the left eye first, as the irises ask by side.
        let order = if self.eyes[0].right { [h[1], h[0]] } else { h };
        (order, v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(look: LookType) -> EyeTypeState {
        // Studio's front pattern, from the game's head.
        EyeTypeState {
            name: "front".into(),
            threshold_angle_difference: 0.0,
            bending_multiplier: 0.4,
            max_angle_difference: 10.0,
            up_bending_angle: -30.0,
            down_bending_angle: 10.0,
            min_bending_angle: -36.0,
            max_bending_angle: 23.0,
            leap_speed: 22.8,
            front_target_distance: 1.0,
            near_distance: 2.0,
            h_angle_limit: 110.0,
            v_angle_limit: 80.0,
            look_type: look,
        }
    }

    /// A head at the origin facing +z, eyes 3 cm either side.
    fn head(look: LookType) -> (Vec<Mat4>, EyeLook) {
        let world = vec![
            Mat4::IDENTITY,
            Mat4::from_translation(Vec3::new(0.0, 0.0, 0.05)),
            Mat4::from_translation(Vec3::new(-0.03, 0.0, 0.05)),
            Mat4::from_translation(Vec3::new(0.03, 0.0, 0.05)),
        ];
        let parents = |n: usize| (n > 1).then_some(0);
        let gaze = EyeLook::new(
            &world,
            &parents,
            0,
            Some(1),
            0.03,
            true,
            1.0,
            Vec3::Z,
            Vec3::Y,
            &[(2, false), (3, true)],
            vec![state(look)],
        )
        .unwrap();
        (world, gaze)
    }

    #[test]
    fn eyes_turn_toward_a_target_and_settle() {
        let (world, mut gaze) = head(LookType::Target);
        let parents = |n: usize| (n > 1).then_some(0);
        // A target up and to the side: both eyes turn the same way, the
        // rates grow, then hold.
        let target = Vec3::new(0.6, 0.4, 1.0);
        let first = gaze.update(&world, &parents, target, 0, 1.0 / 60.0);
        let mut last = first;
        for _ in 0..120 {
            last = gaze.update(&world, &parents, target, 0, 1.0 / 60.0);
        }
        assert!(last.0[0].abs() > first.0[0].abs(), "eased toward the target");
        assert!(last.0[0].signum() == last.0[1].signum() || last.0[1] == 0.0);
        assert!(last.1 != 0.0, "up or down moves the irises too");
        let again = gaze.update(&world, &parents, target, 0, 1.0 / 60.0);
        assert!((again.0[0] - last.0[0]).abs() < 1e-4, "settled");
        // Without time passing nothing moves.
        assert_eq!(gaze.update(&world, &parents, Vec3::new(-1.0, 0.0, 1.0), 0, 0.0), again);
    }

    #[test]
    fn a_built_character_looks_with_the_games_patterns() {
        let Ok(game) = rk_vfs::Game::open_user(&[]) else { return };
        let Some(dir) = rk_core::Config::load().ok().map(|c| c.game_dir) else {
            return;
        };
        let Ok(card) = rk_card::Card::load(dir.join("UserData/chara/female/Simple.png")) else {
            return;
        };
        let opts = crate::BuildOptions {
            clothes: false,
            accessories: false,
            ..Default::default()
        };
        let Ok(chara) = crate::build(&game, &card, &opts) else {
            return;
        };
        let mut gaze = chara.gaze.clone().expect("the head's eye look");
        assert_eq!(gaze.states.len(), 8);
        assert_eq!(gaze.states[0].look_type, LookType::Forward);
        assert_eq!(gaze.states[1].look_type, LookType::Target);
        let (world, _) = chara.scene.world();
        let parents = |n: usize| chara.scene.nodes[n].parent;
        // A camera off to her side and above: "this way" turns the eyes.
        let camera = Vec3::new(0.8, 1.8, 1.5);
        let mut rates = ([0.0; 2], 0.0);
        for _ in 0..60 {
            rates = gaze.update(&world, &parents, camera, 1, 1.0 / 60.0);
        }
        eprintln!("rates {rates:?}");
        assert!(rates.0[0].abs() > 0.05 && rates.0[1].abs() > 0.05, "{rates:?}");
        assert!(rates.1.abs() > 0.01, "{rates:?}");
    }

    #[test]
    fn a_target_behind_the_head_is_looked_past() {
        let (world, mut gaze) = head(LookType::Target);
        let parents = |n: usize| (n > 1).then_some(0);
        let mut ahead = ([0.0; 2], 0.0);
        let (_, mut front) = head(LookType::Forward);
        for _ in 0..120 {
            ahead = gaze.update(&world, &parents, Vec3::new(0.0, 0.0, -2.0), 0, 1.0 / 60.0);
            front.update(&world, &parents, Vec3::ZERO, 0, 1.0 / 60.0);
        }
        // Past the side limit the eyes look ahead, as the front pattern does.
        let fwd = front.update(&world, &parents, Vec3::ZERO, 0, 1.0 / 60.0);
        assert!((ahead.0[0] - fwd.0[0]).abs() < 1e-3 && (ahead.1 - fwd.1).abs() < 1e-3);
    }
}
