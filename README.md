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

Early, but characters now look close to the game. Every change is checked
against pictures taken in CharaStudio with the same camera and light, and the
formulas come from the game's own code and shaders, not from tuning.

## What works

- Reading character cards and coordinate files
- Reading Unity asset bundle files. This is a file format reader, no Unity
  code is involved
- Indexing the game's `abdata` folder and all zipmods, with the item lists and
  loose textures, the way Sideloader does
- Finding the game or mod item for every ID in a card, including items that
  moved between mods
- Building the character: skeleton, body, head, hair, clothes and accessories,
  with body and face sliders
- The game's shaders for skin, hair, eyes, face lines and clothes, in linear
  colour like the game, with the scene's ramp, shadow and line settings
- Card colours, makeup, skin and clothes textures, patterns and emblems
- Clothes that hide what is under them
- Eyes and brows drawn in front of or behind the hair as the game does, with
  Better Repack's defaults
- Expressions, gaze and iris size from the card
- Plugin data: overlays, MaterialEditor (properties, textures, shader swaps,
  material copies), MoreAccessories, ABMX bone edits, Hair Accessory
  Customizer and Pushup
- Rendering to a PNG file, and side by side comparisons with captures from
  the game

## Known problems

- No physics yet: hair, skirts and ribbons stay in their rest pose (in
  progress)
- No shadow map, so hair does not shade the face
- Accessories use an approximate shader, and clothes outlines the old one
- The character stands in a T-pose
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
cargo build --release
```

## Configuration

Create a `renkatsu.toml` in the folder you run the tools from:

```toml
game_dir = "/path/to/Koikatsu"
```

You can also set the `RENKATSU_GAME_DIR` environment variable.

## Usage

Scan the game folder and the mods, and print a summary. The first run takes a
few seconds, later runs use a cache:

```sh
cargo run --release -p rk-cli -- index
```

Show which game or mod item each part of a card uses, and what is missing:

```sh
cargo run --release -p rk-cli -- resolve path/to/card.png
```

Render a card:

```sh
cargo run --release -p rk-cli -- render path/to/card.png -o out.png
```

`--outfit 2` picks another outfit, `--yaw 30` turns the camera and
`--outdoor-shoes` puts on the outdoor shoes.

Compare a card with a capture made in CharaStudio by the reference capture
plugin in `tools/reference-capture`:

```sh
cargo run --release -p rk-cli -- reference path/to/card.png capture.json -o compare.png
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
