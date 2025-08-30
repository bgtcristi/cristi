// src/main.rs
mod config;
mod rpc;
mod jupiter;
mod resolver;
mod arbitrage;
mod limiter;
mod accounts;

// +++ JITO & bundles
mod jito;
mod arbs;

use colored::Colorize;
use accounts::TOKENS;
use limiter::Limiter;

use resolver::Resolver;
use arbitrage::Arbitrage;

use base64::prelude::*;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_client::rpc_request::TokenAccountsFilter;

use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::message::VersionedMessage;
use solana_sdk::transaction::VersionedTransaction;

use anyhow::{anyhow, Result};
use config::{Config, Pair};
use jupiter::JupiterClient;
use rpc::RpcRotator;

use solana_sdk::native_token::sol_to_lamports;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};

use std::{fs, path::Path, sync::Arc, time::Duration};
use std::str::FromStr;

// +++ JITO & bundles
use crate::jito::JitoClient;
use crate::arbs::run_bundles_once;

// NEW: pentru rezumatul orar
use std::collections::HashMap;
use tokio::sync::Mutex;
use chrono::{Local, Datelike, Timelike};

// ======================= Helpers existente =======================

fn load_keypair_from_file(path: &str) -> Result<Keypair> {
    use solana_sdk::signature::read_keypair_file;

    if let Ok(kp) = read_keypair_file(path) {
        return Ok(kp);
    }
    let data = fs::read_to_string(path)?;
    if data.trim_start().starts_with('[') {
        let vec: Vec<u8> = serde_json::from_str(&data)?;
        return Ok(Keypair::from_bytes(&vec)?);
    }
    Err(anyhow!("unsupported keypair format: {}", path))
}

fn looks_like_base64(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return false;
    }
    let no_ws: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    no_ws.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
        && no_ws.len() >= 4
}

/// simbol (ex: "USDC") pentru un mint, dacă există în accounts::TOKENS
fn symbol_for_mint(mint: &str) -> Option<&'static str> {
    for (sym, m) in TOKENS.iter() {
        if *m == mint {
            return Some(*sym);
        }
    }
    None
}

/// Citește balanța SPL totală (în unități UI) pentru un mint dat.
fn spl_balance_ui(client: &RpcClient, owner: &Pubkey, mint_str: &str) -> Result<(u64, f64)> {
    let mint = mint_str.parse::<Pubkey>()?;
    let accs = client.get_token_accounts_by_owner(owner, TokenAccountsFilter::Mint(mint))?;

    let mut total_amount_raw: u128 = 0;
    let mut decimals: Option<u8> = None;

    for keyed in accs {
        let pk = Pubkey::from_str(&keyed.pubkey)?;
        let bal = client.get_token_account_balance(&pk)?;
        if let Ok(v) = bal.amount.parse::<u128>() {
            total_amount_raw += v;
        }
        if decimals.is_none() {
            decimals = bal.decimals.into();
        }
    }

    let d = decimals.unwrap_or(0) as u32;
    let ui = if d == 0 {
        total_amount_raw as f64
    } else {
        (total_amount_raw as f64) / 10f64.powi(d as i32)
    };

    let raw_u64 = total_amount_raw.min(u64::MAX as u128) as u64;
    Ok((raw_u64, ui))
}

// ======================= STATS + REPORTER (NEW) =======================

#[derive(Default)]
struct Stats {
    total_attempts: u64,
    total_execs: u64,
    total_skips: u64,
    // pe pereche (folosim "SYM1→SYM2" dacă putem, altfel mints)
    per_pair_attempts: HashMap<String, u64>,
    per_pair_execs: HashMap<String, u64>,
    // auto-unwind execs pe token (SYM sau mint)
    unwind_execs: HashMap<String, u64>,
}

impl Stats {
    fn new() -> Self { Self::default() }

    fn pair_key(a_mint: &str, b_mint: &str) -> String {
        let a = symbol_for_mint(a_mint).unwrap_or(a_mint);
        let b = symbol_for_mint(b_mint).unwrap_or(b_mint);
        format!("{a}→{b}")
    }

