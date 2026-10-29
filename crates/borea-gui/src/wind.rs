//! Quiet airflow behind the hero: fine streamlines with a slow gust travelling along
//! each one. Decorative only. It never carries meaning, and it holds still when motion
//! is reduced or the window is in the background.
//!
//! The wind picks up: a gust when the page changes, then calms again; and a steady
//! rise while an installation runs, fastest at 100%. Gentle by design: speed
//! changes ease in and out (never a jump), are capped at a few times the calm
//! speed, and never change brightness, so nothing flashes. "Pause wind" stills it
//! all.

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};
use std::sync::OnceLock;

/// One seed per launch. No per-frame randomness: nearby frames stay smooth.
fn launch_seed() -> u64 {
    static SEED: OnceLock<u64> = OnceLock::new();
    *SEED.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64)
    })
}

fn noise(seed: u64, lane: usize) -> f64 {
    let mut x = seed.wrapping_add((lane as u64).wrapping_mul(0x9e3779b97f4a7c15));
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((x ^ (x >> 31)) >> 11) as f64 / ((1_u64 << 53) as f64)
}

fn drift(time: f64, seed: u64, lane: usize) -> (f32, f32) {
    let phase = noise(seed, lane * 3) * std::f64::consts::TAU;
    let speed = 0.035 + noise(seed, lane * 3 + 1) * 0.045;
    (
        ((time * speed + phase).sin() * 0.025) as f32,
        (((time * speed * 0.73 + phase).sin() + (time * speed * 1.37 + phase * 2.0).sin()) * 0.035)
            as f32,
    )
}

/// A cubic Bézier segment in the 1000 × 300 design space of the HTML concept.
type Cubic = [(f32, f32); 4];

/// The concept's four streamlines, with SVG `S` segments expanded to full cubics.
const STREAMS: [&[Cubic]; 4] = [
    &[
        [(-100., 180.), (80., 180.), (40., 20.), (220., 20.)],
        [(220., 20.), (400., 20.), (260., 70.), (350., 40.)],
        [(350., 40.), (440., 10.), (400., -30.), (470., -10.)],
    ],
    &[
        [(-100., 240.), (100., 270.), (0., 80.), (180., 80.)],
        [(180., 80.), (360., 80.), (240., 110.), (320., 80.)],
    ],
    &[
        [(700., 260.), (920., 250.), (730., 90.), (930., 90.)],
        [(930., 90.), (1130., 90.), (1020., 35.), (1100., 65.)],
    ],
    &[
        [(750., 290.), (1040., 290.), (810., 145.), (990., 145.)],
        [(990., 145.), (1170., 145.), (1100., 75.), (1200., 100.)],
    ],
];

/// Seconds for one gust to cross each streamline, and where each starts.
const CYCLES: [(f64, f64); 4] = [(24.0, 0.0), (30.0, 8.0), (27.0, 16.0), (22.0, 4.0)];

/// The gust's length as a share of its streamline.
const GUST: f32 = 0.2;

const SAMPLES_PER_SEGMENT: usize = 48;

/// Repaint cadence while the wind moves: smooth, but far from an uncapped loop.
pub const FRAME: std::time::Duration = std::time::Duration::from_millis(33);

fn point(curve: &Cubic, t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    let weights = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    curve.iter().zip(weights).fold((0.0, 0.0), |(x, y), ((px, py), w)| (x + px * w, y + py * w))
}

/// A streamline as screen points with their cumulative lengths.
fn polyline(stream: &[Cubic], rect: Rect) -> (Vec<Pos2>, Vec<f32>) {
    let to_screen = |(x, y): (f32, f32)| {
        pos2(rect.left() + x / 1000.0 * rect.width(), rect.top() + y / 300.0 * rect.height())
    };
    let points: Vec<Pos2> = stream
        .iter()
        .enumerate()
        .flat_map(|(i, cubic)| {
            let first = if i == 0 { 0 } else { 1 };
            (first..=SAMPLES_PER_SEGMENT)
                .map(move |s| to_screen(point(cubic, s as f32 / SAMPLES_PER_SEGMENT as f32)))
        })
        .collect();
    let lengths = std::iter::once(0.0)
        .chain(points.windows(2).scan(0.0, |total, w| {
            *total += w[0].distance(w[1]);
            Some(*total)
        }))
        .collect();
    (points, lengths)
}

