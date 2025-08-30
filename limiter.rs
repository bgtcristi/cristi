use std::{num::NonZeroU32, sync::Arc, time::Duration};
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use rand::{Rng, thread_rng};
use tokio::time::sleep;

#[derive(Clone)]
pub struct Limiter {
    rl: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    jitter: Duration,
}

impl Limiter {
    // rps = cereri/secundă, burst = capacitate bucket, jitter_ms = întârziere random max
    pub fn new(rps: u32, burst: u32, jitter_ms: u64) -> Self {
        let rps_nz = NonZeroU32::new(rps.max(1)).unwrap();
        let burst_nz = NonZeroU32::new(burst.max(1)).unwrap();

        let quota = Quota::per_second(rps_nz).allow_burst(burst_nz);
        let rl = RateLimiter::direct_with_clock(quota, &DefaultClock::default());

        Self {
            rl: Arc::new(rl),
            jitter: Duration::from_millis(jitter_ms),
        }
    }

    /// Așteaptă până e voie să facem următoarea cerere + aplică jitter
    pub async fn wait(&self) {
        self.rl.until_ready().await;

        if self.jitter.as_millis() > 0 {
            let mut rng = thread_rng();
            let j = rng.gen_range(0..self.jitter.as_millis() as u64);
            if j > 0 {
                sleep(Duration::from_millis(j)).await;
            }
        }
    }
} 