# Renkatsu

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

Early. A card goes in and an image comes out, but the image does not look like
the game yet.

## What works

- Reading character cards and coordinate files
- Reading Unity asset bundle files. This is a file format reader, no Unity
  code is involved
- Indexing the game's `abdata` folder and all zipmods, with the item lists
- Finding the game or mod item for every ID in a card, the way Sideloader
  does, including items that moved between mods
- Building the character: skeleton, body, head, hair, clothes and accessories
- Body and face sliders
- Bone edits saved by ABMX
- Rendering to a PNG file with simple shading

## Known problems

- The Koikatsu shaders are not implemented. Colours from the card, makeup,
  skin and clothes textures are missing or wrong
- Clothes do not hide the body or underwear below them
- Overlays, MaterialEditor, MoreAccessories and other plugin data are ignored
- The character stands in a T-pose. Nothing moves
- There is no editor and no window, only command line tools
- Koikatsu Sunshine content is not supported

## Requirements

- A recent stable Rust toolchain. I build with 1.95
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

`--outfit 2` picks another outfit and `--yaw 30` turns the camera.

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
5. Animation and dynamic bones
6. Maps from glTF and FBX files, and a Renkatsu map format for sharing
7. The character maker
8. Games in real 3D for desktop and Android
9. Koikatsu Sunshine content

The default look will stay anime style, with real shadows and better lighting
than the game has.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Test results with your own cards and
mods help a lot, because I can only test with what I have installed.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE). The plugin behaviour that Renkatsu
reimplements comes from GPL-3.0 community plugins (BepisPlugins, ABMX,
KK_Plugins).
