// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The viewport's camera: where it looks from, what it is focused on, and
//! how drags, keys and the wheel move it. Nothing here touches the GPU or
//! the window, so all of it is tested with plain numbers.

use crate::scene::ObjectId;
use glam::Vec3;
use rk_render::Camera;

/// Radians turned per point of drag while orbiting.
const ORBIT_RATE: f32 = 0.008;
/// Radians turned per point of drag while looking around in place.
const LOOK_RATE: f32 = 0.004;
/// The range of the fly speed, as a multiple of the usual speed.
pub const SPEED_RANGE: (f32, f32) = (0.1, 10.0);

/// Where the camera looks from: angles around a point, at a distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    pub target: Vec3,
    /// Around the vertical axis, radians.
    pub yaw: f32,
    /// Above the horizon, radians.
    pub pitch: f32,
    pub distance: f32,
    pub fov_y_deg: f32,
}

impl Orbit {
    /// The orbit that sees a camera's view: the same eye, target and lens.
    pub fn from_camera(c: &Camera) -> Orbit {
        let v = c.eye - c.target;
        let distance = v.length().max(1e-3);
        Orbit {
            target: c.target,
            yaw: v.x.atan2(-v.z),
            pitch: (v.y / distance).clamp(-1.0, 1.0).asin(),
            distance,
            fov_y_deg: c.fov_y_deg,
        }
    }

    /// The unit vector from the target to the eye.
    fn out(&self) -> Vec3 {
        Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            -self.yaw.cos() * self.pitch.cos(),
        )
    }

    pub fn camera(&self) -> Camera {
        Camera {
            eye: self.target + self.out() * self.distance,
            target: self.target,
            fov_y_deg: self.fov_y_deg,
        }
    }

    /// The view's right, up and forward directions.
    fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let forward = -self.out();
        let right = Vec3::Y.cross(forward).normalize_or_zero();
        (right, forward.cross(right), forward)
    }

    fn turn(&mut self, drag: egui::Vec2, rate: f32) {
        self.yaw -= drag.x * rate;
        self.pitch = (self.pitch + drag.y * rate).clamp(-1.5, 1.5);
    }

    /// Turns around the target, by a drag in points: dragging right turns
    /// the view right and dragging down tilts it down, as in Unity.
    pub fn rotate(&mut self, drag: egui::Vec2) {
        self.turn(drag, ORBIT_RATE);
    }

    /// Turns around `pivot`, which need not be the target: the pivot keeps
    /// its distance from the eye and its place in the picture, so a view
    /// that was slid to one side stays slid while it goes around.
    pub fn rotate_around(&mut self, pivot: Vec3, drag: egui::Vec2) {
        let (right, up, forward) = self.basis();
        let offset = self.target - pivot;
        let local = Vec3::new(offset.dot(right), offset.dot(up), offset.dot(forward));
        self.turn(drag, ORBIT_RATE);
        let (right, up, forward) = self.basis();
        self.target = pivot + right * local.x + up * local.y + forward * local.z;
    }

    /// Turns the camera where it stands, by a drag in points: the eye stays
    /// and the target moves.
    pub fn look(&mut self, drag: egui::Vec2) {
        let eye = self.camera().eye;
        self.turn(drag, LOOK_RATE);
        self.target = eye - self.out() * self.distance;
    }

    /// Flies the camera and its target together. `along` is right, up and
    /// forward, each -1 to 1; `speed` is in distances to the target per
    /// second, so it suits a close-up and a whole room alike.
    pub fn fly(&mut self, along: Vec3, dt: f32, speed: f32) {
        let (right, _, forward) = self.basis();
        let step = self.distance.max(0.5) * speed * dt;
        self.target += (right * along.x + Vec3::Y * along.y + forward * along.z) * step;
    }

    /// Moves the target across the view, by a drag in points.
    pub fn pan(&mut self, drag: egui::Vec2, view_height: f32) {
        let (right, up, _) = self.basis();
        // The distance a point covers on screen at the target's depth.
        let scale = 2.0 * self.distance * (self.fov_y_deg.to_radians() / 2.0).tan() / view_height.max(1.0);
        self.target += (-right * drag.x + up * drag.y) * scale;
    }

    /// Moves closer for a positive scroll, farther for a negative one.
    pub fn zoom(&mut self, scroll: f32) {
        self.distance = (self.distance * (-scroll * 0.002).exp()).clamp(0.05, 50.0);
    }

    /// The view part of the way from this one to `to`, `t` from 0 to 1: the
    /// target, the distance and the angles each go straight to theirs, the
    /// turn around the vertical axis by its shorter side, so the eye swings
    /// around on an arc rather than cutting through what it looks at.
    pub fn toward(&self, to: &Orbit, t: f32) -> Orbit {
        use std::f32::consts::{PI, TAU};
        let t = t.clamp(0.0, 1.0);
        let turn = (to.yaw - self.yaw + PI).rem_euclid(TAU) - PI;
        Orbit {
            target: self.target.lerp(to.target, t),
            yaw: self.yaw + turn * t,
            pitch: self.pitch + (to.pitch - self.pitch) * t,
            distance: self.distance + (to.distance - self.distance) * t,
            fov_y_deg: self.fov_y_deg + (to.fov_y_deg - self.fov_y_deg) * t,
        }
    }

    /// Whether this view differs from `start` enough to be worth a way
    /// back: the eye or the target went farther than a tenth of the
    /// starting distance, the view turned by more than five degrees, or the
    /// distance changed by more than a tenth.
    pub fn moved_from(&self, start: &Orbit) -> bool {
        let reach = start.distance * 0.1;
        let (a, b) = (self.camera(), start.camera());
        let turned = self.out().dot(start.out()).clamp(-1.0, 1.0).acos();
        let ratio = self.distance / start.distance;
        (a.eye - b.eye).length() > reach
            || (a.target - b.target).length() > reach
            || turned > 5.0f32.to_radians()
            || !(1.0 / 1.1..=1.1).contains(&ratio)
    }
}

