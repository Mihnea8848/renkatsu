// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The scene model: what a scene holds, as plain data. Objects carry a
//! stable id, a name, the folder they sit in, their visibility, their
//! transform and what they are. Nothing here knows about the GPU or about
//! built characters; `live.rs` follows this model each frame.
//!
//! The model is serialisable as it stands, as a flat list of objects in
//! tree order, each naming its parent by id, so a scene file is readable
//! text and a change to one object touches a few lines.
//!
//! `Document` wraps a scene with the selection and the undo history. Undo
//! keeps whole snapshots: a scene is a short list of small values, so a
//! snapshot costs little and every operation is undone the same way.

use crate::kinds::Kind;
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Names one object for as long as the scene lives. Ids are never reused,
/// also not after an undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(pub u64);

/// Position, rotation as Unity's euler angles in degrees, and scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
        }
    }
}

impl Transform {
    /// The rotation as a quaternion: Unity applies Z, then X, then Y.
    pub fn quat(&self) -> Quat {
        let [x, y, z] = self.rotation;
        Quat::from_euler(glam::EulerRot::YXZ, y.to_radians(), x.to_radians(), z.to_radians())
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(Vec3::from(self.scale), self.quat(), Vec3::from(self.position))
    }

    /// The largest of the three scales, for sizes that must stay one number.
    pub fn largest_scale(&self) -> f32 {
        self.scale.iter().fold(0.0f32, |a, s| a.max(s.abs()))
    }
}

/// What an object is, with the values only that kind has.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Content {
    Folder,
    Character {
        /// The card the character is built from.
        card: PathBuf,
        /// Which of the card's outfits is worn.
        outfit: usize,
        outdoor_shoes: bool,
        /// Whether hair, skirts and the like move while the physics plays.
        simulate: bool,
    },
    Light {
        /// The light stays at the camera's upper left, as in the game's
        /// maker, instead of shining along its own rotation.
        from_camera: bool,
        intensity: f32,
        shadows: bool,
    },
    Camera {
        focal_length: f32,
        near: f32,
        far: f32,
    },
    Item,
    Map,
}

impl Content {
    pub fn character(card: PathBuf) -> Content {
        Content::Character {
            card,
            outfit: 0,
            outdoor_shoes: false,
            simulate: true,
        }
    }

    pub fn light() -> Content {
        Content::Light {
            from_camera: true,
            intensity: 1.0,
            shadows: true,
        }
    }

    pub fn camera() -> Content {
        Content::Camera {
            focal_length: 35.0,
            near: 0.05,
            far: 100.0,
        }
    }

    pub fn kind(&self) -> Kind {
        match self {
            Content::Folder => Kind::Folder,
            Content::Character { .. } => Kind::Character,
            Content::Light { .. } => Kind::Light,
            Content::Camera { .. } => Kind::Camera,
            Content::Item => Kind::Item,
            Content::Map => Kind::Map,
        }
    }

    /// What a new object of a kind starts as, for the kinds that need
    /// nothing else to exist. A character needs a card.
    pub fn of_kind(kind: Kind) -> Option<Content> {
        match kind {
            Kind::Folder => Some(Content::Folder),
            Kind::Light => Some(Content::light()),
            Kind::Camera => Some(Content::camera()),
            Kind::Item => Some(Content::Item),
            Kind::Map => Some(Content::Map),
            _ => None,
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub id: ObjectId,
    pub name: String,
    /// The folder the object sits in; none at the top of the scene.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ObjectId>,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(flatten)]
    pub content: Content,
    #[serde(default)]
    pub transform: Transform,
}

impl Object {
    pub fn kind(&self) -> Kind {
        self.content.kind()
    }

    /// Unit vector from the scene towards a light with this transform: a
    /// light shines along its own forward axis.
    pub fn light_direction(&self) -> [f32; 3] {
        (-(self.transform.quat() * Vec3::Z)).to_array()
    }
}

/// The rotation a new light gets, which puts it at the upper left of a
/// character seen from the front.
const LIGHT_ROTATION: [f32; 3] = [20.0, 158.0, 0.0];

/// Where an object goes when it is moved in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Last inside a folder, or last at the top of the scene.
    Into(Option<ObjectId>),
    /// Beside another object, in the same folder, before it.
    Before(ObjectId),
    /// Beside another object, in the same folder, after it.
    After(ObjectId),
}

/// A scene: its name and its objects. Siblings keep the order they have
/// in the list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
    next_id: u64,
    #[serde(default)]
    objects: Vec<Object>,
}

