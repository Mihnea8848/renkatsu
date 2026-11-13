// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Character assembly, following the order the game's loader uses.

use crate::compose::{self, ClothColor, Decal};
use crate::loader::{assign_weights, Loader};
use crate::scene::{euler_deg, Image, Instance, Part, Scene, TexSlot};
use crate::shape::{self, Rules, ShapeSet};
use anyhow::{Context, Result};
use glam::Vec3;
use rk_card::Card;
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
    /// The scene's shading ramp: light amount to brightness.
    pub ramp: Option<std::sync::Arc<crate::scene::Image>>,
    /// How many MaterialEditor overrides found their material.
    pub material_edits: crate::material_editor::Applied,
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

    /// Puts a composed texture on a renderer, swapping in the material the
    /// game draws that surface with when it can be loaded.
    fn set_skin(&mut self, inst: &Instance, renderer: &str, draw: Option<crate::scene::MaterialInst>, image: Image) {
        let slot = TexSlot {
            image: Arc::new(image),
            scale: [1.0, 1.0],
            offset: [0.0, 0.0],
        };
        for &ri in &inst.renderers {
            let r = &mut self.scene.renderers[ri];
            if r.name != renderer {
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
    }

    /// Scene renderers a clothing prefab lists under one of its fields
    /// (`rendNormal01`...). The lists live in a script on the prefab root.
    fn listed_renderers(&mut self, inst: &Instance, field: &str) -> Vec<usize> {
        let mut out = Vec::new();
        for (_, script) in &inst.scripts {
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
        for r in self.listed_renderers(inst, "rendHair") {
            if let Some(m) = self.scene.renderers[r].materials.first_mut() {
                m.colors.insert("_Color".into(), solid(part.base_color));
                m.colors.insert("_Color2".into(), solid(part.start_color));
                m.colors.insert("_Color3".into(), solid(part.end_color));
                m.colors.insert("_LineColor".into(), part.outline_color);
            }
        }
        let count = match self.script_field(inst, "acsDefColor") {
            Some(rk_unityfs::UValue::Array(a)) => a.len().min(3),
            _ => 0,
        };
        for r in self.listed_renderers(inst, "rendAccessory") {
            if let Some(m) = self.scene.renderers[r].materials.first_mut() {
                for (j, name) in ["_Color", "_Color2", "_Color3"].iter().enumerate().take(count) {
                    if let Some(c) = part.acs_color.get(j) {
                        m.colors.insert((*name).into(), solid(*c));
                    }
                }
            }
        }
    }

    /// Accessory colours, for the colour slots the accessory says it uses.
    fn color_accessory(&mut self, inst: &Instance, acc: &rk_card::AccessoryPart, hair_root: [f32; 4]) {
        let uses = |b: &Self, field: &str| b.script_field(inst, field).and_then(|v| v.as_bool()).unwrap_or(false);
        let slots = [
            (uses(self, "useColor01"), "_Color"),
            (uses(self, "useColor02"), "_Color2"),
            (uses(self, "useColor03"), "_Color3"),
        ];
        for r in self.listed_renderers(inst, "rendNormal") {
            if let Some(m) = self.scene.renderers[r].materials.first_mut() {
                for (k, (used, name)) in slots.iter().enumerate() {
                    if let (true, Some(c)) = (*used, acc.color.get(k)) {
                        m.colors.insert((*name).into(), *c);
                    }
                }
            }
        }
        if let Some(c) = acc.color.get(3) {
            for r in self.listed_renderers(inst, "rendAlpha") {
                // A fully transparent fourth colour switches the part off.
                self.scene.renderers[r].enabled &= c[3] != 0.0;
                if let Some(m) = self.scene.renderers[r].materials.first_mut() {
                    m.colors.insert("_Color4".into(), *c);
                }
            }
        }
        // Hair-like accessories follow the hair's root colour.
        for r in self.listed_renderers(inst, "rendHair") {
            if let Some(m) = self.scene.renderers[r].materials.first_mut() {
                for name in ["_Color", "_Color2", "_Color3"] {
                    m.colors.insert(name.into(), hair_root);
                }
            }
        }
    }

    /// Eyebrows, eyeliner and the nose line: texture and colour per mesh.
    fn face_lines(&mut self, card: &Card, inst: &Instance) {
        let face = &card.face;
        let up = self.item_image(
            "eyeline",
            "ChaFileFace.eyelineUpId",
            cat::MT_EYELINE_UP,
            face.eyeline_up_id,
            "MainAB",
            "EyelineUpTex",
        );
        let up_shadow = self.item_image(
            "eyeline",
            "ChaFileFace.eyelineUpId",
            cat::MT_EYELINE_UP,
            face.eyeline_up_id,
            "MainAB",
            "EyelineShadowTex",
        );
        let down = self.item_image(
            "eyeline",
            "ChaFileFace.eyelineDownId",
            cat::MT_EYELINE_DOWN,
            face.eyeline_down_id,
            "MainAB",
            "EyelineDownTex",
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
                    // "None" in the maker: nothing is drawn.
                    None => {
                        m.textures.remove("_MainTex");
                        m.floats.insert("_Alpha".into(), 0.0);
                    }
                }
                if let Some(c) = color {
                    m.colors.insert("_Color".into(), *c);
                }
            }
        }
    }

    /// Applies the masks a top brings for what is worn under it: one cuts
    /// the body, one the bra, one the inner layer of the top itself. A top
    /// can also rule out a bra altogether.
    fn hide_under_top(
        &mut self,
        body: &Instance,
        tops: &[&Item],
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
        for item in tops {
            no_bra |= item.int("NotBra") == Some(1);
            for (k, (ab, tex)) in columns.iter().enumerate() {
                if let (Some(ab), Some(tex)) = (item.get(ab), item.get(tex)) {
                    if let Some(image) = self.loader.image_by_name(ab, tex, item.mod_idx) {
                        masks[k] = Some(image);
                    }
                }
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
        }
        if let Some((inst, item)) = bra {
            if no_bra {
                self.scene.nodes[inst.root].active = false;
            } else if let Some(image) = &bra_mask {
                // One-piece bras use the lower half of a taller mask.
                let (scale, offset) = if item.int("Coordinate") == Some(2) {
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
    fn dress(&mut self, inst: &Instance, item: &Item, colors: &[rk_card::ClothesColor], pattern_prop: &str) {
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
            let Some(c) = colors.get(k) else { continue };
            if c.pattern != 0 {
                *slot = self.item_image(
                    "pattern",
                    &format!("{pattern_prop}{k}"),
                    cat::MT_PATTERN,
                    c.pattern,
                    "MainTexAB",
                    "MainTex",
                );
            }
        }
        let color = |k: usize| {
            let c = colors.get(k).cloned().unwrap_or_default();
            ClothColor {
                base: c.base_color,
                pattern: patterns[k].as_deref(),
                pattern_color: c.pattern_color,
                tiling: c.tiling,
            }
        };
        let cloth = [color(0), color(1), color(2)];
        for (main_ab, main_tex, mask_ab, mask_tex, fields) in pairs {
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
            let image = Arc::new(compose::clothes(&main, &mask, &cloth));
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
    fn eyes(&mut self, card: &Card, inst: &Instance) {
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
            let iris = Arc::new(compose::eye(&compose::EyeInputs {
                size: 512,
                main: main.as_deref(),
                gradient: gradient.as_deref(),
                base: pupil.base_color,
                sub: pupil.sub_color,
                blend: pupil.grad_blend,
                gradient_offset_y: pupil.grad_offset_y,
                gradient_scale: pupil.grad_scale,
            }));
            for &ri in &inst.renderers {
                let r = &mut self.scene.renderers[ri];
                let Some(m) = r.materials.first_mut() else { continue };
                if r.name == iris_mesh {
                    m.textures.insert("_MainTex".into(), slot(iris.clone()));
                    for (tex, color, image, value) in [
                        ("_overtex1", "_overcolor1", &hl_up, face.hl_up_color),
                        ("_overtex2", "_overcolor2", &hl_down, face.hl_down_color),
                    ] {
                        match image {
                            Some(image) => drop(m.textures.insert(tex.into(), slot(image.clone()))),
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

    /// Builds the body skin texture from the card.
    fn skin_body(&mut self, card: &Card, inst: &Instance, female: bool) {
        let body = &card.body;
        let main = self.loader.image_by_name(BASE, "cf_body_00_t", None);
        let mask = self
            .loader
            .image_by_name(BASE, if female { "cf_body_00_mc" } else { "cm_body_00_mc" }, None);
        let create = self.loader.material_by_name(MATERIALS, "cf_m_body_create", None);
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
        let draw = self
            .loader
            .material_by_name(MATERIALS, if female { "cf_m_body" } else { "cm_m_body" }, None);
        self.set_skin(inst, "o_body_a", draw, image);
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
            // The second layer is the blush, which is an expression state.
            m.textures.remove("_overtex2");
            m.floats.insert("_nip".into(), 0.0);
        }
        self.set_skin(inst, "cf_O_face", draw, image);
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

    b.skin_body(card, &body, female);

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
            let outfit = opts.outfit.min(card.coordinates.len().saturating_sub(1));
            let makeup = match card.coordinates.get(outfit) {
                Some(co) if co.enable_makeup => &co.makeup,
                _ => &card.face.base_makeup,
            };
            b.skin_face(card, makeup, &head, item);
            b.eyes(card, &head);
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

    // Hair hangs from the upper face bone and keeps its own bones.
    if let Some(&hair_parent) = head_bones.get("cf_J_FaceUp_ty") {
        for (i, hp) in card.hair.parts.iter().enumerate().take(4) {
            let label = format!("hair {}", ["back", "front", "side", "option"][i]);
            let prop = format!("ChaFileHair.{}", cat::HAIR_PROPS[i]);
            if let Some(item) = b.item(&label, &prop, cat::HAIR[i], hp.id) {
                if let Some(inst) = b.part(&label, item, Some(hair_parent), Part::Hair(i as u8)) {
                    b.color_hair(&inst, hp);
                }
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
            // What the top (and its sub-parts) and the bra say about the
            // pieces around them.
            let mut top_items: Vec<&Item> = Vec::new();
            let mut top_insts: Vec<Instance> = Vec::new();
            let mut bra: Option<(Instance, &Item)> = None;
            let (mut no_bottom, mut no_shorts) = (false, false);
            for (i, cp) in co.clothes.parts.iter().enumerate().take(9) {
                // Only one of indoor (7) and outdoor (8) shoes is worn.
                if (i == 7 && shoes_type != 0) || (i == 8 && shoes_type == 0) {
                    continue;
                }
                // One-piece tops replace the bottom, one-piece bras the shorts.
                if (i == 1 && no_bottom) || (i == 3 && no_shorts) {
                    continue;
                }
                let label = format!("outfit{outfit}.{}", rk_card::CLOTHES_KIND_NAMES[i]);
                let prop = format!("outfit{outfit}.ChaFileClothes.{}", cat::CLOTHES_PROPS[i]);
                let Some(item) = b.item(&label, &prop, cat::CLOTHES[i], cp.id) else {
                    continue;
                };
                let inst = b.part(&label, item, Some(top), Part::Clothes(i as u8));
                let pattern_prop = format!("outfit{outfit}.ChaFileClothes.{}Pattern", cat::CLOTHES_PROPS[i]);
                if let Some(inst) = &inst {
                    assign_weights(&mut b.scene, inst, &bones, "cf_j_root");
                    hide_undressed_states(&mut b.scene, inst);
                    b.dress(inst, item, &cp.color_info, &pattern_prop);
                }
                let one_piece = item.int("Coordinate") == Some(2);
                match (i, inst) {
                    (0, inst) => {
                        no_bottom = one_piece;
                        top_items.push(item);
                        top_insts.extend(inst);
                    }
                    (2, Some(inst)) => {
                        no_shorts = one_piece;
                        bra = Some((inst, item));
                    }
                    _ => {}
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
                            // Sub-parts share the top's colours. The third
                            // one takes its first colour from the fourth entry.
                            let mut colors = cp.color_info.clone();
                            if k == 2 && colors.len() > 3 {
                                colors[0] = colors[3].clone();
                            }
                            b.dress(&inst, sub, &colors, &pattern_prop);
                            top_insts.push(inst);
                        }
                        top_items.push(sub);
                    }
                }
            }
            b.hide_under_top(&body, &top_items, &top_insts, bra.as_ref());
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
                let hair_root = card.hair.parts.first().map(|p| p.start_color).unwrap_or([1.0; 4]);
                b.color_accessory(&inst, acc, hair_root);
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
    let material_edits = crate::material_editor::apply(&mut b.scene, &card.ext, outfit, &targets);

    // The game's default ramp. Studio scenes can pick another one.
    let ramp = game.items.vanilla(cat::MT_RAMP, 1).and_then(|item| {
        b.loader
            .image_by_name(item.get("MainTexAB")?, item.get("MainTex")?, None)
    });
    if ramp.is_none() {
        b.loader.warn("shading ramp not found, using a hard edge");
    }
    Ok(Character {
        scene: b.scene,
        top,
        bones,
        ramp,
        material_edits,
        warnings: b.loader.warnings,
    })
}
