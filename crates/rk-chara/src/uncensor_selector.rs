// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Uncensor Selector plugin (`KK_UncensorSelector`): the body mesh and
//! its base textures come from a body a zipmod offers instead of from the
//! game.
//!
//! Which body (`UncensorSelectorController.BodyData`): the one the card
//! names, when it is installed. In Studio that holds whatever its sex.
//! Otherwise the plugin's default, and with the setting left at "Random"
//! one of the installed bodies of the character's sex that allow it,
//! picked with `System.Random` seeded from the card
//! (`birthDay + personality + (int)(voicePitch * 100)`). Without any such
//! body the game's own body stays.
//!
//! What it does with it (`ReloadCharacterBody`, `UpdateSkin`): the body
//! prefab of that mod is loaded beside the character's and its meshes are
//! put on the character's renderers of the same name, the bones matched by
//! name. The skin texture is then rebuilt from that mod's main texture and
//! colour mask, on its body material.
//!
//! The Mod Bone Implantor plugin takes part (`UncensorSelectorSupport`):
//! a body prefab can carry `BoneImplantProcess` scripts, each naming a
//! bone of its own (`trfSrc`) and a bone of the skeleton (`trfDst`). The
//! first is moved under the second, and the meshes then use those bones
//! directly; any other bone is looked up by name among the renderer's
//! current bones and then in the whole body skeleton.
//!
//! Left out, and reported as a warning when a body asks for them: the
//! `uncensorOverlay` and `uncensorUnderlay` textures, colour matched parts
//! (`SetSkinColor`) and the chest normals (`SetChestNormals`). The penis
//! and balls are swapped by the plugin as well; the game hides both on a
//! character at rest, so they are not loaded here.

use crate::scene::{Instance, RendererInst, Scene};
use rk_card::{ExtData, Value};
use rk_vfs::uncensor::{defaults, Body, Uncensors};
use std::collections::{HashMap, HashSet};

pub const KEY: &str = "com.deathweasel.bepinex.uncensorselector";

/// `UncensorSelector.BodyNames`: the skin itself; keeps its material.
pub const BODY_NAMES: [&str; 3] = ["o_body_a", "o_body_cf", "o_body_cm"];
/// `UncensorSelector.BodyParts`: swapped together with their materials.
pub const BODY_PARTS: [&str; 8] = [
    "o_dankon",
    "o_dan_f",
    "o_gomu",
    "o_mnpa",
    "o_mnpb",
    "o_shadowcaster",
    "cm_o_dan00",
    "cm_o_dan_f",
];
/// `PenisParts` and `BallsParts`: these receive shadows after a swap.
const PENIS_AND_BALLS: [&str; 5] = ["o_dankon", "o_gomu", "cm_o_dan00", "o_dan_f", "cm_o_dan_f"];

/// `System.Random` of the game's Mono runtime (its `mscorlib.dll`): the
/// subtractive generator with the second index starting at 31.
pub struct MonoRandom {
    inext: usize,
    inextp: usize,
    seed_array: [i32; 56],
}

impl MonoRandom {
    pub fn new(seed: i32) -> Self {
        let mut a = [0i32; 56];
        // `Math.Abs` throws for the lowest value; no card seed gets there.
        let mut mj = 161803398i32.wrapping_sub(seed.wrapping_abs());
        a[55] = mj;
        let mut mk = 1i32;
        for i in 1..55 {
            let ii = 21 * i % 55;
            a[ii] = mk;
            mk = mj.wrapping_sub(mk);
            if mk < 0 {
                mk = mk.wrapping_add(i32::MAX);
            }
            mj = a[ii];
        }
        for _ in 1..5 {
            for k in 1..56 {
                a[k] = a[k].wrapping_sub(a[1 + (k + 30) % 55]);
                if a[k] < 0 {
                    a[k] = a[k].wrapping_add(i32::MAX);
                }
            }
        }
        Self {
            inext: 0,
            inextp: 31,
            seed_array: a,
        }
    }

