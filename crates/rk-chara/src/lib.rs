// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builds a renderable character from a Koikatsu card: skeleton, body, head,
//! hair, clothes and accessories assembled the way the game does it.

pub mod abmx;
pub mod accessory_clothes;
pub mod accessory_states;
pub mod build;
pub mod clothing_unlocker;
pub mod colliders;
pub mod compose;
pub mod dynamic_bone_editor;
pub mod expression;
pub mod eye_control;
pub mod eyelook;
pub mod foreground;
pub mod hair_accessories;
pub mod hand_size;
pub mod invisible_body;
pub mod loader;
pub mod material_editor;
pub mod more_accessories;
pub mod overlays;
pub mod physics;
pub mod pregnancy;
pub mod pushup;
pub mod scene;
pub mod shape;
pub mod slider_unlocker;
pub mod uncensor_selector;

pub use build::{build, BuildOptions, Character};
pub use loader::Loader;
pub use scene::{
    Blend, Bounds, DrawMesh, DrawState, Image, MaterialInst, Part, RendererInst, Scene, ShadowCasting, ShadowPass,
    Stencil, TexSlot, Trs,
};
