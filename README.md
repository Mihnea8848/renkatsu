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
  Dynamic Bone Editor, Dynamic Bone Distribution Editor, Slider Unlocker,
  Edit Fangs, Clothing BlendShape, Copy Cha Body Mat and Stiletto heels
- Rendering to a PNG file, side by side comparisons with captures from the
  game, and a bench that compares a whole list of cards at once

Version 0.1.1 adds Renkatsu Studio, the editor: projects that own their
characters and the mods those need, a scene with an Outliner, Properties and
a Content browser, and a live viewport running at the display's refresh rate.

## Known problems

- Characters stand in a T-pose. Posing comes next
- CharaStudio's screen effects (ambient occlusion, bloom, vignette, colour
  filter) are not drawn yet, so characters look paler than in the game
- Characters need a Koikatsu install. Renkatsu itself runs without one, and a
  later version keeps imported characters in the project so they open
  without the game too
- Koikatsu Sunshine content is not supported

## Requirements

- A GPU with Vulkan, Metal or DirectX 12
- To use Koikatsu characters, your own Koikatsu install. Renkatsu ships no
  game files and reads everything from your copy
- To build from source, Rust 1.95. `rustup` installs it on its own from
  `rust-toolchain.toml`

## Installation

Download Renkatsu from the
[releases page](https://github.com/Mihnea8848/renkatsu/releases):

- Windows: `Renkatsu-<version>-windows-x64-setup.exe` installs Renkatsu
  Studio with a Start menu entry. A portable `.zip` is there too
- Linux: `Renkatsu-<version>-linux-x86_64.AppImage` runs as it is after
  `chmod +x`. The `.tar.gz` holds the same programs with an `install.sh` that
  adds Renkatsu Studio to the application menu (`install.sh --uninstall`
  removes it)

Both include the `rk` command line tools.

To build from source instead:

```sh
git clone https://github.com/Mihnea8848/renkatsu.git
cd renkatsu
cargo install --path crates/rk-studio
cargo install --path crates/rk-cli
```

This puts `renkatsu` (the studio) and `rk` in Cargo's bin folder
(`~/.cargo/bin`).

## Configuration

Renkatsu Studio asks for your Koikatsu folder in the project selector
("Add Koikatsu folder"), and Edit, Preferences can change it. It is saved in
`renkatsu.toml` in your configuration folder (`~/.config/renkatsu` on Linux,
`%APPDATA%\Renkatsu` on Windows), where the `rk` tools find it too:

```toml
game_dir = "/path/to/Koikatsu"
```

A `renkatsu.toml` in the folder you run the tools from, or the
`RENKATSU_GAME_DIR` environment variable, take precedence.

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
| `rk-studio` | Renkatsu Studio, the editor |

## Roadmap

Renkatsu will be one app with two parts: Renkatsu Studio for scenes, and later
Renkatsu Character Maker with everything the Koikatsu maker can do.

1. Character look: Koikatsu shaders, card colours, texture compositing,
   overlays and the other plugin data that cards depend on
2. The studio: a docked editor with a viewport, scene tree and inspector,
   posing characters (static poses and CharaStudio's FK and IK), and scene
   and project files stored as readable text
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

Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848).

Renkatsu is source-available under the
[PolyForm Shield License 1.0.0](LICENSE). In short, and the license text is
what counts:

- You can use Renkatsu for free, read its code, change it, and contribute.
- You can't sell Renkatsu or a changed copy of it, and you can't use it to
  build a product that competes with it.
- What you make with Renkatsu is yours. You can sell visual novels, renders,
  videos and other work created with it.

Some files reimplement the behaviour of GPL-3.0 community plugins
(BepisPlugins, ABMX, KK_Plugins and others). Those files say
`GPL-3.0-or-later` in their header and stay under that license, whose text is
in [LICENSES/GPL-3.0-or-later.txt](LICENSES/GPL-3.0-or-later.txt).

Renkatsu ships no game files and reads everything from your own copy of
Koikatsu.
