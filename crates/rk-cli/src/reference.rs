// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Compares our render with a picture captured inside the game by the plugin
//! in `tools/reference-capture`, using the camera and light the capture
//! recorded.

use anyhow::{bail, Context, Result};
use glam::{Quat, Vec3};
use rk_card::Card;
use rk_chara::BuildOptions;
use rk_core::Config;
use rk_render::shadow::ShadowSettings;
use rk_render::{load_png, save_png, Camera, Environment, Renderer, StudioEffects};
use rk_vfs::Game;
use serde_json::Value;
use std::path::Path;

const GAP: u32 = 6;

fn floats<const N: usize>(v: &Value) -> Result<[f32; N]> {
    let list = v.as_array().context("expected a list of numbers")?;
    if list.len() != N {
        bail!("expected {N} numbers, found {}", list.len());
    }
    let mut out = [0.0; N];
    for (o, x) in out.iter_mut().zip(list) {
        *o = x.as_f64().context("expected a number")? as f32;
    }
    Ok(out)
}

/// What a capture file says about the camera and the light.
pub struct Capture {
    pub width: u32,
    pub height: u32,
    /// One camera per captured picture, with the view's name.
    pub views: Vec<(String, Camera)>,
    pub background: [f32; 4],
    /// From the scene towards the first directional light.
    pub light_dir: Option<[f32; 3]>,
    pub light_intensity: f32,
    /// Whether that light casts shadows, and how strong they are; `None`
    /// where the capture does not say.
    pub light_shadows: Option<bool>,
    /// Whether they are soft (`LightShadows.Soft`).
    pub light_soft: Option<bool>,
    pub shadow_strength: Option<f32>,
    /// Number of the shading ramp the scene used.
    pub ramp: Option<i32>,
    /// Scene shadow colour, and how deep shadows are (0 to 1).
    pub shadow: Option<([f32; 3], f32)>,
    /// How dark and how wide lines are.
    pub line_depth: Option<f32>,
    pub line_width: Option<f32>,
    /// `RenderSettings.ambientLight` of the scene.
    pub ambient_light: Option<[f32; 3]>,
    /// `RenderSettings.ambientProbe`: nine coefficients per colour channel.
    /// Captures made before the plugin wrote it have none.
    pub ambient_probe: Option<[f32; 27]>,
    /// The scene's screen effects, without the colour table.
    pub effects: StudioEffects,
    /// Number of the scene's colour filter (`ace_no`).
    pub color_filter: Option<i32>,
    /// From the scene towards the character's light while each view was
    /// taken, where the capture says; it replaces `light_dir` for that view.
    pub view_lights: Vec<Option<[f32; 3]>>,
    /// What the capture list asked of the character.
    pub case: Case,
}

/// An animation of CharaStudio's list held at a time, in passes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CasePose {
    pub group: i32,
    pub category: i32,
    pub number: i32,
    pub time: f32,
}

/// Where a capture stands in a played sequence: `frame` captures after the
/// held one, `step` frames of `1 / rate` seconds apart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaseSequence {
    pub frame: u32,
    pub step: u32,
    pub rate: f32,
}

/// What a line of the capture list set on the character. Captures made
/// before the plugin wrote it have everything on and no pose.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Case {
    /// The clothing states the game ended up with (`status.clothes_state`).
    pub clothes: rk_chara::clothes_state::States,
    pub pose: Option<CasePose>,
    pub sequence: Option<CaseSequence>,
    /// The Animator's normalized time when the picture was taken.
    pub normalized_time: Option<f32>,
    /// The scene file name of the map the capture was taken in.
    pub map: Option<String>,
}

