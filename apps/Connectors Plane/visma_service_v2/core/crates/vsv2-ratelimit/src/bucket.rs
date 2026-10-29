use std::time::{Duration, Instant};

use tokio::sync::Mutex;

/// Tokio-async token bucket. Refills at `refill_per_sec`, capped at `capacity`.
#[derive(Debug)]
pub struct TokenBucket {
    pub capacity: u32,
    pub refill_per_sec: f64,
    state: Mutex<BucketState>,
}

#[derive(Debug)]
struct BucketState {
    tokens: f64,
    last_tick: Instant,
}

impl TokenBucket {
    pub fn new(capacity: u32, refill_per_sec: f64) -> Self {
        Self {
            capacity,
            refill_per_sec,
            state: Mutex::new(BucketState {
                tokens: capacity as f64,
                last_tick: Instant::now(),
            }),
        }
    }

    /// Wait until 1 token is available, then consume it.
    pub async fn acquire(&self) {
        loop {
            let wait = {
                let mut s = self.state.lock().await;
                let now = Instant::now();
                let elapsed = now.duration_since(s.last_tick).as_secs_f64();
                s.tokens = (s.tokens + elapsed * self.refill_per_sec).min(self.capacity as f64);
                s.last_tick = now;

                if s.tokens >= 1.0 {
                    s.tokens -= 1.0;
                    return;
                }
                let needed = 1.0 - s.tokens;
                Duration::from_secs_f64(needed / self.refill_per_sec)
            };
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn capacity_consumed_immediately() {
        let b = TokenBucket::new(3, 100.0);
        let started = Instant::now();
        for _ in 0..3 {
            b.acquire().await;
        }
        // 3 tokens preloaded, no sleep needed
        assert!(started.elapsed().as_millis() < 50);
    }

    #[tokio::test]
    async fn refill_blocks_until_token_available() {
        let b = TokenBucket::new(1, 5.0); // 5 tokens/sec → 200 ms per token
        b.acquire().await; // immediate
        let started = Instant::now();
        b.acquire().await; // must wait ~200 ms
        let elapsed = started.elapsed().as_millis();
        assert!(elapsed >= 150, "expected ≥150 ms, got {elapsed}");
    }
}
