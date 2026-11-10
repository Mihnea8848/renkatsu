// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Dynamic value tree produced by type-tree decoding.

use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum UValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    Array(Vec<UValue>),
    Object(Box<UObject>),
}

#[derive(Debug, Clone)]
pub struct UObject {
    pub ty: Arc<str>,
    pub fields: Vec<(Arc<str>, UValue)>,
}

/// Reference to another object: file index (0 = same file) and path id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PPtr {
    pub file_id: i32,
    pub path_id: i64,
}

impl PPtr {
    pub fn is_null(&self) -> bool {
        self.path_id == 0
    }
}

impl UValue {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            UValue::Int(i) => Some(*i),
            UValue::Bool(b) => Some(*b as i64),
            _ => None,
        }
    }
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            UValue::Float(f) => Some(*f as f32),
            UValue::Int(i) => Some(*i as f32),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            UValue::Bool(b) => Some(*b),
            UValue::Int(i) => Some(*i != 0),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            UValue::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            UValue::Bytes(b) => Some(b),
            UValue::Str(s) => Some(s.as_bytes()),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&[UValue]> {
        match self {
            UValue::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_object(&self) -> Option<&UObject> {
        match self {
            UValue::Object(o) => Some(o),
            _ => None,
        }
    }
    pub fn get(&self, name: &str) -> Option<&UValue> {
        self.as_object()?.get(name)
    }
    pub fn as_pptr(&self) -> Option<PPtr> {
        self.as_object()?.as_pptr()
    }
    /// Reads a struct of float fields (Vector3f, Quaternionf, ColorRGBA...) in
    /// declaration order.
    pub fn floats<const N: usize>(&self) -> Option<[f32; N]> {
        let o = self.as_object()?;
        if o.fields.len() < N {
            return None;
        }
        let mut out = [0.0; N];
        for (i, (_, v)) in o.fields.iter().take(N).enumerate() {
            out[i] = v.as_f32()?;
        }
        Some(out)
    }
}

impl UObject {
    pub fn get(&self, name: &str) -> Option<&UValue> {
        self.fields.iter().find(|(n, _)| &**n == name).map(|(_, v)| v)
    }
    /// Dotted path lookup, e.g. `m_ParsedForm.m_Name`.
    pub fn path(&self, path: &str) -> Option<&UValue> {
        let mut parts = path.split('.');
        let mut cur = self.get(parts.next()?)?;
        for p in parts {
            cur = cur.get(p)?;
        }
        Some(cur)
    }
    pub fn str(&self, name: &str) -> Option<&str> {
        self.get(name)?.as_str()
    }
    pub fn i64(&self, name: &str) -> Option<i64> {
        self.get(name)?.as_i64()
    }
    pub fn f32(&self, name: &str) -> Option<f32> {
        self.get(name)?.as_f32()
    }
    pub fn bool(&self, name: &str) -> Option<bool> {
        self.get(name)?.as_bool()
    }
    pub fn array(&self, name: &str) -> Option<&[UValue]> {
        self.get(name)?.as_array()
    }
    pub fn bytes(&self, name: &str) -> Option<&[u8]> {
        self.get(name)?.as_bytes()
    }
    pub fn pptr(&self, name: &str) -> Option<PPtr> {
        self.get(name)?.as_pptr()
    }
    pub fn as_pptr(&self) -> Option<PPtr> {
        Some(PPtr {
            file_id: self.i64("m_FileID")? as i32,
            path_id: self.i64("m_PathID")?,
        })
    }
}
