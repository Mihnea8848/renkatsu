// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! What moves in a map: the objects under its Animators. A map is a live
//! Unity scene in the game, so every enabled Animator on an active object
//! plays its controller from the moment the map loads: its default state,
//! then whatever its transitions lead to on their own. Map files hold
//! meshes baked where the scene stores them, so the Animators are read
//! from the map's bundle and each moved renderer gets a matrix that takes
//! it from where it was baked to where its Animator puts it.
//!
//! Only curves of transforms are played (position, rotation, scale).
//! Curves of materials, lights and active flags are counted and left out.

use crate::build::Tree;
use crate::scene::{deref, open};
use anyhow::Result;
use glam::{Mat4, Quat, Vec3};
use rk_chara::scene::{euler_deg, Trs};
use rk_render::map::{MapMesh, MapScene};
use rk_unityfs::animation::{crc32, AnimationClip, AnimatorController, Target, Transition, EXIT_STATE};
use rk_unityfs::objects::{GameObject, Mesh, MeshFilter, Renderer, Transform};
use rk_unityfs::{class, PPtr};
use rk_vfs::assets::{AssetFile, LoadedBundle};
use rk_vfs::Game;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

/// Which part of a transform a value is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Position,
    Rotation,
    Scale,
}

/// The values a layer gives transforms: positions and scales in x, y, z,
/// rotations as quaternions.
type Pose = BTreeMap<(usize, Kind), [f32; 4]>;

/// A curve set of a clip with the node it drives.
#[derive(Debug, Clone, Copy)]
struct Track {
    node: usize,
    target: Target,
    /// The first of the track's curves in the clip.
    curve: usize,
}

/// A state of a layer, ready to play.
#[derive(Debug, Clone)]
struct State {
    /// The clip and what it drives; none for a state without a motion.
    motion: Option<(Arc<AnimationClip>, Vec<Track>)>,
    /// The state's speed, its speed parameter included.
    speed: f32,
    /// Where each pass starts, in passes.
    cycle_offset: f32,
    /// The transitions that fire on their own: by exit time, without
    /// conditions.
    exits: Vec<Transition>,
}

impl State {
    /// Seconds of one pass. Unity counts a state without a motion, and a
    /// clip without length, as one second.
    fn length(&self) -> f32 {
        match &self.motion {
            Some((clip, _)) if clip.duration() > 0.0 => clip.duration(),
            _ => 1.0,
        }
    }

    /// Passes a second.
    fn rate(&self) -> f32 {
        self.speed / self.length()
    }

    /// The pose at a normalized time, or with `reference` the pose of the
    /// clip's first frame, which an additive layer takes its change from.
    fn pose(&self, normalized: f32, reference: bool, out: &mut Pose) {
        let Some((clip, tracks)) = &self.motion else { return };
        let phase = if reference {
            0.0
        } else if clip.loops {
            (normalized + self.cycle_offset).rem_euclid(1.0)
        } else {
            (normalized + self.cycle_offset).clamp(0.0, 1.0)
        };
        let mut values = Vec::new();
        clip.sample(clip.start + phase * clip.duration(), &mut values);
        for track in tracks {
            let Some(v) = values.get(track.curve..) else { continue };
            let entry = match (track.target, v) {
                (Target::Position, [x, y, z, ..]) => (Kind::Position, [*x, *y, *z, 0.0]),
                (Target::Scale, [x, y, z, ..]) => (Kind::Scale, [*x, *y, *z, 0.0]),
                (Target::Rotation, [x, y, z, w, ..]) => {
                    (Kind::Rotation, Quat::from_xyzw(*x, *y, *z, *w).normalize().to_array())
                }
                (Target::Euler, [x, y, z, ..]) => (Kind::Rotation, euler_deg(*x, *y, *z).to_array()),
                _ => continue,
            };
            out.insert((track.node, entry.0), entry.1);
        }
    }
}

/// Where a layer is at some moment: a state at a normalized time, and
/// while a transition blends, the next state with its share.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Now {
    state: usize,
    at: f32,
    next: Option<(usize, f32, f32)>,
}

/// One layer of an Animator's controller.
#[derive(Debug, Clone)]
struct Layer {
    additive: bool,
    weight: f32,
    default: usize,
    states: Vec<State>,
}

/// The most transitions followed for one moment: a machine that goes
/// round without time passing stops there.
const MOST_STEPS: usize = 100_000;

impl Layer {
    /// Where the layer is `seconds` after it started in `from` (its
    /// default state when the map loads, or one played as `Animator.Play`
    /// does). A transition fires when its state has played to the exit
    /// time; the first to come fires. The machine's exit leads back to the
    /// default state.
    ///
    /// A state that plays backwards, at a negative speed, starts at its
    /// end and plays down to its start, and its exit time is reached
    /// coming down: a door's `close` plays its opening clip backwards and
    /// leaves for `close_Idle` once it is shut.
    fn now(&self, from: usize, seconds: f32) -> Option<Now> {
        let mut state = from;
        let first = self.states.get(state)?;
        // When the state was entered, and at which normalized time.
        let (mut entered, mut start) = (0.0f32, if first.rate() < 0.0 { 1.0f32 } else { 0.0 });
        for _ in 0..MOST_STEPS {
            let s = &self.states[state];
            let rate = s.rate();
            let here = Now {
                state,
                at: start + (seconds - entered) * rate,
                next: None,
            };
            let mut first: Option<(f32, &Transition)> = None;
            for t in &s.exits {
                let wait = if rate < 0.0 {
                    if start <= t.exit_time {
                        0.0
                    } else {
                        (start - t.exit_time) / -rate
                    }
                } else if start >= t.exit_time {
                    0.0
                } else if rate > 0.0 {
                    (t.exit_time - start) / rate
                } else {
                    // A state that stands never gets there.
                    continue;
                };
                if first.is_none_or(|(w, _)| wait < w) {
                    first = Some((wait, t));
                }
            }
            let Some((wait, t)) = first.filter(|(w, _)| entered + w <= seconds) else {
                return Some(here);
            };
            let fired = entered + wait;
            let to = if t.destination >= EXIT_STATE {
                self.default
            } else {
                t.destination as usize
            };
            let Some(into) = self.states.get(to) else {
                return Some(here);
            };
            let blend = if t.fixed_duration {
                t.duration
            } else if rate != 0.0 {
                t.duration / rate.abs()
            } else {
                0.0
            };
            if seconds < fired + blend {
                return Some(Now {
                    next: Some((
                        to,
                        t.offset + (seconds - fired) * into.rate(),
                        (seconds - fired) / blend,
                    )),
                    ..here
                });
            }
            // The next state has played since the transition began.
            (state, entered, start) = (to, fired, t.offset);
        }
        Some(Now {
            state,
            at: start,
            next: None,
        })
    }

