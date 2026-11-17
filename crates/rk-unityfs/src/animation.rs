// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Animation clips and animator controllers as Unity stores them once
//! built: a clip is three blocks of curves (streamed, dense and constant)
//! with a list saying which property of which transform each curve drives,
//! and a controller is a table of states that each name the clips they
//! play. Transforms and names are stored as CRC-32 hashes of their text.

use crate::{Error, PPtr, Result, UObject, UValue};

/// CRC-32 (the zlib polynomial) of a text, as Unity hashes transform
/// paths, state names and parameter names.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// The Unity class of a transform, the only kind of target read here.
const TRANSFORM: i32 = 4;

/// What a curve of a transform drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// `localPosition`, three curves.
    Position,
    /// `localRotation` as a quaternion, four curves: x, y, z, w.
    Rotation,
    /// `localScale`, three curves.
    Scale,
    /// `localEulerAngles`, three curves.
    Euler,
    /// Anything else: a property of another component, one curve.
    Other,
}

/// One entry of a clip's binding list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    /// CRC-32 of the transform's path below the animator's object.
    pub path: u32,
    pub target: Target,
}

impl Binding {
    /// How many curves the binding takes.
    pub fn width(&self) -> usize {
        match self.target {
            Target::Position | Target::Scale | Target::Euler => 3,
            Target::Rotation => 4,
            Target::Other => 1,
        }
    }
}

/// One piece of a streamed curve: from `time` on, the value is the cubic
/// `((c0 t + c1) t + c2) t + c3` of the seconds since `time`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Piece {
    time: f32,
    coeff: [f32; 4],
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Dense {
    frames: usize,
    curves: usize,
    rate: f32,
    begin: f32,
    samples: Vec<f32>,
}

/// A built animation clip.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnimationClip {
    pub name: String,
    pub sample_rate: f32,
    /// The first and the last second of the clip.
    pub start: f32,
    pub stop: f32,
    /// The clip is made to loop (`m_LoopTime`).
    pub loops: bool,
    pub bindings: Vec<Binding>,
    /// The pieces of every streamed curve, by time.
    streamed: Vec<Vec<Piece>>,
    dense: Dense,
    constant: Vec<f32>,
}