impl Case {
    fn parse(v: &Value) -> Self {
        let mut case = Case::default();
        let states = v["status"]["clothes_state"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        for (slot, state) in case.clothes.iter_mut().zip(states) {
            *slot = state.as_u64().unwrap_or(0).min(3) as u8;
        }
        let asked = &v["case"];
        let int = |v: &Value| v.as_i64().map(|n| n as i32);
        let pose = &asked["pose"];
        if let (Some(group), Some(category), Some(number)) =
            (int(&pose["group"]), int(&pose["category"]), int(&pose["number"]))
        {
            case.pose = Some(CasePose {
                group,
                category,
                number,
                time: pose["time"].as_f64().unwrap_or(0.0) as f32,
            });
        }
        let sequence = &asked["sequence"];
        if let Some(frame) = sequence["frame"].as_u64() {
            case.sequence = Some(CaseSequence {
                frame: frame as u32,
                step: sequence["step"].as_u64().unwrap_or(1).max(1) as u32,
                rate: sequence["rate"].as_f64().filter(|r| *r > 0.0).unwrap_or(30.0) as f32,
            });
        }
        case.normalized_time = v["animation"]["normalized_time"].as_f64().map(|n| n as f32);
        case.map = asked["map"].as_str().map(str::to_owned);
        case
    }
}

/// The screen effects a capture's scene had on. The switches come from
/// `studio`; `effect_params`, which newer captures carry, lists each effect
/// component's public fields and replaces the Studio scene's stored values.
fn capture_effects(v: &Value) -> StudioEffects {
    let studio = &v["studio"];
    let on = |key: &str| studio[key].as_bool().unwrap_or(false);
    let mut fx = StudioEffects {
        ao: on("ao").then(Default::default),
        bloom: on("bloom").then(Default::default),
        color: None,
        vignette: on("vignette").then(Default::default),
        fog: None,
        shafts: None,
        dof: None,
        backdrop: None,
    };
    for component in v["effect_params"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let fields = &component["fields"];
        let float = |key: &str, slot: &mut f32| {
            if let Some(n) = fields[key].as_f64() {
                *slot = n as f32;
            }
        };
        let kind = component["type"].as_str().unwrap_or_default();
        if kind.ends_with("AmplifyOcclusionEffect") {
            if let Some(ao) = &mut fx.ao {
                if let Ok(tint) = floats::<4>(&fields["Tint"]) {
                    ao.tint = [tint[0], tint[1], tint[2]];
                }
                float("Intensity", &mut ao.intensity);
                float("Radius", &mut ao.radius);
                float("PowerExponent", &mut ao.power_exponent);
                float("Bias", &mut ao.bias);
                float("BlurSharpness", &mut ao.blur_sharpness);
            }
        } else if kind.ends_with("BloomAndFlares") {
            if let Some(bloom) = &mut fx.bloom {
                float("bloomIntensity", &mut bloom.intensity);
                float("bloomThreshold", &mut bloom.threshold);
                float("sepBlurSpread", &mut bloom.blur_spread);
                if let Some(n) = fields["bloomBlurIterations"].as_u64() {
                    bloom.iterations = n as u32;
                }
            }
        } else if kind.ends_with("VignetteAndChromaticAberration") {
            if let Some(vignette) = &mut fx.vignette {
                float("intensity", &mut vignette.intensity);
                float("blur", &mut vignette.blur);
                float("blurSpread", &mut vignette.blur_spread);
            }
        }
    }
    fx
}

impl Capture {
    pub fn parse(text: &str) -> Result<Self> {
        let v: Value = serde_json::from_str(text)?;
        let size: [f32; 2] = floats(&v["size"])?;
        let cam = &v["camera"];
        let fov_y_deg = cam["fov"].as_f64().context("camera fov")? as f32;
        let mut views = Vec::new();
        let mut view_lights = Vec::new();
        for view in v["views"].as_array().context("views")? {
            let eye = Vec3::from(floats::<3>(&view["position"])?);
            let rot = Quat::from_array(floats::<4>(&view["rotation"])?);
            let camera = Camera {
                eye,
                target: eye + rot * Vec3::Z,
                // A view with its own opening angle says so.
                fov_y_deg: view["fov"].as_f64().map_or(fov_y_deg, |f| f as f32),
            };
            views.push((view["name"].as_str().context("view name")?.to_owned(), camera));
            view_lights.push(
                floats::<3>(&view["light_direction"])
                    .ok()
                    .map(|d| (-Vec3::from(d)).to_array()),
            );
        }
        let mut light_dir = None;
        let mut light_intensity = 1.0;
        let mut light_shadows = None;
        let mut light_soft = None;
        let mut shadow_strength = None;
        for l in v["lights"].as_array().map(Vec::as_slice).unwrap_or_default() {
            if l["type"] == "Directional" {
                let d = Vec3::from(floats::<3>(&l["direction"])?);
                light_dir = Some((-d).to_array());
                light_intensity = l["intensity"].as_f64().unwrap_or(1.0) as f32;
                light_shadows = l["shadows"].as_str().map(|mode| mode != "None");
                light_soft = l["shadows"].as_str().map(|mode| mode == "Soft");
                shadow_strength = l["shadow_strength"].as_f64().map(|n| n as f32);
                break;
            }
        }
        Ok(Self {
            width: size[0] as u32,
            height: size[1] as u32,
            views,
            background: floats(&cam["background"])?,
            light_dir,
            light_intensity,
            light_shadows,
            light_soft,
            shadow_strength,
            ramp: v["studio"]["ramp_g"].as_i64().map(|n| n as i32),
            // The game stores depth as one minus the value in alpha.
            shadow: floats::<4>(&v["globals"]["_ambientshadowG"])
                .ok()
                .map(|c| ([c[0], c[1], c[2]], 1.0 - c[3])),
            line_depth: floats::<4>(&v["globals"]["_LineColorG"]).ok().map(|c| 1.0 - c[3]),
            line_width: v["globals"]["_linewidthG"].as_f64().map(|n| n as f32),
            ambient_light: floats::<4>(&v["globals"]["ambient_light"])
                .ok()
                .map(|c| [c[0], c[1], c[2]]),
            ambient_probe: floats::<27>(&v["globals"]["ambient_probe"]).ok(),
            effects: capture_effects(&v),
            color_filter: v["studio"]["ace_no"].as_i64().map(|n| n as i32),
            view_lights,
            case: Case::parse(&v),
        })
    }
}

/// How far two pictures of the same size are apart.
pub struct Difference {
    /// Pixels that are not background in either picture.
    pub covered: usize,
    /// Share of those that are background in exactly one picture.
    pub outline_mismatch: f32,
    /// Mean colour difference over the covered pixels, 0 to 255.
    pub mean: f32,
    /// One value per pixel, 0 to 255.
    pub map: Vec<u8>,
}

pub fn difference(a: &[u8], b: &[u8], background: [u8; 3]) -> Difference {
    let is_back = |p: &[u8]| (0..3).all(|c| p[c].abs_diff(background[c]) <= 6);
    let mut map = Vec::with_capacity(a.len() / 4);
    let (mut covered, mut one_sided, mut sum) = (0usize, 0usize, 0u64);
    for (pa, pb) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        let d = (0..3).map(|c| pa[c].abs_diff(pb[c]) as u32).sum::<u32>() / 3;
        map.push(d as u8);
        let (ba, bb) = (is_back(pa), is_back(pb));
        if ba && bb {
            continue;
        }
        covered += 1;
        sum += d as u64;
        if ba != bb {
            one_sided += 1;
        }
    }
    let n = covered.max(1) as f32;
    Difference {
        covered,
        outline_mismatch: one_sided as f32 / n,
        mean: sum as f32 / n,
        map,
    }
}

/// Mean colour difference, 0 to 255, over the pixels `covered` marks.
pub fn mean_over(a: &[u8], b: &[u8], covered: impl Fn(usize) -> bool) -> f32 {
    let (mut n, mut sum) = (0usize, 0u64);
    for (i, (pa, pb)) in a.chunks_exact(4).zip(b.chunks_exact(4)).enumerate() {
        if covered(i) {
            n += 1;
            sum += (0..3).map(|c| pa[c].abs_diff(pb[c]) as u64).sum::<u64>() / 3;
        }
    }
    sum as f32 / n.max(1) as f32
}

/// The pixels that are not background in at least one of two pictures.
pub fn character(a: &[u8], b: &[u8], background: [u8; 3]) -> Vec<bool> {
    let is_back = |p: &[u8]| (0..3).all(|c| p[c].abs_diff(background[c]) <= 6);
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .map(|(pa, pb)| !(is_back(pa) && is_back(pb)))
        .collect()
}

