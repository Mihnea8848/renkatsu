// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod audit;
mod bench;
mod bundle;
mod capture_check;
mod card;
mod compare;
mod custom_package;
mod map;
mod package;
mod pose;
mod reference;
mod render;
mod state;
mod ui;
mod vfs;

#[derive(Parser)]
#[command(
    name = "rk",
    version,
    about = "Renkatsu command-line tools: read Koikatsu cards, build and render characters, and compare them with the game"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Inspect character cards.
    Card {
        #[command(subcommand)]
        cmd: CardCmd,
    },
    /// Scan the game folder and all zipmods, and print a summary.
    Index,
    /// Show which game or mod item each part of a card refers to.
    Resolve {
        path: PathBuf,
        /// List every outfit instead of only the first.
        #[arg(long)]
        all: bool,
    },
    /// Render a card to a PNG image.
    Render {
        card: PathBuf,
        #[arg(short, long, default_value = "out.png")]
        out: PathBuf,
        /// Outfit number to dress the character in.
        #[arg(long, default_value_t = 0)]
        outfit: usize,
        #[arg(long, default_value_t = 900)]
        width: u32,
        #[arg(long, default_value_t = 1400)]
        height: u32,
        /// Camera angle around the character, in degrees.
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        yaw: f32,
        /// Where the camera stands, as `x,y,z` in metres (the character
        /// stands at the origin, facing +z, y up). Replaces the framing of
        /// the whole character; give `--look-at` with it.
        #[arg(long, allow_hyphen_values = true)]
        eye: Option<String>,
        /// The point the camera looks at, as `x,y,z` in metres. Without
        /// `--eye` the camera keeps its direction and turns to this point.
        #[arg(long, allow_hyphen_values = true)]
        look_at: Option<String>,
        #[arg(long)]
        no_clothes: bool,
        #[arg(long)]
        no_accessories: bool,
        /// Wear the outdoor shoes; the game starts with the indoor ones.
        #[arg(long)]
        outdoor_shoes: bool,
        /// CharaStudio's Son switch for a male character; the clothes
        /// states then say when the part shows. Without it the card's own
        /// setting counts.
        #[arg(long)]
        son: bool,
        /// Draw the game's censor over the parts it covers. An uncensor
        /// mod replaces those parts, and then there is no censor to draw.
        #[arg(long)]
        censor: bool,
        /// Keep hair, skirts and the bust in their rest pose.
        #[arg(long)]
        no_physics: bool,
        /// The Colliders plugin's switches as Studio's character panel sets
        /// them: the ones to switch on, such as `floor,skirt`, or `none`.
        /// Without it the card's own setting counts.
        #[arg(long)]
        colliders: Option<String>,
        /// Move skirts exactly as the game does, without Advanced Skirt
        /// Physics.
        #[arg(long)]
        game_skirts: bool,
        /// Move the skirts' cloth with the solver that runs on the GPU,
        /// in place of Advanced Skirt Physics' net on the CPU.
        #[arg(long, conflicts_with = "game_skirts")]
        gpu_skirts: bool,
        /// A folder to write the skirts' cloth to after every frame
        /// played, one text per frame, for the walking score.
        #[arg(long)]
        skirt_dump: Option<PathBuf>,
        /// Measure what a frame of the GPU solver costs, with this many
        /// substeps (0 for as many as it takes itself).
        #[arg(long)]
        skirt_time: Option<u32>,
        /// Where the character stands in the map, as `x,y,z` in metres.
        #[arg(long, allow_hyphen_values = true)]
        at: Option<String>,
        /// How far the character is turned round, in degrees.
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        turn: f32,
        /// Print every mesh and material that gets drawn.
        #[arg(long)]
        list: bool,
        /// Leave out the light's real-time shadows.
        #[arg(long)]
        no_shadows: bool,
        /// The states of the clothes: nine digits in the game's order, or
        /// changes such as `top=1,bottom=3` or `all=3` made in turn.
        #[arg(long)]
        clothes: Option<String>,
        /// How much liquid lies on the skin: five digits from 0 to 2 in
        /// the game's order, the face then the body's front top, front
        /// bottom, back top and back bottom.
        #[arg(long)]
        liquid: Option<String>,
        /// Leave the clothes dry. The game always wets them with the
        /// body.
        #[arg(long)]
        dry_clothes: bool,
        /// An animation of CharaStudio's list as `group/category/number`.
        #[arg(long)]
        pose: Option<String>,
        /// Where in the animation, in passes: 0 the start, 0.5 half way.
        #[arg(long, default_value_t = 0.0)]
        time: f32,
        /// Play the animation on for this many frames of 1/60 s from
        /// there, the physics moving with it, before the picture.
        #[arg(long, default_value_t = 0)]
        play: u32,
        /// While playing, also save a picture every this many frames,
        /// numbered after the output's name.
        #[arg(long, default_value_t = 0)]
        every: u32,
        /// Play this animation instead of `--pose`, which the character
        /// starts settled in: a switch from one to the other.
        #[arg(long)]
        then: Option<String>,
        /// Draw as Renkatsu Studio's viewport does: CharaStudio's screen
        /// effects (occlusion, bloom, vignette) and its first colour filter.
        #[arg(long)]
        studio_look: bool,
        /// The colour behind the character, as `r,g,b` from 0 to 1.
        #[arg(long)]
        background: Option<String>,
        /// Smooth the edges of cut-out surfaces (hair strands, lashes, lace)
        /// with alpha to coverage; the game cuts them hard.
        #[arg(long)]
        alpha_to_coverage: bool,
        /// A map to stand the character in, by its number or name, as `rk
        /// map list` shows them. The character stands at its origin.
        #[arg(long)]
        map: Option<String>,
        /// The map's time of day: day, evening or night. `--time` is taken
        /// by the animation.
        #[arg(long, default_value = "day")]
        map_time: String,
    },
    /// CharaStudio's animations: list them, check them on a character, or
    /// write one as a Renkatsu animation file.
    #[command(alias = "pose")]
    Animation {
        #[command(subcommand)]
        cmd: AnimationCmd,
    },
    /// The bones CharaStudio poses by hand (FK), as found on a card's
    /// character, by part.
    Bones { card: PathBuf },
    /// The body's inverse kinematics (FinalIK's FullBodyBipedIK).
    Ik {
        #[command(subcommand)]
        cmd: IkCmd,
    },
    /// Print which states a card's clothes have, and what a row of state
    /// changes such as `top=1 bottom=3 all=0` shows.
    Clothes {
        card: PathBuf,
        #[arg(long, default_value_t = 0)]
        outfit: usize,
        steps: Vec<String>,
    },
    /// Write a contact sheet: each card's own image beside our render.
    Compare {
        /// Cards to compare. Without any, a sample of the game folder is used.
        cards: Vec<PathBuf>,
        /// Sample size when no cards are given.
        #[arg(long, default_value_t = 20)]
        count: usize,
        /// Frame the head only.
        #[arg(long)]
        face: bool,
        #[arg(short, long, default_value = "compare.png")]
        out: PathBuf,
    },
    /// Render a card with the camera and light of a capture made in the game,
    /// and write the game picture, our render and their difference side by side.
    Reference {
        card: PathBuf,
        /// The .json file written by the capture plugin.
        capture: PathBuf,
        #[arg(short, long, default_value = "reference.png")]
        out: PathBuf,
        /// Keep hair, skirts and the bust in their rest pose.
        #[arg(long)]
        no_physics: bool,
        /// Frames of physics to run; the capture waits 90.
        #[arg(long)]
        settle_frames: Option<u32>,
        /// Also list the bones whose name contains one of these texts.
        #[arg(long)]
        bones: Vec<String>,
        /// Leave out the light's real-time shadows.
        #[arg(long)]
        no_shadows: bool,
        /// Compare with the picture taken with Studio's screen effects on
        /// (`<name>.<view>.studio.png`), with the scene's effects on ours.
        #[arg(long)]
        studio: bool,
        /// Smooth the edges of cut-out surfaces with alpha to coverage.
        #[arg(long)]
        alpha_to_coverage: bool,
    },
    /// Build many cards and report failures, warnings and unsupported data.
    Audit {
        /// How many cards to sample from the game folder; a number above the
        /// total takes every card.
        #[arg(long, default_value_t = 150)]
        count: usize,
        #[arg(long, default_value_t = 4)]
        threads: usize,
    },
    /// Compare every card in a list with its capture: one line per card,
    /// then the data differences grouped across all cards.
    Bench {
        /// Card paths relative to UserData/chara/female, one per line.
        list: PathBuf,
        /// Folder with the captures (<card file stem>.json and pictures).
        #[arg(long, default_value = "renders/reference")]
        captures: PathBuf,
        /// Where the comparison pictures go.
        #[arg(long, default_value = "renders/scratch/bench")]
        out: PathBuf,
        /// Also print each card's differences of this kind or about this
        /// property or texture, such as `_MainTex` or `skinning bones`.
        #[arg(long)]
        show: Vec<String>,
        /// Compare with the pictures taken with Studio's screen effects on.
        #[arg(long)]
        studio: bool,
        /// Move skirts exactly as the game does, without Advanced Skirt
        /// Physics, to compare the rest with the game.
        #[arg(long)]
        game_skirts: bool,
    },
    /// Compare every case of a capture batch (the plugin's `out/batch`
    /// folder) with our render of it: one line per case, with the camera,
    /// light, outfit, clothing states and pose the capture recorded. Cases
    /// taken in a map are listed with what they recorded.
    CaptureCheck {
        /// Folder with the batch's .json files and pictures.
        dir: PathBuf,
        /// Where the comparison pictures go.
        #[arg(long, default_value = "renders/scratch/capture-check")]
        out: PathBuf,
        /// Only the cases whose file name contains one of these texts.
        #[arg(long)]
        only: Vec<String>,
        /// Compare with the pictures taken with Studio's screen effects on.
        #[arg(long)]
        studio: bool,
        /// Smooth the edges of cut-out surfaces with alpha to coverage.
        #[arg(long)]
        alpha_to_coverage: bool,
        /// Move skirts exactly as the game does, without Advanced Skirt
        /// Physics.
        #[arg(long)]
        game_skirts: bool,
    },
    /// The maps CharaStudio lists and what their scenes hold.
    Map {
        #[command(subcommand)]
        cmd: MapCmd,
    },
    /// Inspect Unity asset bundle files.
    Bundle {
        #[command(subcommand)]
        cmd: BundleCmd,
    },
    /// Build, install and check packages such as Starter Content.
    Package {
        #[command(subcommand)]
        cmd: PackageCmd,
    },
}

