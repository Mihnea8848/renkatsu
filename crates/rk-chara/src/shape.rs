// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Body and face sliders. Each slider samples a table of keyed bone values
//! shipped with the game; rule tables then combine those samples into local
//! transforms of the skeleton's shape bones.

use crate::scene::{euler_deg, Scene};
use anyhow::{anyhow, bail, Context, Result};
use glam::Vec3;
use rk_unityfs::class;
use rk_vfs::Game;
use std::collections::HashMap;

pub const BODY_RULES: &str = include_str!("../data/shape_body.rules");
pub const HEAD_RULES: &str = include_str!("../data/shape_head.rules");

/// Channel order used everywhere: position xyz, euler xyz, scale xyz.
type Channels = [f32; 9];
const REST: Channels = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];

fn comp_index(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.len() != 2 {
        return None;
    }
    let group = match b[0] {
        b'p' => 0,
        b'r' => 3,
        b's' => 6,
        _ => return None,
    };
    let axis = match b[1] {
        b'x' => 0,
        b'y' => 1,
        b'z' => 2,
        _ => return None,
    };
    Some(group + axis)
}

#[derive(Debug, Clone)]
enum Expr {
    Num(f32),
    Src(String, usize),
    Corr(usize, usize),
    Var(String),
    Neg(Box<Expr>),
    Wrap(Box<Expr>),
    Bin(u8, Box<Expr>, Box<Expr>),
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i] == b' ' {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn sum(&mut self) -> Result<Expr> {
        let mut l = self.product()?;
        while let Some(op @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            l = Expr::Bin(op, Box::new(l), Box::new(self.product()?));
        }
        Ok(l)
    }
    fn product(&mut self) -> Result<Expr> {
        let mut l = self.unary()?;
        while let Some(op @ (b'*' | b'/')) = self.peek() {
            self.i += 1;
            l = Expr::Bin(op, Box::new(l), Box::new(self.unary()?));
        }
        Ok(l)
    }
    fn unary(&mut self) -> Result<Expr> {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(b'(') => {
                self.i += 1;
                let e = self.sum()?;
                if self.peek() != Some(b')') {
                    bail!("expected )");
                }
                self.i += 1;
                Ok(e)
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => {
                let start = self.i;
                while self.i < self.s.len()
                    && (self.s[self.i].is_ascii_digit()
                        || matches!(self.s[self.i], b'.' | b'E' | b'e')
                        || (matches!(self.s[self.i], b'-' | b'+') && matches!(self.s[self.i - 1], b'E' | b'e')))
                {
                    self.i += 1;
                }
                Ok(Expr::Num(std::str::from_utf8(&self.s[start..self.i])?.parse()?))
            }
            Some(_) => {
                let start = self.i;
                while self.i < self.s.len()
                    && (self.s[self.i].is_ascii_alphanumeric() || matches!(self.s[self.i], b'_' | b'.'))
                {
                    self.i += 1;
                }
                let word = std::str::from_utf8(&self.s[start..self.i])?;
                if word == "wrap" {
                    if self.peek() != Some(b'(') {
                        bail!("expected ( after wrap");
                    }
                    return Ok(Expr::Wrap(Box::new(self.unary()?)));
                }
                let parts: Vec<&str> = word.split('.').collect();
                match parts.as_slice() {
                    ["s", name, comp] => Ok(Expr::Src(
                        name.to_string(),
                        comp_index(comp).ok_or_else(|| anyhow!("bad channel {comp}"))?,
                    )),
                    ["v", name] => Ok(Expr::Var(name.to_string())),
                    ["c", n, comp] => Ok(Expr::Corr(
                        n.parse()?,
                        comp_index(comp).ok_or_else(|| anyhow!("bad channel {comp}"))?,
                    )),
                    _ => bail!("unknown term {word:?}"),
                }
            }
            None => bail!("unexpected end of expression"),
        }
    }
}

struct BoneRule {
    bone: String,
    sets: Vec<(usize, Expr)>,
    /// Source channel feeding the head's width, when this is the head root.
    head_scale: Option<Expr>,
}

pub struct Rules {
    bones: Vec<BoneRule>,
}

impl Rules {
    pub fn parse(text: &str) -> Result<Self> {
        let mut bones: Vec<BoneRule> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix("bone ") {
                bones.push(BoneRule {
                    bone: name.trim().to_string(),
                    sets: Vec::new(),
                    head_scale: None,
                });
                continue;
            }
            let (lhs, rhs) = line
                .split_once('=')
                .ok_or_else(|| anyhow!("rule line {}: no =", n + 1))?;
            let mut p = Parser {
                s: rhs.trim().as_bytes(),
                i: 0,
            };
            let expr = p.sum().with_context(|| format!("rule line {}", n + 1))?;
            let rule = bones
                .last_mut()
                .ok_or_else(|| anyhow!("rule line {}: no bone", n + 1))?;
            match lhs.trim() {
                "headscale" => rule.head_scale = Some(expr),
                c => rule.sets.push((
                    comp_index(c).ok_or_else(|| anyhow!("rule line {}: bad target {c}", n + 1))?,
                    expr,
                )),
            }
        }
        Ok(Self { bones })
    }
}