impl Scene {
    pub fn empty() -> Scene {
        Scene {
            name: "Untitled scene".to_owned(),
            next_id: 1,
            objects: Vec::new(),
        }
    }

    /// What a new scene starts with: the key light.
    pub fn new() -> Scene {
        let mut scene = Scene::empty();
        scene.add("Key light", Content::light(), None);
        scene
    }

    pub fn objects(&self) -> &[Object] {
        &self.objects
    }

    pub fn get(&self, id: ObjectId) -> Option<&Object> {
        self.objects.iter().find(|o| o.id == id)
    }

    pub fn get_mut(&mut self, id: ObjectId) -> Option<&mut Object> {
        self.objects.iter_mut().find(|o| o.id == id)
    }

    fn index(&self, id: ObjectId) -> Option<usize> {
        self.objects.iter().position(|o| o.id == id)
    }

    /// The objects directly inside a folder, or at the top of the scene.
    pub fn children(&self, parent: Option<ObjectId>) -> Vec<ObjectId> {
        self.objects
            .iter()
            .filter(|o| o.parent == parent)
            .map(|o| o.id)
            .collect()
    }

    pub fn has_children(&self, id: ObjectId) -> bool {
        self.objects.iter().any(|o| o.parent == Some(id))
    }

    /// Every object in tree order with its depth, the top of the scene
    /// being depth 0.
    pub fn rows(&self) -> Vec<(usize, ObjectId)> {
        let mut rows = Vec::with_capacity(self.objects.len());
        self.walk(None, 0, &mut rows);
        rows
    }

    fn walk(&self, parent: Option<ObjectId>, depth: usize, rows: &mut Vec<(usize, ObjectId)>) {
        for id in self.children(parent) {
            rows.push((depth, id));
            self.walk(Some(id), depth + 1, rows);
        }
    }

    /// An object and everything below it, in tree order.
    pub fn subtree(&self, id: ObjectId) -> Vec<ObjectId> {
        let mut rows = vec![(0, id)];
        self.walk(Some(id), 1, &mut rows);
        rows.into_iter().map(|(_, id)| id).collect()
    }

    /// Whether `id` is `of` itself or one of the folders around it.
    pub fn contains(&self, id: ObjectId, of: ObjectId) -> bool {
        let mut at = Some(of);
        while let Some(o) = at {
            if o == id {
                return true;
            }
            at = self.get(o).and_then(|o| o.parent);
        }
        false
    }

    /// Whether an object shows: it and every folder around it are visible.
    pub fn shown(&self, id: ObjectId) -> bool {
        let mut at = self.get(id);
        while let Some(o) = at {
            if !o.visible {
                return false;
            }
            at = o.parent.and_then(|p| self.get(p));
        }
        true
    }

    /// The light that lights the scene: the first one that shows, in tree
    /// order. The renderer has one light.
    pub fn key_light(&self) -> Option<&Object> {
        self.rows()
            .into_iter()
            .filter_map(|(_, id)| self.get(id))
            .find(|o| o.kind() == Kind::Light && self.shown(o.id))
    }

    /// A name no other object has: `base`, or `base` with the first free
    /// number after it.
    pub fn free_name(&self, base: &str) -> String {
        let taken = |name: &str| self.objects.iter().any(|o| o.name == name);
        if !taken(base) {
            return base.to_owned();
        }
        // "Chair 2" copies to "Chair 3", not to "Chair 2 2".
        let stem = match base.rsplit_once(' ') {
            Some((stem, number)) if number.parse::<u32>().is_ok() => stem,
            _ => base,
        };
        (2..)
            .map(|n| format!("{stem} {n}"))
            .find(|name| !taken(name))
            .expect("some number is free")
    }