fn floats(value: Option<&UValue>) -> Vec<f32> {
    value
        .and_then(UValue::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(UValue::as_f32)
        .collect()
}

fn missing(what: &str) -> Error {
    Error::Format(format!("animation data has no {what}"))
}

impl AnimationClip {
    /// Reads a clip from its decoded object.
    pub fn read(o: &UObject) -> Result<Self> {
        let muscle = o
            .get("m_MuscleClip")
            .and_then(UValue::as_object)
            .ok_or_else(|| missing("m_MuscleClip"))?;
        let clip = muscle
            .path("m_Clip.data")
            .and_then(UValue::as_object)
            .ok_or_else(|| missing("m_Clip"))?;
        let streamed_count = clip
            .path("m_StreamedClip.curveCount")
            .and_then(UValue::as_i64)
            .unwrap_or(0) as usize;
        let words: Vec<u32> = clip
            .path("m_StreamedClip.data")
            .and_then(UValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(UValue::as_i64)
            .map(|w| w as u32)
            .collect();
        let dense = clip.get("m_DenseClip");
        let int = |name: &str| dense.and_then(|d| d.get(name)).and_then(UValue::as_i64).unwrap_or(0);
        let float = |name: &str| dense.and_then(|d| d.get(name)).and_then(UValue::as_f32).unwrap_or(0.0);
        let dense = Dense {
            frames: int("m_FrameCount") as usize,
            curves: int("m_CurveCount") as usize,
            rate: float("m_SampleRate"),
            begin: float("m_BeginTime"),
            samples: floats(dense.and_then(|d| d.get("m_SampleArray"))),
        };
        if dense.samples.len() < dense.frames * dense.curves {
            return Err(Error::Format("dense clip is shorter than its frame count".into()));
        }
        let bindings = o
            .path("m_ClipBindingConstant.genericBindings")
            .and_then(UValue::as_array)
            .unwrap_or_default()
            .iter()
            .map(|b| {
                let int = |name: &str| b.get(name).and_then(UValue::as_i64).unwrap_or(0);
                let target = match (int("typeID") as i32, int("attribute")) {
                    (TRANSFORM, 1) => Target::Position,
                    (TRANSFORM, 2) => Target::Rotation,
                    (TRANSFORM, 3) => Target::Scale,
                    (TRANSFORM, 4) => Target::Euler,
                    _ => Target::Other,
                };
                Binding {
                    path: int("path") as u32,
                    target,
                }
            })
            .collect();
        Ok(Self {
            name: o.str("m_Name").unwrap_or_default().to_owned(),
            sample_rate: o.f32("m_SampleRate").unwrap_or(0.0),
            start: muscle.f32("m_StartTime").unwrap_or(0.0),
            stop: muscle.f32("m_StopTime").unwrap_or(0.0),
            loops: muscle.bool("m_LoopTime").unwrap_or(false),
            bindings,
            streamed: streamed_pieces(&words, streamed_count)?,
            dense,
            constant: floats(clip.path("m_ConstantClip.data")),
        })
    }

    /// A clip from plain curves, for tests and for clips made in code:
    /// `streamed` gives each curve as times with the four coefficients of
    /// the piece that starts there, `dense` as one row of values per frame.
    pub fn from_parts(
        bindings: Vec<Binding>,
        streamed: Vec<Vec<(f32, [f32; 4])>>,
        dense: (f32, Vec<Vec<f32>>),
        constant: Vec<f32>,
        stop: f32,
        loops: bool,
    ) -> Self {
        let (rate, rows) = dense;
        let curves = rows.first().map_or(0, Vec::len);
        Self {
            name: String::new(),
            sample_rate: rate,
            start: 0.0,
            stop,
            loops,
            bindings,
            streamed: streamed
                .into_iter()
                .map(|curve| curve.into_iter().map(|(time, coeff)| Piece { time, coeff }).collect())
                .collect(),
            dense: Dense {
                frames: rows.len(),
                curves,
                rate,
                begin: 0.0,
                samples: rows.into_iter().flatten().collect(),
            },
            constant,
        }
    }

    /// Seconds from the first to the last frame.
    pub fn duration(&self) -> f32 {
        (self.stop - self.start).max(0.0)
    }

    /// How many curves the clip has: the streamed ones come first, then
    /// the dense, then the constant ones, and the bindings take them in
    /// that order.
    pub fn curve_count(&self) -> usize {
        self.streamed.len() + self.dense.curves + self.constant.len()
    }

    /// The value of every curve at `time` seconds, as Unity samples them:
    /// streamed curves as cubic pieces, dense ones blended between the two
    /// frames around the time, constant ones as they are.
    pub fn sample(&self, time: f32, out: &mut Vec<f32>) {
        out.clear();
        out.reserve(self.curve_count());
        for curve in &self.streamed {
            // The last piece that started by now.
            let at = curve.partition_point(|p| p.time <= time);
            out.push(match at.checked_sub(1).map(|i| curve[i]) {
                // The piece before the clip's first frame holds a value.
                Some(p) if p.time < -1e30 => p.coeff[3],
                Some(p) => {
                    let t = time - p.time;
                    ((p.coeff[0] * t + p.coeff[1]) * t + p.coeff[2]) * t + p.coeff[3]
                }
                None => curve.first().map_or(0.0, |p| p.coeff[3]),
            });
        }
        let d = &self.dense;
        if d.curves > 0 && d.frames > 0 {
            let at = ((time - d.begin) * d.rate).clamp(0.0, (d.frames - 1) as f32);
            let first = at.floor() as usize;
            let second = (first + 1).min(d.frames - 1);
            let blend = at - first as f32;
            let (a, b) = (
                &d.samples[first * d.curves..][..d.curves],
                &d.samples[second * d.curves..][..d.curves],
            );
            out.extend(a.iter().zip(b).map(|(a, b)| a * (1.0 - blend) + b * blend));
        } else {
            out.extend(std::iter::repeat_n(0.0, d.curves));
        }
        out.extend_from_slice(&self.constant);
    }

    /// The largest change of any curve over the clip, looked at on every
    /// frame of its sample rate. Zero for a clip that holds one pose.
    pub fn movement(&self) -> f32 {
        let rate = if self.sample_rate > 0.0 { self.sample_rate } else { 30.0 };
        let frames = (self.duration() * rate).round() as usize;
        let (mut first, mut now) = (Vec::new(), Vec::new());
        self.sample(self.start, &mut first);
        let mut most = 0.0f32;
        for frame in 1..=frames {
            let time = (self.start + frame as f32 / rate).min(self.stop);
            self.sample(time, &mut now);
            for (a, b) in first.iter().zip(&now) {
                most = most.max((a - b).abs());
            }
        }
        most
    }
}

/// Cuts the words of a streamed clip into the pieces of each curve. The
/// data is a run of frames, each a time, a count and that many keys of a
/// curve index and four coefficients; the last frame is at infinity and
/// has no keys.
fn streamed_pieces(words: &[u32], curves: usize) -> Result<Vec<Vec<Piece>>> {
    let mut out = vec![Vec::new(); curves];
    let mut at = 0;
    while at + 2 <= words.len() {
        let time = f32::from_bits(words[at]);
        let keys = words[at + 1] as usize;
        at += 2;
        if at + keys * 5 > words.len() {
            return Err(Error::Format("streamed clip ends inside a frame".into()));
        }
        for key in words[at..at + keys * 5].chunks_exact(5) {
            let curve = out
                .get_mut(key[0] as usize)
                .ok_or_else(|| Error::Format("streamed clip names a curve it does not have".into()))?;
            curve.push(Piece {
                time,
                coeff: [
                    f32::from_bits(key[1]),
                    f32::from_bits(key[2]),
                    f32::from_bits(key[3]),
                    f32::from_bits(key[4]),
                ],
            });
        }
        at += keys * 5;
    }
    Ok(out)
}

/// How a blend tree node mixes its children.
#[derive(Debug, Clone, PartialEq)]
pub enum Blend {
    /// The node is one clip: an index into the controller's clips.
    Clip(usize),
    /// A one dimensional blend: the children with the parameter value each
    /// stands at, in rising order.
    Simple1D {
        /// CRC-32 of the parameter's name.
        parameter: u32,
        children: Vec<(usize, f32)>,
    },
    /// A kind of blend this reader does not know (two dimensional and
    /// direct blends); the children are kept so that the first can play.
    Unknown { children: Vec<usize> },
}

/// One node of a state's blend tree.
#[derive(Debug, Clone, PartialEq)]
pub struct BlendNode {
    pub blend: Blend,
}

/// A state of a controller's first layer.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub name: String,
    /// The speed the state plays its motion at.
    pub speed: f32,
    /// The nodes of the state's blend tree; the first is the root. Empty
    /// for a state without a motion.
    pub nodes: Vec<BlendNode>,
    /// How many transitions lead out of the state.
    pub transitions: usize,
    /// The state's motion starts over at its end (`m_Loop`).
    pub loops: bool,
}

/// A float parameter of a controller with the value it starts at.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    /// CRC-32 of the name, as blend trees refer to it.
    pub id: u32,
    pub default: f32,
}

