// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Character assembly, following the order the game's loader uses.

use crate::compose::{self, ClothColor, Decal};
use crate::expression;
use crate::eyelook::{self, EyeLook};
use crate::foreground;
use crate::loader::{assign_weights, Loader};
use crate::overlays::{self, kind, ClothOverlay};
use crate::scene::{euler_deg, Image, Instance, Part, Scene, TexSlot};
use crate::shape::{self, Rules, ShapeSet};
use anyhow::{Context, Result};
use glam::Vec3;
use rk_card::Card;
use rk_vfs::uncensor::ColorMatch;
use rk_vfs::{cat, resolve_infos, Game, Item, Lookup, ResolveInfo};
use std::collections::HashMap;
use std::sync::Arc;

const BASE: &str = "chara/oo_base.unity3d";

#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Which of the card's outfits to dress the character in.
    pub outfit: usize,
    pub clothes: bool,
    pub accessories: bool,
    /// Wear the outdoor shoes instead of the indoor ones. The game loads
    /// cards without their saved status, so it starts with indoor shoes.
    pub outdoor_shoes: bool,
    /// Draw the game's censor over the body parts it covers, the meshes
    /// whose material is `Shader Forge/mnpb`. Without it those parts take
    /// the body's own skin, so a character is uncensored without a mod
    /// for it. A part an uncensor mod swapped is neither: it keeps the
    /// material that came with it.
    pub censor: bool,
    /// CharaStudio's Son switch for a male character, which decides
    /// whether `n_dankon` is drawn at all; the clothes states then say
    /// when. `None` keeps what the card's status says.
    pub visible_son: Option<bool>,
    /// The uncensor body to wear, by its GUID, in place of the one the
    /// card names or the plugin picks.
    pub uncensor_body: Option<String>,
    /// Let hair, skirts, ribbons and the bust settle as the game's
    /// DynamicBone physics does.
    pub physics: bool,
    /// Frames of physics to run before the pose is taken.
    pub settle_frames: u32,
    /// Seconds per settle frame.
    pub frame_time: f32,
    /// Pose the character as Studio does instead of the game proper:
    /// Studio's pose targets overwrite the hand size correction. On by
    /// default, since Renkatsu stands in for CharaStudio.
    pub studio: bool,
    /// Run the Accessory Clothes plugin: accessories made for it are bound
    /// to the body skeleton like clothes.
    pub accessory_clothes: bool,
    /// Run the Invisible Body plugin: a card can hide its body.
    pub invisible_body: bool,
    /// Run the Clothing Unlocker plugin: an unlocked outfit keeps the
    /// bottom, bra and shorts that its top or bra would remove.
    pub clothing_unlocker: bool,
    /// Run the Eye Control plugin: the card limits how far the eyes open.
    pub eye_control: bool,
    /// Run Slider Unlocker: highlight positions follow slider values set
    /// outside 0 to 1 instead of stopping at the ends.
    pub slider_unlocker: bool,
    /// Run Edit Fangs: the card sets the length and spacing of each fang.
    pub edit_fangs: bool,
    /// Run Clothing BlendShape: clothing made for it takes the blend shape
    /// weights the card saved per outfit.
    pub clothing_blend_shape: bool,
    /// Run Copy Cha Body Mat: items made for it draw parts with the body's
    /// own material, so they match the skin.
    pub copy_body_material: bool,
    /// Run the Dynamic Bone Distribution Editor on accessories.
    pub dbde: bool,
    /// Run the Pregnancy plugin: the body grows with the stored week.
    pub pregnancy: bool,
    /// Run the Accessory States plugin: accessories bound to clothes
    /// states or groups are hidden outside their states.
    pub accessory_states: bool,
    /// Run the Uncensor Selector plugin: the body mesh and its base
    /// textures come from the body the card names, or from a seeded pick
    /// among the installed ones.
    pub uncensor_selector: bool,
    /// Run the Colliders plugin: floor, arm and leg colliders where the
    /// card switches them on, and its changes to skirts and the bust.
    pub colliders: bool,
    /// The Colliders plugin's three switches as Studio's character panel
    /// sets them; `None` takes the ones the card was saved with.
    pub collider_switches: Option<crate::colliders::Settings>,
    /// Advanced Skirt Physics: Renkatsu's own pass that keeps legs, arms
    /// and the floor out of skirts. Off moves them exactly as the game.
    pub advanced_skirts: bool,
    /// Run the Stiletto plugin: a worn shoe with a heel file lifts the body
    /// and turns the lower legs, feet and toes.
    pub stiletto: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            outfit: 0,
            clothes: true,
            accessories: true,
            outdoor_shoes: false,
            censor: false,
            visible_son: None,
            uncensor_body: None,
            physics: true,
            settle_frames: crate::physics::SETTLE_FRAMES,
            frame_time: crate::physics::FRAME_TIME,
            studio: true,
            accessory_clothes: true,
            invisible_body: true,
            clothing_unlocker: true,
            eye_control: true,
            slider_unlocker: true,
            edit_fangs: true,
            clothing_blend_shape: true,
            copy_body_material: true,
            dbde: true,
            pregnancy: true,
            accessory_states: true,
            uncensor_selector: true,
            colliders: true,
            collider_switches: None,
            advanced_skirts: true,
            stiletto: true,
        }
    }
}

pub struct Character {
    pub scene: Scene,
    /// Root node everything hangs from.
    pub top: usize,
    /// The object of the body's Animator, which animations name bones
    /// from (`ChaControl.animBody`).
    pub animator: usize,
    /// What animations are told about the body.
    pub parameters: crate::motion::Parameters,
    /// The pieces of clothing and what their states switch.
    pub clothes: crate::clothes_state::Rig,
    /// The heels Stiletto put on, when the worn shoes have some.
    pub heels: Option<crate::stiletto::Heels>,
    /// The body's FullBodyBipedIK as the prefab stores it.
    pub ik: Option<crate::fbbik::Settings>,
    /// Its solver, set up in the pose the character was built in.
    pub ik_rig: Option<crate::ik::Rig>,
    /// The hand patterns loaded for the layer, left then right; one is
    /// played while the layer asks for its pattern.
    pub hands: [Option<crate::hand_pattern::HandClip>; 2],
    /// The face's controllers, to show another face.
    pub face: Option<crate::face::Rig>,
    /// The body's and the face's skin textures before the skin effects,
    /// with the renderers wearing them, and the levels shown.
    skin_base: [Option<(Vec<usize>, Arc<Image>)>; 2],
    skin_shown: crate::skin_effects::Levels,
    /// Skeleton bones by name.
    pub bones: HashMap<String, usize>,
    /// The scene's shading ramp: light amount to brightness.
    pub ramp: Option<std::sync::Arc<crate::scene::Image>>,
    /// How many MaterialEditor overrides found their material.
    pub material_edits: crate::material_editor::Applied,
    /// The DynamicBone components, already settled when the build ran them.
    /// `Physics::step` moves them on by one frame.
    pub physics: crate::physics::Physics,
    pub warnings: Vec<String>,
}

struct Builder<'g> {
    loader: Loader<'g>,
    game: &'g Game,
    infos: HashMap<String, ResolveInfo>,
    scene: Scene,
    skin_overlays: HashMap<i64, Arc<Image>>,
    cloth_overlays: HashMap<String, ClothOverlay>,
    /// The gloss mask chosen in the card, shared by all hair parts.
    hair_gloss: Option<TexSlot>,
    /// Scripts of every loaded prefab, for the physics step.
    physics_sources: Vec<PhysicsSource>,
    /// The outfit is unlocked by the Clothing Unlocker plugin.
    unlocked: bool,
    /// The face controllers, once the head is loaded.
    face: Option<crate::face::Rig>,
    /// The body's and the face's finished skin textures and the renderers
    /// that wear them, for the skin effects to be laid over.
    skin_base: [Option<(Vec<usize>, Arc<Image>)>; 2],
    /// Renderers that a `ChaCopyBodyMaterial` component gives the body's
    /// material, with the queue it asks for (-1 keeps the body's).
    body_copies: Vec<(Vec<usize>, i32)>,
}

/// The scripts of one prefab and how the game treats its DynamicBones.
struct PhysicsSource {
    scripts: Vec<(usize, rk_vfs::ObjRef)>,
    transforms: HashMap<(usize, i64), usize>,
    /// Loaded through `ChaControl.LoadCharaFbxData`: its bones collide with
    /// every collider of the body skeleton instead of their own list.
    part: bool,
    /// `copyDynamicBone`: roots are looked up by name in the body skeleton.
    copy: bool,
    /// False when the card switches the part's shaking off.
    shake: bool,
    /// Accessory slot, for the Dynamic Bone Editor's settings.
    slot: Option<usize>,
    /// What the part is, so that its kind of physics can be held still.
    belongs: crate::physics::Belongs,
    /// The prefab's top object, where paths to DynamicBone roots start.
    root: usize,
    /// The name `LoadCharaFbxData` gives the top object (`ct_clothesBot`
    /// and so on), where it renames it; plugins look for that name.
    object: Option<&'static str>,
}

impl<'g> Builder<'g> {
    /// A list column of a clothing item as the game reads it while the
    /// clothes are put on: the Clothing Unlocker plugin answers for some.
    fn list_int(&self, item: &Item, key: &str) -> Option<i32> {
        crate::clothing_unlocker::info_int(self.unlocked, key, item.int(key))
    }

    fn item(&mut self, label: &str, property: &str, category: i32, id: i32) -> Option<&'g Item> {
        match self.game.items.lookup(&self.infos, property, category, id) {
            Lookup::Found(item) => Some(item),
            Lookup::MissingMod { guid, slot } => {
                self.loader
                    .warn(format!("{label}: mod {guid} is not installed (item {slot})"));
                None
            }
            Lookup::Missing { id } => {
                self.loader
                    .warn(format!("{label}: no item {id} in category {}", cat::name(category)));
                None
            }
        }
    }

    /// Like `item`, falling back as `ChaControl.LoadCharaFbxDataAsync` does
    /// when the list has no such item: to the category's default item, then
    /// to item 0. Clothes, hair and the head load this way; accessories and
    /// textures do not (the game passes -1 or draws nothing for them).
    fn item_or_default(
        &mut self,
        label: &str,
        property: &str,
        category: i32,
        id: i32,
        default: i32,
    ) -> Option<&'g Item> {
        if let Some(item) = self.item(label, property, category, id) {
            return Some(item);
        }
        let items = &self.game.items;
        let fallback = items.vanilla(category, default).or_else(|| items.vanilla(category, 0));
        if fallback.is_some() {
            self.loader.warn(format!(
                "{label}: using the game's default item instead, as the game does"
            ));
        }
        fallback
    }

    /// Remembers a prefab's scripts for the physics step.
    fn physics_source(
        &mut self,
        inst: &Instance,
        part: bool,
        copy: bool,
        shake: bool,
        belongs: crate::physics::Belongs,
    ) {
        self.physics_sources.push(PhysicsSource {
            scripts: inst.scripts.clone(),
            transforms: inst.transform_nodes.clone(),
            part,
            copy,
            shake,
            slot: None,
            root: inst.root,
            belongs,
            object: None,
        });
    }

    /// Instantiates an item's main prefab; `None` for empty slots.
    fn part(&mut self, label: &str, item: &Item, parent: Option<usize>, part: Part) -> Option<Instance> {
        let (ab, data) = (item.get("MainAB")?, item.get("MainData")?);
        if data == "p_dummy" {
            return None;
        }
        match self
            .loader
            .load_prefab(&mut self.scene, ab, data, item.mod_idx, parent, part)
        {
            Ok(inst) => {
                self.find_body_copies(&inst);
                Some(inst)
            }
            Err(e) => {
                self.loader.warn(format!("{label}: {e:#}"));
                None
            }
        }
    }
}

/// The ramp Better Repack ships in the game's settings: a soft gradient.
/// The game's own factory setting is 1, a hard edge.
pub const DEFAULT_RAMP: i32 = 2;

fn ramp_image(loader: &mut Loader, id: i32) -> Option<Arc<Image>> {
    let item = loader.game.items.vanilla(cat::MT_RAMP, id)?;
    loader.image_by_name(item.get("MainTexAB")?, item.get("MainTex")?, None)
}

/// A shading ramp by its number in the game's list: the texture that turns
/// the amount of light on a surface into brightness. The game setting and
/// every Studio scene pick one.
pub fn ramp(game: &Game, id: i32) -> Option<Arc<Image>> {
    ramp_image(&mut Loader::new(game), id)
}

/// Rows of a Studio colour filter sheet (`Filter_<bundle>`, an `ExcelData`):
/// the filter's number, its bundle and the texture's name. The first row
/// holds the column titles, and the second column the name shown.
fn filter_rows(sheet: &rk_unityfs::UObject) -> Vec<(i32, String, String)> {
    let cells = |row: &rk_unityfs::UValue| -> Option<(i32, String, String)> {
        let cells = row.as_object()?.array("list")?;
        let text = |i: usize| cells.get(i).and_then(|c| c.as_str());
        Some((text(0)?.trim().parse().ok()?, text(2)?.to_owned(), text(3)?.to_owned()))
    };
    sheet
        .array("list")
        .unwrap_or_default()
        .iter()
        .skip(1)
        .filter_map(cells)
        .collect()
}

/// Studio's colour filters by their number in the scene
/// (`SceneInfo.aceNo`), as `Studio.Info` and `SystemButtonCtrl.SetACE`
/// find them: every bundle under `studio/info/`, in name order, may list
/// filters, and a later sheet replaces an earlier one's number. Each is
/// its bundle and texture name.
fn filter_list(game: &Game) -> std::collections::BTreeMap<i32, (String, String)> {
    let mut paths: Vec<&String> = game
        .assets
        .index
        .bundles
        .keys()
        .filter(|p| p.starts_with("studio/info/"))
        .collect();
    paths.sort();
    let mut list = std::collections::BTreeMap::new();
    for path in paths {
        let file = path.rsplit('/').next().unwrap_or(path);
        let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
        let sheet = game
            .assets
            .find(
                path,
                &format!("Filter_{stem}"),
                Some(rk_unityfs::class::MONO_BEHAVIOUR),
                None,
            )
            .and_then(|obj| obj.read().ok());
        let Some(sheet) = sheet else { continue };
        for (id, bundle, name) in filter_rows(&sheet) {
            list.insert(id, (bundle, name));
        }
    }
    list
}

/// The colour table of a Studio colour filter by its number in the scene.
pub fn color_filter(game: &Game, id: i32) -> Option<Arc<Image>> {
    let (bundle, name) = filter_list(game).remove(&id)?;
    filter_table(game, &bundle, &name)
}

/// The colour tables of every Studio colour filter, by their numbers.
pub fn color_filters(game: &Game) -> std::collections::BTreeMap<i32, Arc<Image>> {
    filter_list(game)
        .into_iter()
        .filter_map(|(id, (bundle, name))| Some((id, filter_table(game, &bundle, &name)?)))
        .collect()
}

fn filter_table(game: &Game, bundle: &str, name: &str) -> Option<Arc<Image>> {
    let image = Loader::new(game).image_by_name(bundle, name, None)?;
    // The effect reads the table as data, whatever the texture says.
    Some(Arc::new(Image {
        name: image.name.clone(),
        width: image.width,
        height: image.height,
        rgba: image.rgba.clone(),
        srgb: false,
        repeat: false,
    }))
}

