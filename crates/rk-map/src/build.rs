// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! A map's scene made ready to draw: every shown mesh renderer placed in
//! the world with its materials, and the sun, ambient light and fog of a
//! time of day.

use crate::scene::open;
use crate::sky::{self, Compile, SkyShader};
use crate::unity::{self, Light, LightmapSettings, RenderSettings, SunLightInfo, TimeOfDay};
use anyhow::{Context, Result};
use glam::{EulerRot, Mat3, Mat4, Quat, Vec3};
use rk_chara::scene::Image;
use rk_render::map::{
    pick_probe, Blend, Detail, Fog, Lightmap, MapImage, MapLight, MapMaterial, MapMesh, MapObject, MapScene,
    Reflection, ReflectionProbe, SmoothnessFrom, Surface,
};
use rk_render::sky::{Sky, SkyTexture, HDR_PLAIN};
use rk_unityfs::objects::{GameObject, Material, Mesh, MeshFilter, Renderer, Transform};
use rk_unityfs::{class, PPtr};
use rk_vfs::assets::{AssetFile, LoadedBundle, ObjRef};
use rk_vfs::Game;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// The lowest and highest corner of a box in the world.
type Bounds = ([f32; 3], [f32; 3]);

/// What was left out while building, by reason, with how often.
pub type Skipped = HashMap<String, usize>;

/// Unity's gamma to linear conversion of one channel.
pub fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear3(c: [f32; 4]) -> [f32; 3] {
    [to_linear(c[0]), to_linear(c[1]), to_linear(c[2])]
}

/// A map's objects and light, for the time of day `time`. With
/// `compressed`, the game's BC textures are kept as stored, for a GPU that
/// draws them. `vanish` is the text of the map's table of walls that hide
/// (see `vanish::table`).
pub fn build(
    game: &Game,
    bundle: &str,
    time: TimeOfDay,
    compressed: bool,
    vanish: Option<&str>,
) -> Result<(MapScene, Skipped)> {
    build_times(game, bundle, &[time], compressed, vanish)?
        .pop()
        .context("no time of day was built")
}

/// The map at each of `times`, as `build` makes it, with the bundle opened
/// once: what the times share (meshes, materials, textures, cubemaps) is
/// read once and is the same `Arc` in each scene.
pub fn build_times(
    game: &Game,
    bundle: &str,
    times: &[TimeOfDay],
    compressed: bool,
    vanish: Option<&str>,
) -> Result<Vec<(MapScene, Skipped)>> {
    let rows = vanish.map(crate::vanish::parse).unwrap_or_default();
    let loaded = open(game, bundle)?;
    let mut b = Builder {
        game,
        loaded: &loaded,
        meshes: HashMap::new(),
        materials: HashMap::new(),
        images: HashMap::new(),
        cubes: HashMap::new(),
        skipped: Skipped::new(),
        compressed,
        dome: None,
    };
    let mut out = Vec::new();
    for &time in times {
        b.dome = None;
        let scene = build_time(&mut b, &rows, time)?;
        out.push((scene, std::mem::take(&mut b.skipped)));
    }
    Ok(out)
}