/// A built animator controller: the states of its first layer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnimatorController {
    pub name: String,
    pub states: Vec<State>,
    /// The clips the states play, as references from the controller's file.
    pub clips: Vec<PPtr>,
    pub parameters: Vec<Parameter>,
}

fn items(value: Option<&UValue>) -> impl Iterator<Item = &UValue> {
    // Lists of pointers hold each element under `data`.
    value
        .and_then(UValue::as_array)
        .unwrap_or_default()
        .iter()
        .map(|v| v.get("data").unwrap_or(v))
}

impl AnimatorController {
    pub fn read(o: &UObject) -> Result<Self> {
        let controller = o
            .get("m_Controller")
            .and_then(UValue::as_object)
            .ok_or_else(|| missing("m_Controller"))?;
        let names: std::collections::HashMap<u32, &str> = o
            .array("m_TOS")
            .unwrap_or_default()
            .iter()
            .filter_map(|e| Some((e.get("first")?.as_i64()? as u32, e.get("second")?.as_str()?)))
            .collect();
        let name_of = |id: u32| names.get(&id).copied().unwrap_or_default().to_owned();

        // The machine of the first layer.
        let machine_index = items(controller.get("m_LayerArray"))
            .next()
            .and_then(|l| l.get("m_StateMachineIndex"))
            .and_then(UValue::as_i64)
            .unwrap_or(0) as usize;
        let machine = items(controller.get("m_StateMachineArray")).nth(machine_index);
        let mut states = Vec::new();
        for state in items(machine.and_then(|m| m.get("m_StateConstantArray"))) {
            let int = |name: &str| state.get(name).and_then(UValue::as_i64).unwrap_or(0);
            // The tree of the first motion set, the only one of a controller
            // without synced layers.
            let tree_index = state
                .get("m_BlendTreeConstantIndexArray")
                .and_then(UValue::as_array)
                .and_then(|a| a.first())
                .and_then(UValue::as_i64)
                .unwrap_or(-1);
            let tree = usize::try_from(tree_index)
                .ok()
                .and_then(|i| items(state.get("m_BlendTreeConstantArray")).nth(i));
            let nodes = items(tree.and_then(|t| t.get("m_NodeArray")))
                .map(|node| {
                    let int = |name: &str| node.get(name).and_then(UValue::as_i64).unwrap_or(0);
                    let children: Vec<usize> = node
                        .get("m_ChildIndices")
                        .and_then(UValue::as_array)
                        .unwrap_or_default()
                        .iter()
                        .filter_map(UValue::as_i64)
                        .map(|c| c as usize)
                        .collect();
                    let blend = if children.is_empty() {
                        Blend::Clip(int("m_ClipID") as usize)
                    } else if int("m_BlendType") == 0 {
                        let at = floats(
                            node.get("m_Blend1dData")
                                .and_then(|d| d.get("data"))
                                .and_then(|d| d.get("m_ChildThresholdArray")),
                        );
                        Blend::Simple1D {
                            parameter: int("m_BlendEventID") as u32,
                            children: children
                                .iter()
                                .enumerate()
                                .map(|(i, c)| (*c, at.get(i).copied().unwrap_or(0.0)))
                                .collect(),
                        }
                    } else {
                        Blend::Unknown { children }
                    };
                    BlendNode { blend }
                })
                .collect();
            states.push(State {
                name: name_of(int("m_NameID") as u32),
                speed: state.get("m_Speed").and_then(UValue::as_f32).unwrap_or(1.0),
                nodes,
                transitions: state
                    .get("m_TransitionConstantArray")
                    .and_then(UValue::as_array)
                    .map_or(0, <[UValue]>::len),
                loops: state.get("m_Loop").and_then(UValue::as_bool).unwrap_or(false),
            });
        }

        // Parameters: the values table says where each one's default is.
        let defaults = floats(controller.path("m_DefaultValues.data.m_FloatValues"));
        let parameters = controller
            .path("m_Values.data.m_ValueArray")
            .and_then(UValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|v| {
                let int = |name: &str| v.get(name).and_then(UValue::as_i64);
                // Type 1 is a float.
                (int("m_Type")? == 1).then_some(())?;
                let id = int("m_ID")? as u32;
                Some(Parameter {
                    name: name_of(id),
                    id,
                    default: defaults.get(int("m_Index")? as usize).copied().unwrap_or(0.0),
                })
            })
            .collect();
        Ok(Self {
            name: o.str("m_Name").unwrap_or_default().to_owned(),
            states,
            clips: o
                .array("m_AnimationClips")
                .unwrap_or_default()
                .iter()
                .filter_map(UValue::as_pptr)
                .collect(),
            parameters,
        })
    }

