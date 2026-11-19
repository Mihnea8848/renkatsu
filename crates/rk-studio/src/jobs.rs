// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Work that takes long, off the window's thread: named jobs that report
//! a step, a count and a list of what they load, can be nested and can be
//! told to stop. The window only looks at them: a splash while a project
//! opens, notices in the corner, a line in the status bar and a window
//! that waits. What a job made is taken in on the window's thread, once a
//! frame.

use crate::state::{Look, State};
use crate::widgets::{boxed, glyph, hair_h};
use anyhow::{anyhow, Result};
use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use egui_phosphor::regular as icon;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};

/// Seconds a finished notice stays before it fades, and the fade.
pub const KEEP: f64 = 4.0;
const FADE: f64 = 0.4;
/// Seconds a job that makes the window wait runs before its window shows.
pub const WAIT_DELAY: f64 = 0.3;

/// How far one thing of a job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Waiting,
    Running,
    Done,
    Failed,
}

/// One thing a job loads.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: String,
    pub stage: Stage,
    /// What it says at its right: the time it took, or how far it is.
    pub note: String,
}

#[derive(Default)]
struct Text {
    step: String,
    items: Vec<Item>,
}

struct Shared {
    text: Mutex<Text>,
    done: AtomicU64,
    total: AtomicU64,
    /// The count is of bytes.
    bytes: AtomicBool,
    /// One word to stop for a job and everything nested in it.
    cancel: Arc<AtomicBool>,
    children: Mutex<Vec<(f32, Progress)>>,
    wake: Option<egui::Context>,
}

/// What a worker sees of its job: it reports how far it is and looks for
/// the word to stop.
#[derive(Clone)]
pub struct Progress(Arc<Shared>);

impl Progress {
    pub fn new(wake: Option<egui::Context>) -> Progress {
        Progress::with(wake, Arc::new(AtomicBool::new(false)))
    }

    fn with(wake: Option<egui::Context>, cancel: Arc<AtomicBool>) -> Progress {
        Progress(Arc::new(Shared {
            text: Mutex::default(),
            done: AtomicU64::new(0),
            total: AtomicU64::new(0),
            bytes: AtomicBool::new(false),
            cancel,
            children: Mutex::default(),
            wake,
        }))
    }

    fn woke(&self) {
        if let Some(ctx) = &self.0.wake {
            ctx.request_repaint();
        }
    }

    /// Says what the job is doing now.
    pub fn step(&self, text: &str) {
        self.0.text.lock().expect("no panic holds it").step = text.to_owned();
        self.woke();
    }

    pub fn step_text(&self) -> String {
        self.0.text.lock().expect("no panic holds it").step.clone()
    }

    /// Says how many of how many are done.
    pub fn set(&self, done: u64, total: u64) {
        self.0.done.store(done, Ordering::Relaxed);
        self.0.total.store(total, Ordering::Relaxed);
        self.woke();
    }

    /// The same for a count of bytes, which reads as a size.
    pub fn set_bytes(&self, done: u64, total: u64) {
        self.0.bytes.store(true, Ordering::Relaxed);
        self.set(done, total);
    }

    /// Counts some more as done.
    pub fn add(&self, more: u64) {
        self.0.done.fetch_add(more, Ordering::Relaxed);
        self.woke();
    }

    pub fn counts(&self) -> (u64, u64) {
        (
            self.0.done.load(Ordering::Relaxed),
            self.0.total.load(Ordering::Relaxed),
        )
    }

    /// The count as text, or nothing while there is nothing to count.
    pub fn count(&self) -> String {
        let (done, total) = self.counts();
        if total == 0 {
            String::new()
        } else if self.0.bytes.load(Ordering::Relaxed) {
            format!("{} / {}", crate::mods::size_text(done), crate::mods::size_text(total))
        } else {
            format!("{done} / {total}")
        }
    }

    /// Puts the whole list of what the job loads in place.
    pub fn set_items(&self, items: Vec<Item>) {
        self.0.text.lock().expect("no panic holds it").items = items;
    }

    pub fn items(&self) -> Vec<Item> {
        self.0.text.lock().expect("no panic holds it").items.clone()
    }

    /// Asks the work to stop; it does at its next step.
    pub fn cancel(&self) {
        self.0.cancel.store(true, Ordering::Relaxed);
        self.woke();
    }

    pub fn cancelled(&self) -> bool {
        self.0.cancel.load(Ordering::Relaxed)
    }

    /// Fails when the work was asked to stop, for use between steps.
    pub fn check(&self) -> Result<()> {
        if self.cancelled() {
            return Err(anyhow!("cancelled"));
        }
        Ok(())
    }

    /// A part of this job with a count of its own, which fills `share`
    /// of this job's bar. A word to stop reaches it too.
    pub fn child(&self, share: f32) -> Progress {
        let child = Progress::with(self.0.wake.clone(), self.0.cancel.clone());
        self.0
            .children
            .lock()
            .expect("no panic holds it")
            .push((share.clamp(0.0, 1.0), child.clone()));
        child
    }

