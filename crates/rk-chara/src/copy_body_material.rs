// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Copy Cha Body Mat plugin. An item made for it carries a
//! `ChaCopyBodyMaterial` component listing renderers that should look like
//! the character's skin, such as the bare part of a sock. Each of them gets
//! a copy of the body's material (`customMatBody`) as its first material,
//! with the clothing alpha cut-outs (`_alpha_a`, `_alpha_b`) switched off,
//! the queue the component asks for, and a `DoNotEdit_` name so
//! MaterialEditor leaves it alone.

use crate::scene::MaterialInst;

/// Class of the component on items.
pub const SCRIPT: &str = "ChaCopyBodyMaterial";

/// The material a listed renderer ends up with. `queue` is the component's
/// `customRenderQueue`: -1, or anything outside 0 to 5000, keeps the body's.
pub fn copy(body: &MaterialInst, queue: i32) -> MaterialInst {
    let mut m = body.clone();
    if (0..=5000).contains(&queue) {
        m.render_queue = queue;
    }
    m.floats.insert("_alpha_a".into(), 0.0);
    m.floats.insert("_alpha_b".into(), 0.0);
    m.name = format!("DoNotEdit_{}", body.name);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copy_hides_no_skin_and_keeps_its_queue_rule() {
        let body = MaterialInst {
            name: "cf_m_body".into(),
            render_queue: 2000,
            ..Default::default()
        };
        let m = copy(&body, -1);
        assert_eq!(m.name, "DoNotEdit_cf_m_body");
        assert_eq!(m.render_queue, 2000);
        assert_eq!((m.floats["_alpha_a"], m.floats["_alpha_b"]), (0.0, 0.0));
        assert_eq!(copy(&body, 2450).render_queue, 2450);
        assert_eq!(copy(&body, 6000).render_queue, 2000);
    }
}
