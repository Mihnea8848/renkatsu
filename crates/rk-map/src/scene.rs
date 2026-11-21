// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! A map's scene, opened from its bundle and summed up: what it holds and
//! what drawing it will need. A map is a Unity scene stored in a bundle,
//! a level file (`BuildPlayer-<level>`) and its `.sharedAssets`, which
//! hold the meshes, materials and textures the level's objects name.

use crate::unity::{self, Light, LightmapSettings, LightmapUse, ReflectionProbe, RenderSettings, SunLightInfo};
use anyhow::{anyhow, Context, Result};
use rk_unityfs::objects::{Material, Mesh, MeshFilter, Renderer, Texture2D};
use rk_unityfs::{class, PPtr};
use rk_vfs::assets::{AssetFile, LoadedBundle, ObjRef};
use rk_vfs::Game;
use std::collections::BTreeMap;
use std::sync::Arc;

/// The meshes Unity builds in (`unity default resources`), by path id.
pub const BUILT_IN_MESHES: [(i64, &str); 6] = [
    (10202, "Cube"),
    (10206, "Cylinder"),
    (10207, "Sphere"),
    (10208, "Capsule"),
    (10209, "Plane"),
    (10210, "Quad"),
];

/// A lightmap texture as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct LightmapTexture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Unity's `TextureFormat` number.
    pub format: i32,
}

/// What a map's scene holds.
#[derive(Debug, Clone, Default)]
pub struct MapInfo {
    pub level: String,
    /// The bundle's size on disk, in bytes.
    pub bytes: u64,
    pub game_objects: usize,
    pub mesh_renderers: usize,
    pub skinned_renderers: usize,
    /// Vertices and triangles of every drawn mesh, counted once per
    /// renderer that draws it.
    pub vertices: usize,
    pub triangles: usize,
    pub materials: usize,
    /// Shader names by how many materials use them.
    pub shaders: BTreeMap<String, usize>,
    pub textures: usize,
    /// Texture formats by how many textures have them.
    pub texture_formats: BTreeMap<i32, usize>,
    /// The size of all textures as stored, in bytes.
    pub texture_bytes: u64,
    /// Unity's own meshes in use, by name, with how often.
    pub built_in_meshes: BTreeMap<String, usize>,
    /// Meshes that could not be found or read.
    pub missing_meshes: usize,
    /// Where the first few of them were looked for.
    pub missing_where: Vec<String>,
    /// Renderers whose mesh is left empty, which draw nothing in Unity too.
    pub empty_renderers: usize,
    pub lights: Vec<Light>,
    pub render_settings: Option<RenderSettings>,
    /// The shader of the sky's material.
    pub skybox_shader: Option<String>,
    pub lightmap_settings: Option<LightmapSettings>,
    pub lightmaps: Vec<LightmapTexture>,
    /// Renderers whose own fields name a lightmap.
    pub lightmapped_renderers: usize,
    /// What `LightmapPrefab` assigns.
    pub lightmap_prefab: Vec<LightmapUse>,
    pub reflection_probes: Vec<ReflectionProbe>,
    pub cubemaps: usize,
    pub light_probe_sets: usize,
    pub sun: Option<SunLightInfo>,
    pub animators: usize,
    pub particle_systems: usize,
    pub video_players: usize,
    /// The classes of the game's scripts on objects, by how often.
    pub scripts: BTreeMap<String, usize>,
}

/// Opens a map's scene bundle. The game's copy is preferred over a mod's
/// of the same path, as with the other content.
pub fn open(game: &Game, bundle: &str) -> Result<Arc<LoadedBundle>> {
    let mut all = game.assets.bundles(bundle);
    all.sort_by_key(|b| b.mod_idx.is_some());
    all.into_iter().next().ok_or_else(|| anyhow!("no bundle {bundle}"))
}

/// A map's `SunLightInfo`, with the bundle and the file it is in, to
/// follow what it points at.
type SunLight = (SunLightInfo, Arc<LoadedBundle>, Arc<AssetFile>);

/// The map's `SunLightInfo`, when it has one.
fn sun_light(game: &Game, bundle: &str) -> Result<Option<SunLight>> {
    let loaded = open(game, bundle)?;
    for file in &loaded.files {
        for obj in file.sf.objects_of(class::MONO_BEHAVIOUR) {
            let Ok(o) = file.sf.read(obj) else { continue };
            let script = o
                .pptr("m_Script")
                .and_then(|p| deref(game, &loaded, file, p))
                .and_then(|r| r.read().ok())
                .and_then(|s| s.str("m_ClassName").map(str::to_owned))
                .unwrap_or_default();
            if script == "SunLightInfo" {
                let sun = SunLightInfo::read(&o);
                if sun.times.iter().any(|t| t.time.is_some()) {
                    return Ok(Some((sun, loaded.clone(), file.clone())));
                }
            }
        }
    }
    Ok(None)
}

