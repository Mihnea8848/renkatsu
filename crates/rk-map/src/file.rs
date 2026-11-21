// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! A map as a Renkatsu asset file, `.rkmap`: the scene `build` makes from
//! the game's bundle, kept as it is, so that opening a map only reads it
//! back. Textures keep the game's blocks and mip levels byte for byte;
//! nothing is encoded again.
//!
//! What every time of day shares is stored once: placed meshes,
//! materials, textures, lightmaps, cubemaps and reflection probes. Each
//! time of day then has a small chunk of its own with its light, fog and
//! sky and which objects show, with the probe each reflects.
//!
//! ```text
//! meta          the map's description, TOML text (see `Meta`)
//! thumbnail     the game's picture of it: width, height, RGBA
//! scene         tables: textures, cubemaps, materials, probes, objects
//! mesh.<n>      placed meshes, a few megabytes per chunk
//! image.<n>     a texture's pixels: its blocks, each mip level in turn
//! cube.<n>      a cubemap's faces, level by level
//! time.<name>   day, evening or night: light, sky, lightmaps, walls that
//!               hide, the objects shown
//! ```

use crate::build::build_times;
use crate::list::Entry;
use crate::thumbnail::Picture;
use crate::unity::TimeOfDay;
use anyhow::{bail, ensure, Context, Result};
use rayon::prelude::*;
use rk_asset::bytes::{In, Out};
use rk_asset::{Pack, Reader, Writer};
use rk_chara::scene::Image;
use rk_render::map::{
    Blend, Detail, Fog, Lightmap, MapImage, MapLight, MapMaterial, MapMesh, MapObject, MapScene, Reflection,
    ReflectionProbe, SmoothnessFrom, SunShadows, Surface, VanishVolume,
};
use rk_render::sky::{MapCube, ProceduralSky, Sky, SkyLook, SkyTexture, SunDisk};
use rk_vfs::Game;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// The kind of asset file a map is.
pub const KIND: [u8; 4] = *b"MAP\0";
/// The version of the map's chunks and of what `build` makes of the game's
/// bundle. Raise it when either changes: a file of an older version is
/// made again when its map's bundle is there to make it from.
pub const VERSION: u32 = 1;
/// The version of the description in `meta`. The text sidecars of
/// Renkatsu 0.1.8 were version 1.
pub const META_FORMAT: u32 = 2;
/// The extension of a map file.
pub const EXTENSION: &str = "rkmap";
/// Meshes go in chunks of about this many bytes, so that they unpack on
/// several threads at once.
const MESH_CHUNK: usize = 4 << 20;

/// What a map file says the map is, as TOML in its `meta` chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    pub format: u32,
    /// The name the map shows under.
    pub name: String,
    /// The name CharaStudio's list gives it.
    pub listed_name: String,
    /// Its number in CharaStudio's list, which CharaStudio scenes name it
    /// by.
    pub number: i32,
    /// The scene in its bundle.
    pub level: String,
    /// Who brings it: Koikatsu, a package or a mod.
    pub source: String,
    /// The times of day it has, in order: "day", "evening", "night".
    pub times: Vec<String>,
    /// The text of its table of walls that hide, as the game stores it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub walls: String,
    /// The bundles it was made from: its scene bundle first, then those
    /// that one reads from, by where the game keeps them under `abdata`.
    pub bundles: Vec<MetaBundle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaBundle {
    pub path: String,
}

pub fn time_name(time: TimeOfDay) -> &'static str {
    match time {
        TimeOfDay::Day => "day",
        TimeOfDay::Evening => "evening",
        TimeOfDay::Night => "night",
    }
}

pub fn time_named(name: &str) -> Option<TimeOfDay> {
    [TimeOfDay::Day, TimeOfDay::Evening, TimeOfDay::Night]
        .into_iter()
        .find(|t| time_name(*t) == name)
}

/// Whether the file at `path` is a map file, not a text sidecar of an
/// older Renkatsu.
pub fn is_map_file(path: &Path) -> bool {
    matches!(rk_asset::peek(path), Ok(Some(h)) if h.kind == KIND)
}

/// Opens a map file, checking that it is one this build reads.
pub fn open(path: &Path) -> Result<Reader> {
    let r = Reader::open(path)?;
    let h = r.header();
    ensure!(h.kind == KIND, "{} is not a map", path.display());
    ensure!(
        h.version <= VERSION,
        "{} was saved by a newer Renkatsu (map version {})",
        path.display(),
        h.version
    );
    Ok(r)
}

/// Whether a map file was made by an older Renkatsu, whose builder may
/// have drawn the map differently.
pub fn is_older(path: &Path) -> bool {
    matches!(rk_asset::peek(path), Ok(Some(h)) if h.kind == KIND && h.version < VERSION)
}

/// The text of a map file's description.
pub fn meta_text(path: &Path) -> Result<String> {
    let r = open(path)?;
    Ok(String::from_utf8(r.read("meta")?)?)
}

pub fn meta(path: &Path) -> Result<Meta> {
    toml::from_str(&meta_text(path)?).with_context(|| format!("reading {}", path.display()))
}

/// The game's picture of the map, when the file holds one.
pub fn thumbnail(path: &Path) -> Result<Option<Picture>> {
    let r = open(path)?;
    if !r.has("thumbnail") {
        return Ok(None);
    }
    let data = r.read("thumbnail")?;
    let mut i = In::new(&data);
    let (width, height) = (i.u32()?, i.u32()?);
    let rgba = i.bytes()?.to_vec();
    ensure!(
        rgba.len() as u64 == width as u64 * height as u64 * 4,
        "the thumbnail is damaged"
    );
    Ok(Some(Picture { width, height, rgba }))
}

/// Makes the map file of a map of the game at `out`: builds the map at
/// each of its times of day (or those of `only` it has) and writes them
/// with the map's picture and description. `progress` hears how far it is,
/// from 0 to 1, and stops it with an error. Returns the file's length.
pub fn convert(
    game: &Game,
    entry: &Entry,
    name: &str,
    only: Option<&[TimeOfDay]>,
    out: &Path,
    progress: &mut dyn FnMut(f32) -> Result<()>,
) -> Result<u64> {
    progress(0.0)?;
    let all = crate::scene::times(game, &entry.bundle)?;
    let times: Vec<TimeOfDay> = match only {
        Some(only) => all.iter().copied().filter(|t| only.contains(t)).collect(),
        None => all.clone(),
    };
    ensure!(!times.is_empty(), "the map has none of those times of day");
    let walls = crate::vanish::table(game, &entry.vanish_bundle, &entry.vanish_file).unwrap_or_default();
    let mut bundles = vec![MetaBundle {
        path: rk_vfs::index::norm(&entry.bundle),
    }];
    bundles.extend(
        crate::scene::dependencies(game, &entry.bundle)?
            .into_iter()
            .map(|path| MetaBundle {
                path: rk_vfs::index::norm(&path),
            }),
    );
    let meta = Meta {
        format: META_FORMAT,
        name: name.to_owned(),
        listed_name: entry.name.trim().to_owned(),
        number: entry.number,
        level: entry.level.clone(),
        source: entry.source.label().to_owned(),
        times: times.iter().map(|t| time_name(*t).to_owned()).collect(),
        walls,
        bundles,
    };
    progress(0.05)?;
    let rows = (!meta.walls.is_empty()).then_some(meta.walls.as_str());
    // Kept as the game stores them: a reader on a GPU without blocks
    // decodes them as `build` would have.
    let built = build_times(game, &entry.bundle, &times, true, rows)?;
    progress(0.7)?;
    let first = if times.contains(&TimeOfDay::Day) {
        TimeOfDay::Day
    } else {
        times[0]
    };
    let picture = crate::thumbnail::picture(game, &entry.bundle, first);
    let scenes: Vec<(TimeOfDay, &MapScene)> = times.iter().copied().zip(built.iter().map(|b| &b.0)).collect();
    let length = write(out, &meta, picture.as_ref(), &scenes)?;
    progress(1.0)?;
    Ok(length)
}

