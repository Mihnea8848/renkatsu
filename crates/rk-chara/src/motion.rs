// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Playing an animation on a character's skeleton, as CharaStudio does:
//! the body's Animator gets a controller, one of its states is played from
//! a point of its length, and the state's clips write the local position,
//! rotation and scale of the bones they name.
//!
//! A clip names a bone by the CRC-32 of its path below the Animator's
//! object, so a skeleton is first turned into a table of such hashes. A
//! state may blend several clips by a parameter: the game's controllers
//! do so by the character's height (`height`, the first body slider) and
//! in places by the bust (`Breast`). Bones a clip does not name keep what
//! they have.
//!
//! Not followed: transitions between states (CharaStudio plays the state
//! it is told and stays there), layers beyond the first, curves that are
//! not a transform's, and inverse kinematics.

use crate::scene::Scene;
use anyhow::{Context, Result};
use glam::{Quat, Vec3};
use rk_unityfs::animation::{crc32, AnimationClip, AnimatorController, Target};
use rk_unityfs::class;
use rk_vfs::assets::ObjRef;
use rk_vfs::Game;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// The bones below an Animator's object, by the hash of their path.
#[derive(Debug, Clone, Default)]
pub struct Skeleton {
    by_hash: HashMap<u32, usize>,
}

impl Skeleton {
    /// Every node below `root`, under the CRC-32 of its path from there.
    pub fn of(scene: &Scene, root: usize) -> Skeleton {
        let mut by_hash = HashMap::new();
        let mut stack: Vec<(usize, String)> = scene.nodes[root]
            .children
            .iter()
            .map(|&c| (c, scene.nodes[c].name.clone()))
            .collect();
        while let Some((node, path)) = stack.pop() {
            for &child in &scene.nodes[node].children {
                stack.push((child, format!("{path}/{}", scene.nodes[child].name)));
            }
            by_hash.entry(crc32(path.as_bytes())).or_insert(node);
        }
        Skeleton { by_hash }
    }

    pub fn node(&self, path_hash: u32) -> Option<usize> {
        self.by_hash.get(&path_hash).copied()
    }

    pub fn len(&self) -> usize {
        self.by_hash.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }
}

/// A state of a controller, ready to play: its clips with the share each
/// has in the blend.
#[derive(Debug, Clone, Default)]
pub struct Motion {
    pub clips: Vec<(Arc<AnimationClip>, f32)>,
    /// The speed the state itself plays at.
    pub speed: f32,
    /// How many transitions lead out of the state, which are not followed.
    pub transitions: usize,
}

impl Motion {
    /// A motion that is one clip.
    pub fn single(clip: AnimationClip) -> Motion {
        Motion {
            clips: vec![(Arc::new(clip), 1.0)],
            speed: 1.0,
            transitions: 0,
        }
    }

    /// The length of one pass in seconds: the clips' lengths by their
    /// shares, as a blend tree's.
    pub fn duration(&self) -> f32 {
        let total: f32 = self.clips.iter().map(|(_, w)| w).sum();
        if total <= 0.0 {
            return 0.0;
        }
        self.clips.iter().map(|(c, w)| c.duration() * w).sum::<f32>() / total
    }

    /// Whether the motion starts over at its end.
    pub fn loops(&self) -> bool {
        self.clips.iter().any(|(c, _)| c.loops)
    }

    /// The largest change of any curve over the motion; zero for a motion
    /// that holds one pose.
    pub fn movement(&self) -> f32 {
        self.clips.iter().map(|(c, _)| c.movement()).fold(0.0, f32::max)
    }

    /// Where the motion is after `seconds` of play at `speed`, starting at
    /// `start`, in passes (`AnimatorStateInfo.normalizedTime`). A motion
    /// without length stays at its start.
    pub fn normalized(&self, start: f32, seconds: f32, speed: f32) -> f32 {
        let duration = self.duration();
        if duration <= 0.0 {
            return start;
        }
        start + seconds * speed * self.speed / duration
    }