    fn fresh_id(&mut self) -> ObjectId {
        let id = ObjectId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Adds an object last inside `parent`, which must be a folder, or at
    /// the top of the scene. The name is made unique.
    pub fn add(&mut self, name: &str, content: Content, parent: Option<ObjectId>) -> ObjectId {
        let parent = parent.filter(|p| self.get(*p).is_some_and(|o| o.kind() == Kind::Folder));
        let id = self.fresh_id();
        let mut transform = Transform::default();
        if content.kind() == Kind::Light {
            transform.rotation = LIGHT_ROTATION;
        }
        self.objects.push(Object {
            id,
            name: self.free_name(name),
            parent,
            visible: true,
            content,
            transform,
        });
        id
    }

    /// Removes an object and everything below it. Returns what was removed.
    pub fn delete(&mut self, id: ObjectId) -> Vec<ObjectId> {
        if self.get(id).is_none() {
            return Vec::new();
        }
        let gone = self.subtree(id);
        self.objects.retain(|o| !gone.contains(&o.id));
        gone
    }

    /// Copies an object and everything below it, placing the copy right
    /// after the original. Returns the copy's id.
    pub fn duplicate(&mut self, id: ObjectId) -> Option<ObjectId> {
        let source = self.subtree(id);
        self.get(id)?;
        let mut copies: Vec<Object> = Vec::with_capacity(source.len());
        let mut renamed: Vec<(ObjectId, ObjectId)> = Vec::with_capacity(source.len());
        for old in &source {
            let mut copy = self.get(*old)?.clone();
            copy.id = self.fresh_id();
            renamed.push((*old, copy.id));
            if *old != id {
                // Inside the copy, parents are the copied folders.
                copy.parent = copy
                    .parent
                    .and_then(|p| renamed.iter().find(|(from, _)| *from == p))
                    .map(|(_, to)| *to);
            }
            copies.push(copy);
        }
        for mut copy in copies {
            copy.name = self.free_name(&copy.name);
            self.objects.push(copy);
        }
        let new = renamed[0].1;
        self.place(new, Place::After(id));
        Some(new)
    }

    /// Whether an object may be moved to a place: the target must exist,
    /// only folders hold objects, and a folder cannot go inside itself.
    pub fn can_place(&self, id: ObjectId, place: Place) -> bool {
        if self.get(id).is_none() {
            return false;
        }
        let parent = match place {
            Place::Into(parent) => parent,
            Place::Before(other) | Place::After(other) => {
                if other == id {
                    return false;
                }
                match self.get(other) {
                    Some(o) => o.parent,
                    None => return false,
                }
            }
        };
        match parent {
            None => true,
            Some(p) => self.get(p).is_some_and(|o| o.kind() == Kind::Folder) && !self.contains(id, p),
        }
    }

    /// Moves an object, with everything below it, to a place in the tree.
    /// Returns whether it moved.
    pub fn place(&mut self, id: ObjectId, place: Place) -> bool {
        if !self.can_place(id, place) {
            return false;
        }
        let (parent, before) = match place {
            Place::Into(parent) => (parent, None),
            Place::Before(other) => (self.get(other).and_then(|o| o.parent), Some(other)),
            Place::After(other) => {
                let parent = self.get(other).and_then(|o| o.parent);
                let siblings = self.children(parent);
                let next = siblings
                    .iter()
                    .skip_while(|s| **s != other)
                    .skip(1)
                    .find(|s| **s != id)
                    .copied();
                (parent, next)
            }
        };
        let Some(from) = self.index(id) else { return false };
        let mut object = self.objects.remove(from);
        object.parent = parent;
        let to = before.and_then(|b| self.index(b)).unwrap_or(self.objects.len());
        self.objects.insert(to, object);
        true
    }
}

/// A scene and a selection as they were at one moment.
#[derive(Debug, Clone, PartialEq)]
struct Snapshot {
    scene: Scene,
    selected: Option<ObjectId>,
}

/// How many steps back the history goes.
const HISTORY: usize = 200;

/// The scene being edited, with the selection and the undo history. Every
/// change goes through here so that it can be undone.
#[derive(Debug, Clone)]
pub struct Document {
    scene: Scene,
    pub selected: Option<ObjectId>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// A value is being dragged: its further changes belong to the step
    /// already recorded.
    gesture: bool,
}

impl Document {
    pub fn new(scene: Scene) -> Document {
        Document {
            scene,
            selected: None,
            undo: Vec::new(),
            redo: Vec::new(),
            gesture: false,
        }
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn selection(&self) -> Option<&Object> {
        self.scene.get(self.selected?)
    }

    /// Records the scene as it is, before a change.
    fn checkpoint(&mut self) {
        self.undo.push(Snapshot {
            scene: self.scene.clone(),
            selected: self.selected,
        });
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.gesture = false;
    }

    /// Forgets the history: the scene as it is becomes where undo stops.
    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.gesture = false;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn restore(&mut self, snapshot: Snapshot) -> Snapshot {
        let now = Snapshot {
            scene: std::mem::replace(&mut self.scene, snapshot.scene),
            selected: self.selected,
        };
        // Ids handed out since the snapshot stay used.
        self.scene.next_id = self.scene.next_id.max(now.scene.next_id);
        self.selected = snapshot.selected;
        self.gesture = false;
        now
    }

    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else { return false };
        let now = self.restore(snapshot);
        self.redo.push(now);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop() else { return false };
        let now = self.restore(snapshot);
        self.undo.push(now);
        true
    }