    pub fn state(&self, name: &str) -> Option<&State> {
        self.states.iter().find(|s| s.name == name)
    }
}

impl State {
    /// The clips the state plays and how much of each, for the given
    /// parameter values (by the CRC-32 of the parameter's name): pairs of
    /// an index into the controller's clips and a weight. The weights of a
    /// one dimensional blend are Unity's: the two children around the
    /// value share it by distance, and a value outside takes the nearest.
    pub fn weights(&self, parameter: &dyn Fn(u32) -> f32) -> Vec<(usize, f32)> {
        let mut out = Vec::new();
        if !self.nodes.is_empty() {
            self.node_weights(0, 1.0, parameter, &mut out, 0);
        }
        out
    }

    fn node_weights(
        &self,
        node: usize,
        weight: f32,
        parameter: &dyn Fn(u32) -> f32,
        out: &mut Vec<(usize, f32)>,
        depth: usize,
    ) {
        let Some(n) = self.nodes.get(node) else { return };
        if weight <= 0.0 || depth > 16 {
            return;
        }
        match &n.blend {
            Blend::Clip(clip) => out.push((*clip, weight)),
            Blend::Unknown { children } => {
                if let Some(&first) = children.first() {
                    self.node_weights(first, weight, parameter, out, depth + 1);
                }
            }
            Blend::Simple1D {
                parameter: id,
                children,
            } => {
                let value = parameter(*id);
                for (i, &(child, at)) in children.iter().enumerate() {
                    let before = i.checked_sub(1).map(|j| children[j].1);
                    let after = children.get(i + 1).map(|c| c.1);
                    let share = if value < at {
                        match before {
                            None => 1.0,
                            Some(b) if value > b && at > b => (value - b) / (at - b),
                            Some(_) => 0.0,
                        }
                    } else {
                        match after {
                            None => 1.0,
                            Some(a) if value < a && a > at => (a - value) / (a - at),
                            Some(_) => 0.0,
                        }
                    };
                    self.node_weights(child, weight * share, parameter, out, depth + 1);
                }
            }
        }
    }
}