/// The map at one time of day, with the caches of `b`.
fn build_time(b: &mut Builder, rows: &[crate::vanish::Row], time: TimeOfDay) -> Result<MapScene> {
    let loaded = b.loaded;
    let mut volumes: Vec<rk_render::map::VanishVolume> = Vec::new();
    let mut objects = Vec::new();
    let mut sun: Option<SunLightInfo> = None;
    let mut lights: Vec<(Light, Mat4)> = Vec::new();
    let mut settings: Option<RenderSettings> = None;
    let mut sky: Option<Sky> = None;
    let mut reflection: Option<Reflection> = None;
    let mut lightmaps: Vec<Lightmap> = Vec::new();
    let mut probes: Vec<ReflectionProbe> = Vec::new();
    let mut mirrors = Vec::new();
    // The bounds each object looks for its reflection probe with; `None`
    // for a renderer that uses none.
    let mut probe_bounds: Vec<Option<Bounds>> = Vec::new();
    for file in &loaded.files {
        let sf = &file.sf;
        // The hierarchy of this file: transforms by id, game objects by id.
        let mut transforms: HashMap<i64, Transform> = HashMap::new();
        let mut transform_of: HashMap<i64, i64> = HashMap::new();
        for obj in sf.objects_of(class::TRANSFORM) {
            if let Ok(t) = sf.read(obj).and_then(|o| Transform::read(&o)) {
                transform_of.insert(t.game_object.path_id, obj.path_id);
                transforms.insert(obj.path_id, t);
            }
        }
        let mut objects_by_id: HashMap<i64, GameObject> = HashMap::new();
        for obj in sf.objects_of(class::GAME_OBJECT) {
            if let Ok(go) = sf.read(obj).and_then(|o| GameObject::read(&o)) {
                objects_by_id.insert(obj.path_id, go);
            }
        }
        let tree = Tree {
            transforms: &transforms,
            transform_of: &transform_of,
            objects: &objects_by_id,
        };
        // The walls that hide: each row's volume, and the game objects it
        // hides with all under them.
        let mut hidden_by: HashMap<i64, Vec<usize>> = HashMap::new();
        for row in rows {
            // The camera only reacts to colliders on the Map layer
            // (`CameraControl.mapLayer`); maps keep same-named twins on
            // other layers, which never trigger.
            let mut indices = Vec::new();
            for collider in tree
                .find_all(&row.collider)
                .into_iter()
                .filter(|id| tree.layer(*id) == MAP_LAYER)
            {
                if let Some(volume) = volume_of(sf, &tree, collider, b) {
                    volumes.push(volume);
                    indices.push(volumes.len() - 1);
                }
            }
            if indices.is_empty() {
                continue;
            }
            for name in &row.hides {
                if let Some(go) = tree.find(name) {
                    for id in tree.with_descendants(go) {
                        hidden_by.entry(id).or_default().extend(&indices);
                    }
                }
            }
        }
        // Which renderer reads which lightmap: LightmapPrefab puts it back
        // on them when the scene starts, over what the renderers say.
        let mut assigned: HashMap<i64, (i32, [f32; 4])> = HashMap::new();
        for obj in sf.objects_of(class::MONO_BEHAVIOUR) {
            let Ok(o) = sf.read(obj) else { continue };
            match b.script(file, &o).as_str() {
                "SunLightInfo" => sun = Some(SunLightInfo::read(&o)),
                "LightmapPrefab" => {
                    for u in unity::lightmap_prefab(&o)
                        .into_iter()
                        .filter(|u| u.renderer.file_id == 0)
                    {
                        assigned.insert(u.renderer.path_id, (u.index, u.scale_offset));
                    }
                }
                _ => {}
            }
        }
        if let Some(obj) = sf.objects_of(unity::class::LIGHTMAP_SETTINGS).next() {
            let settings = LightmapSettings::read(&sf.read(obj)?);
            for l in &settings.lightmaps {
                let light = b.image(file, l.light);
                let direction = (settings.mode == 1).then(|| b.image(file, l.direction)).flatten();
                match light {
                    Some(light) => lightmaps.push(Lightmap { light, direction }),
                    // Keep the numbering: a missing one reads as none.
                    None => lightmaps.push(Lightmap {
                        light: Arc::new(MapImage::Decoded(Arc::new(Image::unity_black()))),
                        direction: None,
                    }),
                }
            }
        }
        // Objects shown only at another time of day stay hidden, and the
        // current time's are switched on: SunLightInfo sets them active.
        let mut hidden: HashSet<i64> = HashSet::new();
        let mut now: HashSet<i64> = HashSet::new();
        if let Some(sun) = &sun {
            now = sun
                .times
                .iter()
                .filter(|t| t.time == Some(time))
                .flat_map(|t| t.visible.iter().map(|p| p.path_id))
                .collect();
            for t in &sun.times {
                hidden.extend(t.visible.iter().map(|p| p.path_id).filter(|id| !now.contains(id)));
            }
        }
        mirrors.extend(crate::mirror::of_file(b.game, loaded, file, &tree, &hidden, &now));
        for obj in sf.objects_of(unity::class::LIGHT) {
            if let Ok(o) = sf.read(obj) {
                let light = Light::read(&o);
                let world = tree.world_of(light.game_object.path_id);
                lights.push((light, world));
            }
        }
        if let Some(obj) = sf.objects_of(unity::class::RENDER_SETTINGS).next() {
            let read = RenderSettings::read(&sf.read(obj)?);
            sky = b.sky(file, read.skybox);
            // What is reflected outside every probe: the cubemap Unity
            // baked from the sky, or the scene's custom one.
            let cube = if read.reflection_mode == 1 {
                read.custom_reflection
            } else {
                read.sky_reflection
            };
            reflection = Some(b.reflection(file, cube, read.reflection_intensity));
            settings = Some(read);
        }
        for obj in sf.objects_of(unity::class::REFLECTION_PROBE) {
            let Ok(o) = sf.read(obj) else { continue };
            let probe = unity::ReflectionProbe::read(&o);
            let go = probe.game_object.path_id;
            if !probe.enabled || !tree.shown(go, &hidden, &now) {
                continue;
            }
            // Baked and custom probes hold a cubemap; a real-time one is
            // drawn while the game runs, and reflects nothing here.
            let cube = match probe.mode {
                0 => probe.baked,
                2 => probe.custom,
                _ => {
                    *b.skipped.entry("real-time reflection probe".to_owned()).or_default() += 1;
                    PPtr::default()
                }
            };
            let position = tree.world_of(go).transform_point3(Vec3::ZERO);
            probes.push(ReflectionProbe {
                position: position.to_array(),
                // The box follows the probe, without its turn or scale.
                centre: (position + Vec3::from(probe.offset)).to_array(),
                half: (Vec3::from(probe.size) * 0.5).to_array(),
                box_projection: probe.box_projection,
                importance: probe.importance,
                reflection: b.reflection(file, cube, probe.intensity),
            });
        }
        for obj in sf.objects_of(class::MESH_RENDERER) {
            let Ok(renderer) = sf.read(obj).and_then(|o| Renderer::read(&o)) else {
                continue;
            };
            let go = renderer.game_object.path_id;
            if !renderer.enabled || !tree.shown(go, &hidden, &now) {
                continue;
            }
            let lightmap = match assigned.get(&obj.path_id) {
                Some(&(index, scale_offset)) => (index >= 0).then_some((index as usize, scale_offset)),
                None => (renderer.lightmap_index < 0xFFFE)
                    .then_some((renderer.lightmap_index as usize, renderer.lightmap_scale_offset)),
            };
            match b.object(file, &tree, &renderer, lightmap) {
                Ok(Some((mut o, bounds))) => {
                    o.vanish = hidden_by.get(&go).cloned().unwrap_or_default();
                    objects.push(o);
                    probe_bounds.push(bounds.filter(|_| renderer.reflection_probes));
                }
                Ok(None) => {}
                Err(e) => *b.skipped.entry(format!("{e:#}")).or_default() += 1,
            }
        }
        let skinned = sf.objects_of(class::SKINNED_MESH_RENDERER).count();
        if skinned > 0 {
            *b.skipped.entry("skinned renderer".to_owned()).or_default() += skinned;
        }
    }
    let mut light = map_light(sun.as_ref(), time, &lights, settings.as_ref());
    // What CharaStudio shows behind the map is the map's own sky dome; the
    // scene's Unity skybox is kept for drawing on request.
    light.unity_sky = sky;
    light.sky = b.dome.take();
    if let Some(reflection) = reflection {
        light.reflection = reflection;
    }
    for (o, bounds) in objects.iter_mut().zip(&probe_bounds) {
        o.probe = bounds.and_then(|bounds| pick_probe(bounds, &probes));
    }
    Ok(MapScene {
        objects,
        lightmaps,
        light,
        probes,
        camera_mask: rk_render::map::STUDIO_CAMERA_MASK,
        mirrors,
        vanish: volumes,
    })
}