    fn sample(&mut self) -> f64 {
        self.inext += 1;
        if self.inext >= 56 {
            self.inext = 1;
        }
        self.inextp += 1;
        if self.inextp >= 56 {
            self.inextp = 1;
        }
        let mut n = self.seed_array[self.inext].wrapping_sub(self.seed_array[self.inextp]);
        if n < 0 {
            n = n.wrapping_add(i32::MAX);
        }
        self.seed_array[self.inext] = n;
        n as f64 * 4.656612875245797e-10
    }

    /// `Next(maxValue)`.
    pub fn next(&mut self, max: i32) -> i32 {
        (self.sample() * max as f64) as i32
    }
}

/// `ChaFileParameter.voicePitch` for a card's `voiceRate`.
///
/// The game has `Lerp(0.94, 1.06, voiceRate)`, which clamps the rate. The
/// Slider Unlocker plugin replaces the getter with
/// `0.94f + voiceRate * 0.119999945f` (`VoicePitchUnlocker.VoicePitchHook`),
/// which does not; `slider_unlocker` says which one runs.
///
/// The game's Mono runtime works out `float` arithmetic in double
/// precision and rounds when the value is stored as a `float`, so the sum
/// is rounded once.
pub fn voice_pitch(voice_rate: f32, slider_unlocker: bool) -> f32 {
    let rate = if slider_unlocker {
        voice_rate
    } else {
        voice_rate.clamp(0.0, 1.0)
    };
    let range = if slider_unlocker {
        0.119999945f32 as f64
    } else {
        1.06f32 as f64 - 0.94f32 as f64
    };
    (0.94f32 as f64 + rate as f64 * range) as f32
}

/// `DefaultData.GetRandomNumber`'s seed:
/// `birthDay + personality + (int)(voicePitch * 100f)`. The product is
/// not rounded to `float` before it is truncated (see `voice_pitch`): a
/// pitch of 0.94 gives 93.
pub fn seed(birth_day: i64, personality: i64, voice_pitch: f32) -> i32 {
    (birth_day as i32)
        .wrapping_add(personality as i32)
        .wrapping_add((voice_pitch as f64 * 100.0) as i32)
}

/// The body GUID a card asks for (`OnReload`): `BodyGUID` of data version
/// 2, or the body an old `UncensorGUID` migrated to. Blank counts as none.
pub fn stored_body(ext: &ExtData, uncensors: &Uncensors) -> Option<String> {
    let p = ext.get(KEY)?;
    let string = |key: &str| p.data.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty());
    if p.version == 1 {
        let old = string("UncensorGUID")?;
        return uncensors.migrations.get(old).map(|m| m.body_guid.clone());
    }
    string("BodyGUID").map(str::to_string)
}

/// `BodyData` in Studio with the default body setting at "Random".
/// `sex` is the character's (0 male, 1 female).
pub fn select<'a>(uncensors: &'a Uncensors, guid: Option<&str>, sex: u8, seed: i32) -> Option<&'a Body> {
    if let Some(body) = guid.and_then(|g| uncensors.body(g)) {
        return Some(body);
    }
    let random: Vec<&Body> = uncensors
        .bodies
        .iter()
        .filter(|b| b.sex == sex && b.allow_random)
        .collect();
    if !random.is_empty() {
        let at = MonoRandom::new(seed).next(random.len() as i32);
        return random.get(at as usize).copied();
    }
    uncensors.body(if sex == 0 {
        defaults::BODY_MALE_GUID
    } else {
        defaults::BODY_FEMALE_GUID
    })
}

/// Whether a body is the game's own: nothing to swap then.
pub fn is_game_body(body: &Body) -> bool {
    body.oo_base == defaults::OO_BASE
}

/// The parts of a body this module does not apply.
pub fn unsupported(body: &Body) -> Vec<&'static str> {
    let mut out = Vec::new();
    let named = |t: &Option<String>| t.as_ref().is_some_and(|t| !t.trim().is_empty());
    if named(&body.overlay) {
        out.push("uncensorOverlay");
    }
    if named(&body.underlay) {
        out.push("uncensorUnderlay");
    }
    if !body.color_match.is_empty() {
        out.push("colorMatch");
    }
    if body.normals != defaults::NORMALS {
        out.push("normals");
    }
    out
}

/// Class name of the script Mod Bone Implantor looks for on a prefab.
pub const IMPLANT_SCRIPT: &str = "BoneImplantProcess";