#[derive(Subcommand)]
enum AnimationCmd {
    /// List the categories, or with `--all` every animation with whether
    /// it holds one pose or moves.
    List {
        #[arg(long)]
        all: bool,
    },
    /// Read every animation and bind it to a card's skeleton.
    Check { card: PathBuf },
    /// List the installed mods that bring animation lists, with their size.
    Mods,
    /// Write animations of the list as Renkatsu animation files
    /// (`.rkanim`), the files a project and a package's `animations`
    /// folder hold. Without `--number`, the whole category.
    Export {
        /// The group's number, as `rk animation list` prints it.
        group: i32,
        /// The category's number.
        category: i32,
        #[arg(long)]
        number: Option<i32>,
        /// The folder the files go into, under `<Group>/<Category>/`.
        #[arg(short, long, default_value = ".")]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum PackageCmd {
    /// Build Starter Content: Characters & Clothing from the Koikatsu install: the
    /// game's own character parts and default characters, without mods.
    StarterCharacters {
        /// The folder the package is written to.
        #[arg(long)]
        out: PathBuf,
        /// Only say what the package would hold.
        #[arg(long)]
        list: bool,
        /// Cut the package into parts of at most this many megabytes.
        #[arg(long)]
        part_size: Option<u64>,
    },
    /// Start a package of your own: a folder with its package.toml and the
    /// folders content goes in.
    New {
        /// The folder to make; it must be new or empty.
        dir: PathBuf,
        /// The package's id: lower case letters, digits and dashes.
        #[arg(long)]
        id: String,
        /// The name people read; the id with spaces when left out.
        #[arg(long)]
        name: Option<String>,
    },
    /// Build a package folder made with `rk package new` into a .rkpkg.
    Build {
        /// The package folder, with its package.toml.
        dir: PathBuf,
        /// The folder the .rkpkg is written to.
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    /// Build Starter Content: Animations & Poses from the Koikatsu install:
    /// CharaStudio's poses and animations and the H scenes', without mods.
    StarterPoses {
        /// The folder the package file is written to.
        #[arg(long)]
        out: PathBuf,
        /// Only say what the package would hold.
        #[arg(long)]
        list: bool,
        /// Cut the package into parts of at most this many megabytes.
        #[arg(long)]
        part_size: Option<u64>,
    },
    /// Build Starter Content: Maps from the Koikatsu install: CharaStudio's
    /// maps with their pictures, without mods.
    StarterMaps {
        /// The folder the package file is written to.
        #[arg(long)]
        out: PathBuf,
        /// Only say what the package would hold.
        #[arg(long)]
        list: bool,
        /// Cut the package into parts of at most this many megabytes.
        #[arg(long)]
        part_size: Option<u64>,
    },
    /// Install a package from its file.
    Install { file: PathBuf },
    /// Download a package Renkatsu knows about and install it.
    Download { id: String },
    /// List the installed packages and the ones that can be downloaded.
    List,
    /// Check an installed package against its checksums.
    Verify { id: String },
    /// Remove an installed package.
    Remove { id: String },
}

#[derive(Subcommand)]
enum MapCmd {
    /// List the maps.
    Ls {
        /// Also list maps whose bundle is not installed.
        #[arg(long)]
        missing: bool,
        /// Open every map and show what it holds.
        #[arg(long)]
        scan: bool,
    },
    /// Show what one map holds, by its number or level name.
    Info { map: String },
    /// Write a map's file: the map at each of its times of day, ready to
    /// open without the game.
    Convert {
        /// The map's number, level or name.
        map: String,
        /// Only these times of day: day, evening, night.
        #[arg(long, value_delimiter = ',')]
        time: Vec<String>,
        #[arg(short, long)]
        out: PathBuf,
        /// Read each time of day back and compare it with a fresh build.
        #[arg(long)]
        check: bool,
    },
    /// Draw a map to a PNG, plainly lit.
    Render {
        /// The map's number or level, or the path of a map file (.rkmap).
        map: String,
        #[arg(short, long)]
        out: PathBuf,
        /// day, evening or night.
        #[arg(long, default_value = "day")]
        time: String,
        /// inside (eye height, looking across) or above (cut away).
        #[arg(long, default_value = "inside")]
        view: String,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 720)]
        height: u32,
        /// Draw the map's own fog, which CharaStudio leaves off.
        #[arg(long)]
        fog: bool,
        /// Draw the scene's Unity skybox, which CharaStudio's camera leaves
        /// out; the map's own sky dome is drawn either way.
        #[arg(long)]
        sky: bool,
        /// A CharaStudio capture's JSON: its view named by --view is the
        /// camera, at the capture's size.
        #[arg(long)]
        capture: Option<PathBuf>,
        /// Pose what the map's Animators move, this many seconds after
        /// the map loaded. Without it, everything is drawn as baked.
        #[arg(long)]
        motion: Option<f32>,
        /// Where the camera stands, in place of the view's: x,y,z.
        #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
        eye: Vec<f32>,
        /// What the camera looks at, with --eye: x,y,z.
        #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
        target: Vec<f32>,
        /// The sun turned by hand, in place of the map's: heading round
        /// the up axis from +Z toward +X and elevation, in degrees.
        #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
        sun: Vec<f32>,
    },
    /// Show a map's Animators and the objects they move.
    Motion {
        /// The map's number, level or name.
        map: String,
        /// day, evening or night.
        #[arg(long, default_value = "day")]
        time: String,
        /// Also print where the moved objects stand at these seconds.
        #[arg(long, value_delimiter = ',')]
        at: Vec<f32>,
    },
}

