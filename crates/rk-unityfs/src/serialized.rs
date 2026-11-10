// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Unity serialized file: type table, object table, external references,
//! and type-tree driven object decoding.

use crate::common_strings::COMMON_STRINGS;
use crate::io::Cursor;
use crate::value::{UObject, UValue};
use crate::{Error, Result};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// One node of a type tree, with its children nested.
#[derive(Debug, Clone)]
pub struct TypeNode {
    pub ty: Arc<str>,
    pub name: Arc<str>,
    pub byte_size: i32,
    pub flags: u32,
    pub children: Vec<TypeNode>,
}

impl TypeNode {
    fn aligned(&self) -> bool {
        self.flags & 0x4000 != 0
    }
}

#[derive(Debug, Clone)]
pub struct TypeInfo {
    pub class_id: i32,
    pub script_type_index: i16,
    pub tree: Option<Arc<TypeNode>>,
}

#[derive(Debug, Clone, Copy)]
pub struct ObjectInfo {
    pub path_id: i64,
    pub start: usize,
    pub size: usize,
    pub class_id: i32,
    pub type_index: usize,
}

#[derive(Debug, Clone)]
pub struct External {
    pub path: String,
}

impl External {
    /// File name part, e.g. `cab-0123...` from `archive:/CAB-0123.../CAB-0123...`.
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

pub struct SerializedFile {
    pub version: u32,
    pub unity_version: String,
    pub types: Vec<TypeInfo>,
    pub objects: Vec<ObjectInfo>,
    pub externals: Vec<External>,
    data: Arc<[u8]>,
    by_id: HashMap<i64, usize>,
}

fn common_string(offset: u32) -> Option<&'static str> {
    static TABLE: OnceLock<HashMap<u32, &'static str>> = OnceLock::new();
    TABLE
        .get_or_init(|| {
            let mut m = HashMap::new();
            let mut off = 0u32;
            for s in COMMON_STRINGS {
                m.insert(off, *s);
                off += s.len() as u32 + 1;
            }
            m
        })
        .get(&offset)
        .copied()
}

fn read_type_tree(c: &mut Cursor, version: u32) -> Result<TypeNode> {
    struct Flat {
        level: u8,
        node: TypeNode,
    }
    let mut flat = Vec::new();
    if version >= 12 || version == 10 {
        let count = c.u32()? as usize;
        let str_size = c.u32()? as usize;
        let node_size = if version >= 19 { 32 } else { 24 };
        let nodes_start = c.pos();
        c.skip(count * node_size)?;
        let strings = c.take(str_size)?;
        let end = c.pos();
        let lookup = |off: u32| -> Arc<str> {
            if off & 0x8000_0000 != 0 {
                return common_string(off & 0x7fff_ffff).unwrap_or("?").into();
            }
            let rest = strings.get(off as usize..).unwrap_or(&[]);
            let n = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
            String::from_utf8_lossy(&rest[..n]).into()
        };
        for i in 0..count {
            c.seek(nodes_start + i * node_size);
            let _ver = c.u16()?;
            let level = c.u8()?;
            let _is_array = c.u8()?;
            let ty = lookup(c.u32()?);
            let name = lookup(c.u32()?);
            let byte_size = c.i32()?;
            let _index = c.i32()?;
            let flags = c.u32()?;
            flat.push(Flat {
                level,
                node: TypeNode {
                    ty,
                    name,
                    byte_size,
                    flags,
                    children: Vec::new(),
                },
            });
        }
        c.seek(end);
    } else {
        // Old recursive layout.
        fn rec(c: &mut Cursor, version: u32, level: u8, out: &mut Vec<Flat>) -> Result<()> {
            let ty: Arc<str> = c.cstr()?.into();
            let name: Arc<str> = c.cstr()?.into();
            let byte_size = c.i32()?;
            if version == 2 {
                c.i32()?;
            }
            if version != 3 {
                c.i32()?;
            }
            let _is_array = c.i32()?;
            let _ver = c.i32()?;
            let flags = if version != 3 { c.u32()? } else { 0 };
            out.push(Flat {
                level,
                node: TypeNode {
                    ty,
                    name,
                    byte_size,
                    flags,
                    children: Vec::new(),
                },
            });
            let children = c.u32()?;
            for _ in 0..children {
                rec(c, version, level + 1, out)?;
            }
            Ok(())
        }
        rec(c, version, 0, &mut flat)?;
    }
    // Fold the flat, level-tagged list into a tree.
    let mut stack: Vec<Flat> = Vec::new();
    for f in flat {
        while stack.last().is_some_and(|top| top.level >= f.level) {
            let done = stack.pop().unwrap();
            match stack.last_mut() {
                Some(parent) => parent.node.children.push(done.node),
                None => return Err(Error::Format("type tree has several roots".into())),
            }
        }
        stack.push(f);
    }
    while stack.len() > 1 {
        let done = stack.pop().unwrap();
        stack.last_mut().unwrap().node.children.push(done.node);
    }
    stack
        .pop()
        .map(|f| f.node)
        .ok_or_else(|| Error::Format("empty type tree".into()))
}