    /// The folder a new object goes into: the selected folder, or the
    /// folder the selection sits in.
    fn target_folder(&self) -> Option<ObjectId> {
        let selected = self.selection()?;
        if selected.kind() == Kind::Folder {
            Some(selected.id)
        } else {
            selected.parent
        }
    }

    /// Adds an object beside the selection and selects it.
    pub fn add(&mut self, name: &str, content: Content) -> ObjectId {
        self.checkpoint();
        let id = self.scene.add(name, content, self.target_folder());
        self.selected = Some(id);
        id
    }

    /// Adds a character built from a card, standing clear of the others,
    /// and selects it.
    pub fn add_character(&mut self, name: &str, card: PathBuf) -> ObjectId {
        let id = self.add(name, Content::character(card));
        let position = self.free_spot();
        if let Some(object) = self.scene.get_mut(id) {
            object.transform.position = position;
        }
        id
    }

    /// A place on the ground no character stands on: the origin, or the
    /// first free step to its side.
    fn free_spot(&self) -> [f32; 3] {
        const STEP: f32 = 1.0;
        let taken = |x: f32| {
            self.scene.objects.iter().any(|o| {
                let [ox, _, oz] = o.transform.position;
                o.kind() == Kind::Character && Some(o.id) != self.selected && (ox - x).abs() < 0.3 && oz.abs() < 0.3
            })
        };
        // 0, one step right, one step left, two steps right, and so on.
        (0..)
            .map(|n: i32| match n {
                0 => 0.0,
                n if n % 2 == 1 => ((n + 1) / 2) as f32 * STEP,
                n => -(n / 2) as f32 * STEP,
            })
            .find(|x| !taken(*x))
            .map_or([0.0; 3], |x| [x, 0.0, 0.0])
    }

    /// Deletes an object with everything below it. The selection moves to
    /// the next sibling, else the previous one, else the folder around it.
    pub fn delete(&mut self, id: ObjectId) -> bool {
        let Some(object) = self.scene.get(id) else { return false };
        let parent = object.parent;
        let siblings = self.scene.children(parent);
        let at = siblings.iter().position(|s| *s == id).unwrap_or(0);
        let neighbour = siblings
            .get(at + 1)
            .or_else(|| at.checked_sub(1).and_then(|i| siblings.get(i)))
            .copied()
            .or(parent);
        self.checkpoint();
        let gone = self.scene.delete(id);
        if self.selected.is_some_and(|s| gone.contains(&s)) {
            self.selected = neighbour;
        }
        true
    }

