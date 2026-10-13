//! The Analyser (SP-001): a stand-alone spectrum analyser. It listens to an
//! output (what's playing) or an input, and fills the window with the
//! spectrum as a curve, bars or LED columns, in the same colour schemes as
//! the Equaliser. The control strip along the bottom can be hidden for a clean
//! display on any screen, and the window can go full screen or stay on top.

use crate::capture::{self, Capture, Source, DEFAULT_OUTPUT};
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align2, Color32, CornerRadius, Id, Key, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, Vec2,
    ViewportCommand, WindowLevel,
};
use serde::{Deserialize, Serialize};
use taw_panel::analyzer::{analyzer_channel, Analyzer, Ballistics};
use taw_panel::frame::draw_panel_bg;
use taw_panel::spectrum::{self, Look, Scheme};
use taw_panel::theme::*;
use taw_panel::widgets::*;

const STORAGE_KEY: &str = "taw-analyser";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The control strip's design size; it's scaled down to fit narrow windows.
const STRIP: Vec2 = Vec2::new(1490.0, 132.0);
pub const DEFAULT_SIZE: Vec2 = Vec2::new(1280.0, 640.0);
const F_MIN: f32 = 20.0;
const F_MAX: f32 = 20_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum View {
    #[default]
    Curve,
    Bars,
    Leds,
}

impl View {
    const ALL: [View; 3] = [View::Curve, View::Bars, View::Leds];
    fn short(self) -> &'static str {
        match self {
            View::Curve => "CURVE",
            View::Bars => "BARS",
            View::Leds => "LEDS",
        }
    }
}

/// Everything remembered between runs.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    source: Option<String>,
    view: View,
    /// Bars per octave for Bars and LEDs: 3, 6 or 12.
    per_octave: u32,
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
    panel: PanelKind,
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
}

