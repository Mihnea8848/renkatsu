// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builds a renderable character from a Koikatsu card: skeleton, body, head,
//! hair, clothes and accessories assembled the way the game does it.

pub mod abmx;
pub mod build;
pub mod compose;
pub mod expression;
pub mod eyelook;
pub mod foreground;
pub mod hair_accessories;
pub mod loader;
pub mod material_editor;
pub mod more_accessories;
pub mod overlays;
pub mod scene;
pub mod shape;

pub use build::{build, BuildOptions, Character};
pub use loader::Loader;
pub use scene::{DrawMesh, DrawState, Image, MaterialInst, Part, RendererInst, Scene, Stencil, TexSlot, Trs};
