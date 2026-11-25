// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Builds a renderable character from a Koikatsu card: skeleton, body, head,
//! hair, clothes and accessories assembled the way the game does it.

pub mod abmx;
pub mod accessory_clothes;
pub mod accessory_states;
pub mod ahegao;
pub mod animation_file;
pub mod animations;
pub mod better_penetration;
pub mod body_collision;
pub mod bone_info;
pub mod build;
pub mod channels;
pub mod clothes_state;
pub mod clothing_blend_shape;
pub mod clothing_unlocker;
pub mod colliders;
pub mod compose;
pub mod copy_body_material;
pub mod dan_look;
pub mod dbde;
pub mod dynamic_bone_editor;
pub mod edit_fangs;
pub mod expression;
pub mod expression_control;
pub mod eye_control;
pub mod eyelook;
pub mod face;
pub mod fbbik;
pub mod foreground;
pub mod gaze;
pub mod hair_accessories;
pub mod hand_pattern;
pub mod hand_size;
pub mod ik;
pub mod invisible_body;
pub mod keys;
pub mod loader;
pub mod material_editor;
pub mod more_accessories;
pub mod more_outfits;
pub mod motion;
pub mod neck;
pub mod overlays;
pub mod physics;
pub mod pose;
pub mod pregnancy;
pub mod pushup;
pub mod scene;
pub mod shape;
pub mod siru;
pub mod skin_effects;
pub mod slider_unlocker;
pub mod stiletto;
pub mod uncensor_selector;

pub use build::{build, BuildOptions, Character};

/// Whether a material is the censor the game draws over the body parts it
/// covers. It is the only thing `Shader Forge/mnpb` is used for, and an
/// uncensor mod replaces the part's material along with its mesh, so the
/// shader's name is what tells a censored part from an uncensored one.
pub fn is_censor(shader: &str) -> bool {
    shader.rsplit('/').next().unwrap_or(shader).eq_ignore_ascii_case("mnpb")
}

pub use loader::Loader;
pub use scene::{
    Blend, Bounds, DrawMesh, DrawState, Image, MaterialInst, Part, Posed, RendererInst, Scene, ShadowCasting,
    ShadowPass, Skinning, Stencil, TexSlot, Trs,
};

#[cfg(test)]
mod censor_tests {
    #[test]
    fn the_censor_is_told_by_its_shader() {
        assert!(super::is_censor("Shader Forge/mnpb"));
        assert!(super::is_censor("mnpb"));
        // The skin an uncensor gives those parts, and a mod shader whose
        // name only starts the same way.
        assert!(!super::is_censor("Shader Forge/main_skin"));
        assert!(!super::is_censor("Shader Forge/mnpb_plus"));
    }
}
