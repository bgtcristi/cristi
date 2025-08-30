// src/jupiter.rs
use anyhow::{anyhow, Result};
use reqwest::{header::ACCEPT, Client};
use serde_json::Value;

#[derive(Clone)]
pub struct JupiterClient {
    base: String,
    prefer_orca: bool,
    http: Client,
    slippage_bps: u64,
}

impl JupiterClient {
    pub fn new(base: String, prefer_orca: bool, slippage_bps: u64) -> Self {
        let http = Client::builder()
            .gzip(true)
            .brotli(true)
            .deflate(true)
            .build()
            .expect("reqwest client build");

        Self {
            base,
            prefer_orca,
            http,
            slippage_bps,
        }
    }

    pub async fn quote(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        only_direct: Option<bool>,
    ) -> Result<Value> {
        let url = format!("{}/quote", self.base);
        let mut req = self
            .http
            .get(&url)
            .header(ACCEPT, "application/json")
            .query(&[
                ("inputMint", input_mint),
                ("outputMint", output_mint),
                ("amount", &amount.to_string()),
                ("slippageBps", &self.slippage_bps.to_string()),
            ]);

        if let Some(d) = only_direct {
            req = req.query(&[("onlyDirectRoutes", &d.to_string())]);
        }
        if self.prefer_orca {
            // opțional, nu strică dacă Jupiter ignoră
            req = req.query(&[("preferDex", "Orca")]);
        }

        let v = req.send().await?.error_for_status()?.json::<Value>().await?;
        Ok(v)
    }

    /// Întoarce base64-ul tranzacției (câmpul "swapTransaction")
    pub async fn swap_tx(
        &self,
        quote: &Value,
        user_pubkey: &str,
        tip_lamports: u64,
    ) -> Result<String> {
        let url = format!("{}/swap", self.base);

        let body = serde_json::json!({
            "quoteResponse": quote,
            "userPublicKey": user_pubkey,
            "wrapAndUnwrapSol": true,
            "asLegacyTransaction": false,
            "useSharedAccounts": true,
            "prioritizationFeeLamports": tip_lamports
        });

        let resp = self
            .http
            .post(&url)
            .header(ACCEPT, "application/json")
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;

        if let Some(s) = resp.get("swapTransaction").and_then(|x| x.as_str()) {
            Ok(s.to_string())
        } else if let Some(s) = resp.as_str() {
            Ok(s.to_string())
        } else {
            Err(anyhow!("Unexpected /swap response: {}", resp))
        }
    }
} 