    /// The layer's pose after `seconds`. What only one of two blended
    /// states drives is blended with where the scene stores it, as states
    /// that write default values do.
    fn pose(&self, from: usize, seconds: f32, reference: bool, rest: &[Trs]) -> Pose {
        let mut pose = Pose::new();
        let Some(now) = self.now(from, seconds) else {
            return pose;
        };
        self.states[now.state].pose(now.at, reference, &mut pose);
        let Some((next, at, share)) = now.next else { return pose };
        let mut other = Pose::new();
        self.states[next].pose(at, reference, &mut other);
        let keys: std::collections::BTreeSet<(usize, Kind)> = pose.keys().chain(other.keys()).copied().collect();
        for key in keys {
            let stored = stored(&rest[key.0], key.1);
            let a = pose.get(&key).copied().unwrap_or(stored);
            let b = other.get(&key).copied().unwrap_or(stored);
            pose.insert(key, mix(key.1, a, b, share));
        }
        pose
    }
}

/// A part of a transform as a pose holds it.
fn stored(local: &Trs, kind: Kind) -> [f32; 4] {
    match kind {
        Kind::Position => local.pos.extend(0.0).to_array(),
        Kind::Scale => local.scale.extend(0.0).to_array(),
        Kind::Rotation => local.rot.to_array(),
    }
}

/// `a` to `b` by `share`: a straight line, or the short way round for
/// rotations.
fn mix(kind: Kind, a: [f32; 4], b: [f32; 4], share: f32) -> [f32; 4] {
    match kind {
        Kind::Rotation => Quat::from_array(a).slerp(Quat::from_array(b), share).to_array(),
        _ => std::array::from_fn(|i| a[i] + (b[i] - a[i]) * share),
    }
}

/// One Animator: its controller's layers, bound to the nodes.
#[derive(Debug, Clone, Default)]
struct Rig {
    layers: Vec<Layer>,
    /// Its place in `MapMotion::animators`.
    animator: usize,
    /// The first layer's state it was sent to and the second it was, in
    /// place of starting in its default state when the map loads.
    sent: Option<(usize, f32)>,
}

impl Rig {
    /// Writes the Animator's pose after `seconds` over `locals`. The
    /// first layer replaces what it drives; a later one replaces by its
    /// weight, or adds its change since its clip's first frame.
    fn apply(&self, seconds: f32, rest: &[Trs], locals: &mut [Trs]) {
        for (i, layer) in self.layers.iter().enumerate() {
            let weight = if i == 0 { 1.0 } else { layer.weight.clamp(0.0, 1.0) };
            if weight <= 0.0 {
                continue;
            }
            // The first layer runs from where it was sent, if it was.
            let (from, since) = match self.sent.filter(|_| i == 0) {
                Some((state, at)) => (state, (seconds - at).max(0.0)),
                None => (layer.default, seconds),
            };
            let pose = layer.pose(from, since, false, rest);
            let reference = if layer.additive && i > 0 {
                Some(layer.pose(from, since, true, rest))
            } else {
                None
            };
            for ((node, kind), value) in pose {
                let local = &mut locals[node];
                let from = reference
                    .as_ref()
                    .map(|r| r.get(&(node, kind)).copied().unwrap_or(value));
                let v = Vec3::new(value[0], value[1], value[2]);
                match (kind, from) {
                    (Kind::Position, None) => local.pos = local.pos.lerp(v, weight),
                    (Kind::Scale, None) => local.scale = local.scale.lerp(v, weight),
                    (Kind::Rotation, None) => local.rot = local.rot.slerp(Quat::from_array(value), weight),
                    (Kind::Position, Some(r)) => local.pos += (v - Vec3::new(r[0], r[1], r[2])) * weight,
                    (Kind::Scale, Some(r)) => {
                        let by = v / Vec3::new(r[0], r[1], r[2]);
                        if by.is_finite() {
                            local.scale *= Vec3::ONE.lerp(by, weight);
                        }
                    }
                    (Kind::Rotation, Some(r)) => {
                        let turn = Quat::from_array(r).inverse() * Quat::from_array(value);
                        local.rot = (local.rot * Quat::IDENTITY.slerp(turn, weight)).normalize();
                    }
                }
            }
        }
    }
}

/// A transform of the scene that an Animator drives, or one above a
/// moved renderer. A node's parent comes before it.
#[derive(Debug, Clone)]
struct Node {
    parent: Option<usize>,
    rest: Trs,
}

/// A renderer under a driven transform.
#[derive(Debug, Clone)]
pub struct Part {
    /// Its object's name, which the map's object of the same renderer has.
    pub name: String,
    node: usize,
    /// The lowest and highest corner of its mesh where the scene stores
    /// it: what the map's object of the same renderer has as its bounds.
    pub bounds: ([f32; 3], [f32; 3]),
    /// From the world back into the renderer's own space, at rest.
    rest_inverse: Mat4,
    /// The map's object it is, by its place in `MapScene::objects`, once
    /// bound.
    pub object: Option<usize>,
}

