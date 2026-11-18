// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Work that takes long, as the window shows it: a splash at the start,
//! a window that waits for it, notices in the corner and a line in the
//! status bar. A prototype shown with `--loading`, on sample jobs; the
//! work itself still runs where it did.

use crate::selector;
use crate::state::Look;
use crate::widgets::{boxed, glyph, hair_h};
use clap::ValueEnum;
use egui::{Align2, Color32, Pos2, Rect, Stroke, Ui, Vec2};
use egui_phosphor::regular as icon;

/// The ways of showing work that are pictured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Picture {
    /// The splash of the start, over the whole window.
    Splash,
    /// A window that waits: one bar and the step.
    Modal,
    /// The same with the list of what is being loaded.
    ModalList,
    /// Notices in the bottom right corner.
    Notices,
    /// One line in the status bar.
    Status,
}

/// How far one thing of a job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Waiting,
    Running,
    Done,
}

/// One thing a job loads.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: String,
    pub stage: Stage,
    /// What it says at its right: the time it took, or how far it is.
    pub note: String,
}

/// A piece of work as the window shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub name: String,
    /// What it is doing now.
    pub step: String,
    pub done: u64,
    pub total: u64,
    pub items: Vec<Item>,
    pub finished: bool,
}

impl Job {
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        (self.done as f32 / self.total as f32).clamp(0.0, 1.0)
    }

    fn count(&self) -> String {
        format!("{} / {}", self.done, self.total)
    }
}

/// A project being opened, to picture the splash and the window.
fn opening() -> Job {
    let item = |name: &str, stage, note: &str| Item {
        name: name.to_owned(),
        stage,
        note: note.to_owned(),
    };
    Job {
        name: "Opening DevTest1".to_owned(),
        step: "Building Maria Kujou".to_owned(),
        done: 2,
        total: 5,
        items: vec![
            item("Content index", Stage::Done, "0.2 s"),
            item("Default female", Stage::Done, "3.4 s"),
            item("Maria Kujou", Stage::Running, "Clothing"),
            item("Animations", Stage::Waiting, ""),
            item("Viewport", Stage::Waiting, ""),
        ],
        finished: false,
    }
}

/// Work in the background, to picture the notices.
fn background() -> Vec<Job> {
    let job = |name: &str, step: &str, done, total, finished| Job {
        name: name.to_owned(),
        step: step.to_owned(),
        done,
        total,
        items: Vec::new(),
        finished,
    };
    vec![
        job("Imported BR-Chan", "6 mods, 60.9 MB", 1, 1, true),
        job("Face shots", "", 12, 41, false),
        job("Importing animations", "Dance 09", 9, 24, false),
    ]
}

/// A thin bar filled as far as the work is.
fn bar(p: &egui::Painter, k: &Look, rect: Rect, fraction: f32) {
    let t = &k.t;
    p.rect_filled(rect, 0.0, t.input);
    let done = Rect::from_min_size(rect.min, Vec2::new((rect.width() * fraction).round(), rect.height()));
    p.rect_filled(done, 0.0, t.select);
}

/// The mark of a stage.
fn mark(p: &egui::Painter, k: &Look, at: Pos2, stage: Stage) {
    let t = &k.t;
    let (name, color) = match stage {
        Stage::Done => (icon::CHECK, t.play),
        Stage::Running => (icon::CIRCLE_NOTCH, t.select_bright),
        Stage::Waiting => (icon::CIRCLE, t.border),
    };
    glyph(p, k, at, name, 14.0, color);
}