    /// The point of a pass that is played at a normalized time: a looping
    /// motion (or any motion with `force_loop`, CharaStudio's option)
    /// starts over, another holds its last frame.
    pub fn phase(&self, normalized: f32, force_loop: bool) -> f32 {
        if self.loops() || force_loop {
            normalized.rem_euclid(1.0)
        } else {
            normalized.clamp(0.0, 1.0)
        }
    }

    /// Binds the motion to a skeleton.
    pub fn bind(&self, skeleton: &Skeleton) -> Bound {
        let mut unbound = 0;
        let clips = self
            .clips
            .iter()
            .map(|(clip, weight)| {
                let mut tracks = Vec::new();
                let mut curve = 0;
                for binding in &clip.bindings {
                    match (skeleton.node(binding.path), binding.target) {
                        (_, Target::Other) => {}
                        (Some(node), target) => tracks.push(Track { node, target, curve }),
                        (None, _) => unbound += 1,
                    }
                    curve += binding.width();
                }
                BoundClip {
                    clip: clip.clone(),
                    weight: *weight,
                    tracks,
                }
            })
            .collect();
        Bound { clips, unbound }
    }
}

#[derive(Debug, Clone, Copy)]
struct Track {
    node: usize,
    target: Target,
    /// The first of the track's curves in the clip.
    curve: usize,
}

#[derive(Debug, Clone)]
struct BoundClip {
    clip: Arc<AnimationClip>,
    weight: f32,
    tracks: Vec<Track>,
}

/// A motion bound to one character's bones.
#[derive(Debug, Clone, Default)]
pub struct Bound {
    clips: Vec<BoundClip>,
    /// Curves whose bone the skeleton does not have.
    pub unbound: usize,
}

impl Bound {
    /// How many bone properties the motion writes.
    pub fn tracks(&self) -> usize {
        self.clips.iter().map(|c| c.tracks.len()).max().unwrap_or(0)
    }

    /// The nodes the motion writes.
    pub fn nodes(&self) -> Vec<usize> {
        let mut nodes: Vec<usize> = self
            .clips
            .iter()
            .flat_map(|c| c.tracks.iter().map(|t| t.node))
            .collect();
        nodes.sort_unstable();
        nodes.dedup();
        nodes
    }

