// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builds a renderable character from a Koikatsu card: skeleton, body, head,
//! hair, clothes and accessories assembled the way the game does it.

pub mod abmx;
pub mod build;
pub mod compose;
pub mod loader;
pub mod scene;
pub mod shape;

pub use build::{build, BuildOptions, Character};
pub use loader::Loader;
pub use scene::{DrawMesh, Image, MaterialInst, Part, RendererInst, Scene, TexSlot, Trs};
