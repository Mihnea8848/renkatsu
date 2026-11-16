// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! CharaStudio's screen effects.
//!
//! The Studio camera carries, in this order, Amplify Occlusion,
//! BloomAndFlares, Amplify Color and VignetteAndChromaticAberration. The
//! values here are the ones the Studio scene stores for those components
//! (`CharaStudio_Data/level1`) with what `Studio.SceneInfo.Init` and
//! `SystemButtonCtrl` write over them. The camera is HDR, so everything up
//! to Amplify Color works on unclamped linear colour. The shader side is
//! `effects.wgsl`.

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use rk_chara::Image;
use wgpu::util::DeviceExt;

/// Amplify Occlusion as the Studio scene sets it up: applied as a post
/// effect, medium sample count, normals rebuilt from depth, no distance
/// fade, no downsampling, and one blur pass of radius 4.
#[derive(Debug, Clone, PartialEq)]
pub struct AmbientOcclusion {
    /// `Tint`: the colour fully occluded pixels are multiplied by. The
    /// shader gets it as stored (`Shader.SetGlobalColor` in Unity 5.6).
    pub tint: [f32; 3],
    pub intensity: f32,
    /// `Radius`, in world units.
    pub radius: f32,
    pub power_exponent: f32,
    pub bias: f32,
    pub blur_sharpness: f32,
}

impl Default for AmbientOcclusion {
    fn default() -> Self {
        Self {
            // SceneInfo.Init: Utility.ConvertColor(180, 180, 180).
            tint: [180.0 / 255.0; 3],
            intensity: 1.0,
            // SceneInfo.Init: aoeRadius.
            radius: 0.1,
            power_exponent: 1.0,
            bias: 0.25,
            blur_sharpness: 10.0,
        }
    }
}

/// BloomAndFlares with the scene's Add blend mode, no lens flares and no
/// alpha mask. On the HDR camera the threshold is subtracted as it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Bloom {
    pub intensity: f32,
    pub threshold: f32,
    /// `sepBlurSpread`.
    pub blur_spread: f32,
    /// `bloomBlurIterations`.
    pub iterations: u32,
}

impl Default for Bloom {
    fn default() -> Self {
        // SceneInfo.Init: bloomIntensity, bloomThreshold, bloomBlur.
        Self {
            intensity: 0.4,
            threshold: 0.6,
            blur_spread: 0.8,
            iterations: 2,
        }
    }
}

/// Amplify Color: a 1024 by 32 colour table of 32 blue slices, looked up
/// in gamma space. The scene has no tone mapping and no dithering.
#[derive(Debug, Clone)]
pub struct ColorFilter {
    /// The table, as a data texture (not sRGB).
    pub lut: Arc<Image>,
    pub exposure: f32,
}

/// VignetteAndChromaticAberration, with the scene's zero aberration.
#[derive(Debug, Clone, PartialEq)]
pub struct Vignette {
    pub intensity: f32,
    pub blur: f32,
    pub blur_spread: f32,
}

impl Default for Vignette {
    fn default() -> Self {
        Self {
            intensity: 0.1,
            blur: 0.2,
            blur_spread: 0.75,
        }
    }
}

/// The screen effects of a CharaStudio scene. `None` switches one off.
#[derive(Debug, Clone, Default)]
pub struct StudioEffects {
    pub ao: Option<AmbientOcclusion>,
    pub bloom: Option<Bloom>,
    pub color: Option<ColorFilter>,
    pub vignette: Option<Vignette>,
    /// A plain colour (sRGB) for the parts of the picture where nothing
    /// was drawn, kept out of the effects: the editor's grey when the
    /// scene has no map. `None` puts the effects on the whole picture, as
    /// CharaStudio does.
    pub backdrop: Option<[f64; 4]>,
}

impl StudioEffects {
    /// What a new Studio scene has on: occlusion, bloom, the colour table
    /// of filter 0 when the caller found it, and the vignette.
    pub fn studio(lut: Option<Arc<Image>>) -> Self {
        Self {
            ao: Some(AmbientOcclusion::default()),
            bloom: Some(Bloom::default()),
            color: lut.map(|lut| ColorFilter { lut, exposure: 1.0 }),
            vignette: Some(Vignette::default()),
            backdrop: None,
        }
    }
}

