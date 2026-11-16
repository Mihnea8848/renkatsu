// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Stiletto plugin (2.4.2): heels. The worn shoe's list name picks a
//! text file in `BepInEx/config/Stiletto/Heels`; its values lift the whole
//! body and turn the lower legs, feet and toes after the animation has
//! posed them, every frame (`HeelInfo.PostUpdate`,
//! `HeelInfoAnimation.Update`). The same file can warp the shoe's meshes
//! around the foot bones (`HeelInfo.UpdateShoeWarp`).
//!
//! Which parts run is decided by flags looked up by animation
//! (`AnimationFlagsProvider.Load`). This engine plays no animation, so the
//! lookup is made the way the plugin makes it for a character without an
//! animator: with no controller and no clip name.
//!
//! Not ported: knee bend, which moves the effectors of the FullBodyBipedIK
//! solver. It only runs while that component is enabled, and Studio switches
//! it off when it loads a character (`OCIChar.InitKinematic`), so a knee
//! bend flag lifts the body like the plain height does.

use crate::scene::{RendererInst, Scene};
use glam::{Mat4, Quat, Vec3};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// `FilePathSettings.CONFIG_PATH` below the game folder.
pub const CONFIG_DIR: &str = "BepInEx/config/Stiletto";

/// `HeelInfo.AngleRange`: file angles above it are read as negative ones.
const ANGLE_RANGE: f32 = 60.0;

/// `float.TryParse` leaves zero behind when the text is not a number.
fn number(text: &str) -> f32 {
    text.trim().parse().unwrap_or(0.0)
}

/// The `name=value` lines of a settings file, as `DynamicFileProvider.Read`
/// splits them: the value is the text between the first and second `=`.
fn properties(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.trim_start_matches('\u{feff}').lines().filter_map(|line| {
        let mut parts = line.split('=');
        let name = parts.next()?.trim();
        Some((name, parts.next()?.trim()))
    })
}

/// A heel file, with the fix-ups of `HeelInfo.Setup` applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heel {
    /// Degrees around X added to the feet.
    pub ankle: f32,
    /// Degrees around X added to the toes.
    pub toes: f32,
    /// Degrees around X added to the lower legs.
    pub leg: f32,
    /// How far the body is lifted.
    pub height: f32,
    pub shoe_scale: Vec3,
    pub shoe_angle: f32,
    /// Only Y and Z are stored.
    pub shoe_translate: Vec3,
    pub shoe_shear_y: f32,
    pub shoe_shear_z: f32,
}

impl Default for Heel {
    /// What `Setup` leaves when the shoe has no name.
    fn default() -> Self {
        Self {
            ankle: 0.0,
            toes: 0.0,
            leg: 0.0,
            height: 0.0,
            shoe_scale: Vec3::ONE,
            shoe_angle: 0.0,
            shoe_translate: Vec3::ZERO,
            shoe_shear_y: 0.0,
            shoe_shear_z: 0.0,
        }
    }
}

impl Heel {
    /// Reads a heel file. A missing file is read as empty text: every value
    /// is zero then and the shoe scale one.
    pub fn parse(text: &str) -> Self {
        let mut raw: HashMap<&str, f32> = HashMap::new();
        for (name, value) in properties(text) {
            raw.insert(name, number(value));
        }
        let get = |name: &str| raw.get(name).copied().unwrap_or(0.0);
        // `GetAngleInRange`.
        let angle = |name: &str| {
            let v = get(name);
            if v > ANGLE_RANGE + 0.001 {
                -(360.0 - v)
            } else {
                v
            }
        };
        let scale = |name: &str| {
            let v = get(name);
            if v != 0.0 {
                v
            } else {
                1.0
            }
        };
        Self {
            ankle: angle("angleAnkle"),
            toes: angle("angleToes"),
            leg: angle("angleLeg"),
            height: get("height"),
            shoe_scale: Vec3::new(scale("shoeScaleX"), scale("shoeScaleY"), scale("shoeScaleZ")),
            shoe_angle: get("shoeAngle"),
            shoe_translate: Vec3::new(0.0, get("shoeTranslateY"), get("shoeTranslateZ")),
            shoe_shear_y: get("shoeShearY"),
            shoe_shear_z: get("shoeShearZ"),
        }
    }