/// The times of day a map has: those of its `SunLightInfo`, in the
/// game's order, or the day alone for a map without one.
pub fn times(game: &Game, bundle: &str) -> Result<Vec<unity::TimeOfDay>> {
    let Some((sun, ..)) = sun_light(game, bundle)? else {
        return Ok(vec![unity::TimeOfDay::Day]);
    };
    let mut times: Vec<unity::TimeOfDay> = sun.times.iter().filter_map(|t| t.time).collect();
    times.sort_by_key(|t| *t as u8);
    times.dedup();
    Ok(times)
}

/// The colour filter each time of day of a map sets, by the name of its
/// colour table (`SunLightInfo.Info.SetACE`); none for a time that sets
/// none. Studio's filters are known by the same names.
pub fn time_filters(game: &Game, bundle: &str) -> Result<Vec<(unity::TimeOfDay, Option<String>)>> {
    let Some((sun, loaded, file)) = sun_light(game, bundle)? else {
        return Ok(Vec::new());
    };
    Ok(sun
        .times
        .iter()
        .filter_map(|t| {
            let name = deref(game, &loaded, &file, t.lut)
                .filter(|_| !t.lut.is_null())
                .and_then(|r| r.read().ok())
                .and_then(|o| o.str("m_Name").map(str::to_owned));
            Some((t.time?, name))
        })
        .collect())
}

/// A time of day's sun shafts: the time, their colour, and their sun.
pub type TimeShafts = (unity::TimeOfDay, [f32; 3], Option<[f32; 3]>);

/// Each time of day's sun shafts: their colour, and where the sun they
/// come from stands in the map's space (`SunLightInfo` `sunShaftsColor`
/// and `sunShaftsTransform`, as `SetSunShafts` hands them to the Studio
/// camera); no place for a time without one.
pub fn time_sun_shafts(game: &Game, bundle: &str) -> Result<Vec<TimeShafts>> {
    let Some((sun, _, file)) = sun_light(game, bundle)? else {
        return Ok(Vec::new());
    };
    let mut transforms = std::collections::HashMap::new();
    for obj in file.sf.objects_of(class::TRANSFORM) {
        if let Ok(t) = file.sf.read(obj).and_then(|o| rk_unityfs::objects::Transform::read(&o)) {
            transforms.insert(obj.path_id, t);
        }
    }
    let world_of = |start: i64| {
        let mut m = glam::Mat4::IDENTITY;
        let mut at = Some(start);
        let mut depth = 0;
        while let Some(id) = at.filter(|_| depth < 256) {
            let t = transforms.get(&id)?;
            let local = glam::Mat4::from_scale_rotation_translation(
                glam::Vec3::from(t.scale),
                glam::Quat::from_array(t.rotation),
                glam::Vec3::from(t.position),
            );
            m = local * m;
            at = (!t.father.is_null() && t.father.file_id == 0).then_some(t.father.path_id);
            depth += 1;
        }
        Some(m.transform_point3(glam::Vec3::ZERO).to_array())
    };
    Ok(sun
        .times
        .iter()
        .filter_map(|t| {
            let caster = (!t.sun_shafts.is_null() && t.sun_shafts.file_id == 0)
                .then(|| world_of(t.sun_shafts.path_id))
                .flatten();
            let [r, g, b, _] = t.sun_shafts_color;
            Some((t.time?, [r, g, b], caster))
        })
        .collect())
}

