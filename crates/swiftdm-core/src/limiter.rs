//! Token-bucket speed limiter (global, and optionally per download). A rate of 0 means
//! unlimited. The rate can change while downloads run.
//!
//! The global limit is shared fairly between downloads (`acquire_for`): each active download
//! gets an equal share, and what a download cannot use (a slow server) goes to the others.
//! Without this, a download with several warmed-up connections takes nearly all of it.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct Limiter {
    inner: Mutex<Bucket>,
    /// Every byte that passed through, limited or not (used for the download quota).
    received: AtomicU64,
}

struct Bucket {
    rate: u64,
    tokens: f64,
    last: Instant,
    shares: HashMap<usize, Share>,
}

/// One download's use of the global limit.
struct Share {
    /// Recent speed in bytes/s (exponentially decaying average over about a second).
    speed: f64,
    tokens: f64,
    last: Instant,
    since: Instant,
}

/// A download that has stopped reading for this long no longer takes a share.
const IDLE: Duration = Duration::from_secs(2);
/// A new download counts as wanting a full share until its speed is known.
const WARMUP: Duration = Duration::from_secs(2);

impl Limiter {
    pub fn new(rate: u64) -> Self {
        Limiter { inner: Mutex::new(Bucket { rate, tokens: 0.0, last: Instant::now(), shares: HashMap::new() }), received: AtomicU64::new(0) }
    }
    pub fn received(&self) -> u64 {
        self.received.load(Ordering::Relaxed)
    }
    pub fn set_rate(&self, rate: u64) {
        let mut b = self.inner.lock().unwrap();
        if b.rate != rate {
            b.rate = rate;
            b.tokens = 0.0;
            b.last = Instant::now();
            for s in b.shares.values_mut() {
                s.tokens = 0.0;
            }
        }
    }
    pub fn rate(&self) -> u64 {
        self.inner.lock().unwrap().rate
    }
    /// Reserve `n` bytes; returns how long the caller must wait before using them.
    fn reserve(&self, n: usize, key: Option<usize>) -> Duration {
        let mut b = self.inner.lock().unwrap();
        if b.rate == 0 {
            return Duration::ZERO;
        }
        let now = Instant::now();
        let rate = b.rate as f64;
        // Allow at most a quarter second of burst.
        b.tokens = (b.tokens + now.duration_since(b.last).as_secs_f64() * rate).min(rate / 4.0);
        b.last = now;
        b.tokens -= n as f64;
        let mut wait = if b.tokens >= 0.0 { 0.0 } else { -b.tokens / rate };
        if let Some(key) = key {
            b.shares.retain(|_, s| now.duration_since(s.last) < IDLE);
            let s = b.shares.entry(key).or_insert(Share { speed: 0.0, tokens: 0.0, last: now, since: now });
            let dt = now.duration_since(s.last).as_secs_f64();
            s.speed = s.speed * (-dt).exp() + n as f64;
            s.last = now;
            let cap = fair_share(rate, &b.shares, now);
            let s = b.shares.get_mut(&key).unwrap();
            s.tokens = (s.tokens + dt * cap).min(cap / 4.0) - n as f64;
            if s.tokens < 0.0 {
                wait = wait.max(-s.tokens / cap);
            }
        }
        Duration::from_secs_f64(wait)
    }
    pub async fn acquire(&self, n: usize) {
        self.received.fetch_add(n as u64, Ordering::Relaxed);
        let wait = self.reserve(n, None);
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
    /// `acquire` on behalf of one download (`key`), sharing the limit fairly between downloads.
    pub async fn acquire_for(&self, key: usize, n: usize) {
        self.received.fetch_add(n as u64, Ordering::Relaxed);
        let wait = self.reserve(n, Some(key));
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}

/// Water-filling: downloads using clearly less than an equal share keep what they use, and
/// the rest is split equally between the others. Every download may go up to that amount.
fn fair_share(rate: f64, shares: &HashMap<usize, Share>, now: Instant) -> f64 {
    let mut speeds: Vec<f64> =
        shares.values().map(|s| if now.duration_since(s.since) < WARMUP { f64::INFINITY } else { s.speed }).collect();
    speeds.sort_by(f64::total_cmp);
    let (mut left, mut count) = (rate, speeds.len() as f64);
    for sp in speeds {
        if sp >= 0.8 * left / count {
            break;
        }
        left -= sp;
        count -= 1.0;
    }
    (left / count.max(1.0)).max(rate * 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn limits_throughput() {
        let l = Limiter::new(1_000_000);
        let t0 = Instant::now();
        for _ in 0..20 {
            l.acquire(100_000).await;
        }
        let secs = t0.elapsed().as_secs_f64();
        assert!(secs > 1.6 && secs < 2.6, "{secs}");
        let u = Limiter::new(0);
        let t0 = Instant::now();
        u.acquire(1 << 30).await;
        assert!(t0.elapsed() < Duration::from_millis(5));
    }

    #[test]
    fn fair_share_gives_unused_bandwidth_to_others() {
        let now = Instant::now();
        let old = now - Duration::from_secs(10);
        let share = |speed: f64, since| Share { speed, tokens: 0.0, last: now, since };
        // Two busy downloads: half each.
        let two: HashMap<_, _> = [(1, share(900.0, old)), (2, share(900.0, old))].into();
        assert_eq!(fair_share(1000.0, &two, now), 500.0);
        // One held back by a slow server (100): the other may use the remaining 900.
        let slow: HashMap<_, _> = [(1, share(100.0, old)), (2, share(900.0, old))].into();
        assert_eq!(fair_share(1000.0, &slow, now), 900.0);
        // A newcomer counts as busy at once, so the running download makes room for it.
        let new: HashMap<_, _> = [(1, share(1000.0, old)), (2, share(0.0, now))].into();
        assert_eq!(fair_share(1000.0, &new, now), 500.0);
    }
}
