# Renkatsu

[![Latest release](https://img.shields.io/github/v/release/Mihnea8848/renkatsu?label=latest)](https://github.com/Mihnea8848/renkatsu/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/Mihnea8848/renkatsu/total)](https://github.com/Mihnea8848/renkatsu/releases)
[![Windows and Linux](https://img.shields.io/badge/Windows%20%7C%20Linux-installers-2f6fd0)](#installation)
[![Works without Koikatsu](https://img.shields.io/badge/Koikatsu-not%20required-3c8c5a)](#requirements)
[![License: PolyForm Shield 1.0](https://img.shields.io/badge/license-PolyForm%20Shield%201.0-555)](#license)

[![Support me on Ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/mihnea8848)

Renkatsu is a game engine and editor for Koikatsu content, written from
scratch in Rust on wgpu. It reads character cards, zipmods and the data that
plugins save into cards, and draws the characters with its own renderer. It
does not use Unity and does not patch the game.

Renkatsu Studio is the editor: projects that own their characters, a scene
you build in a live viewport, and installers for Windows and Linux.

<p align="center">
  <img src="docs/images/studio.png" alt="Renkatsu Studio with a character in the viewport and the project's characters below" width="900">
</p>

Get the installer for Windows or Linux from the
[latest release](https://github.com/Mihnea8848/renkatsu/releases/latest).
You don't need Koikatsu: on first start Renkatsu offers Starter Content, the
game's own characters, clothing and animations, as a separate download. If
you have Koikatsu, Renkatsu reads your cards and mods from it too.

<p align="center">
  <img src="docs/images/import-characters.png" alt="Importing characters from Starter Content into a project" width="900">
</p>

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

The latest version is 0.2.0, on the
[releases page](https://github.com/Mihnea8848/renkatsu/releases).

- 0.1.0 covered character import. Renkatsu builds a character from a card the
  way Koikatsu and its plugins do, and draws it with the game's shaders. On
  41 reference cards, pictures match CharaStudio at about 99.1% for the full
  body and 99.3% for the face, counting colour error per pixel. Every change
  is checked against captures made in CharaStudio with the same camera and
  light, and the formulas come from the game's own code and shaders, not from
  tuning.
- 0.1.1 adds Renkatsu Studio, the editor, with installers.
- 0.1.2 draws CharaStudio's screen effects (ambient occlusion, bloom, the
  colour filter and the vignette), so characters look as they do in Studio:
  99.0% body and 99.4% face against captures with the effects on. It adds
  packages, and Starter Content: Characters & Clothing, Koikatsu's own vanilla
  character content, so Renkatsu works without a Koikatsu install.
- 0.1.3 checks for new versions and keeps the editor's grey backdrop.
- 0.1.4 toggles clothing as CharaStudio does and poses characters with the
  game's 716 animations, also from Starter Content: Animations, and lets anyone
  make packages.
- 0.1.5 keeps animations in the project with an importer that previews
  them, and adds context menus in Content and the Outliner.
- 0.1.6 reads the rest of the game's animations, 5728 in all with a Koikatsu
  folder, and names them in English. Starter Content: Poses is now Starter
  Content: Animations & Poses and carries 3860 of them.
- 0.1.7 keeps who a character is beside how it looks: the card's name,
  personality, birthday, club, traits and likes, changed per character in the
  scene without touching the card. The animation list puts the H scenes' nine
  groups under one row of their own.
- 0.1.8 moves, rotates and scales objects in the viewport with gizmos, in
  global or local axes, with snapping steps of your own. Objects are selected
  with a click or a box, and every shortcut can be changed under Help.
- 0.2.0 adds maps. CharaStudio's maps are drawn with their lightmaps, sun
  shadows, sky, reflections and walls that hide, at each time of day, and
  are picked from a library with pictures. A project keeps its maps as
  `.rkmap` files, which open without Koikatsu. Objects drop onto the floor
  or the map below them, attach to each other and to a character's bones,
  and snap to a grid. Loading runs in the background, and a home page shows
  the news.
- Next: posing by hand (FK and IK), then baked characters.

## Renkatsu Studio

- A project selector at start, with recent projects as tiles or a list, each
  with its icon or a picture of its last scene
- Projects are plain folders: a readable `.rkproj` file with Characters,
  Animations, Poses, Maps, Items, Scenes, Renders and Mods folders, so a
  project can be moved, copied or backed up as it is
- Importing a card copies it into the project with every mod it needs, after
  showing how many and how large they are. A project uses its own mods when
  it opens
- New projects can start with Renkatsu-Chan, the game's default character
- A scene with an Outliner, Properties and a Content browser: several
  characters, the key light, folders, undo and redo, rename, hide, duplicate
  and delete
- A live viewport at the display's refresh rate, with physics, Unity style
  navigation and eased focus, and characters skinned on the GPU
- Clothing toggles per category as in CharaStudio: on, half off and off,
  with the game's own rules for which pieces pull others along
- Animations, static or moving, with speed and start; play, pause and stop
  drive animations and physics together. A project owns its animations:
  they are imported from Koikatsu, packages and mods through a window with
  sources, groups and categories, a search and a live preview, and copied
  into the project so it plays them anywhere
- Context menus in Content and the Outliner: rename, duplicate, copy, cut,
  paste, delete, folders, and drag and drop; scenes follow files that are
  renamed or moved
- Packages: Starter Content brings Koikatsu's vanilla characters, clothing
  and animations without the game, and `rk package new` and `rk package build`
  make your own ([docs/packages.md](docs/packages.md))
- Maps: CharaStudio's maps from a Koikatsu install with lightmaps, sun
  shadows, sky domes, foliage, reflections, normal maps and walls that hide,
  in day, evening and night, picked from a library grouped by place. A
  project keeps each map as one `.rkmap` file, which can also be imported
  from disk
- Snap to floor drops objects onto what lies below them, characters or the
  map; a grid to snap moves to; objects attach to other objects and to a
  character's bones, and follow them while animations play
- Loading runs in the background with its progress shown, and a home page
  shows the news
- Install Update downloads a new version, installs it and starts it again
- Unreal Engine 5's dark look, with the window's own title bar and border
- Renkatsu runs without Koikatsu. The Koikatsu folder, chosen in the project
  selector or in Preferences, is only needed for Koikatsu characters

## Characters

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

## Known problems

- Characters take animations, static and moving, but cannot be posed by hand
  yet (FK and IK come next), and community pose files cannot be imported yet
- Without a Koikatsu install, characters come from Starter Content only.
  Cards that use mods need the mods, which come with a Koikatsu install
- Characters are not lit by a map's sun yet and cast no shadows on the
  map, and maps from mods are not read yet
- CharaStudio's depth of field, fog and sun shafts (off by default) are not
  drawn yet
- Koikatsu Sunshine content is not supported

## Requirements

- A GPU with Vulkan, Metal or DirectX 12
- Characters come from Starter Content (a download Renkatsu offers on first
  start) or from your own Koikatsu install, which Renkatsu reads and never
  changes
- To build from source, Rust 1.95. `rustup` installs it on its own from
  `rust-toolchain.toml`

## Installation

Download Renkatsu from the
[releases page](https://github.com/Mihnea8848/renkatsu/releases):

- Windows: `Renkatsu-<version>-windows-x64-setup.exe` installs Renkatsu
  Studio with a Start menu entry. A portable `.zip` is there too
- Linux: `Renkatsu-<version>-linux-x86_64-installer` installs Renkatsu
  Studio for your user (run it after `chmod +x`, or open it from the file
  manager), with an application menu entry and an optional desktop
  shortcut. `uninstall.sh` in `~/.local/share/renkatsu/app` removes it. The
  portable `.tar.gz` holds the same programs with an `install.sh`

Both include the `rk` command line tools, and both installers can download
Starter Content: Characters & Clothing on the way. Renkatsu Studio also offers it in
the project selector and in Window, Packages.

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

Start Renkatsu Studio from the Start menu or application menu, or run
`renkatsu`. `renkatsu --project path/to/project` opens a project directly.

Packages bring content to every project; [docs/packages.md](docs/packages.md)
explains how to make your own.

### Command line tools

The `rk` command works on cards and the game folder without a window.

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
`--outdoor-shoes` puts on the outdoor shoes. For a close-up, place the
camera yourself: `--eye 0.2,0.4,0.5 --look-at 0,0.8,0` stands it at that
point and aims it at the other (metres; the character stands at the origin,
faces +z, and y is up).

`--studio-look` draws the picture as Renkatsu Studio's viewport does, with
CharaStudio's screen effects (occlusion, bloom, vignette) and its first
colour filter. `--background 0.2,0.2,0.2` sets the colour behind the
character (`r,g,b` from 0 to 1).

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
   overlays and the other plugin data that cards depend on. Done in 0.1.0,
   with CharaStudio's screen effects in 0.1.2
2. The studio: a docked editor with a viewport, scene tree and inspector,
   and scene and project files stored as readable text, done in 0.1.1. Next
   in this step: posing characters (static poses and CharaStudio's FK and IK)
   and characters kept in the project in Renkatsu's own format. Packages and
   Starter Content, so Renkatsu works without Koikatsu, came in 0.1.2
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

[![CI](https://github.com/Mihnea8848/renkatsu/actions/workflows/ci.yml/badge.svg)](https://github.com/Mihnea8848/renkatsu/actions/workflows/ci.yml)

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

The interface font, Open Sans, is under the SIL Open Font License 1.1, in
[LICENSES/OFL-1.1.txt](LICENSES/OFL-1.1.txt).

The Renkatsu repository holds no game files. Starter Content, a separate
download, carries Koikatsu's own vanilla character content, animations and
default characters, without mods, community cards or voices.
