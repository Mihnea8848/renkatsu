// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed views of the Unity object classes Koikatsu content is made of,
//! built from the dynamic values the type-tree decoder produces.
//!
//! Conventions are kept as stored: left-handed, Y up, clockwise front faces,
//! row-major matrices, textures with row 0 at the bottom.

use crate::value::{PPtr, UObject, UValue};
use crate::{Error, Result};

fn missing(what: &str) -> Error {
    Error::Format(format!("missing or malformed field {what}"))
}

fn pptrs(o: &UObject, name: &str) -> Vec<PPtr> {
    o.array(name)
        .map(|a| a.iter().filter_map(UValue::as_pptr).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------- hierarchy

#[derive(Debug, Clone)]
pub struct GameObject {
    pub name: String,
    pub active: bool,
    pub layer: i32,
    pub components: Vec<PPtr>,
}

impl GameObject {
    pub fn read(o: &UObject) -> Result<Self> {
        let components = o
            .array("m_Component")
            .ok_or_else(|| missing("m_Component"))?
            .iter()
            .filter_map(|c| c.get("component").or_else(|| c.get("second")).and_then(UValue::as_pptr))
            .collect();
        Ok(Self {
            name: o.str("m_Name").unwrap_or_default().to_string(),
            active: o.bool("m_IsActive").unwrap_or(true),
            layer: o.i64("m_Layer").unwrap_or(0) as i32,
            components,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Transform {
    pub game_object: PPtr,
    pub rotation: [f32; 4],
    pub position: [f32; 3],
    pub scale: [f32; 3],
    pub children: Vec<PPtr>,
    pub father: PPtr,
}

impl Transform {
    pub fn read(o: &UObject) -> Result<Self> {
        Ok(Self {
            game_object: o.pptr("m_GameObject").ok_or_else(|| missing("m_GameObject"))?,
            rotation: o
                .get("m_LocalRotation")
                .and_then(UValue::floats)
                .ok_or_else(|| missing("m_LocalRotation"))?,
            position: o
                .get("m_LocalPosition")
                .and_then(UValue::floats)
                .ok_or_else(|| missing("m_LocalPosition"))?,
            scale: o
                .get("m_LocalScale")
                .and_then(UValue::floats)
                .ok_or_else(|| missing("m_LocalScale"))?,
            children: pptrs(o, "m_Children"),
            father: o.pptr("m_Father").unwrap_or_default(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Renderer {
    pub game_object: PPtr,
    pub enabled: bool,
    /// 0 off, 1 on, 2 two-sided, 3 shadows only (the mesh itself is not drawn).
    pub cast_shadows: i32,
    pub materials: Vec<PPtr>,
    /// Skinned renderers only.
    pub mesh: PPtr,
    pub bones: Vec<PPtr>,
    pub root_bone: PPtr,
    pub blend_shape_weights: Vec<f32>,
}

impl Renderer {
    /// Reads a MeshRenderer or SkinnedMeshRenderer.
    pub fn read(o: &UObject) -> Result<Self> {
        Ok(Self {
            game_object: o.pptr("m_GameObject").ok_or_else(|| missing("m_GameObject"))?,
            enabled: o.bool("m_Enabled").unwrap_or(true),
            cast_shadows: o.i64("m_CastShadows").unwrap_or(1) as i32,
            materials: pptrs(o, "m_Materials"),
            mesh: o.pptr("m_Mesh").unwrap_or_default(),
            bones: pptrs(o, "m_Bones"),
            root_bone: o.pptr("m_RootBone").unwrap_or_default(),
            blend_shape_weights: o
                .array("m_BlendShapeWeights")
                .map(|a| a.iter().filter_map(UValue::as_f32).collect())
                .unwrap_or_default(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct MeshFilter {
    pub game_object: PPtr,
    pub mesh: PPtr,
}

impl MeshFilter {
    pub fn read(o: &UObject) -> Result<Self> {
        Ok(Self {
            game_object: o.pptr("m_GameObject").ok_or_else(|| missing("m_GameObject"))?,
            mesh: o.pptr("m_Mesh").unwrap_or_default(),
        })
    }
}

// --------------------------------------------------------------------- mesh

#[derive(Debug, Clone, Copy)]
pub struct SubMesh {
    pub first_index: u32,
    pub index_count: u32,
    /// 0 = triangles.
    pub topology: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BoneWeight {
    pub weights: [f32; 4],
    pub bones: [u32; 4],
}

#[derive(Debug, Clone, Copy)]
pub struct BlendShapeVertex {
    pub index: u32,
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct BlendShape {
    pub name: String,
    /// Deltas of the final frame (Koikatsu shapes have one frame each).
    pub vertices: Vec<BlendShapeVertex>,
    pub full_weight: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 4]>,
    pub colors: Vec<[f32; 4]>,
    pub uv: [Vec<[f32; 2]>; 4],
    pub indices: Vec<u32>,
    pub submeshes: Vec<SubMesh>,
    pub skin: Vec<BoneWeight>,
    /// Row-major 4x4 matrices, one per bone.
    pub bind_poses: Vec<[f32; 16]>,
    pub blend_shapes: Vec<BlendShape>,
}

fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = (h >> 10) & 0x1f;
    let frac = (h & 0x3ff) as f32;
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 => {
            if frac == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        e => (1.0 + frac / 1024.0) * 2f32.powi(e as i32 - 15),
    }
}

impl Mesh {
    pub fn read(o: &UObject) -> Result<Self> {
        let mut m = Mesh {
            name: o.str("m_Name").unwrap_or_default().to_string(),
            ..Default::default()
        };
        if o.i64("m_MeshCompression").unwrap_or(0) != 0 {
            return Err(Error::Format(format!(
                "mesh {} uses packed compression, not supported yet",
                m.name
            )));
        }

        // Indices.
        let ib = o.bytes("m_IndexBuffer").ok_or_else(|| missing("m_IndexBuffer"))?;
        let wide = o.i64("m_IndexFormat").unwrap_or(0) == 1;
        m.indices = if wide {
            ib.chunks_exact(4)
                .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
                .collect()
        } else {
            ib.chunks_exact(2)
                .map(|c| u16::from_le_bytes(c.try_into().unwrap()) as u32)
                .collect()
        };
        let index_size = if wide { 4 } else { 2 };
        for s in o.array("m_SubMeshes").unwrap_or_default() {
            let s = s.as_object().ok_or_else(|| missing("m_SubMeshes"))?;
            let base = s.i64("baseVertex").unwrap_or(0) as u32;
            let first_index = s.i64("firstByte").unwrap_or(0) as u32 / index_size;
            let index_count = s.i64("indexCount").unwrap_or(0) as u32;
            if base != 0 {
                for i in m
                    .indices
                    .iter_mut()
                    .skip(first_index as usize)
                    .take(index_count as usize)
                {
                    *i += base;
                }
            }
            m.submeshes.push(SubMesh {
                first_index,
                index_count,
                topology: s.i64("topology").unwrap_or(0) as u32,
            });
        }

        // Vertex streams.
        let vd = o
            .get("m_VertexData")
            .and_then(UValue::as_object)
            .ok_or_else(|| missing("m_VertexData"))?;
        let count = vd.i64("m_VertexCount").unwrap_or(0) as usize;
        let data = vd.bytes("m_DataSize").ok_or_else(|| missing("m_DataSize"))?;
        struct Chan {
            stream: usize,
            offset: usize,
            format: u8,
            dim: usize,
        }
        let chans: Vec<Chan> = vd
            .array("m_Channels")
            .unwrap_or_default()
            .iter()
            .filter_map(UValue::as_object)
            .map(|c| Chan {
                stream: c.i64("stream").unwrap_or(0) as usize,
                offset: c.i64("offset").unwrap_or(0) as usize,
                format: c.i64("format").unwrap_or(0) as u8,
                dim: (c.i64("dimension").unwrap_or(0) & 0xf) as usize,
            })
            .collect();
        // Legacy (pre-2017) vertex formats: float, half, colour byte, byte, u32.
        let fmt_size = |f: u8| match f {
            0 | 4 => 4,
            1 => 2,
            _ => 1,
        };
        let stream_count = chans
            .iter()
            .filter(|c| c.dim > 0)
            .map(|c| c.stream + 1)
            .max()
            .unwrap_or(0);
        let mut strides = vec![0usize; stream_count];
        for c in chans.iter().filter(|c| c.dim > 0) {
            strides[c.stream] = strides[c.stream].max(c.offset + c.dim * fmt_size(c.format));
        }
        let mut stream_off = vec![0usize; stream_count];
        let mut off = 0usize;
        for s in 0..stream_count {
            stream_off[s] = off;
            off += strides[s] * count;
            off = (off + 15) & !15;
        }
        let read = |c: &Chan, want: usize, default_w: f32| -> Result<Vec<[f32; 4]>> {
            let mut out = Vec::with_capacity(count);
            let stride = strides[c.stream];
            let size = fmt_size(c.format);
            for v in 0..count {
                let base = stream_off[c.stream] + v * stride + c.offset;
                let raw = data.get(base..base + c.dim * size).ok_or(Error::Eof)?;
                let mut val = [0.0, 0.0, 0.0, default_w];
                for (k, slot) in val.iter_mut().enumerate().take(c.dim.min(want)) {
                    let b = &raw[k * size..(k + 1) * size];
                    *slot = match c.format {
                        0 => f32::from_le_bytes(b.try_into().unwrap()),
                        1 => half_to_f32(u16::from_le_bytes(b.try_into().unwrap())),
                        2 => b[0] as f32 / 255.0,
                        3 => b[0] as f32,
                        _ => u32::from_le_bytes(b.try_into().unwrap()) as f32,
                    };
                }
                out.push(val);
            }
            Ok(out)
        };
        let v3 = |v: Vec<[f32; 4]>| v.into_iter().map(|a| [a[0], a[1], a[2]]).collect::<Vec<_>>();
        let v2 = |v: Vec<[f32; 4]>| v.into_iter().map(|a| [a[0], a[1]]).collect::<Vec<_>>();
        // Channel order: position, normal, colour, uv0..uv3, tangent.
        for (i, c) in chans.iter().enumerate() {
            if c.dim == 0 {
                continue;
            }
            match i {
                0 => m.positions = v3(read(c, 3, 0.0)?),
                1 => m.normals = v3(read(c, 3, 0.0)?),
                2 => m.colors = read(c, 4, 1.0)?,
                3..=6 => m.uv[i - 3] = v2(read(c, 2, 0.0)?),
                7 => m.tangents = read(c, 4, 1.0)?,
                _ => {}
            }
        }

        // Skinning.
        for s in o.array("m_Skin").unwrap_or_default() {
            let s = s.as_object().ok_or_else(|| missing("m_Skin"))?;
            let mut bw = BoneWeight::default();
            for (name, v) in &s.fields {
                let Some(idx) = name
                    .as_bytes()
                    .get(name.len().wrapping_sub(2))
                    .map(|b| (b - b'0') as usize)
                else {
                    continue;
                };
                if idx > 3 {
                    continue;
                }
                if name.starts_with("weight") {
                    bw.weights[idx] = v.as_f32().unwrap_or(0.0);
                } else if name.starts_with("boneIndex") {
                    bw.bones[idx] = v.as_i64().unwrap_or(0) as u32;
                }
            }
            m.skin.push(bw);
        }
        for b in o.array("m_BindPose").unwrap_or_default() {
            m.bind_poses
                .push(b.floats::<16>().ok_or_else(|| missing("m_BindPose"))?);
        }

        // Blend shapes.
        if let Some(shapes) = o.get("m_Shapes").and_then(UValue::as_object) {
            let verts = shapes.array("vertices").unwrap_or_default();
            let frames = shapes.array("shapes").unwrap_or_default();
            let weights = shapes.array("fullWeights").unwrap_or_default();
            for ch in shapes.array("channels").unwrap_or_default() {
                let ch = ch.as_object().ok_or_else(|| missing("channels"))?;
                let frame_count = ch.i64("frameCount").unwrap_or(0) as usize;
                if frame_count == 0 {
                    continue;
                }
                let last = ch.i64("frameIndex").unwrap_or(0) as usize + frame_count - 1;
                let Some(frame) = frames.get(last).and_then(UValue::as_object) else {
                    continue;
                };
                let first = frame.i64("firstVertex").unwrap_or(0) as usize;
                let n = frame.i64("vertexCount").unwrap_or(0) as usize;
                let mut vertices = Vec::with_capacity(n);
                for v in verts.iter().skip(first).take(n) {
                    let v = v.as_object().ok_or_else(|| missing("vertices"))?;
                    vertices.push(BlendShapeVertex {
                        index: v.i64("index").unwrap_or(0) as u32,
                        position: v.get("vertex").and_then(UValue::floats).unwrap_or_default(),
                        normal: v.get("normal").and_then(UValue::floats).unwrap_or_default(),
                    });
                }
                m.blend_shapes.push(BlendShape {
                    name: ch.str("name").unwrap_or_default().to_string(),
                    vertices,
                    full_weight: weights.get(last).and_then(UValue::as_f32).unwrap_or(100.0),
                });
            }
        }
        Ok(m)
    }
}

// ----------------------------------------------------------------- material

#[derive(Debug, Clone)]
pub struct TexEnv {
    pub texture: PPtr,
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}

#[derive(Debug, Clone, Default)]
pub struct Material {
    pub name: String,
    pub shader: PPtr,
    pub keywords: String,
    pub render_queue: i32,
    pub textures: Vec<(String, TexEnv)>,
    pub floats: Vec<(String, f32)>,
    pub colors: Vec<(String, [f32; 4])>,
}

impl Material {
    pub fn read(o: &UObject) -> Result<Self> {
        let mut m = Material {
            name: o.str("m_Name").unwrap_or_default().to_string(),
            shader: o.pptr("m_Shader").unwrap_or_default(),
            keywords: o.str("m_ShaderKeywords").unwrap_or_default().to_string(),
            render_queue: o.i64("m_CustomRenderQueue").unwrap_or(-1) as i32,
            ..Default::default()
        };
        let props = o
            .get("m_SavedProperties")
            .and_then(UValue::as_object)
            .ok_or_else(|| missing("m_SavedProperties"))?;
        // Keys are plain strings, or `{name}` structs in older files.
        let key = |v: &UValue| -> Option<String> {
            let k = v.get("first")?;
            k.as_str()
                .or_else(|| k.get("name").and_then(UValue::as_str))
                .map(str::to_string)
        };
        for e in props.array("m_TexEnvs").unwrap_or_default() {
            let (Some(k), Some(v)) = (key(e), e.get("second").and_then(UValue::as_object)) else {
                continue;
            };
            m.textures.push((
                k,
                TexEnv {
                    texture: v.pptr("m_Texture").unwrap_or_default(),
                    scale: v.get("m_Scale").and_then(UValue::floats).unwrap_or([1.0, 1.0]),
                    offset: v.get("m_Offset").and_then(UValue::floats).unwrap_or([0.0, 0.0]),
                },
            ));
        }
        for e in props.array("m_Floats").unwrap_or_default() {
            if let (Some(k), Some(v)) = (key(e), e.get("second").and_then(UValue::as_f32)) {
                m.floats.push((k, v));
            }
        }
        for e in props.array("m_Colors").unwrap_or_default() {
            if let (Some(k), Some(v)) = (key(e), e.get("second").and_then(UValue::floats)) {
                m.colors.push((k, v));
            }
        }
        Ok(m)
    }

    pub fn texture(&self, name: &str) -> Option<&TexEnv> {
        self.textures.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
    pub fn float(&self, name: &str) -> Option<f32> {
        self.floats.iter().find(|(k, _)| k == name).map(|(_, v)| *v)
    }
    pub fn color(&self, name: &str) -> Option<[f32; 4]> {
        self.colors.iter().find(|(k, _)| k == name).map(|(_, v)| *v)
    }
}

/// Name of a Shader object (`Shader Forge/main_skin`, ...).
pub fn shader_name(o: &UObject) -> Option<String> {
    o.path("m_ParsedForm.m_Name")
        .and_then(UValue::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| o.str("m_Name"))
        .map(str::to_string)
}

// ------------------------------------------------------------------ texture

#[derive(Debug, Clone)]
pub struct Texture2D {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: i32,
    pub mip_count: u32,
    /// 0 repeat, 1 clamp.
    pub wrap_mode: i32,
    /// 0 point, 1 bilinear, 2 trilinear.
    pub filter_mode: i32,
    /// 1 for colour (sRGB) textures, 0 for data textures.
    pub color_space: i32,
    /// Pixel data when embedded in the object.
    pub data: Vec<u8>,
    /// Location of the pixel data when it lives in a `.resS` node.
    pub stream: Option<StreamRef>,
}

#[derive(Debug, Clone)]
pub struct StreamRef {
    pub path: String,
    pub offset: u64,
    pub size: u64,
}

impl StreamRef {
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

impl Texture2D {
    pub fn read(o: &UObject) -> Result<Self> {
        let settings = o.get("m_TextureSettings").and_then(UValue::as_object);
        let stream = o.get("m_StreamData").and_then(UValue::as_object).and_then(|s| {
            let size = s.i64("size")? as u64;
            let path = s.str("path")?;
            (size > 0 && !path.is_empty()).then(|| StreamRef {
                path: path.to_string(),
                offset: s.i64("offset").unwrap_or(0) as u64,
                size,
            })
        });
        Ok(Self {
            name: o.str("m_Name").unwrap_or_default().to_string(),
            width: o.i64("m_Width").ok_or_else(|| missing("m_Width"))? as u32,
            height: o.i64("m_Height").ok_or_else(|| missing("m_Height"))? as u32,
            format: o.i64("m_TextureFormat").ok_or_else(|| missing("m_TextureFormat"))? as i32,
            mip_count: o.i64("m_MipCount").unwrap_or(1) as u32,
            wrap_mode: settings
                .and_then(|s| s.i64("m_WrapMode").or_else(|| s.i64("m_WrapU")))
                .unwrap_or(0) as i32,
            filter_mode: settings.and_then(|s| s.i64("m_FilterMode")).unwrap_or(1) as i32,
            color_space: o.i64("m_ColorSpace").unwrap_or(0) as i32,
            data: o.bytes("image data").unwrap_or_default().to_vec(),
            stream,
        })
    }

    /// Decodes the top mip level to RGBA8, rows bottom-up as stored.
    /// `data` is the embedded or streamed pixel data.
    pub fn decode_rgba8(&self, data: &[u8]) -> Result<Vec<u8>> {
        let (w, h) = (self.width as usize, self.height as usize);
        let px = w * h;
        let need = |n: usize| data.get(..n).ok_or(Error::Eof);
        let mut out = vec![0u8; px * 4];
        let bc = |f: fn(&[u8], usize, usize, &mut [u32]) -> std::result::Result<(), &'static str>,
                  out: &mut [u8]|
         -> Result<()> {
            let mut buf = vec![0u32; px];
            f(data, w, h, &mut buf).map_err(|e| Error::Format(format!("texture decode: {e}")))?;
            for (o, p) in out.chunks_exact_mut(4).zip(&buf) {
                // Decoder output is 0xAARRGGBB.
                o.copy_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8]);
            }
            Ok(())
        };
        match self.format {
            1 => {
                for (o, a) in out.chunks_exact_mut(4).zip(need(px)?) {
                    o.copy_from_slice(&[255, 255, 255, *a]);
                }
            }
            63 => {
                for (o, r) in out.chunks_exact_mut(4).zip(need(px)?) {
                    o.copy_from_slice(&[*r, 0, 0, 255]);
                }
            }
            3 => {
                for (o, c) in out.chunks_exact_mut(4).zip(need(px * 3)?.chunks_exact(3)) {
                    o.copy_from_slice(&[c[0], c[1], c[2], 255]);
                }
            }
            4 => out.copy_from_slice(need(px * 4)?),
            5 => {
                for (o, c) in out.chunks_exact_mut(4).zip(need(px * 4)?.chunks_exact(4)) {
                    o.copy_from_slice(&[c[1], c[2], c[3], c[0]]);
                }
            }
            14 => {
                for (o, c) in out.chunks_exact_mut(4).zip(need(px * 4)?.chunks_exact(4)) {
                    o.copy_from_slice(&[c[2], c[1], c[0], c[3]]);
                }
            }
            2 | 13 => {
                for (o, c) in out.chunks_exact_mut(4).zip(need(px * 2)?.chunks_exact(2)) {
                    let v = u16::from_le_bytes([c[0], c[1]]);
                    let n = |s: u16| (((v >> s) & 0xf) * 17) as u8;
                    // ARGB4444 keeps alpha in the top nibble, RGBA4444 in the bottom.
                    *<&mut [u8; 4]>::try_from(o).unwrap() = if self.format == 2 {
                        [n(8), n(4), n(0), n(12)]
                    } else {
                        [n(12), n(8), n(4), n(0)]
                    };
                }
            }
            7 => {
                for (o, c) in out.chunks_exact_mut(4).zip(need(px * 2)?.chunks_exact(2)) {
                    let v = u16::from_le_bytes([c[0], c[1]]);
                    let r = ((v >> 11) & 0x1f) as u32;
                    let g = ((v >> 5) & 0x3f) as u32;
                    let b = (v & 0x1f) as u32;
                    o.copy_from_slice(&[(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8, 255]);
                }
            }
            17 => {
                for (o, c) in out.chunks_exact_mut(4).zip(need(px * 8)?.chunks_exact(8)) {
                    for k in 0..4 {
                        let f = half_to_f32(u16::from_le_bytes([c[k * 2], c[k * 2 + 1]]));
                        o[k] = (f.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    }
                }
            }
            20 => {
                for (o, c) in out.chunks_exact_mut(4).zip(need(px * 16)?.chunks_exact(16)) {
                    for k in 0..4 {
                        let f = f32::from_le_bytes(c[k * 4..k * 4 + 4].try_into().unwrap());
                        o[k] = (f.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    }
                }
            }
            10 => bc(texture2ddecoder::decode_bc1, &mut out)?,
            12 => bc(texture2ddecoder::decode_bc3, &mut out)?,
            25 => bc(texture2ddecoder::decode_bc7, &mut out)?,
            26 => bc(texture2ddecoder::decode_bc4, &mut out)?,
            27 => bc(texture2ddecoder::decode_bc5, &mut out)?,
            28 | 29 => bc(texture2ddecoder::decode_unity_crunch, &mut out)?,
            f => {
                return Err(Error::Format(format!(
                    "texture format {f} is not supported yet ({})",
                    self.name
                )))
            }
        }
        Ok(out)
    }
}
