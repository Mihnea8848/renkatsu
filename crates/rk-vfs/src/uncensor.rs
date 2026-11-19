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
//! `<penis>` and `<balls>` entries are read the same way, into their own
//! dictionaries, each with the game's own mosaic part first
//! (`Default.Penis`, `Default.Balls`).

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
    pub const PENIS_GUID: &str = "Default.Penis";
    pub const BALLS_GUID: &str = "Default.Balls";
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

/// `PenisData` and `BallsData`: a penis or balls a zipmod offers. Both
/// hold the same fields, and neither names a sex.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub guid: String,
    pub display_name: String,
    /// Bundle with the prefab the meshes come from.
    pub file: String,
    pub asset: String,
    pub allow_random: bool,
    pub color_match: Vec<ColorMatch>,
    /// The zipmod it came from; `None` for the game's own.
    pub mod_idx: Option<u32>,
}

impl Part {
    /// `PenisData(guid, displayName)`: the game's own part, which is the
    /// mosaic on the male body prefab.
    pub fn builtin(guid: &str, display_name: &str) -> Self {
        Self {
            guid: guid.into(),
            display_name: display_name.into(),
            file: defaults::OO_BASE.into(),
            asset: defaults::ASSET_MALE.into(),
            allow_random: false,
            color_match: Vec::new(),
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
    pub penis_guid: Option<String>,
    pub balls_guid: Option<String>,
}

/// What one manifest contributes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ManifestData {
    pub bodies: Vec<Body>,
    pub penises: Vec<Part>,
    pub balls: Vec<Part>,
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

/// `<allowRandom>`: anything but "false" or "0" allows it, the element
/// missing included.
fn allow_random(node: roxmltree::Node) -> bool {
    !text(node, "allowRandom").is_some_and(|v| v.to_lowercase() == "false" || v == "0")
}

/// The `<colorMatch>` children of an element; one missing a field is
/// dropped (`ColorMatchPart.Verify`).
fn color_matches(node: roxmltree::Node) -> Vec<ColorMatch> {
    node.children()
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
        .collect()
}

/// `PenisData(XContainer)` and `BallsData(XContainer)`, with the checks
/// `PopulateUncensorLists` makes: all four of these are needed.
fn part(node: roxmltree::Node) -> Option<Part> {
    Some(Part {
        guid: text(node, "guid")?,
        display_name: text(node, "displayName")?,
        file: text(node, "file")?,
        asset: text(node, "asset")?,
        allow_random: allow_random(node),
        color_match: color_matches(node),
        mod_idx: None,
    })
}

/// `BodyData(XContainer)` and the checks `PopulateUncensorLists` makes.
fn body(node: roxmltree::Node) -> Option<Body> {
    use defaults::*;
    let guid = text(node, "guid")?;
    let sex = u8::from(text(node, "sex").is_some_and(|s| s.to_lowercase() == "female"));
    let male = sex == 0;
    let allow_random = allow_random(node);
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
        b.color_match = color_matches(oo);
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
    out.penises = root
        .children()
        .filter(|c| c.has_tag_name("penis"))
        .filter_map(part)
        .collect();
    out.balls = root
        .children()
        .filter(|c| c.has_tag_name("balls"))
        .filter_map(part)
        .collect();
    out.migrations = root
        .children()
        .filter(|c| c.has_tag_name("migration"))
        .filter_map(|m| {
            Some(Migration {
                uncensor_guid: text(m, "guidUncensor")?,
                body_guid: text(m, "guidBody")?,
                penis_guid: text(m, "guidPenis"),
                balls_guid: text(m, "guidBalls"),
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

/// `BodyDictionary`, `PenisDictionary`, `BallsDictionary` and
/// `MigrationDictionary`.
#[derive(Debug, Clone, Default)]
pub struct Uncensors {
    /// In dictionary order: the two built-in bodies, then the mods'.
    pub bodies: Vec<Body>,
    /// The game's own mosaic penis first, then the mods'.
    pub penises: Vec<Part>,
    /// The game's own mosaic balls first, then the mods'.
    pub balls: Vec<Part>,
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
            penises: vec![Part::builtin(defaults::PENIS_GUID, "Mosaic Penis")],
            balls: vec![Part::builtin(defaults::BALLS_GUID, "Mosaic Balls")],
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
            for (from, into) in [(data.penises, &mut out.penises), (data.balls, &mut out.balls)] {
                for mut p in from {
                    p.mod_idx = mod_idx;
                    match into.iter_mut().find(|x| x.guid == p.guid) {
                        Some(slot) => *slot = p,
                        None => into.push(p),
                    }
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

    pub fn penis(&self, guid: &str) -> Option<&Part> {
        self.penises.iter().find(|p| p.guid == guid)
    }

    pub fn balls(&self, guid: &str) -> Option<&Part> {
        self.balls.iter().find(|p| p.guid == guid)
    }

    /// Reads the manifests of the mods in use. What each zipmod holds is
    /// remembered in a cache file of its own, keyed by path, size and time.
    pub fn load(index: &Index) -> Self {
        #[derive(Default, Serialize, Deserialize)]
        struct Cache {
            version: u32,
            zips: HashMap<String, (FileStamp, ManifestData)>,
        }
        const VERSION: u32 = 2;
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
                (data != &ManifestData::default()).then(|| (m.guid.clone(), Some(i as u32), data.clone()))
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
            <penis>
                <guid>SAC.dong</guid>
                <displayName>SAC Dong</displayName>
                <file>chara/sac_dong.unity3d</file>
                <asset>sac_dong</asset>
                <colorMatch><object>o_dankon</object><material>cm_m_dankon</material>
                    <materialCreate>cm_m_dankon_create</materialCreate>
                    <mainTex>cm_dankon_00_t</mainTex><colorMask>cm_dankon_00_mc</colorMask></colorMatch>
            </penis>
            <penis><guid>no.name</guid><file>x</file><asset>y</asset></penis>
            <penis><guid>no.asset</guid><displayName>No asset</displayName><file>x</file></penis>
            <balls>
                <guid>SAC.tama</guid>
                <displayName>SAC Tama</displayName>
                <file>chara/sac_tama.unity3d</file>
                <asset>sac_tama</asset>
                <allowRandom>0</allowRandom>
            </balls>
            <migration><guidUncensor>old</guidUncensor><guidBody>SAC.innie</guidBody>
                <guidPenis>SAC.dong</guidPenis></migration>
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
                body_guid: "SAC.innie".into(),
                penis_guid: Some("SAC.dong".into()),
                balls_guid: None,
            }]
        );
        assert_eq!(parse("<manifest><guid>x</guid></manifest>"), ManifestData::default());
        // A penis needs a GUID, a name, a file and an asset.
        assert_eq!(data.penises.len(), 1);
        let dong = &data.penises[0];
        assert_eq!(dong.display_name, "SAC Dong");
        assert_eq!(dong.file, "chara/sac_dong.unity3d");
        assert!(dong.allow_random);
        assert_eq!(dong.color_match.len(), 1);
        assert_eq!(dong.color_match[0].main_tex, "cm_dankon_00_t");
        assert_eq!(data.balls.len(), 1);
        assert!(!data.balls[0].allow_random);
    }

    #[test]
    fn dictionary_follows_the_manifest_order() {
        let one = |guid: &str| ManifestData {
            bodies: vec![Body {
                guid: guid.into(),
                oo_base: "x".into(),
                ..Default::default()
            }],
            ..Default::default()
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
        // The game's own mosaic parts come first and are never random.
        assert_eq!(u.penises[0].guid, defaults::PENIS_GUID);
        assert_eq!(u.balls[0].guid, defaults::BALLS_GUID);
        assert!(!u.penises[0].allow_random);
        assert_eq!(u.penises[0].file, defaults::OO_BASE);
        assert_eq!(u.penis("SAC.dong").unwrap().mod_idx, Some(1));
        assert_eq!(u.balls("SAC.tama").unwrap().display_name, "SAC Tama");
        assert!(u.penis("no.name").is_none());
    }
}
