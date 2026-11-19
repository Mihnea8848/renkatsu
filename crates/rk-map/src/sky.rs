// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! A scene's sky, from the material `RenderSettings.m_SkyboxMaterial`
//! names: which of Unity's skybox shaders it uses, its numbers as the
//! shader gets them, and how the bundle's copy of the shader was compiled.

use crate::build::to_linear;
use anyhow::{ensure, Result};
use rk_render::map::{RGBM_EXPONENT, RGBM_SCALE};
use rk_render::sky::{MapCube, ProceduralSky, SkyLook, SunDisk, DOUBLE_GAMMA, DOUBLE_LINEAR, HDR_PLAIN};
use rk_unityfs::objects::{Material, Texture2D};

/// A material's colour as a shader gets it in linear colour.
fn linear3(c: [f32; 4]) -> [f32; 3] {
    [to_linear(c[0]), to_linear(c[1]), to_linear(c[2])]
}

/// The built-in skybox shaders ported here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkyShader {
    Procedural,
    SixSided,
    Cubemap,
}

impl SkyShader {
    pub fn named(shader: &str) -> Option<SkyShader> {
        match shader {
            "Skybox/Procedural" => Some(SkyShader::Procedural),
            "Skybox/6 Sided" => Some(SkyShader::SixSided),
            "Skybox/Cubemap" => Some(SkyShader::Cubemap),
            _ => None,
        }
    }
}

/// The colour space a bundle's copy of a shader was compiled for. Unity
/// bakes it into the program: a map built in a gamma project carries
/// programs with other constants, and the game runs them as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compile {
    Linear,
    Gamma,
}

fn holds(programs: &[u8], constant: f32) -> bool {
    let bytes = constant.to_le_bytes();
    programs.windows(4).any(|w| w == bytes)
}

/// How a sky shader's Direct3D 11 programs were compiled, by a constant
/// only the linear ones hold: `unity_ColorSpaceDouble` (4.594794) in the
/// textured skies, the gamma exponent 1/2.2 in the procedural one.
pub fn compile_of(shader: SkyShader, programs: &[u8]) -> Compile {
    let linear = match shader {
        SkyShader::Procedural => holds(programs, 0.454_545_47),
        SkyShader::SixSided | SkyShader::Cubemap => holds(programs, DOUBLE_LINEAR),
    };
    if linear {
        Compile::Linear
    } else {
        Compile::Gamma
    }
}

/// `Skybox/Procedural`'s material. A number the material does not hold is
/// the shader's default.
pub fn procedural(m: &Material) -> ProceduralSky {
    let keywords = m.keywords.to_ascii_uppercase();
    // The first of the shader's variants is the one without a keyword.
    let sun_disk = if keywords.contains("_SUNDISK_HIGH_QUALITY") {
        SunDisk::HighQuality
    } else if keywords.contains("_SUNDISK_SIMPLE") {
        SunDisk::Simple
    } else {
        SunDisk::None
    };
    ProceduralSky {
        sun_disk,
        sun_size: m.float("_SunSize").unwrap_or(0.04),
        atmosphere_thickness: m.float("_AtmosphereThickness").unwrap_or(1.0),
        sky_tint: linear3(m.color("_SkyTint").unwrap_or([0.5, 0.5, 0.5, 1.0])),
        ground: linear3(m.color("_GroundColor").unwrap_or([0.369, 0.349, 0.341, 1.0])),
        exposure: m.float("_Exposure").unwrap_or(1.3),
    }
}

/// What `Skybox/6 Sided` and `Skybox/Cubemap` share, with the constants
/// of the compile.
pub fn look(m: &Material, compile: Compile) -> SkyLook {
    SkyLook {
        tint: linear3(m.color("_Tint").unwrap_or([0.5; 4])),
        exposure: m.float("_Exposure").unwrap_or(1.0),
        rotation: m.float("_Rotation").unwrap_or(0.0),
        double: match compile {
            Compile::Linear => DOUBLE_LINEAR,
            Compile::Gamma => DOUBLE_GAMMA,
        },
        exponent: compile == Compile::Linear,
    }
}

/// `_Tex_HDR` as Unity sets it for a texture in linear colour, by the
/// texture's usage (`m_LightmapFormat`): 0 is a plain texture, 2 is RGBM
/// as the lightmaps are. `None` for a usage not met yet.
pub fn hdr_of(lightmap_format: i32) -> Option<[f32; 4]> {
    match lightmap_format {
        0 => Some(HDR_PLAIN),
        2 => Some([RGBM_SCALE, RGBM_EXPONENT, 0.0, 1.0]),
        _ => None,
    }
}