/// What the camera is focused on: an object of the scene and where it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Focus {
    pub object: ObjectId,
    pub center: Vec3,
    /// The distance from which the whole object fits the view.
    pub distance: f32,
}

/// What holding the right button does at the moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightDrag {
    /// Going around the focused object.
    Orbit,
    /// Looking around where the camera stands, and flying.
    Explore,
}

/// How long the camera takes to ease to a view it was sent to, in seconds.
pub const GLIDE: f32 = 0.3;

/// A move of the camera to another view that is under way.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Glide {
    from: Orbit,
    to: Orbit,
    /// Seconds since it began.
    elapsed: f32,
}

/// Fast at first and slowing into the end, `x` from 0 to 1.
fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

/// The camera of a viewport with its focus and its fly speed.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewCamera {
    pub orbit: Orbit,
    /// The move to another view that is under way.
    glide: Option<Glide>,
    /// How long such a move takes, in seconds; 0 jumps.
    pub glide_time: f32,
    /// The framing and the focus the viewport started with.
    home: (Orbit, Option<Focus>),
    /// The object the camera goes around, or nothing while it is free.
    pub focus: Option<Focus>,
    /// The fly speed as a multiple of the usual one.
    pub speed: f32,
    /// A movement key was pressed during this hold of the right button.
    exploring: bool,
}

impl ViewCamera {
    pub fn new(orbit: Orbit, focus: Option<Focus>) -> ViewCamera {
        ViewCamera {
            orbit,
            glide: None,
            glide_time: GLIDE,
            home: (orbit, focus),
            focus,
            speed: 1.0,
            exploring: false,
        }
    }

    /// Sends the camera to another view: it eases there over the glide
    /// time as `advance` is called, or is there at once when that is 0.
    pub fn glide_to(&mut self, to: Orbit) {
        if self.glide_time > 0.0 && to != self.orbit {
            self.glide = Some(Glide {
                from: self.orbit,
                to,
                elapsed: 0.0,
            });
        } else {
            self.glide = None;
            self.orbit = to;
        }
    }

    /// Moves a glide on by `dt` seconds. Returns whether the camera is
    /// still on its way, and so needs another frame.
    pub fn advance(&mut self, dt: f32) -> bool {
        let Some(glide) = &mut self.glide else { return false };
        glide.elapsed += dt.max(0.0);
        let t = glide.elapsed / self.glide_time.max(1e-6);
        if t >= 1.0 {
            self.orbit = glide.to;
            self.glide = None;
            return false;
        }
        self.orbit = glide.from.toward(&glide.to, ease_out(t));
        true
    }

