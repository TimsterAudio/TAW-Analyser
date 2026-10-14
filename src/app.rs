//! The Analyser (SP-001): a stand-alone spectrum analyser. It listens to an
//! output (what's playing) or an input, and fills the window with the
//! spectrum as a curve, bars or LED columns.
//!
//! The panel is the TAW hi-fi look (TAW-Panel's `hifi`): the nameplate,
//! family switcher and window buttons along the top, the display window in
//! the middle, and the controls in a bar along the bottom. H hides the
//! nameplate and the bar for a clean display; the window can go full screen
//! or stay on top.

use crate::capture::{self, Capture, Source, DEFAULT_OUTPUT};
use crate::family::{self, App};
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{
    Align2, Color32, CornerRadius, Id, Key, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, Vec2,
    ViewportCommand, WindowLevel,
};
use serde::{Deserialize, Serialize};
use taw_panel::analyzer::{analyzer_channel, Analyzer, Ballistics};
use taw_panel::hifi::{self, Seg, Tokens};
use taw_panel::spectrum::{self, Look, Scheme};
use taw_panel::theme::PanelKind;
use taw_panel::widgets::Spec;

const STORAGE_KEY: &str = "taw-analyser";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The narrowest the bottom bar fits at full size; narrower windows scale.
const BAR_W: f32 = 1100.0;
pub const DEFAULT_SIZE: Vec2 = Vec2::new(1280.0, 720.0);
const F_MIN: f32 = 20.0;
const F_MAX: f32 = 20_000.0;
const PLATE_H: f32 = 96.0;
const BAR_DETAILED: f32 = 132.0;
const BAR_SIMPLE: f32 = 62.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum View {
    #[default]
    Curve,
    Bars,
    Leds,
}

impl View {
    const ALL: [View; 3] = [View::Curve, View::Bars, View::Leds];
}

/// Everything remembered between runs.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    source: Option<String>,
    view: View,
    /// Bars per octave for Bars and LEDs: 3, 6 or 12.
    per_octave: u32,
    /// The colour scheme, when not drawing in the display's own ink.
    scheme: Scheme,
    look: Look,
    rise_ms: f32,
    fall_ms: f32,
    hold_s: f32,
    /// How many dB the display spans, down from 0 dBFS.
    range_db: f32,
    /// Tilt in dB per octave around 1 kHz, so pink noise reads flat at 3.
    slope: f32,
    grid: bool,
    controls: bool,
    on_top: bool,
    /// The Jupiter panel before 0.2.0. Kept so older settings still load.
    #[allow(dead_code)]
    panel: PanelKind,
    /// Finish, light or dark, and accent.
    finish: hifi::Look,
    /// Draw in the display's own ink (white, or dark on Porcelain) rather
    /// than a colour scheme.
    ink: bool,
    /// Simple view: just the picture and a few controls.
    simple: bool,
    /// Use the system's title bar instead of the Analyser's own.
    system_title_bar: bool,
    /// Where the user pointed the Analyser to family apps it couldn't find.
    family: std::collections::HashMap<String, std::path::PathBuf>,
}

impl Default for Saved {
    fn default() -> Self {
        Saved {
            source: None,
            view: View::Curve,
            per_octave: 3,
            scheme: Scheme::Display,
            look: Look::FillAndLine,
            rise_ms: 0.0,
            fall_ms: 300.0,
            hold_s: 1.5,
            range_db: 90.0,
            slope: 3.0,
            grid: true,
            controls: true,
            on_top: false,
            panel: PanelKind::Black,
            finish: hifi::Look::default(),
            ink: true,
            simple: false,
            system_title_bar: false,
            family: Default::default(),
        }
    }
}

const RISE: Spec = Spec {
    min: 0.0,
    max: 500.0,
    default: 0.0,
    log: false,
    step: 5.0,
};
const FALL: Spec = Spec {
    min: 20.0,
    max: 3000.0,
    default: 300.0,
    log: true,
    step: 0.0,
};
const HOLD: Spec = Spec {
    min: 0.0,
    max: 10.0,
    default: 1.5,
    log: false,
    step: 0.1,
};
const RANGE: Spec = Spec {
    min: 40.0,
    max: 120.0,
    default: 90.0,
    log: false,
    step: 10.0,
};
const SLOPE: Spec = Spec {
    min: 0.0,
    max: 6.0,
    default: 3.0,
    log: false,
    step: 0.5,
};