    fn inc_attempt(&mut self, a_mint: &str, b_mint: &str) {
        self.total_attempts += 1;
        let k = Self::pair_key(a_mint, b_mint);
        *self.per_pair_attempts.entry(k).or_insert(0) += 1;
    }

    fn inc_exec(&mut self, a_mint: &str, b_mint: &str) {
        self.total_execs += 1;
        let k = Self::pair_key(a_mint, b_mint);
        *self.per_pair_execs.entry(k).or_insert(0) += 1;
    }

    fn inc_skip(&mut self) {
        self.total_skips += 1;
    }

    fn inc_unwind_exec(&mut self, token_mint: &str) {
        let k = symbol_for_mint(token_mint).unwrap_or(token_mint).to_string();
        *self.unwind_execs.entry(k).or_insert(0) += 1;
    }

    fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("=== Hourly Summary ===\n");
        s.push_str(&format!("Attempts: {}\nExecs: {}\nSkips: {}\n", self.total_attempts, self.total_execs, self.total_skips));
        s.push_str("\n-- Pair attempts --\n");
        for (k, v) in self.per_pair_attempts.iter() {
            s.push_str(&format!("{k}: {v}\n"));
        }
        s.push_str("\n-- Pair execs --\n");
        for (k, v) in self.per_pair_execs.iter() {
            s.push_str(&format!("{k}: {v}\n"));
        }
        s.push_str("\n-- Auto-unwind execs by token --\n");
        for (k, v) in self.unwind_execs.iter() {
            s.push_str(&format!("{k}: {v}\n"));
        }
        s
    }
}

// Task periodic: scrie fișier din oră în oră
async fn spawn_hourly_reporter(stats: Arc<Mutex<Stats>>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;

            let now = Local::now();
            let dir = Path::new("logs");
            if let Err(e) = fs::create_dir_all(dir) {
                eprintln!("[REPORT] create_dir_all logs failed: {}", e);
                continue;
            }
            let fname = format!("summary-{}{:02}{:02}-{:02}.txt",
                                now.year(), now.month(), now.day(), now.hour());
            let path = dir.join(fname);

            let snapshot = {
                let st = stats.lock().await;
                st.render()
            };

            if let Err(e) = fs::write(&path, snapshot) {
                eprintln!("[REPORT] write failed {}: {}", path.display(), e);
            } else {
                println!("[REPORT] wrote {}", path.display());
            }
        }
    });
}

// ======================= SWAP single-leg (auto-unwind) =======================

