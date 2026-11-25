// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Where the head turns (`NeckLookCalcVer2`, driven as
//! `NeckLookControllerVer2.LateUpdate` drives it). The neck's script turns
//! the neck and head bones toward a target, each within its pattern's
//! limits, eased by the pattern's speed, after the animation has posed
//! them. A change of pattern blends from the old turn to the new over the
//! script's `changeTypeLeapTime`, along its curve. Studio's neck patterns
//! are the script's `neckTypeStates`: front, toward the camera, away from
//! it, the animation's own, held where it is, and two for H.

use glam::{Mat4, Quat, Vec2, Vec3};
use rk_ik::unity;
use rk_unityfs::UValue;

use crate::gaze::{angle_around_axis, inverse_lerp, rotation};
use crate::scene::Scene;

/// What a pattern does (`NECK_LOOK_TYPE_VER2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookType {
    Animation,
    Target,
    Away,
    Forward,
    Fix,
    Control,
}

impl LookType {
    fn of(n: i64) -> LookType {
        match n {
            1 => LookType::Target,
            2 => LookType::Away,
            3 => LookType::Forward,
            4 => LookType::Fix,
            5 => LookType::Control,
            _ => LookType::Animation,
        }
    }
}

/// One bone's limits in a pattern (`NeckLimitParameter`), in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limit {
    pub up: f32,
    pub down: f32,
    pub min: f32,
    pub max: f32,
}

/// One pattern (`NeckTypeStateVer2`), angles in degrees.
#[derive(Debug, Clone, PartialEq)]
pub struct NeckTypeState {
    pub limits: Vec<Limit>,
    pub leap_speed: f32,
    pub h_limit: f32,
    pub v_limit: f32,
    pub limit_break: f32,
    pub limit_away: f32,
    pub look_type: LookType,
    /// `isLimitBreakBackup`: the target was outside the limits last frame.
    broke: bool,
}

