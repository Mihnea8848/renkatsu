// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! A map's mirrors: the renderers that carry the game's
//! `MirrorReflection` component and a material of its shader,
//! `FX/MirrorReflection_Transparent`. Map files hold none yet, so they
//! are read from the map's bundle.

use crate::build::{to_linear, Tree};
use crate::scene::{deref, open};
use crate::unity::MirrorReflection;
use anyhow::Result;
use glam::Mat4;
use rk_render::mirror::MapMirror;
use rk_unityfs::class;
use rk_unityfs::objects::{GameObject, Material, Renderer, Transform};
use rk_vfs::assets::{AssetFile, LoadedBundle};
use rk_vfs::Game;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// The queue of the shader's pass: Transparent.
const SHADER_QUEUE: i32 = 3000;

/// Whether a shader of this name draws a mirror's surface.
pub fn is_mirror_shader(name: &str) -> bool {
    name.to_ascii_lowercase().contains("mirrorreflection")
}

/// A mirror from what its object holds: `world` its transform, `layer`
/// its object's layer, `color` the material's `_Color` as stored and
/// `queue` the material's own queue, below 0 for the shader's.
pub fn mirror(
    name: &str,
    world: Mat4,
    layer: i32,
    component: &MirrorReflection,
    color: [f32; 4],
    queue: i32,
) -> MapMirror {
    MapMirror {
        name: name.to_owned(),
        world,
        texture_size: component.texture_size.clamp(1, 8192) as u32,
        clip_offset: component.clip_offset,
        layers: component.layers,
        layer: layer.clamp(0, 31) as u32,
        // A colour, which Unity hands to shaders linearised.
        color: [to_linear(color[0]), to_linear(color[1]), to_linear(color[2]), color[3]],
        queue: if queue < 0 { SHADER_QUEUE } else { queue },
    }
}

/// The mirrors of one file of a map's bundle that show: the component
/// and its renderer enabled, the object shown.
pub(crate) fn of_file(
    game: &Game,
    loaded: &LoadedBundle,
    file: &Arc<AssetFile>,
    tree: &Tree,
    hidden: &HashSet<i64>,
    now: &HashSet<i64>,
) -> Vec<MapMirror> {
    let sf = &file.sf;
    let mut out = Vec::new();
    for obj in sf.objects_of(class::MONO_BEHAVIOUR) {
        let Ok(o) = sf.read(obj) else { continue };
        let script = o
            .pptr("m_Script")
            .and_then(|p| deref(game, loaded, file, p))
            .and_then(|r| r.read().ok())
            .and_then(|s| s.str("m_ClassName").map(str::to_owned))
            .unwrap_or_default();
        if script != "MirrorReflection" {
            continue;
        }
        let component = MirrorReflection::read(&o);
        let go = component.game_object.path_id;
        let Some(object) = tree.objects.get(&go) else { continue };
        if !component.enabled || !tree.shown(go, hidden, now) {
            continue;
        }
        // `OnWillRenderObject` asks for an enabled renderer with a
        // material; the surface is the material of the mirror's shader.
        let renderer = object
            .components
            .iter()
            .filter(|c| c.file_id == 0)
            .filter_map(|c| sf.object(c.path_id))
            .filter(|info| info.class_id == class::MESH_RENDERER)
            .find_map(|info| sf.read(info).and_then(|o| Renderer::read(&o)).ok());
        let Some(renderer) = renderer.filter(|r| r.enabled) else {
            continue;
        };
        let material = renderer.materials.iter().find_map(|p| {
            let r = deref(game, loaded, file, *p)?;
            let m = Material::read(&r.read().ok()?).ok()?;
            let shader = game.assets.shader_name(&r.file, m.shader)?;
            is_mirror_shader(&shader).then_some(m)
        });
        let Some(material) = material else { continue };
        out.push(mirror(
            &object.name,
            tree.world_of(go),
            object.layer,
            &component,
            material.color("_Color").unwrap_or([1.0; 4]),
            material.render_queue,
        ));
    }
    out
}

/// The mirrors of a map, read from its bundle: those that show whatever
/// the time of day.
pub fn mirrors(game: &Game, bundle: &str) -> Result<Vec<MapMirror>> {
    let loaded = open(game, bundle)?;
    let mut out = Vec::new();
    for file in &loaded.files {
        let sf = &file.sf;
        let mut transforms: HashMap<i64, Transform> = HashMap::new();
        let mut transform_of: HashMap<i64, i64> = HashMap::new();
        for obj in sf.objects_of(class::TRANSFORM) {
            if let Ok(t) = sf.read(obj).and_then(|o| Transform::read(&o)) {
                transform_of.insert(t.game_object.path_id, obj.path_id);
                transforms.insert(obj.path_id, t);
            }
        }
        let mut objects: HashMap<i64, GameObject> = HashMap::new();
        for obj in sf.objects_of(class::GAME_OBJECT) {
            if let Ok(go) = sf.read(obj).and_then(|o| GameObject::read(&o)) {
                objects.insert(obj.path_id, go);
            }
        }
        let tree = Tree {
            transforms: &transforms,
            transform_of: &transform_of,
            objects: &objects,
        };
        out.extend(of_file(game, &loaded, file, &tree, &HashSet::new(), &HashSet::new()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Quat, Vec3};

    #[test]
    fn a_mirror_is_made_from_what_its_object_holds() {
        // The love hotel's, as its bundle stores it.
        let component = MirrorReflection {
            game_object: Default::default(),
            enabled: true,
            disable_pixel_lights: true,
            texture_size: 1024,
            clip_offset: 0.0,
            layers: 3072,
        };
        let world = Mat4::from_scale_rotation_translation(
            Vec3::new(0.390_166_9, 0.667_286_5, 0.503_699_24),
            Quat::from_xyzw(
                0.0,
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            ),
            Vec3::new(6.1965, 1.949, 0.314),
        );
        let m = mirror(
            "p_koi_map100_mirror00",
            world,
            11,
            &component,
            [0.5, 1.0, 1.0, 0.25],
            -1,
        );
        assert_eq!((m.texture_size, m.layers, m.layer, m.queue), (1024, 3072, 11, 3000));
        assert_eq!(m.camera_mask(), (1 << 10) | (1 << 11));
        // The colour is linear, its alpha as stored.
        assert!((m.color[0] - to_linear(0.5)).abs() < 1e-6);
        assert_eq!(m.color[3], 0.25);
        // The mirror faces along its own Y, here the world's -X.
        assert!((m.up() - Vec3::NEG_X).length() < 1e-5);
        // A material's own queue is kept.
        assert_eq!(mirror("m", world, 40, &component, [1.0; 4], 2450).queue, 2450);
        assert_eq!(mirror("m", world, 40, &component, [1.0; 4], 2450).layer, 31);
    }

    #[test]
    fn only_the_mirror_shader_draws_a_mirror() {
        assert!(is_mirror_shader("FX/MirrorReflection_Transparent"));
        assert!(!is_mirror_shader("Standard"));
    }
}
