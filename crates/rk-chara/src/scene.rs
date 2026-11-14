// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Scene graph a character is assembled into: one node tree shared by the
//! skeleton and every part, plus the renderers that draw meshes on it.
//!
//! Math follows the stored data: left-handed, Y up, metres.

use glam::{Mat4, Quat, Vec3};
use rk_unityfs::objects::Mesh;
use rk_vfs::ObjRef;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub struct Trs {
    pub pos: Vec3,
    pub rot: Quat,
    pub scale: Vec3,
}

impl Default for Trs {
    fn default() -> Self {
        Self {
            pos: Vec3::ZERO,
            rot: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl Trs {
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rot, self.pos)
    }
}

/// Quaternion from Unity euler angles in degrees (applied Z, then X, then Y).
pub fn euler_deg(x: f32, y: f32, z: f32) -> Quat {
    Quat::from_euler(glam::EulerRot::YXZ, y.to_radians(), x.to_radians(), z.to_radians())
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub local: Trs,
    pub active: bool,
}

/// Which part of the character a renderer belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    Body,
    Head,
    /// Back, front, side, option.
    Hair(u8),
    /// Index into the nine clothing slots; sub-parts share their top's slot.
    Clothes(u8),
    Accessory(u16),
}

/// Decoded RGBA8 image, rows bottom-up as Unity stores them.
#[derive(Debug)]
pub struct Image {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// False for data textures (masks, normal maps) that must not be gamma-decoded.
    pub srgb: bool,
    pub repeat: bool,
}

#[derive(Debug, Clone)]
pub struct TexSlot {
    pub image: Arc<Image>,
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}

#[derive(Debug, Clone, Default)]
pub struct MaterialInst {
    pub name: String,
    /// Unity shader name, e.g. `Shader Forge/main_skin`.
    pub shader: String,
    pub keywords: String,
    pub render_queue: i32,
    pub textures: HashMap<String, TexSlot>,
    pub floats: HashMap<String, f32>,
    pub colors: HashMap<String, [f32; 4]>,
}

pub struct RendererInst {
    pub name: String,
    pub node: usize,
    pub part: Part,
    pub mesh: Arc<Mesh>,
    pub materials: Vec<MaterialInst>,
    pub skinned: bool,
    /// Scene node per mesh bone; `None` when the bone could not be found.
    pub bones: Vec<Option<usize>>,
    pub enabled: bool,
    pub shadows_only: bool,
    /// Weight per blend shape of the mesh, 0 to 1. May be shorter than the
    /// shape list; missing entries count as 0.
    pub shape_weights: Vec<f32>,
}

/// Nodes and renderers created by instantiating one prefab.
pub struct Instance {
    pub root: usize,
    pub nodes: Vec<usize>,
    pub renderers: Vec<usize>,
    /// Scene renderer for each renderer component of the prefab, so script
    /// fields that point at renderers can be followed.
    pub renderer_keys: HashMap<(usize, i64), usize>,
    /// Scene node for each game object of the prefab.
    pub object_nodes: HashMap<(usize, i64), usize>,
    /// Scripts attached to the prefab's objects, by node.
    pub scripts: Vec<(usize, ObjRef)>,
}

#[derive(Default)]
pub struct Scene {
    pub nodes: Vec<Node>,
    pub renderers: Vec<RendererInst>,
}

impl Scene {
    pub fn add_node(&mut self, name: &str, parent: Option<usize>, local: Trs) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node {
            name: name.to_string(),
            parent,
            children: Vec::new(),
            local,
            active: true,
        });
        if let Some(p) = parent {
            self.nodes[p].children.push(id);
        }
        id
    }

    pub fn set_parent(&mut self, node: usize, parent: Option<usize>) {
        if let Some(old) = self.nodes[node].parent {
            self.nodes[old].children.retain(|&c| c != node);
        }
        self.nodes[node].parent = parent;
        if let Some(p) = parent {
            self.nodes[p].children.push(node);
        }
    }

    /// Detaches a subtree and switches it off, as destroying it would.
    pub fn remove(&mut self, node: usize) {
        self.set_parent(node, None);
        self.nodes[node].active = false;
    }

    /// Depth-first search for a node by name below (and including) `from`.
    pub fn find(&self, from: usize, name: &str) -> Option<usize> {
        if self.nodes[from].name == name {
            return Some(from);
        }
        self.nodes[from].children.iter().find_map(|&c| self.find(c, name))
    }

    pub fn subtree(&self, from: usize) -> Vec<usize> {
        let mut out = vec![from];
        let mut i = 0;
        while i < out.len() {
            out.extend(self.nodes[out[i]].children.iter().copied());
            i += 1;
        }
        out
    }

    /// Name to node for a subtree; the first node with a name wins.
    pub fn name_map(&self, from: usize) -> HashMap<String, usize> {
        let mut map = HashMap::new();
        for n in self.subtree(from) {
            map.entry(self.nodes[n].name.clone()).or_insert(n);
        }
        map
    }

    /// World matrix of every node, and whether it is active in the hierarchy.
    pub fn world(&self) -> (Vec<Mat4>, Vec<bool>) {
        let mut mats = vec![Mat4::IDENTITY; self.nodes.len()];
        let mut active = vec![false; self.nodes.len()];
        let mut stack: Vec<usize> = (0..self.nodes.len())
            .filter(|&i| self.nodes[i].parent.is_none())
            .collect();
        while let Some(i) = stack.pop() {
            let n = &self.nodes[i];
            let (pm, pa) = n.parent.map(|p| (mats[p], active[p])).unwrap_or((Mat4::IDENTITY, true));
            mats[i] = pm * n.local.matrix();
            active[i] = pa && n.active;
            stack.extend(n.children.iter().copied());
        }
        (mats, active)
    }
}

