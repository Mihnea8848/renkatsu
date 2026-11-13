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

You need Rust 1.95. With `rustup` the right version is picked up from
`rust-toolchain.toml`.

## Testing a change

There are two kinds of tests.

The unit tests build their own small cards and bundles in memory. They run
anywhere, also in CI:

```sh
cargo test
```

The corpus tests read a real Koikatsu install: they parse every card, decode
the character bundles and resolve every card against your mods. They are
skipped when no game folder is configured, so CI cannot run them. Run the full
check before you open a pull request:

```sh
tools/check.sh
```

It checks formatting, runs clippy and all tests, and writes
`renders/compare.png`: each sampled card as the game drew it, next to the
Renkatsu render. For a change that affects how characters look, attach the
sheet from before and after.

A bug fix should come with a unit test that fails without the fix.

## Pull requests

- Fork the repository, create a branch, and open a pull request against `main`.
- CI checks formatting and runs clippy and the unit tests on Linux. That is
  the `build` job and it must pass. A pull request that only changes
  documentation skips the build.
- The Windows tests and the dependency audit run on `main` after a merge, and
  once a week.
- Pull requests are merged with squash, so the branch history does not matter.
- Keep one topic per pull request.

## Code style

- Every source file starts with a copyright line and
  `SPDX-License-Identifier: GPL-3.0-or-later`. Add your own copyright line to
  files you write; keep the existing ones.
- Rust: formatted with `cargo fmt`, no compiler or clippy warnings.
- Parsers return an error on damaged input. They do not panic and do not
  silently drop data.
- Comments explain what is not obvious from the code, briefly. No change logs
  or version notes in comments; that is what git is for.
- Documentation: plain, specific sentences. Say what something does and how
  to use it.
- No game assets and no decompiled game code in the repository.

## License

By contributing you agree that your work is published under the GNU General
Public License, version 3 or later, like the rest of the project. You keep the
copyright to your changes.
