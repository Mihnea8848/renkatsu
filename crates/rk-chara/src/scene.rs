// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Scene graph a character is assembled into: one node tree shared by the
//! skeleton and every part, plus the renderers that draw meshes on it.
//!
//! Math follows the stored data: left-handed, Y up, metres.

use glam::{Mat4, Quat, Vec3, Vec4};
use rk_unityfs::objects::{Mesh, ShaderPass, ShaderProp, ShaderPropKind};
use rk_vfs::ObjRef;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
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

impl Image {
    /// Unity's built-in "black" texture, which is transparent black. A
    /// material shows it when the game sets no texture and the shader names
    /// "black" as the property's default.
    pub fn unity_black() -> Image {
        Image {
            name: "default_black".into(),
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 0],
            srgb: true,
            repeat: true,
        }
    }
}

/// What a face line's `_MainTex` shows when the maker's choice has no
/// texture and ChaControl.ChangeTexture sets null. toon_nose declares
/// "black" as the default, so the nose line vanishes. toon_eyew declares
/// "white", which is what a material without the texture already gets.
pub fn unset_line_texture(shader: &str) -> Option<Image> {
    let name = shader.rsplit('/').next().unwrap_or(shader).to_lowercase();
    name.starts_with("toon_nose").then(Image::unity_black)
}

#[derive(Debug, Clone)]
pub struct TexSlot {
    pub image: Arc<Image>,
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}

/// How a shader uses the stencil buffer. The game's eye and brow shaders
/// mark their pixels, and front hair leaves marked pixels alone, which is
/// what keeps eyes visible through a fringe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Stencil {
    #[default]
    None,
    /// Writes the reference value where it draws.
    Write,
    /// Does not draw where the buffer holds the reference value.
    SkipEqual,
}

/// The shadow caster pass of a shader. The game also draws the camera's
/// depth texture with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShadowPass {
    /// The shader has no such pass: it neither casts nor shows in the
    /// depth texture.
    #[default]
    None,
    /// Both faces cast.
    TwoSided,
    /// Front faces cast.
    CullBack,
    /// The pass comes from the shader's fallback (Legacy Shaders/VertexLit):
    /// front faces cast, nothing is cut out, and the vertices move inwards
    /// by the light's normal bias.
    Fallback,
}

/// Unity's `ShadowCastingMode` of a renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShadowCasting {
    Off,
    #[default]
    On,
    /// Casts with both faces whatever the shader's pass says.
    TwoSided,
    /// Casts, and is not drawn itself.
    ShadowsOnly,
}

impl ShadowCasting {
    /// From the number Unity stores in `m_CastShadows`.
    pub fn from_unity(mode: i32) -> Self {
        match mode {
            0 => Self::Off,
            2 => Self::TwoSided,
            3 => Self::ShadowsOnly,
            _ => Self::On,
        }
    }
}

/// How a pass mixes its colour with what is behind, from the pass's blend
/// factors. The forward passes of the game's character shaders use these
/// three pairs and no others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Blend {
    /// One, Zero: the pixel replaces what is behind.
    #[default]
    Off,
    /// SrcAlpha, OneMinusSrcAlpha.
    Alpha,
    /// One, OneMinusSrcAlpha: the shader scales its colour itself
    /// (toon_glasses). main_skin, main_item, main_opaque and main_emblem
    /// have these factors too.
    Premultiplied,
}

impl Blend {
    /// From Unity's `BlendMode` numbers of the source and destination factor.
    pub fn from_unity(src: i32, dst: i32) -> Self {
        match (src, dst) {
            (5, 10) => Self::Alpha,
            (1, 10) => Self::Premultiplied,
            _ => Self::Off,
        }
    }
}

/// When and how a material is drawn, taken from its shader's forward pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawState {
    /// Unity render queue: lower draws first.
    pub queue: i32,
    pub blend: Blend,
    pub zwrite: bool,
    /// The shader has an outline pass.
    pub outline: bool,
    pub stencil: Stencil,
    pub stencil_ref: u8,
    pub shadow: ShadowPass,
}