/// The first values of the game's `RandomTable.Values`, which Amplify
/// Occlusion builds its 4 by 4 random texture from.
const RANDOM_TABLE: [f32; 32] = [
    0.4639, 0.34, 0.223, 0.4684, 0.3222, 0.9792, 0.0317, 0.9733, 0.7783, 0.4561, 0.2585, 0.33, 0.3873, 0.3801, 0.1798,
    0.9107, 0.5116, 0.0929, 0.1807, 0.6201, 0.1013, 0.5563, 0.6424, 0.442, 0.2151, 0.4752, 0.1573, 0.5688, 0.5012,
    0.6292, 0.6992, 0.7077,
];

/// The random texture's pixels (`GenerateRandomizationData`): the cosine
/// and sine of an angle within an eighth of a turn, and a ray offset. The
/// texture is ARGB32, so each value is rounded to eight bits.
pub fn random_data() -> [[f32; 4]; 16] {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5).floor() / 255.0;
    std::array::from_fn(|i| {
        let angle = std::f32::consts::TAU * RANDOM_TABLE[i * 2] / 8.0;
        [byte(angle.cos()), byte(angle.sin()), byte(RANDOM_TABLE[i * 2 + 1]), 0.0]
    })
}

/// `_AO_UVToView` and `_AO_HalfProjScale` (`UpdateGlobalShaderConstants`).
pub fn ao_projection(fov_y_deg: f32, width: u32, height: u32) -> ([f32; 4], f32) {
    let tan = (fov_y_deg.to_radians() * 0.5).tan();
    let inv = [tan * width as f32 / height as f32, tan];
    let proj_scale = height as f32 / (tan * 2.0);
    ([2.0 * inv[0], -2.0 * inv[1], -inv[0], inv[1]], 0.5 * proj_scale)
}

/// What the picture is multiplied by where the occlusion term is
/// `occlusion` (1 is open, 0 fully occluded).
pub fn ao_factor(ao: &AmbientOcclusion, occlusion: f32) -> [f32; 3] {
    ao.tint
        .map(|t| 1.0 + ao.intensity * ((t + (1.0 - t) * occlusion) - 1.0))
}

/// Weight of a blur tap `k` pixels from the centre, for eye depths
/// `centre` and `tap`.
pub fn ao_blur_weight(k: i32, centre: f32, tap: f32, sharpness: f32) -> f32 {
    let sharp = ((1.0 - centre).clamp(0.0, 1.0) * 100.0 + 1.0) * sharpness;
    let dd = (centre - tap) * sharp;
    (-dd * dd - (k * k) as f32 * 0.125).exp()
}

/// Tap weights of the bloom blur (Hidden/SeparableBlurPlus): the centre,
/// then the pairs at 1, 2, 3 and 6.5 steps.
pub const BLOOM_BLUR: [(f32, f32); 5] = [(0.0, 0.225), (1.0, 0.15), (2.0, 0.11), (3.0, 0.075), (6.5, 0.0525)];
/// Tap weights of the vignette blur (Hidden/SeparableBlur).
pub const VIGNETTE_BLUR: [(f32, f32); 4] = [(0.0, 0.4), (1.0, 0.15), (2.0, 0.1), (3.0, 0.05)];

/// The vertical and horizontal step of bloom blur iteration `i`, in UV.
pub fn bloom_steps(bloom: &Bloom, i: u32, aspect: f32) -> [[f32; 2]; 2] {
    let spread = (1.0 + i as f32 * 0.5) * bloom.blur_spread;
    let one = 2.0 / 1024.0;
    [[0.0, spread * one], [spread / aspect * one, 0.0]]
}

/// The vertical and horizontal step of the vignette blur, in UV.
pub fn vignette_steps(v: &Vignette, aspect: f32) -> [[f32; 2]; 2] {
    let one = v.blur_spread * (2.0 / 1024.0);
    [[0.0, one], [one / aspect, 0.0]]
}

/// `_Intensity` and `_Blur` as the vignette material gets them.
pub fn vignette_params(v: &Vignette) -> [f32; 2] {
    [1.0 / (1.0 - v.intensity) - 1.0, 1.0 / (1.0 - v.blur) - 1.0]
}