    /// How far the job is, from 0 to 1: its parts by their shares, and
    /// its own count for what the parts leave.
    pub fn fraction(&self) -> f32 {
        let (done, total) = self.counts();
        let own = if total == 0 {
            0.0
        } else {
            (done as f32 / total as f32).clamp(0.0, 1.0)
        };
        let children = self.0.children.lock().expect("no panic holds it");
        let shares: f32 = children.iter().map(|(share, _)| share).sum();
        // Shares that add up to more than the whole are scaled to it.
        let scale = if shares > 1.0 { 1.0 / shares } else { 1.0 };
        let parts: f32 = children
            .iter()
            .map(|(share, child)| share * scale * child.fraction())
            .sum();
        (parts + (1.0 - shares * scale) * own).clamp(0.0, 1.0)
    }
}

/// How a job shows in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Show {
    /// A window over the editor, once the job has run a moment.
    Wait,
    /// A notice in the corner and the status bar's line.
    Notice,
    /// Nowhere.
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(u64);

/// How a job ended, or that it has not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Running,
    Done,
    Failed(String),
    Cancelled,
}

/// What a finished job does to the program's state.
type Apply<S> = Box<dyn FnOnce(&mut S)>;
type Ended = Arc<Mutex<Option<Result<(), String>>>>;

struct Entry<S> {
    id: JobId,
    name: String,
    /// What it is called once it is done, when that reads otherwise.
    done_name: Option<String>,
    show: Show,
    progress: Progress,
    started: f64,
    /// When the end was seen, and what it was.
    ended_at: Option<f64>,
    outcome: Outcome,
    /// Set by the worker when the work returned.
    ended: Ended,
    /// Looks for the result and wraps it for the window's thread.
    finish: Option<Box<dyn FnMut() -> Option<Apply<S>>>>,
}

/// The result of a job, for the one who started it.
pub struct Handle<T> {
    result: Receiver<Result<T>>,
    pub progress: Progress,
}

impl<T> Handle<T> {
    /// The result, once it is in.
    pub fn try_take(&self) -> Option<Result<T>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(anyhow!("the work stopped without a result"))),
        }
    }

    /// Waits for the result.
    pub fn wait(&self) -> Result<T> {
        self.result
            .recv()
            .unwrap_or_else(|_| Err(anyhow!("the work stopped without a result")))
    }
}

/// A job that runs somewhere else and only shows here: its owner reports
/// through `progress` and says when it ended.
pub struct Tracker {
    pub progress: Progress,
    ended: Ended,
}

impl Tracker {
    /// Says how the work ended.
    pub fn end(self, result: Result<(), String>) {
        *self.ended.lock().expect("no panic holds it") = Some(result);
        self.progress.woke();
    }
}

/// Whether an error is the word to stop, come back from the work.
pub fn was_cancelled(error: &anyhow::Error) -> bool {
    format!("{error:#}").ends_with("cancelled")
}

/// How many heavy jobs run at once: the others wait their turn.
struct Pool {
    free: Mutex<usize>,
    turn: Condvar,
}

impl Pool {
    fn enter(&self) {
        let mut free = self.free.lock().expect("no panic holds it");
        while *free == 0 {
            free = self.turn.wait(free).expect("no panic holds it");
        }
        *free -= 1;
    }

    fn leave(&self) {
        *self.free.lock().expect("no panic holds it") += 1;
        self.turn.notify_one();
    }
}

/// The jobs that run and the ones that just ended, in the order they
/// were started.
pub struct Jobs<S = State> {
    entries: Vec<Entry<S>>,
    next: u64,
    /// The time of the last frame, in the window's seconds.
    now: f64,
    /// The window, which a worker wakes when it has something to show.
    pub wake: Option<egui::Context>,
    pool: Arc<Pool>,
}

impl<S> Default for Jobs<S> {
    fn default() -> Self {
        // Two cores stay free for the window and the system.
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        Jobs {
            entries: Vec::new(),
            next: 0,
            now: 0.0,
            wake: None,
            pool: Arc::new(Pool {
                free: Mutex::new(cores.saturating_sub(2).max(1)),
                turn: Condvar::new(),
            }),
        }
    }
}

/// What the window draws of a job.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub id: JobId,
    pub name: String,
    pub step: String,
    pub count: String,
    pub fraction: f32,
    pub outcome: Outcome,
    pub show: Show,
    /// Seconds since it started.
    pub age: f64,
    /// How much of it still shows while it fades, from 1 to 0.
    pub opacity: f32,
}

/// How much of a finished notice shows `since` seconds after its end.
pub fn fading(since: f64) -> f32 {
    (1.0 - (since - KEEP) / FADE).clamp(0.0, 1.0) as f32
}

