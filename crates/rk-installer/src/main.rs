// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Renkatsu installer for Linux: one program that carries Renkatsu,
//! installs it for the user who runs it, and can download Starter Content
//! on the way, in the editor's look.

mod install;
mod payload;

use anyhow::Result;
use egui::{Align2, Color32, FontFamily, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use install::{Layout, Options};
use rk_packages::{size_text, Stage, Store, Task};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

// The editor's colours (rk-studio's theme), so the installer looks like it.
const INPUT: Color32 = Color32::from_rgb(0x0f, 0x0f, 0x0f);
const WELL: Color32 = Color32::from_rgb(0x15, 0x15, 0x15);
const PANEL: Color32 = Color32::from_rgb(0x24, 0x24, 0x24);
const HEADER: Color32 = Color32::from_rgb(0x2f, 0x2f, 0x2f);
const CONTROL: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);
const HOVER: Color32 = Color32::from_rgb(0x57, 0x57, 0x57);
const BORDER: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);
const DIVIDER: Color32 = Color32::from_rgb(0x0b, 0x0b, 0x0b);
const TEXT: Color32 = Color32::from_rgb(0xc0, 0xc0, 0xc0);
const BRIGHT: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
const DIM: Color32 = Color32::from_rgb(0x86, 0x86, 0x86);
const SELECT: Color32 = Color32::from_rgb(0x00, 0x70, 0xe0);
const SELECT_BRIGHT: Color32 = Color32::from_rgb(0x4c, 0xa2, 0xff);
const ACCENT: Color32 = Color32::from_rgb(0xf0, 0x8d, 0x00);
const STOP: Color32 = Color32::from_rgb(0xdc, 0x48, 0x40);

const SEMIBOLD: &str = "semibold";
const STARTER: &str = "starter-characters";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Welcome,
    Working,
    Done,
}

/// What the work thread says.
enum Note {
    Step(String),
    Finished(Result<(), String>),
}

struct Installer {
    page: Page,
    layout: Option<Layout>,
    menu: bool,
    desktop: bool,
    starter: bool,
    /// Starter Content is installed already.
    have_starter: bool,
    starter_size: Option<u64>,
    step: String,
    task: Arc<Task>,
    notes: Option<Receiver<Note>>,
    result: Option<Result<(), String>>,
    age: f32,
}

impl Installer {
    fn new() -> Installer {
        let known = rk_packages::index::known().into_iter().find(|k| k.id == STARTER);
        let have_starter = Store::user().ok().and_then(|s| s.find(STARTER)).is_some();
        Installer {
            page: Page::Welcome,
            layout: Layout::user(),
            menu: true,
            desktop: true,
            starter: !have_starter && known.is_some(),
            have_starter,
            starter_size: known.map(|k| k.size()),
            step: String::new(),
            task: Arc::new(Task::new()),
            notes: None,
            result: None,
            age: 0.0,
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        let Some(layout) = self.layout.clone() else { return };
        let options = Options {
            menu: self.menu,
            desktop: self.desktop,
        };
        let starter = self.starter && !self.have_starter;
        let task = self.task.clone();
        let (send, notes) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let tell = |note: Note| {
                let _ = send.send(note);
                ctx.request_repaint();
            };
            let run = || -> Result<()> {
                let files = payload::own()?;
                install::install(&files, &layout, options, &mut |s| tell(Note::Step(s.to_owned())))?;
                if starter {
                    tell(Note::Step("Getting Starter Content: Characters".to_owned()));
                    let known = rk_packages::index::known()
                        .into_iter()
                        .find(|k| k.id == STARTER)
                        .ok_or_else(|| anyhow::anyhow!("this installer does not know Starter Content"))?;
                    rk_packages::download::fetch(&known, &Store::user()?, &task)?;
                }
                Ok(())
            };
            tell(Note::Finished(run().map_err(|e| format!("{e:#}"))));
        });
        self.notes = Some(notes);
        self.page = Page::Working;
        self.age = 0.0;
    }

    fn poll(&mut self) {
        let Some(notes) = &self.notes else { return };
        while let Ok(note) = notes.try_recv() {
            match note {
                Note::Step(step) => self.step = step,
                Note::Finished(result) => {
                    self.result = Some(result);
                    self.page = Page::Done;
                    self.age = 0.0;
                }
            }
        }
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn font(size: f32) -> FontId {
    FontId::proportional(size)
}

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// A flat button in the editor's style; `primary` is the blue one.
fn button(ui: &mut egui::Ui, rect: Rect, text: &str, primary: bool, enabled: bool) -> bool {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(rect, ui.id().with(text), sense);
    let hot = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("hot"), enabled && resp.hovered(), 0.12);
    let (fill, color) = match (enabled, primary) {
        (false, _) => (HEADER, DIM),
        (true, true) => (mix(SELECT, SELECT_BRIGHT, hot), Color32::WHITE),
        (true, false) => (mix(CONTROL, HOVER, hot), mix(TEXT, BRIGHT, hot)),
    };
    ui.painter().rect_filled(rect, 3.0, fill);
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, text, font(13.0), color);
    resp.clicked()
}

