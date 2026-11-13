# Contributing

Bug reports, test results with your own cards and mods, and pull requests are
all useful. I can only test with the content I have installed, so a report
that a card loads (or does not) on your install already helps.

## Reporting a problem

Open an issue with the bug report form. Include the output of the command
that failed. For a card that renders wrong, the output of this is the most
useful thing to attach:

```sh
cargo run --release -p rk-cli -- resolve path/to/card.png
```

Do not attach game files or paid mods.

## Building

```sh
cargo build --release
```

You need a recent stable Rust toolchain. I build with 1.95.

## Testing a change

1. Point `renkatsu.toml` at your Koikatsu install.
2. Run `cargo test --release`. The corpus tests parse every local card, decode
   the character bundles and resolve every card against your mods. Without a
   configured install they are skipped.
3. Render a few cards before and after the change and compare the images.

## Pull requests

- Fork the repository, create a branch, and open a pull request against `main`.
- CI builds the project, runs the tests and checks formatting. It must pass.
- Pull requests are merged with squash, so the branch history does not matter.
- Keep one topic per pull request.

## Code style

- Every source file starts with a copyright line and
  `SPDX-License-Identifier: GPL-3.0-or-later`. Add your own copyright line to
  files you write; keep the existing ones.
- Rust: formatted with `cargo fmt`, no compiler warnings.
- Comments explain what is not obvious from the code, briefly. No change logs
  or version notes in comments; that is what git is for.
- Documentation: plain, specific sentences. Say what something does and how
  to use it.
- No game assets and no decompiled game code in the repository.

## License

By contributing you agree that your work is published under the GNU General
Public License, version 3 or later, like the rest of the project. You keep the
copyright to your changes.