    /// True when the shoe's bind poses stay as they are.
    fn keeps_shoe(&self, active: bool) -> bool {
        let unchanged = self.shoe_scale == Vec3::ONE && self.shoe_angle == 0.0 && self.shoe_translate == Vec3::ZERO;
        (unchanged || !active) && self.shoe_shear_y == 0.0 && self.shoe_shear_z == 0.0
    }
}

/// `AnimationFlags`: which parts of the plugin run for an animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags {
    pub active: bool,
    pub height: bool,
    pub toe_roll: bool,
    pub ankle_roll: bool,
    pub knee_bend: bool,
    pub custom_pose: bool,
}

impl Flags {
    /// `AnimationFlags.Deserialize`: six comma separated fields, `1` for
    /// on, set in order through setters that switch each other.
    pub fn parse(value: &str) -> Self {
        let on: Vec<bool> = value.split(',').map(|x| x.trim() == "1").collect();
        let at = |i: usize| on.get(i).copied().unwrap_or(false);
        let mut f = Flags {
            active: at(0),
            ..Default::default()
        };
        f.set_height(at(1));
        f.toe_roll = at(2);
        f.ankle_roll = at(3);
        f.set_knee_bend(at(4));
        f.set_custom_pose(at(5));
        f
    }

    fn set_height(&mut self, on: bool) {
        self.height = on;
        if !on {
            self.knee_bend = false;
            self.custom_pose = false;
        }
    }

    fn set_knee_bend(&mut self, on: bool) {
        self.knee_bend = on;
        if on {
            self.height = true;
            self.custom_pose = false;
        }
    }

    fn set_custom_pose(&mut self, on: bool) {
        self.custom_pose = on;
        if on {
            self.height = true;
            self.knee_bend = false;
        }
    }

    /// The flags `StilettoContext.Initalize` writes over the default
    /// file's catch-all line each time the game starts.
    pub fn at_start(enable_on_start: bool) -> Self {
        Flags {
            active: enable_on_start,
            height: true,
            toe_roll: true,
            ankle_roll: true,
            knee_bend: false,
            custom_pose: false,
        }
    }
}

/// One flags file (`AnimationSettingsCollection<AnimationFlags>`).
#[derive(Debug, Clone, Default)]
pub struct FlagFile {
    /// `controller/clip` lines.
    keys: HashMap<String, Flags>,
    /// `p:controller` lines.
    paths: HashMap<String, Flags>,
    /// `r:pattern` lines, in file order.
    patterns: Vec<(String, Flags)>,
}

impl FlagFile {
    pub fn parse(text: &str) -> Self {
        let mut file = FlagFile::default();
        for line in text.trim_start_matches('\u{feff}').lines() {
            // `AnimationSettingsValue.Parse`.
            if line.starts_with(';') {
                continue;
            }
            let parts: Vec<&str> = line.split('=').collect();
            let [operation, value] = parts[..] else { continue };
            let flags = Flags::parse(value.trim());
            let operation = operation.trim();
            let split: Vec<&str> = operation.split(':').collect();
            match split[..] {
                [kind, matcher] => match kind.trim() {
                    "p" => drop(file.paths.insert(matcher.trim().to_string(), flags)),
                    "r" => file.patterns.push((matcher.trim().to_string(), flags)),
                    _ => drop(file.keys.insert(matcher.trim().to_string(), flags)),
                },
                _ => drop(file.keys.insert(operation.to_string(), flags)),
            }
        }
        file
    }

    /// `AnimationFlagsProvider.Initialize` on the default file: the first
    /// catch-all line takes the start flags, or one is added at the end.
    pub fn with_start_flags(mut self, start: Flags) -> Self {
        match self.patterns.iter_mut().find(|(p, _)| p.is_empty()) {
            Some((_, flags)) => *flags = start,
            None => self.patterns.push((String::new(), start)),
        }
        self
    }

