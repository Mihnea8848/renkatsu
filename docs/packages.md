# Making packages

A package brings content to every project at once: characters, clothing,
animations and other game data. Renkatsu installs packages per user,
and every project can use what they hold. Starter Content is made of
packages, and you can make your own with the `rk` command line tool that
comes with Renkatsu.

## Make a package folder

```sh
rk package new my-animations --id my-animations --name "My animations"
```

This makes a folder `my-animations` with a `package.toml` and empty
folders for content:

```text
my-animations/
  package.toml       what the package is
  mods/              zipmods
  abdata/            game style bundles and list files
  cards/female/      character cards
  cards/male/
  animations/ poses/ maps/ items/ scenes/
```

The id is how Renkatsu names the package on disk. Use lower case letters,
digits and dashes, and keep it the same for every version of the package.

## Describe it

Open `package.toml` and fill in the description and, if you like, where
the content comes from:

```toml
format = 1
id = "my-animations"
name = "My animations"
version = "1.0.0"
kind = "custom"
description = "Animations for sitting and leaning."
requires_renkatsu = "0.1.4"
source = "Made by me, from the mods listed in the readme of my page."
```

Raise `version` when you publish a new one, so Renkatsu can tell it is
newer.

## Add content

- `mods/`: zipmods, as Koikatsu uses them with Sideloader. This is the
  easiest way to bring animations, clothing and accessories made for
  Koikatsu. Renkatsu reads them with the same rules as Sideloader: the
  newest version of a mod wins over older copies. Packages with mods need
  Renkatsu 0.1.4 or newer.
- `abdata/`: Unity asset bundles and list files at the paths the game uses,
  for example `abdata/studio/anime/...`, as they are inside a zipmod.
- `cards/female/` and `cards/male/`: character cards (`.png`). Projects can
  import them with File, Import from a package.
- `animations/`: Renkatsu animation files (`.rkanim`), in folders by group
  and category. See Animations below.

Files outside these folders are refused when the package is built, so
nothing ends up in it by accident.

## Build it

```sh
rk package build my-animations --out .
```

This checks the folder, counts what it holds and writes
`my-animations-1.0.0.rkpkg`. It prints the file's size and SHA-256, which
you can give to people so they can check their download, and the groups
and categories its animations go into.

## Animations

Renkatsu calls every clip that plays on a character an animation, as
CharaStudio's Anime menu does. Some hold one pose (static) and some move;
the lists show which. A pose in CharaStudio's sense, a file of FK and IK
rotations made by hand, is something else: the `poses/` folder is kept for
those.

Projects do not use a package's animations in place. A project imports
the ones it wants (Content, Animations, Import from package), and each is
copied into the project as a `.rkanim` file. The project then plays them
on a machine that has neither your package nor Koikatsu.

### Groups and categories

Animations are browsed by group and category, for example Studio, Dance.
Renkatsu tells groups and categories apart by name, not by number, and
merges them across everything that is installed:

- To add to a category that exists, in Koikatsu or in another package, use
  its group's name and its name. Your animations show in that category
  beside the others, each marked with the package it comes from. Capitals
  and spaces around a name do not matter.
- To make a new category, use a name no one else uses. A new group works
  the same way.

There are two ways to say which group and category an animation is in.

**Animation files.** Put `.rkanim` files in folders named after the group
and the category:

```text
animations/
  Studio/
    Dance/
      Spin.rkanim         joins Studio, Dance
  My group/
    Leaning/
      Lean on wall.rkanim  a new group with a new category
```

The file's name is the animation's name. You get `.rkanim` files from a
project's `Animations` folder (every imported animation is one), or from
the command line:

```sh
rk animation list                  # groups and categories with their numbers
rk animation export 7 22 --out my-animations/animations
```

**Koikatsu style lists.** A zipmod in `mods/`, or bundles in `abdata/`,
bring lists as CharaStudio reads them: `AnimeGroup_*` names groups by
number, `AnimeCategory_<n>_<group>` names the categories of a group, and
`Anime_<n>_<group>_<category>` lists the animations. In the game a number
that exists adds to that group or category and a new number makes a new
one. Renkatsu reads the numbers the same way and then goes by the names
they give: a list that uses a new number but an existing name still joins
the category of that name. If two packages use the same number for
different names, the one read last names it, as in the game, so pick
numbers no one else uses or use animation files instead.

`rk package build` prints the groups and categories it found, so you can
check them before you share the package:

```text
  12 animations in these groups and categories:
    Studio (8)
      Dance (8)
    My group (4)
      Leaning (4)
```

## Install it

In Renkatsu Studio, open Window, Packages and choose Install from a file.
Or on the command line:

```sh
rk package install my-animations-1.0.0.rkpkg
```

Installing checks every file against the package's checksums. Packages
live in `~/.local/share/renkatsu/packages` on Linux and
`%LOCALAPPDATA%\Renkatsu\packages` on Windows, and Window, Packages
removes them again.

## Your responsibility

Renkatsu is for adults. What you put in a package and share is your
responsibility: only share content you have the right to share, follow the
rules of the place you share it on, and never make or share sexual content
involving characters who are, or appear to be, under 18.
