use anyhow::{anyhow, Result};
use solana_client::rpc_client::RpcClient;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct RpcRotator {
    urls: Vec<String>,
    idx: Mutex<usize>,
    timeout_ms: u64,
}

impl RpcRotator {
    pub fn new(urls: Vec<String>, timeout_ms: u64) -> Self {
        Self { urls, idx: Mutex::new(0), timeout_ms }
    }

    pub fn client(&self) -> RpcClient {
        let i = *self.idx.lock().unwrap();
        let url = &self.urls[i % self.urls.len()];
        RpcClient::new_with_timeout(url.clone(), Duration::from_millis(self.timeout_ms))
    }

    pub fn rotate(&self) {
        let mut i = self.idx.lock().unwrap();
        *i = (*i + 1) % self.urls.len();
    }

    pub fn current_url(&self) -> String {
        let i = *self.idx.lock().unwrap();
        self.urls[i % self.urls.len()].clone()
    }

    pub fn require(&self) -> Result<()> {
        if self.urls.is_empty() {
            return Err(anyhow!("config.rpcs is empty"));
        }
        Ok(())
    }
}

pub type RpcRotatorRef = Arc<RpcRotator>;