/// What one Animator of a map is and does, for looking at.
#[derive(Debug, Clone, Default)]
pub struct AnimatorInfo {
    /// The object's path in the scene.
    pub object: String,
    pub controller: String,
    /// Why it does not play: disabled, on an inactive object, without a
    /// controller. `None` for one that plays.
    pub idle: Option<&'static str>,
    pub layers: Vec<LayerInfo>,
    /// Curve sets of transforms the Animator's object has.
    pub transform_tracks: usize,
    /// Curves of anything else: materials, lights, active flags.
    pub other_curves: usize,
    /// Curve sets of transforms the object does not have.
    pub unbound: usize,
    /// The renderers it moves, by their place in `MapMotion::parts`.
    pub parts: Vec<usize>,
    /// Skinned renderers under it, which are not drawn.
    pub skinned: usize,
}

#[derive(Debug, Clone, Default)]
pub struct LayerInfo {
    pub additive: bool,
    pub weight: f32,
    pub default_state: usize,
    pub states: Vec<StateInfo>,
    /// Transitions that wait for a parameter, which nothing here sets.
    pub conditional: usize,
}

#[derive(Debug, Clone, Default)]
pub struct StateInfo {
    pub name: String,
    pub tag: String,
    pub speed: f32,
    pub cycle_offset: f32,
    /// The clip's name, its seconds and whether it loops.
    pub clip: Option<(String, f32, bool)>,
    /// Where its transitions lead and after how many passes.
    pub exits: Vec<(u32, f32)>,
}

/// An Animator of a map that opens and closes something: a door.
#[derive(Debug, Clone, PartialEq)]
pub struct Opening {
    /// Its place in `MapMotion::animators`, and its object's path.
    pub animator: usize,
    pub object: String,
    /// The renderers it moves, by their place in `MapMotion::parts`.
    pub parts: Vec<usize>,
    /// Its first layer's states: held open, held closed, and the moves.
    pub held_open: usize,
    pub held_closed: usize,
    pub opening: Option<usize>,
    pub closing: Option<usize>,
    /// The game starts it open.
    pub starts_open: bool,
}

/// The moving objects of a map.
#[derive(Debug, Clone, Default)]
pub struct MapMotion {
    nodes: Vec<Node>,
    /// The Animators, those higher in the scene first: one below another
    /// writes after it.
    rigs: Vec<Rig>,
    pub parts: Vec<Part>,
    /// Every Animator of the map, those that play and those that do not.
    pub animators: Vec<AnimatorInfo>,
}

/// An object of a built map as the moving renderers are matched to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    pub bounds: ([f32; 3], [f32; 3]),
    pub lightmapped: bool,
}

/// The objects of a built map, in their order, to bind a map's motion to.
pub fn places(scene: &MapScene) -> Vec<Place> {
    scene
        .objects
        .iter()
        .map(|o| Place {
            name: o.name.clone(),
            bounds: o.mesh.bounds,
            lightmapped: o.lightmap.is_some(),
        })
        .collect()
}

fn middle(bounds: ([f32; 3], [f32; 3])) -> Vec3 {
    (Vec3::from(bounds.0) + Vec3::from(bounds.1)) * 0.5
}

impl MapMotion {
    /// Whether nothing of the map moves.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// The Animators that open and close: those whose first layer has a
    /// state held open and one held closed, by the game's names
    /// (`open_Idle`, `close_Idle`), and the moves between them (`open`,
    /// `close`) where it has them.
    pub fn openings(&self) -> Vec<Opening> {
        let mut out = Vec::new();
        for (animator, info) in self.animators.iter().enumerate() {
            if info.idle.is_some() || !self.rigs.iter().any(|r| r.animator == animator) {
                continue;
            }
            let Some(layer) = info.layers.first() else { continue };
            let find = |held: bool, open: bool| {
                layer.states.iter().position(|s| {
                    let name = s.name.to_lowercase();
                    let opens = name.contains("open");
                    let closes = name.contains("clos");
                    name.contains("idle") == held && if open { opens && !closes } else { closes && !opens }
                })
            };
            let (Some(held_open), Some(held_closed)) = (find(true, true), find(true, false)) else {
                continue;
            };
            out.push(Opening {
                animator,
                object: info.object.clone(),
                parts: info.parts.clone(),
                held_open,
                held_closed,
                opening: find(false, true),
                closing: find(false, false),
                starts_open: layer.default_state == held_open,
            });
        }
        out
    }

    /// Sends an opening Animator to open or closed: at once, or with the
    /// move between them from second `at` of the clock that poses the map.
    pub fn set_open(&mut self, opening: &Opening, open: bool, at: Option<f32>) {
        let Some(rig) = self.rigs.iter_mut().find(|r| r.animator == opening.animator) else {
            return;
        };
        let moving = if open { opening.opening } else { opening.closing };
        let held = if open { opening.held_open } else { opening.held_closed };
        rig.sent = Some(match (moving, at) {
            (Some(state), Some(at)) => (state, at),
            _ => (held, 0.0),
        });
    }

