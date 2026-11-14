// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Turns Unity prefabs into scene nodes and renderers, with caches for the
//! meshes and textures they share.

use crate::scene::{DrawState, Image, Instance, MaterialInst, Part, RendererInst, Scene, ShadowCasting, TexSlot, Trs};
use anyhow::{anyhow, Context, Result};
use glam::{Quat, Vec3};
use rk_unityfs::objects::{GameObject, Material, Mesh, MeshFilter, Renderer, Transform};
use rk_unityfs::{class, PPtr};
use rk_vfs::{Game, ObjRef};
use std::collections::HashMap;
use std::sync::Arc;

pub struct Loader<'a> {
    pub game: &'a Game,
    meshes: HashMap<(usize, i64), Arc<Mesh>>,
    images: HashMap<(usize, i64), Option<Arc<Image>>>,
    /// Loose PNG textures from mods, by their key.
    pngs: HashMap<String, Option<Arc<Image>>>,
    /// Problems that did not stop loading (missing assets, bad textures).
    pub warnings: Vec<String>,
}

impl<'a> Loader<'a> {
    pub fn new(game: &'a Game) -> Self {
        Self {
            game,
            meshes: HashMap::new(),
            images: HashMap::new(),
            pngs: HashMap::new(),
            warnings: Vec::new(),
        }
    }

