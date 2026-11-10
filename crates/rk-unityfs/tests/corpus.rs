// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Opens every bundle under abdata/chara and decodes every object.

use rayon::prelude::*;
use rk_unityfs::{Bundle, SerializedFile};
use std::collections::BTreeMap;
use std::sync::Mutex;

#[test]
fn chara_bundles_decode() {
    let Some(game) = rk_core::test_game_dir() else { return };
    let mut files = Vec::new();
    rk_core::walk_files(&game.join("abdata/chara"), "unity3d", &mut files);
    rk_core::walk_files(&game.join("abdata/list"), "unity3d", &mut files);
    let classes = Mutex::new(BTreeMap::<i32, usize>::new());
    let errors = Mutex::new(Vec::new());
    files.par_iter().for_each(|p| {
        let run = || -> rk_unityfs::Result<()> {
            let b = Bundle::open(p)?;
            for n in b.nodes.iter().filter(|n| n.is_serialized()) {
                let f = SerializedFile::parse(b.read_node(n)?)?;
                let mut local = BTreeMap::new();
                for o in &f.objects {
                    let typed = |v: &rk_unityfs::UObject| -> rk_unityfs::Result<()> {
                        use rk_unityfs::{class, objects::*};
                        match o.class_id {
                            class::MESH => {
                                let m = Mesh::read(v)?;
                                assert!(m.normals.is_empty() || m.normals.len() == m.positions.len());
                                assert!(m.skin.is_empty() || m.skin.len() == m.positions.len(), "{}", m.name);
                                assert!(
                                    m.indices.iter().all(|&i| (i as usize) < m.positions.len()),
                                    "{}",
                                    m.name
                                );
                            }
                            class::MATERIAL => drop(Material::read(v)?),
                            class::TEXTURE2D => {
                                let t = Texture2D::read(v)?;
                                if t.stream.is_none() {
                                    t.decode_rgba8(&t.data)?;
                                }
                            }
                            class::GAME_OBJECT => drop(GameObject::read(v)?),
                            class::TRANSFORM => drop(Transform::read(v)?),
                            class::SKINNED_MESH_RENDERER | class::MESH_RENDERER => drop(Renderer::read(v)?),
                            _ => {}
                        }
                        Ok(())
                    };
                    f.read_strict(o).and_then(|v| typed(&v)).map_err(|e| {
                        rk_unityfs::Error::Format(format!("class {} id {}: {e}", o.class_id, o.path_id))
                    })?;
                    *local.entry(o.class_id).or_insert(0) += 1;
                }
                let mut c = classes.lock().unwrap();
                for (k, v) in local {
                    *c.entry(k).or_insert(0) += v;
                }
            }
            Ok(())
        };
        if let Err(e) = run() {
            errors.lock().unwrap().push(format!("{}: {e}", p.display()));
        }
    });
    let errors = errors.into_inner().unwrap();
    eprintln!("{} bundles, {} errors", files.len(), errors.len());
    eprintln!("objects by class: {:?}", classes.into_inner().unwrap());
    for e in errors.iter().take(15) {
        eprintln!("  {e}");
    }
    assert!(errors.is_empty());
}