/// How far the picture fades to its blurred copy and what it is multiplied
/// by, at `uv`.
pub fn vignette_at(v: &Vignette, uv: [f32; 2]) -> (f32, f32) {
    let [intensity, blur] = vignette_params(v);
    let c = [(uv[0] - 0.5) * 2.0, (uv[1] - 0.5) * 2.0];
    let d = c[0] * c[0] + c[1] * c[1];
    ((d * blur).clamp(0.0, 1.0), 1.0 - d * intensity)
}

/// Amplify Color's linear to gamma step.
pub fn to_gamma(x: f32) -> f32 {
    (x.max(0.0).powf(0.416_666_66) * 1.055 - 0.055).clamp(0.0, 1.0)
}

/// Amplify Color's gamma to linear step.
pub fn to_linear(t: f32) -> f32 {
    t * (t * (t * 0.305_306_02 + 0.682_171_1) + 0.012_522_878)
}

/// The two table positions a gamma colour is read at, and the weight of
/// the second. V counts from the image's bottom row.
pub fn lut_coords(g: [f32; 3]) -> ([f32; 2], [f32; 2], f32) {
    let blue = g[2] * 31.0;
    let f = blue.fract();
    let slice = blue - f;
    let u = g[0] * (31.0 / 1024.0) + 0.5 / 1024.0 + slice / 32.0;
    let v = g[1] * (31.0 / 32.0) + 0.5 / 32.0;
    ([u, v], [u + 1.0 / 32.0, v], f)
}

fn bilinear(image: &Image, uv: [f32; 2]) -> [f32; 3] {
    let (w, h) = (image.width as i64, image.height as i64);
    let x = uv[0] * w as f32 - 0.5;
    let y = uv[1] * h as f32 - 0.5;
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |x: i64, y: i64| {
        let i = ((y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) * 4) as usize;
        [0, 1, 2].map(|c| image.rgba[i + c] as f32 / 255.0)
    };
    let (x0, y0) = (x0 as i64, y0 as i64);
    let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
    [0, 1, 2].map(|i| (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy)
}

/// A linear colour through the colour table, as the shader does it.
pub fn filter_color(filter: &ColorFilter, linear: [f32; 3]) -> [f32; 3] {
    let g = linear.map(|c| to_gamma(c * filter.exposure));
    let (a, b, f) = lut_coords(g);
    let (a, b) = (bilinear(&filter.lut, a), bilinear(&filter.lut, b));
    [0, 1, 2].map(|i| to_linear(a[i] + (b[i] - a[i]) * f))
}

/// `Fx` in `effects.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FxUniform {
    size: [f32; 4],
    src: [f32; 4],
    ao: [f32; 4],
    ao2: [f32; 4],
    levels: [f32; 4],
    uv_to_view: [f32; 4],
    bloom: [f32; 4],
    vignette: [f32; 4],
    rand: [[f32; 4]; 16],
}

/// What the effects need to know about a frame.
pub(crate) struct Frame<'a> {
    pub fx: &'a StudioEffects,
    pub width: u32,
    pub height: u32,
    pub fov_y_deg: f32,
    pub near: f32,
    pub far: f32,
    /// The camera's depth texture.
    pub depth: &'a wgpu::TextureView,
    /// The depth aspect of the main pass's multisampled depth, which tells
    /// the backdrop where something was drawn.
    pub scene_depth: &'a wgpu::TextureView,
    pub white: &'a wgpu::TextureView,
    /// The colour table, uploaded.
    pub lut: Option<&'a wgpu::TextureView>,
}

