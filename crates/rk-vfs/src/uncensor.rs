// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bodies zipmods offer to the Uncensor Selector plugin, from the
//! `<KK_UncensorSelector>` element of their manifests.
//!
//! `UncensorSelector.PopulateUncensorLists` fills `BodyDictionary` with the
//! two built-in bodies and then walks `Sideloader.Manifests` in its order.
//! Sideloader adds manifests sorted by GUID (`orderby x.Key`, the culture
//! string comparison), so that is the order kept here: it decides which
//! body a seeded random pick lands on.
//!
//! Penis and balls entries are not read: the game hides those parts on a
//! character at rest.

use crate::index::{cache_file, stamp, FileStamp, Index};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::io::Read;

/// `UncensorSelector.Defaults`.
pub mod defaults {
    pub const OO_BASE: &str = "chara/oo_base.unity3d";
    pub const MM_BASE: &str = "chara/mm_base.unity3d";
    pub const ASSET_MALE: &str = "p_cm_body_00";
    pub const ASSET_FEMALE: &str = "p_cf_body_00";
    pub const BODY_MAIN_TEX: &str = "cf_body_00_t";
    pub const BODY_COLOR_MASK_MALE: &str = "cm_body_00_mc";
    pub const BODY_COLOR_MASK_FEMALE: &str = "cf_body_00_mc";
    pub const NORMALS: &str = "p_cf_body_00_Nml";
    pub const BODY_MATERIAL_MALE: &str = "cm_m_body";
    pub const BODY_MATERIAL_FEMALE: &str = "cf_m_body";
    pub const BODY_MATERIAL_CREATE: &str = "cf_m_body_create";
    pub const BODY_FEMALE_GUID: &str = "Default.Body.Female";
    pub const BODY_MALE_GUID: &str = "Default.Body.Male";
}

/// `ColorMatchPart`: a mesh that is recoloured to the skin.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ColorMatch {
    pub object: String,
    pub material: String,
    pub material_create: String,
    pub main_tex: String,
    pub color_mask: String,
}

/// `BodyData`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub guid: String,
    pub display_name: String,
    /// Bundle with the body prefab and its textures.
    pub oo_base: String,
    /// Bundle with the body materials.
    pub mm_base: String,
    pub normals: String,
    /// 0 male, 1 female.
    pub sex: u8,
    pub allow_random: bool,
    pub main_tex: String,
    pub color_mask: String,
    pub material: String,
    pub material_create: String,
    pub asset: String,
    pub overlay: Option<String>,
    pub underlay: Option<String>,
    pub color_match: Vec<ColorMatch>,
    pub additional_parts: Vec<String>,
    /// The zipmod it came from; `None` for the built-in bodies.
    pub mod_idx: Option<u32>,
}

impl Body {
    /// `BodyData(sex, guid, displayName)`: a body of the game itself.
    pub fn builtin(sex: u8, guid: &str, display_name: &str) -> Self {
        use defaults::*;
        let male = sex == 0;
        Self {
            guid: guid.into(),
            display_name: display_name.into(),
            oo_base: OO_BASE.into(),
            mm_base: MM_BASE.into(),
            normals: NORMALS.into(),
            sex,
            allow_random: false,
            main_tex: BODY_MAIN_TEX.into(),
            color_mask: if male {
                BODY_COLOR_MASK_MALE
            } else {
                BODY_COLOR_MASK_FEMALE
            }
            .into(),
            material: if male { BODY_MATERIAL_MALE } else { BODY_MATERIAL_FEMALE }.into(),
            material_create: BODY_MATERIAL_CREATE.into(),
            asset: if male { ASSET_MALE } else { ASSET_FEMALE }.into(),
            overlay: None,
            underlay: None,
            color_match: Vec::new(),
            additional_parts: Vec::new(),
            mod_idx: None,
        }
    }
}

/// `MigrationData`: where the selection of an old card (data version 1)
/// went.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Migration {
    pub uncensor_guid: String,
    pub body_guid: String,
}

/// What one manifest contributes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ManifestData {
    pub bodies: Vec<Body>,
    pub migrations: Vec<Migration>,
}