/// How a picture with Studio's screen effects compares, per view.
pub struct StudioScore {
    /// Mean difference of our plain picture from the game's picture with
    /// effects, over the character.
    pub without: f32,
    /// Mean difference with our effects over the whole picture.
    pub whole: f32,
}

/// How well the game's cast shadows are met.
pub struct ShadowDifference {
    /// Character pixels where the game is darker than our picture without
    /// real-time shadows.
    pub pixels: usize,
    /// Mean colour difference between the game and our picture there.
    pub mean: f32,
    /// The same against our picture without real-time shadows.
    pub mean_without: f32,
}

/// Mean channel steps the game must be darker by to count as shadowed.
const SHADOW_STEP: i32 = 4;

/// Compares the game with our picture where the game shows a shadow: the
/// pixels that are character in the game and in `plain` (our picture without
/// real-time shadows) and darker in the game by at least `SHADOW_STEP`.
pub fn shadow_difference(game: &[u8], ours: &[u8], plain: &[u8], background: [u8; 3]) -> ShadowDifference {
    let is_back = |p: &[u8]| (0..3).all(|c| p[c].abs_diff(background[c]) <= 6);
    let apart = |a: &[u8], b: &[u8]| (0..3).map(|c| a[c].abs_diff(b[c]) as u64).sum::<u64>() / 3;
    let (mut pixels, mut sum, mut sum_without) = (0usize, 0u64, 0u64);
    for ((g, o), p) in game
        .chunks_exact(4)
        .zip(ours.chunks_exact(4))
        .zip(plain.chunks_exact(4))
    {
        if is_back(g) || is_back(p) {
            continue;
        }
        let darker = (0..3).map(|c| p[c] as i32 - g[c] as i32).sum::<i32>();
        if darker < SHADOW_STEP * 3 {
            continue;
        }
        pixels += 1;
        sum += apart(g, o);
        sum_without += apart(g, p);
    }
    let n = pixels.max(1) as f32;
    ShadowDifference {
        pixels,
        mean: sum as f32 / n,
        mean_without: sum_without as f32 / n,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn reference(
    card_path: &Path,
    capture_path: &Path,
    out: &Path,
    opts: &BuildOptions,
    watch: &[String],
    shadows: bool,
    studio: bool,
    alpha_to_coverage: bool,
) -> Result<()> {
    let game = Game::open(&Config::load()?)?;
    let mut renderer = Renderer::new()?;
    run(
        &game,
        &mut renderer,
        card_path,
        capture_path,
        out,
        opts,
        watch,
        shadows,
        studio,
        alpha_to_coverage,
        true,
    )?;
    Ok(())
}

/// What one comparison found, for batch runs.
pub struct Summary {
    pub name: String,
    /// Mean colour difference and outline mismatch per view.
    pub views: Vec<(String, f32, f32)>,
    /// With Studio's effects compared: one more score per view.
    pub studio: Vec<StudioScore>,
    pub queues_differ: usize,
    pub state: crate::state::Report,
    pub warnings: Vec<String>,
    pub bones: BoneSummary,
}

/// How far our bones are from the capture's, in units (metres).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BoneSummary {
    pub compared: usize,
    pub mean: f32,
    /// The bone that is furthest off, and by how much.
    pub worst: Option<(String, f32)>,
    /// The same among the skirt's bones and among the bust's, which only
    /// physics moves.
    pub skirt: Option<f32>,
    pub bust: Option<f32>,
}

impl BoneSummary {
    fn of(rows: &[(f32, f32, &str)]) -> Self {
        let worst_of = |part: &str| rows.iter().filter(|r| r.2.contains(part)).map(|r| r.0).reduce(f32::max);
        Self {
            compared: rows.len(),
            mean: rows.iter().map(|r| r.0).sum::<f32>() / rows.len().max(1) as f32,
            worst: rows
                .iter()
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .map(|r| (r.2.to_owned(), r.0)),
            skirt: worst_of("cf_j_sk_"),
            bust: worst_of("cf_j_bust"),
        }
    }
}

/// Puts the character in the pose a capture was taken in: the animation
/// held at its time with the physics settled around it, then, for a later
/// capture of a sequence, played on frame by frame as the game stepped it.
/// Returns a line that says what was played.
fn pose_as_captured(
    game: &Game,
    chara: &mut rk_chara::Character,
    case: &Case,
    pose: &CasePose,
    opts: &BuildOptions,
) -> Result<String> {
    let library = rk_chara::animations::Library::read(game);
    let entry = library.find(pose.group, pose.category, pose.number).with_context(|| {
        format!(
            "CharaStudio's list has no animation {}/{}/{}",
            pose.group, pose.category, pose.number
        )
    })?;
    let motion = rk_chara::motion::load(
        game,
        &entry.bundle,
        &entry.controller,
        &entry.state,
        entry.over(),
        &chara.parameters,
    )?;
    let bound = motion.bind(&rk_chara::motion::Skeleton::of(&chara.scene, chara.animator));
    if let Some(heels) = &mut chara.heels {
        heels.animation(Some(&entry.controller), Some(&entry.state));
    }
    let states = case.clothes;
    let held = motion.phase(pose.time, false);
    if opts.physics {
        for _ in 0..opts.settle_frames {
            chara.physics.begin_frame(&mut chara.scene);
            chara.animate(&bound, held, &states);
            chara.physics.end_frame(&mut chara.scene, opts.frame_time);
        }
    } else {
        chara.animate(&bound, held, &states);
    }
    let mut end = pose.time;
    if let Some(seq) = case.sequence.filter(|s| s.frame > 0) {
        let steps = seq.frame * seq.step;
        let dt = 1.0 / seq.rate;
        // The game's own clock when it says, else our reading of the speed.
        end = case
            .normalized_time
            .unwrap_or_else(|| motion.normalized(pose.time, steps as f32 * dt, 1.0));
        for i in 1..=steps {
            let at = sequence_time(pose.time, end, i, steps);
            if opts.physics {
                chara.physics.begin_frame(&mut chara.scene);
            }
            chara.animate(&bound, motion.phase(at, false), &states);
            if opts.physics {
                chara.physics.end_frame(&mut chara.scene, dt);
            }
        }
    }
    Ok(format!(
        "pose: {} ({}/{}) at {:.3} of {:.2} s",
        entry.name,
        entry.controller,
        entry.state,
        end,
        motion.duration()
    ))
}

/// The normalized time of step `i` of `steps` between a sequence's start
/// and the time the game reported at its end.
fn sequence_time(start: f32, end: f32, i: u32, steps: u32) -> f32 {
    start + (end - start) * i as f32 / steps.max(1) as f32
}

/// Builds, renders and compares one card. With `verbose` off nothing is
/// printed, and the bone listing is skipped.
/// The outfit the character wore in the capture, from `status.coordinate`.
fn captured_outfit(capture: &str) -> Option<usize> {
    let v: Value = serde_json::from_str(capture).ok()?;
    v["status"]["coordinate"].as_u64().map(|c| c as usize)
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    game: &Game,
    renderer: &mut Renderer,
    card_path: &Path,
    capture_path: &Path,
    out: &Path,
    opts: &BuildOptions,
    watch: &[String],
    shadows: bool,
    studio: bool,
    alpha_to_coverage: bool,
    verbose: bool,
) -> Result<Summary> {
    macro_rules! say { ($($t:tt)*) => { if verbose { println!($($t)*); } } }
    let capture_text = std::fs::read_to_string(capture_path)?;
    let cap = Capture::parse(&capture_text)?;
    let (w, h) = (cap.width, cap.height);
    let card = Card::load(card_path)?;
    // Dress the character in the outfit the capture was taken in.
    let mut opts = opts.clone();
    if let Some(outfit) = captured_outfit(&capture_text) {
        opts.outfit = outfit;
    }
    let mut chara = rk_chara::build(game, &card, &opts)?;
    let capture_value: Value = serde_json::from_str(&capture_text)?;
    if let Some(map) = &cap.case.map {
        say!("  taken in the map {map}: Renkatsu draws no maps yet, so the numbers mean little");
    }
    if cap.case.clothes != [rk_chara::clothes_state::ON; rk_chara::clothes_state::KINDS] {
        chara.set_clothes(&cap.case.clothes);
        say!("  clothes states {:?}", cap.case.clothes);
    }
    if let Some(pose) = &cap.case.pose {
        let line = pose_as_captured(game, &mut chara, &cap.case, pose, &opts)?;
        say!("  {line}");
    }
    // Clothing blend shapes are checked before the capture's shapes are
    // copied over for the face's expression.
    let clothing_shapes = crate::state::compare_clothing_shapes(&capture_value, &chara.scene);
    let shaped = apply_shapes(&capture_shapes(&capture_text)?, &mut chara.scene);
    let gazes = apply_iris_placement(&capture_iris(&capture_text)?, &mut chara.scene, verbose);
    let draws = chara.scene.pose();
    let defaults = Environment::default();
    let (shadow_color, shadow_depth) = cap.shadow.unwrap_or((defaults.shadow_color, defaults.shadow_depth));
    let env = Environment {
        ramp: cap
            .ramp
            .and_then(|id| rk_chara::build::ramp(game, id))
            .or_else(|| chara.ramp.clone()),
        shadow_color,
        shadow_depth,
        line_depth: cap.line_depth.unwrap_or(defaults.line_depth),
        line_width: cap.line_width.unwrap_or(defaults.line_width),
        light_dir: cap.light_dir,
        light_intensity: cap.light_intensity,
        ambient_light: cap.ambient_light.unwrap_or(defaults.ambient_light),
        ambient_probe: cap
            .ambient_probe
            .map_or(defaults.ambient_probe, |p| rk_render::modshader::sh_constants(&p)),
        shadows: ShadowSettings {
            enabled: shadows && cap.light_shadows.unwrap_or(true),
            strength: cap.shadow_strength.unwrap_or(defaults.shadows.strength),
            soft: cap.light_soft.unwrap_or(defaults.shadows.soft),
            ..defaults.shadows
        },
        alpha_to_coverage,
        ..defaults
    };
    say!(
        "  scene: ramp {:?}, shadow depth {:.2}, line depth {:.2}, line width {:.3}",
        cap.ramp,
        env.shadow_depth,
        env.line_depth,
        env.line_width
    );
    let back = cap.background.map(f64::from);
    let back8 = [0, 1, 2].map(|c| (cap.background[c] * 255.0 + 0.5) as u8);
    say!("{}: game, Renkatsu, difference", card.full_name());
    let mut views = Vec::new();

    // With `studio` the pictures taken with Studio's screen effects are
    // compared, with the scene's effects on our side.
    let fx_env = studio.then(|| Environment {
        effects: Some(StudioEffects {
            color: cap
                .color_filter
                .and_then(|id| rk_chara::build::color_filter(game, id))
                .map(|lut| rk_render::effects::ColorFilter {
                    lut,
                    exposure: 1.0,
                    blend: 0.0,
                }),
            ..cap.effects.clone()
        }),
        ..env.clone()
    });
    if let Some(fx) = fx_env.as_ref().and_then(|e| e.effects.as_ref()) {
        say!(
            "  effects: occlusion {}, bloom {}, colour filter {:?} ({}), vignette {}",
            fx.ao.is_some(),
            fx.bloom.is_some(),
            cap.color_filter,
            fx.color.as_ref().map_or("not found", |c| c.lut.name.as_str()),
            fx.vignette.is_some()
        );
    }
    let mut scores = Vec::new();

    for (view, (name, camera)) in cap.views.iter().enumerate() {
        // The light as it stood for this view, where the capture says.
        let lit = |e: &Environment| match cap.view_lights.get(view).copied().flatten() {
            Some(dir) => Environment {
                light_dir: Some(dir),
                ..e.clone()
            },
            None => e.clone(),
        };
        let env = lit(&env);
        let fx_env = fx_env.as_ref().map(lit);
        let load = |kind: &str| -> Result<Vec<u8>> {
            let picture = capture_path.with_extension(format!("{name}.{kind}.png"));
            let (gw, gh, px) = load_png(&std::fs::read(&picture).with_context(|| picture.display().to_string())?)?;
            if (gw, gh) != (w, h) {
                bail!("{} is {gw}x{gh}, the capture says {w}x{h}", picture.display());
            }
            Ok(px)
        };
        let mut game_px = load("plain")?;
        let mut ours = renderer.render(&draws, camera, &env, w, h, back)?;
        let mut diff = difference(&game_px, &ours, back8);
        if let Some(fx_env) = &fx_env {
            // The character's pixels are told from the background in the
            // plain pictures, where the background is one colour.
            let covered = character(&game_px, &ours, back8);
            let game_fx = load("studio")?;
            let ours_fx = renderer.render(&draws, camera, fx_env, w, h, back)?;
            scores.push(StudioScore {
                without: mean_over(&game_fx, &ours, |i| covered[i]),
                whole: mean_over(&game_fx, &ours_fx, |_| true),
            });
            diff.mean = mean_over(&game_fx, &ours_fx, |i| covered[i]);
            diff.map = difference(&game_fx, &ours_fx, back8).map;
            game_px = game_fx;
            ours = ours_fx;
        }

        // White is equal, red is far apart.
        let sheet_w = 3 * w + 4 * GAP;
        let sheet_h = h + 2 * GAP;
        let mut sheet = [40u8, 40, 46, 255].repeat((sheet_w * sheet_h) as usize);
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let d = (diff.map[i] as u32 * 4).min(255) as u8;
                let panels = [
                    [game_px[i * 4], game_px[i * 4 + 1], game_px[i * 4 + 2], 255],
                    [ours[i * 4], ours[i * 4 + 1], ours[i * 4 + 2], 255],
                    [255, 255 - d, 255 - d, 255],
                ];
                for (n, px) in panels.iter().enumerate() {
                    let o = (((GAP + y) * sheet_w + GAP + n as u32 * (w + GAP) + x) * 4) as usize;
                    sheet[o..o + 4].copy_from_slice(px);
                }
            }
        }
        let stem = out.file_stem().unwrap_or_default().to_string_lossy();
        let suffix = if studio { "-studio" } else { "" };
        let file = out.with_file_name(format!("{stem}-{name}{suffix}.png"));
        save_png(&file, sheet_w, sheet_h, &sheet)?;
        say!("  {name} -> {}", file.display());
        say!(
            "    character pixels: {} ({:.1}% of the picture)",
            diff.covered,
            100.0 * diff.covered as f32 / (w * h) as f32
        );
        say!(
            "    outline mismatch: {:.1}% of them are character in only one picture",
            100.0 * diff.outline_mismatch
        );
        say!(
            "    match: {:.1}% (mean colour difference {:.1} of 255)",
            crate::ui::match_percent(diff.mean),
            diff.mean
        );
        views.push((name.clone(), diff.mean, diff.outline_mismatch));
        if let Some(score) = scores.last() {
            say!(
                "    without our effects: {:.1}%; with them over the whole picture: {:.1}%",
                crate::ui::match_percent(score.without),
                crate::ui::match_percent(score.whole)
            );
        }
        if env.shadows.enabled && !studio {
            let plain_env = Environment {
                shadows: ShadowSettings {
                    enabled: false,
                    ..env.shadows
                },
                ..env.clone()
            };
            let plain = renderer.render(&draws, camera, &plain_env, w, h, back)?;
            let s = shadow_difference(&game_px, &ours, &plain, back8);
            say!(
                "    shadows: {:.2} of 255 over the {} pixels the game shades ({:.2} without our shadows)",
                s.mean,
                s.pixels,
                s.mean_without
            );
        }
    }
    for warning in &chara.warnings {
        say!("  warning: {warning}");
    }
    say!("  expression: blend shapes of {shaped} meshes and the iris placement of {gazes} eyes set from the capture");
    let queues_differ = compare_queues(&capture_text, &chara.scene, verbose)?;
    let mut state = crate::state::compare(&capture_value, &chara.scene);
    state.merge(clothing_shapes);
    if verbose {
        crate::state::print(&state, 8);
        compare_bones(&capture_text, &chara.scene, watch)?;
    }
    let (rows, _, _) = bone_rows(&capture_text, &chara.scene)?;
    Ok(Summary {
        name: card.full_name(),
        views,
        studio: scores,
        queues_differ,
        state,
        warnings: chara.warnings.clone(),
        bones: BoneSummary::of(&rows),
    })
}