    /// Finds each moving renderer among a built map's objects: the one of
    /// its name whose bounds lie where the renderer's do. A map file keeps
    /// no link to the scene's renderers, and names repeat. A renderer the
    /// map does not hold (another time of day's, or one not drawn) stays
    /// unbound. Returns how many were found.
    pub fn bind(&mut self, places: &[Place]) -> usize {
        let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, p) in places.iter().enumerate() {
            by_name.entry(p.name.as_str()).or_default().push(i);
        }
        let mut taken: HashSet<usize> = HashSet::new();
        let mut found = 0;
        for part in &mut self.parts {
            let centre = middle(part.bounds);
            let size = (Vec3::from(part.bounds.1) - Vec3::from(part.bounds.0)).length();
            // The same mesh through the same matrix: the bounds agree to
            // rounding.
            let near = 1e-3 * (1.0 + size + centre.length());
            part.object = by_name
                .get(part.name.as_str())
                .into_iter()
                .flatten()
                .filter(|i| !taken.contains(*i))
                .map(|i| (*i, middle(places[*i].bounds).distance(centre)))
                .filter(|(_, d)| *d <= near)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            if let Some(i) = part.object {
                taken.insert(i);
                found += 1;
            }
        }
        found
    }

    /// Where each bound object stands `seconds` after the map loaded, as a
    /// matrix over its baked mesh: `animated world * inverse(rest world)`.
    /// By its place in `MapScene::objects`, for
    /// `Renderer::set_map_object_matrices`. Second 0 is the first frame of
    /// each Animator's default state, which need not be where the scene
    /// stores the object.
    pub fn pose(&self, seconds: f32) -> Vec<(usize, Mat4)> {
        let rest: Vec<Trs> = self.nodes.iter().map(|n| n.rest).collect();
        let mut locals = rest.clone();
        for rig in &self.rigs {
            rig.apply(seconds.max(0.0), &rest, &mut locals);
        }
        let mut worlds: Vec<Mat4> = Vec::with_capacity(locals.len());
        for (node, local) in self.nodes.iter().zip(&locals) {
            let above = node.parent.map_or(Mat4::IDENTITY, |p| worlds[p]);
            worlds.push(above * local.matrix());
        }
        self.parts
            .iter()
            .filter_map(|p| Some((p.object?, worlds[p.node] * p.rest_inverse)))
            .collect()
    }

    /// The rest worlds of the nodes, parents first.
    fn rest_worlds(&self) -> Vec<Mat4> {
        let mut worlds: Vec<Mat4> = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let above = node.parent.map_or(Mat4::IDENTITY, |p| worlds[p]);
            worlds.push(above * node.rest.matrix());
        }
        worlds
    }
}

/// The transforms and game objects of one file of a bundle.
struct Hierarchy {
    transforms: HashMap<i64, Transform>,
    transform_of: HashMap<i64, i64>,
    objects: HashMap<i64, GameObject>,
}

impl Hierarchy {
    fn read(file: &AssetFile) -> Hierarchy {
        let sf = &file.sf;
        let mut transforms = HashMap::new();
        let mut transform_of = HashMap::new();
        for obj in sf.objects_of(class::TRANSFORM) {
            if let Ok(t) = sf.read(obj).and_then(|o| Transform::read(&o)) {
                transform_of.insert(t.game_object.path_id, obj.path_id);
                transforms.insert(obj.path_id, t);
            }
        }
        let mut objects = HashMap::new();
        for obj in sf.objects_of(class::GAME_OBJECT) {
            if let Ok(go) = sf.read(obj).and_then(|o| GameObject::read(&o)) {
                objects.insert(obj.path_id, go);
            }
        }
        Hierarchy {
            transforms,
            transform_of,
            objects,
        }
    }

    fn tree(&self) -> Tree<'_> {
        Tree {
            transforms: &self.transforms,
            transform_of: &self.transform_of,
            objects: &self.objects,
        }
    }

    fn name(&self, transform: i64) -> &str {
        self.transforms
            .get(&transform)
            .and_then(|t| self.objects.get(&t.game_object.path_id))
            .map_or("", |o| o.name.as_str())
    }

    /// The transform above, in the same file.
    fn father(&self, transform: i64) -> Option<i64> {
        let t = self.transforms.get(&transform)?;
        (!t.father.is_null() && t.father.file_id == 0 && self.transforms.contains_key(&t.father.path_id))
            .then_some(t.father.path_id)
    }

    /// The transforms from the scene's root down to `transform`.
    fn chain(&self, transform: i64) -> Vec<i64> {
        let mut chain = vec![transform];
        while let Some(f) = self.father(chain[chain.len() - 1]).filter(|_| chain.len() < 256) {
            chain.push(f);
        }
        chain.reverse();
        chain
    }

    /// Every transform below `root` by the CRC-32 of its path from there,
    /// as clips name them; the root itself has the empty path.
    fn paths(&self, root: i64) -> HashMap<u32, i64> {
        let mut out = HashMap::new();
        let mut stack = vec![(root, String::new())];
        while let Some((id, path)) = stack.pop() {
            out.entry(crc32(path.as_bytes())).or_insert(id);
            let Some(t) = self.transforms.get(&id) else { continue };
            if out.len() > 100_000 {
                break;
            }
            for child in t.children.iter().filter(|c| c.file_id == 0) {
                let name = self.name(child.path_id);
                let below = if path.is_empty() {
                    name.to_owned()
                } else {
                    format!("{path}/{name}")
                };
                stack.push((child.path_id, below));
            }
        }
        out
    }
}

/// The nodes of a motion while it is read: transforms of one file by id.
#[derive(Default)]
struct Nodes {
    nodes: Vec<Node>,
    index: HashMap<i64, usize>,
}

impl Nodes {
    /// The node of a transform, made with those above it when new.
    fn of(&mut self, h: &Hierarchy, transform: i64) -> usize {
        let mut parent = None;
        for id in h.chain(transform) {
            let node = match self.index.get(&id) {
                Some(n) => *n,
                None => {
                    let rest = h.transforms.get(&id).map_or(Trs::default(), |t| Trs {
                        pos: Vec3::from(t.position),
                        rot: Quat::from_array(t.rotation),
                        scale: Vec3::from(t.scale),
                    });
                    self.nodes.push(Node { parent, rest });
                    self.index.insert(id, self.nodes.len() - 1);
                    self.nodes.len() - 1
                }
            };
            parent = Some(node);
        }
        parent.expect("a chain holds its transform")
    }
}

/// The class name of a script component.
fn script_of(game: &Game, loaded: &LoadedBundle, file: &Arc<AssetFile>, o: &rk_unityfs::UObject) -> String {
    o.pptr("m_Script")
        .and_then(|p| deref(game, loaded, file, p))
        .and_then(|r| r.read().ok())
        .and_then(|s| s.str("m_ClassName").map(str::to_owned))
        .unwrap_or_default()
}