impl DrawState {
    /// State of the pass that draws the lit surface, or `None` when the
    /// shader has no such pass.
    pub fn from_passes(passes: &[ShaderPass]) -> Option<Self> {
        let helper = |p: &ShaderPass| {
            let name = p.name.to_ascii_uppercase();
            name == "OUTLINE" || matches!(p.light_mode.as_str(), "SHADOWCASTER" | "META" | "FORWARDADD")
        };
        let main = passes
            .iter()
            .find(|p| p.light_mode == "FORWARDBASE")
            .or_else(|| passes.iter().find(|p| !helper(p)))?;
        Some(Self {
            queue: main.queue.unwrap_or(2000),
            blend: Blend::from_unity(main.src_blend, main.dst_blend),
            zwrite: main.zwrite,
            outline: passes.iter().any(|p| p.name.eq_ignore_ascii_case("OUTLINE")),
            stencil: match (main.stencil_comp, main.stencil_pass) {
                (6, _) => Stencil::SkipEqual,
                (8, 2) => Stencil::Write,
                _ => Stencil::None,
            },
            stencil_ref: main.stencil_ref.clamp(0, 255) as u8,
            shadow: match passes.iter().find(|p| p.light_mode == "SHADOWCASTER") {
                None => ShadowPass::None,
                Some(p) if p.from_fallback => ShadowPass::Fallback,
                Some(p) if p.cull == 2 => ShadowPass::CullBack,
                Some(_) => ShadowPass::TwoSided,
            },
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct MaterialInst {
    pub name: String,
    /// Unity shader name, e.g. `Shader Forge/main_skin`.
    pub shader: String,
    /// Draw state read from the shader. `None` when the shader could not be
    /// read, as with shaders that plugins carry inside their own files.
    pub draw: Option<DrawState>,
    pub keywords: String,
    pub render_queue: i32,
    pub textures: HashMap<String, TexSlot>,
    /// Scale and offset of every texture property, also the ones with no
    /// texture assigned: the game fills some in later and keeps the placement.
    pub tex_st: HashMap<String, [f32; 4]>,
    pub floats: HashMap<String, f32>,
    pub colors: HashMap<String, [f32; 4]>,
    /// The renderer's slot points at no material (a null reference in the
    /// prefab). The slot is kept so material indices stay the game's, but
    /// nothing of ours is drawn for it.
    pub absent: bool,
    /// Names of the properties the material stores: the ones saved in its
    /// asset and the ones its shaders declare. `None` when the shader could
    /// not be read; every property then counts as stored.
    pub stored: Option<HashSet<String>>,
    /// Colours and floats set on properties the material does not store.
    /// Unity keeps those in the running property sheet only, which is built
    /// again from the stored values when the shader changes.
    pub unsaved: HashSet<String>,
}

impl MaterialInst {
    /// Whether the material stores the property.
    pub fn stores(&self, name: &str) -> bool {
        self.stored.as_ref().is_none_or(|s| s.contains(name))
    }

    /// `Material.SetColor`. A colour on a property the material does not
    /// store reads back, but does not survive a shader switch.
    pub fn set_color(&mut self, name: &str, value: [f32; 4]) {
        if !self.stores(name) {
            self.unsaved.insert(name.to_string());
        }
        self.colors.insert(name.to_string(), value);
    }

    /// `Material.SetTexture`. `GetTexture` reads the stored list, so a
    /// texture on a property the material does not store reads back as none,
    /// and no shader of the material samples it.
    pub fn set_texture(&mut self, name: &str, slot: TexSlot) {
        if self.stores(name) {
            self.textures.insert(name.to_string(), slot);
        }
    }

    /// What assigning another shader does to the properties: values that
    /// were never stored are gone, the stored ones stay, and every property
    /// the new shader declares that has no value takes the shader's default.
    pub fn adopt_shader(&mut self, props: &[ShaderProp]) {
        for name in std::mem::take(&mut self.unsaved) {
            self.colors.remove(&name);
            self.floats.remove(&name);
        }
        for p in props {
            match p.kind {
                ShaderPropKind::Vector => {
                    self.colors.entry(p.name.clone()).or_insert(p.default);
                }
                ShaderPropKind::Float => {
                    self.floats.entry(p.name.clone()).or_insert(p.default[0]);
                }
                ShaderPropKind::Texture => {
                    self.tex_st.entry(p.name.clone()).or_insert([1.0, 1.0, 0.0, 0.0]);
                }
            }
            if let Some(stored) = &mut self.stored {
                stored.insert(p.name.clone());
            }
        }
    }
}

/// Culling box of a renderer as Unity keeps it: centre and half size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub center: [f32; 3],
    pub extent: [f32; 3],
}

impl Bounds {
    /// The box the game gives every mesh it rebinds to the skeleton
    /// (`ChaControl.bounds`).
    pub const CHARA: Bounds = Bounds {
        center: [0.0, -0.2, 0.0],
        extent: [1.0, 1.0, 1.0],
    };

    /// Lowest and highest corner of the box after `mat`, as Unity's
    /// `TransformAABB` gives them.
    pub fn transformed(&self, mat: Mat4) -> [[f32; 3]; 2] {
        let c = mat.transform_point3(Vec3::from(self.center));
        let e = Vec3::from(self.extent);
        let half =
            mat.x_axis.truncate().abs() * e.x + mat.y_axis.truncate().abs() * e.y + mat.z_axis.truncate().abs() * e.z;
        [(c - half).to_array(), (c + half).to_array()]
    }
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
    pub cast_shadows: ShadowCasting,
    pub receive_shadows: bool,
    /// Weight per blend shape of the mesh, 0 to 1. May be shorter than the
    /// shape list; missing entries count as 0.
    pub shape_weights: Vec<f32>,
    /// Culling box: a skinned renderer's own (`m_AABB` or what the game
    /// set as `localBounds`), else the mesh's. In the space of `root_bone`,
    /// or of the renderer's node without one.
    pub bounds: Option<Bounds>,
    /// `SkinnedMeshRenderer.rootBone`.
    pub root_bone: Option<usize>,
}

/// A renderer with an empty mesh, for tests.
#[cfg(test)]
pub(crate) fn test_renderer(name: &str, node: usize) -> RendererInst {
    RendererInst {
        name: name.into(),
        node,
        part: Part::Body,
        mesh: Arc::new(Mesh::default()),
        materials: Vec::new(),
        skinned: false,
        bones: Vec::new(),
        enabled: true,
        cast_shadows: ShadowCasting::On,
        receive_shadows: true,
        shape_weights: Vec::new(),
        bounds: None,
        root_bone: None,
    }
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
    /// Scene node for each transform of the prefab.
    pub transform_nodes: HashMap<(usize, i64), usize>,
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
    /// Names from below `from` down to `to`, joined by `/`, as Unity's
    /// path from a transform to one of its children reads. Empty when `to`
    /// is not under `from`.
    pub fn path_below(&self, from: usize, to: usize) -> String {
        let mut names = Vec::new();
        let mut n = to;
        while n != from {
            names.push(self.nodes[n].name.as_str());
            match self.nodes[n].parent {
                Some(p) => n = p,
                None => return String::new(),
            }
        }
        names.reverse();
        names.join("/")
    }

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

/// A mesh posed in world space, ready to draw: its vertices after blend
/// shapes and the matrices that carry them into the world. The renderer
/// applies the matrices on the GPU; `posed` does the same on the CPU.
pub struct DrawMesh<'a> {
    pub renderer: &'a RendererInst,
    /// Vertex positions after blend shapes, before skinning. Borrowed from
    /// the mesh when no blend shape is on.
    pub base_positions: Cow<'a, [[f32; 3]]>,
    /// Vertex normals after blend shapes, before skinning.
    pub base_normals: Cow<'a, [[f32; 3]]>,
    pub skinning: Skinning,
    /// Lowest and highest corner of the renderer's culling box in the world
    /// (`Renderer.bounds`); `None` when the files give no box.
    pub bounds: Option<[[f32; 3]; 2]>,
}

/// How a posed mesh's vertices reach the world.
#[derive(Debug, Clone)]
pub enum Skinning {
    /// Each vertex blends up to four bone matrices (the bone's world matrix
    /// times its bind pose) by its weights; a bone that is missing is
    /// `None`. `node` is the renderer's own world matrix.
    Bones { matrices: Vec<Option<Mat4>>, node: Mat4 },
    /// One matrix for every vertex, and its inverse transpose for normals.
    Rigid { matrix: Mat4, normal: Mat4 },
}

/// A mesh's vertices in world space.
pub struct Posed {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Direction along the surface, with Unity's handedness sign in `w`.
    pub tangents: Vec<[f32; 4]>,
}

impl DrawMesh<'_> {
    fn has_normals(&self) -> bool {
        self.renderer.mesh.normals.len() == self.renderer.mesh.positions.len()
    }

    fn has_tangents(&self) -> bool {
        self.renderer.mesh.tangents.len() == self.renderer.mesh.positions.len()
    }

    /// The matrix that carries vertex `vi` into the world.
    fn vertex_matrix(&self, vi: usize) -> Mat4 {
        match &self.skinning {
            Skinning::Rigid { matrix, .. } => *matrix,
            Skinning::Bones { matrices, node } => {
                let bw = &self.renderer.mesh.skin[vi];
                let mut acc = Mat4::ZERO;
                let mut total = 0.0;
                for k in 0..4 {
                    let w = bw.weights[k];
                    if w <= 0.0 {
                        continue;
                    }
                    if let Some(Some(mat)) = matrices.get(bw.bones[k] as usize) {
                        acc += *mat * w;
                        total += w;
                    }
                }
                if total > 0.0 {
                    acc * (1.0 / total)
                } else if bw.weights.iter().all(|w| *w <= 0.0) {
                    // Unity sums weight times bone matrix, so a vertex
                    // without weights gets the zero matrix and lands on
                    // the origin of the skinned space. Triangles made of
                    // such vertices have no area and are not drawn.
                    Mat4::from_cols(Vec4::ZERO, Vec4::ZERO, Vec4::ZERO, node.w_axis)
                } else {
                    *node
                }
            }
        }
    }

    /// The vertex positions in world space.
    pub fn positions(&self) -> Vec<[f32; 3]> {
        (0..self.base_positions.len())
            .map(|vi| {
                self.vertex_matrix(vi)
                    .transform_point3(Vec3::from(self.base_positions[vi]))
                    .to_array()
            })
            .collect()
    }

    /// The vertices in world space, as the renderer's skinning pass makes
    /// them.
    pub fn posed(&self) -> Posed {
        let n = self.base_positions.len();
        let (has_normals, has_tangents) = (self.has_normals(), self.has_tangents());
        let mut out = Posed {
            positions: Vec::with_capacity(n),
            normals: Vec::with_capacity(n),
            tangents: Vec::with_capacity(n),
        };
        for vi in 0..n {
            let mat = self.vertex_matrix(vi);
            out.positions
                .push(mat.transform_point3(Vec3::from(self.base_positions[vi])).to_array());
            out.tangents.push(if has_tangents {
                let t = self.renderer.mesh.tangents[vi];
                let dir = mat.transform_vector3(Vec3::new(t[0], t[1], t[2])).normalize_or_zero();
                [dir.x, dir.y, dir.z, t[3]]
            } else {
                [1.0, 0.0, 0.0, 1.0]
            });
            out.normals.push(if has_normals {
                let nmat = match &self.skinning {
                    Skinning::Rigid { normal, .. } => *normal,
                    Skinning::Bones { .. } => mat,
                };
                nmat.transform_vector3(Vec3::from(self.base_normals[vi]))
                    .normalize_or_zero()
                    .to_array()
            } else {
                [0.0, 0.0, 1.0]
            });
        }
        out
    }
}

impl Scene {
    /// Poses every visible renderer, and the ones that only cast shadows:
    /// applies the blend shapes and works out the skinning matrices.
    pub fn pose(&self) -> Vec<DrawMesh<'_>> {
        let (world, active) = self.world();
        let mut out = Vec::new();
        for r in &self.renderers {
            if !r.enabled || !active[r.node] || r.mesh.positions.is_empty() {
                continue;
            }
            let m = &r.mesh;
            // Blend shapes move vertices before skinning.
            let mut base_positions = Cow::Borrowed(&m.positions[..]);
            let mut base_normals = Cow::Borrowed(&m.normals[..]);
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
            let skinning = if r.skinned && !m.skin.is_empty() && !m.bind_poses.is_empty() {
                Skinning::Bones {
                    matrices: (0..m.bind_poses.len())
                        .map(|i| {
                            let node = r.bones.get(i).copied().flatten()?;
                            Some(world[node] * Mat4::from_cols_array(&m.bind_poses[i]).transpose())
                        })
                        .collect(),
                    node: world[r.node],
                }
            } else {
                let matrix = world[r.node];
                Skinning::Rigid {
                    matrix,
                    normal: matrix.inverse().transpose(),
                }
            };
            let bounds = r.bounds.map(|b| b.transformed(world[r.root_bone.unwrap_or(r.node)]));
            out.push(DrawMesh {
                renderer: r,
                base_positions,
                base_normals,
                skinning,
                bounds,
            });
        }
        out
    }
}