/// The six faces of a Cubemap object, decoded. Unity stores the faces one
/// after the other, each with its mip levels, in Direct3D's order; the top
/// level of each is kept.
pub fn cube(tex: &Texture2D, raw: &[u8]) -> Result<MapCube> {
    ensure!(
        tex.width == tex.height && tex.width > 0,
        "cubemap {} is not square",
        tex.name
    );
    ensure!(
        !raw.is_empty() && raw.len().is_multiple_of(6),
        "cubemap {} does not hold six faces",
        tex.name
    );
    let each = raw.len() / 6;
    let mut faces: [Vec<u8>; 6] = Default::default();
    for (i, face) in faces.iter_mut().enumerate() {
        *face = tex.decode_rgba8(&raw[i * each..(i + 1) * each])?;
    }
    Ok(MapCube {
        name: tex.name.clone(),
        size: tex.width,
        faces,
        srgb: tex.color_space != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material(keywords: &str) -> Material {
        Material {
            keywords: keywords.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn shaders_are_known_by_name() {
        assert_eq!(SkyShader::named("Skybox/Procedural"), Some(SkyShader::Procedural));
        assert_eq!(SkyShader::named("Skybox/6 Sided"), Some(SkyShader::SixSided));
        assert_eq!(SkyShader::named("Skybox/Cubemap"), Some(SkyShader::Cubemap));
        assert_eq!(SkyShader::named("Standard"), None);
    }

    #[test]
    fn the_compile_is_told_by_its_constants() {
        let mut programs = b"DXBC....".to_vec();
        assert_eq!(compile_of(SkyShader::Cubemap, &programs), Compile::Gamma);
        assert_eq!(compile_of(SkyShader::Procedural, &programs), Compile::Gamma);
        programs.extend_from_slice(&DOUBLE_LINEAR.to_le_bytes());
        assert_eq!(compile_of(SkyShader::Cubemap, &programs), Compile::Linear);
        assert_eq!(compile_of(SkyShader::SixSided, &programs), Compile::Linear);
        assert_eq!(compile_of(SkyShader::Procedural, &programs), Compile::Gamma);
        programs.extend_from_slice(&0.454_545_47f32.to_le_bytes());
        assert_eq!(compile_of(SkyShader::Procedural, &programs), Compile::Linear);
    }

    #[test]
    fn a_procedural_sky_reads_its_keyword_and_defaults() {
        let sky = procedural(&material("_SUNDISK_SIMPLE"));
        assert_eq!(sky.sun_disk, SunDisk::Simple);
        assert_eq!(sky.sun_size, 0.04);
        assert_eq!(sky.exposure, 1.3);
        // Half grey in gamma is 0.214 in linear.
        assert!((sky.sky_tint[0] - 0.214).abs() < 1e-3);
        assert_eq!(procedural(&material("")).sun_disk, SunDisk::None);
        assert_eq!(
            procedural(&material("_SUNDISK_HIGH_QUALITY")).sun_disk,
            SunDisk::HighQuality
        );

        let mut m = material("_SUNDISK_NONE");
        m.floats.push(("_Exposure".into(), 2.0));
        m.floats.push(("_SunSize".into(), 0.1));
        m.colors.push(("_GroundColor".into(), [1.0, 0.0, 0.0, 1.0]));
        let sky = procedural(&m);
        assert_eq!((sky.exposure, sky.sun_size), (2.0, 0.1));
        assert_eq!(sky.ground, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_textured_sky_takes_the_constants_of_its_compile() {
        let mut m = material("");
        m.floats.push(("_Rotation".into(), 282.0));
        m.floats.push(("_Exposure".into(), 1.09));
        let linear = look(&m, Compile::Linear);
        assert_eq!((linear.double, linear.exponent), (DOUBLE_LINEAR, true));
        assert_eq!((linear.rotation, linear.exposure), (282.0, 1.09));
        // The default tint times the doubling is white.
        assert!((linear.tint[0] * linear.double - 1.0).abs() < 0.02);
        let gamma = look(&m, Compile::Gamma);
        assert_eq!((gamma.double, gamma.exponent), (DOUBLE_GAMMA, false));
    }

    #[test]
    fn hdr_decoding_follows_the_usage() {
        assert_eq!(hdr_of(0), Some(HDR_PLAIN));
        assert_eq!(hdr_of(2), Some([RGBM_SCALE, RGBM_EXPONENT, 0.0, 1.0]));
        assert_eq!(hdr_of(9), None);
    }

    fn cube_texture(size: u32, mips: u32) -> Texture2D {
        Texture2D {
            name: "sky".into(),
            width: size,
            height: size,
            // RGBA32.
            format: 4,
            mip_count: mips,
            wrap_mode: 1,
            filter_mode: 1,
            color_space: 1,
            lightmap_format: 0,
            data: Vec::new(),
            stream: None,
        }
    }

    #[test]
    fn a_cubemap_splits_into_its_faces() {
        // Two by two faces with a second level of one texel each.
        let mut raw = Vec::new();
        for face in 0..6u8 {
            raw.extend(std::iter::repeat_n(face * 10, 16));
            raw.extend([255; 4]);
        }
        let cube = cube(&cube_texture(2, 2), &raw).expect("splits");
        assert_eq!(cube.size, 2);
        assert!(cube.srgb);
        for (i, face) in cube.faces.iter().enumerate() {
            assert_eq!(face, &vec![i as u8 * 10; 16]);
        }
    }

    #[test]
    fn a_broken_cubemap_is_refused() {
        assert!(cube(&cube_texture(2, 1), &[0; 95]).is_err());
        assert!(cube(&cube_texture(2, 1), &[]).is_err());
        let mut wide = cube_texture(2, 1);
        wide.width = 4;
        assert!(cube(&wide, &[0; 96]).is_err());
    }
}
