// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runtime access to bundles through the index: open by virtual path, find
//! assets by name, follow references between objects and bundles.

use crate::index::{norm, BundleLoc, Index};
use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use rk_unityfs::objects::Texture2D;
use rk_unityfs::{class, Bundle, ObjectInfo, PPtr, SerializedFile, UObject, UValue};
use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;

pub struct AssetFile {
    pub cab: String,
    pub vpath: String,
    pub sf: SerializedFile,
    container: Arc<Bundle>,
}

pub struct LoadedBundle {
    pub vpath: String,
    pub mod_idx: Option<u32>,
    pub files: Vec<Arc<AssetFile>>,
    /// Lowercase asset name (file stem of the container path) to objects.
    names: HashMap<String, Vec<(usize, i64)>>,
}

/// A decoded-on-demand object inside a loaded file.
#[derive(Clone)]
pub struct ObjRef {
    pub file: Arc<AssetFile>,
    pub info: ObjectInfo,
}

impl ObjRef {
    pub fn class_id(&self) -> i32 {
        self.info.class_id
    }
    pub fn read(&self) -> Result<UObject> {
        Ok(self.file.sf.read(&self.info)?)
    }
    pub fn name(&self) -> String {
        self.file.sf.name(&self.info).unwrap_or_default()
    }
    /// Identity usable as a cache key.
    pub fn key(&self) -> (usize, i64) {
        (Arc::as_ptr(&self.file) as usize, self.info.path_id)
    }
}

/// Virtual path plus the mod it comes from (`None` for the game).
type BundleKey = (String, Option<u32>);

pub struct Assets {
    pub index: Arc<Index>,
    loaded: Mutex<HashMap<BundleKey, Arc<LoadedBundle>>>,
    by_cab: Mutex<HashMap<String, Arc<AssetFile>>>,
}

fn stem(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file).to_lowercase()
}

impl Assets {
    pub fn new(index: Arc<Index>) -> Self {
        Self {
            index,
            loaded: Default::default(),
            by_cab: Default::default(),
        }
    }

