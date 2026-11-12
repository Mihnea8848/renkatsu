// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Character assembly, following the order the game's loader uses.

use crate::loader::{assign_weights, Loader};
use crate::scene::{euler_deg, Instance, Part, Scene};
use crate::shape::{self, Rules, ShapeSet};
use anyhow::{Context, Result};
use glam::Vec3;
use rk_card::Card;
use rk_vfs::{cat, resolve_infos, Game, Item, Lookup, ResolveInfo};
use std::collections::HashMap;

const BASE: &str = "chara/oo_base.unity3d";

#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Which of the card's outfits to dress the character in.
    pub outfit: usize,
    pub clothes: bool,
    pub accessories: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            outfit: 0,
            clothes: true,
            accessories: true,
        }
    }
}

pub struct Character {
    pub scene: Scene,
    /// Root node everything hangs from.
    pub top: usize,
    /// Skeleton bones by name.
    pub bones: HashMap<String, usize>,
    pub warnings: Vec<String>,
}

struct Builder<'g> {
    loader: Loader<'g>,
    game: &'g Game,
    infos: HashMap<String, ResolveInfo>,
    scene: Scene,
}

impl<'g> Builder<'g> {
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
            Ok(inst) => Some(inst),
            Err(e) => {
                self.loader.warn(format!("{label}: {e:#}"));
                None
            }
        }
    }
}

/// Clothing prefabs hold a fully worn variant and half-off variants; only
/// the worn one is shown.
fn hide_undressed_states(scene: &mut Scene, inst: &Instance) {
    for &n in &inst.nodes {
        if matches!(
            scene.nodes[n].name.as_str(),
            "n_top_b" | "n_bot_b" | "n_bot_c" | "n_panst_b"
        ) {
            scene.nodes[n].active = false;
        }
    }
}

pub fn build(game: &Game, card: &Card, opts: &BuildOptions) -> Result<Character> {
    let mut b = Builder {
        loader: Loader::new(game),
        game,
        infos: resolve_infos(&card.ext),
        scene: Scene::default(),
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
    let mut bones = b.scene.name_map(body_root);

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

    // Body mesh.
    let body_name = if female { "p_cf_body_00" } else { "p_cm_body_00" };
    let body = b
        .loader
        .load_prefab(&mut b.scene, BASE, body_name, None, Some(top), Part::Body)?;
    assign_weights(&mut b.scene, &body, &bones, "cf_j_root");

    // Parts the game only shows in special states.
    for name in [
        "n_dankon",
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
    match ShapeSet::load(game, BASE, "cf_anmShapeBody", "cf_custombody", None) {
        Ok(mut set) => {
            set.vars.insert("headsize".into(), if female { 1.0 } else { 0.91 });
            set.vars.insert("necksize".into(), if female { 1.0 } else { 0.91 });
            for (i, v) in card.body.shape_value_body.iter().enumerate() {
                set.set(i, *v);
            }
            set.apply(
                &mut b.scene,
                body_root,
                &Rules::parse(shape::BODY_RULES)?,
                corrections.as_deref(),
            );
        }
        Err(e) => b.loader.warn(format!("body sliders: {e:#}")),
    }

    // Head: its prefab carries the bone layout for this face type.
    if let Some(item) = b.item("head", "ChaFileFace.headId", cat::BO_HEAD, card.face.head_id) {
        if let Some(head) = b.part("head", item, None, Part::Head) {
            let src = b.scene.name_map(head.root);
            for (name, &dst) in &head_bones {
                if let Some(&s) = src.get(name) {
                    b.scene.nodes[dst].local = b.scene.nodes[s].local;
                }
            }
            b.scene.set_parent(head.root, Some(head_bone.root));
            assign_weights(&mut b.scene, &head, &head_bones, "cf_J_N_FaceRoot");
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

    // Hair hangs from the upper face bone and keeps its own bones.
    if let Some(&hair_parent) = head_bones.get("cf_J_FaceUp_ty") {
        for (i, hp) in card.hair.parts.iter().enumerate().take(4) {
            let label = format!("hair {}", ["back", "front", "side", "option"][i]);
            let prop = format!("ChaFileHair.{}", cat::HAIR_PROPS[i]);
            if let Some(item) = b.item(&label, &prop, cat::HAIR[i], hp.id) {
                b.part(&label, item, Some(hair_parent), Part::Hair(i as u8));
            }
        }
    }

    let outfit = opts.outfit.min(card.coordinates.len().saturating_sub(1));
    let mut accessory_nodes: HashMap<usize, HashMap<String, usize>> = HashMap::new();
    if let Some(co) = card.coordinates.get(outfit) {
        if opts.clothes {
            let shoes_type = rk_card::map_get(&card.status, "shoesType")
                .and_then(|v| v.as_i64())
                .unwrap_or(1);
            for (i, cp) in co.clothes.parts.iter().enumerate().take(9) {
                // Only one of indoor (7) and outdoor (8) shoes is worn.
                if (i == 7 && shoes_type != 0) || (i == 8 && shoes_type == 0) {
                    continue;
                }
                let label = format!("outfit{outfit}.{}", rk_card::CLOTHES_KIND_NAMES[i]);
                let prop = format!("outfit{outfit}.ChaFileClothes.{}", cat::CLOTHES_PROPS[i]);
                let Some(item) = b.item(&label, &prop, cat::CLOTHES[i], cp.id) else {
                    continue;
                };
                let inst = b.part(&label, item, Some(top), Part::Clothes(i as u8));
                if let Some(inst) = &inst {
                    assign_weights(&mut b.scene, inst, &bones, "cf_j_root");
                    hide_undressed_states(&mut b.scene, inst);
                }
                // Sailor and jacket tops are assembled from three sub-parts.
                let kind = item.int("Kind").unwrap_or(0);
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
                        let Some(sub) = b.item(&sub_label, &sub_prop, cats[k], id) else {
                            continue;
                        };
                        if let Some(inst) = b.part(&sub_label, sub, Some(top), Part::Clothes(0)) {
                            assign_weights(&mut b.scene, &inst, &bones, "cf_j_root");
                            hide_undressed_states(&mut b.scene, &inst);
                        }
                    }
                }
            }
        }

        if opts.accessories {
            // Attachment points are skeleton and head nodes named a_n_*.
            let mut points = bones.clone();
            for (k, v) in &head_bones {
                points.entry(k.clone()).or_insert(*v);
            }
            for (i, acc) in co.accessory.parts.iter().enumerate() {
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
                    assign_weights(&mut b.scene, &inst, &head_bones, "cf_J_N_FaceRoot");
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
                accessory_nodes.insert(i, b.scene.name_map(inst.root));
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
    let mods = crate::abmx::read(&card.ext);
    if !mods.is_empty() {
        crate::abmx::apply(&mut b.scene, &mods, outfit, &bones, &accessory_nodes);
    }
    Ok(Character {
        scene: b.scene,
        top,
        bones,
        warnings: b.loader.warnings,
    })
}
