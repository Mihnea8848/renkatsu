// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! A Renkatsu animation file (`.rkanim`): one animation taken out of the
//! bundle it came in, so that a project that copied it plays it on a
//! machine without that bundle, without the package or the mod that
//! brought it and without Koikatsu.
//!
//! The file holds what playing one state of an animator controller needs
//! and nothing else of its bundle: the state with its blend tree, the
//! controller's parameters, and the clips the tree names. A state that
//! blends clips by the character's height keeps all of them, so the
//! animation still fits every body.
//!
//! ```text
//! "RKANIM"            6 bytes
//! version             u16, little endian as every number
//! header              a count, then that many pairs of key and text
//! state               the state and its blend tree
//! parameters          count, then name, id and default of each
//! clips               count, then each clip's times, bindings and curves
//! ```
//!
//! The header is text by key, so that a list reads what it shows (the
//! name, the category, the length) without reading the curves, and so
//! that a later version can add keys an older reader skips.

use crate::animations::{Timing, STATIC_SECONDS};
use crate::motion::{self, Motion, Parameters};
use anyhow::{bail, Context, Result};
use rk_unityfs::animation::{AnimationClip, AnimatorController, Parameter, Reader, State, Writer};
use rk_unityfs::class;
use rk_vfs::Game;
use std::path::Path;
use std::sync::Arc;

/// The extension of an animation file.
pub const EXTENSION: &str = "rkanim";

const MAGIC: &[u8; 6] = b"RKANIM";

/// The version this build writes, and the newest it reads.
pub const VERSION: u16 = 1;

/// What an animation file says about itself.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Meta {
    /// The name it was listed under where it came from.
    pub name: String,
    /// The group and the category it was listed in, by name.
    pub group: String,
    pub category: String,
    /// What it was copied from: the game, a package or a mod, by name.
    pub source: String,
    /// The bundle, controller and state it was taken out of.
    pub bundle: String,
    pub controller: String,
    pub state: String,
    /// The length of one pass for a middling body, in seconds.
    pub seconds: f32,
    pub loops: bool,
    /// The H scenes list it, rather than CharaStudio's own menu.
    pub h: bool,
}

impl Meta {
    pub fn timing(&self) -> Timing {
        Timing {
            seconds: self.seconds,
            loops: self.loops,
        }
    }

    /// Whether the animation holds one pose.
    pub fn is_static(&self) -> bool {
        !self.loops && self.seconds <= STATIC_SECONDS
    }

    fn pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("name", self.name.clone()),
            ("group", self.group.clone()),
            ("category", self.category.clone()),
            ("source", self.source.clone()),
            ("bundle", self.bundle.clone()),
            ("controller", self.controller.clone()),
            ("state", self.state.clone()),
            ("seconds", self.seconds.to_string()),
            ("loops", self.loops.to_string()),
            ("h", self.h.to_string()),
        ]
    }

    fn set(&mut self, key: &str, value: String) {
        match key {
            "name" => self.name = value,
            "group" => self.group = value,
            "category" => self.category = value,
            "source" => self.source = value,
            "bundle" => self.bundle = value,
            "controller" => self.controller = value,
            "state" => self.state = value,
            "seconds" => self.seconds = value.parse().unwrap_or(0.0),
            "loops" => self.loops = value == "true",
            "h" => self.h = value == "true",
            // A key of a later version.
            _ => {}
        }
    }
}

/// An animation as its file holds it.
#[derive(Debug, Clone)]
pub struct Stored {
    pub meta: Meta,
    state: State,
    parameters: Vec<Parameter>,
    clips: Vec<Arc<AnimationClip>>,
}

impl Stored {
    /// Takes one state of a controller out of the content that is
    /// installed, with the clips it plays. With `over`, the clips of
    /// that controller replace the base's by name first, as the game
    /// layers them for the H scenes' animations. The meta names where it
    /// was found; the caller adds what the list said of it.
    pub fn extract(
        game: &Game,
        bundle: &str,
        controller: &str,
        state: &str,
        over: crate::motion::Override,
    ) -> Result<Stored> {
        let object = game
            .assets
            .find(bundle, controller, Some(class::ANIMATOR_CONTROLLER), None)
            .with_context(|| format!("no animator controller {controller} in {bundle}"))?;
        let read = AnimatorController::read(&object.read()?)?;
        let found = read
            .state(state)
            .with_context(|| format!("controller {controller} has no state {state}"))?
            .clone();
        let indices = found.clip_indices();
        let instead = crate::motion::replacements(game, over)?;
        let mut clips = Vec::with_capacity(indices.len());
        for &index in &indices {
            let pointer = *read
                .clips
                .get(index)
                .with_context(|| format!("controller {controller} has no clip {index}"))?;
            let clip = game
                .assets
                .deref(&object.file, pointer)
                .with_context(|| format!("a clip of {controller} is missing from {bundle}"))?;
            let clip = instead.get(&clip.name()).unwrap_or(&clip);
            clips.push(Arc::new(AnimationClip::read(&clip.read()?)?));
        }
        let state_stored = found.with_clips(&|old| indices.iter().position(|i| *i == old).unwrap_or(0));
        Ok(Stored::new(
            Meta {
                name: state.to_owned(),
                bundle: bundle.to_owned(),
                controller: controller.to_owned(),
                state: state.to_owned(),
                ..Default::default()
            },
            state_stored,
            read.parameters,
            clips,
        ))
    }

