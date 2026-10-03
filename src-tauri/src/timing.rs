//! Hotkey-to-visible timing, recorded inside the app.
//!
//! Two numbers are kept for every time the hotkey shows the launcher:
//!
//! * `shown`: hotkey handler entered, to the OS call that shows and focuses
//!   the window returning. The compositor puts the window on screen at its
//!   next refresh, so what the eye sees is this plus up to one refresh.
//! * `first_frame`: hotkey handler entered, to the webview running its first
//!   animation frame after being shown, as reported back over IPC.
//!
//! Neither includes the time the OS takes to deliver the key press to the
//! process. A true black-box figure needs an external tool.

use serde::Serialize;
use std::time::Instant;

/// Enough for a long session without growing forever.
const KEEP: usize = 500;

#[derive(Default)]
pub struct Timing {
    next_seq: u64,
    pending: Option<(u64, Instant)>,
    shown_ms: Vec<f64>,
    first_frame_ms: Vec<f64>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub count: usize,
    pub last: f64,
    pub median: f64,
    pub p95: f64,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyTimings {
    pub shown: Summary,
    pub first_frame: Summary,
}

impl Timing {
    /// Record that the window was shown; returns the id the webview must echo.
    pub fn shown(&mut self, started: Instant) -> u64 {
        push(&mut self.shown_ms, millis_since(started));
        self.next_seq += 1;
        self.pending = Some((self.next_seq, started));
        self.next_seq
    }

    /// The webview painted. Ignored unless it answers the latest show.
    pub fn first_frame(&mut self, seq: u64) {
        if let Some((pending, started)) = self.pending {
            if pending == seq {
                push(&mut self.first_frame_ms, millis_since(started));
                self.pending = None;
            }
        }
    }

    pub fn report(&self) -> HotkeyTimings {
        HotkeyTimings {
            shown: summarize(&self.shown_ms),
            first_frame: summarize(&self.first_frame_ms),
        }
    }
}

fn millis_since(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1_000.0
}

fn push(samples: &mut Vec<f64>, value: f64) {
    if samples.len() == KEEP {
        samples.remove(0);
    }
    samples.push(value);
}

pub fn summarize(samples: &[f64]) -> Summary {
    let Some(&last) = samples.last() else {
        return Summary::default();
    };
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    // Nearest-rank: the smallest sample with at least q of the data at or below it.
    let at = |q: f64| sorted[((sorted.len() as f64 * q).ceil() as usize).clamp(1, sorted.len()) - 1];
    Summary {
        count: sorted.len(),
        last,
        median: at(0.50),
        p95: at(0.95),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_samples_summarizes_to_zeros() {
        assert_eq!(summarize(&[]), Summary::default());
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let samples: Vec<f64> = (1..=100).rev().map(f64::from).collect();
        let summary = summarize(&samples);
        assert_eq!(summary.count, 100);
        assert_eq!(summary.last, 1.0);
        assert_eq!(summary.median, 50.0);
        assert_eq!(summary.p95, 95.0);
    }

    #[test]
    fn a_single_sample_is_its_own_median_and_p95() {
        let summary = summarize(&[7.5]);
        assert_eq!((summary.median, summary.p95), (7.5, 7.5));
    }

    #[test]
    fn only_the_latest_show_accepts_a_first_frame() {
        let mut timing = Timing::default();
        let first = timing.shown(Instant::now());
        let second = timing.shown(Instant::now());
        timing.first_frame(first);
        assert_eq!(timing.report().first_frame.count, 0);
        timing.first_frame(second);
        timing.first_frame(second);
        assert_eq!(timing.report().first_frame.count, 1);
        assert_eq!(timing.report().shown.count, 2);
    }

    #[test]
    fn old_samples_are_dropped_once_full() {
        let mut samples = Vec::new();
        for n in 0..KEEP + 10 {
            push(&mut samples, n as f64);
        }
        assert_eq!(samples.len(), KEEP);
        assert_eq!(samples[0], 10.0);
    }
}