impl<S: 'static> Jobs<S> {
    /// Runs `work` on a thread of its own and hands back where its result
    /// arrives. A panic in the work is an error like any other. `heavy`
    /// work waits for a free core first.
    pub fn start<T: Send + 'static>(
        &mut self,
        name: &str,
        show: Show,
        heavy: bool,
        work: impl FnOnce(&Progress) -> Result<T> + Send + 'static,
    ) -> Handle<T> {
        let progress = Progress::new(self.wake.clone());
        let ended: Ended = Arc::default();
        let (send, result) = channel();
        let (theirs, flag, pool, wake) = (progress.clone(), ended.clone(), self.pool.clone(), self.wake.clone());
        let label = name.to_owned();
        let run = move || {
            if heavy {
                pool.enter();
            }
            let done = if theirs.cancelled() {
                Err(anyhow!("cancelled"))
            } else {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&theirs))).unwrap_or_else(|panic| {
                    let why = panic
                        .downcast_ref::<&str>()
                        .map(|s| (*s).to_owned())
                        .or_else(|| panic.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "it stopped".to_owned());
                    Err(anyhow!("{label}: {why}"))
                })
            };
            if heavy {
                pool.leave();
            }
            *flag.lock().expect("no panic holds it") = Some(done.as_ref().map(|_| ()).map_err(|e| format!("{e:#}")));
            let _ = send.send(done);
            if let Some(ctx) = wake {
                ctx.request_repaint();
            }
        };
        if let Err(e) = std::thread::Builder::new().name(name.to_owned()).spawn(run) {
            *ended.lock().expect("no panic holds it") = Some(Err(format!("no thread could be started: {e}")));
        }
        self.next += 1;
        self.entries.push(Entry {
            id: JobId(self.next),
            name: name.to_owned(),
            done_name: None,
            show,
            progress: progress.clone(),
            started: self.now,
            ended_at: None,
            outcome: Outcome::Running,
            ended,
            finish: None,
        });
        Handle { result, progress }
    }

    /// Gives the job that was started last another name for when it is
    /// done: "Loading Maria" then reads "Loaded Maria".
    pub fn name_when_done(&mut self, name: &str) {
        if let Some(entry) = self.entries.last_mut() {
            entry.done_name = Some(name.to_owned());
        }
    }

    /// Shows work that runs somewhere else as a job.
    pub fn track(&mut self, name: &str, show: Show) -> Tracker {
        let progress = Progress::new(self.wake.clone());
        let ended: Ended = Arc::default();
        self.next += 1;
        self.entries.push(Entry {
            id: JobId(self.next),
            name: name.to_owned(),
            done_name: None,
            show,
            progress: progress.clone(),
            started: self.now,
            ended_at: None,
            outcome: Outcome::Running,
            ended: ended.clone(),
            finish: None,
        });
        Tracker { progress, ended }
    }

    /// Runs `work` on a thread; `done` gets its result on the window's
    /// thread, with the state, at the next [`Jobs::poll`].
    pub fn spawn<T: Send + 'static>(
        &mut self,
        name: &str,
        show: Show,
        work: impl FnOnce(&Progress) -> Result<T> + Send + 'static,
        done: impl FnOnce(&mut S, Result<T>) + 'static,
    ) -> JobId {
        let handle = self.start(name, show, false, work);
        let mut done = Some(done);
        let entry = self.entries.last_mut().expect("just started");
        entry.finish = Some(Box::new(move || {
            let result = handle.try_take()?;
            let done = done.take()?;
            Some(Box::new(move |state: &mut S| done(state, result)) as Apply<S>)
        }));
        entry.id
    }

    /// Looks at every job once a frame, at the window's time `now`, and
    /// returns what the finished ones do to the state, in the order they
    /// were started. Notices that have had their time go away.
    pub fn poll(&mut self, now: f64) -> Vec<Apply<S>> {
        self.now = now;
        let mut applies = Vec::new();
        for entry in &mut self.entries {
            if entry.outcome == Outcome::Running {
                let ended = entry.ended.lock().expect("no panic holds it").take();
                if let Some(result) = ended {
                    entry.outcome = match result {
                        Ok(()) => Outcome::Done,
                        Err(_) if entry.progress.cancelled() => Outcome::Cancelled,
                        Err(why) => Outcome::Failed(why),
                    };
                    entry.ended_at = Some(now);
                }
            }
            if entry.outcome != Outcome::Running {
                if let Some(finish) = &mut entry.finish {
                    if let Some(apply) = finish() {
                        applies.push(apply);
                        entry.finish = None;
                    }
                }
            }
        }
        self.entries.retain(|e| {
            let since = now - e.ended_at.unwrap_or(now);
            match (&e.outcome, e.show) {
                (Outcome::Running, _) => true,
                // A result that was not taken yet keeps its job.
                _ if e.finish.is_some() => true,
                // An error stays until it is closed.
                (Outcome::Failed(_), Show::Wait | Show::Notice) => true,
                (Outcome::Done, Show::Notice) => since < KEEP + FADE,
                _ => false,
            }
        });
        applies
    }

    /// Asks a job to stop.
    pub fn cancel(&mut self, id: JobId) {
        if let Some(entry) = self.entries.iter().find(|e| e.id == id) {
            entry.progress.cancel();
        }
    }

    /// Takes a job that ended off the list: its notice was closed.
    pub fn close(&mut self, id: JobId) {
        self.entries
            .retain(|e| e.id != id || e.outcome == Outcome::Running || e.finish.is_some());
    }

    /// Whether any job runs.
    pub fn busy(&self) -> bool {
        self.entries.iter().any(|e| e.outcome == Outcome::Running)
    }

    /// The jobs that show, in the order they were started.
    pub fn views(&self) -> Vec<View> {
        self.entries
            .iter()
            .filter(|e| e.show != Show::Quiet)
            .map(|e| View {
                id: e.id,
                name: match (&e.outcome, &e.done_name) {
                    (Outcome::Done, Some(done)) => done.clone(),
                    _ => e.name.clone(),
                },
                step: e.progress.step_text(),
                count: e.progress.count(),
                fraction: e.progress.fraction(),
                outcome: e.outcome.clone(),
                show: e.show,
                age: self.now - e.started,
                opacity: match (&e.outcome, e.ended_at) {
                    (Outcome::Done, Some(at)) => fading(self.now - at),
                    _ => 1.0,
                },
            })
            .collect()
    }

    /// A job that is only shown, for pictures of the window: `outcome`
    /// says how it stands, `step` and the counts how far it is.
    pub fn pictured(&mut self, name: &str, show: Show, step: &str, done: u64, total: u64, outcome: Outcome) {
        let progress = Progress::new(None);
        progress.step(step);
        progress.set(done, total);
        self.next += 1;
        self.entries.push(Entry {
            id: JobId(self.next),
            name: name.to_owned(),
            done_name: None,
            show,
            progress,
            // Old enough for a window that waits to show.
            started: self.now - 1.0,
            // No end to count from, so a finished one does not fade.
            ended_at: None,
            outcome,
            ended: Arc::default(),
            // Never taken, so the job stays for the picture.
            finish: Some(Box::new(|| None)),
        });
    }
}