    pub fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if !self.warnings.contains(&msg) {
            self.warnings.push(msg);
        }
    }

    pub fn mesh(&mut self, obj: &ObjRef) -> Result<Arc<Mesh>> {
        if let Some(m) = self.meshes.get(&obj.key()) {
            return Ok(m.clone());
        }
        let m = Arc::new(Mesh::read(&obj.read()?)?);
        self.meshes.insert(obj.key(), m.clone());
        Ok(m)
    }

    /// Decoded texture, or `None` (with a warning) when it cannot be read.
    pub fn image(&mut self, obj: &ObjRef) -> Option<Arc<Image>> {
        if let Some(i) = self.images.get(&obj.key()) {
            return i.clone();
        }
        let img = match self.game.assets.texture(obj) {
            Ok((tex, rgba)) => Some(Arc::new(Image {
                name: tex.name,
                width: tex.width,
                height: tex.height,
                rgba,
                srgb: tex.color_space == 1,
                repeat: tex.wrap_mode == 0,
            })),
            Err(e) => {
                self.warn(format!("texture {}: {e:#}", obj.name()));
                None
            }
        };
        self.images.insert(obj.key(), img.clone());
        img
    }

    /// Loads a texture asset by bundle path and name.
    pub fn image_by_name(&mut self, vpath: &str, name: &str, prefer_mod: Option<u32>) -> Option<Arc<Image>> {
        // A loose PNG in a mod wins over any bundle, as in Sideloader.
        let key = rk_vfs::png_key(vpath, name);
        if let Some(image) = self.pngs.get(&key) {
            return image.clone();
        }
        if let Some(bytes) = self.game.assets.png(vpath, name) {
            let image = crate::material_editor::decode(&bytes, name.to_string()).map(|mut image| {
                // Sideloader takes the wrap mode from the file's path.
                if key.contains("clamp") {
                    image.repeat = false;
                }
                Arc::new(image)
            });
            self.pngs.insert(key, image.clone());
            if image.is_some() {
                return image;
            }
        }
        let obj = self.game.assets.find(vpath, name, Some(class::TEXTURE2D), prefer_mod)?;
        self.image(&obj)
    }

    pub fn material(&mut self, obj: &ObjRef) -> Result<MaterialInst> {
        let m = Material::read(&obj.read()?)?;
        let (shader, passes) = self.game.assets.shader_info(&obj.file, m.shader).unwrap_or_default();
        let mut inst = MaterialInst {
            name: m.name.clone(),
            shader,
            draw: DrawState::from_passes(&passes),
            keywords: m.keywords.clone(),
            render_queue: m.render_queue,
            ..Default::default()
        };
        for (k, t) in &m.textures {
            inst.tex_st
                .insert(k.clone(), [t.scale[0], t.scale[1], t.offset[0], t.offset[1]]);
            if let Some(tex) = self.game.assets.deref(&obj.file, t.texture) {
                if tex.class_id() != class::TEXTURE2D {
                    continue;
                }
                if let Some(image) = self.image(&tex) {
                    inst.textures.insert(
                        k.clone(),
                        TexSlot {
                            image,
                            scale: t.scale,
                            offset: t.offset,
                        },
                    );
                }
            }
        }
        inst.floats = m.floats.into_iter().collect();
        inst.colors = m.colors.into_iter().collect();
        Ok(inst)
    }

    /// Loads a material asset by bundle path and name.
    pub fn material_by_name(&mut self, vpath: &str, name: &str, prefer_mod: Option<u32>) -> Option<MaterialInst> {
        let obj = self.game.assets.find(vpath, name, Some(class::MATERIAL), prefer_mod)?;
        match self.material(&obj) {
            Ok(m) => Some(m),
            Err(e) => {
                self.warn(format!("material {name}: {e:#}"));
                None
            }
        }
    }

    /// Finds a prefab by bundle path and name and instantiates it.
    pub fn load_prefab(
        &mut self,
        scene: &mut Scene,
        vpath: &str,
        name: &str,
        prefer_mod: Option<u32>,
        parent: Option<usize>,
        part: Part,
    ) -> Result<Instance> {
        let root = self
            .game
            .assets
            .find(vpath, name, Some(class::GAME_OBJECT), prefer_mod)
            .ok_or_else(|| anyhow!("prefab {name} not found in {vpath}"))?;
        self.instantiate(scene, &root, parent, part)
            .with_context(|| format!("loading {vpath} :: {name}"))
    }

    /// Creates scene nodes and renderers for a prefab rooted at a GameObject.
    pub fn instantiate(
        &mut self,
        scene: &mut Scene,
        root: &ObjRef,
        parent: Option<usize>,
        part: Part,
    ) -> Result<Instance> {
        let go = GameObject::read(&root.read()?)?;
        let assets = &self.game.assets;
        let root_tr = go
            .components
            .iter()
            .filter_map(|p| assets.deref(&root.file, *p))
            .find(|c| matches!(c.class_id(), class::TRANSFORM | class::RECT_TRANSFORM))
            .ok_or_else(|| anyhow!("prefab root {} has no transform", go.name))?;

        struct Pending {
            node: usize,
            renderer: ObjRef,
            filter: Option<ObjRef>,
        }
        let mut by_transform: HashMap<(usize, i64), usize> = HashMap::new();
        let mut pending = Vec::new();
        let mut inst = Instance {
            root: 0,
            nodes: Vec::new(),
            renderers: Vec::new(),
            renderer_keys: HashMap::new(),
            object_nodes: HashMap::new(),
            transform_nodes: HashMap::new(),
            scripts: Vec::new(),
        };

        // Iterative walk so deep skeletons cannot overflow the stack.
        let mut stack = vec![(root_tr, parent)];
        let mut first = true;
        while let Some((tr_ref, parent)) = stack.pop() {
            let tr = Transform::read(&tr_ref.read()?)?;
            let Some(go_ref) = assets.deref(&tr_ref.file, tr.game_object) else {
                continue;
            };
            let go = GameObject::read(&go_ref.read()?)?;
            let local = Trs {
                pos: Vec3::from(tr.position),
                rot: Quat::from_array(tr.rotation),
                scale: Vec3::from(tr.scale),
            };
            let node = scene.add_node(&go.name, parent, local);
            scene.nodes[node].active = go.active;
            if first {
                inst.root = node;
                first = false;
            }
            inst.nodes.push(node);
            by_transform.insert(tr_ref.key(), node);
            inst.transform_nodes.insert(tr_ref.key(), node);
            inst.object_nodes.insert(go_ref.key(), node);

            let (mut renderer, mut filter) = (None, None);
            for c in go.components.iter().filter_map(|p| assets.deref(&go_ref.file, *p)) {
                match c.class_id() {
                    class::SKINNED_MESH_RENDERER | class::MESH_RENDERER => renderer = Some(c),
                    class::MESH_FILTER => filter = Some(c),
                    class::MONO_BEHAVIOUR => inst.scripts.push((node, c)),
                    _ => {}
                }
            }
            if let Some(renderer) = renderer {
                pending.push(Pending { node, renderer, filter });
            }
            // Reverse so children keep their order when popped.
            for child in tr.children.iter().rev() {
                if let Some(c) = assets.deref(&tr_ref.file, *child) {
                    stack.push((c, Some(node)));
                }
            }
        }

        for p in pending {
            let r = Renderer::read(&p.renderer.read()?)?;
            let skinned = p.renderer.class_id() == class::SKINNED_MESH_RENDERER;
            let mesh_ptr: PPtr = if skinned {
                r.mesh
            } else {
                match &p.filter {
                    Some(f) => MeshFilter::read(&f.read()?)?.mesh,
                    None => continue,
                }
            };
            let mesh_file = if skinned {
                &p.renderer.file
            } else {
                &p.filter.as_ref().unwrap().file
            };
            let Some(mesh_ref) = self.game.assets.deref(mesh_file, mesh_ptr) else {
                continue;
            };
            let name = scene.nodes[p.node].name.clone();
            let mesh = match self.mesh(&mesh_ref) {
                Ok(m) => m,
                Err(e) => {
                    self.warn(format!("mesh of {name}: {e:#}"));
                    continue;
                }
            };
            let mut materials = Vec::new();
            for m in &r.materials {
                let mat = match self.game.assets.deref(&p.renderer.file, *m) {
                    Some(obj) => self.material(&obj).unwrap_or_else(|e| {
                        self.warn(format!("material on {name}: {e:#}"));
                        MaterialInst::default()
                    }),
                    None => MaterialInst::default(),
                };
                materials.push(mat);
            }
            let bones = r
                .bones
                .iter()
                .map(|b| {
                    self.game
                        .assets
                        .deref(&p.renderer.file, *b)
                        .and_then(|t| by_transform.get(&t.key()).copied())
                })
                .collect();
            inst.renderer_keys.insert(p.renderer.key(), scene.renderers.len());
            inst.renderers.push(scene.renderers.len());
            scene.renderers.push(RendererInst {
                name,
                node: p.node,
                part,
                mesh,
                materials,
                skinned,
                bones,
                enabled: r.enabled,
                cast_shadows: ShadowCasting::from_unity(r.cast_shadows),
                receive_shadows: r.receive_shadows,
                shape_weights: Vec::new(),
            });
        }
        Ok(inst)
    }
}