impl SerializedFile {
    pub fn parse(data: Arc<[u8]>) -> Result<Self> {
        let mut c = Cursor::new(&data, true);
        let mut _metadata_size = c.u32()? as u64;
        let mut _file_size = c.u32()? as u64;
        let version = c.u32()?;
        let mut data_offset = c.u32()? as u64;
        let mut big = true;
        if version >= 9 {
            big = c.u8()? != 0;
            c.skip(3)?;
        }
        if version >= 22 {
            _metadata_size = c.u32()? as u64;
            _file_size = c.u64()?;
            data_offset = c.u64()?;
            c.skip(8)?;
        }
        if version < 9 {
            return Err(Error::Format(format!("serialized file version {version} is too old")));
        }
        c.set_big(big);
        let unity_version = if version >= 7 { c.cstr()? } else { String::new() };
        if version >= 8 {
            c.i32()?; // target platform
        }
        let has_trees = if version >= 13 { c.u8()? != 0 } else { true };

        let type_count = c.u32()? as usize;
        let mut types = Vec::with_capacity(type_count);
        for _ in 0..type_count {
            let class_id = c.i32()?;
            if version >= 16 {
                c.u8()?; // stripped
            }
            let script_type_index = if version >= 17 { c.i16()? } else { -1 };
            if version >= 13 {
                if (version < 16 && class_id < 0) || (version >= 16 && class_id == 114) {
                    c.skip(16)?; // script id
                }
                c.skip(16)?; // old type hash
            }
            let tree = if has_trees {
                Some(Arc::new(read_type_tree(&mut c, version)?))
            } else {
                None
            };
            if has_trees && version >= 21 {
                let n = c.u32()? as usize;
                c.skip(n * 4)?; // type dependencies
            }
            types.push(TypeInfo {
                class_id,
                script_type_index,
                tree,
            });
        }

        if (7..14).contains(&version) {
            c.i32()?; // big id enabled
        }
        let object_count = c.u32()? as usize;
        let mut objects = Vec::with_capacity(object_count);
        for _ in 0..object_count {
            let path_id = if version >= 14 {
                c.align(4);
                c.i64()?
            } else {
                c.i32()? as i64
            };
            let start = if version >= 22 { c.u64()? } else { c.u32()? as u64 } + data_offset;
            let size = c.u32()? as usize;
            let type_id = c.i32()?;
            let (class_id, type_index) = if version >= 16 {
                let idx = type_id as usize;
                let t = types
                    .get(idx)
                    .ok_or_else(|| Error::Format("object type index out of range".into()))?;
                (t.class_id, idx)
            } else {
                let class_id = c.u16()? as i32;
                let idx = types.iter().position(|t| t.class_id == type_id).unwrap_or(usize::MAX);
                (class_id, idx)
            };
            if version < 11 {
                c.u16()?; // destroyed
            }
            if (11..17).contains(&version) {
                c.i16()?; // script type index
            }
            if version == 15 || version == 16 {
                c.u8()?; // stripped
            }
            objects.push(ObjectInfo {
                path_id,
                start: start as usize,
                size,
                class_id,
                type_index,
            });
        }

        if version >= 11 {
            let n = c.u32()? as usize;
            for _ in 0..n {
                c.i32()?;
                if version >= 14 {
                    c.align(4);
                    c.i64()?;
                } else {
                    c.i32()?;
                }
            }
        }
        let ext_count = c.u32()? as usize;
        let mut externals = Vec::with_capacity(ext_count);
        for _ in 0..ext_count {
            if version >= 6 {
                c.cstr()?;
            }
            if version >= 5 {
                c.skip(16)?;
                c.i32()?;
            }
            externals.push(External { path: c.cstr()? });
        }
        if big {
            return Err(Error::Format("big-endian serialized files are not supported".into()));
        }
        let by_id = objects.iter().enumerate().map(|(i, o)| (o.path_id, i)).collect();
        Ok(Self {
            version,
            unity_version,
            types,
            objects,
            externals,
            data,
            by_id,
        })
    }

    pub fn object(&self, path_id: i64) -> Option<&ObjectInfo> {
        self.by_id.get(&path_id).map(|&i| &self.objects[i])
    }

    pub fn objects_of(&self, class_id: i32) -> impl Iterator<Item = &ObjectInfo> {
        self.objects.iter().filter(move |o| o.class_id == class_id)
    }

    pub fn type_tree(&self, obj: &ObjectInfo) -> Option<&Arc<TypeNode>> {
        self.types.get(obj.type_index)?.tree.as_ref()
    }

    pub fn raw(&self, obj: &ObjectInfo) -> Result<&[u8]> {
        self.data.get(obj.start..obj.start + obj.size).ok_or(Error::Eof)
    }

    /// Decodes a whole object into a dynamic value.
    pub fn read(&self, obj: &ObjectInfo) -> Result<UObject> {
        let tree = self.type_tree(obj).ok_or(Error::NoTypeTree(obj.class_id))?;
        let mut c = Cursor::new(self.raw(obj)?, false);
        match read_node(&mut c, tree, None)? {
            UValue::Object(o) => Ok(*o),
            _ => Err(Error::Format("object root is not a struct".into())),
        }
    }