/// The part of a polyline between two arc lengths.
fn between(points: &[Pos2], lengths: &[f32], from: f32, to: f32) -> Vec<Pos2> {
    let at = |s: f32| {
        let i = lengths.partition_point(|&l| l < s).clamp(1, points.len() - 1);
        let (l0, l1) = (lengths[i - 1], lengths[i]);
        let t = if l1 > l0 { (s - l0) / (l1 - l0) } else { 0.0 };
        points[i - 1].lerp(points[i], t.clamp(0.0, 1.0))
    };
    std::iter::once(at(from))
        .chain(lengths.iter().zip(points).filter(|(l, _)| **l > from && **l < to).map(|(_, p)| *p))
        .chain(std::iter::once(at(to)))
        .collect()
}

/// The wind at one frame: real time for the gentle sway of the streamlines, and
/// the wind's own clock for its gusts, which runs faster when the wind picks up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Air {
    pub time: f64,
    pub clock: f64,
    /// 0 when calm, 1 at the fastest: gusts stretch a little as they speed up.
    pub energy: f32,
}

impl Air {
    /// Calm air: gusts at their usual pace.
    pub fn calm(time: f64) -> Air {
        Air { time, clock: time, energy: 0.0 }
    }
}

/// The fastest the gusts ever travel, as a multiple of calm.
pub const TOP_SPEED: f32 = 4.0;
/// How much a page change adds to the speed, before it dies away.
const PAGE_GUST: f32 = 2.6;
/// Seconds for a page-change gust to fall to about a third.
const GUST_FADE: f64 = 0.7;
/// Seconds the speed takes to follow a change (to about two thirds).
const EASE: f64 = 0.45;

/// The wind's state across frames.
#[derive(Debug, Clone, Default)]
pub struct Gusts {
    clock: f64,
    last: Option<f64>,
    speed: f32,
    gust: f32,
    progress: f32,
}

impl Gusts {
    /// Advance to `now`. `page_changed` starts a gust; `progress` (0 to 1) is how
    /// far an installation has come, or `None` when nothing is installing.
    pub fn advance(&mut self, now: f64, page_changed: bool, progress: Option<f32>) -> Air {
        // A long pause (the window was in the background) is not a burst of wind.
        let dt = self.last.map_or(0.0, |last| (now - last).clamp(0.0, 0.1));
        self.last = Some(now);
        if self.speed == 0.0 {
            self.clock = now;
            self.speed = 1.0;
        }
        if page_changed {
            self.gust = 1.0;
        }
        self.gust *= (-dt / GUST_FADE).exp() as f32;
        // Installing only ever speeds the wind up, until it is done.
        self.progress = match progress {
            Some(p) => self.progress.max(p.clamp(0.0, 1.0)),
            None => 0.0,
        };
        let rising = (TOP_SPEED - 1.0) * self.progress.powf(1.6);
        let target = (1.0 + rising + PAGE_GUST * self.gust).min(TOP_SPEED);
        self.speed += (target - self.speed) * (1.0 - (-dt / EASE).exp()) as f32;
        self.clock += dt * f64::from(self.speed);
        Air { time: now, clock: self.clock, energy: (self.speed - 1.0) / (TOP_SPEED - 1.0) }
    }
}