pub struct AnalyserApp {
    s: Saved,
    analyzer: Analyzer,
    capture: Option<Capture>,
    error: Option<String>,
    sources: Vec<Source>,
    frozen: bool,
    fullscreen: bool,
    settings_open: bool,
    installed: [bool; 4],
    message: Option<(String, f64)>,
}

impl AnalyserApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        hifi::install_fonts(&cc.egui_ctx);
        let s: Saved = cc
            .storage
            .and_then(|st| eframe::get_value(st, STORAGE_KEY))
            .unwrap_or_default();
        let mut app = AnalyserApp {
            s,
            analyzer: Analyzer::default(),
            capture: None,
            error: None,
            sources: capture::sources(),
            frozen: false,
            fullscreen: false,
            settings_open: false,
            installed: [false, false, false, true],
            message: None,
        };
        app.analyzer.hop = 1024;
        if app.s.on_top {
            cc.egui_ctx
                .send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
        }
        if app.s.system_title_bar {
            cc.egui_ctx
                .send_viewport_cmd(ViewportCommand::Decorations(true));
        }
        app.refresh_family();
        app.start();
        app
    }

    fn refresh_family(&mut self) {
        for (i, a) in App::ALL.into_iter().enumerate() {
            if a == App::Analyser {
                continue;
            }
            self.installed[i] = family::find(a.program(), self.s.family.get(a.name())).is_some();
        }
    }

    fn start(&mut self) {
        self.capture = None;
        let (tap, rx) = analyzer_channel();
        self.analyzer.connect(rx);
        let id = self
            .s
            .source
            .clone()
            .unwrap_or_else(|| DEFAULT_OUTPUT.to_string());
        match capture::start(&id, tap) {
            Ok(c) => {
                self.capture = Some(c);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn ballistics(&self) -> Ballistics {
        Ballistics {
            rise_ms: self.s.rise_ms,
            fall_ms: self.s.fall_ms,
            hold_s: self.s.hold_s,
            peak_fall_db_s: 20.0,
        }
    }

    fn sample_rate(&self) -> f32 {
        match self
            .capture
            .as_ref()
            .map(|c| c.sample_rate.load(std::sync::atomic::Ordering::Relaxed))
        {
            Some(r) if r > 0 => r as f32,
            _ => 48_000.0,
        }
    }

    fn set_fullscreen(&mut self, ctx: &egui::Context, on: bool) {
        self.fullscreen = on;
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(on));
    }

    fn set_on_top(&mut self, ctx: &egui::Context, on: bool) {
        self.s.on_top = on;
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(if on {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        }));
    }

    /// With the controls showing, scale everything so the bar fits.
    fn fit_to_window(&self, ui: &Ui) {
        let ctx = ui.ctx();
        let zoom = ctx.zoom_factor();
        let screen = ctx.content_rect().size() * zoom;
        let want = if self.s.controls {
            (screen.x / BAR_W).clamp(0.5, 1.0)
        } else {
            1.0
        };
        if (want - zoom).abs() > 0.002 {
            ctx.set_zoom_factor(want);
            ctx.request_repaint();
        }
    }

    fn tell(&mut self, s: impl Into<String>, t: f64) {
        self.message = Some((s.into(), t));
    }

    fn source_label(&self) -> String {
        match (&self.capture, &self.error) {
            (Some(c), _) => {
                let rate = c.sample_rate.load(std::sync::atomic::Ordering::Relaxed);
                if rate > 0 && !self.s.simple {
                    format!("{} · {:.1} kHz", c.description, rate as f32 / 1000.0)
                } else {
                    c.description.clone()
                }
            }
            (None, Some(_)) => "Not listening".into(),
            (None, None) => "Stopped".into(),
        }
    }

    /// The colour of a bar or LED at horizontal position `x` and level `l`.
    fn colour(&self, t: &Tokens, x: f32, l: f32) -> Color32 {
        if self.s.ink {
            t.ink
        } else {
            self.s.scheme.colour(x, l)
        }
    }

    // ------------------------------------------------------------ display

    fn display(&self, ui: &Ui, rect: Rect, t: &Tokens) {
        hifi::display(ui.painter(), rect, t);
        let p = ui.painter().with_clip_rect(rect);
        let plot = if self.s.grid {
            Rect::from_min_max(
                rect.min + Vec2::new(48.0, 34.0),
                rect.max - Vec2::new(20.0, 30.0),
            )
        } else {
            rect.shrink2(Vec2::new(14.0, 30.0))
        };
        if plot.width() < 20.0 || plot.height() < 20.0 {
            return;
        }
        let range = self.s.range_db.clamp(RANGE.min, RANGE.max);
        let x_of = |f: f32| plot.left() + (f / F_MIN).ln() / (F_MAX / F_MIN).ln() * plot.width();
        let f_of = |x: f32| F_MIN * ((x - plot.left()) / plot.width() * (F_MAX / F_MIN).ln()).exp();
        // 0..1 from the bottom of the range to 0 dBFS.
        let lvl = |db: f32| ((db + range) / range).clamp(0.0, 1.0);
        let y_of = |l: f32| plot.bottom() - l * plot.height();
        let slope = |f: f32| self.s.slope * (f / 1000.0).log2();

        if self.s.grid {
            self.grid(&p, plot, range, &x_of, t);
        }

        let sr = self.sample_rate();
        let an = &self.analyzer;
        let holding = self.s.hold_s > 0.0;
        match self.s.view {
            View::Curve => {
                let mut pts = Vec::new();
                let mut levels = Vec::new();
                let mut peaks = Vec::new();
                let mut x = plot.left();
                while x <= plot.right() {
                    let (f0, f1) = (f_of(x - 1.0), f_of(x + 1.0));
                    let tilt = slope(f_of(x));
                    let l = lvl(an.level(f0, f1, sr) + tilt);
                    pts.push(Pos2::new(x, y_of(l)));
                    levels.push(l);
                    if holding {
                        peaks.push(Pos2::new(x, y_of(lvl(an.peak_level(f0, f1, sr) + tilt))));
                    }
                    x += 2.0;
                }
                if self.s.ink {
                    let (fill, line) = match self.s.look {
                        Look::FillAndLine => (0.22, 2.0),
                        Look::LineOnly => (0.0, 2.0),
                        Look::FillOnly => (0.45, 0.0),
                    };
                    hifi::ink_curve(&p, &pts, plot.bottom(), t.ink, fill, line);
                } else {
                    spectrum::draw(
                        &p,
                        &pts,
                        &levels,
                        plot.left(),
                        plot.right(),
                        plot.bottom(),
                        spectrum::Style {
                            scheme: self.s.scheme,
                            fill: self.s.look,
                            strength: 1.0,
                        },
                    );
                }
                if holding {
                    let c = self.colour(t, 0.5, 0.9);
                    p.add(Shape::line(peaks, Stroke::new(1.0, c.gamma_multiply(0.45))));
                }
            }
            View::Bars | View::Leds => {
                let n = self.s.per_octave.clamp(1, 24) as f32;
                // Bands centred on 1 kHz, n per octave, covering 20 Hz to 20 kHz.
                let k0 = (n * (F_MIN / 1000.0).log2()).ceil() as i32;
                let k1 = (n * (F_MAX / 1000.0).log2()).floor() as i32;
                let half = 2f32.powf(0.5 / n);
                for k in k0..=k1 {
                    let fc = 1000.0 * 2f32.powf(k as f32 / n);
                    let (lo, hi) = (fc / half, fc * half);
                    let (x0, x1) = (x_of(lo.max(F_MIN)), x_of(hi.min(F_MAX)));
                    let gap = ((x1 - x0) * 0.18).clamp(1.0, 4.0);
                    let r = Rect::from_min_max(
                        Pos2::new(x0 + gap / 2.0, plot.top()),
                        Pos2::new(x1 - gap / 2.0, plot.bottom()),
                    );
                    let tilt = slope(fc);
                    let l = lvl(an.level(lo, hi, sr) + tilt);
                    let peak = holding.then(|| lvl(an.peak_level(lo, hi, sr) + tilt));
                    let xt = (fc / F_MIN).ln() / (F_MAX / F_MIN).ln();
                    if self.s.view == View::Bars {
                        self.bar(&p, r, xt, l, peak, t);
                    } else {
                        self.led_column(&p, r, xt, l, peak, t);
                    }
                }
            }
        }

        // What it's listening to, top left; frozen, top right.
        hifi::ink_text(
            &p,
            Pos2::new(plot.left() + 4.0, rect.top() + 12.0),
            Align2::LEFT_TOP,
            &self.source_label().to_uppercase(),
            hifi::num(11.5),
            t,
        );
        if let Some(e) = &self.error {
            hifi::ink_text(
                &p,
                plot.center(),
                Align2::CENTER_CENTER,
                &format!("Not listening: {e}"),
                hifi::ui_font(15.0),
                t,
            );
        }
        let mut right = plot.right() - 4.0;
        if self.frozen && self.error.is_none() {
            let r = hifi::ink_text(
                &p,
                Pos2::new(right, rect.top() + 12.0),
                Align2::RIGHT_TOP,
                "FROZEN",
                hifi::num(11.5),
                t,
            );
            right = r.left() - 16.0;
        }
        // A passing message (family keys), on the same line, clear of the
        // spectrum and the frequency scale.
        let now = ui.input(|i| i.time);
        if let Some((m, at)) = &self.message {
            if now - at < 6.0 {
                hifi::ink_text(
                    &p,
                    Pos2::new(right, rect.top() + 11.0),
                    Align2::RIGHT_TOP,
                    m,
                    hifi::ui_font(12.5),
                    t,
                );
            }
        }
    }

    fn grid(
        &self,
        p: &egui::Painter,
        plot: Rect,
        range: f32,
        x_of: &dyn Fn(f32) -> f32,
        t: &Tokens,
    ) {
        let label_col = t.ink_faint(0.6);
        let small = hifi::num(10.5);
        for &f in &[
            20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 200.0, 300.0, 400.0, 500.0,
            600.0, 700.0, 800.0, 900.0, 1000.0, 2000.0, 3000.0, 4000.0, 5000.0, 6000.0, 7000.0,
            8000.0, 9000.0, 10_000.0, 20_000.0,
        ] {
            let x = x_of(f);
            let major = [100.0, 1000.0, 10_000.0].contains(&f);
            p.line_segment(
                [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
                Stroke::new(1.0, t.ink_faint(if major { 0.16 } else { 0.07 })),
            );
        }
        for &(f, l) in &[
            (20.0, "20"),
            (50.0, "50"),
            (100.0, "100"),
            (200.0, "200"),
            (500.0, "500"),
            (1000.0, "1k"),
            (2000.0, "2k"),
            (5000.0, "5k"),
            (10_000.0, "10k"),
            (20_000.0, "20k"),
        ] {
            hifi::label(
                p,
                Pos2::new(x_of(f), plot.bottom() + 7.0),
                Align2::CENTER_TOP,
                l,
                small.clone(),
                label_col,
            );
        }
        let step = if range > 80.0 { 20 } else { 10 };
        let mut db = 0;
        while db as f32 >= -range {
            let y = plot.bottom() - (db as f32 + range) / range * plot.height();
            p.line_segment(
                [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
                Stroke::new(1.0, t.ink_faint(0.10)),
            );
            hifi::label(
                p,
                Pos2::new(plot.left() - 8.0, y),
                Align2::RIGHT_CENTER,
                &db.to_string(),
                small.clone(),
                label_col,
            );
            db -= step;
        }
    }

    /// A solid bar, brighter at the top, with a peak cap.
    fn bar(&self, p: &egui::Painter, r: Rect, xt: f32, l: f32, peak: Option<f32>, t: &Tokens) {
        let top = r.bottom() - l * r.height();
        if l > 0.0 {
            let (c_top, c_bot) = (self.colour(t, xt, l), self.colour(t, xt, 0.0));
            let a = if self.s.look == Look::LineOnly {
                0.35
            } else {
                0.9
            };
            let mut mesh = Mesh::default();
            let v = |pos: Pos2, c: Color32| Vertex {
                pos,
                uv: WHITE_UV,
                color: c,
            };
            mesh.vertices.extend([
                v(Pos2::new(r.left(), top), c_top.gamma_multiply(a)),
                v(Pos2::new(r.right(), top), c_top.gamma_multiply(a)),
                v(
                    Pos2::new(r.left(), r.bottom()),
                    c_bot.gamma_multiply(a * 0.35),
                ),
                v(
                    Pos2::new(r.right(), r.bottom()),
                    c_bot.gamma_multiply(a * 0.35),
                ),
            ]);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(1, 3, 2);
            p.add(Shape::mesh(mesh));
            if self.s.look != Look::FillOnly {
                p.line_segment(
                    [Pos2::new(r.left(), top), Pos2::new(r.right(), top)],
                    Stroke::new(2.0, c_top),
                );
            }
        }
        if let Some(pk) = peak {
            let y = r.bottom() - pk * r.height();
            p.line_segment(
                [Pos2::new(r.left(), y), Pos2::new(r.right(), y)],
                Stroke::new(2.0, self.colour(t, xt, pk).gamma_multiply(0.85)),
            );
        }
    }

    /// A column of LED segments, lit up to the level.
    fn led_column(
        &self,
        p: &egui::Painter,
        r: Rect,
        xt: f32,
        l: f32,
        peak: Option<f32>,
        t: &Tokens,
    ) {
        let seg = (r.height() / 40.0).clamp(4.0, 12.0);
        let count = (r.height() / seg).floor().max(1.0) as usize;
        let peak_seg = peak.map(|pk| ((pk * count as f32).ceil() as usize).min(count));
        let lit = (l * count as f32).round() as usize;
        for i in 0..count {
            let y1 = r.bottom() - i as f32 * seg;
            let cell = Rect::from_min_max(
                Pos2::new(r.left(), y1 - seg + 1.5),
                Pos2::new(r.right(), y1),
            );
            let tt = (i as f32 + 0.5) / count as f32;
            let c = self.colour(t, xt, tt);
            let on = i < lit || peak_seg == Some(i + 1);
            p.rect_filled(
                cell,
                CornerRadius::same(1),
                if on { c } else { c.gamma_multiply(0.07) },
            );
        }
    }

    // ------------------------------------------------------------ top

    fn plate(&mut self, ui: &mut Ui, screen: Rect, t: &Tokens) {
        let now = ui.input(|i| i.time);
        let plate = Rect::from_min_size(screen.min, Vec2::new(screen.width(), PLATE_H));
        if !self.s.system_title_bar {
            hifi::drag_to_move(ui, plate);
        }
        hifi::nameplate(
            ui,
            screen.min + Vec2::new(24.0, 14.0),
            t,
            "Analyser",
            &format!("SP-001 · Spectrum analyser · V{VERSION}"),
        );
        let right = screen.right() - 24.0;
        if !self.s.system_title_bar {
            hifi::window_buttons(ui, Pos2::new(screen.right() - 10.0, screen.top() + 10.0), t);
        }
        let row = screen.top() + 66.0;
        let gear = Rect::from_min_size(Pos2::new(right - 30.0, row - 15.0), Vec2::splat(30.0));
        if hifi::gear_button(ui, gear, Id::new("settings"), t, self.settings_open).clicked() {
            self.settings_open = !self.settings_open;
        }
        let (seg, picked) = hifi::segmented(
            ui,
            Pos2::new(gear.left() - 12.0, row),
            true,
            Id::new("simple"),
            t,
            &["Simple", "Detailed"],
            if self.s.simple { 0 } else { 1 },
            Seg::Plain,
            &[],
            &[],
        );
        if let Some(i) = picked {
            self.s.simple = i == 0;
        }
        let (_, picked) =
            hifi::family_switch(ui, Pos2::new(seg.left() - 14.0, row), t, 3, self.installed);
        if let Some(i) = picked {
            let a = App::ALL[i];
            match family::find(a.program(), self.s.family.get(a.name())) {
                Some(exe) => match family::launch(&exe) {
                    Ok(()) => self.tell(format!("Starting the {}…", a.name()), now),
                    Err(e) => self.tell(format!("The {} wouldn't start: {e}", a.name()), now),
                },
                None => self.tell(
                    format!(
                        "The {} isn't installed here (looked next to the Analyser and on the PATH).",
                        a.name()
                    ),
                    now,
                ),
            }
            self.refresh_family();
        }
    }

    // ------------------------------------------------------------ bottom bar

    fn controls(&mut self, ui: &mut Ui, bar: Rect, t: &Tokens) {
        hifi::bar(ui.painter(), bar, t);
        let ctx = ui.ctx().clone();
        let x0 = bar.left() + 24.0;
        let right = bar.right() - 24.0;
        let row1 = bar.top() + 31.0;
        let lab = |ui: &Ui, x: f32, y: f32, s: &str| {
            hifi::label(
                ui.painter(),
                Pos2::new(x, y),
                Align2::LEFT_CENTER,
                s,
                hifi::ui_font(12.0),
                t.dim,
            )
        };

        // Source
        let mut x = x0;
        if !self.s.simple {
            x = lab(ui, x, row1, "Source").right() + 10.0;
        }
        let mut items: Vec<String> = self.sources.iter().map(|s| s.label.clone()).collect();
        let current = self
            .s
            .source
            .clone()
            .unwrap_or_else(|| DEFAULT_OUTPUT.into());
        let mut cur = self.sources.iter().position(|s| s.id == current);
        if cur.is_none() {
            items.push(format!("{current} (not found)"));
            cur = Some(items.len() - 1);
        }
        let combo = Rect::from_min_size(Pos2::new(x, row1 - 13.0), Vec2::new(230.0, 26.0));
        if let Some(i) = hifi::select(ui, combo, "source", t, &items, cur.unwrap_or(0)) {
            if let Some(src) = self.sources.get(i) {
                self.s.source = Some(src.id.clone());
                self.start();
            }
        }
        x = combo.right() + 6.0;
        if !self.s.simple {
            let w = hifi::button_width(ui, "Rescan");
            let r = Rect::from_min_size(Pos2::new(x, row1 - 13.0), Vec2::new(w, 26.0));
            if hifi::button(ui, r, Id::new("rescan"), t, "Rescan", false).clicked() {
                self.sources = capture::sources();
            }
            x = r.right();
        }
        x += 22.0;

        // View
        if !self.s.simple {
            x = lab(ui, x, row1, "View").right() + 10.0;
        }
        let names: [&str; 3] = if self.s.simple {
            ["Smooth", "Bars", "LEDs"]
        } else {
            ["Curve", "Bars", "LEDs"]
        };
        let sel = View::ALL
            .iter()
            .position(|v| *v == self.s.view)
            .unwrap_or(0);
        let (r, picked) = hifi::segmented(
            ui,
            Pos2::new(x, row1),
            false,
            Id::new("view"),
            t,
            &names,
            sel,
            Seg::Accent,
            &[],
            &[],
        );
        if let Some(i) = picked {
            self.s.view = View::ALL[i];
        }
        x = r.right() + 22.0;

        // Bands per octave, for Bars and LEDs.
        if !self.s.simple {
            x = lab(ui, x, row1, "Bands").right() + 10.0;
            let opts = [3u32, 6, 12];
            let sel = opts
                .iter()
                .position(|n| *n == self.s.per_octave)
                .unwrap_or(0);
            let (_, picked) = hifi::segmented(
                ui,
                Pos2::new(x, row1),
                false,
                Id::new("per-octave"),
                t,
                &["1/3", "1/6", "1/12"],
                sel,
                Seg::Accent,
                &[],
                &[],
            );
            if let Some(i) = picked {
                self.s.per_octave = opts[i];
            }
        }

        // Freeze, on top, full screen (right).
        let labels: [&str; 3] = [
            "Freeze",
            if self.s.simple {
                "Stay on top"
            } else {
                "On top"
            },
            if self.fullscreen {
                "Leave full screen"
            } else {
                "Full screen"
            },
        ];
        let states = [self.frozen, self.s.on_top, self.fullscreen];
        let mut bx = right;
        for i in (0..3).rev() {
            let w = hifi::button_width(ui, labels[i]);
            let r = Rect::from_min_size(Pos2::new(bx - w, row1 - 13.0), Vec2::new(w, 26.0));
            if hifi::button(ui, r, Id::new(("win", i)), t, labels[i], states[i]).clicked() {
                match i {
                    0 => self.frozen = !self.frozen,
                    1 => self.set_on_top(&ctx, !self.s.on_top),
                    _ => self.set_fullscreen(&ctx, !self.fullscreen),
                }
            }
            bx = r.left() - 6.0;
        }

        if self.s.simple {
            return;
        }

        // Row two: motion and scale knobs, grid, colour and style.
        let row2 = bar.top() + 86.0;
        let mut x = x0;
        lab(ui, x, row2, "Motion");
        x += 72.0;
        for (name, spec, v) in [
            ("Rise", &RISE, &mut self.s.rise_ms),
            ("Fall", &FALL, &mut self.s.fall_ms),
            ("Hold", &HOLD, &mut self.s.hold_s),
        ] {
            let shown = match name {
                "Hold" if *v <= 0.0 => "off".to_string(),
                "Hold" => format!("{v:.1} s"),
                _ => format!("{v:.0} ms"),
            };
            hifi::knob(
                ui,
                Pos2::new(x, row2 - 10.0),
                Id::new(name),
                t,
                v,
                spec,
                &shown,
                name,
            );
            x += 64.0;
        }
        let sep = |ui: &Ui, x: f32| {
            ui.painter().line_segment(
                [Pos2::new(x, row2 - 30.0), Pos2::new(x, row2 + 28.0)],
                Stroke::new(1.0, t.line),
            );
        };
        sep(ui, x - 10.0);
        x += 18.0;
        lab(ui, x, row2, "Scale");
        x += 62.0;
        let range = format!("{:.0} dB", self.s.range_db);
        hifi::knob(
            ui,
            Pos2::new(x, row2 - 10.0),
            Id::new("range"),
            t,
            &mut self.s.range_db,
            &RANGE,
            &range,
            "Range",
        );
        x += 70.0;
        let slope = format!("{:.1} dB/oct", self.s.slope);
        hifi::knob(
            ui,
            Pos2::new(x, row2 - 10.0),
            Id::new("slope"),
            t,
            &mut self.s.slope,
            &SLOPE,
            &slope,
            "Slope",
        );
        x += 56.0;
        sep(ui, x);
        x += 26.0;
        hifi::labelled_switch(
            ui,
            Pos2::new(x, row2 - 9.0),
            Id::new("grid"),
            t,
            &mut self.s.grid,
            "Grid",
        );

        // Colour and style (right).
        let mut colours = vec!["Display".to_string()];
        colours.extend(Scheme::ALL.iter().map(|s| title_case(s.name())));
        let sel = if self.s.ink {
            0
        } else {
            1 + Scheme::ALL
                .iter()
                .position(|s| *s == self.s.scheme)
                .unwrap_or(0)
        };
        let styles: Vec<String> = Look::ALL
            .iter()
            .map(|l| {
                match l {
                    Look::FillAndLine => "Fill and line",
                    Look::LineOnly => "Line",
                    Look::FillOnly => "Fill",
                }
                .to_string()
            })
            .collect();
        let style_r = Rect::from_min_size(
            Pos2::new(right - 130.0, row2 - 13.0),
            Vec2::new(130.0, 26.0),
        );
        let ssel = Look::ALL
            .iter()
            .position(|l| *l == self.s.look)
            .unwrap_or(0);
        if let Some(i) = hifi::select(ui, style_r, "style", t, &styles, ssel) {
            self.s.look = Look::ALL[i];
        }
        let sl = hifi::label(
            ui.painter(),
            Pos2::new(style_r.left() - 10.0, row2),
            Align2::RIGHT_CENTER,
            "Style",
            hifi::ui_font(12.0),
            t.dim,
        );
        let col_r = Rect::from_min_size(
            Pos2::new(sl.left() - 20.0 - 120.0, row2 - 13.0),
            Vec2::new(120.0, 26.0),
        );
        if let Some(i) = hifi::select(ui, col_r, "colour", t, &colours, sel) {
            if i == 0 {
                self.s.ink = true;
            } else {
                self.s.ink = false;
                self.s.scheme = Scheme::ALL[i - 1];
            }
        }
        hifi::label(
            ui.painter(),
            Pos2::new(col_r.left() - 10.0, row2),
            Align2::RIGHT_CENTER,
            "Colour",
            hifi::ui_font(12.0),
            t.dim,
        );
    }

    fn settings(&mut self, ctx: &egui::Context, t: &Tokens) {
        let mut open = self.settings_open;
        let mut look = self.s.finish;
        let mut system_bar = self.s.system_title_bar;
        hifi::drawer(ctx, t, "Settings", &mut open, |ui| {
            hifi::look_settings(ui, t, &mut look);
            hifi::drawer_heading(ui, t, "Window");
            let r = hifi::drawer_row(ui, t, "Use the system title bar", 32.0);
            let r = Rect::from_min_size(r.min + Vec2::new(0.0, 4.0), Vec2::new(32.0, 18.0));
            if hifi::switch(ui, r, Id::new("system-bar"), t, system_bar).clicked() {
                system_bar = !system_bar;
            }
            hifi::drawer_heading(ui, t, "Keys");
            for (k, what) in [
                ("H", "Hide or show the controls"),
                ("Space", "Freeze the display"),
                ("F11", "Full screen"),
                ("Esc", "Leave full screen"),
            ] {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [64.0, 18.0],
                        egui::Label::new(
                            egui::RichText::new(k).font(hifi::num(12.0)).color(t.text),
                        ),
                    );
                    ui.label(
                        egui::RichText::new(what)
                            .font(hifi::ui_font(12.5))
                            .color(t.dim),
                    );
                });
            }
        });
        self.settings_open = open;
        self.s.finish = look;
        if system_bar != self.s.system_title_bar {
            self.s.system_title_bar = system_bar;
            ctx.send_viewport_cmd(ViewportCommand::Decorations(system_bar));
        }
    }
}