    /// Stops a glide where it is: the user took the camera.
    pub fn cancel_glide(&mut self) {
        self.glide = None;
    }

    /// Whether the camera is on its way to another view.
    #[cfg(test)]
    pub fn gliding(&self) -> bool {
        self.glide.is_some()
    }

    /// The view the camera is at, or on its way to.
    pub fn destination(&self) -> Orbit {
        self.glide.map_or(self.orbit, |g| g.to)
    }

    /// Focuses on an object and frames it, keeping the direction of view.
    pub fn focus_on(&mut self, focus: Focus) {
        self.focus = Some(focus);
        let mut to = self.destination();
        to.target = focus.center;
        to.distance = focus.distance;
        self.glide_to(to);
    }

    /// Looks at a point from the distance and the direction the view has,
    /// without focusing on anything.
    pub fn look_at(&mut self, point: Vec3) {
        let mut to = self.destination();
        to.target = point;
        self.glide_to(to);
    }

    /// Makes a framing the one the viewport starts from and goes back to,
    /// and goes to it.
    pub fn rehome(&mut self, orbit: Orbit, focus: Option<Focus>) {
        self.home = (orbit, focus);
        self.focus = focus;
        self.glide_to(orbit);
    }

    /// The object `now` names moved or changed size: a view focused on it
    /// goes with it, and so does the starting framing, so that moving an
    /// object does not count as moving the camera.
    pub fn follow(&mut self, now: Focus) {
        let carry = |orbit: &mut Orbit, focus: &mut Option<Focus>| {
            if let Some(was) = focus.filter(|f| f.object == now.object && *f != now) {
                orbit.target += now.center - was.center;
                if was.distance > 0.0 {
                    orbit.distance *= now.distance / was.distance;
                }
                *focus = Some(now);
            }
        };
        // A glide under way ends where the object went.
        if let Some(glide) = &mut self.glide {
            let mut focus = self.focus;
            carry(&mut glide.to, &mut focus);
        }
        carry(&mut self.orbit, &mut self.focus);
        carry(&mut self.home.0, &mut self.home.1);
    }

    /// What the starting framing is focused on.
    pub fn home_focus(&self) -> Option<Focus> {
        self.home.1
    }

    /// An object left the scene: nothing stays focused on it.
    pub fn forget(&mut self, object: ObjectId) {
        for focus in [&mut self.focus, &mut self.home.1] {
            if focus.is_some_and(|f| f.object == object) {
                *focus = None;
            }
        }
    }

    /// Lets go of the focused object; the view stays as it is.
    pub fn release(&mut self) {
        self.focus = None;
    }

    /// Back to the framing and the focus the viewport started with.
    pub fn reset(&mut self) {
        self.focus = self.home.1;
        self.glide_to(self.home.0);
    }

    /// Whether the view left its starting framing far enough to offer the
    /// way back.
    pub fn moved(&self) -> bool {
        self.orbit.moved_from(&self.home.0)
    }

    /// What the right button does now: it goes around the focused object
    /// until a movement key is pressed, and explores when nothing is
    /// focused.
    pub fn right_drag(&self) -> RightDrag {
        if self.exploring || self.focus.is_none() {
            RightDrag::Explore
        } else {
            RightDrag::Orbit
        }
    }

    /// One frame of holding the right button: `drag` in points, the
    /// movement keys as `along` (right, up and forward, each -1 to 1), and
    /// `fast` for Shift. A movement key switches to exploring, which lets
    /// go of the focus and lasts until the button is released.
    pub fn right_held(&mut self, drag: egui::Vec2, along: Vec3, dt: f32, fast: bool) -> RightDrag {
        self.cancel_glide();
        if along != Vec3::ZERO {
            self.exploring = true;
            self.focus = None;
        }
        match (self.right_drag(), self.focus) {
            (RightDrag::Orbit, Some(focus)) => {
                self.orbit.rotate_around(focus.center, drag);
                RightDrag::Orbit
            }
            _ => {
                self.orbit.look(drag);
                let speed = self.speed * if fast { 3.0 } else { 1.0 };
                self.orbit.fly(along, dt, speed);
                RightDrag::Explore
            }
        }
    }

    /// The right button was let go: the next hold starts over.
    pub fn right_released(&mut self) {
        self.exploring = false;
    }

