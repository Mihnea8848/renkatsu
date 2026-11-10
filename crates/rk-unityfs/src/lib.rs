// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reader for Unity asset bundle files (`.unity3d`), the container format all
//! Koikatsu assets and mods are stored in. This is a file-format decoder
//! only; no Unity code or runtime is involved.

mod bundle;
mod common_strings;
mod io;
pub mod objects;
mod serialized;
mod value;

pub use bundle::{Bundle, Node};
pub use serialized::{External, ObjectInfo, SerializedFile, TypeInfo, TypeNode};
pub use value::{PPtr, UObject, UValue};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("unexpected end of data")]
    Eof,
    #[error("{0}")]
    Format(String),
    #[error("{0}")]
    Compression(String),
    #[error("object class {0} has no type tree")]
    NoTypeTree(i32),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Unity class ids used by Koikatsu content.
pub mod class {
    pub const GAME_OBJECT: i32 = 1;
    pub const TRANSFORM: i32 = 4;
    pub const MATERIAL: i32 = 21;
    pub const MESH_RENDERER: i32 = 23;
    pub const TEXTURE2D: i32 = 28;
    pub const MESH_FILTER: i32 = 33;
    pub const MESH: i32 = 43;
    pub const SHADER: i32 = 48;
    pub const TEXT_ASSET: i32 = 49;
    pub const ANIMATION_CLIP: i32 = 74;
    pub const ANIMATOR_CONTROLLER: i32 = 91;
    pub const ANIMATOR: i32 = 95;
    pub const MONO_BEHAVIOUR: i32 = 114;
    pub const MONO_SCRIPT: i32 = 115;
    pub const SKINNED_MESH_RENDERER: i32 = 137;
    pub const ASSET_BUNDLE: i32 = 142;
    pub const RECT_TRANSFORM: i32 = 224;
}