    /// Copies an object with everything below it and selects the copy.
    pub fn duplicate(&mut self, id: ObjectId) -> Option<ObjectId> {
        self.scene.get(id)?;
        self.checkpoint();
        let copy = self.scene.duplicate(id)?;
        self.selected = Some(copy);
        if self.scene.get(copy).is_some_and(|o| o.kind() == Kind::Character) {
            let position = self.free_spot();
            if let Some(object) = self.scene.get_mut(copy) {
                object.transform.position = position;
            }
        }
        Some(copy)
    }

    /// Moves an object in the tree.
    pub fn place(&mut self, id: ObjectId, place: Place) -> bool {
        if !self.scene.can_place(id, place) {
            return false;
        }
        self.checkpoint();
        self.scene.place(id, place)
    }

    /// Renames an object. An empty name is refused.
    pub fn rename(&mut self, id: ObjectId, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || self.scene.get(id).is_none_or(|o| o.name == name) {
            return false;
        }
        self.checkpoint();
        if let Some(object) = self.scene.get_mut(id) {
            object.name = name.to_owned();
        }
        true
    }

    /// Shows or hides an object.
    pub fn set_visible(&mut self, id: ObjectId, visible: bool) -> bool {
        if self.scene.get(id).is_none_or(|o| o.visible == visible) {
            return false;
        }
        self.checkpoint();
        if let Some(object) = self.scene.get_mut(id) {
            object.visible = visible;
        }
        true
    }

    /// Replaces an object's values with edited ones. `held` says the
    /// pointer is down, as while a number is dragged: the changes of one
    /// drag make one step of the history.
    pub fn edit(&mut self, edited: Object, held: bool) -> bool {
        let Some(at) = self.scene.index(edited.id) else {
            return false;
        };
        if self.scene.objects[at] == edited {
            return false;
        }
        if !self.gesture {
            self.checkpoint();
        }
        self.gesture = held;
        // The tree is not edited this way.
        let parent = self.scene.objects[at].parent;
        self.scene.objects[at] = Object { parent, ..edited };
        true
    }

    /// The pointer was let go: the next edit starts a new step.
    pub fn end_gesture(&mut self) {
        self.gesture = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(name: &str) -> PathBuf {
        PathBuf::from(format!("/cards/{name}.png"))
    }

    fn names(scene: &Scene) -> Vec<(usize, &str)> {
        scene
            .rows()
            .into_iter()
            .map(|(depth, id)| (depth, scene.get(id).unwrap().name.as_str()))
            .collect()
    }

    /// Cast (folder) holding Anna and Bea, then a light and a camera.
    fn cast() -> (Document, [ObjectId; 5]) {
        let mut doc = Document::new(Scene::empty());
        let folder = doc.add("Cast", Content::Folder);
        let anna = doc.add_character("Anna", card("anna"));
        doc.selected = Some(folder);
        let bea = doc.add_character("Bea", card("bea"));
        doc.selected = None;
        let light = doc.add("Key light", Content::light());
        doc.selected = None;
        let camera = doc.add("Camera", Content::camera());
        (doc, [folder, anna, bea, light, camera])
    }

    #[test]
    fn objects_get_ids_that_are_never_reused() {
        let (mut doc, ids) = cast();
        let mut sorted = ids.to_vec();
        sorted.dedup();
        assert_eq!(sorted.len(), 5, "ids differ");
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "ids grow");

        // An id handed out before an undo is not handed out again.
        doc.selected = None;
        let chair = doc.add("Chair", Content::Item);
        assert!(doc.undo());
        assert!(doc.scene().get(chair).is_none());
        let desk = doc.add("Desk", Content::Item);
        assert_ne!(desk, chair);
        assert!(desk > chair);
    }