    /// Alt with the left button: around the focused object, or around the
    /// middle of the view while the camera is free.
    pub fn orbit_drag(&mut self, drag: egui::Vec2) {
        self.cancel_glide();
        match self.focus {
            Some(focus) => self.orbit.rotate_around(focus.center, drag),
            None => self.orbit.rotate(drag),
        }
    }

    /// Changes the fly speed by a wheel scroll in points, up for faster.
    /// Returns whether the speed changed.
    pub fn scroll_speed(&mut self, scroll: f32) -> bool {
        let next = (self.speed * (scroll * 0.004).exp()).clamp(SPEED_RANGE.0, SPEED_RANGE.1);
        let changed = next != self.speed;
        self.speed = next;
        changed
    }

    /// Where the speed lies in its range, 0 to 1, on a scale on which each
    /// doubling takes the same room; the usual speed is at the middle.
    pub fn speed_fraction(&self) -> f32 {
        let (lo, hi) = SPEED_RANGE;
        ((self.speed / lo).ln() / (hi / lo).ln()).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Vec2;

    fn orbit() -> Orbit {
        Orbit {
            target: Vec3::new(0.0, 1.0, 0.0),
            yaw: 0.3,
            pitch: 0.2,
            distance: 2.0,
            fov_y_deg: 30.0,
        }
    }

    fn focused() -> ViewCamera {
        let focus = Focus {
            object: ObjectId(2),
            center: Vec3::new(0.0, 1.0, 0.0),
            distance: 2.0,
        };
        let mut cam = ViewCamera::new(orbit(), Some(focus));
        // The tests of where the camera goes do not wait for it.
        cam.glide_time = 0.0;
        cam
    }

    #[test]
    fn the_camera_eases_to_where_it_is_sent_and_input_stops_it() {
        let mut cam = focused();
        cam.glide_time = GLIDE;
        let start = cam.orbit;
        let focus = Focus {
            object: ObjectId(7),
            center: Vec3::new(2.0, 1.0, 0.0),
            distance: 4.0,
        };
        cam.focus_on(focus);
        // Nothing moved yet; the focus is taken at once.
        assert_eq!(cam.orbit, start);
        assert_eq!(cam.focus, Some(focus));
        assert!(cam.gliding());
        assert!(cam.advance(GLIDE / 2.0));
        // Easing out: more than half the way after half the time.
        let x = cam.orbit.target.x;
        assert!(x > 1.0 && x < 2.0, "{x}");
        assert!(cam.orbit.distance > 3.0 && cam.orbit.distance < 4.0);
        assert!(!cam.advance(GLIDE));
        // It ends exactly where it was sent, keeping the direction of view.
        assert_eq!(cam.orbit.target, focus.center);
        assert_eq!(cam.orbit.distance, 4.0);
        assert_eq!((cam.orbit.yaw, cam.orbit.pitch), (start.yaw, start.pitch));
        assert!(!cam.gliding() && !cam.advance(1.0));

        // The way back is eased too, and taking the camera stops it there.
        cam.orbit.rotate(Vec2::new(200.0, 0.0));
        cam.reset();
        assert!(cam.advance(GLIDE / 3.0));
        let held = cam.orbit;
        cam.orbit_drag(Vec2::ZERO);
        assert!(!cam.gliding() && !cam.advance(1.0));
        assert!((cam.orbit.target - held.target).length() < 1e-5 && cam.orbit.yaw == held.yaw);
        assert!(held != start);
        cam.reset();
        cam.right_held(Vec2::ZERO, Vec3::ZERO, 0.0, false);
        assert!(!cam.gliding());

        // With no time to take, the camera is there at once.
        cam.glide_time = 0.0;
        cam.reset();
        assert_eq!(cam.orbit, start);
        assert!(!cam.gliding());
    }

    #[test]
    fn a_turn_takes_the_shorter_way_around() {
        let from = Orbit { yaw: 3.0, ..orbit() };
        let to = Orbit { yaw: -3.0, ..orbit() };
        let half = from.toward(&to, 0.5);
        // From 3 to -3 radians is a short step across the back, not a
        // turn of almost a whole circle through the front.
        assert!((half.yaw - std::f32::consts::PI).abs() < 0.01, "{}", half.yaw);
        assert_eq!(from.toward(&to, 0.0), from);
        let end = from.toward(&to, 1.0);
        assert!((end.camera().eye - to.camera().eye).length() < 1e-4);
        // A glide carries on to where a focused object moved.
        let mut cam = focused();
        cam.glide_time = GLIDE;
        let mut focus = cam.focus.unwrap();
        cam.orbit.pan(Vec2::new(50.0, 0.0), 500.0);
        cam.focus_on(focus);
        focus.center += Vec3::X;
        cam.follow(focus);
        cam.advance(GLIDE);
        assert_eq!(cam.orbit.target, focus.center);
    }

    #[test]
    fn an_orbit_keeps_the_camera_it_was_made_from() {
        let c = Camera {
            eye: Vec3::new(0.3, 1.4, -2.5),
            target: Vec3::new(0.0, 0.9, 0.0),
            fov_y_deg: 30.0,
        };
        let back = Orbit::from_camera(&c).camera();
        assert!((back.eye - c.eye).length() < 1e-4, "{:?}", back.eye);
        assert_eq!(back.target, c.target);
    }

    #[test]
    fn zoom_and_pitch_stay_in_range() {
        let mut o = orbit();
        o.zoom(1.0e6);
        assert!(o.distance >= 0.05);
        o.rotate(Vec2::new(0.0, -1.0e4));
        assert!(o.pitch >= -1.5);
        o.rotate(Vec2::new(0.0, 1.0e4));
        assert!(o.pitch <= 1.5);
    }

    #[test]
    fn looking_keeps_the_eye_and_flying_moves_both() {
        let mut o = orbit();
        let eye = o.camera().eye;
        o.look(Vec2::new(40.0, -25.0));
        assert!((o.camera().eye - eye).length() < 1e-4);
        let (eye, target) = (o.camera().eye, o.target);
        o.fly(Vec3::new(0.0, 0.0, 1.0), 0.5, 1.0);
        let moved = o.target - target;
        assert!(moved.length() > 0.1);
        assert!(((o.camera().eye - eye) - moved).length() < 1e-4);
    }

    #[test]
    fn the_right_button_goes_around_the_focused_object() {
        let mut cam = focused();
        let eye = cam.orbit.camera().eye;
        let mode = cam.right_held(Vec2::new(60.0, 20.0), Vec3::ZERO, 0.016, false);
        assert_eq!(mode, RightDrag::Orbit);
        // The target stays on the object and the eye goes around it.
        assert_eq!(cam.orbit.target, Vec3::new(0.0, 1.0, 0.0));
        let now = cam.orbit.camera().eye;
        assert!((now - eye).length() > 0.5);
        assert!(((now - cam.orbit.target).length() - 2.0).abs() < 1e-4);
        assert!(cam.focus.is_some());
    }

    #[test]
    fn a_movement_key_switches_to_exploring_until_release() {
        let mut cam = focused();
        cam.right_held(Vec2::new(10.0, 0.0), Vec3::ZERO, 0.016, false);
        let mode = cam.right_held(Vec2::ZERO, Vec3::new(0.0, 0.0, 1.0), 0.016, false);
        assert_eq!(mode, RightDrag::Explore);
        assert!(cam.focus.is_none());
        // With the key up again the hold keeps exploring: the eye stays put.
        let eye = cam.orbit.camera().eye;
        let mode = cam.right_held(Vec2::new(30.0, 10.0), Vec3::ZERO, 0.016, false);
        assert_eq!(mode, RightDrag::Explore);
        assert!((cam.orbit.camera().eye - eye).length() < 1e-4);
        // Nothing is focused after the release, so the next hold explores.
        cam.right_released();
        assert_eq!(cam.right_drag(), RightDrag::Explore);
        // Focusing again brings the orbit back.
        cam.focus_on(Focus {
            object: ObjectId(2),
            center: Vec3::new(0.0, 1.0, 0.0),
            distance: 2.0,
        });
        assert_eq!(cam.right_drag(), RightDrag::Orbit);
        assert_eq!(cam.orbit.target, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn an_orbit_around_a_slid_view_keeps_the_object_in_place() {
        let mut cam = focused();
        cam.orbit.pan(Vec2::new(80.0, 0.0), 600.0);
        let pivot = Vec3::new(0.0, 1.0, 0.0);
        let before = cam.orbit;
        let (right, up, forward) = before.basis();
        let seen = pivot - before.camera().eye;
        cam.orbit_drag(Vec2::new(90.0, -30.0));
        let (r2, u2, f2) = cam.orbit.basis();
        let now = pivot - cam.orbit.camera().eye;
        assert!((seen.dot(right) - now.dot(r2)).abs() < 1e-4);
        assert!((seen.dot(up) - now.dot(u2)).abs() < 1e-4);
        assert!((seen.dot(forward) - now.dot(f2)).abs() < 1e-4);
        assert!(cam.focus.is_some());
    }

    #[test]
    fn only_a_significant_change_counts_as_moved() {
        let mut cam = focused();
        assert!(!cam.moved());
        // Two degrees, a slight zoom and a small slide are not worth it.
        cam.orbit.rotate(Vec2::new(2.0f32.to_radians() / ORBIT_RATE, 0.0));
        cam.orbit.distance *= 1.05;
        assert!(!cam.moved());
        let small = cam.clone();

        cam.orbit.rotate(Vec2::new(8.0f32.to_radians() / ORBIT_RATE, 0.0));
        assert!(cam.moved(), "turned");
        cam = small.clone();
        cam.orbit.distance *= 1.2;
        assert!(cam.moved(), "zoomed out");
        cam = small.clone();
        cam.orbit.distance /= 1.3;
        assert!(cam.moved(), "zoomed in");
        cam = small.clone();
        cam.orbit.target += Vec3::new(0.3, 0.0, 0.0);
        assert!(cam.moved(), "slid");
        cam = small;
        cam.orbit.look(Vec2::new(0.0, 12.0f32.to_radians() / LOOK_RATE));
        assert!(cam.moved(), "looked away");

        cam.release();
        cam.reset();
        assert!(!cam.moved());
        assert_eq!(cam.focus.map(|f| f.object), Some(ObjectId(2)));
    }

    #[test]
    fn the_view_goes_with_the_focused_object() {
        let mut cam = focused();
        let start = cam.orbit;
        let moved = Focus {
            object: ObjectId(2),
            center: Vec3::new(1.5, 1.0, 0.0),
            distance: 4.0,
        };
        cam.follow(moved);
        assert_eq!(cam.focus, Some(moved));
        assert!((cam.orbit.target - (start.target + Vec3::new(1.5, 0.0, 0.0))).length() < 1e-5);
        assert!((cam.orbit.distance - start.distance * 2.0).abs() < 1e-5);
        // The object moved, not the camera: there is nothing to reset.
        assert!(!cam.moved());

        // Another object moving changes nothing.
        let other = Focus {
            object: ObjectId(7),
            ..moved
        };
        let before = cam.clone();
        cam.follow(other);
        assert_eq!(cam, before);

        // A free camera stays where it is, but its way back follows.
        cam.release();
        let free = cam.orbit;
        cam.follow(Focus {
            center: Vec3::ZERO,
            ..moved
        });
        assert_eq!(cam.orbit, free);
        cam.reset();
        assert_eq!(cam.focus.map(|f| f.center), Some(Vec3::ZERO));

        cam.forget(ObjectId(2));
        assert_eq!(cam.focus, None);
        cam.reset();
        assert_eq!(cam.focus, None);
    }

    #[test]
    fn the_wheel_changes_the_fly_speed_within_its_range() {
        let mut cam = focused();
        assert!((cam.speed_fraction() - 0.5).abs() < 1e-4);
        assert!(cam.scroll_speed(50.0));
        assert!(cam.speed > 1.0);
        assert!(cam.scroll_speed(-100.0));
        assert!(cam.speed < 1.0);
        assert!(!cam.scroll_speed(0.0));
        cam.scroll_speed(1.0e5);
        assert_eq!(cam.speed, SPEED_RANGE.1);
        assert!(!cam.scroll_speed(50.0));
        cam.scroll_speed(-1.0e5);
        assert_eq!(cam.speed, SPEED_RANGE.0);
        assert_eq!(cam.speed_fraction(), 0.0);

        // A faster speed covers more ground in the same time.
        let mut slow = focused();
        let mut quick = focused();
        quick.scroll_speed(200.0);
        let start = slow.orbit.target;
        slow.right_held(Vec2::ZERO, Vec3::Z, 0.1, false);
        quick.right_held(Vec2::ZERO, Vec3::Z, 0.1, false);
        assert!((quick.orbit.target - start).length() > (slow.orbit.target - start).length() * 1.5);
    }
}