impl AnalyserApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
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
        };
        app.analyzer.hop = 1024;
        if app.s.on_top {
            cc.egui_ctx
                .send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
        }
        app.start();
        app
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

    fn style(&self) -> spectrum::Style {
        spectrum::Style {
            scheme: self.s.scheme,
            fill: self.s.look,
            strength: 1.0,
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

    /// With the controls showing, scale everything so the strip fits.
    fn fit_to_window(&self, ui: &Ui) {
        let ctx = ui.ctx();
        let zoom = ctx.zoom_factor();
        let screen = ctx.content_rect().size() * zoom;
        let want = if self.s.controls {
            (screen.x / STRIP.x).clamp(0.4, 1.0)
        } else {
            1.0
        };
        if (want - zoom).abs() > 0.002 {
            ctx.set_zoom_factor(want);
            ctx.request_repaint();
        }
    }

    // ------------------------------------------------------------ display

    fn display(&self, ui: &Ui, rect: Rect) {
        let p = ui.painter().with_clip_rect(rect.expand(2.0));
        p.rect_filled(rect.expand(2.0), CornerRadius::same(4), Color32::BLACK);
        p.rect_filled(rect, CornerRadius::same(3), LCD_BG);
        let plot = if self.s.grid {
            Rect::from_min_max(
                rect.min + Vec2::new(44.0, 24.0),
                rect.max - Vec2::new(22.0, 26.0),
            )
        } else {
            rect.shrink(8.0)
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
            self.grid(&p, plot, range, &x_of);
        }

        let sr = self.sample_rate();
        let an = &self.analyzer;
        let style = self.style();
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
                spectrum::draw(
                    &p,
                    &pts,
                    &levels,
                    plot.left(),
                    plot.right(),
                    plot.bottom(),
                    style,
                );
                if holding {
                    let c = self.s.scheme.colour(0.5, 0.9);
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
                        self.bar(&p, r, xt, l, peak);
                    } else {
                        self.led_column(&p, r, xt, l, peak);
                    }
                }
            }
        }

        if let Some(e) = &self.error {
            glow_text(
                &p,
                plot.center(),
                Align2::CENTER_CENTER,
                &format!("NO SIGNAL: {}", e.to_uppercase()),
                font(F_MONO, 15.0),
                LCD_INK,
                0.5,
            );
        } else if self.frozen {
            glow_text(
                &p,
                Pos2::new(plot.right() - 8.0, plot.top() + 8.0),
                Align2::RIGHT_TOP,
                "FROZEN",
                font(F_MONO, 14.0),
                LCD_INK,
                0.5,
            );
        }
    }

    fn grid(&self, p: &egui::Painter, plot: Rect, range: f32, x_of: &dyn Fn(f32) -> f32) {
        let label_col = Color32::from_rgb(0xa8, 0x44, 0x2c);
        let small = font(F_MONO, 11.0);
        for &f in &[
            20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 200.0, 300.0, 400.0, 500.0,
            600.0, 700.0, 800.0, 900.0, 1000.0, 2000.0, 3000.0, 4000.0, 5000.0, 6000.0, 7000.0,
            8000.0, 9000.0, 10_000.0, 20_000.0,
        ] {
            let x = x_of(f);
            let major = [100.0, 1000.0, 10_000.0].contains(&f);
            p.line_segment(
                [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
                Stroke::new(1.0, LCD_DIM.gamma_multiply(if major { 1.6 } else { 0.7 })),
            );
        }
        for &(f, l) in &[
            (20.0, "20"),
            (50.0, "50"),
            (100.0, "100"),
            (200.0, "200"),
            (500.0, "500"),
            (1000.0, "1K"),
            (2000.0, "2K"),
            (5000.0, "5K"),
            (10_000.0, "10K"),
            (20_000.0, "20K"),
        ] {
            text(
                p,
                Pos2::new(x_of(f), plot.bottom() + 6.0),
                Align2::CENTER_TOP,
                l,
                small.clone(),
                label_col,
                0.5,
            );
        }
        let step = if range > 80.0 { 20 } else { 10 };
        let mut db = 0;
        while db as f32 >= -range {
            let y = plot.bottom() - (db as f32 + range) / range * plot.height();
            p.line_segment(
                [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
                Stroke::new(1.0, LCD_DIM),
            );
            text(
                p,
                Pos2::new(plot.left() - 7.0, y),
                Align2::RIGHT_CENTER,
                &db.to_string(),
                small.clone(),
                label_col,
                0.3,
            );
            db -= step;
        }
        text(
            p,
            Pos2::new(plot.left() - 7.0, plot.top() - 9.0),
            Align2::RIGHT_BOTTOM,
            "DB",
            small,
            label_col,
            0.3,
        );
    }

    /// A solid bar, brighter at the top, with a peak cap.
    fn bar(&self, p: &egui::Painter, r: Rect, xt: f32, l: f32, peak: Option<f32>) {
        let scheme = self.s.scheme;
        let top = r.bottom() - l * r.height();
        if l > 0.0 {
            let (c_top, c_bot) = (scheme.colour(xt, l), scheme.colour(xt, 0.0));
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
                Stroke::new(2.0, scheme.colour(xt, pk).gamma_multiply(0.85)),
            );
        }
    }

    /// A column of LED segments, lit up to the level.
    fn led_column(&self, p: &egui::Painter, r: Rect, xt: f32, l: f32, peak: Option<f32>) {
        let scheme = self.s.scheme;
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
            let t = (i as f32 + 0.5) / count as f32;
            let c = scheme.colour(xt, t);
            let on = i < lit || peak_seg == Some(i + 1);
            p.rect_filled(
                cell,
                CornerRadius::same(1),
                if on { c } else { c.gamma_multiply(0.07) },
            );
        }
    }

    // ------------------------------------------------------------ controls

    fn controls(&mut self, ui: &mut Ui, o: Vec2, th: &Theme) {
        let at = |x: f32, y: f32| Pos2::new(x, y) + o;

        // Wordmark, logo and model line
        {
            let p = ui.painter();
            let mut job = LayoutJob::default();
            job.append(
                "Timster's Audio Works - ",
                0.0,
                TextFormat {
                    font_id: font(F_MAKER, 12.0),
                    color: th.legend_dim,
                    ..Default::default()
                },
            );
            job.append(
                "Analyser",
                0.0,
                TextFormat {
                    font_id: font(F_MAKER_BOLD, 12.0),
                    color: th.legend,
                    ..Default::default()
                },
            );
            let g = p.layout_job(job);
            let r = Rect::from_min_size(at(18.0, 14.0), g.size() + Vec2::new(18.0, 8.0));
            p.rect_stroke(
                r,
                CornerRadius::same(3),
                Stroke::new(1.0, th.frame),
                StrokeKind::Inside,
            );
            p.galley(r.min + Vec2::new(9.0, 4.0), g, th.legend);
        }
        slanted_text(
            ui,
            at(20.0, 46.0),
            "ANALYSER",
            font(F_LOGO, 24.0),
            th.legend,
            2.5,
            12.0,
        );
        text(
            ui.painter(),
            at(18.0, 86.0),
            Align2::LEFT_TOP,
            &format!("SP-001 · SPECTRUM ANALYSER · V{VERSION}"),
            font(F_LEGEND, 9.5),
            th.legend_dim,
            2.0,
        );
        text(
            ui.painter(),
            at(18.0, 104.0),
            Align2::LEFT_TOP,
            "H CONTROLS · F11 FULL SCREEN · SPACE FREEZE",
            font(F_LEGEND, 9.0),
            th.legend_dim,
            0.8,
        );

        // Source
        let r = Rect::from_min_size(at(256.0, 16.0), Vec2::new(300.0, 104.0));
        section(ui.painter(), r, "Source", th, false);
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
        let combo = Rect::from_min_size(at(270.0, 34.0), Vec2::new(272.0, 24.0));
        if let Some(i) = lcd_combo(ui, combo, "source", &items, cur.unwrap_or(0)) {
            if let Some(src) = self.sources.get(i) {
                self.s.source = Some(src.id.clone());
                self.start();
            }
        }
        let status = match (&self.capture, &self.error) {
            (Some(c), _) => {
                let rate = c.sample_rate.load(std::sync::atomic::Ordering::Relaxed);
                if rate > 0 {
                    format!("{}  {:.1} KHZ", c.description, rate as f32 / 1000.0)
                } else {
                    c.description.clone()
                }
            }
            (None, Some(_)) => "NOT LISTENING".into(),
            (None, None) => "STOPPED".into(),
        };
        let lr = Rect::from_min_size(at(270.0, 66.0), Vec2::new(204.0, 22.0));
        ui.painter().rect_filled(lr, CornerRadius::same(2), LCD_BG);
        glow_text(
            &ui.painter().with_clip_rect(lr.shrink(2.0)),
            lr.left_center() + Vec2::new(8.0, 0.0),
            Align2::LEFT_CENTER,
            &status,
            font(F_MONO, 12.0),
            LCD_INK,
            0.3,
        );
        let kr = Rect::from_min_size(at(488.0, 68.0), Vec2::new(52.0, 18.0));
        if key(ui, kr, Id::new("rescan"), K_GREY, None, "Rescan", th).clicked() {
            self.sources = capture::sources();
        }

        // View
        let r = Rect::from_min_size(at(572.0, 16.0), Vec2::new(176.0, 104.0));
        section(ui.painter(), r, "View", th, false);
        for (i, v) in View::ALL.iter().enumerate() {
            let kr = Rect::from_min_size(at(586.0 + i as f32 * 52.0, 42.0), Vec2::new(44.0, 14.0));
            if key(
                ui,
                kr,
                Id::new(("view", i)),
                K_ORANGE,
                Some(self.s.view == *v),
                v.short(),
                th,
            )
            .clicked()
            {
                self.s.view = *v;
            }
        }
        for (i, n) in [3u32, 6, 12].iter().enumerate() {
            let kr = Rect::from_min_size(at(586.0 + i as f32 * 52.0, 86.0), Vec2::new(44.0, 14.0));
            let label = format!("1/{n} OCT");
            if key(
                ui,
                kr,
                Id::new(("per-oct", i)),
                K_GREY,
                Some(self.s.per_octave == *n),
                &label,
                th,
            )
            .clicked()
            {
                self.s.per_octave = *n;
            }
        }

        // Colour
        let r = Rect::from_min_size(at(764.0, 16.0), Vec2::new(156.0, 104.0));
        section(ui.painter(), r, "Colour", th, false);
        let kr = Rect::from_min_size(at(780.0, 42.0), Vec2::new(56.0, 18.0));
        if key(ui, kr, Id::new("scheme"), K_CREAM, None, "Colour", th).clicked() {
            self.s.scheme = self.s.scheme.next();
        }
        readout(ui.painter(), at(808.0, 80.0), self.s.scheme.name(), 64.0);
        let kr = Rect::from_min_size(at(848.0, 42.0), Vec2::new(56.0, 18.0));
        if key(ui, kr, Id::new("look"), K_CREAM, None, "Style", th).clicked() {
            self.s.look = self.s.look.next();
        }
        readout(ui.painter(), at(876.0, 80.0), self.s.look.name(), 64.0);

        // Motion: rise, fall, hold
        let r = Rect::from_min_size(at(936.0, 16.0), Vec2::new(236.0, 104.0));
        section(ui.painter(), r, "Motion", th, false);
        let knobs: [(&str, &Spec, f32); 3] = [
            ("Rise", &RISE, 976.0),
            ("Fall", &FALL, 1054.0),
            ("Hold", &HOLD, 1132.0),
        ];
        for (name, spec, x) in knobs {
            let v = match name {
                "Rise" => &mut self.s.rise_ms,
                "Fall" => &mut self.s.fall_ms,
                _ => &mut self.s.hold_s,
            };
            knob(
                ui,
                at(x, 54.0),
                16.0,
                Id::new(name),
                v,
                spec,
                Some(FAM_MOD),
                th,
            );
            legend(ui.painter(), at(x, 76.0), Align2::CENTER_TOP, name, th);
            let shown = match name {
                "Hold" if *v <= 0.0 => "OFF".to_string(),
                "Hold" => format!("{v:.1} S"),
                _ => format!("{v:.0} MS"),
            };
            readout(ui.painter(), at(x, 92.0), &shown, 60.0);
        }

        // Scale: range and slope
        let r = Rect::from_min_size(at(1188.0, 16.0), Vec2::new(162.0, 104.0));
        section(ui.painter(), r, "Scale", th, false);
        knob(
            ui,
            at(1228.0, 54.0),
            16.0,
            Id::new("range"),
            &mut self.s.range_db,
            &RANGE,
            None,
            th,
        );
        legend(
            ui.painter(),
            at(1228.0, 76.0),
            Align2::CENTER_TOP,
            "Range",
            th,
        );
        readout(
            ui.painter(),
            at(1228.0, 92.0),
            &format!("{:.0} DB", self.s.range_db),
            60.0,
        );
        knob(
            ui,
            at(1308.0, 54.0),
            16.0,
            Id::new("slope"),
            &mut self.s.slope,
            &SLOPE,
            Some(FAM_TONE),
            th,
        );
        legend(
            ui.painter(),
            at(1308.0, 76.0),
            Align2::CENTER_TOP,
            "Slope",
            th,
        );
        readout(
            ui.painter(),
            at(1308.0, 92.0),
            &format!("{:.1} DB/OCT", self.s.slope),
            70.0,
        );

        // Window keys
        let ctx = ui.ctx().clone();
        let keys: [(&str, bool); 4] = [
            ("Full", self.fullscreen),
            ("On top", self.s.on_top),
            ("Grid", self.s.grid),
            ("Hide", false),
        ];
        for (i, (label, on)) in keys.iter().enumerate() {
            let kr = Rect::from_min_size(
                at(1368.0 + (i % 2) as f32 * 60.0, 34.0 + (i / 2) as f32 * 46.0),
                Vec2::new(50.0, 16.0),
            );
            let led_on = (i < 3).then_some(*on);
            if key(ui, kr, Id::new(("win", i)), K_GREY, led_on, label, th).clicked() {
                match i {
                    0 => self.set_fullscreen(&ctx, !self.fullscreen),
                    1 => self.set_on_top(&ctx, !self.s.on_top),
                    2 => self.s.grid = !self.s.grid,
                    _ => self.s.controls = false,
                }
            }
        }
    }
}