/// The sun, ambient light and fog of a time of day.
fn map_light(
    sun: Option<&SunLightInfo>,
    time: TimeOfDay,
    lights: &[(Light, Mat4)],
    settings: Option<&RenderSettings>,
) -> MapLight {
    let main = lights
        .iter()
        .find(|(l, _)| l.enabled && l.kind == unity::LightKind::Directional);
    let info = sun.and_then(|s| s.times.iter().find(|t| t.time == Some(time)));
    // SunLightInfo turns the map's light to its angles for the time.
    let (rotation, color, intensity) = match (info, main) {
        (Some(t), _) => {
            let [x, y, z] = t.angle.map(f32::to_radians);
            (Quat::from_euler(EulerRot::YXZ, y, x, z), t.color, t.intensity)
        }
        (None, Some((l, world))) => (
            Quat::from_mat3(&Mat3::from_mat4(*world)).normalize(),
            l.color,
            l.intensity,
        ),
        (None, None) => (Quat::from_euler(EulerRot::YXZ, 0.5, 0.8, 0.0), [1.0; 4], 1.0),
    };
    // A light shines along its forward axis.
    let sun_dir = -(rotation * Vec3::Z).normalize_or_zero();
    let sun_color = linear3(color).map(|c| c * intensity);
    // The scene's ambient light as Unity's shaders read it. Ambient from
    // the sky is the probe Unity baked; a gradient or a flat colour is a
    // constant term.
    let ambient_sh = match settings {
        Some(s) if s.ambient_mode == unity::AmbientMode::Skybox && s.ambient_probe.len() == 27 => {
            let mut probe = [0.0f32; 27];
            probe.copy_from_slice(&s.ambient_probe);
            let k = s.ambient_intensity;
            rk_render::modshader::sh_constants(&probe.map(|c| c * k))
        }
        Some(s) => {
            let c = linear3(s.ambient_sky).map(|c| c * s.ambient_intensity);
            let mut sh = [[0.0; 4]; 7];
            for i in 0..3 {
                sh[i][3] = c[i];
            }
            sh
        }
        None => {
            let mut sh = [[0.0; 4]; 7];
            for row in sh.iter_mut().take(3) {
                row[3] = 0.2;
            }
            sh
        }
    };
    let fog = match (info, settings) {
        (Some(t), _) if t.fog => Some(Fog {
            color: linear3(t.fog_color),
            start: t.fog_start,
            end: t.fog_end,
        }),
        (Some(_), _) => None,
        (None, Some(s)) if s.fog => Some(Fog {
            color: linear3(s.fog_color),
            start: s.fog_start,
            end: s.fog_end,
        }),
        _ => None,
    };
    MapLight {
        sun_dir: sun_dir.to_array(),
        sun_color,
        sun_mask: main.map_or(u32::MAX, |(l, _)| l.culling_mask),
        ambient_sh,
        // CharaStudio's camera clears to its background colour, black.
        background: [0.0; 3],
        fog,
        sky: None,
        unity_sky: None,
        cut_above: None,
        // The map's light casts when its shadows are on, as far as
        // Unity's default shadow distance.
        shadows: main
            .filter(|(l, _)| l.shadows > 0)
            .map(|(l, _)| rk_render::map::SunShadows {
                strength: l.shadow_strength,
                distance: 50.0,
            }),
        // Black without a cubemap, as Unity's default reflection is.
        reflection: Reflection {
            cube: None,
            hdr: rk_render::sky::HDR_PLAIN,
        },
        // Stored linear: the colour equals the ambient probe's constant
        // term in the maps that have both.
        indirect_specular: settings.map_or([0.0; 3], |s| {
            [s.indirect_specular[0], s.indirect_specular[1], s.indirect_specular[2]]
        }),
    }
}

/// The object hierarchy of one file.
pub(crate) struct Tree<'a> {
    pub(crate) transforms: &'a HashMap<i64, Transform>,
    pub(crate) transform_of: &'a HashMap<i64, i64>,
    pub(crate) objects: &'a HashMap<i64, GameObject>,
}

/// The volume of the collider on a game object, in the world: a box
/// collider as its box, a sphere as its sphere, a mesh collider as the box
/// around its mesh.
fn volume_of(
    sf: &rk_unityfs::SerializedFile,
    tree: &Tree,
    game_object: i64,
    b: &mut Builder,
) -> Option<rk_render::map::VanishVolume> {
    let go = tree.objects.get(&game_object)?;
    let world = tree.world_of(game_object);
    let file = b.loaded.files.iter().find(|f| std::ptr::eq(&f.sf, sf))?.clone();
    for c in &go.components {
        if c.file_id != 0 {
            continue;
        }
        let Some(info) = sf.object(c.path_id) else { continue };
        let Ok(o) = sf.read(info) else { continue };
        let v3 = |name: &str| unity::vec3(o.get(name)).map(Vec3::from);
        match info.class_id {
            // BoxCollider
            65 => {
                return Some(rk_render::map::VanishVolume::Box {
                    to_local: world.inverse(),
                    centre: v3("m_Center").unwrap_or(Vec3::ZERO),
                    half: v3("m_Size").unwrap_or(Vec3::ONE) * 0.5,
                });
            }
            // SphereCollider
            135 => {
                let (scale, ..) = world.to_scale_rotation_translation();
                let centre = world.transform_point3(v3("m_Center").unwrap_or(Vec3::ZERO));
                let radius = o.f32("m_Radius").unwrap_or(0.5) * scale.abs().max_element();
                return Some(rk_render::map::VanishVolume::Sphere { centre, radius });
            }
            // MeshCollider
            64 => {
                let mesh = o.pptr("m_Mesh").and_then(|p| b.mesh(&file, p).ok().flatten())?;
                let (centre, half) = mesh.local_aabb?;
                return Some(rk_render::map::VanishVolume::Box {
                    to_local: world.inverse(),
                    centre: Vec3::from(centre),
                    half: Vec3::from(half),
                });
            }
            _ => {}
        }
    }
    None
}

/// Unity's "Map" layer, which maps' objects and CharaStudio's camera
/// triggers use.
const MAP_LAYER: i32 = 11;