/// Paint the wind into `rect`. `None` holds it still.
pub fn paint(painter: &Painter, rect: Rect, color: Color32, air: Option<Air>) {
    let time = air.map(|a| a.time);
    let faint = Stroke::new(0.8, color.gamma_multiply(if time.is_some() { 0.10 } else { 0.22 }));
    let gust = Stroke::new(1.0, color.gamma_multiply(0.55));
    let seed = launch_seed();
    for (lane, (stream, (cycle, start))) in STREAMS.iter().zip(CYCLES).enumerate() {
        let (mut points, lengths) = polyline(stream, rect);
        if let Some(time) = time {
            let (dx, dy) = drift(time, seed, lane);
            for point in &mut points {
                *point += eframe::egui::vec2(dx * rect.width(), dy * rect.height());
            }
        }
        painter.add(Shape::line(points.clone(), faint));
        let Some(air) = air else { continue };
        let time = air.clock;
        let total = *lengths.last().unwrap_or(&0.0);
        let offset = noise(seed, lane + 19);
        let cycle = cycle * (0.85 + noise(seed, lane + 29) * 0.45);
        let stretch = 1.0 + 0.5 * air.energy.clamp(0.0, 1.0);
        let dash =
            total * GUST * stretch * (0.75 + 0.25 * (time * 0.09 + offset * 6.0).sin() as f32);
        let phase = ((time + start) / cycle + offset + 0.06 * (time * 0.11 + offset * 6.0).sin())
            .rem_euclid(1.0) as f32;
        let head = phase * (total + dash) - dash;
        let (from, to) = (head.max(0.0), (head + dash).min(total));
        if to > from {
            painter.add(Shape::line(between(&points, &lengths, from, to), gust));
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn run(g: &mut Gusts, from: f64, seconds: f64, progress: Option<f32>) -> Vec<Air> {
        let frames = (seconds * 30.0) as usize;
        (1..=frames).map(|i| g.advance(from + i as f64 / 30.0, false, progress)).collect()
    }

    #[test]
    fn a_page_change_gusts_then_calms_without_jumps() {
        let mut g = Gusts::default();
        run(&mut g, 0.0, 2.0, None);
        let start = g.advance(2.0 + 1.0 / 30.0, true, None);
        let after = run(&mut g, 2.04, 6.0, None);
        let speeds: Vec<f32> =
            after.windows(2).map(|w| ((w[1].clock - w[0].clock) * 30.0) as f32).collect();
        let peak = speeds.iter().cloned().fold(0.0, f32::max);
        assert!(peak > 1.8 && peak <= TOP_SPEED, "a noticeable gust, never too fast: {peak}");
        assert!((speeds.last().unwrap() - 1.0).abs() < 0.05, "back to calm");
        for w in speeds.windows(2) {
            assert!((w[1] - w[0]).abs() < 0.25, "eases, never jumps: {w:?}");
        }
        assert!(after.windows(2).all(|w| w[1].clock > w[0].clock), "never runs backwards");
        assert!(start.energy >= 0.0);
    }

    #[test]
    fn installing_speeds_up_steadily_to_the_top_at_100_percent() {
        let mut g = Gusts::default();
        let mut last = 0.0f32;
        for step in 0..=20 {
            let p = step as f32 / 20.0;
            let air = run(&mut g, step as f64 * 2.0, 2.0, Some(p));
            let energy = air.last().unwrap().energy;
            assert!(energy + 1e-4 >= last, "only ever faster while installing");
            last = energy;
        }
        assert!(last > 0.95 && last <= 1.0, "fastest at 100%: {last}");
        // A stage that reports less progress does not slow it down.
        let before = run(&mut g, 50.0, 1.0, Some(0.2)).last().unwrap().energy;
        assert!(before > 0.95);
        // Done: calm again.
        let calm = run(&mut g, 60.0, 8.0, None).last().unwrap().energy;
        assert!(calm < 0.02);
    }

    #[test]
    fn segments_join_and_s_curves_are_smooth() {
        for stream in STREAMS {
            for pair in stream.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                assert_eq!(a[3], b[0], "segments share endpoints");
                // An SVG `S` reflects the previous control point through the joint.
                let reflected = (2.0 * a[3].0 - a[2].0, 2.0 * a[3].1 - a[2].1);
                assert_eq!(reflected, b[1]);
            }
        }
    }

    #[test]
    fn sub_path_stays_on_the_line() {
        let rect = Rect::from_min_size(Pos2::ZERO, eframe::egui::vec2(1000.0, 300.0));
        let (points, lengths) = polyline(STREAMS[1], rect);
        let total = *lengths.last().unwrap();
        let part = between(&points, &lengths, total * 0.25, total * 0.5);
        assert!(part.len() > 2);
        assert_eq!(between(&points, &lengths, 0.0, total).first(), points.first());
    }

    #[test]
    fn airflow_is_varied_bounded_and_continuous() {
        for lane in 0..4 {
            let a = drift(10.0, 42, lane);
            let b = drift(10.033, 42, lane);
            assert!((a.0 - b.0).abs() < 0.001);
            assert!((a.1 - b.1).abs() < 0.001);
            assert_ne!(a, drift(10.0, 43, lane), "launches have different weather");
            for time in 0..1000 {
                let (x, y) = drift(time as f64, 42, lane);
                assert!(x.abs() <= 0.025 && y.abs() <= 0.07);
            }
        }
    }
}