/// Bones of a body prefab that Mod Bone Implantor moved into the
/// skeleton, and the skeleton they went into.
#[derive(Debug, Clone, Default)]
pub struct Implants {
    /// Every node below an implanted bone, the bone included.
    pub bones: HashSet<usize>,
    /// `aaWeightsBody.dictBone`: the body's bones by name.
    pub skeleton: HashMap<String, usize>,
}

/// `ModBoneImplantor.TryImplantBones`: each `(trfSrc, trfDst)` pair of the
/// prefab moves the first under the skeleton bone named like the second,
/// keeping its local transform. `None` when the prefab has no such script,
/// in which case the plugin leaves the bone transfer alone.
pub fn implant(
    scene: &mut Scene,
    pairs: &[(Option<usize>, Option<usize>)],
    skeleton: &HashMap<String, usize>,
) -> Option<Implants> {
    if pairs.is_empty() {
        return None;
    }
    let mut bones = HashSet::new();
    for &(src, dst) in pairs {
        let (Some(src), Some(dst)) = (src, dst) else { continue };
        if src == dst {
            continue;
        }
        let Some(&target) = skeleton.get(&scene.nodes[dst].name) else {
            continue;
        };
        scene.set_parent(src, Some(target));
        bones.extend(scene.subtree(src));
    }
    Some(Implants {
        bones,
        skeleton: skeleton.clone(),
    })
}

/// `UpdateMeshRenderer`: the mesh of `src` on `dst`, each of its bones
/// taken from `dst`'s current bones by name (`TransferBones`), or the way
/// Mod Bone Implantor does it when it implanted bones for this prefab.
fn update(scene: &mut Scene, src: usize, dst: usize, copy_materials: bool, implants: Option<&Implants>) {
    let implants = implants.filter(|i| !i.bones.is_empty());
    let bones: Vec<Option<usize>> = scene.renderers[src]
        .bones
        .iter()
        .map(|t| {
            let t = (*t)?;
            if implants.is_some_and(|i| i.bones.contains(&t)) {
                return Some(t);
            }
            let name = &scene.nodes[t].name;
            scene.renderers[dst]
                .bones
                .iter()
                .flatten()
                .copied()
                .find(|&c| scene.nodes[c].name == *name)
                .or_else(|| implants.and_then(|i| i.skeleton.get(name).copied()))
        })
        .collect();
    let mesh = scene.renderers[src].mesh.clone();
    let materials = copy_materials.then(|| scene.renderers[src].materials.clone());
    let d = &mut scene.renderers[dst];
    d.shape_weights.clear();
    if PENIS_AND_BALLS.contains(&mesh.name.as_str()) {
        d.receive_shadows = true;
    }
    d.mesh = mesh;
    d.bones = bones;
    if let Some(materials) = materials {
        d.materials = materials;
    }
}