/// A mesh posed in world space, ready to draw.
pub struct DrawMesh<'a> {
    pub renderer: &'a RendererInst,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Direction along the surface, with Unity's handedness sign in `w`.
    pub tangents: Vec<[f32; 4]>,
}

impl Scene {
    /// Applies skinning (or the node transform) to every visible renderer.
    pub fn pose(&self) -> Vec<DrawMesh<'_>> {
        let (world, active) = self.world();
        let mut out = Vec::new();
        for r in &self.renderers {
            if !r.enabled || r.shadows_only || !active[r.node] || r.mesh.positions.is_empty() {
                continue;
            }
            let m = &r.mesh;
            let has_normals = m.normals.len() == m.positions.len();
            // Blend shapes move vertices before skinning.
            let mut base_positions = std::borrow::Cow::Borrowed(&m.positions[..]);
            let mut base_normals = std::borrow::Cow::Borrowed(&m.normals[..]);
            for (shape, &w) in m.blend_shapes.iter().zip(&r.shape_weights) {
                if w == 0.0 {
                    continue;
                }
                let w = w * 100.0 / shape.full_weight.max(f32::EPSILON);
                let (pos, nor) = (base_positions.to_mut(), base_normals.to_mut());
                for v in &shape.vertices {
                    let i = v.index as usize;
                    if let Some(p) = pos.get_mut(i) {
                        *p = (Vec3::from(*p) + Vec3::from(v.position) * w).to_array();
                    }
                    if let Some(n) = nor.get_mut(i) {
                        *n = (Vec3::from(*n) + Vec3::from(v.normal) * w).to_array();
                    }
                }
            }
            let mut positions = Vec::with_capacity(m.positions.len());
            let mut normals = Vec::with_capacity(m.positions.len());
            let has_tangents = m.tangents.len() == m.positions.len();
            let mut tangents = Vec::with_capacity(if has_tangents { m.positions.len() } else { 0 });
            if r.skinned && !m.skin.is_empty() && !m.bind_poses.is_empty() {
                let mats: Vec<Option<Mat4>> = (0..m.bind_poses.len())
                    .map(|i| {
                        let node = r.bones.get(i).copied().flatten()?;
                        Some(world[node] * Mat4::from_cols_array(&m.bind_poses[i]).transpose())
                    })
                    .collect();
                for (vi, p) in base_positions.iter().enumerate() {
                    let bw = &m.skin[vi];
                    let mut acc = Mat4::ZERO;
                    let mut total = 0.0;
                    for k in 0..4 {
                        let w = bw.weights[k];
                        if w <= 0.0 {
                            continue;
                        }
                        if let Some(Some(mat)) = mats.get(bw.bones[k] as usize) {
                            acc += *mat * w;
                            total += w;
                        }
                    }
                    let mat = if total > 0.0 {
                        acc * (1.0 / total)
                    } else {
                        world[r.node]
                    };
                    positions.push(mat.transform_point3(Vec3::from(*p)).to_array());
                    if has_tangents {
                        let t = m.tangents[vi];
                        let dir = mat.transform_vector3(Vec3::new(t[0], t[1], t[2])).normalize_or_zero();
                        tangents.push([dir.x, dir.y, dir.z, t[3]]);
                    }
                    if has_normals {
                        normals.push(
                            mat.transform_vector3(Vec3::from(base_normals[vi]))
                                .normalize_or_zero()
                                .to_array(),
                        );
                    }
                }
            } else {
                let mat = world[r.node];
                let nmat = mat.inverse().transpose();
                for (vi, p) in base_positions.iter().enumerate() {
                    positions.push(mat.transform_point3(Vec3::from(*p)).to_array());
                    if has_tangents {
                        let t = m.tangents[vi];
                        let dir = mat.transform_vector3(Vec3::new(t[0], t[1], t[2])).normalize_or_zero();
                        tangents.push([dir.x, dir.y, dir.z, t[3]]);
                    }
                    if has_normals {
                        normals.push(
                            nmat.transform_vector3(Vec3::from(base_normals[vi]))
                                .normalize_or_zero()
                                .to_array(),
                        );
                    }
                }
            }
            if !has_normals {
                normals = vec![[0.0, 0.0, 1.0]; positions.len()];
            }
            if !has_tangents {
                tangents = vec![[1.0, 0.0, 0.0, 1.0]; positions.len()];
            }
            out.push(DrawMesh {
                renderer: r,
                positions,
                normals,
                tangents,
            });
        }
        out
    }
}
