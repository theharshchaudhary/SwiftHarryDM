//! Download quota: "download no more than N MB every H hours". Some internet plans cap
//! traffic per period; when the cap is reached, all downloads stop until enough of the window
//! has passed. Bytes are counted where they are received (the global speed limiter).
use std::collections::VecDeque;

/// Samples of the cumulative received-byte counter.
#[derive(Debug, Default)]
pub struct QuotaWindow {
    samples: VecDeque<(f64, u64)>,
}

/// Keep roughly one sample per this many seconds (older ones only need coarse resolution).
const SAMPLE_EVERY: f64 = 5.0;

impl QuotaWindow {
    pub fn new(now: f64, counter: u64) -> Self {
        QuotaWindow { samples: VecDeque::from([(now, counter)]) }
    }

    pub fn record(&mut self, now: f64, counter: u64) {
        let n = self.samples.len();
        // Replace the newest sample if the one before it is recent, so ticks every 250 ms do
        // not pile up.
        if n > 1 && now - self.samples[n - 2].0 < SAMPLE_EVERY {
            self.samples[n - 1] = (now, counter);
        } else {
            self.samples.push_back((now, counter));
        }
    }

    /// Bytes received during the last `window` seconds, and drop samples older than needed.
    pub fn used(&mut self, now: f64, window: f64) -> u64 {
        let cutoff = now - window;
        // Keep one sample at or before the cutoff as the baseline.
        while self.samples.len() > 1 && self.samples[1].0 <= cutoff {
            self.samples.pop_front();
        }
        let base = self.samples.front().map(|s| s.1).unwrap_or(0);
        let cur = self.samples.back().map(|s| s.1).unwrap_or(0);
        cur.saturating_sub(base)
    }

    /// Seconds until usage falls to `limit` or below, assuming nothing more is downloaded.
    pub fn seconds_until_below(&self, now: f64, window: f64, limit: u64) -> f64 {
        let cur = self.samples.back().map(|s| s.1).unwrap_or(0);
        // Usage drops as old samples leave the window: find the first sample whose departure
        // brings usage under the limit.
        for s in self.samples.iter() {
            if cur.saturating_sub(s.1) <= limit {
                return (s.0 + window - now).max(0.0);
            }
        }
        window
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sliding_window() {
        let mut q = QuotaWindow::new(0.0, 0);
        for t in 1..=100 {
            q.record(t as f64, t as u64 * 10); // 10 bytes per second
        }
        assert_eq!(q.used(100.0, 3600.0), 1000);
        // Only the last ~60 s count once the window is 60 s (5 s sampling granularity).
        let used = q.used(100.0, 60.0);
        assert!((600..=650).contains(&used), "{used}");
        assert!(q.samples.len() < 30, "samples are compacted: {}", q.samples.len());
        // Nothing more downloaded: usage reaches zero once the window has passed.
        let wait = q.seconds_until_below(100.0, 60.0, 0);
        assert!(wait > 55.0 && wait <= 60.0, "{wait}");
        assert_eq!(q.seconds_until_below(100.0, 60.0, 10_000), 0.0);
    }
}
