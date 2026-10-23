//! Quiet airflow behind the hero: fine streamlines with a slow gust travelling along
//! each one. Decorative only. It never carries meaning, and it holds still when motion
//! is reduced or the window is in the background.

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};

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

/// Paint the wind into `rect`. `time` is seconds since start; `None` holds it still.
pub fn paint(painter: &Painter, rect: Rect, color: Color32, time: Option<f64>) {
    let faint = Stroke::new(0.8, color.gamma_multiply(if time.is_some() { 0.10 } else { 0.22 }));
    let gust = Stroke::new(1.0, color.gamma_multiply(0.55));
    for (stream, (cycle, start)) in STREAMS.iter().zip(CYCLES) {
        let (points, lengths) = polyline(stream, rect);
        painter.add(Shape::line(points.clone(), faint));
        let Some(time) = time else { continue };
        let total = *lengths.last().unwrap_or(&0.0);
        let dash = total * GUST;
        let phase = (((time + start) / cycle) % 1.0) as f32;
        let head = phase * (total + dash) - dash;
        let (from, to) = (head.max(0.0), (head + dash).min(total));
        if to > from {
            painter.add(Shape::line(between(&points, &lengths, from, to), gust));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
