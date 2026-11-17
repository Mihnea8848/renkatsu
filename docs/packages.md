# Making packages

A package brings content to every project at once: characters, clothing,
poses, animations and other game data. Renkatsu installs packages per user,
and every project can use what they hold. Starter Content is made of
packages, and you can make your own with the `rk` command line tool that
comes with Renkatsu.

## Make a package folder

```sh
rk package new my-poses --id my-poses --name "My poses"
```

This makes a folder `my-poses` with a `package.toml` and empty folders for
content:

```text
my-poses/
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
id = "my-poses"
name = "My poses"
version = "1.0.0"
kind = "custom"
description = "Poses for sitting and leaning."
requires_renkatsu = "0.1.4"
source = "Made by me, from the mods listed in the readme of my page."
```

Raise `version` when you publish a new one, so Renkatsu can tell it is
newer.

## Add content

- `mods/`: zipmods, as Koikatsu uses them with Sideloader. This is the
  easiest way to bring poses, animations, clothing and accessories made
  for Koikatsu. Renkatsu reads them with the same rules as Sideloader: the
  newest version of a mod wins over older copies. Packages with mods need
  Renkatsu 0.1.4 or newer.
- `abdata/`: Unity asset bundles and list files at the paths the game uses,
  for example `abdata/studio/anime/...`, as they are inside a zipmod.
- `cards/female/` and `cards/male/`: character cards (`.png`). Projects can
  import them with File, Import from a package.

Files outside these folders are refused when the package is built, so
nothing ends up in it by accident.

## Build it

```sh
rk package build my-poses --out .
```

This checks the folder, counts what it holds and writes
`my-poses-1.0.0.rkpkg`. It prints the file's size and SHA-256, which you
can give to people so they can check their download.

## Install it

In Renkatsu Studio, open Window, Packages and choose Install from a file.
Or on the command line:

```sh
rk package install my-poses-1.0.0.rkpkg
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