impl Tree<'_> {
    /// Every active game object of this name, in the file's order.
    fn find_all(&self, name: &str) -> Vec<i64> {
        let mut ids: Vec<i64> = self
            .objects
            .iter()
            .filter(|(_, o)| o.name == name && o.active)
            .map(|(id, _)| *id)
            .collect();
        ids.sort();
        ids
    }

    fn layer(&self, game_object: i64) -> i32 {
        self.objects.get(&game_object).map_or(0, |o| o.layer)
    }

    /// The first game object of this name, in the file's order.
    fn find(&self, name: &str) -> Option<i64> {
        let mut ids: Vec<&i64> = self
            .objects
            .iter()
            .filter(|(_, o)| o.name == name)
            .map(|(id, _)| id)
            .collect();
        ids.sort();
        ids.first().map(|id| **id)
    }

    /// A game object and every one under it.
    fn with_descendants(&self, game_object: i64) -> Vec<i64> {
        let mut out = Vec::new();
        let mut stack = vec![game_object];
        while let Some(id) = stack.pop() {
            if out.contains(&id) || out.len() > 100_000 {
                continue;
            }
            out.push(id);
            let Some(t) = self.transform_of.get(&id).and_then(|t| self.transforms.get(t)) else {
                continue;
            };
            for child in t.children.iter().filter(|c| c.file_id == 0) {
                if let Some(ct) = self.transforms.get(&child.path_id) {
                    stack.push(ct.game_object.path_id);
                }
            }
        }
        out
    }

    /// The world matrix of a game object, through its parents.
    pub(crate) fn world_of(&self, game_object: i64) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        let mut at = self.transform_of.get(&game_object).copied();
        let mut depth = 0;
        while let Some(id) = at.filter(|_| depth < 256) {
            let Some(t) = self.transforms.get(&id) else { break };
            let local = Mat4::from_scale_rotation_translation(
                Vec3::from(t.scale),
                Quat::from_array(t.rotation),
                Vec3::from(t.position),
            );
            m = local * m;
            at = (!t.father.is_null() && t.father.file_id == 0).then_some(t.father.path_id);
            depth += 1;
        }
        m
    }

    /// Whether a game object shows: it and every parent active, and not
    /// one of the objects of another time of day.
    /// `on` are switched on whatever they are stored as.
    pub(crate) fn shown(&self, game_object: i64, hidden: &HashSet<i64>, on: &HashSet<i64>) -> bool {
        let mut go = Some(game_object);
        let mut depth = 0;
        while let Some(id) = go.filter(|_| depth < 256) {
            let active = on.contains(&id) || self.objects.get(&id).is_none_or(|o| o.active);
            if hidden.contains(&id) || !active {
                return false;
            }
            let father = self
                .transform_of
                .get(&id)
                .and_then(|t| self.transforms.get(t))
                .map(|t| t.father)
                .filter(|f| !f.is_null() && f.file_id == 0);
            go = father
                .and_then(|f| self.transforms.get(&f.path_id))
                .map(|t| t.game_object.path_id);
            depth += 1;
        }
        true
    }
}

struct Builder<'a> {
    game: &'a Game,
    loaded: &'a LoadedBundle,
    meshes: HashMap<(String, i64), Arc<Mesh>>,
    materials: HashMap<(String, i64), Option<Arc<MapMaterial>>>,
    images: HashMap<(String, i64), Option<Arc<MapImage>>>,
    /// Reflection cubemaps with their decoding, before any intensity.
    cubes: HashMap<(String, i64), (Arc<rk_render::sky::MapCube>, [f32; 4])>,
    skipped: Skipped,
    compressed: bool,
    /// The sky dome's sky, from the first shown renderer with a skybox
    /// material.
    dome: Option<Sky>,
}

