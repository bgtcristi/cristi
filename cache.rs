// src/cache.rs
use std::time::{Duration, Instant};
use dashmap::DashMap;
use serde_json::Value;

/// Cheie = (input_mint, output_mint, amount, only_direct, slippage_bps)
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct QuoteKey {
    pub input: String,
    pub output: String,
    pub amount: u64,
    pub only_direct: Option<bool>,
    pub slippage_bps: u64,
}

#[derive(Clone)]
pub struct QuoteCache {
    ttl: Duration,
    map: DashMap<QuoteKey, (Value, Instant)>,
}

impl QuoteCache {
    /// ttl_secs: de ex. 2 sec (industrial: 1–3s ca să reduci 429)
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            ttl: Duration::from_secs(ttl_secs),
            map: DashMap::new(),
        }
    }

    /// Dacă avem un quote proaspăt, îl returnăm (clone).
    pub fn get_if_fresh(&self, key: &QuoteKey) -> Option<Value> {
        if let Some(entry) = self.map.get(key) {
            let (_v, t) = &*entry;
            if t.elapsed() < self.ttl {
                return Some(entry.value().0.clone());
            }
        }
        None
    }

    /// Salvează/actualizează quote-ul.
    pub fn put(&self, key: QuoteKey, value: Value) {
        self.map.insert(key, (value, Instant::now()));
    }

    /// Curăță intrările expirate (opțional; nu e obligatoriu s-o chemi des).
    pub fn gc(&self) {
        let ttl = self.ttl;
        self.map.retain(|, (, t)| t.elapsed() < ttl);
    }
}