    /// Writes the pose at `phase` (0 the first frame, 1 the last) to the
    /// bones. Several clips are blended by their shares: positions and
    /// scales as sums, rotations as a sum of quaternions turned to the
    /// same side and made unit length again.
    pub fn apply(&self, scene: &mut Scene, phase: f32) {
        let total: f32 = self.clips.iter().map(|c| c.weight).sum();
        if total <= 0.0 {
            return;
        }
        let mut values = Vec::new();
        if let [only] = self.clips.as_slice() {
            only.clip
                .sample(only.clip.start + phase * only.clip.duration(), &mut values);
            for track in &only.tracks {
                let Some(v) = values.get(track.curve..track.curve + width(track.target)) else {
                    continue;
                };
                let local = &mut scene.nodes[track.node].local;
                match track.target {
                    Target::Position => local.pos = Vec3::new(v[0], v[1], v[2]),
                    Target::Scale => local.scale = Vec3::new(v[0], v[1], v[2]),
                    Target::Rotation => local.rot = Quat::from_xyzw(v[0], v[1], v[2], v[3]).normalize(),
                    Target::Euler => local.rot = crate::scene::euler_deg(v[0], v[1], v[2]),
                    Target::Other => {}
                }
            }
            return;
        }
        // Sums by node and property.
        let mut sums: HashMap<(usize, u8), ([f32; 4], f32)> = HashMap::new();
        for bound in &self.clips {
            let clip = &bound.clip;
            clip.sample(clip.start + phase * clip.duration(), &mut values);
            let share = bound.weight / total;
            for track in &bound.tracks {
                let Some(v) = values.get(track.curve..track.curve + width(track.target)) else {
                    continue;
                };
                let (kind, mut value) = match track.target {
                    Target::Position => (0, [v[0], v[1], v[2], 0.0]),
                    Target::Scale => (1, [v[0], v[1], v[2], 0.0]),
                    Target::Rotation => (2, [v[0], v[1], v[2], v[3]]),
                    Target::Euler => (2, crate::scene::euler_deg(v[0], v[1], v[2]).to_array()),
                    Target::Other => continue,
                };
                let (sum, weight) = sums.entry((track.node, kind)).or_insert(([0.0; 4], 0.0));
                if kind == 2 && sum.iter().zip(&value).map(|(a, b)| a * b).sum::<f32>() < 0.0 {
                    value = value.map(|c| -c);
                }
                for (s, c) in sum.iter_mut().zip(value) {
                    *s += c * share;
                }
                *weight += share;
            }
        }
        for ((node, kind), (sum, weight)) in sums {
            let local = &mut scene.nodes[node].local;
            // A bone only some of the clips name keeps the rest of itself.
            let rest = (1.0 - weight).max(0.0);
            match kind {
                0 => local.pos = Vec3::new(sum[0], sum[1], sum[2]) + local.pos * rest,
                1 => local.scale = Vec3::new(sum[0], sum[1], sum[2]) + local.scale * rest,
                _ => {
                    let mut mine = local.rot.to_array();
                    if mine.iter().zip(&sum).map(|(a, b)| a * b).sum::<f32>() < 0.0 {
                        mine = mine.map(|c| -c);
                    }
                    let q = Quat::from_xyzw(
                        sum[0] + mine[0] * rest,
                        sum[1] + mine[1] * rest,
                        sum[2] + mine[2] * rest,
                        sum[3] + mine[3] * rest,
                    );
                    if q.length_squared() > 0.0 {
                        local.rot = q.normalize();
                    }
                }
            }
        }
    }
}

fn width(target: Target) -> usize {
    match target {
        Target::Rotation => 4,
        Target::Other => 1,
        _ => 3,
    }
}

/// The values a controller's parameters take for a character, by name:
/// CharaStudio sets `height` to the first body slider and `Breast` to the
/// bust slider. A parameter not named here keeps the controller's default.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Parameters {
    pub height: f32,
    pub breast: f32,
    /// The two values CharaStudio's animation options hold for an H
    /// animation (`Studio.OICharInfo.animeOptionParam`), which take the
    /// place of `height` and `Breast`; none plays at the body's own.
    pub options: Option<[f32; 2]>,
}

/// The parameters the H scenes' two animation options drive, in order,
/// as `Studio.OCIChar.LoadAnime` picks them: `height1` and `Breast1`
/// where the controller declares them, else `height` and `Breast`. The
/// game's own H controllers declare neither, so the options stand in for
/// the body there.
pub const OPTION_PARAMETERS: [[&str; 2]; 2] = [["height1", "height"], ["Breast1", "Breast"]];

impl Parameters {
    /// From a card's body sliders (`ChaFileBody.shapeValueBody`).
    pub fn of(shape_value_body: &[f32]) -> Parameters {
        Parameters {
            height: shape_value_body.first().copied().unwrap_or(0.5),
            breast: shape_value_body.get(4).copied().unwrap_or(0.5),
            options: None,
        }
    }

    /// The same parameters with the H animation options set or cleared.
    pub fn with_options(self, options: Option<[f32; 2]>) -> Parameters {
        Parameters { options, ..self }
    }

    /// Which parameter each option drives, for a controller that
    /// declares the names `has` says it does.
    fn option_names(has: &dyn Fn(&str) -> bool) -> [&'static str; 2] {
        OPTION_PARAMETERS.map(|[first, second]| if has(first) { first } else { second })
    }

