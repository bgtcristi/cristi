use anyhow::{anyhow,Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use bincode;
use reqwest::Client;
use serde_json::json;
use solana_sdk::{
    hash::Hash,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::{Transaction, VersionedTransaction},
};

pub struct Jito {
    http: Client,
    pub block_engine: String,
    pub tip_account: String,
}

impl Jito {
    pub fn new(block_engine: String, tip_account: String) -> Self {
        let http = Client::builder()
            .user_agent("solpro-runner-rs/1.0")
            .build()
            .unwrap();
        Self { http, block_engine, tip_account }
    }

    pub fn tip_pubkey(&self) -> Result<Pubkey> {
        self.tip_account
            .parse::<Pubkey>()
            .map_err(|e| anyhow!("invalid tip_account pubkey: {}", e))
    }

    /// Trimite bundle (tip + swap) la Block Engine. Returnează id-ul bundle-ului.
    pub async fn send_bundle(&self, tip_vtx: &VersionedTransaction, swap_vtx: &VersionedTransaction) -> Result<String> {
        // serialize + b64
        let tip_b64  = B64.encode(bincode::serialize(tip_vtx)?);
        let swap_b64 = B64.encode(bincode::serialize(swap_vtx)?);

        let body = json!({
            "transactions": [tip_b64, swap_b64]
        });

        let resp = self.http.post(&self.block_engine).json(&body).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!("[JITO] HTTP {} {}", status, txt));
        }
        // câmpurile pot varia; încercăm "id" sau un câmp text generic
        let v: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!({}));
        if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
            Ok(id.to_string())
        } else {
            Ok(v.to_string())
        }
    }
}

/// Construiește o tranzacție de tip (transfer SOL) și o întoarce ca VersionedTransaction.
pub fn build_tip_tx_v0(
    payer: &Keypair,
    tip_account: &Pubkey,
    lamports: u64,
    recent_blockhash: Hash,
) -> Result<VersionedTransaction> {
    let ix = system_instruction::transfer(&payer.pubkey(), tip_account, lamports);
    let message = Message::new_with_blockhash(&[ix], Some(&payer.pubkey()), &recent_blockhash);

    let mut legacy = Transaction::new_unsigned(message);
    legacy.try_sign(&[payer], recent_blockhash)?;

    Ok(VersionedTransaction::from(legacy))
}

/// Convertor simplu: legacy Transaction -> VersionedTransaction.
pub fn legacy_to_v0(tx: &Transaction) -> Result<VersionedTransaction> {
    Ok(VersionedTransaction::from(tx.clone()))
}

/// Client Jito minimal (shim). Îl completăm ulterior.
pub struct JitoClient {
    pub block_engine: String,
    pub tip_account: String,
    pub default_tip_lamports: u64,
    pub max_bundle_retries: u32,
}

impl JitoClient {
    pub fn new(
        block_engine: String,
        tip_account: String,
        default_tip_lamports: u64,
        max_bundle_retries: u32,
    ) -> Result<Self> {
        Ok(Self {
            block_engine,
            tip_account,
            default_tip_lamports,
            max_bundle_retries,
        })
    }
}