    #[test]
    fn adding_goes_into_the_selected_folder_and_selects() {
        let (doc, [folder, anna, bea, light, camera]) = cast();
        assert_eq!(
            names(doc.scene()),
            [(0, "Cast"), (1, "Anna"), (1, "Bea"), (0, "Key light"), (0, "Camera")]
        );
        assert_eq!(doc.scene().get(anna).unwrap().parent, Some(folder));
        assert_eq!(doc.scene().get(bea).unwrap().parent, Some(folder));
        assert_eq!(doc.scene().get(light).unwrap().parent, None);
        assert_eq!(doc.selected, Some(camera));
        assert_eq!(doc.scene().children(Some(folder)), [anna, bea]);
        assert!(doc.scene().has_children(folder));
        assert!(!doc.scene().has_children(anna));
    }

    #[test]
    fn only_folders_hold_objects() {
        let mut scene = Scene::empty();
        let light = scene.add("Light", Content::light(), None);
        let item = scene.add("Chair", Content::Item, Some(light));
        assert_eq!(scene.get(item).unwrap().parent, None);
        assert!(!scene.can_place(item, Place::Into(Some(light))));
        assert!(!scene.place(item, Place::Into(Some(ObjectId(99)))));
    }

    #[test]
    fn names_stay_unique() {
        let mut scene = Scene::empty();
        scene.add("Chair", Content::Item, None);
        let second = scene.add("Chair", Content::Item, None);
        assert_eq!(scene.get(second).unwrap().name, "Chair 2");
        let third = scene.duplicate(second).unwrap();
        assert_eq!(scene.get(third).unwrap().name, "Chair 3");
    }

    #[test]
    fn new_characters_stand_clear_of_each_other() {
        let (mut doc, [_, anna, bea, ..]) = cast();
        let x = |doc: &Document, id| doc.scene().get(id).unwrap().transform.position[0];
        assert_eq!(x(&doc, anna), 0.0);
        assert!(x(&doc, bea).abs() > 0.5);
        let cleo = doc.add_character("Cleo", card("cleo"));
        assert!((x(&doc, cleo) - x(&doc, bea)).abs() > 0.5);
        assert!(x(&doc, cleo).abs() > 0.5);
    }

    #[test]
    fn deleting_a_folder_takes_what_it_holds() {
        let (mut doc, [folder, anna, bea, light, _]) = cast();
        doc.selected = Some(anna);
        assert!(doc.delete(folder));
        assert!(doc.scene().get(anna).is_none());
        assert!(doc.scene().get(bea).is_none());
        assert_eq!(names(doc.scene()), [(0, "Key light"), (0, "Camera")]);
        // The selection was inside: it moves to the folder's next sibling.
        assert_eq!(doc.selected, Some(light));
        assert!(!doc.delete(folder), "already gone");
    }

    #[test]
    fn the_selection_after_a_delete_is_a_neighbour() {
        let (mut doc, [folder, anna, bea, light, camera]) = cast();
        doc.selected = Some(anna);
        doc.delete(anna);
        assert_eq!(doc.selected, Some(bea), "the next sibling");
        doc.delete(bea);
        assert_eq!(doc.selected, Some(folder), "the folder, once it is empty");
        doc.selected = Some(camera);
        doc.delete(camera);
        assert_eq!(doc.selected, Some(light), "the previous sibling at the end");
        // Deleting something else leaves the selection alone.
        doc.delete(folder);
        assert_eq!(doc.selected, Some(light));
        doc.delete(light);
        assert_eq!(doc.selected, None);
        assert!(doc.scene().objects().is_empty());
    }