/// The other bundles a map's scene reads from, by their paths under
/// `abdata`: the bundles of the files its own files name, and theirs in
/// turn. Unity's built-in resources are not bundles and are left out.
pub fn dependencies(game: &Game, bundle: &str) -> Result<Vec<String>> {
    let own = rk_vfs::index::norm(bundle);
    let mut found: Vec<String> = Vec::new();
    let mut todo = vec![open(game, bundle)?];
    while let Some(loaded) = todo.pop() {
        for file in &loaded.files {
            for external in &file.sf.externals {
                let cab = external.file_name().to_lowercase();
                if loaded.files.iter().any(|f| f.cab == cab) {
                    continue;
                }
                let Some(vpath) = game.assets.index.cabs.get(&cab) else {
                    continue;
                };
                if *vpath == own || found.contains(vpath) {
                    continue;
                }
                found.push(vpath.clone());
                if let Some(next) = game.assets.bundles(vpath).into_iter().next() {
                    todo.push(next);
                }
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Follows a reference from an object of `file`. A file of the map's own
/// bundle comes first: mods reuse level names, so another bundle may hold
/// a file of the same name.
pub(crate) fn deref(game: &Game, loaded: &LoadedBundle, file: &Arc<AssetFile>, p: PPtr) -> Option<ObjRef> {
    if p.file_id > 0 && !p.is_null() {
        let ext = file.sf.externals.get(p.file_id as usize - 1)?.file_name().to_owned();
        if let Some(own) = loaded.files.iter().find(|f| f.cab.eq_ignore_ascii_case(&ext)) {
            let info = *own.sf.object(p.path_id)?;
            return Some(ObjRef {
                file: own.clone(),
                info,
            });
        }
    }
    game.assets.deref(file, p)
}

/// Whether a reference points into Unity's built-in resources, and which
/// of its meshes it names.
fn built_in(file: &AssetFile, p: PPtr) -> Option<&'static str> {
    if p.file_id == 0 {
        return None;
    }
    let ext = file.sf.externals.get(p.file_id as usize - 1)?;
    if !ext.file_name().eq_ignore_ascii_case("unity default resources") {
        return None;
    }
    Some(
        BUILT_IN_MESHES
            .iter()
            .find(|(id, _)| *id == p.path_id)
            .map_or("other built-in", |(_, name)| name),
    )
}

/// Sums up the scene `level` of `bundle`.
pub fn inspect(game: &Game, bundle: &str, level: &str) -> Result<MapInfo> {
    let loaded = open(game, bundle)?;
    let mut info = MapInfo {
        level: level.to_owned(),
        bytes: bundle_size(game, bundle),
        ..Default::default()
    };
    // Mesh sizes, read once per mesh.
    // Objects are told apart by their file's name and their id: a file
    // may be opened again while the map is read.
    let key = |r: &ObjRef| (r.file.cab.clone(), r.info.path_id);
    let mut mesh_sizes: BTreeMap<(String, i64), Option<(usize, usize)>> = BTreeMap::new();
    let mut size_of = |r: &ObjRef| -> Option<(usize, usize)> {
        *mesh_sizes.entry(key(r)).or_insert_with(|| {
            let mesh = Mesh::read(&r.read().ok()?).ok()?;
            Some((mesh.positions.len(), mesh.indices.len() / 3))
        })
    };
    let mut materials_seen = std::collections::HashSet::new();
    for file in &loaded.files {
        let sf = &file.sf;
        let count = |class_id: i32| sf.objects_of(class_id).count();
        info.game_objects += count(class::GAME_OBJECT);
        info.animators += count(class::ANIMATOR);
        info.particle_systems += count(unity::class::PARTICLE_SYSTEM);
        info.video_players += count(unity::class::VIDEO_PLAYER);
        info.cubemaps += count(unity::class::CUBEMAP);
        info.light_probe_sets += count(unity::class::LIGHT_PROBES);
        for obj in sf.objects_of(class::TEXTURE2D) {
            let Ok(o) = sf.read_until(obj, "m_TextureFormat") else {
                continue;
            };
            let Ok(tex) = Texture2D::read(&o) else { continue };
            info.textures += 1;
            *info.texture_formats.entry(tex.format).or_default() += 1;
            info.texture_bytes += o.i64("m_CompleteImageSize").unwrap_or(0).max(0) as u64;
        }
        for obj in sf.objects_of(class::MESH_FILTER) {
            let Ok(filter) = sf.read(obj).and_then(|o| MeshFilter::read(&o)) else {
                continue;
            };
            if let Some(name) = built_in(file, filter.mesh) {
                *info.built_in_meshes.entry(name.to_owned()).or_default() += 1;
            }
        }
        for (class_id, skinned) in [(class::MESH_RENDERER, false), (class::SKINNED_MESH_RENDERER, true)] {
            for obj in sf.objects_of(class_id) {
                let Ok(renderer) = sf.read(obj).and_then(|o| Renderer::read(&o)) else {
                    continue;
                };
                if skinned {
                    info.skinned_renderers += 1;
                } else {
                    info.mesh_renderers += 1;
                }
                if renderer.lightmap_index < 0xFFFE {
                    info.lightmapped_renderers += 1;
                }
                // The mesh: a skinned renderer names it, a plain one has a
                // filter on its object.
                let mesh = if skinned {
                    Some(renderer.mesh)
                } else {
                    mesh_of(file, renderer.game_object)
                };
                match mesh {
                    Some(p) if p.is_null() => info.empty_renderers += 1,
                    Some(p) if built_in(file, p).is_some() => {}
                    Some(p) => {
                        let found = deref(game, &loaded, file, p);
                        match found.as_ref().and_then(&mut size_of) {
                            Some((v, t)) => {
                                info.vertices += v;
                                info.triangles += t;
                            }
                            None => {
                                info.missing_meshes += 1;
                                if info.missing_where.len() < 5 {
                                    let ext = (p.file_id > 0)
                                        .then(|| file.sf.externals.get(p.file_id as usize - 1))
                                        .flatten()
                                        .map_or("this file".to_owned(), |e| e.file_name().to_owned());
                                    let what = if found.is_some() { "unreadable" } else { "not found" };
                                    info.missing_where.push(format!("{what}: id {} in {ext}", p.path_id));
                                }
                            }
                        }
                    }
                    None => info.empty_renderers += 1,
                }
                for m in &renderer.materials {
                    let Some(r) = deref(game, &loaded, file, *m) else {
                        continue;
                    };
                    if !materials_seen.insert(key(&r)) {
                        continue;
                    }
                    info.materials += 1;
                    let shader = r
                        .read()
                        .ok()
                        .and_then(|o| Material::read(&o).ok())
                        .and_then(|mat| game.assets.shader_name(&r.file, mat.shader))
                        .unwrap_or_else(|| "(unknown)".to_owned());
                    *info.shaders.entry(shader).or_default() += 1;
                }
            }
        }
        for obj in sf.objects_of(unity::class::LIGHT) {
            if let Ok(o) = sf.read(obj) {
                info.lights.push(Light::read(&o));
            }
        }
        for obj in sf.objects_of(unity::class::REFLECTION_PROBE) {
            if let Ok(o) = sf.read(obj) {
                info.reflection_probes.push(ReflectionProbe::read(&o));
            }
        }
        if let Some(obj) = sf.objects_of(unity::class::RENDER_SETTINGS).next() {
            let settings = RenderSettings::read(&sf.read(obj)?);
            info.skybox_shader = deref(game, &loaded, file, settings.skybox).and_then(|r| {
                let mat = Material::read(&r.read().ok()?).ok()?;
                game.assets.shader_name(&r.file, mat.shader)
            });
            info.render_settings = Some(settings);
        }
        if let Some(obj) = sf.objects_of(unity::class::LIGHTMAP_SETTINGS).next() {
            let settings = LightmapSettings::read(&sf.read(obj)?);
            for map in &settings.lightmaps {
                let Some(r) = deref(game, &loaded, file, map.light) else {
                    continue;
                };
                let Ok(tex) = r
                    .file
                    .sf
                    .read_until(&r.info, "m_TextureFormat")
                    .map_err(anyhow::Error::from)
                    .and_then(|o| Ok(Texture2D::read(&o)?))
                else {
                    continue;
                };
                info.lightmaps.push(LightmapTexture {
                    name: tex.name,
                    width: tex.width,
                    height: tex.height,
                    format: tex.format,
                });
            }
            info.lightmap_settings = Some(settings);
        }
        for obj in sf.objects_of(class::MONO_BEHAVIOUR) {
            let Ok(o) = sf.read(obj) else { continue };
            let script = o
                .pptr("m_Script")
                .and_then(|p| deref(game, &loaded, file, p))
                .and_then(|r| r.read().ok())
                .and_then(|s| s.str("m_ClassName").map(str::to_owned))
                .unwrap_or_default();
            match script.as_str() {
                "SunLightInfo" => info.sun = Some(SunLightInfo::read(&o)),
                "LightmapPrefab" => info.lightmap_prefab.extend(unity::lightmap_prefab(&o)),
                _ => {}
            }
            if !script.is_empty() {
                *info.scripts.entry(script).or_default() += 1;
            }
        }
    }
    if info.game_objects == 0 {
        return Err(anyhow!("{bundle} holds no scene")).context(level.to_owned());
    }
    Ok(info)
}

/// The mesh of the MeshFilter on a game object.
fn mesh_of(file: &Arc<AssetFile>, game_object: PPtr) -> Option<PPtr> {
    if game_object.file_id != 0 {
        return None;
    }
    let go = file.sf.object(game_object.path_id)?;
    let components = file.sf.read(go).ok()?;
    for c in components.array("m_Component").unwrap_or_default() {
        let p = c.get("component").and_then(|v| v.as_pptr())?;
        if p.file_id != 0 {
            continue;
        }
        let info = file.sf.object(p.path_id)?;
        if info.class_id == class::MESH_FILTER {
            return MeshFilter::read(&file.sf.read(info).ok()?).ok().map(|f| f.mesh);
        }
    }
    None
}

/// The size on disk of the first copy of a bundle, 0 inside a zipmod.
fn bundle_size(game: &Game, bundle: &str) -> u64 {
    let index = &game.assets.index;
    let Some(loc) = index.bundles.get(&rk_vfs::index::norm(bundle)).and_then(|l| l.first()) else {
        return 0;
    };
    if loc.mod_idx.is_some() {
        return 0;
    }
    index
        .roots
        .get(loc.root as usize)
        .and_then(|root| std::fs::metadata(root.join(&loc.path)).ok())
        .map_or(0, |m| m.len())
}
