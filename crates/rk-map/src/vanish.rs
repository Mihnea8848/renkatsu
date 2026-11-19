// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! A map's table of walls that hide (CharaStudio's vanish,
//! `CameraControl.LoadVanish`). The table is a text asset in the
//! `h/list/` bundles, a line per volume: the name of an object whose
//! collider is the volume, then the names of the objects it hides, each
//! with everything under it, while the camera is inside the volume.

use rk_unityfs::{class, UObject};
use rk_vfs::Game;

/// One line of the table.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub collider: String,
    pub hides: Vec<String>,
}

/// The table's text: the text asset `file` in the bundles under `folder`
/// (a map's list names `h/list/`), as `GlobalMethod.LoadAllListText`
/// finds it.
pub fn table(game: &Game, folder: &str, file: &str) -> Option<String> {
    if folder.is_empty() || file.is_empty() {
        return None;
    }
    let folder = rk_vfs::index::norm(folder);
    let mut paths: Vec<&String> = game
        .assets
        .index
        .bundles
        .keys()
        .filter(|p| p.starts_with(folder.trim_end_matches('/')) && p.ends_with(".unity3d"))
        .collect();
    paths.sort();
    for path in paths {
        for bundle in game.assets.bundles(path) {
            for f in &bundle.files {
                for info in f.sf.objects_of(class::TEXT_ASSET) {
                    if f.sf.name(info).is_some_and(|n| n.eq_ignore_ascii_case(file)) {
                        let text: Option<UObject> = f.sf.read(info).ok();
                        if let Some(t) = text.as_ref().and_then(|o| o.str("m_Script")) {
                            return Some(t.to_owned());
                        }
                    }
                }
            }
        }
    }
    None
}

/// The rows of a table: tab-separated, a line each, empty cells ending a
/// row's names, as `GlobalMethod.GetListString` reads them.
pub fn parse(text: &str) -> Vec<Row> {
    text.split('\n')
        .map(|line| line.replace('\r', ""))
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let mut cells = line.split('\t');
            let collider = cells.next()?.to_owned();
            let hides = cells.take_while(|c| !c.is_empty()).map(str::to_owned).collect();
            Some(Row { collider, hides })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_line_names_a_volume_then_what_it_hides() {
        let rows = parse("wall00_d\twall00\r\nten00_d\tten00\tlamp\t\tignored\r\n\r\n");
        assert_eq!(
            rows,
            [
                Row {
                    collider: "wall00_d".into(),
                    hides: vec!["wall00".into()]
                },
                Row {
                    collider: "ten00_d".into(),
                    hides: vec!["ten00".into(), "lamp".into()]
                },
            ]
        );
    }
}
