// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Material overrides saved by the MaterialEditor plugin: floats, colours,
//! textures, shader swaps and renderer switches, addressed by the object
//! they belong to and a material or renderer name.

use crate::scene::{DrawState, Image, MaterialInst, Scene, TexSlot};
use rk_card::{map_get, ExtData, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub const KEY: &str = "com.deathweasel.bepinex.materialeditor";

/// What an override is attached to. Clothing and accessories also carry an
/// outfit index; hair and the character itself do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Owner {
    Clothing(i64),
    Accessory(i64),
    Hair(i64),
    Character,
}

/// Scene renderers per owner, filled in while the character is built.
pub type Targets = HashMap<Owner, Vec<usize>>;

fn owner(entry: &Value, outfit: i64) -> Option<Owner> {
    let slot = map_get(entry, "Slot").and_then(Value::as_i64).unwrap_or(0);
    let coordinate = map_get(entry, "CoordinateIndex").and_then(Value::as_i64).unwrap_or(0);
    match map_get(entry, "ObjectType").and_then(Value::as_i64)? {
        1 => (coordinate == outfit).then_some(Owner::Clothing(slot)),
        2 => (coordinate == outfit).then_some(Owner::Accessory(slot)),
        3 => Some(Owner::Hair(slot)),
        4 => Some(Owner::Character),
        _ => None,
    }
}

fn text<'a>(entry: &'a Value, key: &str) -> Option<&'a str> {
    map_get(entry, key).and_then(Value::as_str)
}

fn floats(v: &Value) -> Vec<f32> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect())
        .unwrap_or_default()
}

/// Decodes an embedded PNG or JPEG to a bottom-up RGBA image.
pub(crate) fn decode(bytes: &[u8], name: String) -> Option<Image> {
    let img = image::load_from_memory(bytes).ok()?.flipv().into_rgba8();
    Some(Image {
        name,
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
        srgb: true,
        repeat: true,
    })
}

/// A shader MaterialEditor can switch to: its draw state and the queue its
/// XML entry forces, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shader {
    pub draw: DrawState,
    pub queue: Option<i32>,
}

/// Face materials whose queue survives a shader switch.
const EYE_MATERIALS: [&str; 6] = [
    "cf_m_eyeline_00_up",
    "cf_m_eyeline_kage",
    "cf_m_eyeline_down",
    "cf_m_sirome_00",
    "cf_m_hitomi_00",
    "cf_m_mayuge_00",
];

/// Switches a material to another shader as MaterialEditor does. A shader it
/// cannot load leaves the material alone. Otherwise the material takes the
/// new shader's queue, unless the XML entry sets one or the material is one
/// of the eye materials, which keep theirs. A queue saved with the entry
/// wins over all of them.
fn switch_shader(m: &mut MaterialInst, name: Option<&str>, shader: Option<Shader>, queue: Option<i32>) {
    if let (Some(name), Some(sh)) = (name, shader) {
        m.shader = name.to_string();
        m.draw = Some(sh.draw);
        if let Some(q) = sh.queue {
            m.render_queue = q;
        } else if !EYE_MATERIALS.contains(&m.name.as_str()) {
            // Assigning a shader drops the material's own queue.
            m.render_queue = -1;
        }
    }
    if let Some(q) = queue {
        m.render_queue = q;
    }
}

/// Number of overrides applied, for reporting.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    pub floats: usize,
    pub colors: usize,
    pub textures: usize,
    pub shaders: usize,
    pub renderers: usize,
}