    #[test]
    fn objects_move_between_folders_and_places() {
        let (mut doc, [folder, anna, bea, light, camera]) = cast();
        assert!(doc.place(bea, Place::Into(None)));
        assert_eq!(
            names(doc.scene()),
            [(0, "Cast"), (1, "Anna"), (0, "Key light"), (0, "Camera"), (0, "Bea")]
        );
        assert!(doc.place(bea, Place::Before(light)));
        assert_eq!(doc.scene().children(None), [folder, bea, light, camera]);
        assert!(doc.place(camera, Place::After(anna)));
        assert_eq!(doc.scene().children(Some(folder)), [anna, camera]);
        assert!(doc.place(camera, Place::Before(anna)));
        assert_eq!(doc.scene().children(Some(folder)), [camera, anna]);
        assert!(doc.place(light, Place::After(folder)));
        assert_eq!(doc.scene().children(None), [folder, light, bea]);
        // After the last sibling.
        assert!(doc.place(folder, Place::After(bea)));
        assert_eq!(doc.scene().children(None), [light, bea, folder]);
        assert_eq!(
            doc.scene().children(Some(folder)),
            [camera, anna],
            "the folder kept its content"
        );
    }

    #[test]
    fn a_folder_cannot_go_inside_itself() {
        let (mut doc, [folder, anna, ..]) = cast();
        doc.selected = Some(folder);
        let inner = doc.add("Inner", Content::Folder);
        assert_eq!(doc.scene().get(inner).unwrap().parent, Some(folder));
        let before = doc.scene().clone();
        assert!(!doc.place(folder, Place::Into(Some(inner))));
        assert!(!doc.place(folder, Place::Into(Some(folder))));
        assert!(!doc.place(folder, Place::Before(anna)));
        assert!(!doc.place(anna, Place::Before(anna)));
        assert_eq!(doc.scene(), &before);
        assert!(doc.scene().contains(folder, inner));
        assert!(!doc.scene().contains(inner, folder));
    }

    #[test]
    fn a_copy_gets_new_ids_and_sits_after_the_original() {
        let (mut doc, [folder, anna, bea, ..]) = cast();
        let copy = doc.duplicate(folder).unwrap();
        assert_eq!(doc.selected, Some(copy));
        assert_eq!(
            names(doc.scene()),
            [
                (0, "Cast"),
                (1, "Anna"),
                (1, "Bea"),
                (0, "Cast 2"),
                (1, "Anna 2"),
                (1, "Bea 2"),
                (0, "Key light"),
                (0, "Camera"),
            ]
        );
        let inside = doc.scene().children(Some(copy));
        assert_eq!(inside.len(), 2);
        assert!(inside.iter().all(|id| *id != anna && *id != bea));
        let content = |id| doc.scene().get(id).unwrap().content.clone();
        assert_eq!(content(inside[0]), content(anna));
        assert!(doc.duplicate(ObjectId(99)).is_none());
    }

    #[test]
    fn every_operation_is_undone_and_redone() {
        let (mut doc, [folder, anna, bea, light, _]) = cast();
        let start = doc.scene().clone();
        doc.selected = Some(anna);

        doc.delete(folder);
        doc.rename(light, "Sun");
        doc.set_visible(light, false);
        let chair = doc.add("Chair", Content::Item);
        doc.duplicate(chair);
        doc.place(chair, Place::Into(None));
        let end = doc.scene().clone();
        assert_ne!(end, start);

        let mut steps = 0;
        while doc.scene().objects() != start.objects() {
            assert!(doc.undo(), "ran out of history before the start");
            steps += 1;
        }
        assert_eq!(steps, 6);
        assert_eq!(doc.selected, Some(anna), "the selection comes back too");
        assert!(doc.scene().get(bea).is_some());
        assert!(doc.can_redo());
        for _ in 0..steps {
            assert!(doc.redo());
        }
        assert!(!doc.redo());
        assert_eq!(doc.scene().objects(), end.objects());

        // A new change drops what could be redone.
        doc.undo();
        doc.rename(light, "Moon");
        assert!(!doc.can_redo());
    }

    #[test]
    fn changes_that_change_nothing_leave_no_step() {
        let (mut doc, [_, anna, _, light, _]) = cast();
        let steps = |doc: &Document| doc.undo.len();
        let before = steps(&doc);
        assert!(!doc.rename(anna, "Anna"));
        assert!(!doc.rename(anna, "   "));
        assert!(!doc.set_visible(light, true));
        assert!(!doc.edit(doc.scene().get(anna).unwrap().clone(), false));
        assert!(!doc.delete(ObjectId(99)));
        assert_eq!(steps(&doc), before);
    }