impl Builder<'_> {
    /// Follows a reference, the map's own bundle first.
    fn deref(&self, file: &Arc<AssetFile>, p: PPtr) -> Option<ObjRef> {
        crate::scene::deref(self.game, self.loaded, file, p)
    }

    fn script(&self, file: &Arc<AssetFile>, o: &rk_unityfs::UObject) -> String {
        o.pptr("m_Script")
            .and_then(|p| self.deref(file, p))
            .and_then(|r| r.read().ok())
            .and_then(|s| s.str("m_ClassName").map(str::to_owned))
            .unwrap_or_default()
    }

    /// A mesh renderer as an object in the world, or `None` for one with
    /// nothing to draw.
    /// `lightmap` is the lightmap the renderer reads, with where its part
    /// of it lies (scale x, y, offset x, y).
    fn object(
        &mut self,
        file: &Arc<AssetFile>,
        tree: &Tree,
        renderer: &Renderer,
        lightmap: Option<(usize, [f32; 4])>,
    ) -> Result<Option<(MapObject, Option<Bounds>)>> {
        let go = renderer.game_object.path_id;
        let name = tree.objects.get(&go).map(|o| o.name.clone()).unwrap_or_default();
        let Some(filter) = tree
            .objects
            .get(&go)
            .and_then(|o| o.components.iter().find_map(|c| filter_of(file, *c)))
        else {
            return Ok(None);
        };
        if filter.mesh.is_null() {
            return Ok(None);
        }
        // A dome around the map with a skybox material is the map's sky,
        // the way vanilla maps paint theirs (one per time of day, often on
        // Unity's built-in sphere): it is drawn as the sky, not as a mesh.
        if let Some(m) = renderer.materials.iter().find(|m| self.is_sky_material(file, **m)) {
            if self.dome.is_none() {
                self.dome = self.sky(file, *m);
            }
            return Ok(None);
        }
        let Some(mesh) = self.mesh(file, filter.mesh)? else {
            *self.skipped.entry("built-in mesh".to_owned()).or_default() += 1;
            return Ok(None);
        };
        let mut materials = Vec::new();
        for m in &renderer.materials {
            match self.material(file, *m) {
                Some(m) => materials.push(m),
                // A material that is not drawn leaves its submesh out.
                None => materials.push(Arc::new(MapMaterial::hidden())),
            }
        }
        if materials.iter().all(|m| !m.drawn) {
            return Ok(None);
        }
        let world = tree.world_of(go);
        // The renderer's bounds as Unity keeps them: the box around the
        // mesh's own box, moved into the world.
        let bounds = mesh.local_aabb.map(|(centre, half)| world_box(world, centre, half));
        let object = MapObject {
            name,
            mesh: place(&mesh, world, lightmap.map(|l| l.1)),
            materials,
            lightmap: lightmap.map(|l| l.0),
            // 0 is off; on, two-sided and shadows only all cast.
            casts_shadows: renderer.cast_shadows != 0,
            receives_shadows: renderer.receive_shadows,
            shadows_only: renderer.cast_shadows == 3,
            layer: tree.objects.get(&go).map_or(0, |o| o.layer.max(0) as u32),
            vanish: Vec::new(),
            probe: None,
        };
        Ok(Some((object, bounds)))
    }

    fn mesh(&mut self, file: &Arc<AssetFile>, p: PPtr) -> Result<Option<Arc<Mesh>>> {
        let Some(r) = self.deref(file, p) else {
            return Ok(None);
        };
        let key = (r.file.cab.clone(), r.info.path_id);
        if let Some(m) = self.meshes.get(&key) {
            return Ok(Some(m.clone()));
        }
        let mesh = Arc::new(Mesh::read(&r.read()?).context("reading a mesh")?);
        self.meshes.insert(key, mesh.clone());
        Ok(Some(mesh))
    }

    fn material(&mut self, file: &Arc<AssetFile>, p: PPtr) -> Option<Arc<MapMaterial>> {
        let r = self.deref(file, p)?;
        let key = (r.file.cab.clone(), r.info.path_id);
        if let Some(m) = self.materials.get(&key) {
            return m.clone();
        }
        let made = self.make_material(&r);
        self.materials.insert(key, made.clone());
        made
    }

    fn make_material(&mut self, r: &ObjRef) -> Option<Arc<MapMaterial>> {
        let m = Material::read(&r.read().ok()?).ok()?;
        let shader = self.game.assets.shader_name(&r.file, m.shader).unwrap_or_default();
        let lower = shader.to_ascii_lowercase();
        // Drawn by passes of their own: the sky, and a mirror's surface
        // (see `mirror::of_file`).
        if lower.starts_with("skybox/") || crate::mirror::is_mirror_shader(&shader) {
            *self.skipped.entry(format!("shader {shader}")).or_default() += 1;
            return None;
        }
        // Keywords outlive a shader change, so only Standard's own count.
        let standard = lower.starts_with("standard") || lower.starts_with("custom/standard");
        let keywords = if standard {
            m.keywords.to_ascii_uppercase()
        } else {
            String::new()
        };
        // Unity's legacy particle shaders: tinted, unlit, blended by alpha
        // or added; maps paint leaves and grass with them.
        let particle = lower.starts_with("particles/");
        let blend = if particle {
            if lower.contains("additive") {
                Blend::Additive
            } else {
                Blend::Fade
            }
        } else if keywords.contains("_ALPHAPREMULTIPLY_ON") {
            Blend::Premultiplied
        } else if keywords.contains("_ALPHABLEND_ON") || lower.contains("transparent") && !lower.contains("cutout") {
            Blend::Fade
        } else if keywords.contains("_ALPHATEST_ON") || lower.contains("cutout") {
            Blend::Cutout
        } else {
            Blend::Opaque
        };
        let default_queue = match blend {
            Blend::Opaque => 2000,
            Blend::Cutout => 2450,
            Blend::Fade | Blend::Additive | Blend::Premultiplied => 3000,
        };
        let main = m.texture("_MainTex");
        let albedo = main.and_then(|t| self.image(&r.file, t.texture));
        let st = main.map_or([1.0, 1.0, 0.0, 0.0], |t| {
            [t.scale[0], t.scale[1], t.offset[0], t.offset[1]]
        });
        let color = if particle {
            m.color("_TintColor").unwrap_or([0.5; 4])
        } else {
            m.color("_Color").unwrap_or([1.0; 4])
        };
        let [cr, cg, cb] = linear3(color);
        let unlit = lower.starts_with("unlit/");
        // Standard's emission, on with its keyword: the colour, which Unity
        // hands to shaders linearised, times the emission map.
        let (emission, emission_map) = if keywords.contains("_EMISSION") {
            let c = m.color("_EmissionColor").unwrap_or([0.0; 4]);
            let map = m.texture("_EmissionMap").and_then(|t| self.image(&r.file, t.texture));
            (linear3(c), map)
        } else {
            ([0.0; 3], None)
        };
        let surface = (standard && !unlit).then(|| {
            let file = r.file.clone();
            surface_of(&m, &keywords, lower.contains("specular setup"), &mut |p| {
                self.image(&file, p)
            })
        });
        let two_sided =
            particle || lower.contains("double") || lower.contains("2-sided") || m.float("_Cull") == Some(0.0);
        Some(Arc::new(MapMaterial {
            color: [cr, cg, cb, color[3]],
            albedo,
            st,
            emission,
            emission_map,
            blend,
            cutoff: m.float("_Cutoff").unwrap_or(0.5),
            two_sided,
            unlit: unlit || particle,
            particle,
            drawn: true,
            queue: if m.render_queue >= 0 {
                m.render_queue
            } else {
                default_queue
            },
            surface,
        }))
    }

    /// A cubemap to reflect with its mip levels, scaled by `intensity`.
    /// Black when there is none, as Unity's default reflection is.
    fn reflection(&mut self, file: &Arc<AssetFile>, p: PPtr, intensity: f32) -> Reflection {
        let none = Reflection {
            cube: None,
            hdr: rk_render::sky::HDR_PLAIN,
        };
        let Some(r) = self.deref(file, p) else {
            return none;
        };
        let key = (r.file.cab.clone(), r.info.path_id);
        let cached = self.cubes.get(&key).cloned();
        let made = cached.map(Ok).unwrap_or_else(|| {
            let (tex, raw) = self.game.assets.texture_raw(&r)?;
            let hdr = sky::reflection_hdr(&tex, 1.0)
                .with_context(|| format!("reflection cubemap usage {}", tex.lightmap_format))?;
            Ok::<_, anyhow::Error>((Arc::new(sky::cube_levels(&tex, &raw, self.compressed)?), hdr))
        });
        match made {
            Ok((cube, hdr)) => {
                self.cubes.insert(key, (cube.clone(), hdr));
                Reflection {
                    cube: Some(cube),
                    hdr: [hdr[0] * intensity, hdr[1], hdr[2], hdr[3]],
                }
            }
            Err(e) => {
                *self.skipped.entry(format!("reflection: {e:#}")).or_default() += 1;
                none
            }
        }
    }

    /// Whether a material uses one of Unity's skybox shaders.
    fn is_sky_material(&self, file: &Arc<AssetFile>, p: PPtr) -> bool {
        self.deref(file, p)
            .and_then(|r| {
                let m = Material::read(&r.read().ok()?).ok()?;
                self.game.assets.shader_name(&r.file, m.shader)
            })
            .is_some_and(|name| SkyShader::named(&name).is_some())
    }

    /// The scene's sky, when its material uses one of Unity's skybox
    /// shaders.
    fn sky(&mut self, file: &Arc<AssetFile>, p: PPtr) -> Option<Sky> {
        let r = self.deref(file, p)?;
        let m = Material::read(&r.read().ok()?).ok()?;
        let name = self.game.assets.shader_name(&r.file, m.shader).unwrap_or_default();
        let Some(shader) = SkyShader::named(&name) else {
            *self.skipped.entry(format!("sky shader {name}")).or_default() += 1;
            return None;
        };
        // The bundle's own copy of the shader says how it was compiled; the
        // game's maps are linear.
        let compile = self
            .deref(&r.file, m.shader)
            .and_then(|s| s.read().ok())
            .and_then(|o| rk_unityfs::objects::shader_d3d11_programs(&o))
            .map_or(Compile::Linear, |programs| sky::compile_of(shader, &programs));
        let note = |skipped: &mut Skipped, what: String| *skipped.entry(what).or_default() += 1;
        match shader {
            SkyShader::Procedural => {
                if compile == Compile::Gamma {
                    note(
                        &mut self.skipped,
                        "sky: Skybox/Procedural compiled for gamma colour, drawn as the linear one".to_owned(),
                    );
                }
                Some(Sky::Procedural(sky::procedural(&m)))
            }
            SkyShader::SixSided => {
                const FACES: [&str; 6] = ["_FrontTex", "_BackTex", "_LeftTex", "_RightTex", "_UpTex", "_DownTex"];
                let mut faces: [SkyTexture; 6] = Default::default();
                for (face, property) in faces.iter_mut().zip(FACES) {
                    let texture = m.texture(property).map(|t| t.texture).unwrap_or_default();
                    let usage = self.texture_usage(&r.file, texture);
                    face.hdr = sky::hdr_of(usage).unwrap_or_else(|| {
                        note(&mut self.skipped, format!("sky texture usage {usage}: read as plain"));
                        HDR_PLAIN
                    });
                    face.image = self.image(&r.file, texture).or_else(|| Some(grey()));
                }
                Some(Sky::SixSided {
                    faces,
                    look: sky::look(&m, compile),
                })
            }
            SkyShader::Cubemap => {
                let texture = m.texture("_Tex").map(|t| t.texture).unwrap_or_default();
                let mut hdr = HDR_PLAIN;
                let cube = self.deref(&r.file, texture).and_then(|c| {
                    let made = self
                        .game
                        .assets
                        .texture_raw(&c)
                        .and_then(|(tex, raw)| Ok((sky::cube(&tex, &raw)?, tex.lightmap_format)));
                    match made {
                        Ok((cube, usage)) => {
                            hdr = sky::hdr_of(usage).unwrap_or_else(|| {
                                note(&mut self.skipped, format!("sky texture usage {usage}: read as plain"));
                                HDR_PLAIN
                            });
                            Some(Arc::new(cube))
                        }
                        Err(e) => {
                            note(&mut self.skipped, format!("sky cubemap: {e:#}"));
                            None
                        }
                    }
                });
                Some(Sky::Cubemap {
                    cube,
                    hdr,
                    look: sky::look(&m, compile),
                })
            }
        }
    }

    /// A texture's `m_LightmapFormat`, 0 when there is none to read.
    fn texture_usage(&self, file: &Arc<AssetFile>, p: PPtr) -> i32 {
        self.deref(file, p)
            .and_then(|r| r.file.sf.read_until(&r.info, "m_LightmapFormat").ok())
            .and_then(|o| o.i64("m_LightmapFormat"))
            .unwrap_or(0) as i32
    }

    fn image(&mut self, file: &Arc<AssetFile>, p: PPtr) -> Option<Arc<MapImage>> {
        let r = self.deref(file, p)?;
        let key = (r.file.cab.clone(), r.info.path_id);
        if let Some(i) = self.images.get(&key) {
            return i.clone();
        }
        let made = match self.load_image(&r) {
            Ok(i) => Some(Arc::new(i)),
            Err(e) => {
                *self.skipped.entry(format!("texture: {e:#}")).or_default() += 1;
                None
            }
        };
        self.images.insert(key, made.clone());
        made
    }

    /// A texture as the GPU will take it: DXT1 and DXT5 as they are
    /// stored, with the file's own mip levels; the rest decoded.
    fn load_image(&self, r: &ObjRef) -> Result<MapImage> {
        let (tex, raw) = self.game.assets.texture_raw(r)?;
        let block = match tex.format {
            10 => Some(8),
            12 => Some(16),
            _ => None,
        };
        if let Some(block) = block.filter(|_| self.compressed && tex.width % 4 == 0 && tex.height % 4 == 0) {
            let mut levels = Vec::new();
            let mut at = 0usize;
            for i in 0..tex.mip_count.max(1) {
                let (w, h) = ((tex.width >> i).max(1), (tex.height >> i).max(1));
                let len = (w.div_ceil(4) * h.div_ceil(4) * block) as usize;
                let Some(level) = raw.get(at..at + len) else { break };
                levels.push(level.to_vec());
                at += len;
            }
            anyhow::ensure!(!levels.is_empty(), "texture {} holds no data", tex.name);
            return Ok(MapImage::Compressed {
                name: tex.name,
                width: tex.width,
                height: tex.height,
                bc3: block == 16,
                levels,
                srgb: tex.color_space != 0,
                repeat: tex.wrap_mode == 0,
            });
        }
        let rgba = tex.decode_rgba8(&raw)?;
        Ok(MapImage::Decoded(Arc::new(Image {
            name: tex.name,
            width: tex.width,
            height: tex.height,
            rgba,
            srgb: tex.color_space != 0,
            repeat: tex.wrap_mode == 0,
        })))
    }
}