/// Text of a child element; `None` when it is missing or blank
/// (`IsNullOrWhiteSpace`).
fn text(node: roxmltree::Node, name: &str) -> Option<String> {
    node.children()
        .find(|c| c.has_tag_name(name))
        .map(|c| c.text().unwrap_or(""))
        .filter(|t| !t.trim().is_empty())
        .map(str::to_string)
}

/// `BodyData(XContainer)` and the checks `PopulateUncensorLists` makes.
fn body(node: roxmltree::Node) -> Option<Body> {
    use defaults::*;
    let guid = text(node, "guid")?;
    let sex = u8::from(text(node, "sex").is_some_and(|s| s.to_lowercase() == "female"));
    let male = sex == 0;
    let allow_random = !text(node, "allowRandom").is_some_and(|v| v.to_lowercase() == "false" || v == "0");
    let mut b = Body {
        display_name: text(node, "displayName").unwrap_or_else(|| guid.clone()),
        guid,
        sex,
        allow_random,
        ..Body::builtin(sex, "", "")
    };
    // The plugin reads the bundle of materials from the same `oo_base`
    // element as everything else.
    if let Some(oo) = node.children().find(|c| c.has_tag_name("oo_base")) {
        let or = |name: &str, default: &str| text(oo, name).unwrap_or_else(|| default.to_string());
        b.oo_base = or("file", OO_BASE);
        b.asset = or("asset", if male { ASSET_MALE } else { ASSET_FEMALE });
        b.main_tex = or("mainTex", BODY_MAIN_TEX);
        b.color_mask = or(
            "colorMask",
            if male {
                BODY_COLOR_MASK_MALE
            } else {
                BODY_COLOR_MASK_FEMALE
            },
        );
        b.normals = or("normals", NORMALS);
        // These two are kept as written, blank or not.
        let raw = |name: &str| {
            oo.children()
                .find(|c| c.has_tag_name(name))
                .map(|c| c.text().unwrap_or("").to_string())
        };
        b.overlay = raw("uncensorOverlay");
        b.underlay = raw("uncensorUnderlay");
        b.additional_parts = oo
            .children()
            .filter(|c| c.has_tag_name("additionalPart"))
            .filter_map(|c| c.text())
            .filter(|t| !t.trim().is_empty())
            .map(str::to_string)
            .collect();
        b.color_match = oo
            .children()
            .filter(|c| c.has_tag_name("colorMatch"))
            .filter_map(|c| {
                Some(ColorMatch {
                    object: text(c, "object")?,
                    material: text(c, "material")?,
                    material_create: text(c, "materialCreate")?,
                    main_tex: text(c, "mainTex")?,
                    color_mask: text(c, "colorMask")?,
                })
            })
            .collect();
        b.mm_base = or("mm_base", MM_BASE);
        b.material = or("material", if male { BODY_MATERIAL_MALE } else { BODY_MATERIAL_FEMALE });
        b.material_create = or("materialCreate", BODY_MATERIAL_CREATE);
    }
    // "Body was not loaded because oo_base is the default."
    (b.oo_base != OO_BASE).then_some(b)
}

/// Reads the `<KK_UncensorSelector>` element of a manifest.
pub fn parse(manifest: &str) -> ManifestData {
    let mut out = ManifestData::default();
    let Ok(doc) = roxmltree::Document::parse(manifest.trim_start_matches('\u{feff}')) else {
        return out;
    };
    let Some(root) = doc
        .root_element()
        .children()
        .find(|c| c.has_tag_name("KK_UncensorSelector"))
    else {
        return out;
    };
    out.bodies = root
        .children()
        .filter(|c| c.has_tag_name("body"))
        .filter_map(body)
        .collect();
    out.migrations = root
        .children()
        .filter(|c| c.has_tag_name("migration"))
        .filter_map(|m| {
            Some(Migration {
                uncensor_guid: text(m, "guidUncensor")?,
                body_guid: text(m, "guidBody")?,
            })
        })
        .collect();
    out
}

/// Order of two manifest GUIDs in `Sideloader.Manifests`. The game sorts
/// with the culture comparison, which ranks letters without regard to
/// case first. This is that rule for letters and digits; how it ranks
/// punctuation against them is not reproduced.
pub fn guid_order(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| b.cmp(a))
}