/// Bundle and texture name the game tries first for a head type other than
/// the first: the bundle name with its last two characters replaced by `50`,
/// and the texture name with `_<head>` appended.
pub fn head_variant(bundle: &str, texture: &str, head_id: i32) -> Option<(String, String)> {
    if head_id == 0 {
        return None;
    }
    let stem = bundle.replace(".unity3d", "");
    let cut = stem.char_indices().rev().nth(1).map(|(i, _)| i).unwrap_or(0);
    Some((format!("{}50.unity3d", &stem[..cut]), format!("{texture}_{head_id}")))
}

const MATERIALS: &str = "chara/mm_base.unity3d";

/// Sets or clears one of a skin material's overlay layers.
fn set_layer(m: &mut crate::scene::MaterialInst, tex: &str, color: &str, image: Option<Arc<Image>>, value: [f32; 4]) {
    match image {
        Some(image) => drop(m.textures.insert(
            tex.into(),
            TexSlot {
                image,
                scale: [1.0, 1.0],
                offset: [0.0, 0.0],
            },
        )),
        None => drop(m.textures.remove(tex)),
    }
    m.colors.insert(color.into(), value);
}

/// The textures a body or face detail item brings.
#[derive(Default)]
struct SkinDetail {
    lines: Option<Arc<Image>>,
    normal: Option<Arc<Image>>,
}

