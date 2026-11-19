// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! A map's scene made ready to draw: every shown mesh renderer placed in
//! the world with its materials, and the sun, ambient light and fog of a
//! time of day.

use crate::scene::open;
use crate::unity::{self, Light, LightmapSettings, RenderSettings, SunLightInfo, TimeOfDay};
use anyhow::{Context, Result};
use glam::{EulerRot, Mat3, Mat4, Quat, Vec3};
use rk_chara::scene::Image;
use rk_render::map::{Blend, Fog, Lightmap, MapImage, MapLight, MapMaterial, MapMesh, MapObject, MapScene};
use rk_unityfs::objects::{GameObject, Material, Mesh, MeshFilter, Renderer, Transform};
use rk_unityfs::{class, PPtr};
use rk_vfs::assets::{AssetFile, LoadedBundle, ObjRef};
use rk_vfs::Game;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// What was left out while building, by reason, with how often.
pub type Skipped = HashMap<String, usize>;

/// Unity's gamma to linear conversion of one channel.
fn to_linear(c: f32) -> f32 {
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
/// draws them.
pub fn build(game: &Game, bundle: &str, time: TimeOfDay, compressed: bool) -> Result<(MapScene, Skipped)> {
    let loaded = open(game, bundle)?;
    let mut b = Builder {
        game,
        loaded: &loaded,
        meshes: HashMap::new(),
        materials: HashMap::new(),
        images: HashMap::new(),
        skipped: Skipped::new(),
        compressed,
    };
    let mut objects = Vec::new();
    let mut sun: Option<SunLightInfo> = None;
    let mut lights: Vec<(Light, Mat4)> = Vec::new();
    let mut settings: Option<RenderSettings> = None;
    let mut lightmaps: Vec<Lightmap> = Vec::new();
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
        // Objects shown only at another time of day stay hidden.
        let mut hidden: HashSet<i64> = HashSet::new();
        if let Some(sun) = &sun {
            let now: HashSet<i64> = sun
                .times
                .iter()
                .filter(|t| t.time == Some(time))
                .flat_map(|t| t.visible.iter().map(|p| p.path_id))
                .collect();
            for t in &sun.times {
                hidden.extend(t.visible.iter().map(|p| p.path_id).filter(|id| !now.contains(id)));
            }
        }
        for obj in sf.objects_of(unity::class::LIGHT) {
            if let Ok(o) = sf.read(obj) {
                let light = Light::read(&o);
                let world = tree.world_of(light.game_object.path_id);
                lights.push((light, world));
            }
        }
        if let Some(obj) = sf.objects_of(unity::class::RENDER_SETTINGS).next() {
            settings = Some(RenderSettings::read(&sf.read(obj)?));
        }
        for obj in sf.objects_of(class::MESH_RENDERER) {
            let Ok(renderer) = sf.read(obj).and_then(|o| Renderer::read(&o)) else {
                continue;
            };
            let go = renderer.game_object.path_id;
            if !renderer.enabled || !tree.shown(go, &hidden) {
                continue;
            }
            let lightmap = match assigned.get(&obj.path_id) {
                Some(&(index, scale_offset)) => (index >= 0).then_some((index as usize, scale_offset)),
                None => (renderer.lightmap_index < 0xFFFE)
                    .then_some((renderer.lightmap_index as usize, renderer.lightmap_scale_offset)),
            };
            match b.object(file, &tree, &renderer, lightmap) {
                Ok(Some(o)) => objects.push(o),
                Ok(None) => {}
                Err(e) => *b.skipped.entry(format!("{e:#}")).or_default() += 1,
            }
        }
        let skinned = sf.objects_of(class::SKINNED_MESH_RENDERER).count();
        if skinned > 0 {
            *b.skipped.entry("skinned renderer".to_owned()).or_default() += skinned;
        }
    }
    let light = map_light(sun.as_ref(), time, &lights, settings.as_ref());
    Ok((
        MapScene {
            objects,
            lightmaps,
            light,
        },
        b.skipped,
    ))
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
        background: fog.map_or([0.35, 0.5, 0.75], |f| f.color),
        fog,
        cut_above: None,
        // The map's light casts when its shadows are on, as far as
        // Unity's default shadow distance.
        shadows: main
            .filter(|(l, _)| l.shadows > 0)
            .map(|(l, _)| rk_render::map::SunShadows {
                strength: l.shadow_strength,
                distance: 50.0,
            }),
    }
}

/// The object hierarchy of one file.
struct Tree<'a> {
    transforms: &'a HashMap<i64, Transform>,
    transform_of: &'a HashMap<i64, i64>,
    objects: &'a HashMap<i64, GameObject>,
}

impl Tree<'_> {
    /// The world matrix of a game object, through its parents.
    fn world_of(&self, game_object: i64) -> Mat4 {
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
    fn shown(&self, game_object: i64, hidden: &HashSet<i64>) -> bool {
        let mut go = Some(game_object);
        let mut depth = 0;
        while let Some(id) = go.filter(|_| depth < 256) {
            if hidden.contains(&id) || self.objects.get(&id).is_some_and(|o| !o.active) {
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
    skipped: Skipped,
    compressed: bool,
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
    ) -> Result<Option<MapObject>> {
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
        Ok(Some(MapObject {
            name,
            mesh: place(&mesh, world, lightmap.map(|l| l.1)),
            materials,
            lightmap: lightmap.map(|l| l.0),
            // 0 is off; on, two-sided and shadows only all cast.
            casts_shadows: renderer.cast_shadows != 0,
            receives_shadows: renderer.receive_shadows,
            layer: tree.objects.get(&go).map_or(0, |o| o.layer.max(0) as u32),
        }))
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
        // Drawn later, by passes of their own.
        if lower.starts_with("particles/") || lower.starts_with("skybox/") || lower.contains("mirrorreflection") {
            *self.skipped.entry(format!("shader {shader}")).or_default() += 1;
            return None;
        }
        let keywords = m.keywords.to_ascii_uppercase();
        let blend = if keywords.contains("_ALPHABLEND_ON")
            || keywords.contains("_ALPHAPREMULTIPLY_ON")
            || lower.contains("transparent") && !lower.contains("cutout")
        {
            Blend::Fade
        } else if keywords.contains("_ALPHATEST_ON") || lower.contains("cutout") {
            Blend::Cutout
        } else {
            Blend::Opaque
        };
        let default_queue = match blend {
            Blend::Opaque => 2000,
            Blend::Cutout => 2450,
            Blend::Fade => 3000,
        };
        let main = m.texture("_MainTex");
        let albedo = main.and_then(|t| self.image(&r.file, t.texture));
        let st = main.map_or([1.0, 1.0, 0.0, 0.0], |t| {
            [t.scale[0], t.scale[1], t.offset[0], t.offset[1]]
        });
        let color = m.color("_Color").unwrap_or([1.0; 4]);
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
        let two_sided = lower.contains("double") || lower.contains("2-sided") || m.float("_Cull") == Some(0.0);
        Some(Arc::new(MapMaterial {
            color: [cr, cg, cb, color[3]],
            albedo,
            st,
            emission,
            emission_map,
            blend,
            cutoff: m.float("_Cutoff").unwrap_or(0.5),
            two_sided,
            unlit,
            drawn: true,
            queue: if m.render_queue >= 0 {
                m.render_queue
            } else {
                default_queue
            },
        }))
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
        bounds: MapMesh::compute_bounds(&positions),
        positions,
        normals,
        uv: mesh.uv[0].clone(),
        indices,
        submeshes,
    }
}