/// The values the game's own scripts give an Animator's parameters, by
/// the parameter's name. `SpotLaser.Start` sets `offset` to its own
/// field, and its `Update` sets `beam` to 1 while the child
/// `spotBeam_gp/spotBeam_active` is active itself, else 0.
fn script_parameters(
    game: &Game,
    loaded: &LoadedBundle,
    file: &Arc<AssetFile>,
    h: &Hierarchy,
    go: i64,
    paths: &HashMap<u32, i64>,
) -> HashMap<String, f32> {
    let mut out = HashMap::new();
    let Some(object) = h.objects.get(&go) else { return out };
    for c in object.components.iter().filter(|c| c.file_id == 0) {
        let Some(info) = file
            .sf
            .object(c.path_id)
            .filter(|i| i.class_id == class::MONO_BEHAVIOUR)
        else {
            continue;
        };
        let Ok(o) = file.sf.read(info) else { continue };
        if o.i64("m_Enabled").unwrap_or(1) == 0 || script_of(game, loaded, file, &o) != "SpotLaser" {
            continue;
        }
        out.insert("offset".to_owned(), o.f32("offset").unwrap_or(0.0));
        let active = paths
            .get(&crc32(b"spotBeam_gp/spotBeam_active"))
            .and_then(|t| h.transforms.get(t))
            .and_then(|t| h.objects.get(&t.game_object.path_id))
            .is_some_and(|o| o.active);
        out.insert("beam".to_owned(), if active { 1.0 } else { 0.0 });
    }
    out
}

/// A map's Animators and the renderers they move, read from its bundle.
/// Objects count as active the way the scene stores them, whatever the
/// time of day. `bind` then finds the renderers among a built map's
/// objects.
pub fn read(game: &Game, bundle: &str) -> Result<MapMotion> {
    let loaded = open(game, bundle)?;
    let mut motion = MapMotion::default();
    let mut meshes: HashMap<(String, i64), Option<Arc<Mesh>>> = HashMap::new();
    let none = HashSet::new();
    for file in &loaded.files {
        let sf = &file.sf;
        if sf.objects_of(class::ANIMATOR).next().is_none() {
            continue;
        }
        let h = Hierarchy::read(file);
        let tree = h.tree();
        let mut nodes = Nodes {
            nodes: std::mem::take(&mut motion.nodes),
            index: HashMap::new(),
        };
        // Each playing Animator with how deep it stands, where it is in
        // `animators`, and the transforms it drives.
        let mut rigs: Vec<(usize, Rig, usize, HashSet<i64>)> = Vec::new();
        for obj in sf.objects_of(class::ANIMATOR) {
            let Ok(o) = sf.read(obj) else { continue };
            let go = o.pptr("m_GameObject").unwrap_or_default().path_id;
            let Some(&root) = h.transform_of.get(&go) else { continue };
            let chain = h.chain(root);
            let mut info = AnimatorInfo {
                object: chain.iter().map(|t| h.name(*t)).collect::<Vec<_>>().join("/"),
                ..Default::default()
            };
            let controller = o.pptr("m_Controller").and_then(|p| deref(game, &loaded, file, p));
            info.idle = if o.i64("m_Enabled").unwrap_or(1) == 0 {
                Some("disabled")
            } else if !tree.shown(go, &none, &none) {
                Some("inactive")
            } else if controller.is_none() {
                Some("no controller")
            } else {
                None
            };
            let read = controller.as_ref().and_then(|c| {
                let o = c.read().ok()?;
                Some((AnimatorController::read(&o).ok()?, AnimatorController::layers(&o).ok()?))
            });
            let (Some(controller), Some((read, layers))) = (controller, read) else {
                motion.animators.push(info);
                continue;
            };
            info.controller = read.name.clone();
            let paths = h.paths(root);
            let scripted = script_parameters(game, &loaded, file, &h, go, &paths);
            let parameter = |id: u32| {
                read.parameters
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| scripted.get(&p.name).copied().unwrap_or(p.default))
            };
            let mut clips: HashMap<usize, Option<Arc<AnimationClip>>> = HashMap::new();
            let mut driven: HashSet<i64> = HashSet::new();
            let mut rig = Rig::default();
            for layer in &layers {
                let mut layer_info = LayerInfo {
                    additive: layer.additive,
                    weight: layer.weight,
                    default_state: layer.default_state,
                    ..Default::default()
                };
                let mut states = Vec::new();
                for s in &layer.states {
                    // The clip the state plays: of a blend tree, the one
                    // with the largest share at the parameters' values.
                    let index = s
                        .state
                        .weights(&|id| parameter(id).unwrap_or(0.0))
                        .into_iter()
                        .max_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(i, _)| i);
                    let clip = index.and_then(|i| {
                        clips
                            .entry(i)
                            .or_insert_with(|| {
                                let p = *read.clips.get(i)?;
                                let r = deref(game, &loaded, &controller.file, p)?;
                                AnimationClip::read(&r.read().ok()?).ok().map(Arc::new)
                            })
                            .clone()
                    });
                    let motion = clip.map(|clip| {
                        let mut tracks = Vec::new();
                        let mut curve = 0;
                        for b in &clip.bindings {
                            match (b.target, paths.get(&b.path)) {
                                (Target::Other, _) => info.other_curves += 1,
                                (_, None) => info.unbound += 1,
                                (target, Some(&id)) => {
                                    info.transform_tracks += 1;
                                    driven.insert(id);
                                    tracks.push(Track {
                                        node: nodes.of(&h, id),
                                        target,
                                        curve,
                                    });
                                }
                            }
                            curve += b.width();
                        }
                        (clip, tracks)
                    });
                    let exits: Vec<Transition> = s
                        .exits
                        .iter()
                        .filter(|t| t.has_exit_time && t.conditions == 0)
                        .cloned()
                        .collect();
                    layer_info.conditional += s.exits.len() - exits.len();
                    layer_info.states.push(StateInfo {
                        name: s.state.name.clone(),
                        tag: s.tag.clone(),
                        speed: s.state.speed * parameter(s.speed_parameter).unwrap_or(1.0),
                        cycle_offset: parameter(s.cycle_offset_parameter).unwrap_or(s.cycle_offset),
                        clip: motion.as_ref().map(|(c, _)| (c.name.clone(), c.duration(), c.loops)),
                        exits: exits.iter().map(|t| (t.destination, t.exit_time)).collect(),
                    });
                    states.push(State {
                        motion,
                        speed: s.state.speed * parameter(s.speed_parameter).unwrap_or(1.0),
                        cycle_offset: parameter(s.cycle_offset_parameter).unwrap_or(s.cycle_offset),
                        exits,
                    });
                }
                info.layers.push(layer_info);
                rig.layers.push(Layer {
                    additive: layer.additive,
                    weight: layer.weight,
                    default: layer.default_state,
                    states,
                });
            }
            if info.idle.is_none() && !driven.is_empty() {
                rig.animator = motion.animators.len();
                rigs.push((chain.len(), rig, motion.animators.len(), driven));
            }
            motion.animators.push(info);
        }
        // The renderers under a driven transform.
        let driven: HashMap<i64, Vec<usize>> = rigs.iter().fold(HashMap::new(), |mut all, (_, _, info, ids)| {
            for id in ids {
                all.entry(*id).or_default().push(*info);
            }
            all
        });
        for obj in sf.objects_of(class::SKINNED_MESH_RENDERER) {
            let Ok(go) = sf.read(obj).map(|o| o.pptr("m_GameObject").unwrap_or_default().path_id) else {
                continue;
            };
            let Some(&transform) = h.transform_of.get(&go) else {
                continue;
            };
            let chain = h.chain(transform);
            for info in chain.iter().filter_map(|t| driven.get(t)).flatten() {
                motion.animators[*info].skinned += 1;
            }
        }
        for obj in sf.objects_of(class::MESH_RENDERER) {
            let Ok(renderer) = sf.read(obj).and_then(|o| Renderer::read(&o)) else {
                continue;
            };
            let go = renderer.game_object.path_id;
            let Some(&transform) = h.transform_of.get(&go) else {
                continue;
            };
            let chain = h.chain(transform);
            let movers: Vec<usize> = chain.iter().filter_map(|t| driven.get(t)).flatten().copied().collect();
            if movers.is_empty() || !renderer.enabled || !tree.shown(go, &none, &none) {
                continue;
            }
            let Some(object) = h.objects.get(&go) else { continue };
            let Some(filter) = object.components.iter().find_map(|c| filter_of(file, *c)) else {
                continue;
            };
            let Some(mesh) = mesh_of(game, &loaded, file, filter.mesh, &mut meshes) else {
                continue;
            };
            // As the map's builder places it, so that the bounds agree.
            let world = tree.world_of(go);
            let placed: Vec<[f32; 3]> = mesh
                .positions
                .iter()
                .map(|p| world.transform_point3(Vec3::from(*p)).to_array())
                .collect();
            let node = nodes.of(&h, transform);
            motion.parts.push(Part {
                name: object.name.clone(),
                node,
                bounds: MapMesh::compute_bounds(&placed),
                rest_inverse: Mat4::IDENTITY,
                object: None,
            });
            for info in movers {
                let parts = &mut motion.animators[info].parts;
                if parts.last() != Some(&(motion.parts.len() - 1)) {
                    parts.push(motion.parts.len() - 1);
                }
            }
        }
        motion.nodes = nodes.nodes;
        rigs.sort_by_key(|r| r.0);
        motion.rigs.extend(rigs.into_iter().map(|r| r.1));
    }
    let worlds = motion.rest_worlds();
    for part in &mut motion.parts {
        part.rest_inverse = worlds[part.node].inverse();
    }
    Ok(motion)
}