/// Unity's built-in "grey" texture, which the skybox shaders name as the
/// default of their textures: half grey, half transparent.
fn grey() -> Arc<MapImage> {
    plain("default_grey", [128; 4], false)
}

/// What Standard reads of a material, by the keywords that pick its
/// program: a map counts only with its keyword, as in the game. `load`
/// reads a texture.
fn surface_of(
    m: &Material,
    keywords: &str,
    specular_setup: bool,
    load: &mut dyn FnMut(PPtr) -> Option<Arc<MapImage>>,
) -> Surface {
    let has = |k: &str| keywords.split_whitespace().any(|w| w == k);
    let mut image = |name: &str| m.texture(name).and_then(|t| load(t.texture));
    let float = |name: &str, default: f32| m.float(name).unwrap_or(default);
    let gloss_keyword = if specular_setup {
        "_SPECGLOSSMAP"
    } else {
        "_METALLICGLOSSMAP"
    };
    let gloss_map = has(gloss_keyword)
        .then(|| {
            image(if specular_setup {
                "_SpecGlossMap"
            } else {
                "_MetallicGlossMap"
            })
        })
        .flatten();
    let smoothness_from = if has("_SMOOTHNESS_TEXTURE_ALBEDO_CHANNEL_A") {
        SmoothnessFrom::AlbedoAlpha
    } else if has(gloss_keyword) {
        SmoothnessFrom::MapAlpha
    } else {
        SmoothnessFrom::Value
    };
    let detail = has("_DETAIL_MULX2").then(|| {
        let st = m.texture("_DetailAlbedoMap").map_or([1.0, 1.0, 0.0, 0.0], |t| {
            [t.scale[0], t.scale[1], t.offset[0], t.offset[1]]
        });
        Detail {
            // Unity's defaults where a map is not set: grey, which
            // doubled is about white, and a flat normal map.
            albedo: image("_DetailAlbedoMap").or_else(|| Some(plain("default_grey", [128; 4], true))),
            normal: image("_DetailNormalMap").or_else(|| Some(plain("default_bump", [128, 128, 255, 128], false))),
            mask: image("_DetailMask"),
            normal_scale: float("_DetailNormalMapScale", 1.0),
            st,
            second_uv: float("_UVSec", 0.0) != 0.0,
        }
    });
    Surface {
        // The shader marks `_Metallic` as a gamma value (flag 32 of the
        // property in the game's Shader object), so Unity hands it over
        // linearised, as it does colours.
        metallic: to_linear(float("_Metallic", 0.0)),
        smoothness: match smoothness_from {
            SmoothnessFrom::Value => float("_Glossiness", 0.5),
            _ => float("_GlossMapScale", 1.0),
        },
        smoothness_from,
        gloss_map,
        specular: specular_setup.then(|| linear3(m.color("_SpecColor").unwrap_or([0.2, 0.2, 0.2, 1.0]))),
        normal_map: has("_NORMALMAP").then(|| image("_BumpMap")).flatten(),
        normal_scale: float("_BumpScale", 1.0),
        occlusion_map: image("_OcclusionMap"),
        occlusion_strength: float("_OcclusionStrength", 1.0),
        detail,
        height: has("_PARALLAXMAP")
            .then(|| image("_ParallaxMap"))
            .flatten()
            .map(|map| (map, float("_Parallax", 0.02))),
        highlights: !has("_SPECULARHIGHLIGHTS_OFF"),
        reflections: !has("_GLOSSYREFLECTIONS_OFF"),
    }
}