fn title_case(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut c = lower.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

impl eframe::App for AnalyserApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.fit_to_window(ui);
        let ctx = ui.ctx().clone();
        let t = Tokens::from_ctx(&ctx, &self.s.finish);
        hifi::apply_visuals(&ctx, &t);

        // Keys: H shows / hides the controls, F11 full screen, Esc leaves
        // full screen, Space freezes the display.
        let free = ui.memory(|m| m.focused().is_none()) && !self.settings_open;
        let (h, f11, esc, space) = ui.input(|i| {
            (
                i.key_pressed(Key::H),
                i.key_pressed(Key::F11),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::Space),
            )
        });
        if free && h {
            self.s.controls = !self.s.controls;
        }
        if f11 {
            self.set_fullscreen(&ctx, !self.fullscreen);
        }
        if esc && self.fullscreen && !self.settings_open {
            self.set_fullscreen(&ctx, false);
        }
        if free && space {
            self.frozen = !self.frozen;
        }

        self.analyzer.ballistics = self.ballistics();
        self.analyzer.sample_rate = self.sample_rate();
        if !self.frozen {
            self.analyzer.update();
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        let screen = ui.max_rect();
        hifi::faceplate(ui.painter(), screen, &t);
        let display = if self.s.controls {
            let bar_h = if self.s.simple {
                BAR_SIMPLE
            } else {
                BAR_DETAILED
            };
            self.plate(ui, screen, &t);
            let bar = Rect::from_min_max(
                Pos2::new(screen.left(), screen.bottom() - bar_h),
                screen.max,
            );
            self.controls(ui, bar, &t);
            // The bar covers the faceplate's edge; draw the window's outline
            // over both.
            ui.painter()
                .rect_stroke(screen, 0, Stroke::new(1.0, t.edge), StrokeKind::Inside);
            Rect::from_min_max(
                Pos2::new(screen.left() + 24.0, screen.top() + PLATE_H + 4.0),
                Pos2::new(screen.right() - 24.0, bar.top() - 16.0),
            )
        } else {
            // A clean display: drag it to move the window, double-click it
            // to bring the controls back.
            let resp = ui.interact(screen, Id::new("display"), egui::Sense::click_and_drag());
            if resp.double_clicked() {
                self.s.controls = true;
            } else if resp.drag_started_by(egui::PointerButton::Primary) && !self.s.system_title_bar
            {
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }
            screen.shrink(12.0)
        };
        self.display(ui, display, &t);

        self.settings(&ctx, &t);
        if !self.s.system_title_bar && !self.fullscreen {
            hifi::resize_edges(ui, screen);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.s);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Tokens::new(&self.s.finish, false)
            .face_bottom
            .to_normalized_gamma_f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_settings_still_load() {
        // Settings saved by 0.1.0 have no look, ink or simple fields.
        let s: Saved = serde_json::from_str(
            r#"{"view":"Bars","per_octave":6,"scheme":"Amber","panel":"Silver"}"#,
        )
        .unwrap();
        assert_eq!(s.view, View::Bars);
        assert_eq!(s.per_octave, 6);
        assert!(
            s.ink,
            "new installs and upgrades start with the display ink"
        );
        assert_eq!(s.finish, hifi::Look::default());
        assert!(!s.simple);
    }

    #[test]
    fn colour_names() {
        assert_eq!(title_case("RAINBOW"), "Rainbow");
    }
}