/// Keyed bone values: for each source name, one set of channels per key.
pub struct KeyTable {
    keys: HashMap<String, Vec<Channels>>,
}

impl KeyTable {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut pos = 0usize;
        let mut take = |n: usize| -> Result<&[u8]> {
            let s = data
                .get(pos..pos + n)
                .ok_or_else(|| anyhow!("key table is truncated"))?;
            pos += n;
            Ok(s)
        };
        let count = i32::from_le_bytes(take(4)?.try_into()?);
        let mut keys = HashMap::new();
        for _ in 0..count {
            // .NET string: 7-bit encoded length.
            let (mut len, mut shift) = (0usize, 0);
            loop {
                let b = take(1)?[0];
                len |= ((b & 0x7f) as usize) << shift;
                shift += 7;
                if b & 0x80 == 0 {
                    break;
                }
            }
            let name = String::from_utf8_lossy(take(len)?).into_owned();
            let n = i32::from_le_bytes(take(4)?.try_into()?);
            let mut list = Vec::with_capacity(n.max(0) as usize);
            for _ in 0..n {
                take(4)?;
                let mut ch = [0f32; 9];
                for c in ch.iter_mut() {
                    *c = f32::from_le_bytes(take(4)?.try_into()?);
                }
                list.push(ch);
            }
            keys.insert(name, list);
        }
        Ok(Self { keys })
    }

    /// Samples a source at `rate` (0..1 across the keys), as the game's
    /// `AnimationKeyInfo.GetInfo` does. Values outside the range follow the
    /// Slider Unlocker plugin.
    fn sample(&self, name: &str, rate: f32) -> Option<Channels> {
        let list = self.keys.get(name)?;
        if list.len() < 2 {
            return list.first().copied();
        }
        if !(0.0..=1.0).contains(&rate) {
            let clamped = self.sample(name, rate.clamp(0.0, 1.0))?;
            return Some(crate::slider_unlocker::outside(list, name, rate, clamped));
        }
        let f = (list.len() - 1) as f32 * rate;
        let i = (f.floor() as isize).clamp(0, list.len() as isize - 2) as usize;
        let t = f - i as f32;
        let (a, b) = (&list[i], &list[i + 1]);
        let mut out = [0f32; 9];
        for k in 0..9 {
            out[k] = if (3..6).contains(&k) {
                // Shortest-path angle interpolation.
                let mut d = (b[k] - a[k]) % 360.0;
                if d > 180.0 {
                    d -= 360.0;
                } else if d < -180.0 {
                    d += 360.0;
                }
                a[k] + d * t
            } else {
                a[k] + (b[k] - a[k]) * t
            };
        }
        Some(out)
    }
}

struct Category {
    slider: usize,
    source: String,
    used: [bool; 9],
}

/// One slider set (body or face): key table, slider-to-source map, state.
pub struct ShapeSet {
    keys: KeyTable,
    categories: Vec<Category>,
    sources: HashMap<String, Channels>,
    /// Named constants the rules may refer to as `v.<name>`.
    pub vars: HashMap<String, f32>,
}

fn text_asset(game: &Game, vpath: &str, name: &str, prefer_mod: Option<u32>) -> Result<Vec<u8>> {
    let obj = game
        .assets
        .find(vpath, name, Some(class::TEXT_ASSET), prefer_mod)
        .ok_or_else(|| anyhow!("{name} not found in {vpath}"))?;
    Ok(obj.read()?.bytes("m_Script").unwrap_or_default().to_vec())
}

impl ShapeSet {
    pub fn load(
        game: &Game,
        key_bundle: &str,
        key_asset: &str,
        category_asset: &str,
        prefer_mod: Option<u32>,
    ) -> Result<Self> {
        let keys = text_asset(game, key_bundle, key_asset, prefer_mod)?;
        let text = text_asset(game, "list/customshape.unity3d", category_asset, None)?;
        Self::from_tables(&keys, &text)
    }

    /// Builds a slider set from the raw key table and the tab-separated
    /// slider-to-source table.
    pub fn from_tables(keys: &[u8], text: &[u8]) -> Result<Self> {
        let keys = KeyTable::parse(keys)?;
        let mut categories = Vec::new();
        let mut sources = HashMap::new();
        for line in String::from_utf8_lossy(text).lines() {
            let cols: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
            if cols.len() < 11 {
                continue;
            }
            let Ok(slider) = cols[0].trim().parse::<usize>() else {
                continue;
            };
            let mut used = [false; 9];
            for (k, u) in used.iter_mut().enumerate() {
                *u = cols[2 + k].trim() != "0";
            }
            sources.entry(cols[1].to_string()).or_insert(REST);
            categories.push(Category {
                slider,
                source: cols[1].to_string(),
                used,
            });
        }
        Ok(Self {
            keys,
            categories,
            sources,
            vars: HashMap::new(),
        })
    }