impl Frame<'_> {
    fn uniform(&self) -> FxUniform {
        let (w, h) = (self.width as f32, self.height as f32);
        let (uv_to_view, half_proj_scale) = ao_projection(self.fov_y_deg, self.width, self.height);
        let ao = self.fx.ao.clone().unwrap_or_default();
        let bias = ao.bias.clamp(0.0, 1.0);
        let bloom = self.fx.bloom.as_ref();
        FxUniform {
            size: [w, h, 1.0 / w, 1.0 / h],
            src: [0.0; 4],
            ao: [ao.radius, half_proj_scale, ao.power_exponent, bias],
            ao2: [1.0 / (1.0 - bias), ao.blur_sharpness, self.near, self.far],
            levels: [ao.tint[0], ao.tint[1], ao.tint[2], ao.intensity],
            uv_to_view,
            bloom: [
                bloom.map_or(0.0, |b| b.threshold),
                bloom.map_or(0.0, |b| b.intensity),
                self.fx.color.as_ref().map_or(1.0, |c| c.exposure),
                if self.lut.is_some() && self.fx.color.is_some() {
                    1.0
                } else {
                    0.0
                },
            ],
            vignette: {
                let [i, b] = self.fx.vignette.as_ref().map_or([0.0; 2], vignette_params);
                [i, b, 0.0, 0.0]
            },
            rand: random_data(),
        }
    }
}

/// The camera renders in half floats.
pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Occlusion and eye depth (`RenderTextureFormat.RGHalf`).
const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

struct Targets {
    size: [u32; 2],
    hdr_msaa: wgpu::TextureView,
    hdr: wgpu::TextureView,
    ao: [wgpu::TextureView; 2],
    half: wgpu::TextureView,
    quarter: [wgpu::TextureView; 3],
    ldr: wgpu::TextureView,
    ldr_half: [wgpu::TextureView; 2],
    /// The finished effects, before the backdrop goes under them.
    done: wgpu::TextureView,
}