    /// An animation from its parts; the length in the meta is worked out
    /// from the clips, for a middling body.
    pub fn new(meta: Meta, state: State, parameters: Vec<Parameter>, clips: Vec<Arc<AnimationClip>>) -> Stored {
        let mut stored = Stored {
            meta,
            state,
            parameters,
            clips,
        };
        let middling = Parameters {
            height: 0.5,
            breast: 0.5,
        };
        if let Ok(motion) = stored.motion(&middling) {
            stored.meta.seconds = motion.duration();
            stored.meta.loops = motion.loops();
        }
        stored
    }

    /// The animation ready to play on a character with these parameters.
    pub fn motion(&self, parameters: &Parameters) -> Result<Motion> {
        let controller = AnimatorController {
            name: self.meta.controller.clone(),
            states: vec![self.state.clone()],
            clips: Vec::new(),
            parameters: self.parameters.clone(),
        };
        motion::resolve(&controller, &self.state.name, parameters, &mut |index| {
            self.clips
                .get(index)
                .cloned()
                .with_context(|| format!("the animation file has no clip {index}"))
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.0.extend_from_slice(MAGIC);
        w.0.extend_from_slice(&VERSION.to_le_bytes());
        let pairs = self.meta.pairs();
        w.u32(pairs.len() as u32);
        for (key, value) in pairs {
            w.text(key);
            w.text(&value);
        }
        self.state.write(&mut w);
        w.u32(self.parameters.len() as u32);
        for p in &self.parameters {
            w.text(&p.name);
            w.u32(p.id);
            w.f32(p.default);
        }
        w.u32(self.clips.len() as u32);
        for clip in &self.clips {
            clip.write(&mut w);
        }
        w.0
    }

    /// Reads the header of a file's bytes and leaves the reader after it.
    fn header<'a>(bytes: &'a [u8]) -> Result<(Meta, Reader<'a>)> {
        if bytes.len() < 8 || &bytes[..6] != MAGIC {
            bail!("not a Renkatsu animation file");
        }
        let version = u16::from_le_bytes([bytes[6], bytes[7]]);
        if version == 0 || version > VERSION {
            bail!("the animation file was written by a newer Renkatsu (version {version})");
        }
        let mut r = Reader::new(&bytes[8..]);
        let mut meta = Meta::default();
        for _ in 0..r.count(8)? {
            let key = r.text()?;
            meta.set(&key, r.text()?);
        }
        Ok((meta, r))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Stored> {
        let (meta, mut r) = Stored::header(bytes)?;
        let state = State::read_stored(&mut r)?;
        let mut parameters = Vec::new();
        for _ in 0..r.count(12)? {
            parameters.push(Parameter {
                name: r.text()?,
                id: r.u32()?,
                default: r.f32()?,
            });
        }
        let mut clips = Vec::new();
        for _ in 0..r.count(4)? {
            clips.push(Arc::new(AnimationClip::read_stored(&mut r)?));
        }
        Ok(Stored {
            meta,
            state,
            parameters,
            clips,
        })
    }

    pub fn read(path: &Path) -> Result<Stored> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Stored::from_bytes(&bytes).with_context(|| format!("reading {}", path.display()))
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        std::fs::write(path, self.to_bytes()).with_context(|| format!("writing {}", path.display()))
    }
}

/// What a file says about itself, without its curves: the header is at
/// the start, so only that much is read.
pub fn read_meta(path: &Path) -> Result<Meta> {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(path)
        .with_context(|| format!("reading {}", path.display()))?
        .take(16 * 1024)
        .read_to_end(&mut head)?;
    Ok(Stored::header(&head)
        .with_context(|| format!("reading {}", path.display()))?
        .0)
}

/// An animation file found in a folder of them, with the group and
/// category it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct Filed {
    pub path: std::path::PathBuf,
    /// The file's name without its extension.
    pub name: String,
    pub group: String,
    pub category: String,
    pub meta: Meta,
}

