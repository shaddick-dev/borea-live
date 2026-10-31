//! Presentation-only wind clock shared by real-time and deterministic captures.
use crate::wind::{self, Air, Gusts};

pub struct FrostMotion {
    clock: Gusts,
    elapsed: f64,
    air: Air,
}

/// One gust as painted this frame.
pub type Trail = wind::Shape;

impl Default for FrostMotion {
    fn default() -> Self {
        let mut clock = Gusts::default();
        let air = clock.advance(0.0, false, None);
        Self { clock, elapsed: 0.0, air }
    }
}

impl FrostMotion {
    pub fn advance(&mut self, dt: f64, transition: bool, paused: bool, progress: Option<f32>) {
        if !paused {
            self.elapsed += if dt.is_finite() { dt.clamp(0.0, 0.1) } else { 0.0 };
            self.air = self.clock.advance(self.elapsed, transition, progress);
        }
    }

    pub fn trails(&self) -> Vec<Trail> {
        let seed = wind::launch_seed();
        wind::alive(seed, self.air.clock)
            .map(|(g, progress)| wind::shape(seed, &g, progress, self.air.energy))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_accelerates_smoothly_then_settles() {
        let mut wind = FrostMotion::default();
        wind.advance(1.0 / 30.0, true, false, None);
        let first = wind.air.energy;
        assert!(first > 0.0 && first < 0.15, "no instantaneous speed jump");
        let mut peak = first;
        for _ in 0..30 {
            wind.advance(1.0 / 30.0, false, false, None);
            peak = peak.max(wind.air.energy);
        }
        assert!(peak > first * 2.0);
        for _ in 0..180 {
            wind.advance(1.0 / 30.0, false, false, None);
        }
        assert!(wind.air.energy < 0.01);
    }

    #[test]
    fn pause_freezes_every_trail_and_resume_does_not_fast_forward() {
        let mut wind = FrostMotion::default();
        let before = wind.trails();
        let clock = wind.air.clock;
        wind.advance(3600.0, true, true, Some(1.0));
        assert_eq!(wind.trails(), before);
        assert_eq!(wind.air.clock, clock);
        wind.advance(3600.0, false, false, None);
        assert!((0.0..=0.101).contains(&(wind.air.clock - clock)));
    }

    #[test]
    fn the_same_gust_moves_left_as_its_clock_advances() {
        let mut wind = FrostMotion::default();
        let seed = wind::launch_seed();
        let before: Vec<_> = wind::alive(seed, wind.air.clock).collect();
        wind.advance(1.0 / 30.0, false, false, None);
        let after: Vec<_> = wind::alive(seed, wind.air.clock).collect();
        let mut compared = 0;
        for (old, old_progress) in before {
            if let Some((_, new_progress)) = after.iter().find(|(g, _)| g.start == old.start) {
                let x = |p: f32| wind::head(seed, &old, p);
                assert!(x(*new_progress) < x(old_progress));
                compared += 1;
            }
        }
        assert!(compared > 0);
    }
}
