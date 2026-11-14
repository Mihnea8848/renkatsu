// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod audit;
mod bundle;
mod card;
mod compare;
mod reference;
mod render;
mod vfs;

#[derive(Parser)]
#[command(name = "rk", about = "Renkatsu command-line tools")]
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
        #[arg(long)]
        no_clothes: bool,
        #[arg(long)]
        no_accessories: bool,
        /// Wear the outdoor shoes; the game starts with the indoor ones.
        #[arg(long)]
        outdoor_shoes: bool,
        /// Print every mesh and material that gets drawn.
        #[arg(long)]
        list: bool,
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
    },
    /// Build many cards and report failures, warnings and unsupported data.
    Audit {
        /// How many cards to sample from the game folder.
        #[arg(long, default_value_t = 150)]
        count: usize,
        #[arg(long, default_value_t = 4)]
        threads: usize,
    },
    /// Inspect Unity asset bundle files.
    Bundle {
        #[command(subcommand)]
        cmd: BundleCmd,
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
        Cmd::Reference { card, capture, out } => reference::reference(&card, &capture, &out),
        Cmd::Audit { count, threads } => audit::audit(count, threads),
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
            no_clothes,
            no_accessories,
            outdoor_shoes,
            list,
        } => render::render(render::Args {
            card: &card,
            out: &out,
            outfit,
            width,
            height,
            yaw,
            no_clothes,
            no_accessories,
            outdoor_shoes,
            list,
        }),
        Cmd::Resolve { path, all } => vfs::resolve(&path, all),
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