// What the window draws.

/// A thin bar filled as far as the work is.
fn bar(p: &egui::Painter, k: &Look, rect: Rect, fraction: f32) {
    let t = &k.t;
    p.rect_filled(rect, 0.0, t.input);
    let done = Rect::from_min_size(rect.min, Vec2::new((rect.width() * fraction).round(), rect.height()));
    p.rect_filled(done, 0.0, t.select);
}

/// A ring with a gap that turns while work runs.
fn spinner(ui: &Ui, p: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    // A style without animations, as scripted pictures have, holds it still.
    let turning = ui.ctx().global_style().animation_time > 0.0;
    let start = if turning {
        ui.ctx().request_repaint();
        ui.input(|i| i.time) as f32 * 5.0
    } else {
        -1.2
    };
    let points: Vec<Pos2> = (0..=20)
        .map(|n| {
            let angle = start + n as f32 / 20.0 * std::f32::consts::TAU * 0.75;
            center + radius * Vec2::new(angle.cos(), angle.sin())
        })
        .collect();
    p.add(egui::Shape::line(points, Stroke::new(1.5, color)));
}

/// The mark of a stage.
fn mark(ui: &Ui, p: &egui::Painter, k: &Look, at: Pos2, stage: Stage) {
    let t = &k.t;
    match stage {
        Stage::Done => glyph(p, k, at, icon::CHECK, 14.0, t.play),
        Stage::Running => spinner(ui, p, at, 5.5, t.select_bright),
        Stage::Waiting => glyph(p, k, at, icon::CIRCLE, 14.0, t.border),
        Stage::Failed => glyph(p, k, at, icon::WARNING, 14.0, t.accent),
    }
}

/// What a splash says.
pub struct Splash<'a> {
    /// The large line: the program, or the project that opens.
    pub title: &'a str,
    /// The small line under it.
    pub sub: &'a str,
    pub step: &'a str,
    pub count: &'a str,
    /// How far the work is; `None` draws no bar.
    pub fraction: Option<f32>,
    pub items: &'a [Item],
    /// Whether the work can be stopped.
    pub cancel: bool,
    /// How much of the splash shows, from 0 to 1.
    pub opacity: f32,
}