    #[test]
    fn one_drag_is_one_step() {
        let (mut doc, [_, anna, ..]) = cast();
        let start = doc.scene().clone();
        for x in [0.1, 0.2, 0.3] {
            let mut edited = doc.scene().get(anna).unwrap().clone();
            edited.transform.position[0] = x;
            assert!(doc.edit(edited, true));
        }
        doc.end_gesture();
        let mut edited = doc.scene().get(anna).unwrap().clone();
        edited.transform.rotation[1] = 90.0;
        assert!(doc.edit(edited, false));

        assert!(doc.undo());
        let anna_now = doc.scene().get(anna).unwrap();
        assert_eq!(anna_now.transform.position[0], 0.3);
        assert_eq!(anna_now.transform.rotation[1], 0.0);
        assert!(doc.undo());
        assert_eq!(doc.scene().objects(), start.objects());
    }

    #[test]
    fn hiding_a_folder_hides_what_it_holds() {
        let (mut doc, [folder, anna, _, light, _]) = cast();
        assert!(doc.scene().shown(anna));
        doc.set_visible(folder, false);
        assert!(!doc.scene().shown(anna));
        assert!(doc.scene().get(anna).unwrap().visible, "its own switch is untouched");
        assert!(doc.scene().shown(light));
    }

    #[test]
    fn the_key_light_is_the_first_light_that_shows() {
        let (mut doc, [.., light, _]) = cast();
        doc.selected = None;
        let second = doc.add("Fill", Content::light());
        assert_eq!(doc.scene().key_light().map(|o| o.id), Some(light));
        doc.set_visible(light, false);
        assert_eq!(doc.scene().key_light().map(|o| o.id), Some(second));
        doc.set_visible(second, false);
        assert!(doc.scene().key_light().is_none());
    }

    #[test]
    fn a_light_shines_along_its_rotation() {
        let mut scene = Scene::empty();
        let id = scene.add("Light", Content::light(), None);
        let light = scene.get_mut(id).unwrap();
        // Unturned, it shines along +Z, so it stands towards -Z.
        light.transform.rotation = [0.0; 3];
        let d = light.light_direction();
        assert!((Vec3::from(d) - Vec3::NEG_Z).length() < 1e-5);
        // Tilted down by 90 degrees it shines down, so it stands above.
        light.transform.rotation = [90.0, 0.0, 0.0];
        let d = light.light_direction();
        assert!((Vec3::from(d) - Vec3::Y).length() < 1e-5, "{d:?}");
        // A new light stands in front of the character, up and to a side.
        let fresh = Scene::new();
        let d = fresh.key_light().unwrap().light_direction();
        assert!(d[2] > 0.5 && d[1] > 0.2, "{d:?}");
    }

    #[test]
    fn a_transform_matches_unity() {
        let t = Transform {
            position: [1.0, 2.0, 3.0],
            rotation: [0.0, 90.0, 0.0],
            scale: [2.0, 2.0, 2.0],
        };
        // Turned 90 degrees around Y, forward (+Z) points along +X.
        let p = t.matrix().transform_point3(Vec3::Z);
        assert!((p - Vec3::new(3.0, 2.0, 3.0)).length() < 1e-5, "{p:?}");
        assert_eq!(t.largest_scale(), 2.0);
    }

    #[test]
    fn a_scene_is_readable_text_and_reads_back() {
        let (mut doc, [_, anna, ..]) = cast();
        let mut edited = doc.scene().get(anna).unwrap().clone();
        edited.transform.rotation = [0.0, 45.0, 0.0];
        edited.visible = false;
        doc.edit(edited, false);
        let text = toml::to_string(doc.scene()).unwrap();
        assert!(text.contains("name = \"Anna\""), "{text}");
        assert!(text.contains("kind = \"character\""), "{text}");
        assert!(text.contains("card = \"/cards/anna.png\""), "{text}");
        let back: Scene = toml::from_str(&text).unwrap();
        assert_eq!(&back, doc.scene());
    }
}