    pub fn set(&mut self, slider: usize, value: f32) {
        for c in self.categories.iter().filter(|c| c.slider == slider) {
            let Some(sample) = self.keys.sample(&c.source, value) else {
                continue;
            };
            let dst = self.sources.entry(c.source.clone()).or_insert(REST);
            for k in 0..9 {
                if c.used[k] {
                    dst[k] = sample[k];
                }
            }
        }
    }

    fn eval(&self, e: &Expr, corr: Option<&[Channels]>) -> f32 {
        match e {
            Expr::Num(n) => *n,
            Expr::Src(name, c) => self.sources.get(name).map(|s| s[*c]).unwrap_or(REST[*c]),
            Expr::Corr(i, c) => corr.and_then(|t| t.get(*i)).map(|v| v[*c]).unwrap_or(0.0),
            Expr::Var(name) => self.vars.get(name).copied().unwrap_or(1.0),
            Expr::Neg(a) => -self.eval(a, corr),
            Expr::Wrap(a) => {
                let v = self.eval(a, corr);
                if v > 180.0 {
                    v - 360.0
                } else {
                    v
                }
            }
            Expr::Bin(op, a, b) => {
                let (a, b) = (self.eval(a, corr), self.eval(b, corr));
                match op {
                    b'+' => a + b,
                    b'-' => a - b,
                    b'*' => a * b,
                    _ => a / b,
                }
            }
        }
    }

    /// Writes the slider results into the bones below `root`.
    pub fn apply(&self, scene: &mut Scene, root: usize, rules: &Rules, corr: Option<&[Channels]>) {
        let names = scene.name_map(root);
        self.apply_named(scene, &names, rules, corr);
    }

    /// `apply` on bones looked up earlier. Every value is set outright, so
    /// applying again after changing slider values is safe.
    pub fn apply_named(
        &self,
        scene: &mut Scene,
        names: &HashMap<String, usize>,
        rules: &Rules,
        corr: Option<&[Channels]>,
    ) {
        for rule in &rules.bones {
            let Some(&node) = names.get(&rule.bone) else { continue };
            if let Some(e) = &rule.head_scale {
                // Cancel the non-uniform scale inherited from the body so
                // the head keeps its proportions, then apply head width.
                let (world, _) = scene.world();
                let ps = scene.nodes[node].parent.map(|p| {
                    let m = world[p];
                    Vec3::new(
                        m.x_axis.truncate().length(),
                        m.y_axis.truncate().length(),
                        m.z_axis.truncate().length(),
                    )
                });
                let ps = ps.unwrap_or(Vec3::ONE);
                let base = ps.y
                    * if corr.is_some() {
                        1.0 / (1.0 + corr.and_then(|c| c.get(2)).map(|c| c[7]).unwrap_or(0.0))
                    } else {
                        1.0
                    };
                scene.nodes[node].local.scale =
                    Vec3::new(base / ps.x + (self.eval(e, corr) - 1.0), base / ps.y, base / ps.z);
            }
            let mut rot = [None; 3];
            for (comp, e) in &rule.sets {
                let v = self.eval(e, corr);
                let local = &mut scene.nodes[node].local;
                match comp {
                    0..=2 => local.pos[*comp] = v,
                    3..=5 => rot[*comp - 3] = Some(v),
                    _ => local.scale[*comp - 6] = v,
                }
            }
            if rot.iter().any(Option::is_some) {
                scene.nodes[node].local.rot =
                    euler_deg(rot[0].unwrap_or(0.0), rot[1].unwrap_or(0.0), rot[2].unwrap_or(0.0));
            }
        }
    }
}

/// Correction values used when a card selects the alternate bone type.
pub fn load_corrections(game: &Game) -> Result<Vec<Channels>> {
    let data = text_asset(game, "list/shapecorrect/shapecorrect.unity3d", "shapecorrect", None)?;
    let n = i32::from_le_bytes(
        data.get(..4)
            .ok_or_else(|| anyhow!("empty correction table"))?
            .try_into()?,
    ) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let mut ch = [0f32; 9];
        for (k, c) in ch.iter_mut().enumerate() {
            let o = 4 + (i * 9 + k) * 4;
            *c = f32::from_le_bytes(
                data.get(o..o + 4)
                    .ok_or_else(|| anyhow!("correction table is truncated"))?
                    .try_into()?,
            );
        }
        out.push(ch);
    }
    Ok(out)
}