/// The splash: the art over the whole of `full`, the name on it, what
/// loads listed at the right and the step along the bottom. Returns
/// whether Cancel was clicked.
pub fn splash(ui: &mut Ui, k: &Look, full: Rect, what: &Splash) -> bool {
    let t = &k.t;
    ui.set_opacity(what.opacity);
    crate::brand::art(ui, k, full, crate::brand::Art::Splash);
    let p = ui.painter().clone();
    // The text stays readable on any picture.
    let shade = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - 190.0), full.max);
    crate::brand::shade(&p, shade, 225);

    let left = full.left() + 28.0;
    let right = full.right() - 28.0;
    let mut clicked = false;
    if !what.items.is_empty() {
        let row = 26.0;
        let width = 300.0;
        let height = what.items.len() as f32 * row;
        let list = Rect::from_min_size(
            Pos2::new(right - width, full.bottom() - 64.0 - height),
            Vec2::new(width, height),
        );
        boxed(&p, list, k.radius(), t.input.gamma_multiply(0.92), t.border);
        for (n, item) in what.items.iter().enumerate() {
            let at = Rect::from_min_size(
                Pos2::new(list.left(), list.top() + n as f32 * row),
                Vec2::new(width, row),
            );
            if n > 0 {
                hair_h(&p, at.left() + 1.0, at.right() - 1.0, at.top(), t.line);
            }
            let middle = at.center().y;
            mark(ui, &p, k, Pos2::new(at.left() + 16.0, middle), item.stage);
            let color = match item.stage {
                Stage::Running => t.text_bright,
                Stage::Done => t.text,
                Stage::Waiting => t.text_dim,
                Stage::Failed => t.accent,
            };
            let note = p.layout_no_wrap(item.note.clone(), k.small(), t.text_dim);
            let room = width - 32.0 - 20.0 - note.size().x;
            crate::selector::line(
                ui,
                Pos2::new(at.left() + 32.0, middle),
                room,
                &item.name,
                k.font(),
                color,
            );
            p.galley(
                Pos2::new(at.right() - 10.0 - note.size().x, middle - note.size().y / 2.0),
                note,
                t.text_dim,
            );
        }
    }
    p.text(
        Pos2::new(left, full.bottom() - 92.0),
        Align2::LEFT_CENTER,
        what.title,
        k.bold(26.0),
        t.text_bright,
    );
    p.text(
        Pos2::new(left, full.bottom() - 64.0),
        Align2::LEFT_CENTER,
        what.sub,
        k.small(),
        t.text_dim,
    );
    let y = full.bottom() - 26.0;
    p.text(Pos2::new(left, y), Align2::LEFT_CENTER, what.step, k.font(), t.text);
    let mut x = right;
    if what.cancel {
        let button = Rect::from_min_size(Pos2::new(right - 84.0, y - 13.0), Vec2::new(84.0, 26.0));
        clicked = crate::selector::button(ui, k, button, "Cancel", false, true);
        x = button.left() - 14.0;
    }
    p.text(Pos2::new(x, y), Align2::RIGHT_CENTER, what.count, k.small(), t.text_dim);
    if let Some(fraction) = what.fraction {
        let line = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - 3.0), full.max);
        bar(&p, k, line, fraction);
    }
    clicked
}

/// The line under a notice's name.
fn under(view: &View) -> String {
    match (&view.outcome, view.count.is_empty(), view.step.is_empty()) {
        (Outcome::Failed(why), ..) => why.clone(),
        (Outcome::Cancelled, ..) => "Cancelled".to_owned(),
        (Outcome::Done, ..) => view.step.clone(),
        (Outcome::Running, true, _) => view.step.clone(),
        (Outcome::Running, false, true) => view.count.clone(),
        (Outcome::Running, false, false) => format!("{}   {}", view.count, view.step),
    }
}

fn stage_of(outcome: &Outcome) -> Stage {
    match outcome {
        Outcome::Running => Stage::Running,
        Outcome::Done => Stage::Done,
        Outcome::Failed(_) | Outcome::Cancelled => Stage::Failed,
    }
}