/// `BodyDictionary` and `MigrationDictionary`.
#[derive(Debug, Clone, Default)]
pub struct Uncensors {
    /// In dictionary order: the two built-in bodies, then the mods'.
    pub bodies: Vec<Body>,
    pub migrations: HashMap<String, Migration>,
    /// `AllAdditionalParts`: mesh names some body adds.
    pub additional_parts: HashSet<String>,
}

impl Uncensors {
    /// Builds the dictionaries from manifests given as (GUID, mod, data).
    pub fn build(mut manifests: Vec<(String, Option<u32>, ManifestData)>) -> Self {
        let mut out = Self {
            bodies: vec![
                Body::builtin(0, defaults::BODY_MALE_GUID, "Default Body M"),
                Body::builtin(1, defaults::BODY_FEMALE_GUID, "Default Body F"),
            ],
            ..Default::default()
        };
        manifests.sort_by(|a, b| guid_order(&a.0, &b.0));
        for (_, mod_idx, data) in manifests {
            for mut body in data.bodies {
                body.mod_idx = mod_idx;
                out.additional_parts.extend(body.additional_parts.iter().cloned());
                // Assigning to an existing key keeps its place.
                match out.bodies.iter_mut().find(|b| b.guid == body.guid) {
                    Some(slot) => *slot = body,
                    None => out.bodies.push(body),
                }
            }
            for m in data.migrations {
                out.migrations.insert(m.uncensor_guid.clone(), m);
            }
        }
        out
    }

    pub fn body(&self, guid: &str) -> Option<&Body> {
        self.bodies.iter().find(|b| b.guid == guid)
    }