/// A check box line with a hint under it.
fn check(ui: &mut egui::Ui, top_left: Pos2, width: f32, on: &mut bool, text: &str, hint: &str, enabled: bool) {
    let rect = Rect::from_min_size(top_left, Vec2::new(width, 40.0));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(rect, ui.id().with(text), sense);
    if resp.clicked() {
        *on = !*on;
    }
    let p = ui.painter();
    let boxed = Rect::from_min_size(top_left + Vec2::new(0.0, 2.0), Vec2::splat(16.0));
    let shown = *on && enabled;
    p.rect_filled(boxed, 2.0, if shown { SELECT } else { INPUT });
    p.rect_stroke(
        boxed,
        2.0,
        Stroke::new(1.0, if shown { SELECT } else { BORDER }),
        StrokeKind::Inside,
    );
    if shown {
        let c = boxed.center();
        p.line_segment(
            [c + Vec2::new(-4.0, 0.0), c + Vec2::new(-1.0, 3.0)],
            Stroke::new(1.6, Color32::WHITE),
        );
        p.line_segment(
            [c + Vec2::new(-1.0, 3.0), c + Vec2::new(4.5, -3.5)],
            Stroke::new(1.6, Color32::WHITE),
        );
    }
    let x = boxed.right() + 10.0;
    let color = if !enabled {
        DIM
    } else if resp.hovered() {
        BRIGHT
    } else {
        TEXT
    };
    p.text(
        Pos2::new(x, boxed.center().y),
        Align2::LEFT_CENTER,
        text,
        font(13.0),
        color,
    );
    p.text(
        Pos2::new(x, boxed.center().y + 18.0),
        Align2::LEFT_CENTER,
        hint,
        font(11.5),
        DIM,
    );
}