/// Rebinds an instance's skinned meshes to bones of the same name in `dict`
/// (the main skeleton), then removes the instance's own copy of the skeleton.
///
/// Bones the skeleton lacks are grafted onto it under the matching parent,
/// so clothes and accessories with extra bones keep working.
pub fn assign_weights(scene: &mut Scene, inst: &Instance, dict: &HashMap<String, usize>, del_top: &str) {
    for &ri in &inst.renderers {
        let bones = scene.renderers[ri].bones.clone();
        let mut out = Vec::with_capacity(bones.len());
        for b in bones {
            let Some(b) = b else {
                out.push(None);
                continue;
            };
            if let Some(&target) = dict.get(&scene.nodes[b].name) {
                out.push(Some(target));
                continue;
            }
            // Graft: move the topmost ancestor the skeleton lacks under the
            // skeleton node named like that ancestor's parent.
            let mut top = b;
            let mut anchor = None;
            while let Some(p) = scene.nodes[top].parent {
                if let Some(&a) = dict.get(&scene.nodes[p].name) {
                    anchor = Some(a);
                    break;
                }
                top = p;
            }
            if let Some(a) = anchor {
                if scene.nodes[top].parent != Some(a) {
                    scene.set_parent(top, Some(a));
                }
            }
            out.push(Some(b));
        }
        scene.renderers[ri].bones = out;
    }
    if let Some(top) = scene.find(inst.root, del_top) {
        scene.remove(top);
    }
}