/// A renderer name with its blend shape weights, as shape index and 0 to 1.
pub type Shapes = (String, Vec<(usize, f32)>);

/// Blend shape weights the game had set.
pub fn capture_shapes(capture: &str) -> Result<Vec<Shapes>> {
    let v: Value = serde_json::from_str(capture)?;
    let mut out = Vec::new();
    for r in v["renderers"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let (Some(name), Some(shapes)) = (r["name"].as_str(), r["shapes"].as_array()) else {
            continue;
        };
        let mut weights = Vec::new();
        for s in shapes {
            let [index, percent] = floats::<2>(s)?;
            weights.push((index as usize, percent / 100.0));
        }
        out.push((name.to_owned(), weights));
    }
    Ok(out)
}

/// Replaces the blend shape weights of the renderers the capture names.
/// Returns how many renderers were set.
fn apply_shapes(shapes: &[Shapes], scene: &mut rk_chara::Scene) -> usize {
    let mut count = 0;
    for (name, weights) in shapes {
        for r in scene.renderers.iter_mut().filter(|r| &r.name == name) {
            r.shape_weights = vec![0.0; r.mesh.blend_shapes.len()];
            for &(index, w) in weights {
                if let Some(slot) = r.shape_weights.get_mut(index) {
                    *slot = w;
                }
            }
            count += 1;
        }
    }
    count
}