// ------------------------------------------------------------------ writing

/// What is stored once, numbered as the chunks refer to it.
#[derive(Default)]
struct Tables<'a> {
    images: Vec<&'a Arc<MapImage>>,
    image_at: HashMap<usize, usize>,
    cubes: Vec<&'a Arc<MapCube>>,
    cube_at: HashMap<usize, usize>,
    materials: Vec<Vec<u8>>,
    material_at: HashMap<usize, usize>,
    probes: Vec<Vec<u8>>,
    probe_at: HashMap<Vec<u8>, usize>,
    /// Each object's record and its placed mesh.
    objects: Vec<(Vec<u8>, Vec<u8>)>,
    object_at: HashMap<(Vec<u8>, u64), Vec<usize>>,
}

fn key<T>(a: &Arc<T>) -> usize {
    Arc::as_ptr(a) as *const u8 as usize
}

impl<'a> Tables<'a> {
    fn image(&mut self, i: &'a Arc<MapImage>) -> usize {
        *self.image_at.entry(key(i)).or_insert_with(|| {
            self.images.push(i);
            self.images.len() - 1
        })
    }

    fn opt_image(&mut self, o: &mut Out, i: &'a Option<Arc<MapImage>>) {
        let at = i.as_ref().map(|i| self.image(i));
        o.index(at);
    }

    fn cube(&mut self, o: &mut Out, c: &'a Option<Arc<MapCube>>) {
        let at = c.as_ref().map(|c| {
            *self.cube_at.entry(key(c)).or_insert_with(|| {
                self.cubes.push(c);
                self.cubes.len() - 1
            })
        });
        o.index(at);
    }

    fn reflection(&mut self, o: &mut Out, r: &'a Reflection) {
        let Reflection { cube, hdr } = r;
        self.cube(o, cube);
        o.floats(hdr);
    }

    fn material(&mut self, m: &'a Arc<MapMaterial>) -> usize {
        if let Some(at) = self.material_at.get(&key(m)) {
            return *at;
        }
        let mut o = Out::default();
        let MapMaterial {
            color,
            albedo,
            st,
            emission,
            emission_map,
            blend,
            cutoff,
            two_sided,
            unlit,
            particle,
            drawn,
            queue,
            surface,
        } = m.as_ref();
        o.floats(color);
        self.opt_image(&mut o, albedo);
        o.floats(st);
        o.floats(emission);
        self.opt_image(&mut o, emission_map);
        o.u8(match blend {
            Blend::Opaque => 0,
            Blend::Cutout => 1,
            Blend::Fade => 2,
            Blend::Additive => 3,
            Blend::Premultiplied => 4,
        });
        o.f32(*cutoff);
        o.bool(*two_sided);
        o.bool(*unlit);
        o.bool(*particle);
        o.bool(*drawn);
        o.i32(*queue);
        o.bool(surface.is_some());
        if let Some(s) = surface {
            let Surface {
                metallic,
                smoothness,
                smoothness_from,
                gloss_map,
                specular,
                normal_map,
                normal_scale,
                occlusion_map,
                occlusion_strength,
                detail,
                height,
                highlights,
                reflections,
            } = s;
            o.f32(*metallic);
            o.f32(*smoothness);
            o.u8(match smoothness_from {
                SmoothnessFrom::Value => 0,
                SmoothnessFrom::MapAlpha => 1,
                SmoothnessFrom::AlbedoAlpha => 2,
            });
            self.opt_image(&mut o, gloss_map);
            o.bool(specular.is_some());
            o.floats(&specular.unwrap_or_default());
            self.opt_image(&mut o, normal_map);
            o.f32(*normal_scale);
            self.opt_image(&mut o, occlusion_map);
            o.f32(*occlusion_strength);
            o.bool(detail.is_some());
            if let Some(d) = detail {
                let Detail {
                    albedo,
                    normal,
                    mask,
                    normal_scale,
                    st,
                    second_uv,
                } = d;
                self.opt_image(&mut o, albedo);
                self.opt_image(&mut o, normal);
                self.opt_image(&mut o, mask);
                o.f32(*normal_scale);
                o.floats(st);
                o.bool(*second_uv);
            }
            o.bool(height.is_some());
            if let Some((map, scale)) = height {
                let at = self.image(map);
                o.len_of(at);
                o.f32(*scale);
            }
            o.bool(*highlights);
            o.bool(*reflections);
        }
        self.materials.push(o.into_vec());
        let at = self.materials.len() - 1;
        self.material_at.insert(key(m), at);
        at
    }

    fn probe(&mut self, p: &'a ReflectionProbe) -> usize {
        let ReflectionProbe {
            position,
            centre,
            half,
            box_projection,
            importance,
            reflection,
        } = p;
        let mut o = Out::default();
        o.floats(position);
        o.floats(centre);
        o.floats(half);
        o.bool(*box_projection);
        o.i32(*importance);
        self.reflection(&mut o, reflection);
        let bytes = o.into_vec();
        if let Some(at) = self.probe_at.get(&bytes) {
            return *at;
        }
        self.probes.push(bytes.clone());
        self.probe_at.insert(bytes, self.probes.len() - 1);
        self.probes.len() - 1
    }

