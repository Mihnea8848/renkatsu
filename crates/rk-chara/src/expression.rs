// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Face expressions. The head prefab carries a script with three controllers
//! (eyebrows, eyes, mouth). Each lists the meshes it moves and, per expression
//! pattern, which blend shape is the closed and which the open form. The game
//! always applies one pattern, so a face with no weights set is not neutral.

use rk_unityfs::{PPtr, UObject, UValue};

/// One mesh a controller moves.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// The game object holding the mesh.
    pub object: PPtr,
    /// Closed and open blend shape per pattern; -1 means none.
    pub patterns: Vec<(i64, i64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Controller {
    pub targets: Vec<Target>,
    pub open_min: f32,
    pub open_max: f32,
}

impl Controller {
    /// Reads a serialized `FBSCtrlEyes`, `FBSCtrlEyebrow` or `FBSCtrlMouth`.
    pub fn read(o: &UObject) -> Option<Self> {
        let mut targets = Vec::new();
        for t in o.array("FBSTarget")? {
            let t = t.as_object()?;
            let patterns = t
                .array("PtnSet")?
                .iter()
                .filter_map(UValue::as_object)
                .map(|p| (p.i64("Close").unwrap_or(-1), p.i64("Open").unwrap_or(-1)))
                .collect();
            targets.push(Target {
                object: t.pptr("ObjTarget")?,
                patterns,
            });
        }
        let float = |name: &str, default: f32| o.get(name).and_then(UValue::as_f32).unwrap_or(default);
        Some(Self {
            targets,
            open_min: float("OpenMin", 0.0),
            open_max: float("OpenMax", 1.0),
        })
    }

    /// Blend shape weights (0 to 1) for one target showing `pattern`, with
    /// `open` running from closed (0) to open (1). The game works in whole
    /// percent.
    pub fn weights(&self, target: &Target, pattern: usize, open: f32) -> Vec<(usize, f32)> {
        let Some(&(close_shape, open_shape)) = target.patterns.get(pattern) else {
            return Vec::new();
        };
        let rate = self.open_min + (self.open_max - self.open_min) * open.clamp(0.0, 1.0);
        let percent = (rate * 100.0).clamp(0.0, 100.0) as i64;
        let mut out: Vec<(usize, f32)> = Vec::new();
        for (shape, weight) in [(close_shape, 100 - percent), (open_shape, percent)] {
            if shape < 0 || weight == 0 {
                continue;
            }
            match out.iter_mut().find(|(s, _)| *s == shape as usize) {
                Some(entry) => entry.1 += weight as f32 / 100.0,
                None => out.push((shape as usize, weight as f32 / 100.0)),
            }
        }
        out
    }
}

/// How open each controller is on a character at rest: eyes and eyebrows
/// open, mouth closed.
pub const REST: [(&str, f32); 3] = [("EyebrowCtrl", 1.0), ("EyesCtrl", 1.0), ("MouthCtrl", 0.0)];

#[cfg(test)]
mod tests {
    use super::*;

    fn controller(patterns: Vec<(i64, i64)>) -> Controller {
        Controller {
            targets: vec![Target {
                object: PPtr::default(),
                patterns,
            }],
            open_min: 0.0,
            open_max: 1.0,
        }
    }

    #[test]
    fn open_and_closed_pick_one_shape() {
        let c = controller(vec![(0, 1), (2, 3)]);
        let t = &c.targets[0];
        assert_eq!(c.weights(t, 0, 1.0), vec![(1, 1.0)]);
        assert_eq!(c.weights(t, 0, 0.0), vec![(0, 1.0)]);
        assert_eq!(c.weights(t, 1, 1.0), vec![(3, 1.0)]);
        assert_eq!(c.weights(t, 0, 0.25), vec![(0, 0.75), (1, 0.25)]);
    }

    #[test]
    fn limits_missing_shapes_and_patterns() {
        let mut c = controller(vec![(-1, 4), (5, 5)]);
        let t = c.targets[0].clone();
        // No closed shape: only the open one counts.
        assert_eq!(c.weights(&t, 0, 0.5), vec![(4, 0.5)]);
        // Same shape for both ends adds up to full.
        assert_eq!(c.weights(&t, 1, 0.3), vec![(5, 1.0)]);
        assert!(c.weights(&t, 9, 1.0).is_empty());
        // The open range is scaled by the controller's limits.
        c.open_max = 0.5;
        assert_eq!(c.weights(&t, 0, 1.0), vec![(4, 0.5)]);
    }
}