/// The notices: one block a job, stacked from the bottom right corner of
/// the editor, above the status bar. The cross of a running one stops
/// it; the cross of a finished one closes it.
pub fn notices<S: 'static>(ctx: &egui::Context, k: &Look, jobs: &mut Jobs<S>) {
    let t = &k.t;
    let views: Vec<View> = jobs
        .views()
        .into_iter()
        .filter(|v| v.show == Show::Notice || matches!(v.outcome, Outcome::Failed(_)))
        .collect();
    if views.is_empty() {
        return;
    }
    let full = ctx.content_rect();
    let size = Vec2::new(300.0, 56.0);
    let mut bottom = full.bottom() - k.d.status_height - 12.0;
    for view in views.iter().rev() {
        let rect = Rect::from_min_size(Pos2::new(full.right() - 12.0 - size.x, bottom - size.y), size);
        let area = egui::Area::new(egui::Id::new(("notice", view.id)))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .interactable(true);
        area.show(ctx, |ui| {
            ui.set_opacity(view.opacity);
            ui.allocate_rect(rect, Sense::hover());
            let p = ui.painter().clone();
            p.rect_filled(
                rect.translate(Vec2::new(0.0, 2.0)).expand(1.0),
                0.0,
                Color32::from_black_alpha(70),
            );
            boxed(&p, rect, 0.0, t.menu, t.border);
            let y = rect.top() + 19.0;
            mark(ui, &p, k, Pos2::new(rect.left() + 18.0, y), stage_of(&view.outcome));
            crate::selector::line(
                ui,
                Pos2::new(rect.left() + 34.0, y),
                size.x - 34.0 - 34.0,
                &view.name,
                k.font(),
                t.text_bright,
            );
            let cross = Rect::from_center_size(Pos2::new(rect.right() - 16.0, y), Vec2::splat(20.0));
            let resp = ui.interact(cross, ui.id().with("cross"), Sense::click());
            let tint = if resp.hovered() { t.text_bright } else { t.text_dim };
            glyph(&p, k, cross.center(), icon::X, 12.0, tint);
            if resp.clicked() {
                if view.outcome == Outcome::Running {
                    jobs.cancel(view.id);
                } else {
                    jobs.close(view.id);
                }
            }
            let failed = matches!(view.outcome, Outcome::Failed(_));
            crate::selector::line(
                ui,
                Pos2::new(rect.left() + 34.0, y + 19.0),
                size.x - 34.0 - 12.0,
                &under(view),
                k.small(),
                if failed { t.accent } else { t.text_dim },
            );
            if view.outcome == Outcome::Running {
                let line = Rect::from_min_max(
                    Pos2::new(rect.left() + 1.0, rect.bottom() - 3.0),
                    rect.max - Vec2::splat(1.0),
                );
                bar(&p, k, line, view.fraction);
            }
        });
        bottom = rect.top() - 8.0;
    }
    if views.iter().any(|v| v.outcome == Outcome::Done) {
        // A finished notice goes away by itself.
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// The line in the status bar: the newest job that runs, its bar, and
/// how many more run beside it.
pub fn status_line<S: 'static>(ui: &Ui, k: &Look, jobs: &Jobs<S>, rect: Rect) {
    let t = &k.t;
    let running: Vec<View> = jobs
        .views()
        .into_iter()
        .filter(|v| v.outcome == Outcome::Running)
        .collect();
    let p = ui.painter().clone();
    let y = rect.center().y + 0.5;
    let mut x = rect.center().x - 170.0;
    let Some(view) = running.last() else {
        // Nothing runs: what finished last says so while its notice shows.
        let done = jobs
            .views()
            .into_iter()
            .rfind(|v| v.outcome == Outcome::Done && v.show == Show::Notice);
        if let Some(view) = done {
            let mut p = p.clone();
            p.set_opacity(view.opacity);
            mark(ui, &p, k, Pos2::new(x, y), Stage::Done);
            p.text(
                Pos2::new(x + 12.0, y),
                Align2::LEFT_CENTER,
                &view.name,
                k.small(),
                t.text,
            );
        }
        return;
    };
    let more = running.len() - 1;
    mark(ui, &p, k, Pos2::new(x, y), Stage::Running);
    x = p
        .text(
            Pos2::new(x + 12.0, y),
            Align2::LEFT_CENTER,
            &view.name,
            k.small(),
            t.text,
        )
        .right();
    let line = Rect::from_min_size(Pos2::new(x + 12.0, y - 2.0), Vec2::new(120.0, 4.0));
    bar(&p, k, line, view.fraction);
    x = line.right();
    if !view.count.is_empty() {
        x = p
            .text(
                Pos2::new(x + 10.0, y),
                Align2::LEFT_CENTER,
                &view.count,
                k.small(),
                t.text_dim,
            )
            .right();
    }
    if more > 0 {
        p.text(
            Pos2::new(x + 12.0, y),
            Align2::LEFT_CENTER,
            format!("+{more}"),
            k.small(),
            t.text_dim,
        );
    }
}