    /// An object, stored once for all the times that show it. Its probe
    /// is the time's: probes are picked among those shown then.
    fn object(&mut self, ob: &'a MapObject) -> usize {
        let MapObject {
            name,
            mesh,
            materials,
            lightmap,
            casts_shadows,
            receives_shadows,
            shadows_only,
            layer,
            vanish,
            probe: _,
        } = ob;
        let mut o = Out::default();
        o.str(name);
        o.len_of(materials.len());
        for m in materials {
            let at = self.material(m);
            o.len_of(at);
        }
        o.index(*lightmap);
        o.bool(*casts_shadows);
        o.bool(*receives_shadows);
        o.bool(*shadows_only);
        o.u32(*layer);
        o.len_of(vanish.len());
        for v in vanish {
            o.len_of(*v);
        }
        let record = o.into_vec();
        let mesh = mesh_bytes(mesh);
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            mesh.hash(&mut h);
            h.finish()
        };
        let same = self.object_at.entry((record.clone(), hash)).or_default();
        if let Some(at) = same.iter().find(|at| self.objects[**at].1 == mesh) {
            return *at;
        }
        self.objects.push((record, mesh));
        same.push(self.objects.len() - 1);
        self.objects.len() - 1
    }

    fn sky(&mut self, o: &mut Out, sky: &'a Option<Sky>) {
        let look = |o: &mut Out, l: &SkyLook| {
            let SkyLook {
                tint,
                exposure,
                rotation,
                double,
                exponent,
            } = l;
            o.floats(tint);
            o.f32(*exposure);
            o.f32(*rotation);
            o.f32(*double);
            o.bool(*exponent);
        };
        match sky {
            None => o.u8(0),
            Some(Sky::Procedural(p)) => {
                o.u8(1);
                let ProceduralSky {
                    sun_disk,
                    sun_size,
                    atmosphere_thickness,
                    sky_tint,
                    ground,
                    exposure,
                } = p;
                o.u8(match sun_disk {
                    SunDisk::None => 0,
                    SunDisk::Simple => 1,
                    SunDisk::HighQuality => 2,
                });
                o.f32(*sun_size);
                o.f32(*atmosphere_thickness);
                o.floats(sky_tint);
                o.floats(ground);
                o.f32(*exposure);
            }
            Some(Sky::SixSided { faces, look: l }) => {
                o.u8(2);
                for f in faces {
                    let SkyTexture { image, hdr } = f;
                    self.opt_image(o, image);
                    o.floats(hdr);
                }
                look(o, l);
            }
            Some(Sky::Cubemap { cube, hdr, look: l }) => {
                o.u8(3);
                self.cube(o, cube);
                o.floats(hdr);
                look(o, l);
            }
        }
    }

    /// One time of day's chunk.
    fn time(&mut self, scene: &'a MapScene) -> Vec<u8> {
        let MapScene {
            objects,
            lightmaps,
            light,
            probes,
            vanish,
            camera_mask,
            mirrors: _,
        } = scene;
        let mut o = Out::default();
        let MapLight {
            sun_dir,
            sun_color,
            sun_mask,
            ambient_sh,
            fog,
            sky,
            unity_sky,
            background,
            cut_above,
            shadows,
            reflection,
            indirect_specular,
        } = light;
        o.floats(sun_dir);
        o.floats(sun_color);
        o.u32(*sun_mask);
        for row in ambient_sh {
            o.floats(row);
        }
        o.bool(fog.is_some());
        if let Some(Fog { color, start, end }) = fog {
            o.floats(color);
            o.f32(*start);
            o.f32(*end);
        }
        self.sky(&mut o, sky);
        self.sky(&mut o, unity_sky);
        o.floats(background);
        o.bool(cut_above.is_some());
        o.f32(cut_above.unwrap_or_default());
        o.bool(shadows.is_some());
        if let Some(SunShadows { strength, distance }) = shadows {
            o.f32(*strength);
            o.f32(*distance);
        }
        self.reflection(&mut o, reflection);
        o.floats(indirect_specular);
        o.u32(*camera_mask);
        o.len_of(lightmaps.len());
        for Lightmap { light, direction } in lightmaps {
            let at = self.image(light);
            o.len_of(at);
            self.opt_image(&mut o, direction);
        }
        o.len_of(vanish.len());
        for v in vanish {
            match v {
                VanishVolume::Box { to_local, centre, half } => {
                    o.u8(0);
                    o.floats(&to_local.to_cols_array());
                    o.floats(&centre.to_array());
                    o.floats(&half.to_array());
                }
                VanishVolume::Sphere { centre, radius } => {
                    o.u8(1);
                    o.floats(&centre.to_array());
                    o.f32(*radius);
                }
            }
        }
        o.len_of(probes.len());
        for p in probes {
            let at = self.probe(p);
            o.len_of(at);
        }
        o.len_of(objects.len());
        for ob in objects {
            let at = self.object(ob);
            o.len_of(at);
            o.index(ob.probe);
        }
        o.into_vec()
    }
}

fn mesh_bytes(m: &MapMesh) -> Vec<u8> {
    let MapMesh {
        positions,
        normals,
        uv,
        uv2,
        colors,
        tangents,
        uv1,
        indices,
        submeshes,
        bounds,
    } = m;
    let mut o = Out::default();
    o.arrays(positions);
    o.arrays(normals);
    o.arrays(uv);
    o.arrays(uv2);
    o.arrays(colors);
    o.arrays(tangents);
    o.arrays(uv1);
    o.u32s(indices);
    o.len_of(submeshes.len());
    for (first, count) in submeshes {
        o.u32(*first);
        o.u32(*count);
    }
    o.floats(&bounds.0);
    o.floats(&bounds.1);
    o.into_vec()
}

/// A texture's header in the scene chunk, and its pixels for its own.
fn image_bytes(o: &mut Out, image: &MapImage) -> Vec<u8> {
    match image {
        MapImage::Compressed {
            name,
            width,
            height,
            bc3,
            levels,
            srgb,
            repeat,
        } => {
            o.u8(0);
            o.str(name);
            o.u32(*width);
            o.u32(*height);
            o.bool(*bc3);
            o.bool(*srgb);
            o.bool(*repeat);
            o.len_of(levels.len());
            for l in levels {
                o.len_of(l.len());
            }
            levels.concat()
        }
        MapImage::Decoded(i) => {
            let Image {
                name,
                width,
                height,
                rgba,
                srgb,
                repeat,
            } = i.as_ref();
            o.u8(1);
            o.str(name);
            o.u32(*width);
            o.u32(*height);
            o.bool(*srgb);
            o.bool(*repeat);
            o.len_of(rgba.len());
            rgba.clone()
        }
    }
}

fn cube_bytes(o: &mut Out, cube: &MapCube) -> Vec<u8> {
    let MapCube {
        name,
        size,
        faces,
        srgb,
        bc6h,
        mips,
    } = cube;
    o.str(name);
    o.u32(*size);
    o.bool(*srgb);
    o.bool(*bc6h);
    o.len_of(mips.len() + 1);
    let mut data = Vec::new();
    for level in std::iter::once(faces).chain(mips) {
        for face in level {
            o.len_of(face.len());
            data.extend_from_slice(face);
        }
    }
    data
}

/// Writes a map file: the description, the picture and the map at each
/// time of day. Returns the file's length.
pub fn write(path: &Path, meta: &Meta, picture: Option<&Picture>, scenes: &[(TimeOfDay, &MapScene)]) -> Result<u64> {
    ensure!(!scenes.is_empty(), "a map file needs a time of day");
    let mut t = Tables::default();
    let times: Vec<(TimeOfDay, Vec<u8>)> = scenes.iter().map(|(time, s)| (*time, t.time(s))).collect();
    let mut chunks: Vec<(String, Vec<u8>, Pack)> = Vec::new();
    chunks.push(("meta".to_owned(), toml::to_string(meta)?.into_bytes(), Pack::Raw));
    if let Some(p) = picture {
        let mut o = Out::default();
        o.u32(p.width);
        o.u32(p.height);
        o.bytes(&p.rgba);
        chunks.push(("thumbnail".to_owned(), o.into_vec(), Pack::Zstd));
    }
    // The tables.
    let mut s = Out::default();
    s.len_of(t.images.len());
    for (n, image) in t.images.iter().enumerate() {
        let data = image_bytes(&mut s, image);
        // Blocks barely pack; colour decoded from the game's other formats
        // does.
        chunks.push((format!("image.{n}"), data, Pack::Auto));
    }
    s.len_of(t.cubes.len());
    for (n, cube) in t.cubes.iter().enumerate() {
        let data = cube_bytes(&mut s, cube);
        chunks.push((format!("cube.{n}"), data, Pack::Auto));
    }
    s.len_of(t.materials.len());
    for m in &t.materials {
        s.bytes(m);
    }
    s.len_of(t.probes.len());
    for p in &t.probes {
        s.bytes(p);
    }
    // Objects, with where their mesh lies.
    let mut mesh_chunk = Vec::new();
    let mut mesh_n = 0;
    s.len_of(t.objects.len());
    for (record, mesh) in &t.objects {
        if !mesh_chunk.is_empty() && mesh_chunk.len() + mesh.len() > MESH_CHUNK {
            chunks.push((format!("mesh.{mesh_n}"), std::mem::take(&mut mesh_chunk), Pack::Zstd));
            mesh_n += 1;
        }
        s.bytes(record);
        s.len_of(mesh_n);
        s.u64(mesh_chunk.len() as u64);
        s.u64(mesh.len() as u64);
        mesh_chunk.extend_from_slice(mesh);
    }
    if !mesh_chunk.is_empty() {
        chunks.push((format!("mesh.{mesh_n}"), mesh_chunk, Pack::Zstd));
    }
    s.len_of(times.len());
    for (time, _) in &times {
        s.str(time_name(*time));
    }
    chunks.push(("scene".to_owned(), s.into_vec(), Pack::Zstd));
    for (time, data) in times {
        chunks.push((format!("time.{}", time_name(time)), data, Pack::Zstd));
    }
    let packed = chunks
        .par_iter()
        .map(|(name, data, how)| rk_asset::pack(name, data, *how))
        .collect::<Result<Vec<_>>>()?;
    drop(chunks);
    let mut w = Writer::create(path, KIND, VERSION)?;
    for p in packed {
        w.add_packed(p)?;
    }
    w.finish()
}