impl NeckTypeState {
    pub fn read(v: &UValue) -> Option<NeckTypeState> {
        let f = |name: &str| v.get(name).and_then(UValue::as_f32);
        let limits = v
            .get("aParam")?
            .as_array()?
            .iter()
            .map(|p| {
                let g = |name: &str| p.get(name).and_then(UValue::as_f32);
                Some(Limit {
                    up: g("upBendingAngle")?,
                    down: g("downBendingAngle")?,
                    min: g("minBendingAngle")?,
                    max: g("maxBendingAngle")?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(NeckTypeState {
            limits,
            leap_speed: f("leapSpeed")?,
            h_limit: f("hAngleLimit")?,
            v_limit: f("vAngleLimit")?,
            limit_break: f("limitBreakCorrectionValue")?,
            limit_away: f("limitAway")?,
            look_type: LookType::of(v.get("lookType")?.as_i64()?),
            broke: false,
        })
    }
}

/// A Unity `AnimationCurve` of plain Hermite keys: time, value, in and
/// out slopes. Clamped at its ends.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve(pub Vec<[f32; 4]>);

impl Curve {
    pub fn read(v: &UValue) -> Option<Curve> {
        let keys = v
            .get("m_Curve")?
            .as_array()?
            .iter()
            .map(|k| {
                let g = |name: &str| k.get(name).and_then(UValue::as_f32);
                Some([g("time")?, g("value")?, g("inSlope")?, g("outSlope")?])
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Curve(keys))
    }

    pub fn evaluate(&self, t: f32) -> f32 {
        let keys = &self.0;
        let (Some(first), Some(last)) = (keys.first(), keys.last()) else {
            return t;
        };
        if t <= first[0] {
            return first[1];
        }
        if t >= last[0] {
            return last[1];
        }
        let i = keys.iter().position(|k| k[0] > t).unwrap_or(keys.len() - 1);
        let (a, b) = (keys[i - 1], keys[i]);
        let dt = b[0] - a[0];
        let s = (t - a[0]) / dt;
        let (s2, s3) = (s * s, s * s * s);
        let (m0, m1) = (a[3] * dt, b[2] * dt);
        (2.0 * s3 - 3.0 * s2 + 1.0) * a[1] + (s3 - 2.0 * s2 + s) * m0 + (-2.0 * s3 + 3.0 * s2) * b[1] + (s3 - s2) * m1
    }
}

/// One turned bone (`NeckObjectVer2`).
#[derive(Debug, Clone)]
struct Bone {
    /// `referenceCalc`: the node the turn is measured from.
    reference: usize,
    /// `neckBone`: the node turned.
    bone: usize,
    /// `fixAngle`: the local rotation worked out.
    fix: Quat,
    /// `fixAngleBackup`, the turn a new pattern blends from; none until a
    /// pattern changes, as the game's zero quaternion slerps to the other.
    fix_backup: Option<Quat>,
    /// `backupLocalRotaionByTarget`, the animation's local rotation.
    by_target: Option<Quat>,
    angle_h: f32,
    angle_v: f32,
}

#[derive(Debug, Clone)]
pub struct NeckLook {
    /// `transformAim`, a point on the face.
    aim: usize,
    /// `boneCalcAngle`, whose axes the angles are read along.
    calc: usize,
    bones: Vec<Bone>,
    pub states: Vec<NeckTypeState>,
    change_time: f32,
    change_curve: Option<Curve>,
    calc_lerp: f32,
    look_type: LookType,
    change_timer: f32,
    now_angle: Vec2,
    /// The bones' local rotations before the script turned them, to turn
    /// from again while the pose stays as it was.
    base: Option<Vec<Quat>>,
}

fn world_rot(scene: &Scene, node: usize) -> Quat {
    rotation(&scene.world_of(node))
}

/// Sets a node's world rotation (`Transform.rotation`).
fn set_world_rot(scene: &mut Scene, node: usize, rot: Quat) {
    let parent = scene.nodes[node].parent.map_or(Quat::IDENTITY, |p| world_rot(scene, p));
    scene.nodes[node].local.rot = (unity::inverse(parent) * rot).normalize();
}

/// `Quaternion.Slerp` from the game's blend start, which is the other
/// rotation itself while there is none.
fn slerp(from: Option<Quat>, to: Quat, t: f32) -> Quat {
    from.map_or(to, |from| unity::quat_slerp(from, to, t.clamp(0.0, 1.0)))
}

impl NeckLook {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        aim: usize,
        calc: usize,
        bones: &[(usize, usize, Quat)],
        states: Vec<NeckTypeState>,
        change_time: f32,
        change_curve: Option<Curve>,
        calc_lerp: f32,
    ) -> Option<NeckLook> {
        if bones.is_empty() || states.is_empty() {
            return None;
        }
        Some(NeckLook {
            aim,
            calc,
            bones: bones
                .iter()
                .map(|&(reference, bone, fix)| Bone {
                    reference,
                    bone,
                    fix,
                    fix_backup: None,
                    by_target: None,
                    angle_h: 0.0,
                    angle_v: 0.0,
                })
                .collect(),
            states,
            change_time,
            change_curve,
            calc_lerp,
            // `Init`.
            look_type: LookType::Animation,
            change_timer: 0.0,
            now_angle: Vec2::ZERO,
            base: None,
        })
    }

    /// The turned bones, neck first.
    pub fn bones(&self) -> impl Iterator<Item = usize> + '_ {
        self.bones.iter().map(|b| b.bone)
    }

    /// Takes the bones as the pose just left them, to turn from.
    pub fn posed(&mut self, scene: &Scene) {
        self.base = Some(self.bones.iter().map(|b| scene.nodes[b.bone].local.rot).collect());
    }

    /// Puts the bones back as the pose left them, for a frame that did not
    /// pose them again. The pose is taken when none was.
    pub fn back_to_pose(&mut self, scene: &mut Scene) {
        match &self.base {
            Some(base) => {
                for (b, rot) in self.bones.iter().zip(base) {
                    scene.nodes[b.bone].local.rot = *rot;
                }
            }
            None => self.posed(scene),
        }
    }

    /// Forgets the pose, for bones put back on their rest.
    pub fn forget_pose(&mut self) {
        self.base = None;
    }

    /// One frame turning toward `target` in pattern `ptn`, `dt` seconds
    /// after the last: `UpdateCall` then `NeckUpdateCalc`, as the
    /// controller runs them with its target (its `rate` is 1, so the
    /// target is taken as it is).
    pub fn update(&mut self, scene: &mut Scene, target: Vec3, ptn: usize, dt: f32) {
        let ptn = if ptn >= self.states.len() { 0 } else { ptn };
        self.update_call(scene, ptn);
        self.calc(scene, target, ptn, dt);
    }

    fn update_call(&mut self, scene: &mut Scene, ptn: usize) {
        let look = self.states[ptn].look_type;
        if self.look_type != look {
            self.look_type = look;
            self.change_timer = 0.0;
            for b in &mut self.bones {
                b.fix_backup = Some(b.fix);
            }
            if look == LookType::Forward {
                for b in &mut self.bones {
                    b.angle_h = 0.0;
                    b.angle_v = 0.0;
                }
            }
        }
        if self.look_type == LookType::Target {
            for b in &mut self.bones {
                b.by_target = Some(scene.nodes[b.bone].local.rot);
                scene.nodes[b.bone].local.rot = b.fix;
            }
        }
    }

    fn calc(&mut self, scene: &mut Scene, target: Vec3, ptn: usize, dt: f32) {
        // The game skips a frame without time.
        if dt == 0.0 {
            return;
        }
        let mut state = self.states[ptn].clone();
        if state.limits.len() != self.bones.len() {
            return;
        }
        self.change_timer = (self.change_timer + dt).clamp(0.0, self.change_time);
        let mut num = inverse_lerp(0.0, self.change_time, self.change_timer);
        if let Some(curve) = &self.change_curve {
            num = curve.evaluate(num);
        }
        let last = self.bones.len() - 1;
        match state.look_type {
            LookType::Animation => {
                let mut angle = self.angle_to_target(scene, target, last);
                for i in (0..self.bones.len()).rev() {
                    let (h, v) = clamp_turn(&state.limits[i], &mut angle);
                    self.bones[i].angle_h = h;
                    self.bones[i].angle_v = v;
                }
                for b in &mut self.bones {
                    b.fix = scene.nodes[b.bone].local.rot;
                    scene.nodes[b.bone].local.rot = slerp(b.fix_backup, b.fix, num);
                }
                return;
            }
            LookType::Forward | LookType::Fix => {
                for b in &mut self.bones {
                    if state.look_type == LookType::Forward {
                        b.fix = Quat::IDENTITY;
                    }
                    let local = scene.nodes[b.bone].local.rot;
                    let held = unity::quat_slerp(local, b.fix, self.calc_lerp);
                    scene.nodes[b.bone].local.rot = slerp(b.fix_backup, held, num);
                }
                return;
            }
            _ => {}
        }
        let calc = scene.world_of(self.calc);
        let (calc_pos, calc_rot) = (calc.w_axis.truncate(), rotation(&calc));
        let dir = target - calc_pos;
        let h = angle_around_axis(calc_rot * Vec3::Z, dir, calc_rot * Vec3::Y);
        let v = angle_around_axis(calc_rot * Vec3::Z, dir, calc_rot * Vec3::X);
        let correction = if state.broke { 0.0 } else { state.limit_break };
        let broke = h.abs() > state.h_limit + correction || v.abs() > state.v_limit + correction;
        state.broke = broke;
        self.states[ptn].broke = broke;
        if broke {
            self.now_angle = Vec2::ZERO;
        } else {
            let mut target = target;
            self.now_angle = self.angle_to_target(scene, target, last);
            if state.look_type == LookType::Target {
                // A target low behind the head, past the face's side: the
                // head looks at its own face's point, which keeps it still.
                let aim = scene.world_of(self.aim);
                let (scale, _, aim_pos) = aim.to_scale_rotation_translation();
                let to_local = Mat4::from_scale_rotation_translation(scale, calc_rot, aim_pos).inverse();
                let there = to_local.transform_point3(target);
                let neck = to_local.transform_point3(calc_pos);
                if there.z < 0.0 && there.y < 0.0 {
                    let past = if neck.x < 0.0 {
                        neck.x < there.x && there.x < 0.0
                    } else {
                        neck.x > there.x && there.x > 0.0
                    };
                    if past {
                        target = aim_pos;
                    }
                }
                if (aim_pos - target).length() == 0.0 {
                    for i in 0..self.bones.len() {
                        self.calc_neck_bone(scene, i);
                        let b = &self.bones[i];
                        scene.nodes[b.bone].local.rot = slerp(b.fix_backup, b.fix, num);
                    }
                    return;
                }
            } else if state.look_type == LookType::Away {
                let turned: f32 = self.bones.iter().map(|b| b.angle_h).sum();
                let max: f32 = state.limits.iter().map(|l| l.max).sum();
                let min: f32 = state.limits.iter().map(|l| l.min).sum();
                if self.now_angle.y <= turned {
                    self.now_angle.y = if self.now_angle.y <= max - state.limit_away || self.now_angle.y < 0.0 {
                        max
                    } else {
                        min
                    };
                } else if self.now_angle.y >= min + state.limit_away || self.now_angle.y > 0.0 {
                    self.now_angle.y = min;
                } else {
                    self.now_angle.y = max;
                }
                self.now_angle.x = -self.now_angle.x;
            }
        }
        let mut angle = self.now_angle;
        let t = (dt * state.leap_speed).clamp(0.0, 1.0);
        for i in (0..self.bones.len()).rev() {
            let (h, v) = clamp_turn(&state.limits[i], &mut angle);
            let b = &mut self.bones[i];
            b.angle_h += (h - b.angle_h) * t;
            b.angle_v += (v - b.angle_v) * t;
            self.calc_neck_bone(scene, i);
        }
        for b in &self.bones {
            let turned = slerp(b.by_target, b.fix, self.calc_lerp);
            scene.nodes[b.bone].local.rot = slerp(b.fix_backup, turned, num);
        }
    }

    /// `GetAngleToTarget`: how far the head would turn to face `target`,
    /// down (x) and sideways (y), in degrees.
    fn angle_to_target(&self, scene: &Scene, target: Vec3, bone: usize) -> Vec2 {
        let aim = scene.world_of(self.aim);
        let (aim_pos, aim_rot) = (aim.w_axis.truncate(), rotation(&aim));
        let calc_rot = world_rot(scene, self.calc);
        let up = calc_rot * Vec3::Y;
        let turn = unity::from_to_rotation(aim_rot * Vec3::Z, target - aim_pos);
        let faced = turn * world_rot(scene, self.bones[bone].bone);
        let sideways = angle_around_axis(calc_rot * Vec3::Z, faced * Vec3::Z, up);
        let across = unity::angle_axis(sideways, up) * calc_rot;
        let axis = unity::cross(up, across * Vec3::Z);
        let down = angle_around_axis(across * Vec3::Z, faced * Vec3::Z, axis);
        Vec2::new(down, sideways)
    }

    /// `CalcNeckBone`: the bone turned by its angles about its reference.
    fn calc_neck_bone(&mut self, scene: &mut Scene, i: usize) {
        let b = &self.bones[i];
        let reference = world_rot(scene, b.reference);
        let rot = unity::angle_axis(b.angle_h, reference * Vec3::Y)
            * unity::angle_axis(b.angle_v, reference * Vec3::X)
            * reference;
        set_world_rot(scene, b.bone, rot);
        self.bones[i].fix = scene.nodes[b.bone].local.rot;
    }
}

/// The part of the turn left in `angle` one bone takes within its limits
/// (`RotateToAngle` and `MaxRotateToAngle`): sideways and down.
fn clamp_turn(limit: &Limit, angle: &mut Vec2) -> (f32, f32) {
    let h = angle.y.clamp(limit.min, limit.max);
    let v = angle.x.clamp(limit.up, limit.down);
    *angle -= Vec2::new(v, h);
    (h, v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Trs;

    fn limits() -> Vec<Limit> {
        vec![
            Limit {
                up: -25.0,
                down: 25.0,
                min: -40.0,
                max: 40.0,
            };
            2
        ]
    }

    fn state(look_type: LookType) -> NeckTypeState {
        NeckTypeState {
            limits: limits(),
            leap_speed: 2.0,
            h_limit: 90.0,
            v_limit: 90.0,
            limit_break: 10.0,
            limit_away: 0.0,
            look_type,
            broke: false,
        }
    }

    /// A spine with the neck, the head and their references, and a point
    /// on the face, as the body's prefab has them.
    fn rig(states: Vec<NeckTypeState>) -> (Scene, NeckLook) {
        let mut scene = Scene::default();
        let at = |y: f32, z: f32| Trs {
            pos: Vec3::new(0.0, y, z),
            ..Default::default()
        };
        let spine = scene.add_node("spine", None, at(1.3, 0.0));
        let neck_ref = scene.add_node("NeckRef", Some(spine), at(0.1, 0.0));
        let neck = scene.add_node("cf_j_neck", Some(spine), at(0.1, 0.0));
        let head_ref = scene.add_node("HeadRef", Some(neck), at(0.1, 0.0));
        let head = scene.add_node("cf_j_head", Some(neck), at(0.1, 0.0));
        let aim = scene.add_node("aim", Some(head), at(0.05, 0.08));
        let curve = Curve(vec![[0.0, 0.002, 2.2, 2.2], [1.0, 1.0, 0.0, 0.0]]);
        let look = NeckLook::new(
            aim,
            neck_ref,
            &[(neck_ref, neck, Quat::IDENTITY), (head_ref, head, Quat::IDENTITY)],
            states,
            1.0,
            Some(curve),
            1.0,
        )
        .unwrap();
        (scene, look)
    }

    fn facing(scene: &Scene, node: usize) -> Vec3 {
        world_rot(scene, node) * Vec3::Z
    }

    #[test]
    fn a_built_character_turns_her_head_with_the_games_patterns() {
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
        let Ok(mut chara) = crate::build(&game, &card, &opts) else {
            return;
        };
        let mut look = chara.neck.clone().expect("the body's neck look");
        assert_eq!(look.states.len(), 7);
        let names: Vec<&str> = look.bones().map(|b| chara.scene.nodes[b].name.as_str()).collect();
        assert_eq!(names, ["cf_j_neck", "cf_j_head"]);
        assert_eq!(look.states[1].look_type, LookType::Target);
        assert_eq!(look.states[3].look_type, LookType::Animation);
        let head = look.bones[1].bone;
        let ahead = facing(&chara.scene, head);
        // A camera off to her side: "this way" turns the head toward it.
        let camera = Vec3::new(1.5, 1.4, 1.5);
        for _ in 0..180 {
            look.update(&mut chara.scene, camera, 1, 1.0 / 60.0);
        }
        let turned = facing(&chara.scene, head);
        let angle = ahead.angle_between(turned).to_degrees();
        assert!(angle > 30.0 && angle < 81.0, "{angle}");
        assert!(turned.x > ahead.x, "toward the camera");
    }

    #[test]
    fn the_curve_eases_from_its_first_key_to_its_last() {
        let curve = Curve(vec![[0.0, 0.002, 2.2, 2.2], [1.0, 1.0, 0.0, 0.0]]);
        assert_eq!(curve.evaluate(-1.0), 0.002);
        assert_eq!(curve.evaluate(2.0), 1.0);
        let half = curve.evaluate(0.5);
        assert!(half > 0.5 && half < 1.0, "{half}");
    }

    #[test]
    fn the_head_turns_toward_a_target_within_its_limits() {
        let (mut scene, mut look) = rig(vec![state(LookType::Target)]);
        let head = look.bones[1].bone;
        // Off to her left, at head height.
        let target = Vec3::new(2.0, 1.55, 2.0);
        for _ in 0..240 {
            look.update(&mut scene, target, 0, 1.0 / 60.0);
        }
        let f = facing(&scene, head);
        let sideways = f.x.atan2(f.z).to_degrees();
        assert!((sideways - 45.0).abs() < 1.0, "turned {sideways}");
        // Further to her side than the two bones can turn: 80 degrees at
        // most. (Straight to her side the game's upward angle passes its
        // limit, and the head comes back ahead.)
        let target = Vec3::new(2.0, 1.55, 0.3);
        for _ in 0..240 {
            look.update(&mut scene, target, 0, 1.0 / 60.0);
        }
        let f = facing(&scene, head);
        let sideways = f.x.atan2(f.z).to_degrees();
        assert!((sideways - 80.0).abs() < 1.0, "turned {sideways}");
    }

    #[test]
    fn behind_the_limits_the_head_comes_back_ahead() {
        let (mut scene, mut look) = rig(vec![state(LookType::Target)]);
        let head = look.bones[1].bone;
        for _ in 0..240 {
            look.update(&mut scene, Vec3::new(0.5, 1.55, -2.0), 0, 1.0 / 60.0);
        }
        let f = facing(&scene, head);
        assert!(f.x.abs() < 0.01 && f.z > 0.99, "{f}");
    }

    #[test]
    fn the_animation_pattern_keeps_the_animations_turn() {
        let (mut scene, mut look) = rig(vec![state(LookType::Animation)]);
        let neck = look.bones[0].bone;
        let posed = Quat::from_rotation_y(0.3);
        scene.nodes[neck].local.rot = posed;
        look.update(&mut scene, Vec3::new(2.0, 1.5, 2.0), 0, 1.0 / 60.0);
        assert!(scene.nodes[neck].local.rot.angle_between(posed) < 1e-4);
    }

    #[test]
    fn a_new_pattern_blends_in_over_a_second() {
        let (mut scene, mut look) = rig(vec![state(LookType::Target), state(LookType::Forward)]);
        let head = look.bones[1].bone;
        let target = Vec3::new(2.0, 1.55, 2.0);
        for _ in 0..240 {
            look.update(&mut scene, target, 0, 1.0 / 60.0);
        }
        // Half a second into Forward: on the way, not there yet.
        for _ in 0..30 {
            look.update(&mut scene, target, 1, 1.0 / 60.0);
        }
        let f = facing(&scene, head);
        let sideways = f.x.atan2(f.z).to_degrees();
        assert!(sideways > 1.0 && sideways < 44.0, "{sideways}");
        for _ in 0..60 {
            look.update(&mut scene, target, 1, 1.0 / 60.0);
        }
        let f = facing(&scene, head);
        assert!(f.x.abs() < 1e-3, "{f}");
        // Without time passing nothing moves.
        let before = scene.nodes[head].local.rot;
        look.update(&mut scene, Vec3::new(-2.0, 1.5, 2.0), 1, 0.0);
        assert_eq!(scene.nodes[head].local.rot, before);
    }
}