/// The window that waits for a job that keeps the editor: its name, the
/// step, the bar and Cancel. It shows once the job has run a moment, so
/// quick work shows nothing.
pub fn wait_window<S: 'static>(ctx: &egui::Context, k: &Look, jobs: &mut Jobs<S>) {
    let t = &k.t;
    let waiting = jobs
        .views()
        .into_iter()
        .find(|v| v.show == Show::Wait && v.outcome == Outcome::Running);
    let Some(view) = waiting else { return };
    if view.age < WAIT_DELAY {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        return;
    }
    const WIDTH: f32 = 440.0;
    let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::symmetric(16, 14));
    egui::Modal::new(egui::Id::new("waiting for a job"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(WIDTH, 106.0), Sense::hover());
            let p = ui.painter().clone();
            let (left, right) = (rect.left(), rect.right());
            p.text(
                Pos2::new(left, rect.top() + 10.0),
                Align2::LEFT_CENTER,
                &view.name,
                k.bold(15.0),
                t.text_bright,
            );
            let y = rect.top() + 44.0;
            crate::selector::line(ui, Pos2::new(left, y), WIDTH - 120.0, &view.step, k.font(), t.text);
            p.text(
                Pos2::new(right, y),
                Align2::RIGHT_CENTER,
                &view.count,
                k.small(),
                t.text_dim,
            );
            let line = Rect::from_min_size(Pos2::new(left, y + 16.0), Vec2::new(WIDTH, 4.0));
            bar(&p, k, line, view.fraction);
            let cancel = Rect::from_min_size(Pos2::new(right - 96.0, line.bottom() + 14.0), Vec2::new(96.0, 28.0));
            if crate::selector::button(ui, k, cancel, "Cancel", false, true) {
                jobs.cancel(view.id);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Polls until nothing runs, as the frames of a window would.
    fn settle(jobs: &mut Jobs<Vec<String>>, state: &mut Vec<String>, now: f64) {
        for _ in 0..2000 {
            for apply in jobs.poll(now) {
                apply(state);
            }
            if !jobs.busy() && jobs.entries.iter().all(|e| e.finish.is_none()) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("the jobs did not end");
    }

    #[test]
    fn a_bar_is_as_far_as_its_count_and_its_parts() {
        let job = Progress::new(None);
        assert_eq!(job.fraction(), 0.0);
        job.set(2, 5);
        assert_eq!(job.fraction(), 0.4);
        assert_eq!(job.count(), "2 / 5");
        job.set(9, 5);
        assert_eq!(job.fraction(), 1.0);
        // Two parts take a quarter and a half; the job's own count fills
        // the quarter they leave.
        let job = Progress::new(None);
        let (a, b) = (job.child(0.25), job.child(0.5));
        a.set(1, 1);
        b.set(1, 2);
        job.set(1, 2);
        assert!((job.fraction() - (0.25 + 0.25 + 0.125)).abs() < 1e-6);
        // A part of a part counts through both shares.
        let inner = b.child(1.0);
        inner.set(1, 1);
        assert!((job.fraction() - (0.25 + 0.5 + 0.125)).abs() < 1e-6);
        // Shares over the whole are scaled to it.
        let job = Progress::new(None);
        let (a, b) = (job.child(1.0), job.child(1.0));
        a.set(1, 1);
        b.set(0, 1);
        assert!((job.fraction() - 0.5).abs() < 1e-6);
        let bytes = Progress::new(None);
        bytes.set_bytes(1024 * 1024, 4 * 1024 * 1024);
        assert_eq!(bytes.count(), "1.0 MB / 4.0 MB");
    }

    #[test]
    fn a_job_lists_what_it_loads_and_says_its_step() {
        let job = Progress::new(None);
        let item = |name: &str, stage, note: &str| Item {
            name: name.to_owned(),
            stage,
            note: note.to_owned(),
        };
        job.set_items(vec![item("Content", Stage::Running, "")]);
        job.set_items(vec![
            item("Content", Stage::Done, "0.2 s"),
            item("Maria", Stage::Waiting, ""),
        ]);
        let items = job.items();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].stage, Stage::Done);
        assert_eq!(items[0].note, "0.2 s");
        job.step("Building Maria");
        assert_eq!(job.step_text(), "Building Maria");
    }

    #[test]
    fn results_reach_the_state_in_the_order_the_jobs_started() {
        let mut jobs: Jobs<Vec<String>> = Jobs::default();
        let mut state = Vec::new();
        jobs.spawn(
            "slow",
            Show::Quiet,
            |p| {
                std::thread::sleep(std::time::Duration::from_millis(30));
                p.set(1, 1);
                Ok("slow")
            },
            |state, result| state.push(result.unwrap().to_owned()),
        );
        jobs.spawn(
            "quick",
            Show::Quiet,
            |_| Ok(7),
            |state, result| state.push(format!("{}", result.unwrap())),
        );
        assert!(jobs.busy());
        // Both are in before the first look, so the order is the jobs'.
        std::thread::sleep(std::time::Duration::from_millis(120));
        settle(&mut jobs, &mut state, 1.0);
        assert_eq!(state, ["slow", "7"]);
        assert!(jobs.entries.is_empty());
    }

    #[test]
    fn a_panic_or_an_error_in_a_worker_is_a_failed_job() {
        let mut jobs: Jobs<Vec<String>> = Jobs::default();
        let mut state = Vec::new();
        jobs.spawn(
            "panics",
            Show::Notice,
            |_| -> Result<()> { panic!("the file is odd") },
            |state, result| state.push(format!("{:#}", result.unwrap_err())),
        );
        jobs.spawn(
            "fails",
            Show::Quiet,
            |_| -> Result<()> { Err(anyhow!("no such file")) },
            |state, result| state.push(format!("{:#}", result.unwrap_err())),
        );
        settle(&mut jobs, &mut state, 1.0);
        assert_eq!(state, ["panics: the file is odd", "no such file"]);
        // The notice of the failed job stays until it is closed, however
        // long; the quiet one is gone.
        jobs.poll(1000.0);
        let views = jobs.views();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].outcome, Outcome::Failed("panics: the file is odd".to_owned()));
        assert_eq!(under(&views[0]), "panics: the file is odd");
        jobs.close(views[0].id);
        assert!(jobs.views().is_empty());
    }

    #[test]
    fn a_job_stops_between_steps_when_asked() {
        let mut jobs: Jobs<Vec<String>> = Jobs::default();
        let mut state = Vec::new();
        let id = jobs.spawn(
            "copies",
            Show::Notice,
            |p| {
                let part = p.child(1.0);
                for n in 0..10_000 {
                    // The word to stop reaches the parts of a job.
                    part.check()?;
                    part.set(n, 10_000);
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Ok(())
            },
            |state, result| state.push(if result.is_err() { "stopped" } else { "done" }.to_owned()),
        );
        jobs.poll(0.0);
        assert_eq!(jobs.views()[0].outcome, Outcome::Running);
        jobs.cancel(id);
        settle(&mut jobs, &mut state, 1.0);
        assert_eq!(state, ["stopped"]);
        // A cancelled job leaves no notice.
        assert!(jobs.views().is_empty());
    }

    #[test]
    fn a_finished_notice_fades_after_its_time() {
        let mut jobs: Jobs<Vec<String>> = Jobs::default();
        let mut state = Vec::new();
        jobs.spawn(
            "imports",
            Show::Notice,
            |p| {
                p.step("6 mods, 60.9 MB");
                Ok(())
            },
            |_, _| {},
        );
        settle(&mut jobs, &mut state, 10.0);
        let view = &jobs.views()[0];
        assert_eq!((view.outcome.clone(), view.opacity), (Outcome::Done, 1.0));
        assert_eq!(under(view), "6 mods, 60.9 MB");
        jobs.poll(10.0 + KEEP - 0.1);
        assert_eq!(jobs.views()[0].opacity, 1.0);
        jobs.poll(10.0 + KEEP + FADE / 2.0);
        assert!((jobs.views()[0].opacity - 0.5).abs() < 1e-3);
        jobs.poll(10.0 + KEEP + FADE);
        assert!(jobs.views().is_empty());
        assert_eq!(fading(0.0), 1.0);
        assert_eq!(fading(KEEP + FADE * 2.0), 0.0);
    }

    #[test]
    fn a_handle_gives_the_result_to_the_one_who_started_the_job() {
        let mut jobs: Jobs<Vec<String>> = Jobs::default();
        let handle = jobs.start("Loading Maria", Show::Notice, true, |p| {
            p.set(3, 3);
            Ok(42)
        });
        jobs.name_when_done("Loaded Maria");
        jobs.poll(0.0);
        assert_eq!(handle.wait().unwrap(), 42);
        assert_eq!(handle.progress.counts(), (3, 3));
        // Done, it reads as loaded until its notice has had its time.
        std::thread::sleep(std::time::Duration::from_millis(20));
        jobs.poll(1.0);
        assert_eq!(jobs.views()[0].name, "Loaded Maria");
        jobs.poll(1.0 + KEEP + FADE);
        assert!(jobs.entries.is_empty());
    }

    #[test]
    fn tracked_work_shows_until_its_owner_ends_it() {
        let mut jobs: Jobs<Vec<String>> = Jobs::default();
        let tracker = jobs.track("Starter Content", Show::Notice);
        tracker.progress.set_bytes(512, 2048);
        tracker.progress.step("Downloading");
        jobs.poll(1.0);
        assert_eq!(under(&jobs.views()[0]), "512 bytes / 2 KB   Downloading");
        assert!(jobs.busy());
        tracker.end(Err("the download broke off".to_owned()));
        jobs.poll(2.0);
        assert_eq!(
            jobs.views()[0].outcome,
            Outcome::Failed("the download broke off".to_owned())
        );
        assert!(was_cancelled(&anyhow!("cancelled").context("copying a.zipmod")));
        assert!(!was_cancelled(&anyhow!("no room left")));
    }

    #[test]
    fn a_pictured_job_shows_as_told() {
        let mut jobs: Jobs<Vec<String>> = Jobs::default();
        jobs.pictured("Face shots", Show::Notice, "", 12, 41, Outcome::Running);
        jobs.poll(0.0);
        let view = &jobs.views()[0];
        assert_eq!(under(view), "12 / 41");
        assert!(view.age >= WAIT_DELAY);
    }
}