    /// Reads the manifests of the mods in use. What each zipmod holds is
    /// remembered in a cache file of its own, keyed by path, size and time.
    pub fn load(index: &Index) -> Self {
        #[derive(Default, Serialize, Deserialize)]
        struct Cache {
            version: u32,
            zips: HashMap<String, (FileStamp, ManifestData)>,
        }
        const VERSION: u32 = 1;
        let file = cache_file(&index.game_dir, "uncensor");
        let mut cache: Cache = std::fs::read(&file)
            .ok()
            .and_then(|b| rmp_serde::from_slice(&b).ok())
            .filter(|c: &Cache| c.version == VERSION)
            .unwrap_or_default();
        cache.version = VERSION;
        let stamps: Vec<FileStamp> = index
            .mods
            .iter()
            .map(|m| stamp(&index.game_dir.join(&m.path)))
            .collect();
        let todo: Vec<usize> = (0..index.mods.len())
            .filter(|&i| cache.zips.get(&index.mods[i].path).is_none_or(|(s, _)| *s != stamps[i]))
            .collect();
        let scanned: Vec<(usize, ManifestData)> = todo
            .par_iter()
            .map(|&i| {
                let read = || -> Option<String> {
                    let f = std::fs::File::open(index.game_dir.join(&index.mods[i].path)).ok()?;
                    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(f)).ok()?;
                    let mut manifest = String::new();
                    zip.by_name("manifest.xml").ok()?.read_to_string(&mut manifest).ok()?;
                    Some(manifest)
                };
                (i, read().map(|m| parse(&m)).unwrap_or_default())
            })
            .collect();
        let dirty = !scanned.is_empty();
        for (i, data) in scanned {
            cache.zips.insert(index.mods[i].path.clone(), (stamps[i].clone(), data));
        }
        if dirty {
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir).ok();
            }
            if let Ok(bytes) = rmp_serde::to_vec(&cache) {
                std::fs::write(&file, bytes).ok();
            }
        }
        let manifests = index
            .mods
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                let (_, data) = cache.zips.get(&m.path)?;
                (!data.bodies.is_empty() || !data.migrations.is_empty())
                    .then(|| (m.guid.clone(), Some(i as u32), data.clone()))
            })
            .collect();
        Self::build(manifests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"<manifest schema-ver="1">
        <guid>SAC.ucspack</guid>
        <KK_UncensorSelector>
            <body>
                <guid>SAC.innie</guid>
                <displayName>SAC Innie</displayName>
                <sex>Female</sex>
                <allowRandom>True</allowRandom>
                <oo_base>
                    <file>chara/oo_base_sacinnie.unity3d</file>
                    <asset>p_cf_body_00</asset>
                    <mainTex>cf_body_00_t</mainTex>
                    <additionalPart>o_extra</additionalPart>
                    <colorMatch><object>o_x</object><material>m</material><materialCreate>mc</materialCreate>
                        <mainTex>t</mainTex><colorMask>c</colorMask></colorMatch>
                    <colorMatch><object>o_y</object></colorMatch>
                </oo_base>
            </body>
            <body>
                <guid>SAC.kupaa</guid>
                <allowRandom>False</allowRandom>
                <oo_base><file>chara/oo_base_sackupaa.unity3d</file></oo_base>
            </body>
            <body><guid>no.bundle</guid><sex>Female</sex></body>
            <body><displayName>No guid</displayName></body>
            <migration><guidUncensor>old</guidUncensor><guidBody>SAC.innie</guidBody></migration>
            <migration><guidUncensor>broken</guidUncensor></migration>
        </KK_UncensorSelector>
    </manifest>"#;

    #[test]
    fn bodies_are_read_with_the_plugin_defaults() {
        let data = parse(MANIFEST);
        assert_eq!(data.bodies.len(), 2);
        let innie = &data.bodies[0];
        assert_eq!(innie.guid, "SAC.innie");
        assert_eq!(innie.sex, 1);
        assert!(innie.allow_random);
        assert_eq!(innie.oo_base, "chara/oo_base_sacinnie.unity3d");
        assert_eq!(innie.color_mask, defaults::BODY_COLOR_MASK_FEMALE);
        assert_eq!(innie.mm_base, defaults::MM_BASE);
        assert_eq!(innie.material, defaults::BODY_MATERIAL_FEMALE);
        assert_eq!(innie.additional_parts, vec!["o_extra"]);
        // An incomplete colour match is dropped.
        assert_eq!(innie.color_match.len(), 1);
        // No sex means male, and the name falls back to the GUID.
        let kupaa = &data.bodies[1];
        assert_eq!(kupaa.sex, 0);
        assert!(!kupaa.allow_random);
        assert_eq!(kupaa.display_name, "SAC.kupaa");
        assert_eq!(kupaa.asset, defaults::ASSET_MALE);
        assert_eq!(kupaa.color_mask, defaults::BODY_COLOR_MASK_MALE);
        assert_eq!(
            data.migrations,
            vec![Migration {
                uncensor_guid: "old".into(),
                body_guid: "SAC.innie".into()
            }]
        );
        assert_eq!(parse("<manifest><guid>x</guid></manifest>"), ManifestData::default());
    }

    #[test]
    fn dictionary_follows_the_manifest_order() {
        let one = |guid: &str| ManifestData {
            bodies: vec![Body {
                guid: guid.into(),
                oo_base: "x".into(),
                ..Default::default()
            }],
            migrations: Vec::new(),
        };
        let mut again = one("b1");
        again.bodies[0].display_name = "newer".into();
        let u = Uncensors::build(vec![
            ("uncensor.KK_LO".into(), Some(0), one("lo")),
            ("SAC.ucspack".into(), Some(1), parse(MANIFEST)),
            ("bp.one".into(), Some(2), one("b1")),
            ("anon.x".into(), Some(3), one("an")),
            ("zz".into(), Some(4), again),
        ]);
        let guids: Vec<&str> = u.bodies.iter().map(|b| b.guid.as_str()).collect();
        assert_eq!(
            guids,
            [
                "Default.Body.Male",
                "Default.Body.Female",
                "an",
                "b1",
                "SAC.innie",
                "SAC.kupaa",
                "lo"
            ]
        );
        // A repeated GUID replaces the body where it stands.
        assert_eq!(u.body("b1").unwrap().display_name, "newer");
        assert_eq!(u.body("b1").unwrap().mod_idx, Some(4));
        assert!(u.additional_parts.contains("o_extra"));
        assert_eq!(u.migrations["old"].body_guid, "SAC.innie");
    }
}