/// The splash: the art over the whole window, the name, and what is
/// being loaded along the bottom.
fn splash(ctx: &egui::Context, k: &Look) {
    let t = &k.t;
    let job = opening();
    let full = ctx.content_rect();
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("splash")));
    p.rect_filled(full, 0.0, t.input);
    let mut x = full.left() - full.height();
    while x < full.right() {
        p.line_segment(
            [Pos2::new(x, full.bottom()), Pos2::new(x + full.height(), full.top())],
            Stroke::new(1.0, t.well),
        );
        x += 22.0;
    }
    let slot = format!("Splash art  {:.0} × {:.0}", full.width(), full.height());
    p.text(
        Pos2::new(full.right() - 18.0, full.top() + 20.0),
        Align2::RIGHT_CENTER,
        slot,
        k.small(),
        t.text_dim,
    );
    // The text stays readable on any picture.
    let shade = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - 150.0), full.max);
    let mut mesh = egui::Mesh::default();
    let dark = Color32::from_black_alpha(215);
    mesh.colored_vertex(shade.left_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(shade.right_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(shade.right_bottom(), dark);
    mesh.colored_vertex(shade.left_bottom(), dark);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(mesh);

    let left = full.left() + 24.0;
    p.text(
        Pos2::new(left, full.bottom() - 74.0),
        Align2::LEFT_CENTER,
        "Renkatsu Studio",
        k.bold(24.0),
        t.text_bright,
    );
    p.text(
        Pos2::new(left, full.bottom() - 48.0),
        Align2::LEFT_CENTER,
        format!("Version {}", env!("CARGO_PKG_VERSION")),
        k.small(),
        t.text_dim,
    );
    let y = full.bottom() - 20.0;
    p.text(Pos2::new(left, y), Align2::LEFT_CENTER, &job.step, k.font(), t.text);
    p.text(
        Pos2::new(full.right() - 24.0, y),
        Align2::RIGHT_CENTER,
        job.count(),
        k.small(),
        t.text_dim,
    );
    let line = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - 3.0), full.max);
    bar(&p, k, line, job.fraction());
    p.rect_stroke(full, 0.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
}

const WIDTH: f32 = 440.0;

/// The window that waits for a job: its name, the step, the bar and,
/// with `list`, what it loads.
fn modal(ctx: &egui::Context, k: &Look, list: bool) {
    let t = &k.t;
    let job = opening();
    let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::symmetric(16, 14));
    egui::Modal::new(egui::Id::new("loading")).frame(frame).show(ctx, |ui| {
        ui.set_width(WIDTH);
        let rows = if list {
            job.items.len() as f32 * 26.0 + 14.0
        } else {
            0.0
        };
        let (rect, _) = ui.allocate_exact_size(Vec2::new(WIDTH, 78.0 + rows + 28.0), egui::Sense::hover());
        let p = ui.painter().clone();
        let (left, right) = (rect.left(), rect.right());
        p.text(
            Pos2::new(left, rect.top() + 10.0),
            Align2::LEFT_CENTER,
            &job.name,
            k.bold(15.0),
            t.text_bright,
        );
        let y = rect.top() + 44.0;
        p.text(Pos2::new(left, y), Align2::LEFT_CENTER, &job.step, k.font(), t.text);
        p.text(
            Pos2::new(right, y),
            Align2::RIGHT_CENTER,
            job.count(),
            k.small(),
            t.text_dim,
        );
        let line = Rect::from_min_size(Pos2::new(left, y + 16.0), Vec2::new(WIDTH, 4.0));
        bar(&p, k, line, job.fraction());

        let mut y = line.bottom() + 14.0;
        if list {
            boxed(
                &p,
                Rect::from_min_size(Pos2::new(left, y), Vec2::new(WIDTH, rows - 14.0)),
                k.radius(),
                t.input,
                t.border,
            );
            for (n, item) in job.items.iter().enumerate() {
                let row = Rect::from_min_size(Pos2::new(left, y), Vec2::new(WIDTH, 26.0));
                if n > 0 {
                    hair_h(&p, row.left() + 1.0, row.right() - 1.0, row.top(), t.line);
                }
                let middle = row.center().y;
                mark(&p, k, Pos2::new(row.left() + 16.0, middle), item.stage);
                let color = match item.stage {
                    Stage::Running => t.text_bright,
                    Stage::Done => t.text,
                    Stage::Waiting => t.text_dim,
                };
                p.text(
                    Pos2::new(row.left() + 32.0, middle),
                    Align2::LEFT_CENTER,
                    &item.name,
                    k.font(),
                    color,
                );
                p.text(
                    Pos2::new(row.right() - 10.0, middle),
                    Align2::RIGHT_CENTER,
                    &item.note,
                    k.small(),
                    t.text_dim,
                );
                y += 26.0;
            }
            y += 14.0;
        }
        let cancel = Rect::from_min_size(Pos2::new(right - 96.0, y), Vec2::new(96.0, 28.0));
        selector::button(ui, k, cancel, "Cancel", false, true);
    });
}