/// Scale and offset the game had on each iris texture, per renderer name.
/// They include where the eyes were looking.
pub fn capture_iris(capture: &str) -> Result<Vec<(String, [f32; 4])>> {
    let v: Value = serde_json::from_str(capture)?;
    let mut out = Vec::new();
    for r in v["renderers"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let material = &r["materials"][0];
        let shader = material["shader"].as_str().unwrap_or_default().to_lowercase();
        // The iris shader, not the one for eye whites and lashes.
        if !shader.contains("eye") || shader.contains("eyew") {
            continue;
        }
        if let (Some(name), Some(_)) = (r["name"].as_str(), material["main_tex_st"].as_array()) {
            out.push((name.to_owned(), floats::<4>(&material["main_tex_st"])?));
        }
    }
    Ok(out)
}

/// Puts the captured scale and offset on the iris textures. The highlights
/// keep their distance from the iris. Returns how many eyes were set.
fn apply_iris_placement(placements: &[(String, [f32; 4])], scene: &mut rk_chara::Scene, verbose: bool) -> usize {
    let mut count = 0;
    for (name, st) in placements {
        for r in scene.renderers.iter_mut().filter(|r| &r.name == name) {
            let Some(m) = r.materials.first_mut() else { continue };
            let Some((old, old_scale)) = m.textures.get("_MainTex").map(|t| (t.offset, t.scale)) else {
                continue;
            };
            // The scale does not depend on the gaze, so it checks our own rule.
            if verbose {
                println!(
                    "  iris {name}: scale from the card {:.4} x {:.4}, in the game {:.4} x {:.4}",
                    old_scale[0], old_scale[1], st[0], st[1]
                );
            }
            for (key, slot) in m.textures.iter_mut() {
                if !matches!(key.as_str(), "_MainTex" | "_overtex1" | "_overtex2") {
                    continue;
                }
                slot.scale = [st[0], st[1]];
                slot.offset = [st[2] + slot.offset[0] - old[0], st[3] + slot.offset[1] - old[1]];
            }
            count += 1;
        }
    }
    count
}