/// Writes numbers and text in a fixed byte order, for clips and states
/// kept outside a Unity bundle.
#[derive(Debug, Default)]
pub struct Writer(pub Vec<u8>);

impl Writer {
    pub fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub fn text(&mut self, v: &str) {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v.as_bytes());
    }

    fn floats(&mut self, v: &[f32]) {
        self.u32(v.len() as u32);
        for f in v {
            self.f32(*f);
        }
    }
}

/// Reads what [`Writer`] wrote. Every read fails at the end of the bytes
/// instead of going past it.
#[derive(Debug)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.bytes.len());
        let end = end.ok_or_else(|| Error::Format("stored animation ends early".into()))?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    pub fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// A count of things that each take at least `each` bytes: one that
    /// the bytes left cannot hold is refused before anything is reserved.
    pub fn count(&mut self, each: usize) -> Result<usize> {
        let n = self.u32()? as usize;
        if n.saturating_mul(each.max(1)) > self.bytes.len() - self.at {
            return Err(Error::Format("stored animation names more than it holds".into()));
        }
        Ok(n)
    }

    pub fn text(&mut self) -> Result<String> {
        let n = self.count(1)?;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| Error::Format("stored animation has broken text".into()))
    }

    fn floats(&mut self) -> Result<Vec<f32>> {
        let n = self.count(4)?;
        (0..n).map(|_| self.f32()).collect()
    }
}