/// A digest of everything a scene holds that drawing it reads, through
/// the same encoding the file uses: two scenes with the same digest draw
/// the same picture. For checking a file against a fresh build.
pub fn digest(scene: &MapScene) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut t = Tables::default();
    let time = t.time(scene);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    time.hash(&mut h);
    for image in &t.images {
        let mut o = Out::default();
        let data = image_bytes(&mut o, image);
        o.into_vec().hash(&mut h);
        data.hash(&mut h);
    }
    for cube in &t.cubes {
        let mut o = Out::default();
        let data = cube_bytes(&mut o, cube);
        o.into_vec().hash(&mut h);
        data.hash(&mut h);
    }
    t.materials.hash(&mut h);
    t.probes.hash(&mut h);
    t.objects.hash(&mut h);
    h.finish()
}

// ------------------------------------------------------------------ reading

/// A texture's header, its pixels apart.
enum ImageHead {
    Compressed {
        name: String,
        width: u32,
        height: u32,
        bc3: bool,
        srgb: bool,
        repeat: bool,
        levels: Vec<usize>,
    },
    Decoded {
        name: String,
        width: u32,
        height: u32,
        srgb: bool,
        repeat: bool,
        len: usize,
    },
}

struct CubeHead {
    name: String,
    size: u32,
    srgb: bool,
    bc6h: bool,
    /// Each level's six face lengths.
    levels: Vec<[usize; 6]>,
}

struct ObjectHead<'a> {
    record: &'a [u8],
    chunk: usize,
    offset: usize,
    len: usize,
}

/// The tables of the scene chunk.
struct SceneTables<'a> {
    images: Vec<ImageHead>,
    cubes: Vec<CubeHead>,
    materials: Vec<&'a [u8]>,
    probes: Vec<&'a [u8]>,
    objects: Vec<ObjectHead<'a>>,
    times: Vec<String>,
}

fn scene_tables(data: &[u8]) -> Result<SceneTables<'_>> {
    let mut i = In::new(data);
    let mut images = Vec::new();
    for _ in 0..i.len_of()? {
        images.push(match i.u8()? {
            0 => ImageHead::Compressed {
                name: i.str()?.to_owned(),
                width: i.u32()?,
                height: i.u32()?,
                bc3: i.bool()?,
                srgb: i.bool()?,
                repeat: i.bool()?,
                levels: (0..i.len_of()?).map(|_| i.len_of()).collect::<Result<_>>()?,
            },
            1 => ImageHead::Decoded {
                name: i.str()?.to_owned(),
                width: i.u32()?,
                height: i.u32()?,
                srgb: i.bool()?,
                repeat: i.bool()?,
                len: i.len_of()?,
            },
            other => bail!("unknown texture kind {other}"),
        });
    }
    let mut cubes = Vec::new();
    for _ in 0..i.len_of()? {
        let name = i.str()?.to_owned();
        let size = i.u32()?;
        let srgb = i.bool()?;
        let bc6h = i.bool()?;
        let mut levels = Vec::new();
        for _ in 0..i.len_of()? {
            let mut faces = [0usize; 6];
            for f in &mut faces {
                *f = i.len_of()?;
            }
            levels.push(faces);
        }
        cubes.push(CubeHead {
            name,
            size,
            srgb,
            bc6h,
            levels,
        });
    }
    let materials = (0..i.len_of()?).map(|_| i.bytes()).collect::<Result<_>>()?;
    let probes = (0..i.len_of()?).map(|_| i.bytes()).collect::<Result<_>>()?;
    let mut objects = Vec::new();
    for _ in 0..i.len_of()? {
        objects.push(ObjectHead {
            record: i.bytes()?,
            chunk: i.len_of()?,
            offset: i.u64()? as usize,
            len: i.u64()? as usize,
        });
    }
    let times = (0..i.len_of()?)
        .map(|_| i.str().map(str::to_owned))
        .collect::<Result<_>>()?;
    ensure!(i.done(), "the scene chunk has more than it should");
    Ok(SceneTables {
        images,
        cubes,
        materials,
        probes,
        objects,
        times,
    })
}

/// Turns a texture's header and pixels into the texture. Without
/// `compressed`, blocks are decoded as `build` decodes them for a GPU
/// that does not draw blocks.
fn make_image(head: &ImageHead, data: Vec<u8>, compressed: bool) -> Result<MapImage> {
    match head {
        ImageHead::Compressed {
            name,
            width,
            height,
            bc3,
            srgb,
            repeat,
            levels,
        } => {
            ensure!(levels.iter().sum::<usize>() == data.len(), "texture {name} is damaged");
            if !compressed {
                let tex = texture(name, *width, *height, if *bc3 { 12 } else { 10 });
                let rgba = tex.decode_rgba8(&data[..levels.first().copied().unwrap_or(0)])?;
                return Ok(MapImage::Decoded(Arc::new(Image {
                    name: name.clone(),
                    width: *width,
                    height: *height,
                    rgba,
                    srgb: *srgb,
                    repeat: *repeat,
                })));
            }
            let mut at = 0;
            let levels = levels
                .iter()
                .map(|len| {
                    at += len;
                    data[at - len..at].to_vec()
                })
                .collect();
            Ok(MapImage::Compressed {
                name: name.clone(),
                width: *width,
                height: *height,
                bc3: *bc3,
                levels,
                srgb: *srgb,
                repeat: *repeat,
            })
        }
        ImageHead::Decoded {
            name,
            width,
            height,
            srgb,
            repeat,
            len,
        } => {
            ensure!(*len == data.len(), "texture {name} is damaged");
            Ok(MapImage::Decoded(Arc::new(Image {
                name: name.clone(),
                width: *width,
                height: *height,
                rgba: data,
                srgb: *srgb,
                repeat: *repeat,
            })))
        }
    }
}

/// A texture of the game's format, for decoding blocks.
fn texture(name: &str, width: u32, height: u32, format: i32) -> rk_unityfs::objects::Texture2D {
    rk_unityfs::objects::Texture2D {
        name: name.to_owned(),
        width,
        height,
        format,
        mip_count: 1,
        wrap_mode: 0,
        filter_mode: 1,
        color_space: 0,
        lightmap_format: 0,
        data: Vec::new(),
        stream: None,
    }
}

fn make_cube(head: &CubeHead, data: Vec<u8>, compressed: bool) -> Result<MapCube> {
    ensure!(
        head.levels.iter().flatten().sum::<usize>() == data.len() && !head.levels.is_empty(),
        "cubemap {} is damaged",
        head.name
    );
    // A GPU without blocks gets the high range decoded, as `build` gives it.
    let decode = head.bc6h && !compressed;
    let mut at = 0;
    let mut levels: Vec<[Vec<u8>; 6]> = Vec::new();
    for (n, lens) in head.levels.iter().enumerate() {
        let side = (head.size >> n).max(1);
        let mut faces: [Vec<u8>; 6] = Default::default();
        for (face, len) in faces.iter_mut().zip(lens) {
            let bytes = &data[at..at + len];
            *face = if decode {
                texture(&head.name, side, side, 24).decode_rgba8(bytes)?
            } else {
                bytes.to_vec()
            };
            at += len;
        }
        levels.push(faces);
    }
    let faces = levels.remove(0);
    Ok(MapCube {
        name: head.name.clone(),
        size: head.size,
        faces,
        srgb: head.srgb,
        bc6h: head.bc6h && !decode,
        mips: levels,
    })
}

