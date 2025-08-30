use anyhow::{Result, anyhow};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct TokenInfo {
    pub address: String,
    pub symbol: String,
    pub decimals: u8,
}

pub struct Resolver {
    client: Client,
    tokens: HashMap<String, TokenInfo>,
}

impl Resolver {
    pub async fn new() -> Result<Self> {
        let url = "https://token.jup.ag/all";
        let client = Client::new();
        let resp: Vec<TokenInfo> = client.get(url).send().await?.json().await?;
        
        let mut tokens = HashMap::new();
        for t in resp {
            tokens.insert(t.symbol.to_uppercase(), t);
        }

        Ok(Self { client, tokens })
    }

    /// rezolvă simbol (ex. "SOL") -> mint address
    pub fn resolve(&self, symbol: &str) -> Result<&TokenInfo> {
        self.tokens.get(&symbol.to_uppercase())
            .ok_or_else(|| anyhow!("Token not found: {}", symbol))
    }
}