/// Checks the draw order numbers we set ourselves against the game's.
fn compare_queues(capture: &str, scene: &rk_chara::Scene, verbose: bool) -> Result<usize> {
    let v: Value = serde_json::from_str(capture)?;
    let (mut same, mut differ) = (0, Vec::new());
    for r in v["renderers"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let Some(name) = r["name"].as_str() else { continue };
        // Unity names instances "<material> (Instance)"; names repeat across
        // renderers, so the first material's name picks between them.
        let first = r["materials"][0]["name"].as_str().unwrap_or("");
        let first = first.split(" (Instance)").next().unwrap_or(first);
        let mut same_name = scene.renderers.iter().filter(|x| x.name == name);
        let Some(ours) = same_name
            .clone()
            .find(|x| x.materials.first().is_some_and(|m| m.name == first))
            .or_else(|| same_name.next())
        else {
            continue;
        };
        for (i, m) in r["materials"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let (Some(game), Some(mine)) = (m["queue"].as_i64(), ours.materials.get(i)) else {
                continue;
            };
            // The material's own queue, or else its shader's.
            let queue = if mine.render_queue > 0 {
                mine.render_queue
            } else {
                match mine.draw {
                    Some(d) => d.queue,
                    None => continue,
                }
            };
            if game == queue as i64 {
                same += 1;
            } else {
                differ.push(format!("{name}[{i}] game {game}, ours {queue}"));
            }
        }
    }
    if verbose {
        println!("  draw order: {same} materials match the game, {} differ", differ.len());
        for line in differ.iter().take(12) {
            println!("    {line}");
        }
    }
    Ok(differ.len())
}

/// One row per bone both sides have: how far it is from the capture's
/// place, how far its rotation and scale are off as matrix distance, and
/// its name; then how many were compared and how many only the game has.
type BoneRows<'a> = (Vec<(f32, f32, &'a str)>, usize, usize);

fn bone_rows<'a>(capture: &str, scene: &'a rk_chara::Scene) -> Result<BoneRows<'a>> {
    let v: Value = serde_json::from_str(capture)?;
    let (world, _) = scene.world();
    let mut ours = std::collections::HashMap::new();
    for (i, node) in scene.nodes.iter().enumerate() {
        // Names can repeat; keep the first, as the game's lookups do.
        ours.entry(node.name.as_str()).or_insert(world[i]);
    }
    let mut seen = std::collections::HashSet::new();
    let mut rows = Vec::new();
    let (mut matched, mut missing) = (0, 0);
    for b in v["bones"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let Some(name) = b["name"].as_str() else { continue };
        if !seen.insert(name.to_owned()) {
            continue;
        }
        let Some((name, mine)) = ours.get_key_value(name) else {
            missing += 1;
            continue;
        };
        let name: &'a str = name;
        matched += 1;
        // The capture stores rows; glam stores columns.
        let game = glam::Mat4::from_cols_array(&floats::<16>(&b["world"])?).transpose();
        let moved = (game.w_axis - mine.w_axis).truncate().length();
        let shape = (0..3)
            .map(|c| (game.col(c) - mine.col(c)).truncate().length())
            .fold(0.0f32, f32::max);
        rows.push((moved, shape, name));
    }
    Ok((rows, matched, missing))
}