/// Everything loaded for one time of day.
struct Loaded {
    images: HashMap<usize, Arc<MapImage>>,
    cubes: HashMap<usize, Arc<MapCube>>,
    materials: HashMap<usize, Arc<MapMaterial>>,
}

impl Loaded {
    fn image(&self, at: usize) -> Result<Arc<MapImage>> {
        self.images.get(&at).cloned().context("a texture is missing")
    }

    fn opt_image(&self, i: &mut In) -> Result<Option<Arc<MapImage>>> {
        i.index()?.map(|at| self.image(at)).transpose()
    }

    fn cube(&self, i: &mut In) -> Result<Option<Arc<MapCube>>> {
        i.index()?
            .map(|at| self.cubes.get(&at).cloned().context("a cubemap is missing"))
            .transpose()
    }

    fn reflection(&self, i: &mut In) -> Result<Reflection> {
        Ok(Reflection {
            cube: self.cube(i)?,
            hdr: i.floats()?,
        })
    }

    fn sky(&self, i: &mut In) -> Result<Option<Sky>> {
        fn look(i: &mut In) -> Result<SkyLook> {
            Ok(SkyLook {
                tint: i.floats()?,
                exposure: i.f32()?,
                rotation: i.f32()?,
                double: i.f32()?,
                exponent: i.bool()?,
            })
        }
        Ok(match i.u8()? {
            0 => None,
            1 => Some(Sky::Procedural(ProceduralSky {
                sun_disk: match i.u8()? {
                    0 => SunDisk::None,
                    1 => SunDisk::Simple,
                    2 => SunDisk::HighQuality,
                    other => bail!("unknown sun disk {other}"),
                },
                sun_size: i.f32()?,
                atmosphere_thickness: i.f32()?,
                sky_tint: i.floats()?,
                ground: i.floats()?,
                exposure: i.f32()?,
            })),
            2 => {
                let mut faces: [SkyTexture; 6] = Default::default();
                for f in &mut faces {
                    *f = SkyTexture {
                        image: self.opt_image(i)?,
                        hdr: i.floats()?,
                    };
                }
                Some(Sky::SixSided { faces, look: look(i)? })
            }
            3 => Some(Sky::Cubemap {
                cube: self.cube(i)?,
                hdr: i.floats()?,
                look: look(i)?,
            }),
            other => bail!("unknown sky {other}"),
        })
    }

    fn material(&self, data: &[u8]) -> Result<MapMaterial> {
        let mut i = In::new(data);
        let color = i.floats()?;
        let albedo = self.opt_image(&mut i)?;
        let st = i.floats()?;
        let emission = i.floats()?;
        let emission_map = self.opt_image(&mut i)?;
        let blend = match i.u8()? {
            0 => Blend::Opaque,
            1 => Blend::Cutout,
            2 => Blend::Fade,
            3 => Blend::Additive,
            4 => Blend::Premultiplied,
            other => bail!("unknown blend {other}"),
        };
        let cutoff = i.f32()?;
        let two_sided = i.bool()?;
        let unlit = i.bool()?;
        let particle = i.bool()?;
        let drawn = i.bool()?;
        let queue = i.i32()?;
        let surface = if i.bool()? {
            let metallic = i.f32()?;
            let smoothness = i.f32()?;
            let smoothness_from = match i.u8()? {
                0 => SmoothnessFrom::Value,
                1 => SmoothnessFrom::MapAlpha,
                2 => SmoothnessFrom::AlbedoAlpha,
                other => bail!("unknown smoothness source {other}"),
            };
            let gloss_map = self.opt_image(&mut i)?;
            let has_specular = i.bool()?;
            let specular: [f32; 3] = i.floats()?;
            let normal_map = self.opt_image(&mut i)?;
            let normal_scale = i.f32()?;
            let occlusion_map = self.opt_image(&mut i)?;
            let occlusion_strength = i.f32()?;
            let detail = if i.bool()? {
                Some(Detail {
                    albedo: self.opt_image(&mut i)?,
                    normal: self.opt_image(&mut i)?,
                    mask: self.opt_image(&mut i)?,
                    normal_scale: i.f32()?,
                    st: i.floats()?,
                    second_uv: i.bool()?,
                })
            } else {
                None
            };
            let height = if i.bool()? {
                let map = self.image(i.len_of()?)?;
                Some((map, i.f32()?))
            } else {
                None
            };
            Some(Surface {
                metallic,
                smoothness,
                smoothness_from,
                gloss_map,
                specular: has_specular.then_some(specular),
                normal_map,
                normal_scale,
                occlusion_map,
                occlusion_strength,
                detail,
                height,
                highlights: i.bool()?,
                reflections: i.bool()?,
            })
        } else {
            None
        };
        ensure!(i.done(), "a material has more than it should");
        Ok(MapMaterial {
            color,
            albedo,
            st,
            emission,
            emission_map,
            blend,
            cutoff,
            two_sided,
            unlit,
            particle,
            drawn,
            queue,
            surface,
        })
    }
}

/// What a chunk refers to, gathered before anything is loaded so that
/// only what a time of day shows is read.
#[derive(Default)]
struct Wanted {
    images: Vec<usize>,
    cubes: Vec<usize>,
}

impl Wanted {
    fn image(&mut self, i: &mut In) -> Result<()> {
        if let Some(at) = i.index()? {
            self.images.push(at);
        }
        Ok(())
    }

    fn cube(&mut self, i: &mut In) -> Result<()> {
        if let Some(at) = i.index()? {
            self.cubes.push(at);
        }
        Ok(())
    }

    fn sky(&mut self, i: &mut In) -> Result<()> {
        match i.u8()? {
            0 => {}
            1 => {
                i.take(1 + 4 * 2 + 4 * 3 * 2 + 4)?;
            }
            2 => {
                for _ in 0..6 {
                    self.image(i)?;
                    i.take(16)?;
                }
                i.take(4 * 6 + 1)?;
            }
            3 => {
                self.cube(i)?;
                i.take(16 + 4 * 6 + 1)?;
            }
            other => bail!("unknown sky {other}"),
        }
        Ok(())
    }

    fn material(&mut self, data: &[u8]) -> Result<()> {
        let mut i = In::new(data);
        i.take(16)?;
        self.image(&mut i)?;
        i.take(16 + 12)?;
        self.image(&mut i)?;
        i.take(1 + 4 + 4 + 4)?;
        if i.bool()? {
            i.take(4 + 4 + 1)?;
            self.image(&mut i)?;
            i.take(1 + 12)?;
            self.image(&mut i)?;
            i.take(4)?;
            self.image(&mut i)?;
            i.take(4)?;
            if i.bool()? {
                self.image(&mut i)?;
                self.image(&mut i)?;
                self.image(&mut i)?;
                i.take(4 + 16 + 1)?;
            }
            if i.bool()? {
                self.images.push(i.len_of()?);
            }
        }
        Ok(())
    }
}