/// The effects' GPU side, made the first time a frame asks for effects.
pub(crate) struct Gpu {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipelines: HashMap<(&'static str, wgpu::TextureFormat), wgpu::RenderPipeline>,
    ao_apply: wgpu::RenderPipeline,
    backdrop_layout: wgpu::BindGroupLayout,
    backdrop: wgpu::RenderPipeline,
    targets: Option<Targets>,
}

impl Gpu {
    pub fn new(
        device: &wgpu::Device,
        ldr_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects"),
            source: wgpu::ShaderSource::Wgsl(include_str!("effects.wgsl").into()),
        });
        let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let float = wgpu::TextureSampleType::Float { filterable: true };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(1, float),
                texture(2, float),
                texture(3, wgpu::TextureSampleType::Depth),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture(5, float),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let make = |entry: &'static str,
                    target: wgpu::ColorTargetState,
                    depth_stencil: Option<wgpu::DepthStencilState>,
                    count: u32| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_fx"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(target)],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil,
                multisample: wgpu::MultisampleState {
                    count,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        let mut pipelines = HashMap::new();
        for (entry, format) in [
            ("fs_ao", AO_FORMAT),
            ("fs_ao_blur", AO_FORMAT),
            ("fs_down4", HDR_FORMAT),
            ("fs_bright", HDR_FORMAT),
            ("fs_blur_plus", HDR_FORMAT),
            ("fs_color", ldr_format),
            ("fs_down4", ldr_format),
            ("fs_blur", ldr_format),
            ("fs_vignette", ldr_format),
        ] {
            pipelines.insert((entry, format), make(entry, format.into(), None, 1));
        }
        // Blend DstColor Zero, drawn into the camera's multisampled target
        // between the solid and the later queues.
        let multiply = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Dst,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        };
        let multiply_alpha = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::DstAlpha,
            ..multiply
        };
        let ao_apply = make(
            "fs_ao_apply",
            wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: Some(wgpu::BlendState {
                    color: multiply,
                    alpha: multiply_alpha,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            },
            Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            samples,
        );
        let backdrop_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("backdrop"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(1, float),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: true,
                    },
                    count: None,
                },
            ],
        });
        let backdrop_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("backdrop"),
            source: wgpu::ShaderSource::Wgsl(include_str!("backdrop.wgsl").into()),
        });
        let backdrop = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("backdrop"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("backdrop"),
                bind_group_layouts: &[Some(&backdrop_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &backdrop_shader,
                entry_point: Some("vs_backdrop"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &backdrop_shader,
                entry_point: Some("fs_backdrop"),
                targets: &[Some(ldr_format.into())],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effects"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            layout,
            sampler,
            pipelines,
            ao_apply,
            backdrop_layout,
            backdrop,
            targets: None,
        }
    }

    /// Makes the frame's targets when the size changed, and returns the
    /// multisampled HDR target with the texture it resolves to.
    pub fn hdr_targets(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        samples: u32,
        ldr_format: wgpu::TextureFormat,
    ) -> (wgpu::TextureView, wgpu::TextureView) {
        if self.targets.as_ref().is_none_or(|t| t.size != [width, height]) {
            let make = |w: u32, h: u32, format, count| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("effects"),
                        size: wgpu::Extent3d {
                            width: w.max(1),
                            height: h.max(1),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: count,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: if count > 1 {
                            wgpu::TextureUsages::RENDER_ATTACHMENT
                        } else {
                            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING
                        },
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            // BloomAndFlares and the vignette size their buffers by integer
            // division of the frame's.
            let (hw, hh, qw, qh) = (width / 2, height / 2, width / 4, height / 4);
            self.targets = Some(Targets {
                size: [width, height],
                hdr_msaa: make(width, height, HDR_FORMAT, samples),
                hdr: make(width, height, HDR_FORMAT, 1),
                ao: std::array::from_fn(|_| make(width, height, AO_FORMAT, 1)),
                half: make(hw, hh, HDR_FORMAT, 1),
                quarter: std::array::from_fn(|_| make(qw, qh, HDR_FORMAT, 1)),
                ldr: make(width, height, ldr_format, 1),
                ldr_half: std::array::from_fn(|_| make(hw, hh, ldr_format, 1)),
                done: make(width, height, ldr_format, 1),
            });
        }
        let t = self.targets.as_ref().expect("made above");
        (t.hdr_msaa.clone(), t.hdr.clone())
    }

    fn group(
        &self,
        device: &wgpu::Device,
        f: &Frame,
        uniform: FxUniform,
        a: &wgpu::TextureView,
        b: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("effects"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let view = wgpu::BindingResource::TextureView;
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: view(a),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: view(b),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: view(f.depth),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: view(f.lut.unwrap_or(f.white)),
                },
            ],
        })
    }

    /// One full-screen pass of `entry` into `target`.
    #[allow(clippy::too_many_arguments)]
    fn pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        f: &Frame,
        entry: &'static str,
        format: wgpu::TextureFormat,
        target: &wgpu::TextureView,
        uniform: FxUniform,
        a: &wgpu::TextureView,
        b: &wgpu::TextureView,
    ) {
        let group = self.group(device, f, uniform, a, b);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(entry),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipelines[&(entry, format)]);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Computes and blurs the occlusion from the depth texture. Returns
    /// the pipeline and bind group that multiply it into the camera's
    /// target; `None` when occlusion is off.
    pub fn occlusion(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        f: &Frame,
    ) -> Option<(&wgpu::RenderPipeline, wgpu::BindGroup)> {
        f.fx.ao.as_ref()?;
        let t = self.targets.as_ref()?;
        let base = f.uniform();
        self.pass(device, encoder, f, "fs_ao", AO_FORMAT, &t.ao[0], base, f.white, f.white);
        // BlurPasses is 1: once across, once down.
        let across = FxUniform {
            src: [0.0, 0.0, 1.0, 0.0],
            ..base
        };
        let down = FxUniform {
            src: [0.0, 0.0, 0.0, 1.0],
            ..base
        };
        self.pass(
            device,
            encoder,
            f,
            "fs_ao_blur",
            AO_FORMAT,
            &t.ao[1],
            across,
            &t.ao[0],
            f.white,
        );
        self.pass(
            device,
            encoder,
            f,
            "fs_ao_blur",
            AO_FORMAT,
            &t.ao[0],
            down,
            &t.ao[1],
            f.white,
        );
        Some((&self.ao_apply, self.group(device, f, base, &t.ao[0], f.white)))
    }

    /// Runs bloom, the colour table and the vignette on the resolved HDR
    /// picture and writes the result to `target`.
    pub fn finish(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        f: &Frame,
        ldr_format: wgpu::TextureFormat,
        target: &wgpu::TextureView,
    ) {
        let Some(t) = self.targets.as_ref() else { return };
        // With a backdrop the effects finish in `done`, and the backdrop
        // pass then writes the target.
        let out = if f.fx.backdrop.is_some() { &t.done } else { target };
        let base = f.uniform();
        let (w, h) = (f.width as f32, f.height as f32);
        let aspect = w / h;
        let texel = |w: u32, h: u32| [1.0 / w.max(1) as f32, 1.0 / h.max(1) as f32];
        let with = |texel: [f32; 2], step: [f32; 2]| FxUniform {
            src: [texel[0], texel[1], step[0], step[1]],
            ..base
        };
        let full = texel(f.width, f.height);
        let half = texel(f.width / 2, f.height / 2);

        let bloom = if let Some(bloom) = &f.fx.bloom {
            let q = &t.quarter;
            self.pass(
                device,
                encoder,
                f,
                "fs_down4",
                HDR_FORMAT,
                &t.half,
                with(full, [0.0; 2]),
                &t.hdr,
                f.white,
            );
            self.pass(
                device,
                encoder,
                f,
                "fs_down4",
                HDR_FORMAT,
                &q[0],
                with(half, [0.0; 2]),
                &t.half,
                f.white,
            );
            self.pass(device, encoder, f, "fs_bright", HDR_FORMAT, &q[1], base, &q[0], f.white);
            let mut source = 1;
            for i in 0..bloom.iterations.max(1) {
                let [down, across] = bloom_steps(bloom, i, aspect);
                self.pass(
                    device,
                    encoder,
                    f,
                    "fs_blur_plus",
                    HDR_FORMAT,
                    &q[2],
                    with(half, down),
                    &q[source],
                    f.white,
                );
                self.pass(
                    device,
                    encoder,
                    f,
                    "fs_blur_plus",
                    HDR_FORMAT,
                    &q[0],
                    with(half, across),
                    &q[2],
                    f.white,
                );
                source = 0;
            }
            &q[0]
        } else {
            // No bloom: the uniform's intensity is zero.
            f.white
        };

        let Some(vignette) = &f.fx.vignette else {
            self.pass(device, encoder, f, "fs_color", ldr_format, out, base, &t.hdr, bloom);
            self.under(device, encoder, f, target);
            return;
        };
        self.pass(device, encoder, f, "fs_color", ldr_format, &t.ldr, base, &t.hdr, bloom);
        let [a, b] = &t.ldr_half;
        self.pass(
            device,
            encoder,
            f,
            "fs_down4",
            ldr_format,
            a,
            with(full, [0.0; 2]),
            &t.ldr,
            f.white,
        );
        let [down, across] = vignette_steps(vignette, aspect);
        for _ in 0..2 {
            self.pass(
                device,
                encoder,
                f,
                "fs_blur",
                ldr_format,
                b,
                with(half, down),
                a,
                f.white,
            );
            self.pass(
                device,
                encoder,
                f,
                "fs_blur",
                ldr_format,
                a,
                with(half, across),
                b,
                f.white,
            );
        }
        self.pass(device, encoder, f, "fs_vignette", ldr_format, out, base, &t.ldr, a);
        self.under(device, encoder, f, target);
    }

    /// Puts the backdrop under the finished effects, when the frame has one.
    fn under(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, f: &Frame, target: &wgpu::TextureView) {
        let (Some(colour), Some(t)) = (f.fx.backdrop, self.targets.as_ref()) else {
            return;
        };
        let linear = backdrop_linear(colour);
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("backdrop"),
            contents: bytemuck::bytes_of(&linear),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("backdrop"),
            layout: &self.backdrop_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&t.done),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(f.scene_depth),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("backdrop"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.backdrop);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// A backdrop colour as the shader takes it: linear, since the target