impl eframe::App for Installer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        let ctx = ui.ctx().clone();
        self.age += ctx.input(|i| i.stable_dt).min(0.1);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(PANEL))
            .show(ui, |ui| {
                let full = ui.max_rect();
                // A short fade and rise as each page comes in.
                let come = (self.age / 0.22).clamp(0.0, 1.0);
                let come = 1.0 - (1.0 - come).powi(3);
                ui.set_opacity(come);
                let rise = Vec2::new(0.0, ((1.0 - come) * 8.0).round());

                let side = Rect::from_min_max(full.min, Pos2::new(full.left() + 200.0, full.bottom()));
                let foot = Rect::from_min_max(Pos2::new(side.right(), full.bottom() - 60.0), full.max);
                let body = Rect::from_min_max(Pos2::new(side.right(), full.top()), foot.right_top());
                {
                    let p = ui.painter();
                    p.rect_filled(side, 0.0, WELL);
                    p.rect_filled(foot, 0.0, HEADER);
                    p.line_segment([side.right_top(), side.right_bottom()], Stroke::new(1.0, DIVIDER));
                    p.line_segment([foot.left_top(), foot.right_top()], Stroke::new(1.0, DIVIDER));
                    // The mark, as the editor's title bar draws it.
                    let mark = Rect::from_min_size(side.min + Vec2::new(24.0, 28.0), Vec2::splat(44.0));
                    p.rect_filled(mark, 4.0, ACCENT);
                    p.text(mark.center(), Align2::CENTER_CENTER, "R", bold(30.0), INPUT);
                    p.text(
                        Pos2::new(side.left() + 24.0, mark.bottom() + 24.0),
                        Align2::LEFT_CENTER,
                        "Renkatsu Studio",
                        bold(15.0),
                        BRIGHT,
                    );
                    p.text(
                        Pos2::new(side.left() + 24.0, mark.bottom() + 44.0),
                        Align2::LEFT_CENTER,
                        format!("Version {}", env!("CARGO_PKG_VERSION")),
                        font(12.0),
                        DIM,
                    );
                    for (n, (name, page)) in [
                        ("Options", Page::Welcome),
                        ("Installing", Page::Working),
                        ("Done", Page::Done),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let y = mark.bottom() + 96.0 + n as f32 * 30.0;
                        let on = self.page == page;
                        if on {
                            p.rect_filled(
                                Rect::from_min_size(Pos2::new(side.left(), y - 13.0), Vec2::new(2.0, 26.0)),
                                0.0,
                                SELECT,
                            );
                        }
                        p.text(
                            Pos2::new(side.left() + 24.0, y),
                            Align2::LEFT_CENTER,
                            name,
                            font(13.0),
                            if on { BRIGHT } else { DIM },
                        );
                    }
                }

                let x = body.left() + 32.0;
                let width = body.width() - 64.0;
                let top = body.top() + 32.0 + rise.y;
                match self.page {
                    Page::Welcome => {
                        let p = ui.painter();
                        let title = if self.layout.as_ref().is_some_and(Layout::installed) {
                            "Update Renkatsu Studio"
                        } else {
                            "Install Renkatsu Studio"
                        };
                        p.text(Pos2::new(x, top), Align2::LEFT_TOP, title, bold(18.0), BRIGHT);
                        let to = self
                            .layout
                            .as_ref()
                            .map(|l| format!("For your user, into {}", l.app.display()))
                            .unwrap_or_else(|| "No home folder is known, so Renkatsu cannot be installed.".to_owned());
                        p.text(Pos2::new(x, top + 30.0), Align2::LEFT_TOP, to, font(12.0), DIM);
                        let mut y = top + 72.0;
                        check(
                            ui,
                            Pos2::new(x, y),
                            width,
                            &mut self.menu,
                            "Add Renkatsu Studio to the application menu",
                            "With its icon, under Graphics.",
                            true,
                        );
                        y += 52.0;
                        check(
                            ui,
                            Pos2::new(x, y),
                            width,
                            &mut self.desktop,
                            "Put a shortcut on the desktop",
                            "",
                            true,
                        );
                        y += 52.0;
                        let (hint, enabled) = match (self.have_starter, self.starter_size) {
                            (true, _) => ("Already installed.".to_owned(), false),
                            (false, Some(size)) => (
                                format!(
                                    "Koikatsu's own characters, so Renkatsu works without the game. {} download.",
                                    size_text(size)
                                ),
                                true,
                            ),
                            (false, None) => ("Not available in this installer.".to_owned(), false),
                        };
                        check(
                            ui,
                            Pos2::new(x, y),
                            width,
                            &mut self.starter,
                            "Download Starter Content: Characters",
                            &hint,
                            enabled,
                        );
                        let p = ui.painter();
                        p.text(
                            Pos2::new(x, body.bottom() - 40.0),
                            Align2::LEFT_CENTER,
                            "Renkatsu is source available under the PolyForm Shield License 1.0.0.",
                            font(11.5),
                            DIM,
                        );
                        let install = Rect::from_min_size(
                            Pos2::new(foot.right() - 24.0 - 120.0, foot.center().y - 16.0),
                            Vec2::new(120.0, 32.0),
                        );
                        let cancel = install.translate(Vec2::new(-128.0, 0.0));
                        if button(ui, cancel, "Cancel", false, true) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        let label = if self.layout.as_ref().is_some_and(Layout::installed) {
                            "Update"
                        } else {
                            "Install"
                        };
                        if button(ui, install, label, true, self.layout.is_some()) {
                            self.start(&ctx);
                        }
                    }
                    Page::Working => {
                        let p = ui.painter();
                        p.text(Pos2::new(x, top), Align2::LEFT_TOP, "Installing", bold(18.0), BRIGHT);
                        p.text(Pos2::new(x, top + 40.0), Align2::LEFT_TOP, &self.step, font(13.0), TEXT);
                        let (stage, done, total) = self.task.progress();
                        let bar = Rect::from_min_size(Pos2::new(x, top + 72.0), Vec2::new(width, 6.0));
                        p.rect_filled(bar, 0.0, INPUT);
                        let downloading = !matches!(stage, Stage::Waiting) && total > 0;
                        if downloading {
                            let f = (done as f32 / total as f32).clamp(0.0, 1.0);
                            p.rect_filled(
                                Rect::from_min_size(bar.min, Vec2::new(bar.width() * f, bar.height())),
                                0.0,
                                SELECT,
                            );
                            p.text(
                                Pos2::new(x, top + 96.0),
                                Align2::LEFT_TOP,
                                format!("{}, {} of {}", stage.label(), size_text(done), size_text(total)),
                                font(12.0),
                                DIM,
                            );
                        } else {
                            // Copying is quick; a moving band says work goes on.
                            let t = (ctx.input(|i| i.time) as f32 * 0.8).fract();
                            let w = bar.width() * 0.25;
                            let left = bar.left() + (bar.width() + w) * t - w;
                            let band = Rect::from_min_max(
                                Pos2::new(left.max(bar.left()), bar.top()),
                                Pos2::new((left + w).min(bar.right()), bar.bottom()),
                            );
                            p.rect_filled(band, 0.0, SELECT);
                        }
                        let stop = Rect::from_min_size(
                            Pos2::new(foot.right() - 24.0 - 120.0, foot.center().y - 16.0),
                            Vec2::new(120.0, 32.0),
                        );
                        if button(ui, stop, "Stop download", false, downloading) {
                            self.task.cancel();
                        }
                        ctx.request_repaint();
                    }
                    Page::Done => {
                        let ok = matches!(self.result, Some(Ok(())));
                        let p = ui.painter();
                        let (title, color) = if ok {
                            ("Renkatsu Studio is installed", BRIGHT)
                        } else {
                            ("The installation did not finish", STOP)
                        };
                        p.text(Pos2::new(x, top), Align2::LEFT_TOP, title, bold(18.0), color);
                        let text = match &self.result {
                            Some(Ok(())) => "Start it from the application menu, or here.".to_owned(),
                            Some(Err(e)) => e.clone(),
                            None => String::new(),
                        };
                        let galley = ui
                            .painter()
                            .layout(text, font(13.0), if ok { TEXT } else { DIM }, width);
                        ui.painter().galley(Pos2::new(x, top + 40.0), galley, TEXT);
                        let launch = Rect::from_min_size(
                            Pos2::new(foot.right() - 24.0 - 120.0, foot.center().y - 16.0),
                            Vec2::new(120.0, 32.0),
                        );
                        let close = launch.translate(Vec2::new(-128.0, 0.0));
                        if button(ui, close, "Close", !ok, true) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        let program = self.layout.as_ref().map(|l| l.app.join("renkatsu"));
                        if ok && button(ui, launch, "Launch", true, true) {
                            if let Some(program) = program {
                                let _ = std::process::Command::new(program).spawn();
                            }
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }
            });
    }
}

fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    let regular = include_bytes!("../../rk-studio/assets/fonts/OpenSans-Regular.ttf");
    let semibold = include_bytes!("../../rk-studio/assets/fonts/OpenSans-SemiBold.ttf");
    fonts
        .font_data
        .insert("interface".into(), Arc::new(egui::FontData::from_static(regular)));
    fonts.font_data.insert(
        "interface-semibold".into(),
        Arc::new(egui::FontData::from_static(semibold)),
    );
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "interface".into());
    let mut bold_list = fallback;
    bold_list.insert(0, "interface-semibold".into());
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), bold_list);
    fonts
}

fn main() -> eframe::Result {
    // `--attach PAYLOAD.zip OUT` makes the installer that is released: this
    // program with the zip of Renkatsu's files at its end.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--attach") {
        let (Some(zip), Some(out)) = (args.get(2), args.get(3)) else {
            eprintln!("usage: renkatsu-installer --attach PAYLOAD.zip OUT");
            std::process::exit(2);
        };
        let attach = || -> Result<()> {
            let program = std::fs::read(std::env::current_exe()?)?;
            let zip = std::fs::read(zip)?;
            payload::files(&zip)?;
            std::fs::write(out, payload::append(&program, &zip))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(out, std::fs::Permissions::from_mode(0o755))?;
            }
            Ok(())
        };
        if let Err(e) = attach() {
            eprintln!("could not make the installer: {e:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("Renkatsu Studio installer")
            .with_app_id("renkatsu-installer")
            .with_inner_size([760.0, 460.0])
            .with_resizable(false),
        ..Default::default()
    };
    eframe::run_native(
        "Renkatsu Studio installer",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_fonts(fonts());
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(Installer::new()))
        }),
    )
}