/// The notices: one block a job, stacked from the bottom right corner of
/// the editor, above the status bar.
fn notices(ctx: &egui::Context, k: &Look) {
    let t = &k.t;
    let full = ctx.content_rect();
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("notices")));
    let size = Vec2::new(300.0, 56.0);
    let mut bottom = full.bottom() - k.d.status_height - 12.0;
    for job in background().iter().rev() {
        let rect = Rect::from_min_size(Pos2::new(full.right() - 12.0 - size.x, bottom - size.y), size);
        p.rect_filled(
            rect.translate(Vec2::new(0.0, 2.0)).expand(1.0),
            0.0,
            Color32::from_black_alpha(70),
        );
        boxed(&p, rect, 0.0, t.menu, t.border);
        let y = rect.top() + 19.0;
        let stage = if job.finished { Stage::Done } else { Stage::Running };
        mark(&p, k, Pos2::new(rect.left() + 18.0, y), stage);
        p.text(
            Pos2::new(rect.left() + 34.0, y),
            Align2::LEFT_CENTER,
            &job.name,
            k.font(),
            t.text_bright,
        );
        glyph(&p, k, Pos2::new(rect.right() - 16.0, y), icon::X, 12.0, t.text_dim);
        let under = match (job.finished, job.step.is_empty()) {
            (true, _) => job.step.clone(),
            (false, true) => job.count(),
            (false, false) => format!("{}   {}", job.count(), job.step),
        };
        p.text(
            Pos2::new(rect.left() + 34.0, y + 19.0),
            Align2::LEFT_CENTER,
            under,
            k.small(),
            t.text_dim,
        );
        if !job.finished {
            let line = Rect::from_min_max(
                Pos2::new(rect.left() + 1.0, rect.bottom() - 3.0),
                rect.max - Vec2::splat(1.0),
            );
            bar(&p, k, line, job.fraction());
        }
        bottom = rect.top() - 8.0;
    }
}

/// The line in the status bar: the job that runs, its bar, and how many
/// more wait behind it.
fn status(ctx: &egui::Context, k: &Look) {
    let t = &k.t;
    let jobs = background();
    let Some(job) = jobs.iter().rev().find(|j| !j.finished) else {
        return;
    };
    let more = jobs.iter().filter(|j| !j.finished).count() - 1;
    let full = ctx.content_rect();
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("status job")));
    let y = full.bottom() - k.d.status_height / 2.0 + 0.5;
    let mut x = full.center().x - 170.0;
    mark(&p, k, Pos2::new(x, y), Stage::Running);
    x = p
        .text(
            Pos2::new(x + 12.0, y),
            Align2::LEFT_CENTER,
            &job.name,
            k.small(),
            t.text,
        )
        .right();
    let line = Rect::from_min_size(Pos2::new(x + 12.0, y - 2.0), Vec2::new(120.0, 4.0));
    bar(&p, k, line, job.fraction());
    x = p
        .text(
            Pos2::new(line.right() + 10.0, y),
            Align2::LEFT_CENTER,
            job.count(),
            k.small(),
            t.text_dim,
        )
        .right();
    if more > 0 {
        x = p
            .text(
                Pos2::new(x + 12.0, y),
                Align2::LEFT_CENTER,
                format!("+{more}"),
                k.small(),
                t.text_dim,
            )
            .right();
    }
    glyph(&p, k, Pos2::new(x + 16.0, y), icon::X, 11.0, t.text_dim);
}

/// Draws the pictured way of showing work over the window.
pub fn picture(ui: &mut Ui, k: &Look, which: Picture) {
    let ctx = ui.ctx().clone();
    match which {
        Picture::Splash => splash(&ctx, k),
        Picture::Modal => modal(&ctx, k, false),
        Picture::ModalList => modal(&ctx, k, true),
        Picture::Notices => notices(&ctx, k),
        Picture::Status => status(&ctx, k),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_is_as_far_as_its_count() {
        let mut job = opening();
        assert_eq!(job.fraction(), 0.4);
        job.total = 0;
        assert_eq!(job.fraction(), 0.0);
        job.total = 1;
        assert_eq!(job.fraction(), 1.0);
    }
}