/// The time of day a file has that is nearest to the one asked for: that
/// one, else day, else its first.
fn pick_time(times: &[String], want: TimeOfDay) -> Result<String> {
    let has = |name: &str| times.iter().any(|t| t == name);
    if has(time_name(want)) {
        Ok(time_name(want).to_owned())
    } else if has("day") {
        Ok("day".to_owned())
    } else {
        times.first().cloned().context("the map has no time of day")
    }
}

/// Reads the map at `time` from a map file. With `compressed`, textures
/// stay the game's blocks, for a GPU that draws them; without, they are
/// decoded as `build` decodes them. A time the map does not have falls
/// back to day, or its first.
pub fn read(path: &Path, time: TimeOfDay, compressed: bool) -> Result<MapScene> {
    let r = open(path)?;
    let scene_data = r.read("scene")?;
    let tables = scene_tables(&scene_data)?;
    let time = pick_time(&tables.times, time)?;
    let time_data = r.read(&format!("time.{time}"))?;

    // What this time of day refers to.
    let mut wanted = Wanted::default();
    let mut shown: Vec<(usize, Option<usize>)> = Vec::new();
    let mut probe_list: Vec<usize> = Vec::new();
    {
        let mut i = In::new(&time_data);
        i.take(12 + 12 + 4 + 7 * 16)?;
        if i.bool()? {
            i.take(12 + 8)?;
        }
        wanted.sky(&mut i)?;
        wanted.sky(&mut i)?;
        i.take(12 + 1 + 4)?;
        if i.bool()? {
            i.take(8)?;
        }
        wanted.cube(&mut i)?;
        i.take(16 + 12 + 4)?;
        for _ in 0..i.len_of()? {
            wanted.images.push(i.len_of()?);
            wanted.image(&mut i)?;
        }
        for _ in 0..i.len_of()? {
            match i.u8()? {
                0 => i.take(4 * (16 + 3 + 3))?,
                1 => i.take(4 * 4)?,
                other => bail!("unknown wall volume {other}"),
            };
        }
        for _ in 0..i.len_of()? {
            probe_list.push(i.len_of()?);
        }
        for _ in 0..i.len_of()? {
            shown.push((i.len_of()?, i.index()?));
        }
        ensure!(i.done(), "the chunk of {time} has more than it should");
    }
    for p in &probe_list {
        let mut i = In::new(tables.probes.get(*p).context("a probe is missing")?);
        i.take(12 * 3 + 1 + 4)?;
        wanted.cube(&mut i)?;
    }
    let mut materials_wanted: Vec<usize> = Vec::new();
    for (ob, _) in &shown {
        let head = tables.objects.get(*ob).context("an object is missing")?;
        let mut i = In::new(head.record);
        i.str()?;
        for _ in 0..i.len_of()? {
            materials_wanted.push(i.len_of()?);
        }
    }
    materials_wanted.sort_unstable();
    materials_wanted.dedup();
    for m in &materials_wanted {
        wanted.material(tables.materials.get(*m).context("a material is missing")?)?;
    }
    wanted.images.sort_unstable();
    wanted.images.dedup();
    wanted.cubes.sort_unstable();
    wanted.cubes.dedup();
    let mut mesh_chunks: Vec<usize> = shown.iter().map(|(ob, _)| tables.objects[*ob].chunk).collect();
    mesh_chunks.sort_unstable();
    mesh_chunks.dedup();

    // Everything read and unpacked at once, on every thread.
    enum Job {
        Image(usize),
        Cube(usize),
        Mesh(usize),
    }
    enum Done {
        Image(usize, MapImage),
        Cube(usize, MapCube),
        Mesh(usize, Vec<u8>),
    }
    let jobs: Vec<Job> = wanted
        .images
        .iter()
        .map(|i| Job::Image(*i))
        .chain(wanted.cubes.iter().map(|c| Job::Cube(*c)))
        .chain(mesh_chunks.iter().map(|m| Job::Mesh(*m)))
        .collect();
    let done = jobs
        .par_iter()
        .map(|job| -> Result<Done> {
            Ok(match job {
                Job::Image(n) => {
                    let head = tables.images.get(*n).context("a texture is missing")?;
                    Done::Image(*n, make_image(head, r.read(&format!("image.{n}"))?, compressed)?)
                }
                Job::Cube(n) => {
                    let head = tables.cubes.get(*n).context("a cubemap is missing")?;
                    Done::Cube(*n, make_cube(head, r.read(&format!("cube.{n}"))?, compressed)?)
                }
                Job::Mesh(n) => Done::Mesh(*n, r.read(&format!("mesh.{n}"))?),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut loaded = Loaded {
        images: HashMap::new(),
        cubes: HashMap::new(),
        materials: HashMap::new(),
    };
    let mut meshes: HashMap<usize, Vec<u8>> = HashMap::new();
    for d in done {
        match d {
            Done::Image(n, i) => {
                loaded.images.insert(n, Arc::new(i));
            }
            Done::Cube(n, c) => {
                loaded.cubes.insert(n, Arc::new(c));
            }
            Done::Mesh(n, m) => {
                meshes.insert(n, m);
            }
        }
    }
    for m in &materials_wanted {
        let made = loaded.material(tables.materials[*m])?;
        loaded.materials.insert(*m, Arc::new(made));
    }

    // The time of day itself.
    let mut i = In::new(&time_data);
    let sun_dir = i.floats()?;
    let sun_color = i.floats()?;
    let sun_mask = i.u32()?;
    let mut ambient_sh = [[0.0; 4]; 7];
    for row in &mut ambient_sh {
        *row = i.floats()?;
    }
    let fog = if i.bool()? {
        Some(Fog {
            color: i.floats()?,
            start: i.f32()?,
            end: i.f32()?,
        })
    } else {
        None
    };
    let sky = loaded.sky(&mut i)?;
    let unity_sky = loaded.sky(&mut i)?;
    let background = i.floats()?;
    let has_cut = i.bool()?;
    let cut = i.f32()?;
    let shadows = if i.bool()? {
        Some(SunShadows {
            strength: i.f32()?,
            distance: i.f32()?,
        })
    } else {
        None
    };
    let reflection = loaded.reflection(&mut i)?;
    let indirect_specular = i.floats()?;
    let light = MapLight {
        sun_dir,
        sun_color,
        sun_mask,
        ambient_sh,
        fog,
        sky,
        unity_sky,
        background,
        cut_above: has_cut.then_some(cut),
        shadows,
        reflection,
        indirect_specular,
    };
    let camera_mask = i.u32()?;
    let mut lightmaps = Vec::new();
    for _ in 0..i.len_of()? {
        lightmaps.push(Lightmap {
            light: loaded.image(i.len_of()?)?,
            direction: loaded.opt_image(&mut i)?,
        });
    }
    let mut vanish = Vec::new();
    for _ in 0..i.len_of()? {
        vanish.push(match i.u8()? {
            0 => VanishVolume::Box {
                to_local: glam::Mat4::from_cols_array(&i.floats()?),
                centre: glam::Vec3::from(i.floats::<3>()?),
                half: glam::Vec3::from(i.floats::<3>()?),
            },
            1 => VanishVolume::Sphere {
                centre: glam::Vec3::from(i.floats::<3>()?),
                radius: i.f32()?,
            },
            other => bail!("unknown wall volume {other}"),
        });
    }
    let mut probes = Vec::new();
    for _ in 0..i.len_of()? {
        let mut p = In::new(tables.probes[i.len_of()?]);
        probes.push(ReflectionProbe {
            position: p.floats()?,
            centre: p.floats()?,
            half: p.floats()?,
            box_projection: p.bool()?,
            importance: p.i32()?,
            reflection: loaded.reflection(&mut p)?,
        });
    }
    let objects = shown
        .par_iter()
        .map(|(ob, probe)| {
            let head = &tables.objects[*ob];
            let chunk = meshes.get(&head.chunk).context("a mesh chunk is missing")?;
            let mesh = chunk
                .get(head.offset..head.offset.saturating_add(head.len))
                .context("a mesh lies outside its chunk")?;
            let mut i = In::new(head.record);
            let name = i.str()?.to_owned();
            let materials = (0..i.len_of()?)
                .map(|_| {
                    let at = i.len_of()?;
                    loaded.materials.get(&at).cloned().context("a material is missing")
                })
                .collect::<Result<Vec<_>>>()?;
            let object = MapObject {
                name,
                mesh: read_mesh(mesh)?,
                materials,
                lightmap: i.index()?,
                casts_shadows: i.bool()?,
                receives_shadows: i.bool()?,
                shadows_only: i.bool()?,
                layer: i.u32()?,
                vanish: (0..i.len_of()?).map(|_| i.len_of()).collect::<Result<_>>()?,
                probe: *probe,
            };
            ensure!(i.done(), "an object has more than it should");
            Ok(object)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(MapScene {
        objects,
        lightmaps,
        light,
        probes,
        vanish,
        camera_mask,
        // Not in the file yet: read from the game's bundle
        // (`mirror::mirrors`).
        mirrors: Vec::new(),
    })
}

fn read_mesh(data: &[u8]) -> Result<MapMesh> {
    let mut i = In::new(data);
    let mesh = MapMesh {
        positions: i.arrays()?,
        normals: i.arrays()?,
        uv: i.arrays()?,
        uv2: i.arrays()?,
        colors: i.arrays()?,
        tangents: i.arrays()?,
        uv1: i.arrays()?,
        indices: i.u32s()?,
        submeshes: (0..i.len_of()?)
            .map(|_| Ok((i.u32()?, i.u32()?)))
            .collect::<Result<_>>()?,
        bounds: (i.floats()?, i.floats()?),
    };
    ensure!(i.done(), "a mesh has more than it should");
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Mat4, Vec3};

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("renkatsu-mapfile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn blocks(name: &str, side: u32, bc3: bool, seed: u8) -> Arc<MapImage> {
        let block = if bc3 { 16 } else { 8 };
        let levels = (0..3)
            .map(|l| {
                let s = (side >> l).max(1);
                let n = (s.div_ceil(4) * s.div_ceil(4) * block) as usize;
                (0..n).map(|i| (i as u8).wrapping_mul(seed)).collect()
            })
            .collect();
        Arc::new(MapImage::Compressed {
            name: name.into(),
            width: side,
            height: side,
            bc3,
            levels,
            srgb: true,
            repeat: false,
        })
    }

    fn decoded(name: &str) -> Arc<MapImage> {
        Arc::new(MapImage::Decoded(Arc::new(Image {
            name: name.into(),
            width: 2,
            height: 1,
            rgba: vec![1, 2, 3, 4, 5, 6, 7, 8],
            srgb: false,
            repeat: true,
        })))
    }

    fn cube(bc6h: bool) -> Arc<MapCube> {
        let face = |n: usize, v: u8| vec![v; n];
        let (top, mip) = if bc6h { (16 * 4, 16) } else { (8 * 8 * 4, 4 * 4 * 4) };
        Arc::new(MapCube {
            name: "probe".into(),
            size: 8,
            // Zero BC6H blocks: texture2ddecoder's BC6H decoder overflows
            // on some others in a debug build.
            faces: std::array::from_fn(|i| face(top, if bc6h { 0 } else { i as u8 })),
            srgb: !bc6h,
            bc6h,
            mips: vec![std::array::from_fn(|i| face(mip, if bc6h { 0 } else { 10 + i as u8 }))],
        })
    }

    fn mesh(shift: f32) -> MapMesh {
        let positions = vec![[shift, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, -0.0]];
        MapMesh {
            bounds: MapMesh::compute_bounds(&positions),
            positions,
            normals: vec![[0.0, 0.0, 1.0]; 3],
            uv: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            uv2: vec![[0.5, 0.5]; 3],
            colors: vec![[1.0, 0.5, 0.25, 1.0]; 3],
            tangents: vec![[1.0, 0.0, 0.0, -1.0]; 3],
            uv1: Vec::new(),
            indices: vec![0, 1, 2, 2, 1, 0],
            submeshes: vec![(0, 3), (3, 3)],
        }
    }

    fn object(name: &str, shift: f32, materials: Vec<Arc<MapMaterial>>, probe: Option<usize>) -> MapObject {
        MapObject {
            name: name.into(),
            mesh: mesh(shift),
            materials,
            lightmap: Some(0),
            casts_shadows: true,
            receives_shadows: false,
            shadows_only: false,
            layer: 11,
            vanish: vec![0, 1],
            probe,
        }
    }

    fn light(sky: Option<Sky>, reflection: Reflection) -> MapLight {
        MapLight {
            sun_dir: [0.0, 1.0, 0.0],
            sun_color: [1.0, 0.9, 0.8],
            sun_mask: 0xFFFF_FFFF,
            ambient_sh: [[0.1, 0.2, 0.3, 0.4]; 7],
            fog: Some(Fog {
                color: [0.5; 3],
                start: 10.0,
                end: 300.0,
            }),
            sky,
            unity_sky: Some(Sky::Procedural(ProceduralSky {
                sun_disk: SunDisk::HighQuality,
                sun_size: 0.04,
                atmosphere_thickness: 1.0,
                sky_tint: [0.5; 3],
                ground: [0.37, 0.35, 0.34],
                exposure: 1.3,
            })),
            background: [0.0; 3],
            cut_above: None,
            shadows: Some(SunShadows {
                strength: 0.8,
                distance: 50.0,
            }),
            reflection,
            indirect_specular: [0.2, 0.2, 0.25],
        }
    }

    /// Two times of day sharing a wall, a material and its textures; the
    /// evening adds a lamp and has a sky of six textures.
    fn scenes() -> Vec<(TimeOfDay, MapScene)> {
        let albedo = blocks("wall", 16, false, 3);
        let normal = blocks("wall_n", 8, true, 5);
        let wall = Arc::new(MapMaterial {
            color: [1.0, 1.0, 1.0, 1.0],
            albedo: Some(albedo.clone()),
            st: [2.0, 2.0, 0.0, 0.5],
            emission: [0.0; 3],
            emission_map: None,
            blend: Blend::Cutout,
            cutoff: 0.3,
            two_sided: true,
            unlit: false,
            particle: false,
            drawn: true,
            queue: 2450,
            surface: Some(Surface {
                normal_map: Some(normal.clone()),
                specular: Some([0.1, 0.2, 0.3]),
                smoothness_from: SmoothnessFrom::AlbedoAlpha,
                detail: Some(Detail {
                    albedo: Some(decoded("detail")),
                    normal: None,
                    mask: Some(albedo.clone()),
                    normal_scale: 0.5,
                    st: [4.0, 4.0, 0.0, 0.0],
                    second_uv: true,
                }),
                height: Some((normal.clone(), 0.02)),
                ..Default::default()
            }),
        });
        let lamp = Arc::new(MapMaterial {
            emission: [2.0, 1.5, 1.0],
            emission_map: Some(decoded("glow")),
            blend: Blend::Premultiplied,
            ..MapMaterial::hidden()
        });
        let lightmaps = || {
            vec![Lightmap {
                light: blocks("lightmap", 32, true, 7),
                direction: Some(decoded("direction")),
            }]
        };
        let shared_lightmaps = lightmaps();
        let probe_cube = cube(true);
        let probe = ReflectionProbe {
            position: [1.0, 2.0, 3.0],
            centre: [1.0, 2.5, 3.0],
            half: [4.0, 2.0, 4.0],
            box_projection: true,
            importance: 1,
            reflection: Reflection {
                cube: Some(probe_cube.clone()),
                hdr: [1.5, 1.0, 0.0, 0.0],
            },
        };
        let vanish = vec![
            VanishVolume::Box {
                to_local: Mat4::from_translation(Vec3::new(1.0, 0.0, -2.0)),
                centre: Vec3::ZERO,
                half: Vec3::new(1.0, 2.0, 3.0),
            },
            VanishVolume::Sphere {
                centre: Vec3::ONE,
                radius: 2.5,
            },
        ];
        let reflection = Reflection {
            cube: Some(cube(false)),
            hdr: [1.0, 1.0, 0.0, 0.0],
        };
        let day = MapScene {
            objects: vec![object("wall", 0.0, vec![wall.clone(), wall.clone()], Some(0))],
            lightmaps: shared_lightmaps,
            light: light(None, reflection.clone()),
            probes: vec![probe.clone()],
            vanish: vanish.clone(),
            camera_mask: rk_render::map::STUDIO_CAMERA_MASK,
            mirrors: Vec::new(),
        };
        let faces = std::array::from_fn(|i| SkyTexture {
            image: (i != 2).then(|| albedo.clone()),
            hdr: [1.0, 1.0, 0.0, i as f32],
        });
        let six = Sky::SixSided {
            faces,
            look: SkyLook {
                tint: [0.5; 3],
                exposure: 1.0,
                rotation: 90.0,
                double: 4.594_794,
                exponent: true,
            },
        };
        let mut evening_light = light(Some(six), reflection);
        evening_light.fog = None;
        evening_light.cut_above = Some(2.3);
        let evening = MapScene {
            objects: vec![
                object("lamp", 5.0, vec![lamp, wall.clone()], None),
                object("wall", 0.0, vec![wall.clone(), wall], Some(0)),
            ],
            lightmaps: day.lightmaps.clone_shallow(),
            light: evening_light,
            probes: vec![probe],
            vanish,
            camera_mask: 1,
            mirrors: Vec::new(),
        };
        vec![(TimeOfDay::Day, day), (TimeOfDay::Evening, evening)]
    }

    trait CloneShallow {
        fn clone_shallow(&self) -> Self;
    }

    impl CloneShallow for Vec<Lightmap> {
        fn clone_shallow(&self) -> Self {
            self.iter()
                .map(|l| Lightmap {
                    light: l.light.clone(),
                    direction: l.direction.clone(),
                })
                .collect()
        }
    }

    fn sample_meta() -> Meta {
        Meta {
            format: META_FORMAT,
            name: "Rooftop".into(),
            listed_name: "屋上".into(),
            number: 22,
            level: "Rooftop".into(),
            source: "Koikatsu".into(),
            times: vec!["day".into(), "evening".into()],
            walls: "wall00_d\twall00\r\n".into(),
            bundles: vec![MetaBundle {
                path: "map/scene/rooftop.unity3d".into(),
            }],
        }
    }

    fn written(name: &str) -> (std::path::PathBuf, Vec<(TimeOfDay, MapScene)>) {
        let dir = scratch(name);
        let path = dir.join("Rooftop.rkmap");
        let scenes = scenes();
        let picture = Picture {
            width: 2,
            height: 2,
            rgba: (0..16).collect(),
        };
        let refs: Vec<(TimeOfDay, &MapScene)> = scenes.iter().map(|(t, s)| (*t, s)).collect();
        write(&path, &sample_meta(), Some(&picture), &refs).unwrap();
        (path, scenes)
    }

    #[test]
    fn every_time_of_day_reads_back_as_it_was() {
        let (path, scenes) = written("times");
        for (time, scene) in &scenes {
            let read = read(&path, *time, true).unwrap();
            assert_eq!(format!("{read:?}"), format!("{scene:?}"), "{time:?}");
            assert_eq!(digest(&read), digest(scene), "{time:?}");
        }
        // Shared textures are one: the wall's material reads one albedo.
        let evening = read(&path, TimeOfDay::Evening, true).unwrap();
        let wall = &evening.objects[1].materials;
        assert!(Arc::ptr_eq(&wall[0], &wall[1]));
        let Some(Sky::SixSided { faces, .. }) = &evening.light.sky else {
            panic!("a sky of six textures")
        };
        assert!(Arc::ptr_eq(
            faces[0].image.as_ref().unwrap(),
            wall[0].albedo.as_ref().unwrap()
        ));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn what_the_times_share_is_stored_once() {
        let (path, _) = written("shared");
        let r = open(&path).unwrap();
        let count = |prefix: &str| r.entries().iter().filter(|e| e.name.starts_with(prefix)).count();
        // wall, wall_n, detail, glow, lightmap, direction.
        assert_eq!(count("image."), 6);
        assert_eq!(count("cube."), 2);
        assert_eq!(count("time."), 2);
        let tables_data = r.read("scene").unwrap();
        let tables = scene_tables(&tables_data).unwrap();
        assert_eq!(tables.objects.len(), 2);
        assert_eq!(tables.materials.len(), 2);
        assert_eq!(tables.probes.len(), 1);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_missing_time_falls_back_to_day() {
        let (path, scenes) = written("fallback");
        let night = read(&path, TimeOfDay::Night, true).unwrap();
        assert_eq!(format!("{night:?}"), format!("{:?}", scenes[0].1));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_description_and_picture_read_alone() {
        let (path, _) = written("meta");
        assert!(is_map_file(&path));
        assert!(!is_older(&path));
        assert_eq!(meta(&path).unwrap(), sample_meta());
        let text = meta_text(&path).unwrap();
        assert!(text.contains("name = \"Rooftop\""), "{text}");
        let picture = thumbnail(&path).unwrap().unwrap();
        assert_eq!((picture.width, picture.height), (2, 2));
        assert_eq!(picture.rgba, (0..16).collect::<Vec<u8>>());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn without_blocks_textures_decode_as_the_builder_decodes_them() {
        let (path, scenes) = written("decoded");
        let read = read(&path, TimeOfDay::Day, false).unwrap();
        let Some(MapImage::Compressed { levels, .. }) = scenes[0].1.objects[0].materials[0].albedo.as_deref() else {
            panic!("blocks")
        };
        let want = texture("wall", 16, 16, 10).decode_rgba8(&levels[0]).unwrap();
        match read.objects[0].materials[0].albedo.as_deref() {
            Some(MapImage::Decoded(i)) => {
                assert_eq!(i.rgba, want);
                assert!(i.srgb && !i.repeat);
            }
            other => panic!("decoded, not {other:?}"),
        }
        // A cubemap of plain colour stays as it is.
        let cube = read.light.reflection.cube.as_ref().unwrap();
        assert!(!cube.bc6h && cube.srgb);
        assert_eq!(cube.mips[0][3], vec![13; 64]);
        // BC6H blocks are decoded, level by level.
        let probe = read.probes[0].reflection.cube.as_ref().unwrap();
        assert!(!probe.bc6h);
        assert_eq!(probe.faces[0].len(), 8 * 8 * 4);
        assert_eq!(probe.mips[0][5].len(), 4 * 4 * 4);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_newer_map_file_is_refused() {
        let (path, _) = written("newer");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[16..20].copy_from_slice(&(VERSION + 1).to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        let e = read(&path, TimeOfDay::Day, true).unwrap_err();
        assert!(format!("{e:#}").contains("newer"), "{e:#}");
        assert!(!is_older(&path));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