impl eframe::App for AnalyserApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.fit_to_window(ui);
        let th = Theme::new(self.s.panel, KnobStyle::Colour);
        set_visuals(ui.ctx(), &th);
        let ctx = ui.ctx().clone();

        // Keys: H shows / hides the controls, F11 full screen, Esc leaves
        // full screen, Space freezes the display.
        let free = ui.memory(|m| m.focused().is_none());
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
        if esc && self.fullscreen {
            self.set_fullscreen(&ctx, false);
        }
        if free && space {
            self.frozen = !self.frozen;
        }
        // A double-click on the display brings the controls back.
        let screen = ui.max_rect();
        if !self.s.controls {
            let resp = ui.interact(screen, Id::new("display"), egui::Sense::click());
            if resp.double_clicked() {
                self.s.controls = true;
            }
        }

        self.analyzer.ballistics = self.ballistics();
        self.analyzer.sample_rate = self.sample_rate();
        if !self.frozen {
            self.analyzer.update();
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        ui.painter().rect_filled(screen, 0, th.page);
        draw_panel_bg(ui.painter(), screen, &th);
        let strip_h = if self.s.controls { STRIP.y } else { 0.0 };
        let display = Rect::from_min_max(
            screen.min + Vec2::splat(12.0),
            Pos2::new(screen.right() - 12.0, screen.bottom() - 12.0 - strip_h),
        );
        self.display(ui, display);
        if self.s.controls {
            let left = (screen.center().x - STRIP.x / 2.0).max(screen.left());
            let o = Vec2::new(left, screen.bottom() - STRIP.y);
            self.controls(ui, o, &th);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.s);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Theme::new(self.s.panel, KnobStyle::Colour)
            .page
            .to_normalized_gamma_f32()
    }
}

fn set_visuals(ctx: &egui::Context, th: &Theme) {
    let mut v = egui::Visuals::dark();
    v.window_fill = Color32::from_rgb(0x1f, 0x1f, 0x1f);
    v.panel_fill = th.panel;
    v.extreme_bg_color = LCD_BG;
    v.selection.bg_fill = LCD_DIM;
    v.selection.stroke = Stroke::new(1.0, LCD_INK);
    v.override_text_color = Some(Color32::from_rgb(0xec, 0xe8, 0xdd));
    v.window_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xec, 0xe8, 0xdd, 60));
    v.widgets.hovered.weak_bg_fill = LCD_DIM;
    if ctx.global_style().visuals != v {
        ctx.set_visuals(v);
    }
}