    /// Like [`read`](Self::read), but fails unless decoding consumed the
    /// object's exact byte length. Used by tests to prove the decoder right.
    pub fn read_strict(&self, obj: &ObjectInfo) -> Result<UObject> {
        let tree = self.type_tree(obj).ok_or(Error::NoTypeTree(obj.class_id))?;
        let mut c = Cursor::new(self.raw(obj)?, false);
        let v = read_node(&mut c, tree, None)?;
        if obj.size - c.pos().min(obj.size) > 3 || c.pos() > obj.size + 3 {
            return Err(Error::Format(format!("decoded {} of {} bytes", c.pos(), obj.size)));
        }
        match v {
            UValue::Object(o) => Ok(*o),
            _ => Err(Error::Format("object root is not a struct".into())),
        }
    }

    /// Decodes only the leading fields up to and including `last_field`.
    /// Cheap way to get `m_Name` or a MonoBehaviour's `m_Script`.
    pub fn read_until(&self, obj: &ObjectInfo, last_field: &str) -> Result<UObject> {
        let tree = self.type_tree(obj).ok_or(Error::NoTypeTree(obj.class_id))?;
        let mut c = Cursor::new(self.raw(obj)?, false);
        match read_node(&mut c, tree, Some(last_field))? {
            UValue::Object(o) => Ok(*o),
            _ => Err(Error::Format("object root is not a struct".into())),
        }
    }

    /// `m_Name` of an object, when its type has one.
    pub fn name(&self, obj: &ObjectInfo) -> Option<String> {
        let tree = self.type_tree(obj)?;
        if !tree.children.iter().any(|n| &*n.name == "m_Name") {
            return None;
        }
        self.read_until(obj, "m_Name").ok()?.str("m_Name").map(str::to_string)
    }
}

fn read_array(c: &mut Cursor, array: &TypeNode) -> Result<UValue> {
    let elem = array
        .children
        .get(1)
        .ok_or_else(|| Error::Format("array node without element type".into()))?;
    let len = c.i32()?;
    if len < 0 || len as usize > c.remaining() {
        return Err(Error::Format(format!("array length {len} out of range")));
    }
    let len = len as usize;
    let v = if elem.children.is_empty() && matches!(&*elem.ty, "UInt8" | "char" | "SInt8") {
        UValue::Bytes(c.take(len)?.to_vec())
    } else {
        let mut items = Vec::with_capacity(len.min(1 << 20));
        for _ in 0..len {
            items.push(read_node(c, elem, None)?);
        }
        UValue::Array(items)
    };
    if array.aligned() {
        c.align(4);
    }
    Ok(v)
}

fn read_node(c: &mut Cursor, node: &TypeNode, stop_after: Option<&str>) -> Result<UValue> {
    let v = match &*node.ty {
        "bool" => UValue::Bool(c.u8()? != 0),
        "SInt8" => UValue::Int(c.u8()? as i8 as i64),
        "UInt8" | "char" => UValue::Int(c.u8()? as i64),
        "SInt16" | "short" => UValue::Int(c.i16()? as i64),
        "UInt16" | "unsigned short" => UValue::Int(c.u16()? as i64),
        "SInt32" | "int" => UValue::Int(c.i32()? as i64),
        "UInt32" | "unsigned int" | "Type*" => UValue::Int(c.u32()? as i64),
        "SInt64" | "long long" => UValue::Int(c.i64()?),
        "UInt64" | "unsigned long long" | "FileSize" => UValue::Int(c.u64()? as i64),
        "float" => UValue::Float(c.f32()? as f64),
        "double" => UValue::Float(c.f64()?),
        "string" => {
            let len = c.i32()?;
            if len < 0 {
                return Err(Error::Format("negative string length".into()));
            }
            let bytes = c.take(len as usize)?;
            c.align(4);
            // TextAsset payloads are declared as strings but hold raw bytes.
            return Ok(match std::str::from_utf8(bytes) {
                Ok(s) => UValue::Str(s.to_string()),
                Err(_) => UValue::Bytes(bytes.to_vec()),
            });
        }
        "TypelessData" => {
            let len = c.i32()?;
            if len < 0 {
                return Err(Error::Format("negative data length".into()));
            }
            UValue::Bytes(c.take(len as usize)?.to_vec())
        }
        _ => {
            if node.children.len() == 1 && &*node.children[0].ty == "Array" {
                read_array(c, &node.children[0])?
            } else if &*node.ty == "Array" {
                return read_array(c, node);
            } else {
                let mut fields = Vec::with_capacity(node.children.len());
                for child in &node.children {
                    fields.push((child.name.clone(), read_node(c, child, None)?));
                    if stop_after == Some(&*child.name) {
                        return Ok(UValue::Object(Box::new(UObject {
                            ty: node.ty.clone(),
                            fields,
                        })));
                    }
                }
                UValue::Object(Box::new(UObject {
                    ty: node.ty.clone(),
                    fields,
                }))
            }
        }
    };
    if node.aligned() {
        c.align(4);
    }
    Ok(v)
}