/// The MeshFilter among a game object's components.
fn filter_of(file: &Arc<AssetFile>, component: PPtr) -> Option<MeshFilter> {
    let info = file.sf.object(component.path_id).filter(|_| component.file_id == 0)?;
    if info.class_id != class::MESH_FILTER {
        return None;
    }
    MeshFilter::read(&file.sf.read(info).ok()?).ok()
}

fn mesh_of(
    game: &Game,
    loaded: &LoadedBundle,
    file: &Arc<AssetFile>,
    p: PPtr,
    meshes: &mut HashMap<(String, i64), Option<Arc<Mesh>>>,
) -> Option<Arc<Mesh>> {
    let r = deref(game, loaded, file, p)?;
    meshes
        .entry((r.file.cab.clone(), r.info.path_id))
        .or_insert_with(|| Mesh::read(&r.read().ok()?).ok().map(Arc::new))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_unityfs::animation::Binding;

    fn exit(destination: u32, exit_time: f32, duration: f32) -> Transition {
        Transition {
            destination,
            has_exit_time: true,
            exit_time,
            duration,
            fixed_duration: true,
            offset: 0.0,
            conditions: 0,
        }
    }

    fn track(target: Target) -> Vec<Track> {
        vec![Track {
            node: 0,
            target,
            curve: 0,
        }]
    }

    /// A clip that turns node 0 about Y from 0 to `degrees` over
    /// `seconds`, in a straight line.
    fn turn(degrees: f32, seconds: f32, loops: bool) -> State {
        let clip = AnimationClip::from_parts(
            vec![Binding {
                path: 0,
                target: Target::Euler,
            }],
            vec![
                vec![(0.0, [0.0; 4])],
                vec![(0.0, [0.0, 0.0, degrees / seconds, 0.0])],
                vec![(0.0, [0.0; 4])],
            ],
            (0.0, Vec::new()),
            Vec::new(),
            seconds,
            loops,
        );
        State {
            motion: Some((Arc::new(clip), track(Target::Euler))),
            speed: 1.0,
            cycle_offset: 0.0,
            exits: Vec::new(),
        }
    }

    /// A clip that holds node 0 at `degrees` about Y, as the doors'
    /// `Fix_90_Yaw` does.
    fn hold(degrees: f32) -> State {
        let clip = AnimationClip::from_parts(
            vec![Binding {
                path: 0,
                target: Target::Euler,
            }],
            Vec::new(),
            (0.0, Vec::new()),
            vec![0.0, degrees, 0.0],
            0.0,
            false,
        );
        State {
            motion: Some((Arc::new(clip), track(Target::Euler))),
            speed: 1.0,
            cycle_offset: 0.0,
            exits: Vec::new(),
        }
    }

    fn empty() -> State {
        State {
            motion: None,
            speed: 1.0,
            cycle_offset: 0.0,
            exits: Vec::new(),
        }
    }

    fn layer(states: Vec<State>) -> Layer {
        Layer {
            additive: false,
            weight: 0.0,
            default: 0,
            states,
        }
    }

    fn yaw(pose: &Pose) -> f32 {
        let q = Quat::from_array(pose[&(0, Kind::Rotation)]);
        let (axis, angle) = q.to_axis_angle();
        (angle * axis.y.signum()).to_degrees()
    }

    #[test]
    fn a_looping_state_starts_over_and_another_holds_its_end() {
        let rest = [Trs::default()];
        let looping = layer(vec![turn(120.0, 6.0, true)]);
        assert!((yaw(&looping.pose(looping.default, 1.5, false, &rest)) - 30.0).abs() < 1e-3);
        assert!((yaw(&looping.pose(looping.default, 7.5, false, &rest)) - 30.0).abs() < 1e-3);
        let once = layer(vec![turn(120.0, 6.0, false)]);
        assert!((yaw(&once.pose(once.default, 3.0, false, &rest)) - 60.0).abs() < 1e-3);
        assert!((yaw(&once.pose(once.default, 60.0, false, &rest)) - 120.0).abs() < 1e-3);
    }

    #[test]
    fn speed_and_cycle_offset_shift_the_clip() {
        let rest = [Trs::default()];
        // SpotLaser's: a quarter of a pass ahead, and stopped while the
        // beam is off.
        let mut s = turn(120.0, 12.0, true);
        s.cycle_offset = 0.25;
        let ahead = layer(vec![s.clone()]);
        assert!((yaw(&ahead.pose(ahead.default, 0.0, false, &rest)) - 30.0).abs() < 1e-3);
        assert!((yaw(&ahead.pose(ahead.default, 3.0, false, &rest)) - 60.0).abs() < 1e-3);
        s.speed = 0.0;
        let stopped = layer(vec![s]);
        assert!((yaw(&stopped.pose(stopped.default, 100.0, false, &rest)) - 30.0).abs() < 1e-3);
    }

    #[test]
    fn a_state_without_exits_stays() {
        // The doors: `open_Idle` is the default state and leads nowhere.
        let rest = [Trs::default()];
        let mut open = turn(90.0, 0.8333, false);
        open.exits = vec![exit(0, 0.0, 0.25)];
        let doors = layer(vec![hold(90.0), open, hold(0.0)]);
        for seconds in [0.0, 1.0, 500.0] {
            assert_eq!(doors.now(doors.default, seconds).map(|n| n.state), Some(0));
            assert!((yaw(&doors.pose(doors.default, seconds, false, &rest)) - 90.0).abs() < 1e-3);
        }
    }

    #[test]
    fn exit_times_lead_through_the_states_and_back_to_the_first() {
        // The stage's spot lights: a second-long empty state for 81
        // passes, then a 4 second clip held for 20.5 passes, then out.
        let mut wait = empty();
        wait.exits = vec![exit(1, 81.0, 0.0)];
        let mut play = turn(40.0, 4.0, false);
        play.exits = vec![exit(EXIT_STATE + 1, 20.5, 0.0)];
        let l = layer(vec![wait, play]);
        let at = |seconds: f32| l.now(l.default, seconds).expect("has states");
        assert_eq!(at(80.9).state, 0);
        assert_eq!(at(81.0).state, 1);
        assert!((at(83.0).at - 0.5).abs() < 1e-4);
        assert_eq!(at(162.9).state, 1);
        // 81 + 20.5 * 4 seconds in, the machine leaves and starts again.
        assert_eq!(at(163.0).state, 0);
        assert!((at(164.0).at - 1.0).abs() < 1e-3);
        assert_eq!(at(163.0 + 81.5).state, 1);
        let rest = [Trs::default()];
        assert!(l.pose(l.default, 10.0, false, &rest).is_empty());
        assert!((yaw(&l.pose(l.default, 83.0, false, &rest)) - 20.0).abs() < 1e-3);
        assert!((yaw(&l.pose(l.default, 100.0, false, &rest)) - 40.0).abs() < 1e-3);
    }

    #[test]
    fn a_transition_blends_the_two_states() {
        let mut from = hold(0.0);
        from.exits = vec![exit(1, 2.0, 0.5)];
        let l = layer(vec![from, hold(90.0)]);
        let rest = [Trs::default()];
        assert!(yaw(&l.pose(l.default, 1.9, false, &rest)).abs() < 1e-3);
        let now = l.now(l.default, 2.25).expect("has states");
        assert_eq!(now.state, 0);
        assert!(matches!(now.next, Some((1, _, share)) if (share - 0.5).abs() < 1e-4));
        let mid = yaw(&l.pose(l.default, 2.25, false, &rest));
        assert!((mid - 45.0).abs() < 1e-2, "{mid} {now:?}");
        assert!((yaw(&l.pose(l.default, 2.6, false, &rest)) - 90.0).abs() < 1e-3);
    }

    #[test]
    fn a_door_closes_by_playing_its_opening_backwards() {
        // The game's door: `close` plays the 0 to 90 degree turn at speed
        // -1 and leaves for `close_Idle`, held at 0, at exit time 0.
        let mut close = turn(90.0, 1.0, false);
        close.speed = -1.0;
        close.exits = vec![exit(1, 1.2548346e-9, 0.0)];
        let l = layer(vec![close, hold(0.0)]);
        let rest = [Trs::default()];
        assert!((yaw(&l.pose(0, 0.0, false, &rest)) - 90.0).abs() < 1e-3, "starts open");
        assert!((yaw(&l.pose(0, 0.5, false, &rest)) - 45.0).abs() < 1e-2, "half shut");
        assert_eq!(l.now(0, 0.5).map(|n| n.state), Some(0));
        assert_eq!(l.now(0, 1.5).map(|n| n.state), Some(1), "held shut once it got there");
        assert!(yaw(&l.pose(0, 1.5, false, &rest)).abs() < 1e-3);
    }

    #[test]
    fn a_machine_that_goes_round_at_once_ends() {
        let mut a = empty();
        a.exits = vec![exit(0, 0.0, 0.0)];
        assert!(layer(vec![a]).now(0, 1.0).is_some());
        assert!(layer(Vec::new()).now(0, 1.0).is_none());
    }

    /// A pivot at x = 2 with a part one unit along its x.
    fn swinging(rig: Rig) -> MapMotion {
        let pivot = Trs {
            pos: Vec3::new(2.0, 0.0, 0.0),
            ..Default::default()
        };
        let leaf = Trs {
            pos: Vec3::new(1.0, 0.0, 0.0),
            ..Default::default()
        };
        let mut motion = MapMotion {
            nodes: vec![
                Node {
                    parent: None,
                    rest: pivot,
                },
                Node {
                    parent: Some(0),
                    rest: leaf,
                },
            ],
            rigs: vec![rig],
            parts: vec![Part {
                name: "door".into(),
                node: 1,
                bounds: ([2.9, -1.0, -0.1], [3.1, 1.0, 0.1]),
                rest_inverse: Mat4::IDENTITY,
                object: None,
            }],
            animators: Vec::new(),
        };
        let worlds = motion.rest_worlds();
        motion.parts[0].rest_inverse = worlds[1].inverse();
        motion
    }

    fn place(name: &str, centre: [f32; 3]) -> Place {
        let c = Vec3::from(centre);
        Place {
            name: name.into(),
            bounds: (
                (c - Vec3::new(0.1, 1.0, 0.1)).to_array(),
                (c + Vec3::new(0.1, 1.0, 0.1)).to_array(),
            ),
            lightmapped: false,
        }
    }

    #[test]
    fn a_part_is_carried_from_where_it_was_baked() {
        let mut motion = swinging(Rig {
            layers: vec![layer(vec![turn(90.0, 2.0, false)])],
            ..Default::default()
        });
        // Unbound parts are left out.
        assert!(motion.pose(1.0).is_empty());
        let places = [
            place("wall", [3.0, 0.0, 0.0]),
            place("door", [9.0, 0.0, 0.0]),
            place("door", [3.0, 0.0, 0.0]),
        ];
        assert_eq!(motion.bind(&places), 1);
        assert_eq!(motion.parts[0].object, Some(2));
        // At the clip's first frame the pivot is as stored.
        let start = motion.pose(0.0);
        assert_eq!(start.len(), 1);
        assert!(start[0].1.abs_diff_eq(Mat4::IDENTITY, 1e-6));
        // A quarter turn about Y at the pivot takes the baked point
        // (3, 0, 0) to (2, 0, -1).
        let end = motion.pose(2.0);
        let moved = end[0].1.transform_point3(Vec3::new(3.0, 0.0, 0.0));
        assert!(moved.abs_diff_eq(Vec3::new(2.0, 0.0, -1.0), 1e-5), "{moved}");
        let half = motion.pose(1.0)[0].1.transform_point3(Vec3::new(3.0, 0.0, 0.0));
        let s = std::f32::consts::FRAC_1_SQRT_2;
        assert!(half.abs_diff_eq(Vec3::new(2.0 + s, 0.0, -s), 1e-5), "{half}");
    }

    #[test]
    fn a_part_the_map_does_not_hold_stays_unbound() {
        let mut motion = swinging(Rig::default());
        assert_eq!(motion.bind(&[place("door", [9.0, 0.0, 0.0])]), 0);
        assert_eq!(motion.parts[0].object, None);
        // Two parts never take the same object.
        let mut two = swinging(Rig::default());
        two.parts.push(two.parts[0].clone());
        assert_eq!(two.bind(&[place("door", [3.0, 0.0, 0.0])]), 1);
        assert_eq!((two.parts[0].object, two.parts[1].object), (Some(0), None));
    }

    #[test]
    fn a_later_layer_replaces_or_adds() {
        let rest = [Trs {
            rot: euler_deg(0.0, 10.0, 0.0),
            ..Default::default()
        }];
        let over = Layer {
            weight: 1.0,
            ..layer(vec![hold(90.0)])
        };
        let mut locals = rest;
        Rig {
            layers: vec![layer(vec![empty()]), over],
            ..Default::default()
        }
        .apply(0.0, &rest, &mut locals);
        assert!(locals[0].rot.abs_diff_eq(euler_deg(0.0, 90.0, 0.0), 1e-5));
        // The stage's centre spots: the turn since the clip's first frame
        // goes on top of where the object stands.
        let mut spin = turn(120.0, 4.0, true);
        spin.cycle_offset = 0.0;
        let added = Layer {
            additive: true,
            weight: 1.0,
            ..layer(vec![spin])
        };
        let mut locals = rest;
        Rig {
            layers: vec![layer(vec![empty()]), added],
            ..Default::default()
        }
        .apply(1.0, &rest, &mut locals);
        assert!(locals[0].rot.abs_diff_eq(euler_deg(0.0, 40.0, 0.0), 1e-5));
    }

    #[test]
    fn an_animator_below_another_writes_after_it() {
        let mut motion = swinging(Rig {
            layers: vec![layer(vec![hold(90.0)])],
            ..Default::default()
        });
        motion.rigs.push(Rig {
            layers: vec![layer(vec![hold(0.0)])],
            ..Default::default()
        });
        motion.bind(&[place("door", [3.0, 0.0, 0.0])]);
        assert!(motion.pose(0.0)[0].1.abs_diff_eq(Mat4::IDENTITY, 1e-6));
    }
}