pub fn apply(
    scene: &mut Scene,
    ext: &ExtData,
    outfit: usize,
    targets: &Targets,
    shaders: &dyn Fn(&str) -> Option<Shader>,
) -> Applied {
    let mut applied = Applied::default();
    let Some(plugin) = ext.get(KEY) else { return applied };
    let list = |key: &str| -> Vec<Value> {
        match plugin.nested(key) {
            Some(Value::Array(a)) => a,
            _ => Vec::new(),
        }
    };
    let outfit = outfit as i64;
    let all: Vec<usize> = (0..scene.renderers.len()).collect();
    let renderers_of = |o: Owner| -> &[usize] {
        match o {
            Owner::Character => &all,
            o => targets.get(&o).map(Vec::as_slice).unwrap_or(&[]),
        }
    };

    // Embedded textures, decoded on first use.
    let blobs: HashMap<i64, &[u8]> = match plugin.data.get("TextureDictionary") {
        Some(Value::Binary(b)) => match rmpv::decode::read_value_ref(&mut &b[..]) {
            Ok(rmpv::ValueRef::Map(m)) => m
                .into_iter()
                .filter_map(|(k, v)| match (k, v) {
                    (rmpv::ValueRef::Integer(i), rmpv::ValueRef::Binary(bytes)) => Some((i.as_i64()?, bytes)),
                    _ => None,
                })
                .collect(),
            _ => HashMap::new(),
        },
        _ => HashMap::new(),
    };
    let mut decoded: HashMap<i64, Option<Arc<Image>>> = HashMap::new();

    // Runs `f` on every material with the given name under an owner.
    let on_materials = |scene: &mut Scene, entry: &Value, f: &mut dyn FnMut(&mut crate::scene::MaterialInst)| -> bool {
        let (Some(o), Some(name)) = (owner(entry, outfit), text(entry, "MaterialName")) else {
            return false;
        };
        let mut hit = false;
        for &ri in renderers_of(o) {
            for m in scene.renderers[ri].materials.iter_mut().filter(|m| m.name == name) {
                f(m);
                hit = true;
            }
        }
        hit
    };

    for e in list("MaterialShaderList") {
        let name = text(&e, "ShaderName");
        let shader = name.and_then(shaders);
        let queue = map_get(&e, "RenderQueue").and_then(Value::as_i64).map(|q| q as i32);
        applied.shaders += on_materials(scene, &e, &mut |m| switch_shader(m, name, shader, queue)) as usize;
    }
    for e in list("MaterialFloatPropertyList") {
        // Values are stored as text.
        let (Some(prop), Some(value)) = (
            text(&e, "Property"),
            text(&e, "Value").and_then(|v| v.trim().parse::<f32>().ok()),
        ) else {
            continue;
        };
        let key = format!("_{prop}");
        applied.floats += on_materials(scene, &e, &mut |m| {
            m.floats.insert(key.clone(), value);
        }) as usize;
    }
    for e in list("MaterialColorPropertyList") {
        let (Some(prop), Some(v)) = (text(&e, "Property"), map_get(&e, "Value").map(floats)) else {
            continue;
        };
        if v.len() < 4 {
            continue;
        }
        let key = format!("_{prop}");
        applied.colors += on_materials(scene, &e, &mut |m| {
            m.colors.insert(key.clone(), [v[0], v[1], v[2], v[3]]);
        }) as usize;
    }
    for e in list("MaterialTexturePropertyList") {
        let Some(prop) = text(&e, "Property") else { continue };
        let key = format!("_{prop}");
        let image = map_get(&e, "TexID").and_then(Value::as_i64).and_then(|id| {
            decoded
                .entry(id)
                .or_insert_with(|| {
                    blobs
                        .get(&id)
                        .and_then(|b| decode(b, format!("material_editor_{id}")))
                        .map(Arc::new)
                })
                .clone()
        });
        let pair = |k: &str| {
            map_get(&e, k)
                .map(floats)
                .filter(|v| v.len() >= 2)
                .map(|v| [v[0], v[1]])
        };
        let (scale, offset) = (pair("Scale"), pair("Offset"));
        applied.textures += on_materials(scene, &e, &mut |m| {
            if let Some(image) = &image {
                let old = m.textures.get(&key);
                let slot = TexSlot {
                    image: image.clone(),
                    scale: old.map(|s| s.scale).unwrap_or([1.0, 1.0]),
                    offset: old.map(|s| s.offset).unwrap_or([0.0, 0.0]),
                };
                m.textures.insert(key.clone(), slot);
            }
            if let Some(slot) = m.textures.get_mut(&key) {
                slot.scale = scale.unwrap_or(slot.scale);
                slot.offset = offset.unwrap_or(slot.offset);
            }
        }) as usize;
    }
    for e in list("RendererPropertyList") {
        // Property 0 is "enabled"; the others concern shadows and normals.
        if map_get(&e, "Property").and_then(Value::as_i64) != Some(0) {
            continue;
        }
        let (Some(o), Some(name), Some(value)) = (owner(&e, outfit), text(&e, "RendererName"), text(&e, "Value"))
        else {
            continue;
        };
        for &ri in renderers_of(o) {
            if scene.renderers[ri].name == name {
                scene.renderers[ri].enabled = value.trim() == "1";
                applied.renderers += 1;
            }
        }
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Stencil;

    fn state(queue: i32) -> DrawState {
        DrawState {
            queue,
            blend: true,
            zwrite: true,
            outline: true,
            stencil: Stencil::None,
            stencil_ref: 0,
        }
    }

    fn material(name: &str, queue: i32) -> MaterialInst {
        MaterialInst {
            name: name.into(),
            shader: "Shader Forge/main_item".into(),
            draw: Some(state(2450)),
            render_queue: queue,
            ..Default::default()
        }
    }

    #[test]
    fn a_switch_takes_the_new_shaders_queue() {
        let mut m = material("hair", 3040);
        let sh = Shader {
            draw: state(2475),
            queue: None,
        };
        switch_shader(&mut m, Some("xukmi/HairFrontPlus"), Some(sh), None);
        assert_eq!(m.shader, "xukmi/HairFrontPlus");
        assert_eq!(m.draw.map(|d| d.queue), Some(2475));
        assert_eq!(m.render_queue, -1);
    }

    #[test]
    fn eye_materials_keep_their_queue() {
        let mut m = material("cf_m_hitomi_00", 2474);
        let sh = Shader {
            draw: state(3000),
            queue: None,
        };
        switch_shader(&mut m, Some("xukmi/EyePlus"), Some(sh), None);
        assert_eq!(m.render_queue, 2474);
    }

    #[test]
    fn xml_and_saved_queues_win() {
        let sh = Shader {
            draw: state(2000),
            queue: Some(2600),
        };
        let mut m = material("a", 3040);
        switch_shader(&mut m, Some("s"), Some(sh), None);
        assert_eq!(m.render_queue, 2600);
        switch_shader(&mut m, Some("s"), Some(sh), Some(3000));
        assert_eq!(m.render_queue, 3000);
    }

    #[test]
    fn an_unknown_shader_changes_nothing() {
        let mut m = material("a", 3040);
        switch_shader(&mut m, Some("missing"), None, None);
        assert_eq!(m.shader, "Shader Forge/main_item");
        assert_eq!(m.render_queue, 3040);
        // A saved queue still applies, as MaterialEditor sets it separately.
        switch_shader(&mut m, Some("missing"), None, Some(2500));
        assert_eq!(m.render_queue, 2500);
    }
}