/// `ReloadCharacterBody`: puts the meshes of `donor` (the selected body's
/// prefab, instantiated) on the renderers of `body`.
pub fn swap_meshes(
    scene: &mut Scene,
    body: &mut Instance,
    donor: &Instance,
    data: &Body,
    uncensors: &Uncensors,
    implants: Option<&Implants>,
) {
    let skinned = |scene: &Scene, inst: &Instance| -> Vec<usize> {
        inst.renderers
            .iter()
            .copied()
            .filter(|&r| scene.renderers[r].skinned)
            .collect()
    };
    let donor_named = |scene: &Scene, name: &str| -> Option<usize> {
        skinned(scene, donor)
            .into_iter()
            .find(|&r| scene.renderers[r].name == name)
    };

    // Parts only this body has are copied over first.
    let skin = skinned(scene, body)
        .into_iter()
        .find(|&r| BODY_NAMES.contains(&scene.renderers[r].name.as_str()));
    if let (Some(skin), false) = (skin, data.additional_parts.is_empty()) {
        for src in skinned(scene, donor) {
            let name = scene.renderers[src].name.clone();
            if !data.additional_parts.contains(&name) {
                continue;
            }
            if skinned(scene, body).iter().any(|&r| scene.renderers[r].name == name) {
                continue;
            }
            // The new object goes under the object of the skin's subtree
            // that is named like the part's parent in the prefab.
            let Some(parent_name) = scene.nodes[scene.renderers[src].node]
                .parent
                .map(|p| scene.nodes[p].name.clone())
            else {
                continue;
            };
            let Some(parent) = scene
                .subtree(scene.renderers[skin].node)
                .into_iter()
                .find(|&n| scene.nodes[n].name == parent_name)
            else {
                continue;
            };
            let local = scene.nodes[scene.renderers[src].node].local;
            let node = scene.add_node(&name, Some(parent), local);
            let s = &scene.renderers[src];
            let k = &scene.renderers[skin];
            let copy = RendererInst {
                name,
                node,
                part: k.part,
                mesh: s.mesh.clone(),
                materials: s.materials.clone(),
                skinned: true,
                // The skin's bones; the swap below orders them for the mesh.
                bones: k.bones.clone(),
                enabled: s.enabled,
                cast_shadows: s.cast_shadows,
                receive_shadows: s.receive_shadows,
                shape_weights: Vec::new(),
                // The copy keeps the prefab's box and loses its root bone
                // with the prefab; the skin's are used instead.
                bounds: k.bounds,
                root_bone: k.root_bone,
            };
            scene.renderers.push(copy);
            body.renderers.push(scene.renderers.len() - 1);
            body.nodes.push(node);
        }
    }

    for dst in skinned(scene, body) {
        let name = scene.renderers[dst].name.clone();
        let src = donor_named(scene, &name);
        // The skin keeps its material; body parts and colour matched
        // parts take the prefab's.
        let is_skin = BODY_NAMES.contains(&name.as_str());
        let is_part = BODY_PARTS.contains(&name.as_str()) || data.color_match.iter().any(|p| p.object == name);
        if let (Some(src), true) = (src, is_skin || is_part) {
            update(scene, src, dst, !is_skin, implants);
        }
        if !uncensors.additional_parts.contains(&name) {
            continue;
        }
        if !data.additional_parts.contains(&name) {
            // A part of some other body: the renderer is destroyed.
            scene.renderers[dst].enabled = false;
            continue;
        }
        if let Some(src) = src {
            update(scene, src, dst, true, implants);
        }
    }

    // The prefab is destroyed.
    for &r in &donor.renderers {
        scene.renderers[r].enabled = false;
    }
    scene.remove(donor.root);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::test_renderer;
    use rk_card::PluginData;
    use rk_unityfs::objects::Mesh;
    use rk_vfs::uncensor::{ManifestData, Migration};
    use std::sync::Arc;

    #[test]
    fn random_matches_the_runtime() {
        // Values worked out separately from the algorithm in the game's
        // mscorlib (not taken from a running game).
        let mut r = MonoRandom::new(142);
        let got: Vec<i32> = (0..5).map(|_| r.next(1000)).collect();
        assert_eq!(got, [307, 338, 485, 521, 674]);
        let mut r = MonoRandom::new(0);
        let got: Vec<i32> = (0..3).map(|_| r.next(i32::MAX)).collect();
        assert_eq!(got, [1976681210, 551155468, 2145952487]);
        assert_eq!(MonoRandom::new(95).next(22), 7);
        // Same seed, same sequence; the sign of the seed is ignored.
        let a: Vec<i32> = {
            let mut r = MonoRandom::new(137);
            (0..4).map(|_| r.next(1000)).collect()
        };
        let b: Vec<i32> = {
            let mut r = MonoRandom::new(-137);
            (0..4).map(|_| r.next(1000)).collect()
        };
        assert_eq!(a, b);
        assert!(a.iter().all(|v| (0..1000).contains(v)));
        assert_ne!(a[0], a[1]);
    }

    #[test]
    fn seed_adds_the_card_fields() {
        // The middle rate lands on a pitch of exactly 1.
        assert_eq!(voice_pitch(0.5, true), 1.0);
        assert_eq!(seed(27, 15, voice_pitch(0.5, true)), 27 + 15 + 100);
        // 0.94 times 100 falls just short of 94.
        assert_eq!(seed(1, 0, voice_pitch(0.0, true)), 1 + 93);
        // Slider Unlocker lets the rate leave 0..1; the game clamps it.
        assert_eq!(seed(0, 0, voice_pitch(-0.15, true)), 92);
        assert_eq!(seed(0, 0, voice_pitch(-0.15, false)), 93);
        assert_eq!(seed(0, 0, voice_pitch(2.0, true)), 117);
        assert_eq!(seed(0, 0, voice_pitch(2.0, false)), 105);
    }

    fn uncensors() -> Uncensors {
        let body = |guid: &str, sex: u8, allow_random: bool| Body {
            guid: guid.into(),
            oo_base: format!("chara/{guid}.unity3d"),
            sex,
            allow_random,
            ..Default::default()
        };
        Uncensors::build(vec![(
            "pack".into(),
            Some(0),
            ManifestData {
                bodies: vec![
                    body("f1", 1, true),
                    body("m1", 0, true),
                    body("f2", 1, true),
                    body("f3", 1, false),
                ],
                migrations: vec![Migration {
                    uncensor_guid: "old".into(),
                    body_guid: "f3".into(),
                }],
            },
        )])
    }

    fn ext(version: i64, key: &str, value: Value) -> ExtData {
        let mut p = PluginData {
            version,
            ..Default::default()
        };
        p.data.insert(key.into(), value);
        let mut ext = ExtData::new();
        ext.insert(KEY.into(), p);
        ext
    }

    #[test]
    fn card_names_its_body() {
        let u = uncensors();
        assert_eq!(stored_body(&ExtData::new(), &u), None);
        assert_eq!(stored_body(&ext(2, "BodyGUID", Value::Nil), &u), None);
        assert_eq!(stored_body(&ext(2, "BodyGUID", " ".into()), &u), None);
        assert_eq!(stored_body(&ext(2, "BodyGUID", "f2".into()), &u), Some("f2".into()));
        assert_eq!(
            stored_body(&ext(1, "UncensorGUID", "old".into()), &u),
            Some("f3".into())
        );
        assert_eq!(stored_body(&ext(1, "UncensorGUID", "gone".into()), &u), None);
    }

    #[test]
    fn selection_falls_back_to_a_seeded_pick() {
        let u = uncensors();
        // A named body is used whatever its sex or random flag.
        assert_eq!(select(&u, Some("m1"), 1, 0).unwrap().guid, "m1");
        assert_eq!(select(&u, Some("f3"), 1, 0).unwrap().guid, "f3");
        // Unknown or missing: one of the two random female bodies, and
        // always the same one for a seed.
        let pick = select(&u, Some("missing"), 1, 142).unwrap().guid.clone();
        assert!(pick == "f1" || pick == "f2");
        assert_eq!(select(&u, None, 1, 142).unwrap().guid, pick);
        assert_eq!(select(&u, None, 0, 142).unwrap().guid, "m1");
        // Nothing to pick from: the game's body.
        let empty = Uncensors::build(Vec::new());
        let body = select(&empty, None, 1, 142).unwrap();
        assert_eq!(body.guid, defaults::BODY_FEMALE_GUID);
        assert!(is_game_body(body));
    }

    fn mesh(name: &str) -> Arc<Mesh> {
        Arc::new(Mesh {
            name: name.into(),
            ..Default::default()
        })
    }

    #[test]
    fn meshes_move_over_and_bones_follow_by_name() {
        swap(false);
        swap(true);
    }

    fn swap(with_implantor: bool) {
        let mut s = Scene::default();
        let top = s.add_node("BodyTop", None, Default::default());
        let hips = s.add_node("cf_j_hips", Some(top), Default::default());
        let waist = s.add_node("cf_j_waist", Some(top), Default::default());
        let add = |s: &mut Scene, name: &str, parent: usize, mesh_name: &str, bones: Vec<Option<usize>>| {
            let node = s.add_node(name, Some(parent), Default::default());
            let mut r = test_renderer(name, node);
            r.skinned = true;
            r.mesh = mesh(mesh_name);
            r.bones = bones;
            r.materials = vec![crate::scene::MaterialInst {
                name: mesh_name.into(),
                ..Default::default()
            }];
            s.renderers.push(r);
            s.renderers.len() - 1
        };
        // The character's body, bound to the skeleton.
        let body_root = s.add_node("p_cf_body_00", Some(top), Default::default());
        let skin = add(&mut s, "o_body_a", body_root, "game", vec![Some(hips), Some(waist)]);
        let caster = add(&mut s, "o_shadowcaster", body_root, "game", vec![Some(hips)]);
        let other = add(&mut s, "o_other_extra", body_root, "game", vec![Some(hips)]);
        let tongue = add(&mut s, "o_tang", body_root, "game", vec![Some(hips)]);
        let mut body = Instance {
            root: body_root,
            nodes: vec![body_root],
            renderers: vec![skin, caster, other, tongue],
            renderer_keys: Default::default(),
            object_nodes: Default::default(),
            transform_nodes: Default::default(),
            scripts: Vec::new(),
        };
        // The donor prefab with its own skeleton, bones in another order.
        let donor_root = s.add_node("p_cf_body_00", Some(top), Default::default());
        let d_waist = s.add_node("cf_j_waist", Some(donor_root), Default::default());
        let d_hips = s.add_node("cf_j_hips", Some(donor_root), Default::default());
        let d_new = s.add_node("cf_j_new", Some(donor_root), Default::default());
        let d_skin = add(
            &mut s,
            "o_body_a",
            donor_root,
            "mod",
            vec![Some(d_waist), Some(d_hips), Some(d_new)],
        );
        let d_caster = add(&mut s, "o_shadowcaster", donor_root, "mod", vec![Some(d_hips)]);
        let d_holder = s.add_node("o_body_a", Some(donor_root), Default::default());
        let d_extra = add(&mut s, "o_extra", d_holder, "extra", vec![Some(d_hips)]);
        let d_tongue = add(&mut s, "o_tang", donor_root, "mod", vec![Some(d_hips)]);
        let donor = Instance {
            root: donor_root,
            nodes: vec![donor_root],
            renderers: vec![d_skin, d_caster, d_extra, d_tongue],
            renderer_keys: Default::default(),
            object_nodes: Default::default(),
            transform_nodes: Default::default(),
            scripts: Vec::new(),
        };
        let data = Body {
            guid: "mod".into(),
            additional_parts: vec!["o_extra".into()],
            ..Default::default()
        };
        let mut u = Uncensors::build(Vec::new());
        u.additional_parts
            .extend(["o_extra".to_string(), "o_other_extra".to_string()]);

        // Mod Bone Implantor: the prefab's new bone goes under the hips.
        let skeleton = s.name_map(top);
        assert!(implant(&mut s, &[], &skeleton).is_none());
        let skeleton: HashMap<String, usize> =
            [("cf_j_hips".to_string(), hips), ("cf_j_waist".to_string(), waist)].into();
        let implants = if with_implantor {
            implant(&mut s, &[(Some(d_new), Some(d_hips)), (None, Some(d_hips))], &skeleton)
        } else {
            None
        };

        swap_meshes(&mut s, &mut body, &donor, &data, &u, implants.as_ref());

        // Skin: new mesh, own material, bones by name. The bone the
        // skeleton lacks is missing, or the implanted one.
        assert_eq!(s.renderers[skin].mesh.name, "mod");
        assert_eq!(s.renderers[skin].materials[0].name, "game");
        let new_bone = with_implantor.then_some(d_new);
        assert_eq!(s.renderers[skin].bones, vec![Some(waist), Some(hips), new_bone]);
        if with_implantor {
            assert_eq!(s.nodes[d_new].parent, Some(hips));
        }
        // A body part takes the donor's material too.
        assert_eq!(s.renderers[caster].mesh.name, "mod");
        assert_eq!(s.renderers[caster].materials[0].name, "mod");
        // Anything else is left alone.
        assert_eq!(s.renderers[tongue].mesh.name, "game");
        // Another body's extra part goes, this body's is added.
        assert!(!s.renderers[other].enabled);
        let extra = *body.renderers.last().unwrap();
        assert_eq!(s.renderers[extra].name, "o_extra");
        assert_eq!(s.renderers[extra].mesh.name, "extra");
        assert_eq!(s.renderers[extra].bones, vec![Some(hips)]);
        assert_eq!(s.nodes[s.renderers[extra].node].parent, Some(s.renderers[skin].node));
        // The donor is gone.
        assert!(!s.nodes[donor_root].active);
        assert!(!s.renderers[d_skin].enabled);
    }
}