async fn swap_single_leg(
    jup: &JupiterClient,
    client: &RpcClient,
    kp: &Keypair,
    input_mint: &str,
    output_mint: &str,
    amount_raw: u64,
    max_price_impact: f64,
    tip_lamports: u64,
    dry_run: bool,
    // NEW: raportare
    stats: &Arc<Mutex<Stats>>,
) -> Result<Option<String>> {
    let quote = jup.quote(input_mint, output_mint, amount_raw, Some(false)).await?;

    let out_u: u64 = quote
        .get("outAmount").and_then(|x| x.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let impact: f64 = quote
        .get("priceImpactPct").and_then(|x| x.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    let amm_label = quote
        .get("routePlan").and_then(|rp| rp.get(0))
        .and_then(|r0| r0.get("swapInfo"))
        .and_then(|si| si.get("label"))
        .and_then(|l| l.as_str())
        .unwrap_or("?");

    println!(
        "[UNWIND QUOTE] {}→{} amt={} out={} amm={} impact={}",
        input_mint, output_mint, amount_raw, out_u, amm_label, impact
    );

    if out_u == 0 || impact > max_price_impact {
        println!("[UNWIND] skip: out=0 sau impact prea mare ({impact})");
        return Ok(None);
    }

    if dry_run {
        println!("[UNWIND] DRY-RUN: ar executa acum.");
        return Ok(None);
    }

    let swap_b64: String = jup.swap_tx(&quote, &kp.pubkey().to_string(), tip_lamports).await?;
    let tx_bytes = BASE64_STANDARD.decode(&swap_b64)?;
    let mut vtx: VersionedTransaction = bincode::deserialize(&tx_bytes)?;

    let msg: &VersionedMessage = &vtx.message;
    let sig = kp.sign_message(&msg.serialize());
    if vtx.signatures.is_empty() {
        vtx.signatures = vec![sig];
    } else {
        vtx.signatures[0] = sig;
    }

    // (simulate a rămas doar pe rutele clasice; aici trimitem direct)
    let sig = client.send_transaction_with_config(
        &vtx,
        RpcSendTransactionConfig {
            skip_preflight: true,
            max_retries: Some(1),
            preflight_commitment: Some(CommitmentConfig::confirmed().commitment),
            ..Default::default()
        },
    )?;
    println!("[UNWIND EXECUTED] sig={}", sig);

    // contor
    {
        let mut st = stats.lock().await;
        st.inc_unwind_exec(input_mint);
    }

    Ok(Some(sig.to_string()))
}

// ======================= AUTO-UNWIND loop =======================

async fn auto_unwind_loop(
    cfg: Arc<Config>,
    jup: Arc<JupiterClient>,
    rpcs: Arc<RpcRotator>,
    kp: Arc<Keypair>,
    stats: Arc<Mutex<Stats>>, // NEW
) {
    let Some(au) = cfg.auto_unwind.clone() else {
        return;
    };
    if !au.enabled {
        return;
    }

    let base_mint = au.base_mint.clone();
    let check_ms = au.check_every_ms;
    let min_ui = au.min_token_ui.max(0.0);
    let max_impact = 0.003_f64; // 0.3% toleranță implicită la unwind

    println!("[AUTO-UNWIND] enabled: base={} min_ui={} every={}ms mode={}",
        base_mint, min_ui, check_ms, au.mode);

    loop {
        let client = rpcs.client();
        for (sym, mint) in TOKENS.iter() {
            if mint == &base_mint { continue; }

            match spl_balance_ui(&client, &kp.pubkey(), mint) {
                Ok((amount_raw, amount_ui)) => {
                    if amount_ui < min_ui {
                        continue;
                    }
                    let sell_raw = ((amount_raw as f64) * 0.995) as u64;
                    if sell_raw == 0 { continue; }

                    println!("[AUTO-UNWIND] {} ({}) balance_ui={:.9} -> selling_raw={}",
                             sym, mint, amount_ui, sell_raw);

                    let _ = swap_single_leg(
                        &jup,
                        &client,
                        &kp,
                        mint,
                        &base_mint,
                        sell_raw,
                        max_impact,
                        cfg.fees.priority_fee_lamports,
                        cfg.dry_run,
                        &stats,
                    ).await;
                }
                Err(e) => {
                    eprintln!("[AUTO-UNWIND] balance read failed for {} ({}): {}", sym, mint, e);
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(check_ms)).await;
    }
}

// ======================= MAIN =======================

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Config::load_from_file("config.json")?;
    let mode = if cfg.dry_run { "DRY-RUN" } else { "LIVE" };
    println!("SolProRunner — Jupiter v6 (Rust) — {}", mode);

    println!("[ACCOUNTS] loaded {} tokens:", TOKENS.len());
    for (sym, mint) in TOKENS.iter() {
        println!("  {} -> {}", sym, mint);
    }

    // pre-scan informativ
    {
        let resolver = Resolver::new().await?;
        let arbitrage = Arbitrage::new();
        let tokens = vec!["SOL", "USDT", "BONK", "mSOL"];

        match arbitrage.check_all_routes(&resolver, &tokens, 1_000_000, 50).await {
            Ok(results) => {
                for (route, profit) in results {
                    println!("[TRI] Route: {}, Profit est.: {}", route, profit);
                }
            }
            Err(e) => eprintln!("[TRI] scan error: {:#}", e),
        }
    }

    // wallet
    let wallet_path = cfg.wallet_keypair_path.as_deref().unwrap_or("wallet.json");
    if !Path::new(wallet_path).exists() {
        return Err(anyhow!("wallet file not found: {}", wallet_path));
    }
    let kp = Arc::new(load_keypair_from_file(wallet_path)?);
    println!("Wallet: {}", kp.pubkey());

    // RPC rotator
    let rpcs = Arc::new(RpcRotator::new(cfg.rpcs.clone(), cfg.rpc_config.timeout_ms));
    rpcs.require()?;
    println!("Using JSON-RPC: {}", rpcs.current_url());

    // Jupiter client
    let jup = Arc::new(JupiterClient::new(
        cfg.jupiter_base.clone(),
        cfg.prefer_orca,
        cfg.max_slippage_bps,
    ));

    // Limiter
    let limiter = Arc::new(Limiter::new(
        cfg.limiter.rps,
        cfg.limiter.burst,
        cfg.limiter.jitter_ms,
    ));

    // NEW: stats shared + reporter
    let stats = Arc::new(Mutex::new(Stats::new()));
    spawn_hourly_reporter(stats.clone()).await;

    // după ce ai cfg, jup, limiter, stats etc.
    let jito_cfg_opt: Option<&config::JitoConfig> =
    if cfg.jito.use_ { Some(&cfg.jito) } else { None };

    // +++ JITO INIT (opțional din config)
    let jito = if cfg.jito.use_ {
     println!("[JITO] enabled: block_engine={} tip_account={} default_tip={} retries={}",
        cfg.jito.block_engine, cfg.jito.tip_account, cfg.jito.default_tip_lamports, cfg.jito.max_bundle_retries);
    Some(JitoClient::new(
        cfg.jito.block_engine.clone(),
        cfg.jito.tip_account.clone(),
        cfg.jito.default_tip_lamports,
        cfg.jito.max_bundle_retries,
    )?)
} else {
    println!("[JITO] disabled");
    None
};

    // AUTO-UNWIND: pornește în paralel, dacă e activat
    {
        let cfg_arc = Arc::new(cfg.clone());
        let jup_arc = jup.clone();
        let rpcs_arc = rpcs.clone();
        let kp_arc = kp.clone();
        let stats_arc = stats.clone();
        tokio::spawn(async move {
            auto_unwind_loop(cfg_arc, jup_arc, rpcs_arc, kp_arc, stats_arc).await; 
        });
    }

    // loop principal (rutele clasice)
    loop {
        let jito_cfg_opt = if cfg.jito.use_ { Some(&cfg.jito) } else { None };
        run_bundles_once(&cfg, &jup, &rpcs, &kp, jito_cfg_opt).await;

        for pair in &cfg.pairs {
            limiter.wait().await;
            if let Err(e) = handle_pair(&cfg, &jup, &rpcs, &kp, pair, &stats).await {
                eprintln!("[PAIR {}→{}] ERR {}", pair.input_mint, pair.output_mint, e);
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        println!("Iteration complete, sleeping for {} ms...", cfg.poll_ms);
        tokio::time::sleep(Duration::from_millis(cfg.poll_ms)).await;
    }
}

// ======================= handle_pair =======================

async fn handle_pair(
    cfg: &Config,
    jup: &JupiterClient,
    rpcs: &Arc<RpcRotator>,
    kp: &Arc<Keypair>,
    pair: &Pair,
    stats: &Arc<Mutex<Stats>>, // NEW
) -> anyhow::Result<()> {
    let client = rpcs.client();
    let balance = client.get_balance(&kp.pubkey()).unwrap_or(0);
    let in_u: u64 = sol_to_lamports(cfg.notional_sol);
    let need = in_u + cfg.fees.lamports_per_signature;
    if !cfg.dry_run && balance < need {
        println!(
            "[PAIR {}→{}] Skipping, balance {} too low vs required {}",
            pair.input_mint, pair.output_mint, balance, need
        );
        // skip count
        {
            let mut st = stats.lock().await;
            st.inc_skip();
        }
        return Ok(());
    }

    // mark attempt
    {
        let mut st = stats.lock().await;
        st.inc_attempt(&pair.input_mint, &pair.output_mint);
    }

    let only_direct = if cfg.aggressive.enabled {
        Some(cfg.aggressive.only_direct_routes)
    } else {
        None
    };
    println!(
        "[PAIR {}→{}] Running (balance {}, dry_run {}, direct {:?})",
        pair.input_mint, pair.output_mint, balance, cfg.dry_run, only_direct
    );

    let quote_fwd = jup
        .quote(&pair.input_mint, &pair.output_mint, in_u, only_direct)
        .await?;

    let out_u_est_b: u64 = quote_fwd
        .get("outAmount").and_then(|x| x.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let impact: f64 = quote_fwd
        .get("priceImpactPct").and_then(|x| x.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    let amm_label = quote_fwd
        .get("routePlan").and_then(|rp| rp.get(0))
        .and_then(|r0| r0.get("swapInfo"))
        .and_then(|si| si.get("label"))
        .and_then(|l| l.as_str())
        .unwrap_or("?");

    println!("[QUOTE FWD] outAmount(B)={}, amm={}, priceImpact={}", out_u_est_b, amm_label, impact);

    let max_impact = 0.001_f64;
    if impact > max_impact {
        println!(
            "{} {}",
            "[DECISION] NO-EXEC".yellow(),
            format!("impact too high ({})", impact)
        );
        let mut st = stats.lock().await;
        st.inc_skip();
        return Ok(());
    }

    let quote_rev = jup
        .quote(&pair.output_mint, &pair.input_mint, out_u_est_b, only_direct)
        .await?;

    let back_to_a_est: u64 = quote_rev
        .get("outAmount").and_then(|x| x.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let fee_buffer: u64 =
        2 * cfg.fees.lamports_per_signature + cfg.fees.priority_fee_lamports;
    let pnl_lamports: i128 = back_to_a_est as i128 - in_u as i128 - fee_buffer as i128;

    let mut thresh_lamports: i128 = 0;
    if let Some(mp) = &cfg.min_profit {
        match mp.mode.as_str() {
            "abs" => {
                if mp.denom.as_deref() == Some("SOL") {
                    let v = (mp.value * 1_000_000_000.0).round() as i128;
                    thresh_lamports = v.max(0);
                }
            }
            "pct" => {
                let v = ((in_u as f64) * mp.value).round() as i128;
                thresh_lamports = v.max(0);
            }
            _ => {}
        }
    }
    if thresh_lamports == 0 {
        let v = ((in_u as u128) * (cfg.min_profit_bps as u128) / 10_000u128) as i128;
        thresh_lamports = v.max(0);
    }

    println!(
        "[CYCLE] in(A)={}, back(A)_est={}, fee_buf={}, pnl={}, thresh={}",
        in_u, back_to_a_est, fee_buffer, pnl_lamports, thresh_lamports
    );

    if pnl_lamports < thresh_lamports {
        println!(
     "{}: cycle pnl {} < threshold {} (skip)",
     "[DECISION] NO-EXEC".red().bold(),
     pnl_lamports,
     thresh_lamports
    );
    let mut st = stats.lock().await;
        st.inc_skip();
        return Ok(());
    }

    if cfg.dry_run {
        println!("[DRY-RUN] Guard passed, would EXEC now.");
        let mut st = stats.lock().await;
        st.inc_exec(&pair.input_mint, &pair.output_mint);
        return Ok(());
    }

    // EXEC
    let user_pubkey = kp.pubkey().to_string();
    let tip: u64 = cfg.fees.priority_fee_lamports;

    let swap_b64: String = jup.swap_tx(&quote_fwd, &user_pubkey, tip).await?;
    let tx_bytes = BASE64_STANDARD.decode(&swap_b64)?;
    let mut vtx: VersionedTransaction = bincode::deserialize(&tx_bytes)?;

    let msg: &VersionedMessage = &vtx.message;
    let sig = kp.sign_message(&msg.serialize());
    if vtx.signatures.is_empty() {
        vtx.signatures = vec![sig];
    } else {
        vtx.signatures[0] = sig;
    }

    let sig_str = client.send_transaction_with_config(
        &vtx,
        RpcSendTransactionConfig {
            skip_preflight: true,
            max_retries: Some(1),
            preflight_commitment: Some(CommitmentConfig::confirmed().commitment),
            ..Default::default()
        },
    )?;
    println!(
    "{} sig={}",
    "[EXECUTED]".green().bold(),
    sig_str
    );

    // contor exec
    {
        let mut st = stats.lock().await;
        st.inc_exec(&pair.input_mint, &pair.output_mint);
    }

    Ok(())
} 