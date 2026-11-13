// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{anyhow, Result};
use rk_unityfs::{Bundle, SerializedFile, TypeNode, UValue};
use serde_json::{json, Map, Value as J};
use std::path::Path;

fn files(path: &Path) -> Result<(Bundle, Vec<SerializedFile>)> {
    let b = Bundle::open(path)?;
    let mut out = Vec::new();
    for n in b.nodes.iter().filter(|n| n.is_serialized()) {
        out.push(SerializedFile::parse(b.read_node(n)?)?);
    }
    Ok((b, out))
}

pub fn ls(path: &Path, class: Option<i32>) -> Result<()> {
    let (b, fs) = files(path)?;
    println!("format {} unity {}", b.format, b.unity_version);
    for n in &b.nodes {
        println!("node {} ({} bytes, flags {})", n.path, n.size, n.flags);
    }
    for f in &fs {
        for e in &f.externals {
            println!("external {}", e.path);
        }
        for o in &f.objects {
            if class.is_some_and(|c| c != o.class_id) {
                continue;
            }
            println!(
                "{:>22} class {:>4} {:>9} bytes  {}",
                o.path_id,
                o.class_id,
                o.size,
                f.name(o).unwrap_or_default()
            );
        }
    }
    Ok(())
}

pub fn to_json(v: &UValue, max_items: usize) -> J {
    match v {
        UValue::Bool(b) => json!(b),
        UValue::Int(i) => json!(i),
        UValue::Float(f) => json!(f),
        UValue::Str(s) => json!(s),
        UValue::Bytes(b) => json!(format!("<{} bytes>", b.len())),
        UValue::Array(a) => {
            let mut items: Vec<J> = a.iter().take(max_items).map(|v| to_json(v, max_items)).collect();
            if a.len() > max_items {
                items.push(json!(format!("... {} more", a.len() - max_items)));
            }
            J::Array(items)
        }
        UValue::Object(o) => {
            let mut m = Map::new();
            for (k, v) in &o.fields {
                m.insert(k.to_string(), to_json(v, max_items));
            }
            J::Object(m)
        }
    }
}

fn print_tree(n: &TypeNode, depth: usize) {
    println!(
        "{}{} {} (size {}, flags {:#x})",
        "  ".repeat(depth),
        n.ty,
        n.name,
        n.byte_size,
        n.flags
    );
    for c in &n.children {
        print_tree(c, depth + 1);
    }
}

pub fn dump(path: &Path, id: Option<i64>, name: Option<&str>, tree: bool, max_items: usize) -> Result<()> {
    let (_, fs) = files(path)?;
    for f in &fs {
        for o in &f.objects {
            let hit = match (id, name) {
                (Some(id), _) => o.path_id == id,
                (None, Some(n)) => f.name(o).is_some_and(|x| x.eq_ignore_ascii_case(n)),
                _ => false,
            };
            if !hit {
                continue;
            }
            if tree {
                print_tree(f.type_tree(o).ok_or_else(|| anyhow!("no type tree"))?, 0);
            } else {
                let v = f.read(o)?;
                println!("// path id {} class {}", o.path_id, o.class_id);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&to_json(&UValue::Object(Box::new(v)), max_items))?
                );
            }
        }
    }
    Ok(())
}