/// Lists the bones whose place or shape differs most from the capture.
fn compare_bones(capture: &str, scene: &rk_chara::Scene, watch: &[String]) -> Result<()> {
    let (mut rows, matched, missing) = bone_rows(capture, scene)?;
    println!("  bones: {matched} compared, {missing} only in the game");
    rows.sort_by(|a, b| (b.0 + b.1 * 0.1).total_cmp(&(a.0 + a.1 * 0.1)));
    println!("  largest differences (position in units, rotation and scale as matrix distance):");
    for (moved, shape, name) in rows.iter().take(40) {
        println!("    {name:<28} moved {moved:.4}  turned or scaled {shape:.3}");
    }
    for (moved, shape, name) in rows.iter().filter(|r| watch.iter().any(|w| r.2.contains(w.as_str()))) {
        println!("    watched {name:<28} moved {moved:.4}  turned or scaled {shape:.3}");
    }
    let off = rows.iter().filter(|r| r.0 > 0.002 || r.1 > 0.02).count();
    println!("  {off} of {} bones differ noticeably", rows.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "size": [4, 2],
        "camera": {"fov": 23, "background": [0.5, 0.5, 0.5, 1]},
        "views": [{"name": "body", "position": [0, 1, 8], "rotation": [0, 1, 0, 0]},
                  {"name": "face", "position": [0, 1.5, 2], "rotation": [0, 1, 0, 0]}],
        "studio": {"ramp_g": 2},
        "globals": {"_ambientshadowG": [0.5, 0.4, 0.3, 0.74], "_LineColorG": [0.5, 0.5, 0.5, 0], "_linewidthG": 0.307,
                    "ambient_light": [0.3, 0.3405, 0.3885, 1.5]},
        "lights": [
            {"type": "Point", "direction": [1, 0, 0], "intensity": 3},
            {"type": "Directional", "direction": [0, -0.6, -0.8], "intensity": 2, "shadows": "Soft", "shadow_strength": 0.75}
        ]
    }"#;

    #[test]
    fn capture_effects_follow_the_scene() {
        // A capture without the switches has no effects.
        let none = Capture::parse(SAMPLE).unwrap();
        assert!(none.effects.ao.is_none() && none.effects.bloom.is_none() && none.effects.vignette.is_none());
        assert_eq!(none.color_filter, None);
        // The switches turn Studio's stored values on.
        let studio = r#""studio": {"ramp_g": 2, "ace_no": 4, "ao": true, "bloom": true, "vignette": false}"#;
        let on = Capture::parse(&SAMPLE.replace(r#""studio": {"ramp_g": 2}"#, studio)).unwrap();
        assert_eq!(on.color_filter, Some(4));
        assert_eq!(on.effects.ao, Some(Default::default()));
        assert_eq!(on.effects.bloom, Some(Default::default()));
        assert!(on.effects.vignette.is_none());
        // The dumped fields of a component replace them.
        let params = format!(
            r#"{studio}, "effect_params": [
                {{"type": "AmplifyOcclusionEffect", "enabled": true,
                  "fields": {{"Radius": 0.5, "Tint": [0.1, 0.2, 0.3, 1], "SampleCount": "Medium"}}}},
                {{"type": "UnityStandardAssets.ImageEffects.BloomAndFlares", "enabled": true,
                  "fields": {{"bloomIntensity": 1.5, "bloomBlurIterations": 3}}}}]"#
        );
        let set = Capture::parse(&SAMPLE.replace(r#""studio": {"ramp_g": 2}"#, &params)).unwrap();
        let ao = set.effects.ao.unwrap();
        assert_eq!((ao.radius, ao.tint, ao.bias), (0.5, [0.1, 0.2, 0.3], 0.25));
        let bloom = set.effects.bloom.unwrap();
        assert_eq!((bloom.intensity, bloom.iterations, bloom.threshold), (1.5, 3, 0.6));
    }

    #[test]
    fn scores_count_only_the_marked_pixels() {
        let back = [200, 200, 200];
        let a = [200, 200, 200, 255, 10, 20, 30, 255, 200, 200, 200, 255];
        let b = [200, 200, 200, 255, 13, 26, 39, 255, 50, 50, 50, 255];
        let covered = character(&a, &b, back);
        assert_eq!(covered, [false, true, true]);
        assert_eq!(mean_over(&a, &b, |i| i == 1), 6.0);
        assert_eq!(mean_over(&a, &b, |i| covered[i]), (6.0 + 150.0) / 2.0);
        assert_eq!(mean_over(&a, &b, |_| true), (6.0 + 150.0) / 3.0);
        assert_eq!(mean_over(&a, &b, |_| false), 0.0);
    }

    #[test]
    fn capture_camera_and_light() {
        let c = Capture::parse(SAMPLE).unwrap();
        assert_eq!((c.width, c.height), (4, 2));
        // Turned half a circle around the vertical axis: looks down -Z.
        assert_eq!(c.views.len(), 2);
        let (name, camera) = &c.views[0];
        assert_eq!(name, "body");
        assert!((camera.target - Vec3::new(0.0, 1.0, 7.0)).length() < 1e-5);
        assert_eq!(camera.fov_y_deg, 23.0);
        assert_eq!(c.views[1].0, "face");
        let l = c.light_dir.unwrap();
        assert!((Vec3::from(l) - Vec3::new(0.0, 0.6, 0.8)).length() < 1e-6);
        assert_eq!(c.light_intensity, 2.0);
        assert_eq!((c.light_shadows, c.shadow_strength), (Some(true), Some(0.75)));
        assert_eq!(c.light_soft, Some(true));
        let off = Capture::parse(&SAMPLE.replace("\"Soft\"", "\"None\"")).unwrap();
        assert_eq!(off.light_shadows, Some(false));
        let silent = Capture::parse(&SAMPLE.replace(", \"shadows\": \"Soft\", \"shadow_strength\": 0.75", "")).unwrap();
        assert_eq!((silent.light_shadows, silent.shadow_strength), (None, None));
        assert_eq!(c.ramp, Some(2));
        let (color, depth) = c.shadow.unwrap();
        assert_eq!(color, [0.5, 0.4, 0.3]);
        assert!((depth - 0.26).abs() < 1e-6);
        assert_eq!(c.line_depth, Some(1.0));
        assert_eq!(c.line_width, Some(0.307));
        assert_eq!(c.ambient_light, Some([0.3, 0.3405, 0.3885]));
        let bare = Capture::parse(&SAMPLE.replace("\"ambient_light\"", "\"other\"")).unwrap();
        assert_eq!(bare.ambient_light, None);
        // Older captures carry no ambient probe.
        assert_eq!(c.ambient_probe, None);
        let probe = SAMPLE.replace(
            "\"ambient_light\"",
            &format!("\"ambient_probe\": [{}], \"ambient_light\"", ["0.5"; 27].join(", ")),
        );
        assert_eq!(Capture::parse(&probe).unwrap().ambient_probe, Some([0.5; 27]));
    }

    #[test]
    fn capture_case() {
        // An older capture: everything on, no pose, the scene's light.
        let plain = Capture::parse(SAMPLE).unwrap();
        assert_eq!(plain.case, Case::default());
        assert_eq!(plain.view_lights, vec![None, None]);
        let text = SAMPLE
            .replace(
                r#""studio": {"ramp_g": 2}"#,
                r#""studio": {"ramp_g": 2},
                   "status": {"clothes_state": [1, 3, 0, 0, 0, 0, 0, 0, 0]},
                   "animation": {"normalized_time": 0.75},
                   "case": {"pose": {"group": 0, "category": 3, "number": 2, "time": 0.5},
                            "sequence": {"frame": 2, "frames": 8, "step": 3, "rate": 30},
                            "map": "myroom"}"#,
            )
            .replace(
                r#"{"name": "face", "position": [0, 1.5, 2], "rotation": [0, 1, 0, 0]}"#,
                r#"{"name": "face", "position": [0, 1.5, 2], "rotation": [0, 1, 0, 0], "fov": 60,
                    "light_direction": [0, -1, 0]}"#,
            );
        let c = Capture::parse(&text).unwrap();
        assert_eq!(c.case.clothes, [1, 3, 0, 0, 0, 0, 0, 0, 0]);
        let pose = c.case.pose.unwrap();
        assert_eq!((pose.group, pose.category, pose.number, pose.time), (0, 3, 2, 0.5));
        let seq = c.case.sequence.unwrap();
        assert_eq!((seq.frame, seq.step, seq.rate), (2, 3, 30.0));
        assert_eq!(c.case.normalized_time, Some(0.75));
        assert_eq!(c.case.map.as_deref(), Some("myroom"));
        // A view's own angle and light replace the scene's.
        assert_eq!((c.views[0].1.fov_y_deg, c.views[1].1.fov_y_deg), (23.0, 60.0));
        assert_eq!(c.view_lights, vec![None, Some([0.0, 1.0, 0.0])]);
        // The maker's pose is number -1.
        let maker = SAMPLE.replace(
            r#""studio": {"ramp_g": 2}"#,
            r#""studio": {"ramp_g": 2}, "case": {"pose": {"group": 0, "category": 0, "number": -1, "time": 0.25}}"#,
        );
        assert_eq!(Capture::parse(&maker).unwrap().case.pose.unwrap().number, -1);
    }

    #[test]
    fn sequence_steps_run_from_the_held_time_to_the_reported_one() {
        assert_eq!(sequence_time(0.5, 1.1, 0, 6), 0.5);
        assert!((sequence_time(0.5, 1.1, 3, 6) - 0.8).abs() < 1e-6);
        assert!((sequence_time(0.5, 1.1, 6, 6) - 1.1).abs() < 1e-6);
        assert_eq!(sequence_time(0.5, 1.1, 1, 0), 1.1);
    }

    #[test]
    fn bone_summary_names_the_worst_and_the_physics_bones() {
        let rows = [
            (0.01, 0.0, "cf_j_head"),
            (0.03, 0.0, "cf_j_sk_00_01"),
            (0.02, 0.0, "cf_j_sk_03_02"),
            (0.005, 0.0, "cf_j_bust02_L"),
        ];
        let s = BoneSummary::of(&rows);
        assert_eq!(s.compared, 4);
        assert!((s.mean - 0.01625).abs() < 1e-6);
        assert_eq!(s.worst, Some(("cf_j_sk_00_01".to_owned(), 0.03)));
        assert_eq!((s.skirt, s.bust), (Some(0.03), Some(0.005)));
        assert_eq!(BoneSummary::of(&[]), BoneSummary::default());
    }

    #[test]
    fn capture_without_lights() {
        let text = SAMPLE.replace("\"Directional\"", "\"Spot\"");
        assert!(Capture::parse(&text).unwrap().light_dir.is_none());
        assert!(Capture::parse("{}").is_err());
    }

    #[test]
    fn capture_blend_shapes() {
        let text = r#"{"renderers": [
            {"name": "face", "shapes": [[0, 30], [1, 70]]},
            {"name": "shoe"},
            {"name": "brow", "shapes": []}
        ]}"#;
        let shapes = capture_shapes(text).unwrap();
        assert_eq!(shapes.len(), 2);
        assert_eq!(shapes[0], ("face".to_owned(), vec![(0, 0.3), (1, 0.7)]));
        assert_eq!(shapes[1], ("brow".to_owned(), vec![]));
        assert!(capture_shapes("{}").unwrap().is_empty());
    }

    #[test]
    fn capture_iris_placement() {
        let text = r#"{"renderers": [
            {"name": "iris_l", "materials": [{"shader": "Shader Forge/toon_eye_lod0", "main_tex_st": [1.05, 1.1, -0.04, -0.01]}]},
            {"name": "white_l", "materials": [{"shader": "Shader Forge/toon_eyew_lod0", "main_tex_st": [1, 1, 0, 0]}]},
            {"name": "shirt", "materials": [{"shader": "Shader Forge/main_opaque", "main_tex_st": [1, 1, 0, 0]}]},
            {"name": "bare", "materials": []}
        ]}"#;
        let iris = capture_iris(text).unwrap();
        assert_eq!(iris, vec![("iris_l".to_owned(), [1.05, 1.1, -0.04, -0.01])]);
    }

    #[test]
    fn shadow_difference_looks_where_the_game_is_darker() {
        let back = [128, 128, 128];
        let px = |v: u8| [v, v, v, 255];
        // Background, a lit pixel, a shadowed pixel we meet, one we miss.
        let game: Vec<u8> = [px(128), px(200), px(150), px(150)].concat();
        let plain: Vec<u8> = [px(128), px(201), px(200), px(200)].concat();
        let ours: Vec<u8> = [px(128), px(201), px(152), px(200)].concat();
        let s = shadow_difference(&game, &ours, &plain, back);
        assert_eq!(s.pixels, 2);
        assert_eq!(s.mean, 26.0);
        assert_eq!(s.mean_without, 50.0);
        // Nothing darker: no pixels, and no division by zero.
        let none = shadow_difference(&plain, &ours, &plain, back);
        assert_eq!((none.pixels, none.mean), (0, 0.0));
    }

    #[test]
    fn difference_counts_only_the_character() {
        let back = [10, 10, 10];
        let b = [10, 10, 10, 255];
        let red = [110, 10, 10, 255];
        let a: Vec<u8> = [b, red, red, b].concat();
        let c: Vec<u8> = [b, red, b, [13, 10, 10, 255]].concat();
        let d = difference(&a, &c, back);
        // Pixel 0 and 3 are background in both (3 is within tolerance).
        assert_eq!(d.covered, 2);
        assert_eq!(d.outline_mismatch, 0.5);
        // Pixel 1 equal, pixel 2 differs by 100 in one of three channels.
        assert_eq!(d.mean, 16.5);
        assert_eq!(d.map, vec![0, 0, 33, 1]);
    }
}
