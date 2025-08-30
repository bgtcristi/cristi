// src/config.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub wallet_keypair_path: Option<String>,

    // Jupiter
    pub jupiter_base: String,
    pub prefer_orca: bool,
    pub max_slippage_bps: u64,
    pub min_profit_bps: u64,
    #[serde(default)]
    pub min_profit: Option<MinProfitCfg>,

    // exec
    pub notional_sol: f64,
    pub dry_run: bool,
    pub poll_ms: u64,

    // agresivitate / rute
    pub aggressive: AggressiveConfig,

    // perechi
    pub pairs: Vec<Pair>,

    // === Bundles (2-leg / 3+-leg / exec) ===
    #[serde(default)]
    pub bundles: Option<BundlesConfig>,

    // token map & markets (opționale)
    #[serde(default)]
    pub tokens: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    pub markets: Option<serde_json::Value>,

    // RPC
    pub rpcs: Vec<String>,
    #[serde(default)]
    pub ws_rpcs: Option<Vec<String>>,
    pub rpc_config: RpcConfig,

    // taxe
    pub fees: Fees,

    // jito
    pub jito: JitoConfig,

    // limiter
    pub limiter: LimiterConfig,

    // auto-unwind
    #[serde(default)]
    pub auto_unwind: Option<AutoUnwindCfg>,

    // diverse opționale
    #[serde(default)]
    pub logs: Option<serde_json::Value>,
    #[serde(default)]
    pub diagnostics: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinProfitCfg {
    pub mode: String,          // "abs" | "pct"
    pub value: f64,
    #[serde(default)]
    pub denom: Option<String>, // ex. "SOL" dacă mode="abs"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggressiveConfig {
    pub enabled: bool,
    #[serde(rename = "only_direct_routes")]
    pub only_direct_routes: bool,
    pub sleep_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pair {
    #[serde(rename = "inputMint")]
    pub input_mint: String,
    #[serde(rename = "outputMint")]
    pub output_mint: String,
    #[serde(default)]
    pub label: Option<String>,
}

/* ===================== Bundles ===================== */

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundlesConfig {
    /// Listele de bundle-uri pe 2 picioare (A<->B)
    #[serde(default)]
    pub two_leg: Vec<TwoLegBundle>,

    /// Listele de bundle-uri pe 3+ picioare.
    /// Acceptă atât cheia "three_leg" cât și "tri_leg".
    #[serde(default, rename = "three_leg", alias = "tri_leg")]
    pub tri_leg: Vec<BundlePath>,

    /// Setări de execuție pentru bundle-uri
    #[serde(default)]
    pub execution: BundleExecConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwoLegBundle {
    /// Etichetă/nume (poate veni ca "name" sau "label")
    #[serde(default, alias = "name", alias = "label")]
    pub label: Option<String>,

    /// Mints: from -> to
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundlePath {
    /// Etichetă/nume (din JSON poate veni ca "name" sau "label")
    #[serde(default, alias = "name")]
    pub label: Option<String>,

    /// Picioare explicite ca listă (ex. ["SOL","USDC","SOL"])
    /// Acceptă atât "legs" cât și "path" din JSON.
    #[serde(default, alias = "path")]
    pub legs: Vec<String>,

    /// Pentru 2-leg, dacă JSON-ul are format {from:"...", to:"..."}
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
}

/* -------- Exec config pentru Bundles -------- */

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleExecConfig {
    #[serde(default)] pub simulate_first: bool,
    #[serde(default)] pub commit: bool,
    #[serde(default = "default_timeout_ms")] pub timeout_ms: u64,
    #[serde(default = "default_retries")] pub retries: u32,
    #[serde(default = "default_price_impact_bps_limit")] pub price_impact_bps_limit: f64,
    #[serde(default = "default_fee_buffer_lamports")] pub fee_buffer_lamports: u64,
    #[serde(default = "default_min_cycle_pnl_lamports")] pub min_cycle_pnl_lamports: u64,
}

impl Default for BundleExecConfig {
    fn default() -> Self {
        Self {
            simulate_first: true,
            commit: true,
            timeout_ms: default_timeout_ms(),
            retries: default_retries(),
            price_impact_bps_limit: default_price_impact_bps_limit(),
            fee_buffer_lamports: default_fee_buffer_lamports(),
            min_cycle_pnl_lamports: default_min_cycle_pnl_lamports(),
        }
    }
}

fn default_timeout_ms() -> u64 { 35_000 }
fn default_retries() -> u32 { 1 }
fn default_price_impact_bps_limit() -> f64 { 20.0 }
fn default_fee_buffer_lamports() -> u64 { 4_050 }
fn default_min_cycle_pnl_lamports() -> u64 { 100_000 }

/* ===================== Alte config-uri ===================== */

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcConfig {
    pub commitment: String,
    #[serde(default)]
    pub preflight_commitment: Option<String>,
    #[serde(default)]
    pub skip_preflight: Option<bool>,
    pub max_retries: u64,
    pub timeout_ms: u64,
    #[serde(default)]
    pub encoding: Option<String>,
    #[serde(default)]
    pub encoding_json: Option<String>,
    #[serde(default)]
    pub http_headers: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fees {
    pub lamports_per_signature: u64,
    pub priority_fee_lamports: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JitoConfig {
    #[serde(rename = "use", alias = "use_", default)]
    pub use_: bool,
    pub block_engine: String,
    pub tip_account: String,
    #[serde(default = "default_tip_lamports")]
    pub default_tip_lamports: u64,
    #[serde(default = "default_max_bundle_retries")]
    pub max_bundle_retries: u32,
}

fn default_tip_lamports() -> u64 { 100_000 }
fn default_max_bundle_retries() -> u32 { 2 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimiterConfig {
    pub rps: u32,
    pub burst: u32,
    pub jitter_ms: u64,
}

// Auto-unwind
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoUnwindCfg {
    pub enabled: bool,
    pub base_mint: String,   // ex: "So1111..." (SOL)
    pub min_token_ui: f64,   // prag minim per token (UI) ca să încerci vânzarea
    pub mode: String,        // "always" | "pnl_gt_0" | "bps"
    pub min_profit_bps: u64, // folosit doar dacă mode="bps"
    pub check_every_ms: u64, // cât de des verifici balanțele pentru unwind
}

impl Config {
    pub fn load_from_file(path: &str) -> Result<Self> {
        let txt = fs::read_to_string(Path::new(path))?;
        let cfg: Self = serde_json::from_str(&txt)?;
        Ok(cfg)
    }
} 