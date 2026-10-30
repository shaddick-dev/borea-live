//! Airflow behind the hero: gusts blowing right to left, each its own: where it
//! crosses, how long and how wavy it is, how fast it travels and how long it
//! lasts. The wind has weather, too: lulls with a stray wisp, then bursts of
//! several gusts at once. Decorative only: it never carries meaning.
//!
//! The wind picks up: a gust of speed when the page changes, then calms again;
//! and a steady rise while an installation runs, fastest at 100%. Gentle by
//! design: speed changes ease in and out (never a jump), are capped at a few
//! times the calm speed, and never change brightness, so nothing flashes.
//! "Pause wind" (or a window in the background) holds one still frame.

use eframe::egui::{Color32, Painter, Rect, Stroke, pos2};
use std::f64::consts::TAU;
use std::sync::OnceLock;

/// One seed per launch: every launch has different weather. No per-frame
/// randomness: each gust is a pure function of the seed and its number.
pub(crate) fn launch_seed() -> u64 {
    static SEED: OnceLock<u64> = OnceLock::new();
    *SEED.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64)
    })
}

/// A number in [0, 1) from a seed and an index.
fn noise(seed: u64, n: u64) -> f64 {
    let mut x = seed.wrapping_add(n.wrapping_mul(0x9e3779b97f4a7c15));
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((x ^ (x >> 31)) >> 11) as f64 / ((1_u64 << 53) as f64)
}

/// Repaint cadence while the wind moves: smooth, but far from an uncapped loop.
pub const FRAME: std::time::Duration = std::time::Duration::from_millis(33);

/// Gusts may start this often (seconds of wind clock); most slots stay empty.
const SLOT: f64 = 0.3;
/// The longest a gust lives, so only recent slots need looking at.
const LONGEST: f64 = 7.0;

/// How gusty the weather is at `t`, 0 (a lull) to 1 (a burst): slow, uneven
/// swells from three incommensurate waves.
pub fn weather(seed: u64, t: f64) -> f64 {
    let wave = |k: u64, period: f64| (t / period * TAU + noise(seed, 1000 + k) * TAU).sin();
    let w = 0.5 * wave(0, 23.0) + 0.3 * wave(1, 9.7) + 0.2 * wave(2, 4.3);
    // Lulls are long and bursts brief: the swell, sharpened.
    ((w + 1.0) / 2.0).powf(1.8)
}

/// One gust, fixed by the seed and its slot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gust {
    pub start: f64,
    /// Seconds to cross, at calm speed.
    pub life: f64,
    /// Height, as a share of the area.
    pub y: f32,
    /// Length, as a share of the area's width.
    pub length: f32,
    /// Waviness: height in points, and waves across the area.
    pub swell: f32,
    pub waves: f32,
    /// Drifts up (negative) or down as it goes, as a share of the area.
    pub rise: f32,
    pub width: f32,
    pub alpha: f32,
}

/// The gust in slot `n`, if the weather starts one there.
pub fn gust(seed: u64, n: u64) -> Option<Gust> {
    let r = |k: u64| noise(seed, n * 16 + k);
    let start = n as f64 * SLOT + r(0) * SLOT;
    let chance = 0.04 + 0.55 * weather(seed, start);
    if r(1) >= chance {
        return None;
    }
    Some(Gust {
        start,
        life: 2.6 + r(2) * (LONGEST - 2.6),
        y: (0.08 + r(3) * 0.84) as f32,
        length: (0.10 + r(4) * 0.30) as f32,
        swell: (2.0 + r(5) * 14.0) as f32,
        waves: (0.6 + r(6) * 2.2) as f32,
        rise: ((r(7) - 0.5) * 0.25) as f32,
        width: (0.8 + r(8) * 0.9) as f32,
        alpha: (0.22 + r(9) * 0.4) as f32,
    })
}

/// The first wind-clock time from `from` with a few gusts crossing: where the
/// wind starts, and the still frame shown when it is paused.
pub fn busy(seed: u64, from: f64) -> f64 {
    (0..400)
        .map(|k| from + f64::from(k) * 0.5)
        .find(|t| alive(seed, *t).filter(|(_, p)| (0.25..0.75).contains(p)).count() >= 3)
        .unwrap_or(from)
}

/// Gusts alive at wind-clock `t`, with how far each has come (0 to 1).
pub fn alive(seed: u64, t: f64) -> impl Iterator<Item = (Gust, f32)> {
    let first = ((t - LONGEST) / SLOT).floor().max(0.0) as u64;
    let last = (t / SLOT).floor().max(0.0) as u64;
    (first..=last).filter_map(move |n| {
        let g = gust(seed, n)?;
        let progress = (t - g.start) / g.life;
        (0.0..1.0).contains(&progress).then_some((g, progress as f32))
    })
}