/// The animation files below `root`, as a package's `animations` folder
/// holds them, in path order. The folders around a file name its group
/// (the first) and its category (the last); a file at the top keeps the
/// group and category written in it, and one folder deep the folder is
/// the group. Files that cannot be read are left out.
pub fn filed(root: &Path) -> Vec<Filed> {
    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case(EXTENSION)) {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let meta = read_meta(&path).ok()?;
            let folders: Vec<String> = path
                .strip_prefix(root)
                .ok()?
                .parent()
                .into_iter()
                .flat_map(|p| p.components())
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            let (group, category) = match &folders[..] {
                [] => (meta.group.clone(), meta.category.clone()),
                [one] => (one.clone(), meta.category.clone()),
                [first, .., last] => (first.clone(), last.clone()),
            };
            Some(Filed {
                name: path.file_stem()?.to_string_lossy().into_owned(),
                path,
                group,
                category,
                meta,
            })
        })
        .collect()
}

/// A small animation made in code, for tests here and in the crates that
/// keep animation files: a state that blends two clips by height, as the
/// game's do, lasting `seconds` for a middling body.
pub fn sample(name: &str, seconds: f32, loops: bool) -> Stored {
    use rk_unityfs::animation::{Binding, Blend, BlendNode, Target};
    let clip = |length: f32, value: f32| {
        Arc::new(AnimationClip::from_parts(
            vec![Binding {
                path: 5,
                target: Target::Position,
            }],
            Vec::new(),
            (30.0, vec![vec![0.0, 0.0, 0.0], vec![value, 0.0, 0.0]]),
            Vec::new(),
            length,
            loops,
        ))
    };
    let height = rk_unityfs::animation::crc32(b"height");
    Stored::new(
        Meta {
            name: name.to_owned(),
            group: "Character".into(),
            category: "Walk and run".into(),
            source: "Koikatsu".into(),
            bundle: "studio/anime/00.unity3d".into(),
            controller: "walk_run".into(),
            state: "f_aruki_00".into(),
            ..Default::default()
        },
        State {
            name: "f_aruki_00".into(),
            speed: 1.0,
            nodes: vec![
                BlendNode {
                    blend: Blend::Simple1D {
                        parameter: height,
                        children: vec![(1, 0.0), (2, 1.0)],
                    },
                },
                BlendNode { blend: Blend::Clip(0) },
                BlendNode { blend: Blend::Clip(1) },
            ],
            transitions: 0,
            loops,
        },
        vec![Parameter {
            name: "height".into(),
            id: height,
            default: 0.0,
        }],
        vec![clip(seconds * 2.0 / 3.0, 1.0), clip(seconds * 4.0 / 3.0, 2.0)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored() -> Stored {
        sample("Walk 1", 1.5, true)
    }

    #[test]
    fn an_animation_file_plays_as_what_it_was_made_from() {
        let first = stored();
        // The length is the blend's for a middling body.
        assert_eq!((first.meta.seconds, first.meta.loops), (1.5, true));
        assert!(!first.meta.is_static());
        let bytes = first.to_bytes();
        let back = Stored::from_bytes(&bytes).unwrap();
        assert_eq!(back.meta, first.meta);
        // Every body gets its own blend out of the file.
        for (height, seconds) in [(0.0, 1.0), (0.5, 1.5), (1.0, 2.0)] {
            let parameters = Parameters { height, breast: 0.5 };
            let (a, b) = (first.motion(&parameters).unwrap(), back.motion(&parameters).unwrap());
            assert_eq!(a.duration(), seconds);
            assert_eq!(b.duration(), seconds);
            assert_eq!(a.clips.len(), b.clips.len());
            for ((x, wx), (y, wy)) in a.clips.iter().zip(&b.clips) {
                assert_eq!((x.as_ref(), wx), (y.as_ref(), wy));
            }
        }
    }

    #[test]
    fn the_header_reads_alone_and_bad_files_are_refused() {
        let dir = std::env::temp_dir().join(format!("renkatsu-rkanim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Walk 1.rkanim");
        let first = stored();
        first.write(&file).unwrap();
        assert_eq!(read_meta(&file).unwrap(), first.meta);
        assert_eq!(Stored::read(&file).unwrap().meta.category, "Walk and run");
        assert!(Stored::from_bytes(b"not an animation").is_err());
        let mut newer = first.to_bytes();
        newer[6] = 99;
        assert!(Stored::from_bytes(&newer).is_err());
        let bytes = first.to_bytes();
        assert!(Stored::from_bytes(&bytes[..bytes.len() - 9]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