/// Sets a texture as `Material.SetTexture` does: the placement the material
/// stores for the property stays, and no texture clears the slot.
fn set_detail(m: &mut crate::scene::MaterialInst, name: &str, image: Option<Arc<Image>>) {
    match image {
        Some(image) => {
            let st = m.tex_st.get(name).copied().unwrap_or([1.0, 1.0, 0.0, 0.0]);
            m.textures.insert(
                name.into(),
                TexSlot {
                    image,
                    scale: [st[0], st[1]],
                    offset: [st[2], st[3]],
                },
            );
        }
        None => drop(m.textures.remove(name)),
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl<'g> Builder<'g> {
    /// Texture of an item, named by two of its list columns.
    fn item_image(
        &mut self,
        label: &str,
        property: &str,
        category: i32,
        id: i32,
        ab_key: &str,
        tex_key: &str,
    ) -> Option<Arc<Image>> {
        let item = self.item(label, property, category, id)?;
        self.loader
            .image_by_name(item.get(ab_key)?, item.get(tex_key)?, item.mod_idx)
    }

    /// Texture of a face item that can have a variant per head type. For a
    /// head other than the first, the game looks in the item bundle's `50`
    /// twin, first for `<texture>_<head>` and then for the plain name. When
    /// that finds nothing the plain lookup is used, which is what the
    /// community's modded-head fix adds and mods rely on.
    fn head_image(
        &mut self,
        label: &str,
        property: &str,
        category: i32,
        id: i32,
        tex_key: &str,
        head_id: i32,
    ) -> Option<Arc<Image>> {
        let item = self.item(label, property, category, id)?;
        let (ab, tex) = (item.get("MainAB")?, item.get(tex_key)?);
        if ab == "0" || tex == "0" {
            return None;
        }
        if let Some((twin, variant)) = head_variant(ab, tex, head_id) {
            let found = self
                .loader
                .image_by_name(&twin, &variant, item.mod_idx)
                .or_else(|| self.loader.image_by_name(&twin, tex, item.mod_idx));
            if found.is_some() {
                return found;
            }
        }
        self.loader.image_by_name(ab, tex, item.mod_idx)
    }

    /// Puts a composed texture on a renderer, swapping in the material the
    /// game draws that surface with when it can be loaded.
    fn set_skin(
        &mut self,
        inst: &Instance,
        renderers: &[&str],
        draw: Option<crate::scene::MaterialInst>,
        image: Image,
    ) -> (Vec<usize>, Arc<Image>) {
        let image = Arc::new(image);
        let worn: Vec<usize> = inst
            .renderers
            .iter()
            .copied()
            .filter(|&r| renderers.contains(&self.scene.renderers[r].name.as_str()))
            .collect();
        let slot = TexSlot {
            image: image.clone(),
            scale: [1.0, 1.0],
            offset: [0.0, 0.0],
        };
        for &ri in &inst.renderers {
            let r = &mut self.scene.renderers[ri];
            if !renderers.contains(&r.name.as_str()) {
                continue;
            }
            if r.materials.is_empty() {
                r.materials.push(Default::default());
            }
            if let Some(draw) = draw.clone() {
                r.materials[0] = draw;
            }
            r.materials[0].textures.insert("_MainTex".into(), slot.clone());
        }
        (worn, image)
    }

    /// Sets the values the game writes onto a skin material from the card:
    /// the line mask and the detail normal map of the chosen detail
    /// (`ChangeSettingBodyDetail`, `ChangeSettingFaceDetail`), and plain
    /// numbers.
    fn skin_values(&mut self, inst: &Instance, renderer: &str, detail: SkinDetail, floats: &[(&str, f32)]) {
        for &ri in &inst.renderers {
            let r = &mut self.scene.renderers[ri];
            let Some(m) = r.materials.first_mut().filter(|_| r.name == renderer) else {
                continue;
            };
            set_detail(m, "_LineMask", detail.lines.clone());
            set_detail(m, "_NormalMapDetail", detail.normal.clone());
            for (name, value) in floats {
                m.floats.insert((*name).into(), *value);
            }
        }
    }

    /// The two textures of a body or face detail item. The list column of
    /// the normal map is spelled `NormallMapDetail`.
    fn skin_detail(&mut self, label: &str, property: &str, category: i32, id: i32) -> SkinDetail {
        let Some(item) = self.item(label, property, category, id) else {
            return SkinDetail::default();
        };
        let mut load = |key: &str| {
            let (ab, tex) = (item.get("MainAB")?, item.get(key)?);
            if ab == "0" || tex == "0" {
                return None;
            }
            self.loader.image_by_name(ab, tex, item.mod_idx)
        };
        SkinDetail {
            lines: load("LineMask"),
            normal: load("NormallMapDetail"),
        }
    }

    /// Scene renderers a clothing prefab lists under one of its fields
    /// (`rendNormal01`...). The lists live in a script on the prefab root.
    fn listed_renderers(&mut self, inst: &Instance, field: &str) -> Vec<usize> {
        self.listed_renderers_in(inst, None, field)
    }

    /// The renderers of a clothing piece that take the liquid levels:
    /// `ChaClothesComponent.rendNormal01`, `02` and `03`.
    fn liquid_renderers(&mut self, inst: &Instance) -> Vec<usize> {
        ["rendNormal01", "rendNormal02", "rendNormal03"]
            .iter()
            .flat_map(|field| self.listed_renderers(inst, field))
            .collect()
    }

    /// Class name of a script, from its MonoScript.
    fn script_class(&self, script: &rk_vfs::ObjRef) -> Option<String> {
        let p = script.read().ok()?.get("m_Script")?.as_pptr()?;
        let ms = self.game.assets.deref(&script.file, p)?.read().ok()?;
        ms.get("m_ClassName")?.as_str().map(str::to_string)
    }

    /// Collects the DynamicBone components and colliders of everything
    /// loaded, set up the way `ChaControl.LoadCharaFbxData` leaves them.
    fn read_physics(
        &self,
        top: usize,
        bones: &HashMap<String, usize>,
        edits: &[crate::dynamic_bone_editor::Edit],
        dbde: &[crate::dbde::Edit],
        colliders: Option<(crate::colliders::Settings, f32)>,
    ) -> crate::physics::Physics {
        use crate::physics::{self, Bone, DynamicBone, Resolve, Ver01, Ver02};
        let assets = &self.game.assets;
        let scene = &self.scene;
        // Bones of parts whose skeleton was merged away no longer exist.
        let alive = |node: usize| {
            let mut n = node;
            while let Some(p) = scene.nodes[n].parent {
                n = p;
            }
            n == top
        };
        let classes: Vec<Vec<Option<String>>> = self
            .physics_sources
            .iter()
            .map(|s| s.scripts.iter().map(|(_, script)| self.script_class(script)).collect())
            .collect();

        let mut out = physics::Physics::default();
        // Colliders first, so bones can refer to them.
        let mut collider_ids: HashMap<(usize, i64), usize> = HashMap::new();
        let mut body_colliders = Vec::new();
        for (source, classes) in self.physics_sources.iter().zip(&classes) {
            for ((node, script), class) in source.scripts.iter().zip(classes) {
                if class.as_deref() != Some("DynamicBoneCollider") {
                    continue;
                }
                let Ok(obj) = script.read() else { continue };
                collider_ids.insert(script.key(), out.colliders.len());
                if !source.part {
                    body_colliders.push(out.colliders.len());
                }
                out.colliders.push(physics::read_collider(&obj, *node));
            }
        }

        // The Colliders plugin: its switches, the card's bust size and the
        // colliders it created.
        let plugin =
            colliders.map(|(settings, bust)| (settings, bust, crate::colliders::add(&mut out.colliders, bones)));

        for (source, classes) in self.physics_sources.iter().zip(&classes) {
            for ((node, script), class) in source.scripts.iter().zip(classes) {
                let Some(class) = class.as_deref() else { continue };
                if !matches!(class, "DynamicBone" | "DynamicBone_Ver01" | "DynamicBone_Ver02") {
                    continue;
                }
                let Ok(obj) = script.read() else { continue };
                let node_of = |p: rk_unityfs::PPtr| {
                    let n = *source.transforms.get(&assets.deref(&script.file, p)?.key())?;
                    // copyDynamicBone: the body's bone of the same name.
                    let n = match bones.get(&scene.nodes[n].name) {
                        Some(&body) if source.copy => body,
                        _ => n,
                    };
                    alive(n).then_some(n)
                };
                let collider_of =
                    |p: rk_unityfs::PPtr| collider_ids.get(&assets.deref(&script.file, p)?.key()).copied();
                let resolve = Resolve {
                    node: &node_of,
                    collider: &collider_of,
                };
                let on = source.shake && alive(*node);
                match class {
                    "DynamicBone" => {
                        let mut def = physics::read_dynamic_bone(&obj, *node, &resolve);
                        def.enabled &= on;
                        if source.part {
                            def.colliders = body_colliders.clone();
                        }
                        if let Some((settings, _, added)) = &plugin {
                            let object = match source.object {
                                Some(name) if *node == source.root => name,
                                _ => scene.nodes[*node].name.as_str(),
                            };
                            crate::colliders::dynamic_bone(&mut def, scene, object, *settings, added);
                        }
                        if let (Some(slot), Some(root)) = (source.slot, def.root) {
                            crate::dynamic_bone_editor::apply(edits, slot, &scene.nodes[root].name, &mut def);
                            // DBDE loads after the Dynamic Bone Editor and
                            // finds the bone by its path in the accessory.
                            if let Some(e) = crate::dbde::find(dbde, slot, &scene.path_below(source.root, root)) {
                                if let Some(f) = e.force {
                                    def.force = glam::Vec3::from(f);
                                }
                                if let Some(w) = e.weight {
                                    def.weight = w;
                                }
                                if let Some(a) = e.active {
                                    def.enabled &= a;
                                }
                            }
                        }
                        out.bones.push(Bone::Dynamic(DynamicBone::new(def)));
                        out.belongs.push(source.belongs);
                    }
                    "DynamicBone_Ver01" => {
                        let mut def = physics::read_ver01(&obj, *node, &resolve);
                        def.enabled &= on;
                        out.bones.push(Bone::Ver01(Ver01::new(def)));
                        out.belongs.push(source.belongs);
                    }
                    _ => {
                        let mut def = physics::read_ver02(&obj, *node, &resolve);
                        def.enabled &= on;
                        if let Some((settings, bust, added)) = &plugin {
                            crate::colliders::ver02(&mut def, *settings, added, *bust);
                        }
                        out.bones.push(Bone::Ver02(Ver02::new(def)));
                        out.belongs.push(source.belongs);
                    }
                }
            }
        }
        out
    }

    /// Runs the Accessory Clothes plugin on an accessory: when the prefab has
    /// a `ChaAccessoryClothes` script, the `rendNormal` renderers of the
    /// `ChaAccessoryComponent` on the same object are bound to the body
    /// skeleton and the prefab's own armature is destroyed.
    fn accessory_clothes(&mut self, inst: &Instance, dict: &HashMap<String, usize>, root_bone: Option<usize>) {
        use crate::accessory_clothes::{self as ac, ARMATURE_ROOT, COMPONENT, RENDERERS, SCRIPT};
        for (node, script) in &inst.scripts {
            if self.script_class(script).as_deref() != Some(SCRIPT) {
                continue;
            }
            // `Start` reads `GetComponent<ChaAccessoryComponent>()` first and
            // throws without one, before anything is bound or destroyed.
            let Some(component) = inst
                .scripts
                .iter()
                .find(|(n, s)| n == node && self.script_class(s).as_deref() == Some(COMPONENT))
                .and_then(|(_, s)| s.read().ok())
            else {
                continue;
            };
            let assets = &self.game.assets;
            let renderers: Vec<usize> = match component.get(RENDERERS) {
                Some(rk_unityfs::UValue::Array(a)) => a
                    .iter()
                    .filter_map(|v| v.as_pptr())
                    .filter_map(|p| inst.renderer_keys.get(&assets.deref(&script.file, p)?.key()).copied())
                    .collect(),
                _ => Vec::new(),
            };
            let armature = script
                .read()
                .ok()
                .and_then(|o| o.get(ARMATURE_ROOT)?.as_pptr())
                .and_then(|p| inst.transform_nodes.get(&assets.deref(&script.file, p)?.key()).copied());
            ac::bind(&mut self.scene, &renderers, armature, dict, root_bone);
        }
    }

    /// Like `listed_renderers`, limited to scripts of one class when given.
    fn listed_renderers_in(&mut self, inst: &Instance, class: Option<&str>, field: &str) -> Vec<usize> {
        self.listed(inst, class, field, false)
    }

    /// Renderers a field of the hair part's ChaCustomHairComponent lists.
    /// The game takes the component with `GetComponent` on the part's root
    /// object, so components further down the prefab are not used.
    fn hair_listed(&mut self, inst: &Instance, field: &str) -> Vec<usize> {
        self.listed(inst, Some("ChaCustomHairComponent"), field, true)
    }

    /// Records the renderers a Copy Cha Body Mat component on the item
    /// lists, so they can take the body's material once it is finished.
    fn find_body_copies(&mut self, inst: &Instance) {
        for (_, script) in &inst.scripts {
            if self.script_class(script).as_deref() != Some(crate::copy_body_material::SCRIPT) {
                continue;
            }
            let Ok(obj) = script.read() else { continue };
            let targets: Vec<usize> = match obj.get("rendTarget") {
                Some(rk_unityfs::UValue::Array(a)) => a
                    .iter()
                    .filter_map(|v| v.as_pptr())
                    .filter_map(|p| self.game.assets.deref(&script.file, p))
                    .filter_map(|o| inst.renderer_keys.get(&o.key()).copied())
                    .collect(),
                _ => Vec::new(),
            };
            let queue = obj
                .get("customRenderQueue")
                .and_then(rk_unityfs::UValue::as_i64)
                .unwrap_or(-1) as i32;
            if !targets.is_empty() {
                self.body_copies.push((targets, queue));
            }
        }
    }

    /// Clothing BlendShape on one clothing item: the weights its
    /// `ChaClothesBlendShape` component lists, from the card's entries.
    fn clothing_blend_shape(
        &mut self,
        inst: &Instance,
        entries: &[crate::clothing_blend_shape::Entry],
        outfit: usize,
        category: i32,
        kind: usize,
    ) {
        use crate::clothing_blend_shape as cbs;
        let Some(script) = inst
            .scripts
            .iter()
            .find(|(_, s)| self.script_class(s).as_deref() == Some(cbs::SCRIPT))
            .map(|(_, s)| s.clone())
        else {
            return;
        };
        let Ok(obj) = script.read() else { return };
        let mut slots = Vec::new();
        for n in 1..=4 {
            let named = obj
                .get(&format!("ItemName0{n}"))
                .and_then(rk_unityfs::UValue::as_str)
                .is_some_and(|s| !s.is_empty());
            let shape = obj
                .get(&format!("BlendShapeIndex0{n}"))
                .and_then(rk_unityfs::UValue::as_i64)
                .unwrap_or(0)
                .max(0) as usize;
            let meshes = match obj.get(&format!("Meshes0{n}")) {
                Some(rk_unityfs::UValue::Array(a)) => a
                    .iter()
                    .filter_map(|v| v.as_pptr())
                    .filter_map(|p| self.game.assets.deref(&script.file, p))
                    .filter_map(|o| inst.renderer_keys.get(&o.key()).copied())
                    .map(|r| (r, self.scene.renderers[r].mesh.blend_shapes.len()))
                    .collect(),
                _ => Vec::new(),
            };
            slots.push(cbs::Slot { named, meshes, shape });
        }
        let weights = cbs::weights_for(entries, outfit, category, kind);
        for (r, shape, w) in cbs::settle(&slots, weights) {
            let sw = &mut self.scene.renderers[r].shape_weights;
            if sw.len() <= shape {
                sw.resize(shape + 1, 0.0);
            }
            sw[shape] = w;
        }
    }

    /// Renderers a field of the accessory's ChaAccessoryComponent lists; the
    /// game's `cusAcsCmp` is the component on the accessory's root object.
    fn accessory_listed(&mut self, inst: &Instance, field: &str) -> Vec<usize> {
        self.listed(inst, Some("ChaAccessoryComponent"), field, true)
    }

    /// A field of the script of class `class` on the part's root object.
    fn root_field(&self, inst: &Instance, class: &str, field: &str) -> Option<rk_unityfs::UValue> {
        inst.scripts
            .iter()
            .filter(|(node, s)| *node == inst.root && self.script_class(s).as_deref() == Some(class))
            .find_map(|(_, s)| s.read().ok()?.get(field).cloned())
    }

    fn listed(&mut self, inst: &Instance, class: Option<&str>, field: &str, root_only: bool) -> Vec<usize> {
        let mut out = Vec::new();
        for (node, script) in &inst.scripts {
            if root_only && *node != inst.root {
                continue;
            }
            if class.is_some_and(|c| self.script_class(script).as_deref() != Some(c)) {
                continue;
            }
            let Ok(obj) = script.read() else { continue };
            let entries: Vec<rk_unityfs::PPtr> = match obj.get(field) {
                Some(rk_unityfs::UValue::Array(a)) => a.iter().filter_map(|v| v.as_pptr()).collect(),
                Some(v) => v.as_pptr().into_iter().collect(),
                None => continue,
            };
            for p in entries {
                if let Some(r) = self
                    .game
                    .assets
                    .deref(&script.file, p)
                    .and_then(|o| inst.renderer_keys.get(&o.key()).copied())
                {
                    out.push(r);
                }
            }
        }
        out
    }

    /// The card's emblems on a clothing piece, as the game's
    /// `ChangeCustomEmblem` does: the first emblem goes to the renderers in
    /// `rendEmblem01`, `rendEmblem02` and `exRendEmblem01`, the second to
    /// `exRendEmblem02`. Sub-parts of a top take the top's emblems.
    fn emblems(&mut self, inst: &Instance, part: &rk_card::ClothesPart, property: &str) {
        let slots: [(i32, &[&str]); 2] = [
            (part.embleme_id, &["rendEmblem01", "rendEmblem02", "exRendEmblem01"]),
            (part.embleme_id2, &["exRendEmblem02"]),
        ];
        for (k, (id, fields)) in slots.into_iter().enumerate() {
            let targets: Vec<usize> = fields.iter().flat_map(|f| self.listed_renderers(inst, f)).collect();
            if targets.is_empty() {
                continue;
            }
            // Studio draws the full-detail texture, without the `_low` suffix.
            let image = self.item_image(
                "emblem",
                // Sideloader's resolver names them ClothesTopEmblem and
                // ClothesTopEmblem2.
                &if k == 0 {
                    format!("{property}Emblem")
                } else {
                    format!("{property}Emblem2")
                },
                cat::MT_EMBLEM,
                id,
                "MainTexAB",
                "MainTex",
            );
            for ri in targets {
                for m in &mut self.scene.renderers[ri].materials {
                    match &image {
                        Some(image) => {
                            let st = m.tex_st.get("_MainTex").copied().unwrap_or([1.0, 1.0, 0.0, 0.0]);
                            m.textures.insert(
                                "_MainTex".into(),
                                TexSlot {
                                    image: image.clone(),
                                    scale: [st[0], st[1]],
                                    offset: [st[2], st[3]],
                                },
                            );
                        }
                        // No emblem: the game sets no texture, so the shader's
                        // default applies. main_emblem's is "black", which Unity
                        // makes transparent black, and its cutoff hides it.
                        None => {
                            m.textures.insert(
                                "_MainTex".into(),
                                TexSlot {
                                    image: Arc::new(Image::unity_black()),
                                    scale: [1.0, 1.0],
                                    offset: [0.0, 0.0],
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    /// Paints a plugin overlay onto a source texture before it is used.
    fn under(&self, kind: i64, base: Option<Arc<Image>>) -> Option<Arc<Image>> {
        match (self.skin_overlays.get(&kind), base) {
            (Some(over), Some(base)) => Some(Arc::new(compose::blend(&base, over))),
            (_, base) => base,
        }
    }

    /// Paints a plugin overlay onto a finished texture.
    fn over(&self, kind: i64, image: Image) -> Image {
        match self.skin_overlays.get(&kind) {
            Some(over) => compose::blend(&image, over),
            None => image,
        }
    }

    /// Sets when each brow and eye layer is drawn, so it ends up in front of
    /// or behind front hair as the card and the game setting say.
    fn face_layer_order(&mut self, inst: &Instance, eyes_flag: u8, brow_flag: u8) {
        for (node, script) in &inst.scripts {
            let Some(stored) = script.read().ok().as_ref().and_then(foreground::read) else {
                continue;
            };
            for &ri in &inst.renderers {
                let r = &mut self.scene.renderers[ri];
                if r.node != *node {
                    continue;
                }
                let flag = if foreground::is_brow(&r.name) {
                    brow_flag
                } else {
                    eyes_flag
                };
                let front = foreground::in_front(flag, foreground::DEFAULT_IN_FRONT);
                for q in foreground::with_hair_shadows_fix(&r.name, stored.clone()) {
                    if let Some(m) = r.materials.get_mut(q.material) {
                        m.render_queue = if front { q.in_front } else { q.behind };
                    }
                }
            }
        }
    }

    /// The head's face controllers bound to its meshes, showing `start`;
    /// `start_rate` is the eyes' open rate it starts with.
    fn face_rig(&mut self, inst: &Instance, start: crate::face::Face, start_rate: f32) -> crate::face::Rig {
        let mut rig = crate::face::Rig {
            start,
            start_rate,
            up_correct: crate::eye_control::EYE_LOOK_UP_CORRECT,
            ..Default::default()
        };
        for (_, script) in &inst.scripts {
            let Ok(obj) = script.read() else { continue };
            for field in [expression::EYEBROWS, expression::EYES, expression::MOUTH] {
                let Some(controller) = obj
                    .get(field)
                    .and_then(rk_unityfs::UValue::as_object)
                    .and_then(expression::Controller::read)
                else {
                    continue;
                };
                if field == expression::EYES {
                    if let Some(up) = obj.get("EyeLookUpCorrect").and_then(rk_unityfs::UValue::as_f32) {
                        rig.up_correct = up;
                    }
                }
                let renderers = controller
                    .targets
                    .iter()
                    .map(|target| {
                        let node = self
                            .game
                            .assets
                            .deref(&script.file, target.object)
                            .and_then(|o| inst.object_nodes.get(&o.key()).copied());
                        inst.renderers
                            .iter()
                            .copied()
                            .filter(|&r| Some(self.scene.renderers[r].node) == node)
                            .collect()
                    })
                    .collect();
                let part = Some(crate::face::Part { controller, renderers });
                match field {
                    expression::EYEBROWS => rig.eyebrows = part,
                    expression::EYES => rig.eyes = part,
                    _ => rig.mouth = part,
                }
            }
        }
        // Studio's eye patterns are the `cha_eyeset` items. Studio's
        // characters are high poly, so the pictures have no `_low`.
        for id in self.game.items.vanilla_ids(cat::CHA_EYESET) {
            let Some(item) = self.game.items.vanilla(cat::CHA_EYESET, id) else {
                continue;
            };
            let Some(pattern) = item.int("EyesPtn") else { continue };
            let expression = match (item.get("EpsTexAB"), item.get("EpsTex")) {
                (Some(bundle), Some(name)) if bundle != "0" && name != "0" => {
                    self.loader.image_by_name(bundle, name, None)
                }
                _ => None,
            };
            rig.eyesets.insert(
                id,
                crate::face::EyeRow {
                    pattern,
                    base: item.int("EyeBase") == Some(1),
                    pupils: item.int("EyeHitomi").unwrap_or(0) != 0,
                    gag: item.int("EyeObjNo").unwrap_or(0),
                    expression,
                },
            );
        }
        rig.tears = ["cf_O_namida_S", "cf_O_namida_M", "cf_O_namida_L"].map(|n| self.scene.find(inst.root, n));
        rig.face = inst
            .renderers
            .iter()
            .copied()
            .find(|&r| self.scene.renderers[r].name == "cf_O_face");
        rig.eye_base = self.scene.find(inst.root, "n_eyebase");
        rig.pupils = self.scene.find(inst.root, "n_hitomi");
        rig.irises = inst
            .renderers
            .iter()
            .copied()
            .filter(|&r| {
                matches!(
                    self.scene.renderers[r].name.as_str(),
                    "cf_Ohitomi_L02" | "cf_Ohitomi_R02"
                )
            })
            .collect();
        rig.show(&mut self.scene, None);
        rig
    }

    /// The FullBodyBipedIK component of a prefab, its bones as scene nodes.
    fn read_fbbik(&self, inst: &crate::scene::Instance) -> Option<crate::fbbik::Settings> {
        let (_, script) = inst
            .scripts
            .iter()
            .find(|(_, s)| self.script_class(s).as_deref() == Some("FullBodyBipedIK"))?;
        let obj = script.read().ok()?;
        let node = |p: rk_unityfs::PPtr| {
            let target = self.game.assets.deref(&script.file, p)?;
            inst.transform_nodes.get(&target.key()).copied()
        };
        Some(crate::fbbik::read(&obj, &node))
    }

    /// First value of a script field on the prefab, e.g. a `useColor01` flag.
    fn script_field(&self, inst: &Instance, field: &str) -> Option<rk_unityfs::UValue> {
        inst.scripts
            .iter()
            .find_map(|(_, script)| script.read().ok()?.get(field).cloned())
    }

    /// Hair colours: base, root and tip on the hair meshes, and the card's
    /// colours on ornaments that are part of the hair style.
    fn color_hair(&mut self, inst: &Instance, part: &rk_card::HairPart) {
        let solid = |c: [f32; 4]| [c[0], c[1], c[2], 1.0];
        for r in self.hair_listed(inst, "rendHair") {
            if let Some(m) = self.scene.renderers[r].materials.first_mut() {
                if let Some(gloss) = &self.hair_gloss {
                    // The material keeps where the mask sits on the strands.
                    let mut slot = gloss.clone();
                    if let Some(st) = m.tex_st.get("_HairGloss") {
                        slot.scale = [st[0], st[1]];
                        slot.offset = [st[2], st[3]];
                    }
                    m.set_texture("_HairGloss", slot);
                }
                m.set_color("_Color", solid(part.base_color));
                m.set_color("_Color2", solid(part.start_color));
                m.set_color("_Color3", solid(part.end_color));
                m.set_color("_LineColor", part.outline_color);
            }
        }
        let count = match self.script_field(inst, "acsDefColor") {
            Some(rk_unityfs::UValue::Array(a)) => a.len().min(3),
            _ => 0,
        };
        for r in self.hair_listed(inst, "rendAccessory") {
            if let Some(m) = self.scene.renderers[r].materials.first_mut() {
                for (j, name) in ["_Color", "_Color2", "_Color3"].iter().enumerate().take(count) {
                    if let Some(c) = part.acs_color.get(j) {
                        m.set_color(name, solid(*c));
                    }
                }
            }
        }
    }

    /// Accessory colours, for the colour slots the accessory says it uses.
    fn color_accessory(
        &mut self,
        inst: &Instance,
        acc: &rk_card::AccessoryPart,
        hair: Option<&rk_card::HairPart>,
        settings: Option<crate::hair_accessories::HairAccessory>,
    ) {
        let uses = |b: &Self, field: &str| {
            b.root_field(inst, "ChaAccessoryComponent", field)
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        };
        let slots = [
            (uses(self, "useColor01"), "_Color"),
            (uses(self, "useColor02"), "_Color2"),
            (uses(self, "useColor03"), "_Color3"),
        ];
        for r in self.accessory_listed(inst, "rendNormal") {
            if let Some(m) = self.scene.renderers[r].materials.first_mut() {
                for (k, (used, name)) in slots.iter().enumerate() {
                    if let (true, Some(c)) = (*used, acc.color.get(k)) {
                        m.set_color(name, *c);
                    }
                }
            }
        }
        if let Some(c) = acc.color.get(3) {
            for r in self.accessory_listed(inst, "rendAlpha") {
                // A fully transparent fourth colour switches the part off.
                self.scene.renderers[r].enabled &= c[3] != 0.0;
                if let Some(m) = self.scene.renderers[r].materials.first_mut() {
                    m.set_color("_Color4", *c);
                }
            }
        }
        // The accessory component's own hair renderers follow the hair's
        // root colour. The strands of a hair component are not among them.
        for r in self.accessory_listed(inst, "rendHair") {
            if let Some(m) = self.scene.renderers[r].materials.first_mut() {
                for name in ["_Color", "_Color2", "_Color3"] {
                    m.set_color(name, hair.map(|h| h.start_color).unwrap_or([1.0; 4]));
                }
            }
        }
        if let Some(settings) = settings {
            self.hair_accessory(inst, hair, settings);
        }
    }

    /// Hair length, as `ChaCustomHairComponent.Update` sets it every frame:
    /// each bone in `trfLength` sits at its stored base height plus
    /// `addLength` times the length rate. `rate` is the card's or plugin's
    /// value; without one the prefab's own rate stays.
    fn hair_length(&mut self, inst: &Instance, rate: Option<f32>) {
        let Some(script) = inst
            .scripts
            .iter()
            .map(|(_, s)| s)
            .find(|s| self.script_class(s).as_deref() == Some("ChaCustomHairComponent"))
            .cloned()
        else {
            return;
        };
        let Ok(obj) = script.read() else { return };
        let float = |v: Option<&rk_unityfs::UValue>| v.and_then(|v| v.as_f32());
        let add = float(obj.get("addLength")).unwrap_or(0.0);
        let rate = rate.or_else(|| float(obj.get("lengthRate"))).unwrap_or(0.0);
        let (Some(rk_unityfs::UValue::Array(bones)), Some(rk_unityfs::UValue::Array(base))) =
            (obj.get("trfLength"), obj.get("baseLength"))
        else {
            return;
        };
        // Mathf.Lerp clamps its rate.
        let extra = add * rate.clamp(0.0, 1.0);
        for (bone, base) in bones.iter().zip(base) {
            let (Some(p), Some(base)) = (bone.as_pptr(), base.as_f32()) else {
                continue;
            };
            let Some(node) = self
                .game
                .assets
                .deref(&script.file, p)
                .and_then(|o| inst.transform_nodes.get(&o.key()).copied())
            else {
                continue;
            };
            self.scene.nodes[node].local.pos.y = base + extra;
        }
    }

    /// The Hair Shadows Fix (IllusionFixes) draws front hair between the two
    /// queues of the face layers. It hooks every object the game loads that
    /// has a hair component, so hair worn as an accessory counts too.
    fn hair_shadows_fix(&mut self, inst: &Instance) {
        if !inst
            .scripts
            .iter()
            .any(|(_, s)| self.script_class(s).as_deref() == Some("ChaCustomHairComponent"))
        {
            return;
        }
        for &ri in &inst.renderers {
            for m in &mut self.scene.renderers[ri].materials {
                if m.shader == foreground::FRONT_HAIR_SHADER {
                    m.render_queue = foreground::FRONT_HAIR_QUEUE;
                }
            }
        }
    }

    /// Hair Accessory Customizer's settings on an accessory with a hair
    /// component, applied after the game's colours as the plugin does.
    fn hair_accessory(
        &mut self,
        inst: &Instance,
        hair: Option<&rk_card::HairPart>,
        settings: crate::hair_accessories::HairAccessory,
    ) {
        const HAIR: &str = "ChaCustomHairComponent";
        if !inst
            .scripts
            .iter()
            .any(|(_, s)| self.script_class(s).as_deref() == Some(HAIR))
        {
            return;
        }
        let gloss = self.hair_gloss.clone();
        for r in self.listed_renderers_in(inst, Some(HAIR), "rendHair") {
            let Some(m) = self.scene.renderers[r].materials.first_mut() else {
                continue;
            };
            if let (true, Some(h)) = (settings.color_match, hair) {
                m.set_color("_Color", h.base_color);
                m.set_color("_Color2", h.start_color);
                m.set_color("_Color3", h.end_color);
            }
            match (&gloss, settings.hair_gloss) {
                (Some(g), true) => {
                    let mut slot = g.clone();
                    if let Some(st) = m.tex_st.get("_HairGloss") {
                        slot.scale = [st[0], st[1]];
                        slot.offset = [st[2], st[3]];
                    }
                    m.set_texture("_HairGloss", slot);
                }
                _ => {
                    m.textures.remove("_HairGloss");
                }
            }
            let outline = match (settings.color_match, hair) {
                (true, Some(h)) => h.outline_color,
                _ => settings.outline_color,
            };
            m.set_color("_LineColor", outline);
        }
        for r in self.listed_renderers_in(inst, Some(HAIR), "rendAccessory") {
            if let Some(m) = self.scene.renderers[r].materials.first_mut() {
                for name in ["_Color", "_Color2", "_Color3"] {
                    m.set_color(name, settings.accessory_color);
                }
            }
        }
    }

    /// Eyebrows, eyeliner and the nose line: texture and colour per mesh.
    fn face_lines(&mut self, card: &Card, inst: &Instance) {
        let face = &card.face;
        let up = self.head_image(
            "eyeline",
            "ChaFileFace.eyelineUpId",
            cat::MT_EYELINE_UP,
            face.eyeline_up_id,
            "EyelineUpTex",
            face.head_id,
        );
        let up_shadow = self.head_image(
            "eyeline",
            "ChaFileFace.eyelineUpId",
            cat::MT_EYELINE_UP,
            face.eyeline_up_id,
            "EyelineShadowTex",
            face.head_id,
        );
        let down = self.head_image(
            "eyeline",
            "ChaFileFace.eyelineDownId",
            cat::MT_EYELINE_DOWN,
            face.eyeline_down_id,
            "EyelineDownTex",
            face.head_id,
        );
        let brow = self.item_image(
            "eyebrow",
            "ChaFileFace.eyebrowId",
            cat::MT_EYEBROW,
            face.eyebrow_id,
            "MainAB",
            "EyebrowTex",
        );
        let nose = self.item_image(
            "nose",
            "ChaFileFace.noseId",
            cat::MT_NOSE,
            face.nose_id,
            "MainAB",
            "NoseTex",
        );
        let (up, down) = (
            self.under(kind::EYELINE_UNDER, up),
            self.under(kind::EYELINE_UNDER, down),
        );
        let brow = self.under(kind::EYEBROW_UNDER, brow);
        let skin = card.body.skin_main_color;
        // (mesh, material index, texture, colour)
        type Line = (&'static str, usize, Option<Arc<Image>>, Option<[f32; 4]>);
        let plan: [Line; 5] = [
            ("cf_O_eyeline", 0, up, Some(face.eyeline_color)),
            ("cf_O_eyeline", 1, up_shadow, Some(skin)),
            ("cf_O_eyeline_low", 0, down, Some(face.eyeline_color)),
            ("cf_O_mayuge", 0, brow, Some(face.eyebrow_color)),
            ("cf_O_noseline", 0, nose, None),
        ];
        for &ri in &inst.renderers {
            let r = &mut self.scene.renderers[ri];
            for (mesh, index, image, color) in &plan {
                let Some(m) = (r.name == *mesh).then(|| r.materials.get_mut(*index)).flatten() else {
                    continue;
                };
                match image {
                    Some(image) => {
                        m.textures.insert(
                            "_MainTex".into(),
                            TexSlot {
                                image: image.clone(),
                                scale: [1.0, 1.0],
                                offset: [0.0, 0.0],
                            },
                        );
                    }
                    // "None" in the maker: the game sets a null texture and
                    // the shader's default shows.
                    None => match crate::scene::unset_line_texture(&m.shader) {
                        Some(image) => {
                            m.textures.insert(
                                "_MainTex".into(),
                                TexSlot {
                                    image: Arc::new(image),
                                    scale: [1.0, 1.0],
                                    offset: [0.0, 0.0],
                                },
                            );
                        }
                        None => {
                            m.textures.remove("_MainTex");
                        }
                    },
                }
                if let Some(c) = color {
                    m.colors.insert("_Color".into(), *c);
                }
            }
        }
    }

    /// Applies the masks a top brings for what is worn under it: one cuts
    /// the body, one the bra, one the inner layer of the top itself. A top
    /// can also rule out a bra altogether. `tops` holds the top, then the
    /// parts of a sailor or jacket top with their part number: as
    /// ChangeClothesTopParts does, the first part clears the body and bra
    /// masks and loads its own, the second does the same for the inner one.
    fn hide_under_top(
        &mut self,
        body: &Instance,
        tops: &[(&Item, Option<usize>)],
        top_insts: &[Instance],
        bra: Option<&(Instance, &Item)>,
    ) {
        let mut masks: [Option<Arc<Image>>; 3] = [None, None, None];
        let columns = [
            ("OverBodyMaskAB", "OverBodyMask"),
            ("OverBraMaskAB", "OverBraMask"),
            ("OverInnerMaskAB", "OverInnerMask"),
        ];
        let mut no_bra = false;
        // The first part of a sailor or jacket top sets the body mask even
        // when it has none, which removes the one the body material stores.
        let first_part = tops.iter().any(|&(_, part)| part == Some(0));
        for &(item, part) in tops {
            no_bra |= self.list_int(item, "NotBra") == Some(1);
            let which: &[usize] = match part {
                None => &[0, 1, 2],
                Some(0) => &[0, 1],
                Some(1) => &[2],
                Some(_) => &[],
            };
            if part.is_some() {
                for &k in which {
                    masks[k] = None;
                }
            }
            for (k, (ab, tex)) in columns.iter().enumerate() {
                if !which.contains(&k) {
                    continue;
                }
                if let (Some(ab), Some(tex)) = (item.get(ab), item.get(tex)) {
                    if let Some(image) = self.loader.image_by_name(ab, tex, item.mod_idx) {
                        masks[k] = Some(image);
                    }
                }
            }
        }
        // KoiClothesOverlayX puts its own mask in place of whatever the game
        // sets, and the game sets one for each of these while a top exists.
        if !tops.is_empty() {
            for (mask, kind) in masks.iter_mut().zip(overlays::MASK_IDS) {
                *mask = overlays::mask(&self.cloth_overlays, kind, mask.take());
            }
        }
        let [body_mask, bra_mask, inner_mask] = masks;
        let put = |scene: &mut Scene,
                   inst: &Instance,
                   names: &[&str],
                   image: &Arc<Image>,
                   scale: [f32; 2],
                   offset: [f32; 2]| {
            for &ri in &inst.renderers {
                let r = &mut scene.renderers[ri];
                if !names.contains(&r.name.as_str()) {
                    continue;
                }
                if let Some(m) = r.materials.first_mut() {
                    m.textures.insert(
                        "_AlphaMask".into(),
                        TexSlot {
                            image: image.clone(),
                            scale,
                            offset,
                        },
                    );
                    // Fully dressed: both mask channels apply.
                    m.floats.insert("_alpha_a".into(), 1.0);
                    m.floats.insert("_alpha_b".into(), 1.0);
                }
            }
        };
        if let Some(image) = &body_mask {
            put(&mut self.scene, body, &["o_body_a"], image, [1.0, 1.0], [0.0, 0.0]);
        } else if first_part {
            for &ri in &body.renderers {
                let r = &mut self.scene.renderers[ri];
                if r.name == "o_body_a" {
                    if let Some(m) = r.materials.first_mut() {
                        m.textures.remove("_AlphaMask");
                    }
                }
            }
        }
        if let Some((inst, item)) = bra {
            if no_bra {
                self.scene.nodes[inst.root].active = false;
            } else if let Some(image) = &bra_mask {
                // One-piece bras use the lower half of a taller mask.
                let (scale, offset) = if self.list_int(item, "Coordinate") == Some(2) {
                    ([1.0, 2.0], [0.0, -1.0])
                } else {
                    ([1.0, 1.0], [0.0, 0.0])
                };
                put(&mut self.scene, inst, &["o_bra_a", "o_bra_b"], image, scale, offset);
            }
        }
        if let Some(image) = &inner_mask {
            for inst in top_insts {
                put(
                    &mut self.scene,
                    inst,
                    &["o_inner_a", "o_inner_b"],
                    image,
                    [1.0, 1.0],
                    [0.0, 0.0],
                );
            }
        }
    }

    /// Builds a clothing piece's textures from the card's colours and
    /// patterns and puts them on the meshes the prefab assigns them to.
    fn dress(
        &mut self,
        inst: &Instance,
        item: &Item,
        colors: &[rk_card::ClothesColor],
        sources: [usize; 3],
        pattern_prop: &str,
        clothes_id: &str,
    ) {
        let pairs = [
            (
                "MainTexAB",
                "MainTex",
                "ColorMaskAB",
                "ColorMaskTex",
                &["rendNormal01", "rendAlpha01", "rendAccessory"][..],
            ),
            (
                "MainTex02AB",
                "MainTex02",
                "ColorMask02AB",
                "ColorMask02Tex",
                &["rendNormal02"][..],
            ),
            (
                "MainTex03AB",
                "MainTex03",
                "ColorMask03AB",
                "ColorMask03Tex",
                &["rendNormal03"][..],
            ),
        ];
        let mut patterns = [None, None, None];
        for (k, slot) in patterns.iter_mut().enumerate() {
            let Some(c) = colors.get(sources[k]) else { continue };
            if c.pattern != 0 {
                // Sideloader keys a pattern by the card entry it is stored
                // in, which for a top's third sub-part is the fourth one.
                *slot = self.item_image(
                    "pattern",
                    &format!("{pattern_prop}{}", sources[k]),
                    cat::MT_PATTERN,
                    c.pattern,
                    "MainTexAB",
                    "MainTex",
                );
            }
        }
        let color = |k: usize| {
            let c = colors.get(sources[k]).cloned().unwrap_or_default();
            ClothColor {
                base: c.base_color,
                pattern: patterns[k].as_deref(),
                pattern_color: c.pattern_color,
                tiling: c.tiling,
            }
        };
        let cloth = [color(0), color(1), color(2)];
        let overlay = self.cloth_overlays.get(clothes_id).cloned();
        let mask_override = self
            .cloth_overlays
            .get(&overlays::mask_id(clothes_id))
            .and_then(|o| o.image.clone());
        for (index, (main_ab, main_tex, mask_ab, mask_tex, fields)) in pairs.into_iter().enumerate() {
            let ab = |key: &str| item.get(key).or_else(|| item.get("MainAB"));
            let (Some(main), Some(mask)) = (
                ab(main_ab)
                    .zip(item.get(main_tex))
                    .and_then(|(ab, t)| self.loader.image_by_name(ab, t, item.mod_idx)),
                ab(mask_ab)
                    .zip(item.get(mask_tex))
                    .and_then(|(ab, t)| self.loader.image_by_name(ab, t, item.mod_idx)),
            ) else {
                continue;
            };
            let targets: Vec<usize> = fields.iter().flat_map(|f| self.listed_renderers(inst, f)).collect();
            if targets.is_empty() {
                continue;
            }
            // Plugin overlays: a replacement colour mask, then an image
            // painted over (or instead of) the first two textures.
            let mask = mask_override.clone().unwrap_or(mask);
            let mut image = compose::clothes(&main, &mask, &cloth);
            if let (true, Some(o)) = (index < 2, &overlay) {
                if o.replace {
                    image = compose::clear(image.width, image.height, "clothes_composed");
                }
                if let Some(over) = &o.image {
                    image = compose::blend(&image, over);
                }
            }
            let image = Arc::new(image);
            for r in targets {
                if let Some(m) = self.scene.renderers[r].materials.first_mut() {
                    m.textures.insert(
                        "_MainTex".into(),
                        TexSlot {
                            image: image.clone(),
                            scale: [1.0, 1.0],
                            offset: [0.0, 0.0],
                        },
                    );
                }
            }
        }
    }

    /// Builds iris and eye-white textures and sets the eye highlights.
    fn eyes(&mut self, card: &Card, inst: &Instance, unclamped_highlights: bool) {
        let face = &card.face;
        let white = self.item_image(
            "eye white",
            "ChaFileFace.whiteId",
            cat::MT_EYE_WHITE,
            face.white_id,
            "MainAB",
            "EyeWhiteTex",
        );
        let white = Arc::new(compose::eye_white(
            512,
            white.as_deref(),
            face.white_base_color,
            face.white_sub_color,
        ));
        let hl_up = self.item_image(
            "eye highlight",
            "ChaFileFace.hlUpId",
            cat::MT_EYE_HI_UP,
            face.hl_up_id,
            "MainAB",
            "EyeHiUpTex",
        );
        let hl_down = self.item_image(
            "eye highlight",
            "ChaFileFace.hlDownId",
            cat::MT_EYE_HI_DOWN,
            face.hl_down_id,
            "MainAB",
            "EyeHiDownTex",
        );
        let slot = |image: Arc<Image>| TexSlot {
            image,
            scale: [1.0, 1.0],
            offset: [0.0, 0.0],
        };
        // Each eye's script holds how far a look may move the iris.
        let mut looks = [EyeLook::default(); 2];
        for (_, script) in &inst.scripts {
            if let Some((side, look)) = script.read().ok().as_ref().and_then(EyeLook::read) {
                if let Some(l) = looks.get_mut(side) {
                    *l = look;
                }
            }
        }
        let pupil_sliders = eyelook::Pupil {
            x: face.pupil_x,
            y: face.pupil_y,
            width: face.pupil_width,
            height: face.pupil_height,
        };

        for (side, (iris_mesh, white_mesh)) in [("cf_Ohitomi_L02", "cf_Ohitomi_L"), ("cf_Ohitomi_R02", "cf_Ohitomi_R")]
            .into_iter()
            .enumerate()
        {
            let Some(pupil) = face.pupil.get(side) else { continue };
            let main = self.item_image(
                "pupil",
                &format!("ChaFileFace.Pupil{}", side + 1),
                cat::MT_EYE,
                pupil.id,
                "MainAB",
                "EyeTex",
            );
            let gradient = self.item_image(
                "pupil gradient",
                &format!("ChaFileFace.PupilGradient{}", side + 1),
                cat::MT_EYE_GRADATION,
                pupil.grad_mask_id,
                "ColorMaskAB",
                "ColorMaskTex",
            );
            let (under, over) = if side == 0 {
                (kind::EYE_UNDER_L, kind::EYE_OVER_L)
            } else {
                (kind::EYE_UNDER_R, kind::EYE_OVER_R)
            };
            let main = self.under(under, main);
            let iris = compose::eye(&compose::EyeInputs {
                size: 512,
                main: main.as_deref(),
                gradient: gradient.as_deref(),
                base: pupil.base_color,
                sub: pupil.sub_color,
                blend: pupil.grad_blend,
                gradient_offset_y: pupil.grad_offset_y,
                gradient_scale: pupil.grad_scale,
            });
            let iris = Arc::new(self.over(over, iris));
            for &ri in &inst.renderers {
                let r = &mut self.scene.renderers[ri];
                let Some(m) = r.materials.first_mut() else { continue };
                if r.name == iris_mesh {
                    // Eyes at rest look straight ahead.
                    let place = eyelook::placement(&looks[side], side == 1, pupil_sliders, [0.0, 0.0]);
                    let placed = |image: Arc<Image>, shift_y: f32| TexSlot {
                        image,
                        scale: place.scale,
                        offset: [place.offset[0], place.offset[1] + shift_y],
                    };
                    m.textures.insert("_MainTex".into(), placed(iris.clone(), 0.0));
                    // ChangeSettingEyeTilt: face slider 33 tilts the irises,
                    // Lerp(0.02, -0.02, v) on the left and the mirror on the right.
                    let tilt = face.shape_value_face.get(33).copied().unwrap_or(0.5).clamp(0.0, 1.0);
                    let rotation = 0.02 - 0.04 * tilt;
                    m.floats
                        .insert("_rotation".into(), if side == 1 { -rotation } else { rotation });
                    for (tex, color, image, value, shift) in [
                        (
                            "_overtex1",
                            "_overcolor1",
                            &hl_up,
                            face.hl_up_color,
                            eyelook::highlight_shift(face.hl_up_y, unclamped_highlights),
                        ),
                        (
                            "_overtex2",
                            "_overcolor2",
                            &hl_down,
                            face.hl_down_color,
                            eyelook::highlight_shift(face.hl_down_y, unclamped_highlights),
                        ),
                    ] {
                        match image {
                            Some(image) => drop(m.textures.insert(tex.into(), placed(image.clone(), shift))),
                            None => drop(m.textures.remove(tex)),
                        }
                        m.colors.insert(color.into(), value);
                    }
                } else if r.name == white_mesh {
                    m.textures.insert("_MainTex".into(), slot(white.clone()));
                }
            }
        }
    }

    /// Mod Bone Implantor: the `(trfSrc, trfDst)` pair of every
    /// `BoneImplantProcess` script a prefab carries.
    fn implant_pairs(&self, donor: &Instance) -> Vec<(Option<usize>, Option<usize>)> {
        let mut pairs = Vec::new();
        for (_, script) in &donor.scripts {
            if self.script_class(script).as_deref() != Some(crate::uncensor_selector::IMPLANT_SCRIPT) {
                continue;
            }
            let Ok(obj) = script.read() else { continue };
            let node = |field: &str| {
                let target = self.game.assets.deref(&script.file, obj.pptr(field)?)?;
                donor.transform_nodes.get(&target.key()).copied()
            };
            pairs.push((node("trfSrc"), node("trfDst")));
        }
        pairs
    }

    /// `SetSkinColor`: a part an uncensor matches to the skin has its own
    /// main texture rebuilt in the skin colours and put on the material
    /// that came with it. The mod's `materialCreate` is the body's create
    /// shader, so the body's own texture is what is built; the sunburn,
    /// the body paint and the nails have no place in it.
    fn color_match(&mut self, card: &Card, inst: &Instance, file: &str, mod_idx: Option<u32>, parts: &[ColorMatch]) {
        let vpath = rk_vfs::index::norm(file);
        for p in parts {
            let main = self.loader.image_by_name(&vpath, &p.main_tex, mod_idx);
            let mask = self.loader.image_by_name(&vpath, &p.color_mask, mod_idx);
            // The plugin gives up on the part when either is missing.
            let (Some(main), Some(mask)) = (main, mask) else {
                self.loader.warn(format!(
                    "colour match {}: {} or {} is missing",
                    p.object, p.main_tex, p.color_mask
                ));
                continue;
            };
            let mut image = compose::body(&compose::BodyInputs {
                size: 2048,
                main: Some(&main),
                color_mask: Some(&mask),
                skin_main: card.body.skin_main_color,
                skin_sub: card.body.skin_sub_color,
                // White leaves the nails alone, and without a sunburn
                // texture its colour never reaches the skin.
                nail: [1.0; 4],
                sunburn: None,
                sunburn_color: [1.0; 4],
                paint_mask: None,
                paints: [Decal::none(), Decal::none()],
            });
            image.name = format!("{}_composed", p.object);
            self.set_skin(inst, &[&p.object], None, image);
        }
    }

    /// Builds the body skin texture from the card.
    /// `uncensor` is the body the Uncensor Selector plugin put in place:
    /// its bundles and asset names stand in for the game's
    /// (`CreateBodyTexture` and `InitBaseCustomTextureBody` transpilers).
    fn skin_body(
        &mut self,
        card: &Card,
        inst: &Instance,
        female: bool,
        uncensor: Option<&rk_vfs::uncensor::Body>,
        censor: bool,
    ) {
        use rk_vfs::uncensor::{defaults, Body};
        let game_body;
        let source = match uncensor {
            Some(data) => data,
            None => {
                game_body = Body::builtin(u8::from(female), "", "");
                &game_body
            }
        };
        let oo_base = rk_vfs::index::norm(&source.oo_base);
        let mm_base = rk_vfs::index::norm(&source.mm_base);
        // A mod's own bundle is asked from that mod.
        let oo_mod = source.mod_idx.filter(|_| oo_base != defaults::OO_BASE);
        let mm_mod = source.mod_idx.filter(|_| mm_base != defaults::MM_BASE);
        let body = &card.body;
        let main = self.loader.image_by_name(&oo_base, &source.main_tex, oo_mod);
        let mask = self.loader.image_by_name(&oo_base, &source.color_mask, oo_mod);
        let create = self.loader.material_by_name(&mm_base, &source.material_create, mm_mod);
        let paint_mask = create
            .as_ref()
            .and_then(|m| m.textures.get("_paintmask"))
            .map(|t| t.image.clone());
        let sunburn = self.item_image(
            "sunburn",
            "ChaFileBody.sunburnId",
            cat::MT_SUNBURN,
            body.sunburn_id,
            "MainAB",
            "SunburnTex",
        );

        let mut images = [None, None];
        let mut layouts = [[0.0f32; 4]; 2];
        for k in 0..2 {
            let Some(&id) = body.paint_id.get(k) else { continue };
            let prop = format!("ChaFileBody.PaintID{}", k + 1);
            images[k] = self.item_image("body paint", &prop, cat::MT_BODY_PAINT, id, "MainAB", "PaintTex");
            let layout_id = body.paint_layout_id.get(k).copied().unwrap_or(0);
            let prop = format!("ChaFileBody.PaintLayoutID{}", k + 1);
            let (Some(item), Some(l)) = (
                self.item("body paint layout", &prop, cat::BODYPAINT_LAYOUT, layout_id),
                body.paint_layout.get(k),
            ) else {
                continue;
            };
            let f = |key: &str| item.float(key).unwrap_or(0.0);
            layouts[k] = [
                lerp(f("CenterX") + f("MoveX"), f("CenterX") - f("MoveX"), l[0]),
                lerp(f("CenterY") + f("MoveY"), f("CenterY") - f("MoveY"), l[1]),
                lerp(-1.0, 1.0, l[2]),
                lerp(f("CenterScale") - f("AddScale"), f("CenterScale") + f("AddScale"), l[3]),
            ];
        }
        let decal = |k: usize| Decal {
            image: images[k].as_deref(),
            color: body.paint_color.get(k).copied().unwrap_or([1.0; 4]),
            layout: layouts[k],
        };
        let main = self.under(kind::BODY_UNDER, main);
        let image = compose::body(&compose::BodyInputs {
            size: 2048,
            main: main.as_deref(),
            color_mask: mask.as_deref(),
            skin_main: body.skin_main_color,
            skin_sub: body.skin_sub_color,
            nail: body.nail_color,
            sunburn: sunburn.as_deref(),
            sunburn_color: body.sunburn_color,
            paint_mask: paint_mask.as_deref(),
            paints: [decal(0), decal(1)],
        });
        let mut draw = self.loader.material_by_name(&mm_base, &source.material, mm_mod);
        // ChangeSettingNip, ChangeSettingAreolaSize, ChangeSettingNipColor,
        // ChangeSettingUnderhair and ChangeSettingUnderhairColor set these on
        // the body's material, as layers the skin shader draws.
        let nip = self.item_image(
            "nipple",
            "ChaFileBody.nipId",
            cat::MT_NIP,
            body.nip_id,
            "MainAB",
            "NipTex",
        );
        let underhair = self.item_image(
            "underhair",
            "ChaFileBody.underhairId",
            cat::MT_UNDERHAIR,
            body.underhair_id,
            "MainAB",
            "UnderhairTex",
        );
        if let Some(m) = draw.as_mut() {
            set_layer(m, "_overtex1", "_overcolor1", nip, body.nip_color);
            set_layer(m, "_overtex2", "_overcolor2", underhair, body.underhair_color);
            m.floats.insert("_nipsize".into(), body.areola_size);
        }
        let image = self.over(kind::BODY_OVER, image);
        // The parts the game censors take the body's own skin, so that a
        // character is uncensored without a mod for it. A part an
        // uncensor already swapped keeps the material that came with it,
        // and with `censor` the game's own censor stays as it is.
        let censored: Vec<&str> = crate::uncensor_selector::BODY_PARTS
            .iter()
            .copied()
            .filter(|name| {
                !censor
                    && inst.renderers.iter().any(|&r| {
                        let r = &self.scene.renderers[r];
                        r.name == *name && r.materials.first().is_some_and(|m| crate::is_censor(&m.shader))
                    })
            })
            .collect();
        let mut skinned = vec!["o_body_a"];
        skinned.extend(censored);
        self.skin_base[0] = Some(self.set_skin(inst, &skinned, draw, image));
        // Body detail: painted lines, the detail normal map and their
        // strength, and the shine sliders.
        let detail = self.skin_detail(
            "body detail",
            "ChaFileBody.detailId",
            cat::MT_BODY_DETAIL,
            body.detail_id,
        );
        self.skin_values(
            inst,
            "o_body_a",
            detail,
            &[
                ("_DetailNormalMapScale", body.detail_power),
                // Status can add gloss on top; a character at rest has none.
                ("_SpecularPower", body.skin_gloss_power),
                ("_SpecularPowerNail", body.nail_gloss_power),
                ("_nip_specular", body.nip_gloss_power),
                ("_linetexon", if body.draw_add_line { 1.0 } else { 0.0 }),
            ],
        );
    }

    /// Builds the face skin texture from the card and the active makeup.
    fn skin_face(&mut self, card: &Card, makeup: &rk_card::Makeup, inst: &Instance, head: &Item) {
        let face = &card.face;
        let image_of =
            |b: &mut Self, ab: &str, tex: &str| b.loader.image_by_name(head.get(ab)?, head.get(tex)?, head.mod_idx);
        let main = image_of(self, "MainTexAB", "MainTex");
        let mask = image_of(self, "ColorMaskAB", "ColorMaskTex");
        let create = self.loader.material_by_name(MATERIALS, "cf_m_face_create", None);
        let paint_mask = create
            .as_ref()
            .and_then(|m| m.textures.get("_paintmask"))
            .map(|t| t.image.clone());
        let vector = |name: &str| {
            create
                .as_ref()
                .and_then(|m| m.colors.get(name))
                .copied()
                .unwrap_or([0.0; 4])
        };

        let cheek = self.item_image(
            "cheek",
            "ChaFileMakeup.cheekId",
            cat::MT_CHEEK,
            makeup.cheek_id,
            "MainAB",
            "CheekTex",
        );
        let lip_line = self.item_image(
            "lip line",
            "ChaFileFace.lipLineId",
            cat::MT_LIPLINE,
            face.lip_line_id,
            "MainAB",
            "LiplineTex",
        );
        let mole = self.item_image(
            "mole",
            "ChaFileFace.moleId",
            cat::MT_MOLE,
            face.mole_id,
            "MainAB",
            "MoleTex",
        );
        let mut paints = [None, None];
        for (k, paint) in paints.iter_mut().enumerate() {
            let Some(&id) = makeup.paint_id.get(k) else { continue };
            *paint = self.item_image(
                "face paint",
                &format!("ChaFileMakeup.PaintID{}", k + 1),
                cat::MT_FACE_PAINT,
                id,
                "MainAB",
                "PaintTex",
            );
        }
        let paint = |k: usize| {
            let l = makeup.paint_layout.get(k).copied().unwrap_or([0.5; 4]);
            Decal {
                image: paints[k].as_deref(),
                color: makeup.paint_color.get(k).copied().unwrap_or([1.0; 4]),
                layout: [
                    lerp(0.25, -0.25, l[0]),
                    lerp(0.3, -0.3, l[1]),
                    lerp(1.0, -1.0, l[2]),
                    lerp(-8.0, 0.7, l[3]),
                ],
            }
        };
        let m = face.mole_layout;
        let main = self.under(kind::FACE_UNDER, main);
        let image = compose::face(&compose::FaceInputs {
            size: 1024,
            main: main.as_deref(),
            color_mask: mask.as_deref(),
            skin_main: card.body.skin_main_color,
            skin_sub: card.body.skin_sub_color,
            cheek: Decal {
                image: cheek.as_deref(),
                color: makeup.cheek_color,
                layout: vector("_tex4uv"),
            },
            lip_line: Decal {
                image: lip_line.as_deref(),
                color: face.lip_line_color,
                layout: vector("_tex5uv"),
            },
            paint_mask: paint_mask.as_deref(),
            paints: [paint(0), paint(1)],
            mole: Decal {
                image: mole.as_deref(),
                color: face.mole_color,
                layout: [
                    lerp(0.25, -0.25, m[0]),
                    lerp(0.3, -0.3, m[1]),
                    0.0,
                    lerp(0.0, 0.7, m[3]),
                ],
            },
        });
        let mut draw = match (head.get("MatAB"), head.get("MatData")) {
            (Some(ab), Some(name)) => self.loader.material_by_name(ab, name, head.mod_idx),
            _ => None,
        };
        let lip = self.item_image(
            "lip",
            "ChaFileMakeup.lipId",
            cat::MT_LIP,
            makeup.lip_id,
            "MainAB",
            "LipTex",
        );
        let shadow = self.item_image(
            "eyeshadow",
            "ChaFileMakeup.eyeshadowId",
            cat::MT_EYESHADOW,
            makeup.eyeshadow_id,
            "MainAB",
            "EyeshadowTex",
        );
        if let Some(m) = draw.as_mut() {
            set_layer(m, "_overtex1", "_overcolor1", lip, makeup.lip_color);
            set_layer(m, "_overtex3", "_overcolor3", shadow, makeup.eyeshadow_color);
            // The second layer is the blush, an expression state: the texture
            // comes with the material and ChangeHohoAkaRate sets its strength
            // to Lerp(0, 0.2, rate). Studio loads cards without their status,
            // so the rate is 0.
            if let Some(c) = m.colors.get_mut("_overcolor2") {
                c[3] = 0.0;
            }
            m.floats.insert("_nip".into(), 0.0);
        }
        let image = self.over(kind::FACE_OVER, image);
        self.skin_base[1] = Some(self.set_skin(inst, &["cf_O_face"], draw, image));
        // Face detail: painted lines, the detail normal map and their
        // strength, cheek and lip shine.
        let detail = self.skin_detail(
            "face detail",
            "ChaFileFace.detailId",
            cat::MT_FACE_DETAIL,
            face.detail_id,
        );
        self.skin_values(
            inst,
            "cf_O_face",
            detail,
            &[
                ("_DetailNormalMapScale", face.detail_power),
                ("_SpecularPower", face.cheek_gloss_power),
                ("_SpecularPowerNail", face.lip_gloss_power),
            ],
        );
    }
}

pub fn build(game: &Game, card: &Card, opts: &BuildOptions) -> Result<Character> {
    let mut b = Builder {
        loader: Loader::new(game),
        game,
        infos: resolve_infos(&card.ext),
        scene: Scene::default(),
        skin_overlays: overlays::skin(&card.ext, opts.outfit.min(card.coordinates.len().saturating_sub(1))),
        cloth_overlays: overlays::clothes(&card.ext, opts.outfit.min(card.coordinates.len().saturating_sub(1))),
        hair_gloss: None,
        physics_sources: Vec::new(),
        body_copies: Vec::new(),
        unlocked: opts.clothing_unlocker
            && crate::clothing_unlocker::unlocked(&card.ext, opts.outfit.min(card.coordinates.len().saturating_sub(1))),
        face: None,
        skin_base: [None, None],
    };
    let female = card.sex() != 0;
    let top = b.scene.add_node("BodyTop", None, Default::default());

    // Skeleton.
    let anim = b
        .loader
        .load_prefab(&mut b.scene, BASE, "p_cf_body_bone", None, Some(top), Part::Body)?;
    let body_root = b
        .scene
        .find(anim.root, "cf_j_root")
        .context("skeleton has no cf_j_root")?;
    let ik = b.read_fbbik(&anim);
    let mut bones = b.scene.name_map(body_root);
    // Root bone of every mesh the game rebinds (`RefObjKey.A_ROOTBONE`).
    let hips = bones.get("cf_j_hips").copied();
    b.physics_source(&anim, false, false, true, crate::physics::Belongs::Body);

    // Head bones hang from the neck end.
    let head_parent = *bones.get("cf_s_head").context("skeleton has no cf_s_head")?;
    let head_bone = b.loader.load_prefab(
        &mut b.scene,
        BASE,
        "p_cf_head_bone",
        None,
        Some(head_parent),
        Part::Head,
    )?;
    let head_bones = b.scene.name_map(head_bone.root);
    b.physics_source(&head_bone, false, false, true, crate::physics::Belongs::Body);

    // Body mesh.
    let body_name = if female { "p_cf_body_00" } else { "p_cm_body_00" };
    let mut body = b
        .loader
        .load_prefab(&mut b.scene, BASE, body_name, None, Some(top), Part::Body)?;
    assign_weights(&mut b.scene, &body, &bones, "cf_j_root", hips);
    b.physics_source(&body, false, false, true, crate::physics::Belongs::Body);

    // Uncensor Selector: another body's meshes on the same renderers.
    let mut uncensor = None;
    // `SetSkinColor` runs once the skin is built: (bundle, mod, parts).
    let mut matched: Vec<(String, Option<u32>, Vec<ColorMatch>)> = Vec::new();
    if opts.uncensor_selector {
        use crate::uncensor_selector as us;
        use rk_vfs::uncensor::defaults as ud;
        let uncensors = game.assets.uncensors();
        let guid = opts
            .uncensor_body
            .clone()
            .or_else(|| us::stored_body(&card.ext, uncensors));
        let chosen = us::stored_parts(&card.ext, uncensors, u8::from(female));
        let voice_rate = rk_card::map_get(&card.parameter, "voiceRate")
            .and_then(rk_card::Value::as_f64)
            .unwrap_or(0.5) as f32;
        // Slider Unlocker is part of every setup this engine stands for.
        let seed = us::seed(
            card.param_int("birthDay").unwrap_or(1),
            card.param_int("personality").unwrap_or(0),
            us::voice_pitch(voice_rate, true),
        );
        // Which prefab the body's meshes come from, so that a part
        // taken from the same one is not copied onto itself.
        let mut from = (BASE.to_string(), body_name.to_string());
        if let Some(data) = us::select(uncensors, guid.as_deref(), u8::from(female), seed) {
            let vpath = rk_vfs::index::norm(&data.oo_base);
            if vpath != BASE || data.asset != body_name {
                match b
                    .loader
                    .load_prefab(&mut b.scene, &vpath, &data.asset, data.mod_idx, Some(top), Part::Body)
                {
                    Ok(donor) => {
                        // Mod Bone Implantor: bones the prefab asks to
                        // have moved into the skeleton.
                        let pairs = b.implant_pairs(&donor);
                        let implants = us::implant(&mut b.scene, &pairs, &bones);
                        us::swap_meshes(&mut b.scene, &mut body, &donor, data, uncensors, implants.as_ref());
                        from = (vpath.clone(), data.asset.clone());
                    }
                    Err(e) => b.loader.warn(format!("uncensor body {}: {e:#}", data.guid)),
                }
            }
            for part in us::unsupported(data) {
                b.loader
                    .warn(format!("uncensor body {}: {part} is not applied", data.guid));
            }
            if !data.color_match.is_empty() {
                matched.push((data.oo_base.clone(), data.mod_idx, data.color_match.clone()));
            }
            uncensor = Some(data);
        }

        // The penis and the balls, each over its own meshes.
        let picks = [
            (
                "penis",
                us::select_part(&uncensors.penises, chosen.penis.as_deref(), seed, ud::PENIS_GUID),
                &us::PENIS_PARTS[..],
            ),
            (
                "balls",
                us::select_part(&uncensors.balls, chosen.balls.as_deref(), seed, ud::BALLS_GUID),
                &us::BALLS_PARTS[..],
            ),
        ];
        for (what, pick, names) in picks {
            let Some(data) = pick else { continue };
            // A body without these meshes, a female's, has nothing to
            // swap; the plugin loads the prefab anyway and finds none.
            if !body
                .renderers
                .iter()
                .any(|&r| names.contains(&b.scene.renderers[r].name.as_str()))
            {
                continue;
            }
            let vpath = rk_vfs::index::norm(&data.file);
            // The game's own part on the prefab the body already came
            // from would copy those meshes onto themselves.
            if (&vpath, &data.asset) != (&from.0, &from.1) {
                match b
                    .loader
                    .load_prefab(&mut b.scene, &vpath, &data.asset, data.mod_idx, Some(top), Part::Body)
                {
                    Ok(donor) => {
                        let pairs = b.implant_pairs(&donor);
                        let implants = us::implant(&mut b.scene, &pairs, &bones);
                        us::swap_parts(&mut b.scene, &body, &donor, names, implants.as_ref());
                    }
                    Err(e) => b.loader.warn(format!("uncensor {what} {}: {e:#}", data.guid)),
                }
            }
            if !data.color_match.is_empty() {
                matched.push((data.file.clone(), data.mod_idx, data.color_match.clone()));
            }
        }

        // `ReloadCharacterBalls` draws the balls only when the card says
        // so; the first of those meshes is the one it switches.
        if !chosen.display_balls {
            let found = body
                .renderers
                .iter()
                .copied()
                .find(|&r| us::BALLS_PARTS.contains(&b.scene.renderers[r].name.as_str()));
            if let Some(r) = found {
                b.scene.renderers[r].enabled = false;
            }
        }
    }

    b.skin_body(card, &body, female, uncensor, opts.censor);
    for (file, mod_idx, parts) in &matched {
        b.color_match(card, &body, file, *mod_idx, parts);
    }

    // Parts the game only shows in special states. `n_dankon` is not
    // among them: the clothes states decide it, in `clothes_state`.
    for name in [
        "o_mnpa",
        "o_mnpb",
        "o_gomu",
        "n_silhouetteTop",
        "n_body_silhouette",
        "n_tang_silhouette",
    ] {
        if let Some(n) = b.scene.find(body.root, name) {
            b.scene.nodes[n].active = false;
        }
    }

    // Body sliders.
    let corrections = if card.body.type_bone != 0 {
        shape::load_corrections(game).ok()
    } else {
        None
    };
    // Kept so the Pushup plugin's bust can be applied once the clothes are known.
    let body_rules = Rules::parse(shape::BODY_RULES)?;
    let body_bones = b.scene.name_map(body_root);
    let mut body_set = match ShapeSet::load(game, BASE, "cf_anmShapeBody", "cf_custombody", None) {
        Ok(mut set) => {
            set.vars.insert("headsize".into(), if female { 1.0 } else { 0.91 });
            set.vars.insert("necksize".into(), if female { 1.0 } else { 0.91 });
            for (i, v) in card.body.shape_value_body.iter().enumerate() {
                set.set(i, *v);
            }
            set.apply_named(&mut b.scene, &body_bones, &body_rules, corrections.as_deref());
            Some(set)
        }
        Err(e) => {
            b.loader.warn(format!("body sliders: {e:#}"));
            None
        }
    };

    // Head: its prefab carries the bone layout for this face type.
    if let Some(item) = b.item_or_default("head", "ChaFileFace.headId", cat::BO_HEAD, card.face.head_id, 0) {
        if let Some(head) = b.part("head", item, None, Part::Head) {
            b.physics_source(&head, true, false, true, crate::physics::Belongs::Body);
            let src = b.scene.name_map(head.root);
            for (name, &dst) in &head_bones {
                if let Some(&s) = src.get(name) {
                    b.scene.nodes[dst].local = b.scene.nodes[s].local;
                }
            }
            b.scene.set_parent(head.root, Some(head_bone.root));
            assign_weights(&mut b.scene, &head, &head_bones, "cf_J_N_FaceRoot", hips);
            // Studio loads cards without their status (`LoadCharaFile`'s
            // `noLoadStatus`), so a character starts with a new status's
            // face; then Eye Control's: it sets the eye slider and, to stop
            // blinking, turns blinking off after it, which forces the open
            // rate to 1.
            let mut start = crate::face::Face::default();
            let mut start_rate = crate::face::Rig::eyes_rate(&start);
            if opts.eye_control && card.ext.contains_key(crate::eye_control::KEY) {
                let eye = crate::eye_control::read(&card.ext);
                start.eyes_open = eye.eye_open_max.clamp(0.0, 1.0);
                start_rate = crate::face::Rig::eyes_rate(&start);
                if eye.disable_blinking {
                    start.blink = false;
                    start_rate = 1.0;
                }
            }
            b.face = Some(b.face_rig(&head, start, start_rate));
            b.face_layer_order(&head, card.face.foreground_eyes, card.face.foreground_eyebrow);
            let outfit = opts.outfit.min(card.coordinates.len().saturating_sub(1));
            let makeup = match card.coordinates.get(outfit) {
                Some(co) if co.enable_makeup => &co.makeup,
                _ => &card.face.base_makeup,
            };
            b.skin_face(card, makeup, &head, item);
            b.eyes(card, &head, opts.slider_unlocker);
            b.face_lines(card, &head);
            let mut hidden = vec![
                "cf_O_namida_S",
                "cf_O_namida_M",
                "cf_O_namida_L",
                "cf_O_gag_eye_00",
                "cf_O_gag_eye_01",
                "cf_O_gag_eye_02",
            ];
            if !card.face.double_tooth {
                hidden.push("cf_O_canine");
            }
            for name in hidden {
                if let Some(n) = b.scene.find(head.root, name) {
                    b.scene.nodes[n].active = false;
                }
            }

            // Face sliders use the key table that ships with this head.
            if let (Some(ab), Some(anm)) = (item.get("MainAB"), item.get("ShapeAnime")) {
                match ShapeSet::load(game, ab, anm, "cf_customhead", item.mod_idx) {
                    Ok(mut set) => {
                        for (i, v) in card.face.shape_value_face.iter().enumerate() {
                            set.set(i, *v);
                        }
                        set.apply(
                            &mut b.scene,
                            head_bone.root,
                            &Rules::parse(shape::HEAD_RULES)?,
                            corrections.as_deref(),
                        );
                    }
                    Err(e) => b.loader.warn(format!("face sliders: {e:#}")),
                }
            }
        }
    }

    // One gloss mask is shared by all hair parts.
    b.hair_gloss = b
        .item_image(
            "hair gloss",
            "ChaFileHair.glossId",
            cat::MT_HAIRGLOSS,
            card.hair.gloss_id,
            "MainTexAB",
            "MainTex",
        )
        .map(|image| TexSlot {
            image,
            scale: [1.0, 1.0],
            offset: [0.0, 0.0],
        });

    // Hair hangs from the upper face bone and keeps its own bones.
    if let Some(&hair_parent) = head_bones.get("cf_J_FaceUp_ty") {
        for (i, hp) in card.hair.parts.iter().enumerate().take(4) {
            let label = format!("hair {}", ["back", "front", "side", "option"][i]);
            let prop = format!("ChaFileHair.{}", cat::HAIR_PROPS[i]);
            // Back, front, side and option hair default to 0, 1, 0 and 0.
            if let Some(item) = b.item_or_default(&label, &prop, cat::HAIR[i], hp.id, [0, 1, 0, 0][i]) {
                if let Some(inst) = b.part(&label, item, Some(hair_parent), Part::Hair(i as u8)) {
                    b.hair_shadows_fix(&inst);
                    b.color_hair(&inst, hp);
                    b.hair_length(&inst, Some(hp.length));
                    b.physics_source(&inst, true, false, !hp.no_shake, crate::physics::Belongs::Hair);
                }
            }
        }
    }

    let outfit = opts.outfit.min(card.coordinates.len().saturating_sub(1));
    let mut accessory_nodes: HashMap<usize, HashMap<String, usize>> = HashMap::new();
    // The worn shoe's list entry and renderers, for Stiletto.
    let mut shoe: Option<(&Item, Vec<usize>)> = None;
    // What the clothes states switch.
    let mut rig = crate::clothes_state::Rig::new(opts.outdoor_shoes);
    rig.add_body(&b.scene, &body);
    rig.son = crate::clothes_state::Son::of_card(card);
    if let Some(on) = opts.visible_son {
        rig.son.switched_on = on;
    }
    let hair_settings = crate::hair_accessories::for_outfit(&card.ext, outfit);
    // Bust size, softness and weight in effect; Pushup may replace them.
    let mut bust = crate::pushup::BodyData::from_body(
        &card.body.shape_value_body,
        card.body.bust_softness,
        card.body.bust_weight,
    );
    if let Some(co) = card.coordinates.get(outfit) {
        if opts.clothes {
            // What the top (and its sub-parts) and the bra say about the
            // pieces around them.
            let mut top_items: Vec<(&Item, Option<usize>)> = Vec::new();
            let mut top_insts: Vec<Instance> = Vec::new();
            let mut bra: Option<(Instance, &Item)> = None;
            let (mut no_bottom, mut no_shorts) = (false, false);
            let mut kinds = crate::pushup::StateKinds::default();
            let blend_shapes = crate::clothing_blend_shape::read(&card.ext);
            for (i, cp) in co.clothes.parts.iter().enumerate().take(9) {
                // Only one of indoor (7) and outdoor (8) shoes is worn.
                if (i == 7 && opts.outdoor_shoes) || (i == 8 && !opts.outdoor_shoes) {
                    // The game loads both pairs and shows one, so the pair
                    // that is not built still has its states.
                    let prop = format!("outfit{outfit}.ChaFileClothes.{}", cat::CLOTHES_PROPS[i]);
                    if let Lookup::Found(item) = b.game.items.lookup(&b.infos, &prop, cat::CLOTHES[i], cp.id) {
                        let loads = |key: &str| item.get(key).is_some_and(|v| v != "0" && v != "p_dummy");
                        if loads("MainAB") && loads("MainData") {
                            rig.worn.state_type[i] = state_type(item);
                        }
                    }
                    continue;
                }
                // One-piece tops replace the bottom, one-piece bras the shorts.
                if (i == 1 && no_bottom) || (i == 3 && no_shorts) {
                    continue;
                }
                let label = format!("outfit{outfit}.{}", rk_card::CLOTHES_KIND_NAMES[i]);
                let prop = format!("outfit{outfit}.ChaFileClothes.{}", cat::CLOTHES_PROPS[i]);
                let Some(item) = b.item_or_default(&label, &prop, cat::CLOTHES[i], cp.id, 0) else {
                    continue;
                };
                let inst = b.part(&label, item, Some(top), Part::Clothes(i as u8));
                if i >= 7 {
                    let renderers = inst.as_ref().map(|inst| inst.renderers.clone());
                    shoe = Some((item, renderers.unwrap_or_default()));
                }
                let pattern_prop = format!("outfit{outfit}.ChaFileClothes.{}Pattern", cat::CLOTHES_PROPS[i]);
                if let Some(inst) = &inst {
                    if opts.clothing_blend_shape {
                        b.clothing_blend_shape(inst, &blend_shapes, outfit, item.category, i);
                    }
                    // The game copies DynamicBone roots onto the body
                    // skeleton for tops and bottoms only.
                    b.physics_source(inst, true, i < 2, true, crate::physics::Belongs::Clothes);
                    if let Some(source) = b.physics_sources.last_mut() {
                        source.object = Some(crate::overlays::MAIN_IDS[i]);
                    }
                    assign_weights(&mut b.scene, inst, &bones, "cf_j_root", hips);
                    rig.add_piece(&b.scene, i, inst);
                    let sources = compose::color_sources(None);
                    b.dress(
                        inst,
                        item,
                        &cp.color_info,
                        sources,
                        &pattern_prop,
                        overlays::MAIN_IDS[i],
                    );
                    let prop = format!("outfit{outfit}.ChaFileClothes.{}", cat::CLOTHES_PROPS[i]);
                    b.emblems(inst, cp, &prop);
                }
                let one_piece = b.list_int(item, "Coordinate") == Some(2);
                let has_top = inst
                    .as_ref()
                    .is_some_and(|inst| inst.nodes.iter().any(|&n| b.scene.nodes[n].name == "n_top_a"));
                let state = state_type(item);
                // `infoClothes` is set once the piece's object is loaded; a
                // sailor or jacket top loads an empty object.
                let kind = item.int("Kind").unwrap_or(0);
                // `UpdateClothesSiru` lays the liquid levels on the
                // piece's own renderers, except for a sailor or jacket
                // top, whose parts take them instead; those are noted
                // where the parts are loaded.
                if let Some(inst) = &inst {
                    if i < crate::siru::CLOTHES_KINDS && !(i == 0 && (kind == 1 || kind == 2)) {
                        let renderers = b.liquid_renderers(inst);
                        rig.add_liquid(i, renderers);
                    }
                }
                if inst.is_some() || (i == 0 && (kind == 1 || kind == 2)) {
                    rig.worn.state_type[i] = state;
                }
                if i == 1 && inst.is_some() {
                    // `ChangeClothesBotAsync`: the list says whether the
                    // bottom hides the shorts; lists from before that
                    // column go by the item's number.
                    const HIDE_SHORTS: [i32; 13] = [2, 3, 9, 16, 22, 25, 28, 29, 30, 32, 34, 35, 45];
                    rig.worn.hide_shorts_bot = match b.list_int(item, "HideShorts") {
                        Some(hide) => hide == 1,
                        None => item.mod_idx.is_none() && HIDE_SHORTS.contains(&cp.id),
                    };
                }
                match i {
                    0 => {
                        kinds.top = state;
                        kinds.not_bra = b.list_int(item, "NotBra") == Some(1);
                    }
                    1 => {
                        kinds.bottom = state;
                        kinds.bottom_has_top = has_top;
                    }
                    2 => {
                        kinds.bra = state;
                        kinds.not_shorts = one_piece;
                    }
                    3 => {
                        kinds.shorts = state;
                        kinds.shorts_have_top = has_top;
                    }
                    _ => {}
                }
                match (i, inst) {
                    (0, inst) => {
                        no_bottom = one_piece;
                        top_items.push((item, None));
                        top_insts.extend(inst);
                    }
                    (2, Some(inst)) => {
                        no_shorts = one_piece;
                        bra = Some((inst, item));
                    }
                    _ => {}
                }
                // Sailor and jacket tops are assembled from three sub-parts.
                if i == 0 && (kind == 1 || kind == 2) {
                    let (cats, names) = if kind == 1 {
                        (
                            [cat::CPO_SAILOR_A, cat::CPO_SAILOR_B, cat::CPO_SAILOR_C],
                            ["ClothesSailorSubA", "ClothesSailorSubB", "ClothesSailorSubC"],
                        )
                    } else {
                        (
                            [cat::CPO_JACKET_A, cat::CPO_JACKET_B, cat::CPO_JACKET_C],
                            ["ClothesJacketSubA", "ClothesJacketSubB", "ClothesJacketSubC"],
                        )
                    };
                    for k in 0..3 {
                        let id = co.clothes.sub_parts_id.get(k).copied().unwrap_or(0);
                        let sub_label = format!("{label}.sub{k}");
                        let sub_prop = format!("outfit{outfit}.ChaFileClothes.{}", names[k]);
                        // The game's default sub-parts: sailor tops {0, 0, 1},
                        // jacket tops {0, 1, 1}.
                        let default = if kind == 1 { [0, 0, 1][k] } else { [0, 1, 1][k] };
                        let Some(sub) = b.item_or_default(&sub_label, &sub_prop, cats[k], id, default) else {
                            continue;
                        };
                        // ChangeClothesTopParts: the first sailor part, or
                        // either of the first two jacket parts, decides notBra.
                        let sub_no_bra = b.list_int(sub, "NotBra") == Some(1);
                        match (kind, k) {
                            (1, 0) => kinds.not_bra = sub_no_bra,
                            (2, 0) => kinds.not_bra = sub_no_bra,
                            (2, 1) => kinds.not_bra |= sub_no_bra,
                            _ => {}
                        }
                        if let Some(inst) = b.part(&sub_label, sub, Some(top), Part::Clothes(0)) {
                            if opts.clothing_blend_shape {
                                b.clothing_blend_shape(&inst, &blend_shapes, outfit, sub.category, 0);
                            }
                            b.physics_source(&inst, true, true, true, crate::physics::Belongs::Clothes);
                            if let Some(source) = b.physics_sources.last_mut() {
                                source.object = Some(crate::overlays::SUB_IDS[k]);
                            }
                            assign_weights(&mut b.scene, &inst, &bones, "cf_j_root", hips);
                            rig.add_part(&b.scene, k, &inst);
                            // A sailor top's liquid goes on its one part,
                            // a jacket top's on its first two
                            // (`GetCustomClothesSubComponent`).
                            if (kind == 1 && k == 0) || (kind == 2 && k < 2) {
                                let renderers = b.liquid_renderers(&inst);
                                rig.add_liquid(0, renderers);
                            }
                            // Sub-parts share the top's colours.
                            let sources = compose::color_sources(Some(k));
                            b.dress(&inst, sub, &cp.color_info, sources, &pattern_prop, overlays::SUB_IDS[k]);
                            // Their emblems are the top's, under the top's name.
                            let top_prop = format!("outfit{outfit}.ChaFileClothes.{}", cat::CLOTHES_PROPS[0]);
                            b.emblems(&inst, cp, &top_prop);
                            top_insts.push(inst);
                        }
                        top_items.push((sub, Some(k)));
                    }
                }
            }
            // Every piece starts fully on, as a character added in Studio.
            rig.worn.not_bot = no_bottom;
            rig.worn.not_bra = kinds.not_bra;
            rig.worn.not_shorts = no_shorts;
            rig.apply_visibility(&mut b.scene, &[crate::clothes_state::ON; crate::clothes_state::KINDS]);
            b.hide_under_top(&body, &top_items, &top_insts, bra.as_ref());

            // Pushup: the bust of a worn top or bra replaces the card's.
            let base = bust;
            let pushed = crate::pushup::for_outfit(&card.ext, outfit, &base)
                .and_then(|o| crate::pushup::bust(&base, &o, kinds.top_kind(), kinds.bra_kind()));
            if let (Some(pushed), Some(set)) = (pushed, body_set.as_mut()) {
                for (i, v) in pushed.shapes().into_iter().enumerate() {
                    set.set(crate::pushup::SHAPE_FIRST + i, v);
                }
                set.apply_named(&mut b.scene, &body_bones, &body_rules, corrections.as_deref());
                bust = pushed;
            }
        }

        if opts.accessories {
            // Attachment points are skeleton and head nodes named a_n_*.
            let mut points = bones.clone();
            for (k, v) in &head_bones {
                points.entry(k.clone()).or_insert(*v);
            }
            // Old MoreAccessories data continues the list after slot twenty.
            let extra = crate::more_accessories::read(&card.ext)
                .remove(&outfit)
                .unwrap_or_default();
            let extra = if co.accessory.parts.len() > 20 {
                &[][..]
            } else {
                &extra[..]
            };
            let slots = co
                .accessory
                .parts
                .iter()
                .take(20.max(co.accessory.parts.len()))
                .chain(extra.iter());
            let slot_count = 20.max(co.accessory.parts.len()) + extra.len();
            let states_off = if opts.accessory_states {
                let bound = crate::accessory_states::coordinate(&card.ext, outfit, card.coordinates.len());
                crate::accessory_states::hidden(&bound, opts.outdoor_shoes as u8, slot_count)
            } else {
                Vec::new()
            };
            for (i, acc) in slots.enumerate() {
                if acc.is_empty() {
                    continue;
                }
                let label = format!("outfit{outfit}.accessory{i}");
                let prop = format!("outfit{outfit}.accessory{i}.ChaFileAccessory.PartsInfo.id");
                let Some(item) = b.item(&label, &prop, acc.kind, acc.id) else {
                    continue;
                };
                let item_parent = item.raw("Parent").unwrap_or("").trim();
                let Some(inst) = b.part(&label, item, Some(top), Part::Accessory(i as u16)) else {
                    continue;
                };
                if item_parent == "null" {
                    // Skinned accessory: follows the head bones.
                    assign_weights(&mut b.scene, &inst, &head_bones, "cf_J_N_FaceRoot", hips);
                }
                let key = if acc.parent_key.is_empty() {
                    item_parent
                } else {
                    acc.parent_key.as_str()
                };
                if item_parent != "0" && item_parent != "null" {
                    match points.get(key) {
                        Some(&p) => b.scene.set_parent(inst.root, Some(p)),
                        None => b.loader.warn(format!("{label}: no attachment point {key}")),
                    }
                }
                if opts.accessory_clothes {
                    b.accessory_clothes(&inst, &bones, hips);
                }
                if states_off.contains(&i) {
                    // `SetAccessoryState(slot, false)`: the object is
                    // switched off, not removed.
                    b.scene.nodes[inst.root].active = false;
                }
                accessory_nodes.insert(i, b.scene.name_map(inst.root));
                b.physics_source(&inst, true, false, !acc.no_shake, crate::physics::Belongs::Accessory);
                if let Some(source) = b.physics_sources.last_mut() {
                    source.slot = Some(i);
                }
                b.color_accessory(&inst, acc, card.hair.parts.first(), hair_settings.get(&i).copied());
                b.hair_length(&inst, hair_settings.get(&i).map(|h| h.hair_length));
                b.hair_shadows_fix(&inst);
                for (k, name) in ["N_move", "N_move2"].iter().enumerate() {
                    let Some(node) = b.scene.find(inst.root, name) else {
                        continue;
                    };
                    let (Some(p), Some(r), Some(s)) =
                        (acc.add_move.get(k, 0), acc.add_move.get(k, 1), acc.add_move.get(k, 2))
                    else {
                        continue;
                    };
                    let local = &mut b.scene.nodes[node].local;
                    local.pos = Vec3::from(*p) * 0.01;
                    local.rot = euler_deg(r[0], r[1], r[2]);
                    local.scale = Vec3::from(*s);
                }
            }
        }
    }

    // Keep grafted bones findable.
    for (k, v) in b.scene.name_map(body_root) {
        bones.entry(k).or_insert(v);
    }

    // Plugin bone edits go on top of the slider result.
    // The game narrows the hands first, so ABMX builds on that.
    if female {
        crate::hand_size::correct(&mut b.scene, &bones);
    }
    let mut mods = crate::abmx::read(&card.ext);
    if opts.pregnancy {
        crate::pregnancy::combine(&mut mods, &crate::pregnancy::effects(&card.ext));
    }
    if !mods.is_empty() {
        crate::abmx::apply(&mut b.scene, &mods, outfit, &bones, &accessory_nodes);
    }
    if female && opts.studio {
        crate::hand_size::studio_copy(&mut b.scene, &bones, &mods);
    }
    // Stiletto turns the bones after the animation, each frame.
    let mut heels = None;
    if let Some((item, renderers)) = shoe.filter(|_| opts.stiletto) {
        use crate::stiletto as st;
        let config = st::Config::load(&game.assets.index.game_dir);
        let heel = config.heel(item.name());
        // No animation is played, so the lookup has no controller or clip.
        match st::flags_for(&config.flag_files, None, None) {
            Ok(flags) => {
                st::warp_shoe(&mut b.scene, &renderers, &heel, flags);
                // `objBodyBone.transform.parent`: what holds `cf_j_root`.
                let holder = b.scene.nodes[body_root].parent.unwrap_or(anim.root);
                let worn = st::Heels::new(&b.scene, &bones, holder, heel, &config, flags);
                worn.apply(&mut b.scene, &bones);
                heels = Some(worn);
            }
            Err(pattern) => b.loader.warn(format!(
                "Stiletto: flags pattern `{pattern}` is not supported, heels are not applied"
            )),
        }
    }
    // ChaControl.UpdateVisible: tongue state 0 shows the face's tongue
    // (S_TongueF) and hides the body's (S_TongueB); mouth pattern 21 sets
    // state 1, the other way round.
    let tongue = |part: Part| -> Vec<usize> {
        b.scene
            .renderers
            .iter()
            .enumerate()
            .filter(|(_, r)| r.name == "o_tang" && r.part == part)
            .map(|(i, _)| i)
            .collect()
    };
    let tongues = [tongue(Part::Head), tongue(Part::Body)];
    if let Some(face) = &mut b.face {
        face.tongues = tongues;
        face.keep_iris_places(&b.scene);
        face.show(&mut b.scene, None);
    } else {
        for &r in &tongues[1] {
            b.scene.renderers[r].enabled = false;
        }
    }
    // MaterialEditor overrides come last, so they win over everything the
    // card's own settings produced.
    let mut targets = crate::material_editor::Targets::new();
    for (ri, r) in b.scene.renderers.iter().enumerate() {
        use crate::material_editor::Owner;
        let owner = match r.part {
            Part::Clothes(k) => Owner::Clothing(k as i64),
            Part::Accessory(k) => Owner::Accessory(k as i64),
            Part::Hair(k) => Owner::Hair(k as i64),
            Part::Body | Part::Head => continue,
        };
        targets.entry(owner).or_default().push(ri);
    }
    let assets = &b.loader.game.assets;
    let shaders = |name: &str| {
        let info = assets.me_shader(name)?;
        Some(crate::material_editor::Shader {
            draw: crate::scene::DrawState::from_passes(&info.passes)?,
            queue: info.queue,
            props: info.props.clone(),
            defaults: info
                .defaults
                .iter()
                .filter_map(|(n, kind, v)| Some((n.clone(), crate::material_editor::ShaderDefault::parse(kind, v)?)))
                .collect(),
        })
    };
    // The component copies the body material when the item starts, before
    // MaterialEditor has changed the body.
    if opts.copy_body_material {
        let body = b
            .scene
            .renderers
            .iter()
            .find(|r| r.part == Part::Body && r.name == "o_body_a")
            .and_then(|r| r.materials.first().cloned());
        if let Some(body) = body {
            for (targets, queue) in std::mem::take(&mut b.body_copies) {
                for r in targets {
                    if let Some(m) = b.scene.renderers[r].materials.first_mut() {
                        *m = crate::copy_body_material::copy(&body, queue);
                    }
                }
            }
        }
    }

    let material_edits = crate::material_editor::apply(&mut b.scene, &card.ext, outfit, &targets, &shaders);

    if let Some(fangs) = opts.edit_fangs.then(|| crate::edit_fangs::read(&card.ext)).flatten() {
        for r in b.scene.renderers.iter_mut() {
            if r.part == Part::Head && r.name == crate::edit_fangs::RENDERER {
                if let Some(mesh) = crate::edit_fangs::apply(&r.mesh, fangs) {
                    r.mesh = std::sync::Arc::new(mesh);
                }
            }
        }
    }

    // Physics runs on the finished skeleton, as the game's does once a
    // character is loaded.
    let bone_edits = crate::dynamic_bone_editor::for_outfit(&card.ext, outfit);
    let collider_plugin = opts.colliders.then(|| {
        let bust = card
            .body
            .shape_value_body
            .get(crate::colliders::BREAST_SIZE)
            .copied()
            .unwrap_or(0.0);
        let switches = opts
            .collider_switches
            .unwrap_or_else(|| crate::colliders::read(&card.ext));
        (switches, bust)
    });
    let dbde_edits = if opts.dbde {
        crate::dbde::for_outfit(&card.ext, outfit)
    } else {
        Vec::new()
    };
    let mut physics = b.read_physics(top, &bones, &bone_edits, &dbde_edits, collider_plugin);
    if opts.physics {
        physics.start(&mut b.scene);
        // Advanced Skirt Physics is fitted to the body as built.
        physics.fit_cloth(&b.scene, &bones, opts.advanced_skirts);
        physics.setup_bust(&mut b.scene, bust.softness, bust.weight, bust.size);
        physics.settle(&mut b.scene, opts.settle_frames, opts.frame_time);
    }

    if opts.invisible_body && !crate::invisible_body::visible(&card.ext) {
        crate::invisible_body::hide(&mut b.scene, top);
    }

    let ramp = ramp_image(&mut b.loader, DEFAULT_RAMP);
    if ramp.is_none() {
        b.loader.warn("shading ramp not found, using a hard edge");
    }
    let loaded: Vec<crate::scene::Trs> = b.scene.nodes.iter().map(|n| n.local).collect();
    let ik_rig = ik
        .as_ref()
        .and_then(|s| crate::ik::Rig::new(s, &b.scene, &loaded, anim.root, top));
    Ok(Character {
        ik_rig,
        hands: [None, None],
        face: b.face.take(),
        skin_base: std::mem::take(&mut b.skin_base),
        skin_shown: Default::default(),
        scene: b.scene,
        top,
        animator: anim.root,
        parameters: crate::motion::Parameters::of(&card.body.shape_value_body),
        clothes: rig,
        heels,
        ik,
        bones,
        ramp,
        material_edits,
        physics,
        warnings: b.loader.warnings,
    })
}

impl Character {
    /// Brings the clothes to the given states without building again: the
    /// objects each state shows, the masks under the top, and the heels,
    /// which come off with the shoes. Bones the heels moved are put back
    /// as they were first, so an animation is to be applied again after.
    /// The physics of a piece that comes off stops and its bones go back
    /// to rest, as Unity's `OnDisable` does when the game hides it.
    pub fn set_clothes(&mut self, states: &crate::clothes_state::States) {
        self.clothes.apply(&mut self.scene, states);
        self.physics.follow_objects(&mut self.scene);
        if let Some(heels) = &self.heels {
            heels.reset(&mut self.scene);
        }
        self.wear_heels(states);
    }

    /// Lays the liquid levels on the skin without building again, as
    /// `ChaControl.UpdateSiru` does, and on the clothes that take them
    /// where it is asked for. The game always wets the clothes with the
    /// body; here it is a choice, and `clothes` false leaves them dry.
    pub fn set_siru(&mut self, levels: &crate::siru::Levels, clothes: bool) {
        crate::siru::apply(&mut self.scene, levels);
        let worn = if clothes { *levels } else { [0; crate::siru::PARTS] };
        let renderers = self.clothes.liquid_renderers();
        crate::siru::dress(&mut self.scene, &worn, &renderers);
    }

    /// Whether the body carries the mask that liquid levels are read
    /// against; without it nothing of them shows.
    pub fn liquid_mask(&self) -> bool {
        crate::siru::masked(&self.scene)
    }

    /// Takes the heels off and, when the shoes are on, puts them on again
    /// on top of the bones as they are now.
    pub fn refresh_heels(&mut self, states: &crate::clothes_state::States) {
        if let Some(heels) = &self.heels {
            heels.reset(&mut self.scene);
        }
        self.wear_heels(states);
    }

    /// Whether the worn shoes are fully on, which is when Stiletto acts.
    fn shoes_on(&self, states: &crate::clothes_state::States) -> bool {
        use crate::clothes_state::{ON, SHOES_INDOOR, SHOES_OUTDOOR};
        let kind = if self.clothes.outdoor_shoes {
            SHOES_OUTDOOR
        } else {
            SHOES_INDOOR
        };
        states[kind] == ON
    }

    /// Puts the heels on top of the bones as they are now, when the shoes
    /// are on. Call after an animation wrote the bones.
    pub fn wear_heels(&mut self, states: &crate::clothes_state::States) {
        if !self.shoes_on(states) {
            return;
        }
        if let Some(heels) = &self.heels {
            heels.apply(&mut self.scene, &self.bones);
        }
    }

    /// Lets the physics settle on a new pose for `frames` frames of `dt`
    /// seconds, as the game's frames do: `pose` writes the pose each frame.
    /// With Advanced Skirt Physics on, the pose is reached over the first
    /// `EASE_FRAMES` frames from where the bones were before.
    pub fn settle_pose(&mut self, frames: u32, dt: f32, mut pose: impl FnMut(&mut Character)) {
        let from: Vec<crate::scene::Trs> = self.scene.nodes.iter().map(|n| n.local).collect();
        for frame in 0..frames {
            self.physics.begin_frame(&mut self.scene);
            pose(self);
            let amount = (frame + 1) as f32 / (crate::physics::EASE_FRAMES + 1) as f32;
            self.physics.ease(&mut self.scene, &from, amount);
            // The eased bones pass through poses no body takes (each
            // turns on its own): the cloth nets start again once the
            // pose is reached and move into it themselves.
            if frame == crate::physics::EASE_FRAMES {
                self.physics.restart_cloth();
            }
            self.physics.end_frame(&mut self.scene, dt);
        }
    }

    /// Writes one frame of a motion to the bones: the heels come off, the
    /// motion writes the bones it names, and the heels go on again.
    pub fn animate(&mut self, motion: &crate::motion::Bound, phase: f32, states: &crate::clothes_state::States) {
        self.pose_frame(Some((motion, phase)), &[], None, states);
    }

    /// Poses one frame: the heels come off, the motion (if any) writes the
    /// bones it names, the edits made by hand go on top of that
    /// (`crate::pose`), IK moves the body to the layer's targets while it
    /// is on (`crate::ik`), and the heels go on again.
    /// Lays the skin effects at `levels` over the body and the face, as
    /// KK_SkinEffects does; a part whose effects did not change keeps its
    /// texture.
    pub fn show_skin_effects(&mut self, levels: &crate::skin_effects::Levels) {
        use crate::skin_effects::{paint, EFFECTS};
        for (part, face) in [(0, false), (1, true)] {
            let changed = EFFECTS
                .iter()
                .zip(levels.iter().zip(&self.skin_shown))
                .any(|(e, (a, b))| e.face == face && a != b);
            let Some((worn, base)) = self.skin_base[part].as_ref().filter(|_| changed) else {
                continue;
            };
            let image = paint(base, levels, face).map_or_else(|| base.clone(), Arc::new);
            for &r in worn {
                if let Some(slot) = self.scene.renderers[r]
                    .materials
                    .first_mut()
                    .and_then(|m| m.textures.get_mut("_MainTex"))
                {
                    slot.image = image.clone();
                }
            }
        }
        self.skin_shown = *levels;
    }

    pub fn pose_frame(
        &mut self,
        motion: Option<(&crate::motion::Bound, f32)>,
        edits: &[(usize, crate::pose::BoneEdit)],
        layer: Option<&crate::pose::Layer>,
        states: &crate::clothes_state::States,
    ) {
        let ik = layer.filter(|l| l.ik_on);
        if let (Some(rig), Some(_)) = (&self.ik_rig, ik) {
            rig.fix(&mut self.scene);
        }
        if let Some(heels) = &self.heels {
            heels.reset(&mut self.scene);
        }
        if let Some((motion, phase)) = motion {
            motion.apply(&mut self.scene, phase);
        }
        // The hand patterns over the animation's fingers, held at their
        // first frame: the clips are still to within 0.004, and the game's
        // hand Animator is wherever it got to since the pattern was chosen.
        if let Some(layer) = layer {
            for (clip, pattern) in self.hands.iter().zip(layer.hands) {
                if let Some(clip) = clip.as_ref().filter(|c| pattern != 0 && c.pattern == pattern) {
                    clip.apply(&mut self.scene, 0.0);
                }
            }
        }
        crate::pose::apply(&mut self.scene, edits);
        // IK over the animation and FK, before the heels go on.
        if let (Some(rig), Some(layer)) = (self.ik_rig.as_mut(), ik) {
            rig.solve(&mut self.scene, self.top, layer);
        }
        self.wear_heels(states);
    }
}

/// A clothing item's `StateType`, as `ChaControl` reads it: a list without
/// the column gives "0" (`ListInfoBase.GetInfo`), so the piece has the
/// usual on, half off and off states.
fn state_type(item: &Item) -> Option<i32> {
    match item.get("StateType") {
        None => Some(0),
        Some(_) => item.int("StateType"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_sheets_list_number_bundle_and_texture() {
        use rk_unityfs::{UObject, UValue};
        let row = |cells: &[&str]| {
            UValue::Object(Box::new(UObject {
                ty: "Param".into(),
                fields: vec![(
                    "list".into(),
                    UValue::Array(cells.iter().map(|c| UValue::Str((*c).into())).collect()),
                )],
            }))
        };
        let sheet = UObject {
            ty: "ExcelData".into(),
            fields: vec![(
                "list".into(),
                UValue::Array(vec![
                    row(&["number", "name", "bundle", "file"]),
                    row(&["0", "day", "studio/filter/00.unity3d", "Lut_A"]),
                    row(&["x", "broken"]),
                    row(&["7", "flat", "studio/filter/00.unity3d", "Lut_B"]),
                ]),
            )],
        };
        assert_eq!(
            filter_rows(&sheet),
            [
                (0, "studio/filter/00.unity3d".to_owned(), "Lut_A".to_owned()),
                (7, "studio/filter/00.unity3d".to_owned(), "Lut_B".to_owned()),
            ]
        );
    }

    #[test]
    fn a_detail_texture_keeps_the_materials_placement() {
        let image = Arc::new(Image {
            name: "cf_body_00_n".into(),
            width: 1,
            height: 1,
            rgba: vec![128, 128, 255, 128],
            srgb: false,
            repeat: true,
        });
        let mut m = crate::scene::MaterialInst::default();
        m.tex_st.insert("_NormalMapDetail".into(), [2.0, 3.0, 0.5, 0.25]);
        set_detail(&mut m, "_NormalMapDetail", Some(image.clone()));
        set_detail(&mut m, "_LineMask", Some(image));
        let normal = &m.textures["_NormalMapDetail"];
        assert_eq!((normal.scale, normal.offset), ([2.0, 3.0], [0.5, 0.25]));
        assert_eq!(normal.image.name, "cf_body_00_n");
        // A property the material does not place sits at the full square.
        let lines = &m.textures["_LineMask"];
        assert_eq!((lines.scale, lines.offset), ([1.0, 1.0], [0.0, 0.0]));
        // An item without the texture clears what the material had.
        set_detail(&mut m, "_NormalMapDetail", None);
        assert!(!m.textures.contains_key("_NormalMapDetail"));
    }
}