impl AnimationClip {
    /// Writes the clip as it plays: its times, bindings and curves.
    pub fn write(&self, w: &mut Writer) {
        w.text(&self.name);
        w.f32(self.sample_rate);
        w.f32(self.start);
        w.f32(self.stop);
        w.u32(u32::from(self.loops));
        w.u32(self.bindings.len() as u32);
        for b in &self.bindings {
            w.u32(b.path);
            w.u32(match b.target {
                Target::Position => 1,
                Target::Rotation => 2,
                Target::Scale => 3,
                Target::Euler => 4,
                Target::Other => 0,
            });
        }
        w.u32(self.streamed.len() as u32);
        for curve in &self.streamed {
            w.u32(curve.len() as u32);
            for piece in curve {
                w.f32(piece.time);
                for c in piece.coeff {
                    w.f32(c);
                }
            }
        }
        w.u32(self.dense.frames as u32);
        w.u32(self.dense.curves as u32);
        w.f32(self.dense.rate);
        w.f32(self.dense.begin);
        w.floats(&self.dense.samples);
        w.floats(&self.constant);
    }

    /// Reads a clip that [`AnimationClip::write`] wrote.
    pub fn read_stored(r: &mut Reader) -> Result<Self> {
        let name = r.text()?;
        let (sample_rate, start, stop) = (r.f32()?, r.f32()?, r.f32()?);
        let loops = r.u32()? != 0;
        let bindings = (0..r.count(8)?)
            .map(|_| {
                Ok(Binding {
                    path: r.u32()?,
                    target: match r.u32()? {
                        1 => Target::Position,
                        2 => Target::Rotation,
                        3 => Target::Scale,
                        4 => Target::Euler,
                        _ => Target::Other,
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut streamed = Vec::new();
        for _ in 0..r.count(4)? {
            let pieces = (0..r.count(20)?)
                .map(|_| {
                    Ok(Piece {
                        time: r.f32()?,
                        coeff: [r.f32()?, r.f32()?, r.f32()?, r.f32()?],
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            streamed.push(pieces);
        }
        let (frames, curves) = (r.u32()? as usize, r.u32()? as usize);
        let (rate, begin) = (r.f32()?, r.f32()?);
        let samples = r.floats()?;
        if samples.len() < frames.saturating_mul(curves) {
            return Err(Error::Format("dense clip is shorter than its frame count".into()));
        }
        Ok(Self {
            name,
            sample_rate,
            start,
            stop,
            loops,
            bindings,
            streamed,
            dense: Dense {
                frames,
                curves,
                rate,
                begin,
                samples,
            },
            constant: r.floats()?,
        })
    }
}

impl State {
    /// Writes the state with its blend tree. The clips it names are
    /// indices the caller gives meaning to.
    pub fn write(&self, w: &mut Writer) {
        w.text(&self.name);
        w.f32(self.speed);
        w.u32(self.transitions as u32);
        w.u32(u32::from(self.loops));
        w.u32(self.nodes.len() as u32);
        for node in &self.nodes {
            match &node.blend {
                Blend::Clip(clip) => {
                    w.u32(0);
                    w.u32(*clip as u32);
                }
                Blend::Simple1D { parameter, children } => {
                    w.u32(1);
                    w.u32(*parameter);
                    w.u32(children.len() as u32);
                    for (child, at) in children {
                        w.u32(*child as u32);
                        w.f32(*at);
                    }
                }
                Blend::Unknown { children } => {
                    w.u32(2);
                    w.u32(children.len() as u32);
                    for child in children {
                        w.u32(*child as u32);
                    }
                }
            }
        }
    }

    /// Reads a state that [`State::write`] wrote.
    pub fn read_stored(r: &mut Reader) -> Result<Self> {
        let name = r.text()?;
        let speed = r.f32()?;
        let transitions = r.u32()? as usize;
        let loops = r.u32()? != 0;
        let mut nodes = Vec::new();
        for _ in 0..r.count(8)? {
            let blend = match r.u32()? {
                0 => Blend::Clip(r.u32()? as usize),
                1 => {
                    let parameter = r.u32()?;
                    let children = (0..r.count(8)?)
                        .map(|_| Ok((r.u32()? as usize, r.f32()?)))
                        .collect::<Result<Vec<_>>>()?;
                    Blend::Simple1D { parameter, children }
                }
                2 => Blend::Unknown {
                    children: (0..r.count(4)?)
                        .map(|_| Ok(r.u32()? as usize))
                        .collect::<Result<Vec<_>>>()?,
                },
                other => return Err(Error::Format(format!("stored animation has a blend of kind {other}"))),
            };
            nodes.push(BlendNode { blend });
        }
        Ok(State {
            name,
            speed,
            nodes,
            transitions,
            loops,
        })
    }

    /// The clips the state's blend tree names, each once, in rising order.
    pub fn clip_indices(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self
            .nodes
            .iter()
            .filter_map(|n| match n.blend {
                Blend::Clip(clip) => Some(clip),
                _ => None,
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The state with its clips renumbered by `to`.
    pub fn with_clips(mut self, to: &dyn Fn(usize) -> usize) -> State {
        for node in &mut self.nodes {
            if let Blend::Clip(clip) = &mut node.blend {
                *clip = to(*clip);
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_is_the_zlib_one() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"a"), 0xE8B7_BE43);
    }

    fn one(target: Target) -> Binding {
        Binding { path: 1, target }
    }

    #[test]
    fn curves_come_streamed_then_dense_then_constant() {
        let clip = AnimationClip::from_parts(
            vec![one(Target::Position), one(Target::Rotation)],
            // One streamed curve: holds 2, then from second 1 rises by 3 a
            // second with a curve of 1 t squared.
            vec![vec![
                (f32::MIN, [0.0, 0.0, 0.0, 2.0]),
                (0.0, [0.0, 0.0, 0.0, 2.0]),
                (1.0, [0.0, 1.0, 3.0, 2.0]),
            ]],
            // Two dense curves at ten frames a second.
            (10.0, vec![vec![0.0, 10.0], vec![1.0, 20.0], vec![4.0, 20.0]]),
            vec![7.0, 8.0, 9.0, 1.0],
            2.0,
            true,
        );
        assert_eq!(clip.curve_count(), 7);
        assert_eq!(clip.bindings.iter().map(Binding::width).sum::<usize>(), 7);
        let mut out = Vec::new();
        clip.sample(0.5, &mut out);
        // The dense block stops at its last frame.
        assert_eq!(out, [2.0, 4.0, 20.0, 7.0, 8.0, 9.0, 1.0]);
        clip.sample(0.05, &mut out);
        assert!((out[1] - 0.5).abs() < 1e-6 && (out[2] - 15.0).abs() < 1e-5);
        clip.sample(1.5, &mut out);
        // 1 * 0.25 + 3 * 0.5 + 2.
        assert!((out[0] - 3.75).abs() < 1e-6);
        // Before every piece, the first value holds.
        clip.sample(-1.0, &mut out);
        assert_eq!(out[0], 2.0);
        assert!(clip.movement() > 1.0);
    }

    #[test]
    fn a_clip_that_holds_one_pose_does_not_move() {
        let clip = AnimationClip::from_parts(
            vec![one(Target::Scale)],
            vec![vec![(f32::MIN, [0.0, 0.0, 0.0, 1.0]), (0.0, [0.0, 0.0, 0.0, 1.0])]],
            (30.0, vec![vec![5.0], vec![5.0], vec![5.0]]),
            vec![1.0],
            2.0 / 30.0,
            false,
        );
        assert_eq!(clip.movement(), 0.0);
    }

    #[test]
    fn streamed_words_are_cut_into_frames() {
        let f = f32::to_bits;
        let words = [
            f(f32::MIN),
            1,
            0,
            f(0.0),
            f(0.0),
            f(0.0),
            f(4.0),
            f(0.5),
            1,
            0,
            f(1.0),
            f(2.0),
            f(3.0),
            f(4.0),
            f(f32::INFINITY),
            0,
        ];
        let pieces = streamed_pieces(&words, 1).unwrap();
        assert_eq!(pieces[0].len(), 2);
        assert_eq!(
            pieces[0][1],
            Piece {
                time: 0.5,
                coeff: [1.0, 2.0, 3.0, 4.0]
            }
        );
        // A frame cut short, and a curve that is not there.
        assert!(streamed_pieces(&words[..10], 1).is_err());
        assert!(streamed_pieces(&words, 0).is_err());
    }

    fn state(children: &[(usize, f32)]) -> State {
        let mut nodes = vec![BlendNode {
            blend: Blend::Simple1D {
                parameter: 9,
                children: children.to_vec(),
            },
        }];
        nodes.extend((0..children.len()).map(|i| BlendNode { blend: Blend::Clip(i) }));
        State {
            name: "s".into(),
            speed: 1.0,
            nodes,
            transitions: 0,
            loops: false,
        }
    }

    #[test]
    fn a_one_dimensional_blend_shares_by_distance() {
        let s = state(&[(1, 0.0), (2, 0.5), (3, 1.0)]);
        let at = |v: f32| s.weights(&|_| v);
        assert_eq!(at(0.0), [(0, 1.0)]);
        assert_eq!(at(0.5), [(1, 1.0)]);
        assert_eq!(at(0.25), [(0, 0.5), (1, 0.5)]);
        let w = at(0.9);
        assert!((w[0].1 - 0.2).abs() < 1e-6 && (w[1].1 - 0.8).abs() < 1e-6);
        assert_eq!((w[0].0, w[1].0), (1, 2));
        // Outside the range the nearest child plays alone.
        assert_eq!(at(-3.0), [(0, 1.0)]);
        assert_eq!(at(7.0), [(2, 1.0)]);
        // A state that is one clip.
        let single = State {
            name: "c".into(),
            speed: 1.0,
            nodes: vec![BlendNode { blend: Blend::Clip(4) }],
            transitions: 0,
            loops: true,
        };
        assert_eq!(single.weights(&|_| 0.3), [(4, 1.0)]);
        assert!(single.loops && !s.loops);
    }

    #[test]
    fn a_clip_and_a_state_come_back_from_their_stored_bytes() {
        let clip = AnimationClip::from_parts(
            vec![
                Binding {
                    path: 7,
                    target: Target::Rotation,
                },
                Binding {
                    path: 9,
                    target: Target::Position,
                },
            ],
            vec![vec![(0.0, [0.0, 0.0, 1.0, 2.0]), (0.5, [0.0, 0.0, 0.0, 2.5])]],
            (30.0, vec![vec![1.0, 2.0, 3.0], vec![2.0, 3.0, 4.0]]),
            vec![0.5, 0.25, 0.125],
            1.0,
            true,
        );
        let state = State {
            name: "walk".into(),
            speed: 1.5,
            nodes: vec![
                BlendNode {
                    blend: Blend::Simple1D {
                        parameter: 11,
                        children: vec![(1, 0.0), (2, 1.0)],
                    },
                },
                BlendNode { blend: Blend::Clip(4) },
                BlendNode { blend: Blend::Clip(9) },
            ],
            transitions: 2,
            loops: true,
        };
        let mut w = Writer::default();
        clip.write(&mut w);
        state.write(&mut w);
        let mut r = Reader::new(&w.0);
        assert_eq!(AnimationClip::read_stored(&mut r).unwrap(), clip);
        assert_eq!(State::read_stored(&mut r).unwrap(), state);
        assert_eq!(state.clip_indices(), [4, 9]);
        let renumbered = state.with_clips(&|c| if c == 4 { 0 } else { 1 });
        assert_eq!(renumbered.clip_indices(), [0, 1]);
        // Cut short, or with a count no file could hold, it is refused.
        assert!(AnimationClip::read_stored(&mut Reader::new(&w.0[..w.0.len() / 3])).is_err());
        assert!(AnimationClip::read_stored(&mut Reader::new(&[255, 255, 255, 255, 1])).is_err());
    }
}
