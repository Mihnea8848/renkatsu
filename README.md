# Renkatsu

[![CI](https://github.com/Mihnea8848/renkatsu/actions/workflows/ci.yml/badge.svg)](https://github.com/Mihnea8848/renkatsu/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/Mihnea8848/renkatsu)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-wgpu-b7410e?logo=rust&logoColor=white)](#how-it-works)

[![Support me on Ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/mihnea8848)

Renkatsu is a game engine for Koikatsu content, written from scratch in Rust
on wgpu. It reads character cards, zipmods and the data that plugins save into
cards, and draws the characters with its own renderer. It does not use Unity
and does not patch the game.

## Why I made this

Koikatsu is no longer developed. The community keeps it alive with patches and
mods on an old Unity version, and that has limits nobody can fix from the
outside.

Visual novels made with Koikatsu characters have the same problem. The usual
way is to pose a scene in CharaStudio, take a screenshot and put the image in
Ren'Py. After that the image is dead: if a character changes later, every old
scene has to be opened and rendered again. I want scenes that stay live, so a
change to a character shows up everywhere it is used.

That needs an engine that understands Koikatsu content on its own. This is my
attempt at one.

## Status

Version 0.1.0 covers character import. Renkatsu builds a character from a card
the way Koikatsu and its plugins do, and draws it with the game's shaders. On
41 reference cards, pictures match CharaStudio at about 99.1% for the full body
and 99.3% for the face, counting colour error per pixel. Every change is
checked against captures made in CharaStudio with the same camera and light,
and the formulas come from the game's own code and shaders, not from tuning.

## What works

- Reading character cards and coordinate files
- Reading Unity asset bundle files. This is a file format reader, no Unity
  code is involved
- Indexing the game's `abdata` folder and all zipmods, with the item lists and
  loose textures, the way Sideloader does, including which mod wins when
  several replace the same file
- Finding the game or mod item for every ID in a card, including items that
  moved between mods
- Building the character: skeleton, body, head, hair, clothes and accessories,
  with body and face sliders
- The game's shaders for skin, hair, eyes, face lines, clothes and items, and
  xukmi's Vanilla Plus shaders, in linear colour like the game, with the
  scene's ramp, shadow and line settings
- Real-time shadows from the key light, and hair, skirt, ribbon and bust
  physics as the game's DynamicBone runs them
- Card colours, makeup, skin and clothes textures, normal maps, liquid layers,
  patterns and emblems
- Clothes that hide what is under them
- Eyes and brows drawn in front of or behind the hair as the game does, with
  Better Repack's defaults
- Expressions, gaze, iris size and tilt from the card
- Plugin data: overlays,
  MaterialEditor (properties, textures, shader swaps with their defaults,
  material copies), MoreAccessories, ABMX bone edits, Hair Accessory
  Customizer, Pushup, Uncensor Selector, Clothing Unlocker, Accessory States,
  Accessory Clothes, Eye Control, Pregnancy, Colliders, Invisible Body,
  Dynamic Bone Editor and Slider Unlocker
- Rendering to a PNG file, side by side comparisons with captures from the
  game, and a bench that compares a whole list of cards at once

## Known problems

- `rk render` draws the character in a T-pose. Poses only come from captures
  for now
- CharaStudio's screen effects (ambient occlusion, bloom, vignette, colour
  filter) are not drawn yet
- There is no editor and no window, only command line tools
- Koikatsu Sunshine content is not supported

## Requirements

- Rust 1.95. `rustup` installs it on its own from `rust-toolchain.toml`
- A GPU with Vulkan, Metal or DirectX 12
- Your own Koikatsu install. Renkatsu ships no game files and reads everything
  from your copy

## Installation

```sh
git clone https://github.com/Mihnea8848/renkatsu.git
cd renkatsu
cargo install --path crates/rk-cli
```

This builds the `rk` command and puts it in Cargo's bin folder
(`~/.cargo/bin`). While working on Renkatsu itself, `cargo build --release`
and `./target/release/rk` do the same without installing.

## Configuration

Create a `renkatsu.toml` in the folder you run the tools from:

```toml
game_dir = "/path/to/Koikatsu"
```

You can also set the `RENKATSU_GAME_DIR` environment variable.

## Usage

Scan the game folder and the mods, and print a summary. The first run takes a
while, later runs use a cache:

```sh
rk index
```

Show which game or mod item each part of a card uses, and what is missing:

```sh
rk resolve path/to/card.png
```

Render a card:

```sh
rk render path/to/card.png -o out.png
```

`--outfit 2` picks another outfit, `--yaw 30` turns the camera and
`--outdoor-shoes` puts on the outdoor shoes.

Compare a card with a capture made in CharaStudio by the reference capture
plugin in `tools/reference-capture`:

```sh
rk reference path/to/card.png capture.json -o compare.png
```

It prints how much each view matches the game, in percent, and every value
that differs from the game's: material properties, textures, shadow flags and
skinning bones.

Compare a whole list of cards with their captures. The list holds card paths
relative to `UserData/chara/female`, one per line:

```sh
rk bench cards.txt --captures captures/
```

Each card gets one line with its match percentages, and the run ends with the
averages and the differences grouped by what they are about. `--show _Color`
also prints each card's differences of that kind.

Build many cards and report failures, warnings and plugin data that is not
supported yet:

```sh
rk audit --count 200 --threads 2
```

## How it works

The code is split into crates, each with one job:

| Crate | Job |
|---|---|
| `rk-card` | Card and coordinate file parser |
| `rk-unityfs` | Unity asset bundle reader |
| `rk-vfs` | Index of game files and zipmods, item lists, ID lookup |
| `rk-chara` | Builds a character from a card |
| `rk-render` | wgpu renderer |
| `rk-cli` | Command line tools |

## Roadmap

Renkatsu will be one app with two parts: Renkatsu Studio for scenes, and later
Renkatsu Character Maker with everything the Koikatsu maker can do.

1. Character look: Koikatsu shaders, card colours, texture compositing,
   overlays and the other plugin data that cards depend on
2. The studio: a docked editor with a viewport, scene tree and inspector, and
   scene and project files stored as readable text
3. Import of existing CharaStudio scenes
4. The Ren'Py link: baked images and video that are rendered again when a
   character changes
5. Animation
6. Maps from glTF and FBX files, and a Renkatsu map format for sharing
7. The character maker
8. Games in real 3D for desktop and Android
9. Koikatsu Sunshine content
10. Renkatsu's own shading: a modern anime look in the spirit of games such
    as Arknights: Endfield, offered beside the Koikatsu look. This comes
    last, once the engine, the editor and the main features are done

The default look will stay anime style, with real shadows and better lighting
than the game has.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Test results with your own cards and
mods help a lot, because I can only test with what I have installed.

If this project is useful to you and you want to support it, you can buy me a
coffee on [Ko-fi](https://ko-fi.com/mihnea8848).

## License

Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848). Free software under the GNU
General Public License, version 3 or later: you can use, change and share it,
and anything you distribute based on it must stay under the same license, with
source code. See [LICENSE](LICENSE).

The plugin behaviour that Renkatsu
reimplements comes from GPL-3.0 community plugins (BepisPlugins, ABMX,
KK_Plugins).