#[derive(Subcommand)]
enum BundleCmd {
    /// List the objects in a bundle.
    Ls {
        path: PathBuf,
        /// Only show objects of this Unity class id.
        #[arg(long)]
        class: Option<i32>,
    },
    /// Print one object as JSON, selected by path id or name.
    Dump {
        path: PathBuf,
        #[arg(long, allow_hyphen_values = true)]
        id: Option<i64>,
        #[arg(long)]
        name: Option<String>,
        /// Print the type layout instead of the values.
        #[arg(long)]
        tree: bool,
        #[arg(long, default_value_t = 8)]
        max_items: usize,
    },
}

#[derive(Subcommand)]
enum IkCmd {
    /// Print the FullBodyBipedIK settings the game's body stores, with a
    /// card's character for the bones.
    Settings {
        card: PathBuf,
        /// A `.ik.json` of the capture plugin: say where CharaStudio's
        /// solver had other settings.
        #[arg(long)]
        capture: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum CardCmd {
    /// Print a card's contents as JSON.
    Dump {
        path: PathBuf,
        /// Include plugin data in full instead of a summary.
        #[arg(long)]
        ext: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Card {
            cmd: CardCmd::Dump { path, ext },
        } => card::dump(&path, ext),
        Cmd::Index => vfs::index(),
        Cmd::Package { cmd } => match cmd {
            PackageCmd::StarterCharacters { out, list, part_size } => {
                package::starter_characters(&out, list, part_size.map(|mb| mb * 1024 * 1024))
            }
            PackageCmd::New { dir, id, name } => custom_package::new(&dir, &id, name.as_deref()),
            PackageCmd::Build { dir, out } => custom_package::build(&dir, &out),
            PackageCmd::StarterPoses { out, list, part_size } => {
                package::starter_poses(&out, list, part_size.map(|mb| mb * 1024 * 1024))
            }
            PackageCmd::StarterMaps { out, list, part_size } => {
                package::starter_maps(&out, list, part_size.map(|mb| mb * 1024 * 1024))
            }
            PackageCmd::Install { file } => package::install(&file),
            PackageCmd::Download { id } => package::download(&id),
            PackageCmd::List => package::list(),
            PackageCmd::Verify { id } => package::verify(&id),
            PackageCmd::Remove { id } => package::remove(&id),
        },
        Cmd::Reference {
            card,
            capture,
            out,
            no_physics,
            settle_frames,
            bones,
            no_shadows,
            studio,
            alpha_to_coverage,
        } => {
            // Captures are made in Studio, which is also the default.
            let mut opts = rk_chara::BuildOptions {
                physics: !no_physics,
                ..Default::default()
            };
            if let Some(frames) = settle_frames {
                opts.settle_frames = frames;
            }
            reference::reference(
                &card,
                &capture,
                &out,
                &opts,
                &bones,
                !no_shadows,
                studio,
                alpha_to_coverage,
            )
        }
        Cmd::Audit { count, threads } => audit::audit(count, threads),
        Cmd::Bench {
            list,
            captures,
            out,
            show,
            studio,
            game_skirts,
        } => bench::bench(&list, &captures, &out, &show, studio, game_skirts),
        Cmd::CaptureCheck {
            dir,
            out,
            only,
            studio,
            alpha_to_coverage,
            game_skirts,
        } => capture_check::check(&dir, &out, &only, studio, alpha_to_coverage, game_skirts),
        Cmd::Compare {
            cards,
            count,
            face,
            out,
        } => compare::compare(cards, count, face, &out),
        Cmd::Render {
            card,
            out,
            outfit,
            width,
            height,
            yaw,
            eye,
            look_at,
            no_clothes,
            no_accessories,
            outdoor_shoes,
            son,
            censor,
            no_physics,
            colliders,
            game_skirts,
            gpu_skirts,
            skirt_dump,
            skirt_time,
            at,
            turn,
            list,
            no_shadows,
            clothes,
            liquid,
            dry_clothes,
            pose,
            time,
            play,
            every,
            then,
            studio_look,
            background,
            alpha_to_coverage,
            map,
            map_time,
        } => render::render(render::Args {
            card: &card,
            out: &out,
            outfit,
            width,
            height,
            yaw,
            eye: eye.as_deref(),
            look_at: look_at.as_deref(),
            no_clothes,
            no_accessories,
            outdoor_shoes,
            son,
            censor,
            no_physics,
            colliders: colliders.as_deref(),
            game_skirts,
            gpu_skirts,
            skirt_dump: skirt_dump.as_deref(),
            skirt_time,
            at: at.as_deref(),
            turn,
            list,
            shadows: !no_shadows,
            clothes: clothes.as_deref(),
            liquid: liquid.as_deref(),
            dry_clothes,
            pose: pose.as_deref(),
            time,
            play,
            every,
            then: then.as_deref(),
            studio_look,
            background: background.as_deref(),
            alpha_to_coverage,
            map: map.as_deref(),
            map_time: &map_time,
        }),
        Cmd::Bones { card } => pose::bones(&card),
        Cmd::Ik {
            cmd: IkCmd::Settings { card, capture },
        } => pose::ik_settings(&card, capture.as_deref()),
        Cmd::Animation {
            cmd: AnimationCmd::List { all },
        } => pose::list(all),
        Cmd::Animation {
            cmd: AnimationCmd::Check { card },
        } => pose::check(&card),
        Cmd::Animation {
            cmd: AnimationCmd::Mods,
        } => pose::mods(),
        Cmd::Animation {
            cmd:
                AnimationCmd::Export {
                    group,
                    category,
                    number,
                    out,
                },
        } => pose::export(group, category, number, &out),
        Cmd::Clothes { card, outfit, steps } => pose::clothes(&card, outfit, &steps),
        Cmd::Resolve { path, all } => vfs::resolve(&path, all),
        Cmd::Map {
            cmd: MapCmd::Ls { missing, scan },
        } => map::ls(missing, scan),
        Cmd::Map {
            cmd: MapCmd::Info { map },
        } => map::info(&map),
        Cmd::Map {
            cmd: MapCmd::Convert { map, time, out, check },
        } => map::convert(&map, &time, &out, check),
        Cmd::Map {
            cmd:
                MapCmd::Render {
                    map,
                    out,
                    time,
                    view,
                    width,
                    height,
                    fog,
                    sky,
                    capture,
                    motion,
                    eye,
                    target,
                    sun,
                },
        } => map::render(
            &map,
            &time,
            &view,
            &out,
            (width, height),
            map::RenderOptions {
                fog,
                sky,
                capture: capture.as_deref(),
                motion,
                look: match (eye.as_slice(), target.as_slice()) {
                    ([a, b, c], [x, y, z]) => Some(([*a, *b, *c], [*x, *y, *z])),
                    _ => None,
                },
                sun: match sun.as_slice() {
                    [heading, elevation] => Some([*heading, *elevation]),
                    _ => None,
                },
            },
        ),
        Cmd::Map {
            cmd: MapCmd::Motion { map, time, at },
        } => map::motion(&map, &time, &at),
        Cmd::Bundle {
            cmd: BundleCmd::Ls { path, class },
        } => bundle::ls(&path, class),
        Cmd::Bundle {
            cmd:
                BundleCmd::Dump {
                    path,
                    id,
                    name,
                    tree,
                    max_items,
                },
        } => bundle::dump(&path, id, name.as_deref(), tree, max_items),
    }
}