    /// `AnimationSettingsCollection.Load`. `Err` names a pattern this port
    /// cannot evaluate: the plugin uses .NET regular expressions, and only
    /// patterns without special characters (plain text searches) are run.
    fn load(&self, path: Option<&str>, name: Option<&str>) -> Result<Option<Flags>, String> {
        let key = format!("{}/{}", path.unwrap_or(""), name.unwrap_or(""));
        if let Some(f) = self.keys.get(&key) {
            return Ok(Some(*f));
        }
        if let Some(f) = path.and_then(|p| self.paths.get(p)) {
            return Ok(Some(*f));
        }
        for (pattern, flags) in &self.patterns {
            if pattern.contains(|c| r"\^$.|?*+()[]{}".contains(c)) {
                return Err(pattern.clone());
            }
            if key.contains(pattern.as_str()) {
                return Ok(Some(*flags));
            }
        }
        Ok(None)
    }
}

/// `AnimationFlagsProvider.Load`: the first file with a matching line wins;
/// no match at all means every flag is off. `files` are the files of the
/// `Flags` folder in path order, then `_flags.txt`, then `_dumps.txt`.
pub fn flags_for(files: &[FlagFile], path: Option<&str>, name: Option<&str>) -> Result<Flags, String> {
    for file in files {
        if let Some(flags) = file.load(path, name)? {
            return Ok(flags);
        }
    }
    Ok(Flags::default())
}

/// `GeneralSettings`, from `_settings.txt`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub body_length_power: f32,
    pub body_length_multiplier: f32,
    pub heel_height_power: f32,
    pub heel_height_multiplier: f32,
    pub enable_on_start: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            body_length_power: 0.075,
            body_length_multiplier: 0.3,
            heel_height_power: 0.75,
            heel_height_multiplier: 32.0,
            enable_on_start: true,
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> Self {
        let mut s = Settings::default();
        for (name, value) in properties(text) {
            match name {
                "CustomPose_BodyLengthPower" => s.body_length_power = number(value),
                "CustomPose_BodyLengthMultiplier" => s.body_length_multiplier = number(value),
                "CustomPose_HeelHeightPower" => s.heel_height_power = number(value),
                "CustomPose_HeelHeightMultiplier" => s.heel_height_multiplier = number(value),
                // `bool.TryParse`: any case, false when it is neither word.
                "Enable_On_Start" => s.enable_on_start = value.eq_ignore_ascii_case("true"),
                _ => {}
            }
        }
        s
    }
}

/// `CustomPose`: angles in degrees a pose file stores per animation. The
/// default, all zero, is what an animation without a pose line gets.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CustomPose {
    pub waist: f32,
    pub right_thigh: f32,
    pub right_leg: f32,
    pub right_ankle: f32,
    pub left_thigh: f32,
    pub left_leg: f32,
    pub left_ankle: f32,
}

/// The plugin's files, read once per character.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub settings: Settings,
    pub flag_files: Vec<FlagFile>,
    heels: PathBuf,
}

