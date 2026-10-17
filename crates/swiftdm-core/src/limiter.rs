//! Token-bucket speed limiter (global, and optionally per download). A rate of 0 means
//! unlimited. The rate can change while downloads run.
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct Limiter {
    inner: Mutex<Bucket>,
}

struct Bucket {
    rate: u64,
    tokens: f64,
    last: Instant,
}

impl Limiter {
    pub fn new(rate: u64) -> Self {
        Limiter { inner: Mutex::new(Bucket { rate, tokens: 0.0, last: Instant::now() }) }
    }
    pub fn set_rate(&self, rate: u64) {
        let mut b = self.inner.lock().unwrap();
        if b.rate != rate {
            b.rate = rate;
            b.tokens = 0.0;
            b.last = Instant::now();
        }
    }
    pub fn rate(&self) -> u64 {
        self.inner.lock().unwrap().rate
    }
    /// Reserve `n` bytes; returns how long the caller must wait before using them.
    fn reserve(&self, n: usize) -> Duration {
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
        if b.tokens >= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(-b.tokens / rate)
        }
    }
    pub async fn acquire(&self, n: usize) {
        let wait = self.reserve(n);
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
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
}
