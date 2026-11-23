// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The port of Unity's maths against Unity's own answers: the capture
//! plugin's `unitymath` key (Ctrl+Shift+F11 in CharaStudio) writes
//! `UserData/renkatsu/out/unitymath.json`; copied to
//! `tests/fixtures/unitymath.json`, it is checked here case by case.
//! Without the file the test says so and passes: no answers are made up.

use glam::{Quat, Vec3};
use rk_ik::unity;
use serde_json::Value;

const TOLERANCE: f32 = 1e-6;

fn v(r: &[f32], at: usize) -> Vec3 {
    Vec3::new(r[at], r[at + 1], r[at + 2])
}

fn q(r: &[f32], at: usize) -> Quat {
    Quat::from_xyzw(r[at], r[at + 1], r[at + 2], r[at + 3])
}

fn off(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

/// Checks every row of one function: `ours` gives our result from a row,
/// `theirs` is where Unity's result starts in it.
fn check(json: &Value, name: &str, theirs: usize, ours: impl Fn(&[f32]) -> Vec<f32>) -> Vec<String> {
    let mut wrong = Vec::new();
    let rows = json[name]
        .as_array()
        .unwrap_or_else(|| panic!("{name} missing from the capture"));
    for (i, row) in rows.iter().enumerate() {
        let r: Vec<f32> = row
            .as_array()
            .expect("a row of numbers")
            .iter()
            .map(|n| n.as_f64().expect("a number") as f32)
            .collect();
        let mine = ours(&r);
        let want = &r[theirs..theirs + mine.len()];
        let d = off(&mine, want);
        if d > TOLERANCE {
            wrong.push(format!(
                "{name} #{i}: in {:?}, ours {mine:?}, Unity {want:?} (off {d:e})",
                &r[..theirs]
            ));
        }
    }
    wrong
}

#[test]
fn the_port_answers_as_unity_does() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/unitymath.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!(
            "skipped: {} is not there yet; press Ctrl+Shift+F11 in CharaStudio with the capture plugin and copy \
             UserData/renkatsu/out/unitymath.json there",
            path.display()
        );
        return;
    };
    let json: Value = serde_json::from_str(&text).expect("the capture is JSON");
    let qa = |x: Quat| x.to_array().to_vec();
    let va = |x: Vec3| x.to_array().to_vec();
    let mut wrong = Vec::new();
    wrong.extend(check(&json, "look_rotation", 6, |r| {
        qa(unity::look_rotation(v(r, 0), v(r, 3)))
    }));
    wrong.extend(check(&json, "from_to_rotation", 6, |r| {
        qa(unity::from_to_rotation(v(r, 0), v(r, 3)))
    }));
    wrong.extend(check(&json, "angle_axis", 4, |r| qa(unity::angle_axis(r[0], v(r, 1)))));
    wrong.extend(check(&json, "inverse", 4, |r| qa(unity::inverse(q(r, 0)))));
    wrong.extend(check(&json, "quat_lerp", 9, |r| {
        qa(unity::quat_lerp(q(r, 0), q(r, 4), r[8]))
    }));
    wrong.extend(check(&json, "quat_slerp", 9, |r| {
        qa(unity::quat_slerp(q(r, 0), q(r, 4), r[8]))
    }));
    wrong.extend(check(&json, "ortho_normalize", 6, |r| {
        let (mut a, mut b) = (v(r, 0), v(r, 3));
        unity::ortho_normalize(&mut a, &mut b);
        [va(a), va(b)].concat()
    }));
    wrong.extend(check(&json, "managed", 14, |r| {
        let (a, b, x, y) = (q(r, 0), q(r, 4), v(r, 8), v(r, 11));
        [
            qa(unity::mul(a, b)),
            va(unity::rotate(a, x)),
            va(unity::normalize(x)),
            va(unity::project(x, y)),
            vec![unity::angle(x, y), unity::distance(x, y)],
        ]
        .concat()
    }));
    let shown: Vec<&String> = wrong.iter().take(20).collect();
    assert!(wrong.is_empty(), "{} cases differ from Unity:\n{shown:#?}", wrong.len());
}