fn read(path: &Path) -> String {
    std::fs::read(path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

impl Config {
    /// Reads the settings and flags below the game folder. Missing files
    /// give the plugin's defaults.
    pub fn load(game_dir: &Path) -> Self {
        let dir = game_dir.join(CONFIG_DIR);
        let settings = Settings::parse(&read(&dir.join("_settings.txt")));
        // `Directory.GetFiles(root, "*.txt", AllDirectories)`, ordered.
        let mut paths = Vec::new();
        rk_core::walk_files(&dir.join("Flags"), "txt", &mut paths);
        paths.sort();
        let mut flag_files: Vec<FlagFile> = paths.iter().map(|p| FlagFile::parse(&read(p))).collect();
        flag_files.push(
            FlagFile::parse(&read(&dir.join("_flags.txt"))).with_start_flags(Flags::at_start(settings.enable_on_start)),
        );
        flag_files.push(FlagFile::parse(&read(&dir.join("_dumps.txt"))));
        Self {
            settings,
            flag_files,
            heels: dir.join("Heels"),
        }
    }

    /// `DynamicProvider<CustomHeel>.Load`: the file `<name>.txt` in the
    /// `Heels` folder, where `name` is the shoe's list name
    /// (`ListInfoBase.Name`). Without a file every value is zero.
    pub fn heel(&self, name: &str) -> Heel {
        Heel::parse(&heel_file(&self.heels, name).map(|p| read(&p)).unwrap_or_default())
    }
}

/// The heel file of a shoe name, found without regard to letter case as on
/// the Windows file system the plugin runs on.
fn heel_file(dir: &Path, name: &str) -> Option<PathBuf> {
    // `Path.Combine` throws on these, and the plugin then sets nothing up.
    if name.is_empty() || name.contains(|c: char| c.is_control() || "\"<>|".contains(c)) {
        return None;
    }
    let exact = dir.join(format!("{name}.txt").replace('\\', "/"));
    if exact.is_file() {
        return Some(exact);
    }
    let wanted = format!("{name}.txt").to_lowercase();
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().to_lowercase() == wanted)
        .map(|e| e.path())
}

fn turn_x(degrees: f32) -> Quat {
    Quat::from_rotation_x(degrees.to_radians())
}

/// `localRotation *= Quaternion.Euler(degrees, 0, 0)`.
fn turn(scene: &mut Scene, node: usize, degrees: f32) {
    let rot = &mut scene.nodes[node].local.rot;
    *rot *= turn_x(degrees);
}

/// `HeelInfo.PostUpdate` and `HeelInfoAnimation.Update` for a shoe that is
/// fully on (clothes state 0). `bones` is the body skeleton by name and
/// `body` the object holding `cf_j_root`, which the height moves.
pub fn pose(
    scene: &mut Scene,
    bones: &HashMap<String, usize>,
    body: usize,
    heel: &Heel,
    flags: Flags,
    custom: &CustomPose,
    settings: &Settings,
) {
    let find = |name: &str| bones.get(name).copied();
    let (Some(low_waist), Some(thigh_l), Some(thigh_r), Some(leg01_l), Some(leg01_r)) = (
        find("cf_j_waist02"),
        find("cf_j_thigh00_L"),
        find("cf_j_thigh00_R"),
        find("cf_j_leg01_L"),
        find("cf_j_leg01_R"),
    ) else {
        return;
    };
    let (Some(leg03_l), Some(leg03_r), Some(foot_l), Some(foot_r), Some(toe_l), Some(toe_r)) = (
        find("cf_j_leg03_L"),
        find("cf_j_leg03_R"),
        find("cf_j_foot_L"),
        find("cf_j_foot_R"),
        find("cf_j_toes_L"),
        find("cf_j_toes_R"),
    ) else {
        return;
    };
    let height = if flags.active && flags.height { heel.height } else { 0.0 };
    let ankle = if flags.active && flags.ankle_roll {
        heel.ankle
    } else {
        0.0
    };
    let toes = if flags.active && flags.toe_roll { heel.toes } else { 0.0 };
    let leg = if flags.active { heel.leg } else { 0.0 };
    let custom = if flags.active && flags.custom_pose {
        *custom
    } else {
        CustomPose::default()
    };
    if flags.custom_pose {
        let (world, _) = scene.world();
        let at = |node: usize| world[node].w_axis.truncate();
        let length = |a: usize, b: usize| at(a).distance(at(b));
        let by_length = |l: f32| l.powf(settings.body_length_power) * settings.body_length_multiplier;
        let by_height = height.powf(settings.heel_height_power) * settings.heel_height_multiplier;
        let waist = custom.waist * by_height;
        let thigh_r_deg = custom.right_thigh * by_height * by_length(length(low_waist, leg01_r));
        let thigh_l_deg = custom.left_thigh * by_height * by_length(length(low_waist, leg01_l));
        let leg_r_deg = custom.right_leg * by_height * by_length(length(leg01_r, leg03_r));
        let leg_l_deg = custom.left_leg * by_height * by_length(length(leg01_l, leg03_l));
        let mut turn = |node: usize, degrees: f32| turn(scene, node, degrees);
        turn(low_waist, -waist);
        turn(thigh_r, -thigh_r_deg);
        turn(thigh_l, -thigh_l_deg);
        turn(leg01_r, thigh_r_deg + leg_r_deg);
        turn(leg01_l, thigh_l_deg + leg_l_deg);
        turn(leg03_r, -leg_r_deg);
        turn(leg03_l, -leg_l_deg);
        turn(foot_r, custom.right_ankle);
        turn(foot_l, custom.left_ankle);
        scene.nodes[body].local.pos = Vec3::ZERO;
    } else {
        // Also the knee bend case: its solver is off in Studio.
        scene.nodes[body].local.pos = Vec3::new(0.0, height, 0.0);
    }
    let mut turn = |node: usize, degrees: f32| turn(scene, node, degrees);
    turn(foot_l, ankle);
    turn(foot_r, ankle);
    turn(toe_l, toes);
    turn(toe_r, toes);
    turn(leg03_l, leg);
    turn(leg03_r, leg);
}

/// The matrix `UpdateShoeWarp` puts in front of a foot bone's bind pose.
fn warp(heel: &Heel, active: bool) -> Mat4 {
    let (scale, angle, translate) = if active {
        (heel.shoe_scale, heel.shoe_angle, heel.shoe_translate)
    } else {
        (Vec3::ONE, 0.0, Vec3::ZERO)
    };
    let tilt = heel.ankle + heel.leg;
    let mut shear = Mat4::IDENTITY;
    // Unity's m12 and m21: row, then column.
    shear.z_axis.y = heel.shoe_shear_y.to_radians().tan();
    shear.y_axis.z = heel.shoe_shear_z.to_radians().tan();
    Mat4::from_quat(turn_x(-tilt))
        * shear
        * Mat4::from_translation(translate)
        * Mat4::from_scale(scale)
        * Mat4::from_quat(turn_x(tilt + angle))
}

/// `HeelInfo.UpdateShoeWarp`: the bind poses of `cf_j_foot_L` and
/// `cf_j_foot_R` in the shoe's skinned meshes are scaled, moved, turned and
/// sheared in the foot's own space. `renderers` are the worn shoe's.
pub fn warp_shoe(scene: &mut Scene, renderers: &[usize], heel: &Heel, flags: Flags) {
    if heel.keeps_shoe(flags.active) {
        return;
    }
    let warp = warp(heel, flags.active);
    let (_, active) = scene.world();
    for &ri in renderers {
        let Some(r) = scene.renderers.get(ri) else { continue };
        // `GetComponentsInChildren` skips switched off objects.
        if !r.skinned || !active[r.node] {
            continue;
        }
        let mut feet = Vec::new();
        for (i, bone) in r.bones.iter().enumerate() {
            // A missing bone ends the whole update.
            let Some(bone) = *bone else { return };
            let name = scene.nodes[bone].name.as_str();
            if (name == "cf_j_foot_L" || name == "cf_j_foot_R") && i < r.mesh.bind_poses.len() {
                feet.push(i);
            }
        }
        if feet.is_empty() {
            continue;
        }
        let r: &mut RendererInst = &mut scene.renderers[ri];
        let mut mesh = (*r.mesh).clone();
        for i in feet {
            // Stored by rows; glam reads columns.
            let bind = Mat4::from_cols_array(&mesh.bind_poses[i]).transpose();
            mesh.bind_poses[i] = (warp * bind).transpose().to_cols_array();
        }
        r.mesh = Arc::new(mesh);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Part, Trs};
    use rk_unityfs::objects::Mesh;

    const HEEL: &str = "angleAnkle=25.40322\r\nangleToes=336.0484\r\nangleLeg=10.16129\r\nheight=0.055\r\n\
        shoeScaleX=1\r\nshoeScaleY=0\r\nshoeScaleZ=1\r\nshoeAngle=0\r\nshoeTranslateY=0\r\nshoeTranslateZ=0\r\n\
        shoeShearY=0\r\nshoeShearZ=0\r\n";

    fn legs() -> (Scene, HashMap<String, usize>, usize) {
        let mut scene = Scene::default();
        let body = scene.add_node("p_cf_body_bone", None, Trs::default());
        let root = scene.add_node("cf_j_root", Some(body), Trs::default());
        let parent = scene.add_node("cf_j_waist02", Some(root), Trs::default());
        let mut bones = HashMap::from([("cf_j_waist02".to_string(), parent)]);
        for side in ["L", "R"] {
            let mut p = parent;
            for (name, y) in [
                ("thigh00", -0.1),
                ("leg01", -0.4),
                ("leg03", -0.4),
                ("foot", -0.05),
                ("toes", 0.0),
            ] {
                let local = Trs {
                    pos: Vec3::new(0.0, y, 0.0),
                    ..Default::default()
                };
                let name = format!("cf_j_{name}_{side}");
                p = scene.add_node(&name, Some(p), local);
                bones.insert(name, p);
            }
        }
        (scene, bones, body)
    }

    fn degrees_x(scene: &Scene, node: usize) -> f32 {
        let (axis, angle) = scene.nodes[node].local.rot.to_axis_angle();
        angle.to_degrees() * axis.x
    }

    #[test]
    fn a_heel_file_wraps_large_angles_and_defaults_the_scale() {
        let heel = Heel::parse(HEEL);
        assert_eq!((heel.ankle, heel.leg, heel.height), (25.40322, 10.16129, 0.055));
        assert!((heel.toes + 23.9516).abs() < 1e-3);
        // A zero scale is read as one.
        assert_eq!(heel.shoe_scale, Vec3::ONE);
        // No file: nothing moves.
        assert_eq!(Heel::parse(""), Heel::default());
        // The value ends at a second `=`; text that is no number is zero.
        assert_eq!(Heel::parse("height = 0.5=9\nangleLeg=steep").height, 0.5);
        assert_eq!(Heel::parse("angleLeg=steep").leg, 0.0);
        // 60 degrees and a hair more stay positive.
        assert_eq!(Heel::parse("angleLeg=60.001").leg, 60.001);
    }

    #[test]
    fn flags_follow_the_setters_and_the_file_order() {
        let f = Flags::parse("1,1,1,1,0,0");
        assert_eq!(f, Flags::at_start(true));
        // Knee bend and custom pose exclude each other; the later one wins.
        let f = Flags::parse("1,0,0,0,1,1");
        assert!(f.height && f.custom_pose && !f.knee_bend);
        assert_eq!(
            Flags::parse("1"),
            Flags {
                active: true,
                ..Default::default()
            }
        );

        let file = FlagFile::parse("; note\nr:=0,0,0,0,0,0\np:kind=1,1,0,0,0,0\nkind/walk=1,0,0,0,0,0\nbroken\n");
        // The start flags replace the catch-all line.
        let file = file.with_start_flags(Flags::at_start(true));
        let files = [FlagFile::default(), file];
        assert_eq!(flags_for(&files, None, None), Ok(Flags::at_start(true)));
        assert!(!flags_for(&files, Some("kind"), Some("walk")).unwrap().height);
        assert!(!flags_for(&files, Some("kind"), Some("run")).unwrap().toe_roll);
        // A file without a catch-all line gets one.
        let files = [FlagFile::default().with_start_flags(Flags::at_start(false))];
        assert_eq!(flags_for(&files, None, None), Ok(Flags::at_start(false)));
        // Nothing matches: all off.
        assert_eq!(flags_for(&[FlagFile::default()], None, None), Ok(Flags::default()));
        // Plain text patterns search the key; real expressions are refused.
        let files = [FlagFile::parse("r:walk=1,1,0,0,0,0\nr:^run$=1,1,1,1,0,0")];
        assert!(flags_for(&files, Some("kind"), Some("walk")).unwrap().height);
        assert_eq!(flags_for(&files, Some("kind"), Some("run")), Err("^run$".into()));
    }

    #[test]
    fn settings_keep_defaults_for_missing_lines() {
        let s = Settings::parse("CustomPose_HeelHeightMultiplier=16\r\nEnable_On_Start=False\r\n");
        assert_eq!(s.heel_height_multiplier, 16.0);
        assert!(!s.enable_on_start);
        assert_eq!(s.body_length_power, 0.075);
        assert_eq!(Settings::parse(""), Settings::default());
    }

    #[test]
    fn the_body_rises_and_the_feet_turn() {
        let (mut scene, bones, body) = legs();
        let heel = Heel::parse(HEEL);
        let (custom, settings) = (CustomPose::default(), Settings::default());
        pose(
            &mut scene,
            &bones,
            body,
            &heel,
            Flags::at_start(true),
            &custom,
            &settings,
        );
        assert_eq!(scene.nodes[body].local.pos, Vec3::new(0.0, 0.055, 0.0));
        for side in ["L", "R"] {
            assert!((degrees_x(&scene, bones[&format!("cf_j_foot_{side}")]) - 25.40322).abs() < 1e-3);
            assert!((degrees_x(&scene, bones[&format!("cf_j_toes_{side}")]) + 23.9516).abs() < 1e-3);
            assert!((degrees_x(&scene, bones[&format!("cf_j_leg03_{side}")]) - 10.16129).abs() < 1e-3);
            assert_eq!(
                scene.nodes[bones[&format!("cf_j_leg01_{side}")]].local.rot,
                Quat::IDENTITY
            );
        }

        // Switched off: nothing moves. Without the roll flags the lower leg
        // still turns.
        let (mut scene, bones, body) = legs();
        pose(
            &mut scene,
            &bones,
            body,
            &heel,
            Flags::at_start(false),
            &custom,
            &settings,
        );
        assert_eq!(scene.nodes[body].local.pos, Vec3::ZERO);
        assert_eq!(scene.nodes[bones["cf_j_foot_L"]].local.rot, Quat::IDENTITY);
        let flags = Flags::parse("1,0,0,0,0,0");
        pose(&mut scene, &bones, body, &heel, flags, &custom, &settings);
        assert_eq!(scene.nodes[body].local.pos, Vec3::ZERO);
        assert_eq!(scene.nodes[bones["cf_j_foot_L"]].local.rot, Quat::IDENTITY);
        assert!((degrees_x(&scene, bones["cf_j_leg03_L"]) - 10.16129).abs() < 1e-3);

        // A skeleton without the leg bones is left alone.
        let mut bare = Scene::default();
        let root = bare.add_node("root", None, Trs::default());
        pose(
            &mut bare,
            &HashMap::new(),
            root,
            &heel,
            Flags::at_start(true),
            &custom,
            &settings,
        );
        assert_eq!(bare.nodes[root].local.pos, Vec3::ZERO);
    }

    #[test]
    fn a_custom_pose_bends_the_legs_instead_of_lifting() {
        let (mut scene, bones, body) = legs();
        let heel = Heel::parse("height=0.0625");
        let custom = CustomPose {
            waist: 1.0,
            right_thigh: 2.0,
            right_leg: 1.0,
            right_ankle: 3.0,
            ..Default::default()
        };
        // Powers of one keep the numbers readable: height 0.0625 times 32
        // is 2, and a length of 0.5 or 0.4 times 10 is 5 or 4.
        let settings = Settings {
            body_length_power: 1.0,
            body_length_multiplier: 10.0,
            heel_height_power: 1.0,
            heel_height_multiplier: 32.0,
            enable_on_start: true,
        };
        pose(
            &mut scene,
            &bones,
            body,
            &heel,
            Flags::parse("1,1,0,0,0,1"),
            &custom,
            &settings,
        );
        assert_eq!(scene.nodes[body].local.pos, Vec3::ZERO);
        let x = |name: &str| degrees_x(&scene, bones[name]);
        assert!((x("cf_j_waist02") + 2.0).abs() < 1e-3);
        // Thigh: 2 * 2 * 5 = 20; leg: 1 * 2 * 4 = 8.
        assert!((x("cf_j_thigh00_R") + 20.0).abs() < 1e-3);
        assert!((x("cf_j_leg01_R") - 28.0).abs() < 1e-3);
        assert!((x("cf_j_leg03_R") + 8.0).abs() < 1e-3);
        assert!((x("cf_j_foot_R") - 3.0).abs() < 1e-3);
        assert_eq!(x("cf_j_thigh00_L"), 0.0);
    }

    fn shoe(scene: &mut Scene, bones: &HashMap<String, usize>) -> usize {
        let node = scene.add_node("shoe", None, Trs::default());
        let identity = Mat4::IDENTITY.to_cols_array();
        scene.renderers.push(RendererInst {
            name: "shoe".into(),
            node,
            part: Part::Clothes(7),
            mesh: Arc::new(Mesh {
                bind_poses: vec![identity, identity],
                ..Default::default()
            }),
            materials: Vec::new(),
            skinned: true,
            bones: vec![Some(bones["cf_j_leg03_L"]), Some(bones["cf_j_foot_L"])],
            enabled: true,
            cast_shadows: crate::scene::ShadowCasting::On,
            receive_shadows: true,
            shape_weights: Vec::new(),
            bounds: None,
            root_bone: None,
        });
        scene.renderers.len() - 1
    }

    #[test]
    fn the_shoe_warp_changes_only_the_foot_bind_poses() {
        let (mut scene, bones, _) = legs();
        let ri = shoe(&mut scene, &bones);
        let before = scene.renderers[ri].mesh.clone();

        // A heel file without warp values leaves the mesh untouched.
        warp_shoe(&mut scene, &[ri], &Heel::parse(HEEL), Flags::at_start(true));
        assert!(Arc::ptr_eq(&before, &scene.renderers[ri].mesh));

        let heel = Heel::parse("shoeScaleX=1.1\nshoeTranslateY=0.5\nshoeShearZ=45");
        warp_shoe(&mut scene, &[ri], &heel, Flags::at_start(true));
        let mesh = &scene.renderers[ri].mesh;
        assert_eq!(mesh.bind_poses[0], Mat4::IDENTITY.to_cols_array());
        // Rows: X scaled; Y moved; Z gains Y through the shear, and with it
        // the move.
        let m = mesh.bind_poses[1];
        let close = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5);
        assert!(close(&m[0..4], &[1.1, 0.0, 0.0, 0.0]));
        assert!(close(&m[4..8], &[0.0, 1.0, 0.0, 0.5]));
        assert!(close(&m[8..12], &[0.0, 1.0, 1.0, 0.5]));

        // Switched off, the scale and move go but the shear stays.
        let (mut scene, bones, _) = legs();
        let ri = shoe(&mut scene, &bones);
        warp_shoe(&mut scene, &[ri], &heel, Flags::at_start(false));
        let m = scene.renderers[ri].mesh.bind_poses[1];
        assert!(close(&m[0..4], &[1.0, 0.0, 0.0, 0.0]));
        assert!(close(&m[8..12], &[0.0, 1.0, 1.0, 0.0]));

        // The shoe angle turns the mesh around the foot's X axis.
        let (mut scene, bones, _) = legs();
        let ri = shoe(&mut scene, &bones);
        let heel = Heel::parse("angleAnkle=20\nangleLeg=10\nshoeAngle=90");
        warp_shoe(&mut scene, &[ri], &heel, Flags::at_start(true));
        let m = scene.renderers[ri].mesh.bind_poses[1];
        assert!(close(&m[4..8], &[0.0, 0.0, -1.0, 0.0]));
        assert!(close(&m[8..12], &[0.0, 1.0, 0.0, 0.0]));
    }

    #[test]
    fn heel_files_are_found_by_shoe_name() {
        let dir = std::env::temp_dir().join(format!("rk-stiletto-{}", std::process::id()));
        let heels = dir.join(CONFIG_DIR).join("Heels");
        std::fs::create_dir_all(&heels).unwrap();
        std::fs::write(heels.join("[Maker]Tall_shoes.txt"), "height=0.05\r\n").unwrap();
        std::fs::write(dir.join(CONFIG_DIR).join("_settings.txt"), "Enable_On_Start=False\r\n").unwrap();
        let config = Config::load(&dir);
        assert_eq!(config.heel("[Maker]Tall_shoes").height, 0.05);
        assert_eq!(config.heel("[maker]tall_SHOES").height, 0.05);
        assert_eq!(config.heel("Flat shoes"), Heel::default());
        assert_eq!(config.heel("a|b"), Heel::default());
        // No flags file: the start flags, here switched off.
        assert_eq!(flags_for(&config.flag_files, None, None), Ok(Flags::at_start(false)));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