/// The wind at one frame: real time, and the wind's own clock for its gusts,
/// which runs faster when the wind picks up.
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
            // Start mid-weather, with gusts already crossing, not an empty sky.
            self.clock = busy(launch_seed(), now + LONGEST);
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

/// Paint the wind into `rect`. `None` holds one still frame.
pub fn paint(painter: &Painter, rect: Rect, color: Color32, air: Option<Air>) {
    let seed = launch_seed();
    let (t, energy) = air
        .map_or_else(|| (busy(seed, 2.0 * LONGEST), 0.0), |a| (a.clock, a.energy.clamp(0.0, 1.0)));
    let w = rect.width();
    let h = rect.height();
    // Small areas (the rail's mark) get gentler waves.
    let scale = (h / 230.0).clamp(0.25, 1.0);
    for (g, progress) in alive(seed, t) {
        // Faster gusts stretch a little: speed lines, not brightness.
        let length = w * g.length * (1.0 + 0.5 * energy);
        // A slight push as it goes: it eases in and leaves quicker.
        let travel = progress * (0.75 + 0.25 * progress);
        // Right to left: the head enters at the right edge, the tail trails behind it.
        let head = rect.right() + length - travel * (w + 2.0 * length);
        // Fades in, holds, fades out: never pops.
        let envelope = (progress * std::f32::consts::PI).sin().powf(0.6);
        const STEPS: usize = 28;
        let point = |i: usize| {
            let along = i as f32 / STEPS as f32;
            let x = head + (1.0 - along) * length;
            let phase = (x - rect.left()) / w * g.waves * std::f32::consts::TAU
                + (t as f32) * 0.8
                + g.y * 9.0;
            let y = rect.top() + h * (g.y + g.rise * travel) + g.swell * scale * phase.sin();
            (pos2(x, y), along)
        };
        let mut previous = point(0);
        for i in 1..=STEPS {
            let next = point(i);
            // Thin, faint tail; fuller head.
            let along = next.1;
            let a = g.alpha * envelope * along.powf(1.4);
            if a > 0.01 && rect.expand(length).contains(next.0) {
                let stroke = Stroke::new(
                    g.width * scale.max(0.6) * (0.4 + 0.6 * along),
                    color.gamma_multiply(a),
                );
                painter.line_segment([previous.0, next.0], stroke);
            }
            previous = next;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn gusts_differ_and_the_weather_changes() {
        let gusts: Vec<Gust> = (0..400).filter_map(|n| gust(42, n)).collect();
        assert!(gusts.len() > 20, "it does blow");
        let first = gusts[0];
        assert!(gusts.iter().any(|g| (g.y - first.y).abs() > 0.2), "different heights");
        assert!(gusts.iter().any(|g| (g.life - first.life).abs() > 1.0), "different speeds");
        assert!(gusts.iter().any(|g| (g.length - first.length).abs() > 0.1), "different lengths");
        // Lulls and bursts: how many are blowing changes over a minute.
        let counts: Vec<usize> = (0..60).map(|s| alive(42, 30.0 + s as f64).count()).collect();
        let (lo, hi) = (counts.iter().min().unwrap(), counts.iter().max().unwrap());
        assert!(hi - lo >= 3, "not constant: {counts:?}");
        assert_ne!(gust(42, 7), gust(43, 7).or(Some(first)), "launches have different weather");
    }

    #[test]
    fn the_wind_starts_and_pauses_with_gusts_in_view() {
        for seed in 0..20 {
            let t = busy(seed, 10.0);
            assert!(alive(seed, t).count() >= 3, "seed {seed}");
        }
    }

    #[test]
    fn gusts_move_on_and_live_their_time() {
        let (g, _) = alive(42, 100.0).next().or_else(|| alive(42, 120.0).next()).unwrap();
        let at = |t: f64| alive(42, t).find(|(x, _)| x.start == g.start).map(|(_, p)| p);
        let p1 = at(g.start + g.life * 0.3).unwrap();
        let p2 = at(g.start + g.life * 0.6).unwrap();
        assert!(p2 > p1, "moves on");
        assert!(at(g.start + g.life + 0.01).is_none(), "and is gone");
        for n in 0..400 {
            if let Some(g) = gust(1, n) {
                assert!((0.0..=1.0).contains(&g.y) && g.life <= LONGEST && g.alpha < 0.7);
            }
        }
    }

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
}