/// stores sRGB.
fn backdrop_linear(c: [f64; 4]) -> [f32; 4] {
    let lin = |x: f64| {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    [lin(c[0]) as f32, lin(c[1]) as f32, lin(c[2]) as f32, c[3] as f32]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backdrop_shader_is_valid_wgsl_and_its_colour_is_linear() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("backdrop.wgsl")).expect("parses");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("validates");
        let grey = backdrop_linear([0.22, 0.22, 0.23, 1.0]);
        assert!(
            (grey[0] - 0.0395).abs() < 0.001 && (grey[3] - 1.0).abs() < 1e-6,
            "{grey:?}"
        );
        assert_eq!(backdrop_linear([0.0, 1.0, 2.0, 1.0])[..3], [0.0, 1.0, 1.0]);
    }

    #[test]
    fn effects_shader_is_valid_wgsl() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("effects.wgsl")).expect("parses");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validates");
    }

    #[test]
    fn uniform_matches_the_shader() {
        // Eight vectors and the sixteen random pixels.
        assert_eq!(std::mem::size_of::<FxUniform>(), (8 + 16) * 16);
    }

    #[test]
    fn studio_defaults_are_the_scene_values() {
        let fx = StudioEffects::studio(None);
        let ao = fx.ao.expect("on");
        assert!((ao.tint[0] - 0.705_882_4).abs() < 1e-6);
        assert_eq!((ao.radius, ao.power_exponent, ao.bias), (0.1, 1.0, 0.25));
        let bloom = fx.bloom.expect("on");
        assert_eq!(
            (bloom.intensity, bloom.threshold, bloom.blur_spread, bloom.iterations),
            (0.4, 0.6, 0.8, 2)
        );
        assert!(fx.color.is_none());
        assert_eq!(
            fx.vignette.expect("on"),
            Vignette {
                intensity: 0.1,
                blur: 0.2,
                blur_spread: 0.75
            }
        );
    }

    #[test]
    fn random_pixels_are_unit_directions_in_eight_bits() {
        let data = random_data();
        // The first: angle 2 pi * 0.4639 / 8, offset 0.34.
        let angle = std::f32::consts::TAU * 0.4639 / 8.0;
        assert!((data[0][0] - angle.cos()).abs() < 0.5 / 255.0 + 1e-6);
        assert!((data[0][1] - angle.sin()).abs() < 0.5 / 255.0 + 1e-6);
        assert!((data[0][2] - 87.0 / 255.0).abs() < 1e-6);
        for p in data {
            assert!((p[0] * 255.0).fract().min(1.0 - (p[0] * 255.0).fract()) < 1e-3);
            assert!(p[0] > 0.7 && p[1] >= 0.0 && p[1] < 0.71);
        }
    }

    #[test]
    fn occlusion_projection_follows_the_camera() {
        let (uv_to_view, half) = ao_projection(23.0, 900, 1400);
        let tan = (11.5f32).to_radians().tan();
        assert!((uv_to_view[1] + 2.0 * tan).abs() < 1e-6);
        assert!((uv_to_view[3] - tan).abs() < 1e-6);
        assert!((uv_to_view[0] - 2.0 * tan * 900.0 / 1400.0).abs() < 1e-6);
        assert!((half - 1400.0 / (4.0 * tan)).abs() < 1e-2);
    }

    #[test]
    fn occlusion_multiplies_by_the_tint() {
        let ao = AmbientOcclusion::default();
        assert_eq!(ao_factor(&ao, 1.0), [1.0; 3]);
        assert!((ao_factor(&ao, 0.0)[0] - 180.0 / 255.0).abs() < 1e-6);
        let half = ao_factor(&ao, 0.5)[1];
        assert!((half - (1.0 + 180.0 / 255.0) / 2.0).abs() < 1e-6);
    }

    #[test]
    fn occlusion_blur_stops_at_depth_edges() {
        // Same depth: the Gaussian alone, sigma 2.
        assert!((ao_blur_weight(2, 3.0, 3.0, 10.0) - (-0.5f32).exp()).abs() < 1e-6);
        assert!((ao_blur_weight(4, 3.0, 3.0, 10.0) - (-2.0f32).exp()).abs() < 1e-6);
        // Half a unit behind: nothing.
        assert!(ao_blur_weight(1, 3.0, 3.5, 10.0) < 1e-9);
        // Closer than one unit the edge gets sharper.
        assert!(ao_blur_weight(1, 0.5, 0.51, 10.0) < ao_blur_weight(1, 3.0, 3.01, 10.0));
    }

    #[test]
    fn blur_weights_add_up_to_one() {
        for taps in [&BLOOM_BLUR[..], &VIGNETTE_BLUR[..]] {
            let sum: f32 = taps.iter().map(|(o, w)| if *o == 0.0 { *w } else { 2.0 * w }).sum();
            assert!((sum - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn bloom_blur_widens_with_each_iteration() {
        let bloom = Bloom::default();
        let aspect = 900.0 / 1400.0;
        let [down, across] = bloom_steps(&bloom, 0, aspect);
        assert_eq!(down, [0.0, 0.8 * 2.0 / 1024.0]);
        assert!((across[0] - 0.8 / aspect * 2.0 / 1024.0).abs() < 1e-9);
        let [down, _] = bloom_steps(&bloom, 1, aspect);
        assert!((down[1] - 1.2 * 2.0 / 1024.0).abs() < 1e-9);
    }

    #[test]
    fn vignette_darkens_and_blurs_towards_the_corners() {
        let v = Vignette::default();
        let [intensity, blur] = vignette_params(&v);
        assert!((intensity - 1.0 / 9.0).abs() < 1e-6);
        assert!((blur - 0.25).abs() < 1e-6);
        assert_eq!(vignette_at(&v, [0.5, 0.5]), (0.0, 1.0));
        let (soft, mask) = vignette_at(&v, [0.0, 0.0]);
        assert!((soft - 0.5).abs() < 1e-6);
        assert!((mask - (1.0 - 2.0 / 9.0)).abs() < 1e-6);
        let (soft, mask) = vignette_at(&v, [1.0, 0.5]);
        assert!((soft - 0.25).abs() < 1e-6);
        assert!((mask - (1.0 - 1.0 / 9.0)).abs() < 1e-6);
    }

    /// Amplify Color's neutral table (`CreateDefaultLut`).
    fn neutral_lut() -> Image {
        let mut rgba = vec![0u8; 1024 * 32 * 4];
        for b in 0..32 {
            for g in 0..32 {
                for r in 0..32 {
                    let i = (b * 32 + g * 1024 + r) * 4;
                    let byte = |k: usize| (k as f32 / 31.0 * 255.0) as u8;
                    rgba[i..i + 4].copy_from_slice(&[byte(r), byte(g), byte(b), 255]);
                }
            }
        }
        Image {
            name: "DefaultLut".into(),
            width: 1024,
            height: 32,
            rgba,
            srgb: false,
            repeat: false,
        }
    }

    #[test]
    fn table_positions_split_blue_into_slices() {
        let (a, b, f) = lut_coords([0.0, 0.0, 0.0]);
        assert_eq!((a, f), ([0.5 / 1024.0, 0.5 / 32.0], 0.0));
        assert!((b[0] - a[0] - 1.0 / 32.0).abs() < 1e-9);
        // Full red and green land on the last texel centre of the slice.
        let (a, _, f) = lut_coords([1.0, 1.0, 1.0]);
        assert!((a[0] - (31.0 * 32.0 + 31.5) / 1024.0).abs() < 1e-6);
        assert!((a[1] - 31.5 / 32.0).abs() < 1e-6);
        assert_eq!(f, 0.0);
        // Blue between two slices blends them.
        let (a, _, f) = lut_coords([0.0, 0.0, 10.5 / 31.0]);
        assert!((a[0] - (10.0 * 32.0 + 0.5) / 1024.0).abs() < 1e-6);
        assert!((f - 0.5).abs() < 1e-4);
    }

    #[test]
    fn gamma_steps_undo_each_other() {
        for x in [0.02f32, 0.2, 0.5, 0.85, 1.0] {
            assert!((to_linear(to_gamma(x)) - x).abs() < 4e-3, "{x}");
        }
        assert_eq!(to_gamma(-1.0), 0.0);
        assert_eq!(to_gamma(7.0), 1.0);
    }

    #[test]
    fn neutral_table_keeps_colours() {
        let filter = ColorFilter {
            lut: Arc::new(neutral_lut()),
            exposure: 1.0,
        };
        for c in [[0.1f32, 0.5, 0.9], [0.8, 0.8, 0.85], [0.3, 0.02, 0.6]] {
            let out = filter_color(&filter, c);
            for i in 0..3 {
                // The table's bytes are cut, not rounded: up to 1/255 low.
                assert!((out[i] - c[i]).abs() < 0.012, "{c:?} gave {out:?}");
            }
        }
        // HDR values clip at white.
        let out = filter_color(&filter, [4.0, 4.0, 4.0]);
        assert!(out.iter().all(|v| (v - 1.0).abs() < 1e-3));
    }
}