/// A one texel texture of one colour, for Unity's default textures.
fn plain(name: &str, rgba: [u8; 4], srgb: bool) -> Arc<MapImage> {
    Arc::new(MapImage::Decoded(Arc::new(Image {
        name: name.into(),
        width: 1,
        height: 1,
        rgba: rgba.to_vec(),
        srgb,
        repeat: true,
    })))
}

/// The box in the world around a box given by its centre and half size
/// in an object's own space.
fn world_box(world: Mat4, centre: [f32; 3], half: [f32; 3]) -> Bounds {
    let c = world.transform_point3(Vec3::from(centre));
    let m = Mat3::from_mat4(world);
    let h = Vec3::from(half);
    let reach = m.x_axis.abs() * h.x + m.y_axis.abs() * h.y + m.z_axis.abs() * h.z;
    ((c - reach).to_array(), (c + reach).to_array())
}

/// The MeshFilter among a game object's components.
fn filter_of(file: &Arc<AssetFile>, component: PPtr) -> Option<MeshFilter> {
    if component.file_id != 0 {
        return None;
    }
    let info = file.sf.object(component.path_id)?;
    if info.class_id != class::MESH_FILTER {
        return None;
    }
    MeshFilter::read(&file.sf.read(info).ok()?).ok()
}

/// A mesh moved into the world by `world`. With `lightmap`, the scale and
/// offset of its part of a lightmap, its second UV set (the first when it
/// has none, as Unity reads it) is moved there.
fn place(mesh: &Mesh, world: Mat4, lightmap: Option<[f32; 4]>) -> MapMesh {
    let normal = Mat3::from_mat4(world).inverse().transpose();
    let positions: Vec<[f32; 3]> = mesh
        .positions
        .iter()
        .map(|p| world.transform_point3(Vec3::from(*p)).to_array())
        .collect();
    let normals = mesh
        .normals
        .iter()
        .map(|n| (normal * Vec3::from(*n)).normalize_or_zero().to_array())
        .collect();
    // A mirrored object turns its triangles inside out; turn them back.
    let mirrored = world.determinant() < 0.0;
    // Tangents turn with the object; the binormal's sign turns once more
    // for a mirrored one (Unity's `unity_WorldTransformParams.w`).
    let linear = Mat3::from_mat4(world);
    let flip = if mirrored { -1.0 } else { 1.0 };
    let tangents = mesh
        .tangents
        .iter()
        .map(|t| {
            let w = (linear * Vec3::new(t[0], t[1], t[2])).normalize_or_zero();
            [w.x, w.y, w.z, t[3] * flip]
        })
        .collect();
    let mut indices = mesh.indices.clone();
    if mirrored {
        for tri in indices.chunks_exact_mut(3) {
            tri.swap(1, 2);
        }
    }
    // Only triangles are drawn; other topologies keep their place empty.
    let submeshes = mesh
        .submeshes
        .iter()
        .map(|s| {
            if s.topology == 0 {
                (s.first_index, s.index_count)
            } else {
                (0, 0)
            }
        })
        .collect();
    let uv2 = match lightmap {
        Some([sx, sy, ox, oy]) => {
            let source = if mesh.uv[1].is_empty() {
                &mesh.uv[0]
            } else {
                &mesh.uv[1]
            };
            source.iter().map(|[u, v]| [u * sx + ox, v * sy + oy]).collect()
        }
        None => Vec::new(),
    };
    MapMesh {
        uv2,
        colors: mesh.colors.clone(),
        tangents,
        uv1: mesh.uv[1].clone(),
        bounds: MapMesh::compute_bounds(&positions),
        positions,
        normals,
        uv: mesh.uv[0].clone(),
        indices,
        submeshes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rk_unityfs::objects::TexEnv;

    fn standard(keywords: &str, floats: &[(&str, f32)], textures: &[&str]) -> Material {
        Material {
            keywords: keywords.to_owned(),
            floats: floats.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect(),
            textures: textures
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let env = TexEnv {
                        texture: PPtr {
                            file_id: 0,
                            path_id: i as i64 + 1,
                        },
                        scale: [2.0, 3.0],
                        offset: [0.25, 0.5],
                    };
                    ((*name).to_owned(), env)
                })
                .collect(),
            ..Default::default()
        }
    }

    /// Every texture that is set loads.
    fn load(p: PPtr) -> Option<Arc<MapImage>> {
        (!p.is_null()).then(|| plain("set", [255; 4], false))
    }

    #[test]
    fn a_plain_standard_material_reads_its_values() {
        let m = standard(
            "",
            &[("_Metallic", 0.5), ("_Glossiness", 0.3), ("_GlossMapScale", 0.9)],
            &[],
        );
        let s = surface_of(&m, "", false, &mut load);
        // The shader takes `_Metallic` as a gamma value.
        assert!((s.metallic - to_linear(0.5)).abs() < 1e-6);
        assert_eq!(s.smoothness, 0.3);
        assert_eq!(s.smoothness_from, SmoothnessFrom::Value);
        assert!(s.gloss_map.is_none() && s.normal_map.is_none() && s.detail.is_none() && s.height.is_none());
        assert!(s.highlights && s.reflections && s.specular.is_none());
        assert_eq!((s.normal_scale, s.occlusion_strength), (1.0, 1.0));
    }

    #[test]
    fn maps_count_only_with_their_keyword() {
        let textures = [
            "_MetallicGlossMap",
            "_BumpMap",
            "_ParallaxMap",
            "_OcclusionMap",
            "_DetailAlbedoMap",
        ];
        let floats = [("_Glossiness", 0.3), ("_GlossMapScale", 0.9), ("_Parallax", 0.05)];
        let m = standard("", &floats, &textures);
        let s = surface_of(&m, "", false, &mut load);
        assert!(s.gloss_map.is_none() && s.normal_map.is_none() && s.height.is_none() && s.detail.is_none());
        // Standard reads the occlusion map without a keyword.
        assert!(s.occlusion_map.is_some());

        let keywords = "_METALLICGLOSSMAP _NORMALMAP _PARALLAXMAP _DETAIL_MULX2 _SPECULARHIGHLIGHTS_OFF";
        let s = surface_of(&m, keywords, false, &mut load);
        assert!(s.gloss_map.is_some() && s.normal_map.is_some());
        assert_eq!(s.smoothness_from, SmoothnessFrom::MapAlpha);
        assert_eq!(s.smoothness, 0.9);
        assert_eq!(s.height.as_ref().map(|h| h.1), Some(0.05));
        assert!(!s.highlights && s.reflections);
        let d = s.detail.expect("detail");
        assert_eq!(d.st, [2.0, 3.0, 0.25, 0.5]);
        // The maps that are not set read Unity's defaults.
        assert!(d.albedo.is_some() && d.normal.is_some() && d.mask.is_none());
        assert!(!d.second_uv);

        let s = surface_of(
            &m,
            "_SMOOTHNESS_TEXTURE_ALBEDO_CHANNEL_A _GLOSSYREFLECTIONS_OFF",
            false,
            &mut load,
        );
        assert_eq!(s.smoothness_from, SmoothnessFrom::AlbedoAlpha);
        assert_eq!(s.smoothness, 0.9);
        assert!(!s.reflections);
    }

    #[test]
    fn the_specular_setup_reads_its_own_colour_and_map() {
        let mut m = standard("", &[("_Glossiness", 0.4)], &["_SpecGlossMap", "_MetallicGlossMap"]);
        m.colors.push(("_SpecColor".to_owned(), [1.0, 0.5, 0.0, 1.0]));
        let s = surface_of(&m, "_METALLICGLOSSMAP", true, &mut load);
        assert_eq!(s.specular, Some([1.0, to_linear(0.5), 0.0]));
        assert!(s.gloss_map.is_none());
        assert_eq!(s.smoothness_from, SmoothnessFrom::Value);
        let s = surface_of(&m, "_SPECGLOSSMAP", true, &mut load);
        assert!(s.gloss_map.is_some());
        assert_eq!(s.smoothness_from, SmoothnessFrom::MapAlpha);
    }

    #[test]
    fn a_box_moved_into_the_world_keeps_everything_inside() {
        // A quarter turn about Y swaps the box's x and z sizes.
        let world =
            Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)) * Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let (lo, hi) = world_box(world, [0.0, 1.0, 0.0], [1.0, 2.0, 3.0]);
        let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4);
        assert!(near(lo, [7.0, -1.0, -1.0]), "{lo:?}");
        assert!(near(hi, [13.0, 3.0, 1.0]), "{hi:?}");
    }

    fn quad() -> Mesh {
        Mesh {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            tangents: vec![[1.0, 0.0, 0.0, -1.0]; 3],
            uv: [
                vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                vec![[0.5, 0.5]; 3],
                Vec::new(),
                Vec::new(),
            ],
            indices: vec![0, 1, 2],
            ..Default::default()
        }
    }

    #[test]
    fn tangents_turn_with_the_object() {
        let world = Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2) * Mat4::from_scale(Vec3::splat(3.0));
        let placed = place(&quad(), world, None);
        let t = placed.tangents[0];
        // +X turns to -Z, stays a unit vector, and keeps its sign.
        assert!(
            (t[0]).abs() < 1e-5 && (t[2] + 1.0).abs() < 1e-5 && t[3] == -1.0,
            "{t:?}"
        );
        assert_eq!(placed.uv1, vec![[0.5, 0.5]; 3]);
    }

    #[test]
    fn a_mirrored_object_turns_the_binormal() {
        let placed = place(&quad(), Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)), None);
        assert_eq!(placed.tangents[0], [-1.0, 0.0, 0.0, 1.0]);
        assert_eq!(placed.indices, vec![0, 2, 1]);
    }
}