    fn get(&self, name: &str, options: &[&str; 2]) -> Option<f32> {
        if let Some(values) = self.options {
            if let Some(at) = options.iter().position(|driven| *driven == name) {
                return Some(values[at]);
            }
        }
        match name {
            "height" => Some(self.height),
            "Breast" => Some(self.breast),
            _ => None,
        }
    }
}

/// The clips a state plays for the given parameters, out of a controller
/// whose clips `clip` reads by their index.
pub fn resolve(
    controller: &AnimatorController,
    state: &str,
    parameters: &Parameters,
    clip: &mut dyn FnMut(usize) -> Result<Arc<AnimationClip>>,
) -> Result<Motion> {
    let found = controller
        .state(state)
        .with_context(|| format!("controller {} has no state {state}", controller.name))?;
    let has = |name: &str| controller.parameters.iter().any(|p| p.name == name);
    let options = Parameters::option_names(&has);
    let value = |id: u32| {
        controller
            .parameters
            .iter()
            .find(|p| p.id == id)
            .map_or(0.0, |p| parameters.get(&p.name, &options).unwrap_or(p.default))
    };
    let mut clips = Vec::new();
    for (index, weight) in found.weights(&value) {
        clips.push((clip(index)?, weight));
    }
    Ok(Motion {
        clips,
        speed: found.speed,
        transitions: found.transitions,
    })
}

/// Where a second controller's clips are, for an animation whose list
/// names one: the bundle and the controller asset in it. The H scenes'
/// animations share one base controller for a whole category and differ
/// only by this, so an animation that names one and plays without it
/// plays its neighbour's motion.
pub type Override<'a> = Option<(&'a str, &'a str)>;

/// The clips of `over` by their name, which replace the base
/// controller's clips of the same name. This is what the game's
/// `Illusion.Utils.Animator.SetupAnimatorOverrideController` does: it
/// builds an override controller over the base and assigns every clip of
/// the second controller by name.
pub(crate) fn replacements(game: &Game, over: Override) -> Result<BTreeMap<String, ObjRef>> {
    let mut out = BTreeMap::new();
    let Some((bundle, controller)) = over.filter(|(b, c)| !b.is_empty() && !c.is_empty()) else {
        return Ok(out);
    };
    let object = game
        .assets
        .find(bundle, controller, Some(class::ANIMATOR_CONTROLLER), None)
        .with_context(|| format!("no animator controller {controller} in {bundle}"))?;
    let read = AnimatorController::read(&object.read()?)?;
    for pointer in &read.clips {
        if let Some(clip) = game.assets.deref(&object.file, *pointer) {
            out.insert(clip.name(), clip);
        }
    }
    Ok(out)
}