    fn open(&self, vpath: &str, loc: &BundleLoc) -> Result<Arc<LoadedBundle>> {
        let key = (vpath.to_string(), loc.mod_idx);
        if let Some(b) = self.loaded.lock().get(&key) {
            return Ok(b.clone());
        }
        let bundle = match loc.mod_idx {
            None => Bundle::open(self.index.game_dir.join("abdata").join(&loc.path))?,
            Some(m) => {
                let zip_path = self.index.game_dir.join(&self.index.mods[m as usize].path);
                let mut zip = zip::ZipArchive::new(std::fs::File::open(&zip_path)?)?;
                let mut entry = zip.by_name(&loc.path)?;
                let mut bytes = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut bytes)?;
                Bundle::from_bytes(bytes)?
            }
        };
        let bundle = Arc::new(bundle);
        let mut files = Vec::new();
        for n in bundle.nodes.iter().filter(|n| n.is_serialized()) {
            let sf = SerializedFile::parse(bundle.read_node(n)?)?;
            files.push(Arc::new(AssetFile {
                cab: n.path.to_lowercase(),
                vpath: vpath.to_string(),
                sf,
                container: bundle.clone(),
            }));
        }
        let mut names: HashMap<String, Vec<(usize, i64)>> = HashMap::new();
        for (fi, f) in files.iter().enumerate() {
            for o in f.sf.objects_of(class::ASSET_BUNDLE) {
                let Ok(ab) = f.sf.read(o) else { continue };
                for e in ab.array("m_Container").unwrap_or_default() {
                    let (Some(path), Some(p)) = (
                        e.get("first").and_then(UValue::as_str),
                        e.get("second").and_then(|s| s.get("asset")).and_then(UValue::as_pptr),
                    ) else {
                        continue;
                    };
                    if p.file_id == 0 {
                        names.entry(stem(path)).or_default().push((fi, p.path_id));
                    }
                }
            }
        }
        let lb = Arc::new(LoadedBundle {
            vpath: vpath.to_string(),
            mod_idx: loc.mod_idx,
            files,
            names,
        });
        let mut cabs = self.by_cab.lock();
        for f in &lb.files {
            cabs.entry(f.cab.clone()).or_insert_with(|| f.clone());
        }
        self.loaded.lock().insert(key, lb.clone());
        Ok(lb)
    }

    /// All sources of a virtual bundle path, zipmods first. A bundle that
    /// fails to open is skipped.
    pub fn bundles(&self, vpath: &str) -> Vec<Arc<LoadedBundle>> {
        let vpath = norm(vpath);
        let Some(locs) = self.index.bundles.get(&vpath) else {
            return Vec::new();
        };
        locs.iter().filter_map(|l| self.open(&vpath, l).ok()).collect()
    }

    /// Sources of `vpath` restricted to one mod (or the game when `None`),
    /// falling back to every source.
    fn sources(&self, vpath: &str, prefer_mod: Option<u32>) -> Vec<Arc<LoadedBundle>> {
        let mut all = self.bundles(vpath);
        all.sort_by_key(|b| b.mod_idx != prefer_mod);
        all
    }

    /// Finds an asset by name, as the game's `LoadAsset(bundle, name)` does.
    /// `prefer_mod` is the mod the request comes from, searched first.
    pub fn find(&self, vpath: &str, name: &str, class_id: Option<i32>, prefer_mod: Option<u32>) -> Option<ObjRef> {
        let want = stem(name);
        let sources = self.sources(vpath, prefer_mod);
        for b in &sources {
            if let Some(hits) = b.names.get(&want) {
                for &(fi, id) in hits {
                    let f = &b.files[fi];
                    if let Some(info) = f.sf.object(id) {
                        if class_id.is_none_or(|c| c == info.class_id) {
                            return Some(ObjRef {
                                file: f.clone(),
                                info: *info,
                            });
                        }
                    }
                }
            }
        }
        // Not a named asset: fall back to scanning object names.
        let class_id = class_id?;
        for b in &sources {
            for f in &b.files {
                for info in f.sf.objects_of(class_id) {
                    if f.sf.name(info).is_some_and(|n| n.eq_ignore_ascii_case(name)) {
                        return Some(ObjRef {
                            file: f.clone(),
                            info: *info,
                        });
                    }
                }
            }
        }
        None
    }

    /// Follows a reference from an object in `from`.
    pub fn deref(&self, from: &Arc<AssetFile>, p: PPtr) -> Option<ObjRef> {
        if p.is_null() {
            return None;
        }
        let file = if p.file_id == 0 {
            from.clone()
        } else {
            let ext = from.sf.externals.get(p.file_id as usize - 1)?;
            let cab = ext.file_name().to_lowercase();
            let known = self.by_cab.lock().get(&cab).cloned();
            match known {
                Some(f) => f,
                None => {
                    let vpath = self.index.cabs.get(&cab)?.clone();
                    self.bundles(&vpath);
                    self.by_cab.lock().get(&cab).cloned()?
                }
            }
        };
        let info = *file.sf.object(p.path_id)?;
        Some(ObjRef { file, info })
    }

    /// Reads a Texture2D and decodes it to RGBA8 (rows bottom-up).
    pub fn texture(&self, obj: &ObjRef) -> Result<(Texture2D, Vec<u8>)> {
        let tex = Texture2D::read(&obj.read()?)?;
        let pixels = match &tex.stream {
            None => tex.decode_rgba8(&tex.data)?,
            Some(s) => {
                let node = obj
                    .file
                    .container
                    .node(s.file_name())
                    .ok_or_else(|| anyhow!("texture {}: stream {} not in bundle", tex.name, s.path))?;
                let raw = obj.file.container.read_range(node.offset + s.offset, s.size)?;
                tex.decode_rgba8(&raw)?
            }
        };
        Ok((tex, pixels))
    }

    /// Name of the shader a material uses, following the reference.
    pub fn shader_name(&self, from: &Arc<AssetFile>, shader: PPtr) -> Option<String> {
        let obj = self.deref(from, shader)?;
        let v = obj.file.sf.read_until(&obj.info, "m_ParsedForm").ok()?;
        rk_unityfs::objects::shader_name(&v)
    }

    pub fn mod_guid(&self, mod_idx: Option<u32>) -> Option<&str> {
        mod_idx.map(|m| self.index.mods[m as usize].guid.as_str())
    }
}

/// Convenience bundle of everything needed to load content.
pub struct Game {
    pub assets: Assets,
    pub items: crate::items::ItemDb,
}

impl Game {
    pub fn open(cfg: &rk_core::Config) -> Result<Self> {
        let index = Arc::new(Index::load(cfg).context("building content index")?);
        let items = crate::items::ItemDb::build(&index);
        Ok(Self {
            assets: Assets::new(index),
            items,
        })
    }
}