/// Loads a state of a controller from the game's files, as
/// `ChaControl.LoadAnimation` and `Animator.Play` find it: the controller
/// asset `controller` of `bundle`, and its state `state`. With `over`,
/// the clips of that controller replace the base's by name first, as
/// `Studio.OCIChar.LoadAnime` does for the H scenes' animations.
pub fn load(
    game: &Game,
    bundle: &str,
    controller: &str,
    state: &str,
    over: Override,
    parameters: &Parameters,
) -> Result<Motion> {
    let object = game
        .assets
        .find(bundle, controller, Some(class::ANIMATOR_CONTROLLER), None)
        .with_context(|| format!("no animator controller {controller} in {bundle}"))?;
    let read = AnimatorController::read(&object.read()?)?;
    let instead = replacements(game, over)?;
    resolve(&read, state, parameters, &mut |index| {
        let pointer = *read
            .clips
            .get(index)
            .with_context(|| format!("controller {controller} has no clip {index}"))?;
        let clip = game
            .assets
            .deref(&object.file, pointer)
            .with_context(|| format!("a clip of {controller} is missing from {bundle}"))?;
        let clip = instead.get(&clip.name()).unwrap_or(&clip);
        Ok(Arc::new(AnimationClip::read(&clip.read()?)?))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Trs;
    use rk_unityfs::animation::Binding;

    /// root / hips / (arm, leg), and the hashes of the paths.
    fn skeleton() -> (Scene, usize, [usize; 3]) {
        let mut scene = Scene::default();
        let root = scene.add_node("anim", None, Trs::default());
        let hips = scene.add_node("hips", Some(root), Trs::default());
        let arm = scene.add_node("arm", Some(hips), Trs::default());
        let leg = scene.add_node("leg", Some(hips), Trs::default());
        (scene, root, [hips, arm, leg])
    }

    fn bind(path: &str, target: Target) -> Binding {
        Binding {
            path: crc32(path.as_bytes()),
            target,
        }
    }

    #[test]
    fn bones_are_found_by_the_hash_of_their_path() {
        let (scene, root, [hips, arm, _]) = skeleton();
        let skeleton = Skeleton::of(&scene, root);
        assert_eq!(skeleton.len(), 3);
        assert_eq!(skeleton.node(crc32(b"hips")), Some(hips));
        assert_eq!(skeleton.node(crc32(b"hips/arm")), Some(arm));
        // The root itself has no path, and a bare name is not a path.
        assert_eq!(skeleton.node(crc32(b"anim")), None);
        assert_eq!(skeleton.node(crc32(b"arm")), None);
    }

    #[test]
    fn a_clip_writes_the_bones_it_names() {
        let (mut scene, root, [hips, arm, leg]) = skeleton();
        scene.nodes[leg].local.pos = Vec3::new(9.0, 9.0, 9.0);
        // Hips move along x over one second; the arm holds a turn; a bone
        // the skeleton lacks is skipped with its curves.
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let clip = AnimationClip::from_parts(
            vec![
                bind("hips", Target::Position),
                bind("gone", Target::Scale),
                bind("hips/arm", Target::Rotation),
            ],
            vec![],
            (
                1.0,
                vec![vec![0.0, 0.0, 0.0, 5.0, 5.0, 5.0], vec![2.0, 0.0, 0.0, 5.0, 5.0, 5.0]],
            ),
            vec![0.0, 2.0 * half, 0.0, 2.0 * half],
            1.0,
            false,
        );
        let motion = Motion::single(clip);
        let bound = motion.bind(&Skeleton::of(&scene, root));
        assert_eq!((bound.tracks(), bound.unbound), (2, 1));
        assert_eq!(bound.nodes(), [hips, arm]);
        bound.apply(&mut scene, 0.25);
        assert!((scene.nodes[hips].local.pos - Vec3::new(0.5, 0.0, 0.0)).length() < 1e-6);
        // The rotation is made unit length.
        let rot = scene.nodes[arm].local.rot;
        assert!((rot.to_array()[1] - half).abs() < 1e-6 && (rot.length() - 1.0).abs() < 1e-6);
        // What the clip does not name stays.
        assert_eq!(scene.nodes[leg].local.pos, Vec3::new(9.0, 9.0, 9.0));
        assert_eq!(scene.nodes[hips].local.scale, Vec3::ONE);
    }

    fn still(x: f32, seconds: f32, loops: bool) -> AnimationClip {
        AnimationClip::from_parts(
            vec![bind("hips", Target::Position)],
            vec![],
            (1.0, vec![]),
            vec![x, 0.0, 0.0],
            seconds,
            loops,
        )
    }

    #[test]
    fn clips_blend_by_their_shares() {
        let (mut scene, root, [hips, ..]) = skeleton();
        let motion = Motion {
            clips: vec![
                (Arc::new(still(1.0, 2.0, true)), 0.25),
                (Arc::new(still(3.0, 4.0, true)), 0.75),
            ],
            speed: 1.0,
            transitions: 0,
        };
        assert!((motion.duration() - 3.5).abs() < 1e-6);
        motion.bind(&Skeleton::of(&scene, root)).apply(&mut scene, 0.0);
        assert!((scene.nodes[hips].local.pos.x - 2.5).abs() < 1e-6);
        assert_eq!(motion.movement(), 0.0);
    }

    #[test]
    fn time_runs_in_passes_and_loops_or_holds() {
        let looping = Motion::single(still(0.0, 2.0, true));
        // Three seconds at speed 1 from the start: one pass and a half.
        assert!((looping.normalized(0.0, 3.0, 1.0) - 1.5).abs() < 1e-6);
        assert!((looping.phase(1.5, false) - 0.5).abs() < 1e-6);
        // Speed and the starting point count.
        assert!((looping.normalized(0.25, 1.0, 0.5) - 0.5).abs() < 1e-6);
        let once = Motion::single(still(0.0, 2.0, false));
        assert_eq!(once.phase(1.5, false), 1.0);
        assert!((once.phase(1.5, true) - 0.5).abs() < 1e-6);
        // A motion without length stays where it starts.
        let pose = Motion::single(still(0.0, 0.0, false));
        assert_eq!(pose.normalized(0.3, 10.0, 1.0), 0.3);
    }

    #[test]
    fn a_state_takes_its_parameters_from_the_character() {
        use rk_unityfs::animation::{Blend, BlendNode, Parameter, State};
        let height = crc32(b"height");
        let controller = AnimatorController {
            name: "pose".into(),
            states: vec![State {
                name: "stand".into(),
                speed: 2.0,
                nodes: vec![
                    BlendNode {
                        blend: Blend::Simple1D {
                            parameter: height,
                            children: vec![(1, 0.0), (2, 1.0)],
                        },
                    },
                    BlendNode { blend: Blend::Clip(0) },
                    BlendNode { blend: Blend::Clip(1) },
                ],
                transitions: 0,
                loops: false,
            }],
            clips: Vec::new(),
            parameters: vec![Parameter {
                name: "height".into(),
                id: height,
                default: 0.0,
            }],
        };
        let mut asked = Vec::new();
        let parameters = Parameters::of(&[0.25, 0.0, 0.0, 0.0, 0.9]);
        assert_eq!(parameters.breast, 0.9);
        let motion = resolve(&controller, "stand", &parameters, &mut |i| {
            asked.push(i);
            Ok(Arc::new(still(i as f32, 1.0, false)))
        })
        .unwrap();
        assert_eq!(asked, [0, 1]);
        assert_eq!(motion.speed, 2.0);
        let weights: Vec<f32> = motion.clips.iter().map(|c| c.1).collect();
        assert_eq!(weights, [0.75, 0.25]);
        assert!(resolve(&controller, "sit", &parameters, &mut |_| unreachable!()).is_err());

        // The H scenes' first animation option takes the place of the
        // body's height, the parameter the game's H controllers name,
        // since none of them declares `height1`.
        let with = parameters.with_options(Some([0.75, 0.5]));
        let motion = resolve(&controller, "stand", &with, &mut |i| {
            Ok(Arc::new(still(i as f32, 1.0, false)))
        })
        .unwrap();
        let weights: Vec<f32> = motion.clips.iter().map(|c| c.1).collect();
        assert_eq!(weights, [0.25, 0.75]);
        // A controller that does declare `height1` keeps the body's
        // height on `height` and gives the option its own parameter.
        let mut named = controller.clone();
        named.parameters[0].name = "height1".into();
        let motion = resolve(&controller, "stand", &with.with_options(None), &mut |i| {
            Ok(Arc::new(still(i as f32, 1.0, false)))
        })
        .unwrap();
        assert_eq!(motion.clips.iter().map(|c| c.1).collect::<Vec<f32>>(), [0.75, 0.25]);
        let motion = resolve(&named, "stand", &with, &mut |i| {
            Ok(Arc::new(still(i as f32, 1.0, false)))
        })
        .unwrap();
        assert_eq!(motion.clips.iter().map(|c| c.1).collect::<Vec<f32>>(), [0.25, 0.75]);
    }
}
