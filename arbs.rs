// src/arbs.rs
use std::{sync::Arc, time::Duration};

use anyhow::Result;
use base64::prelude::*;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::message::VersionedMessage;
use solana_sdk::native_token::sol_to_lamports;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::VersionedTransaction;

use crate::config::{Config, JitoConfig};
use crate::jupiter::JupiterClient;
use crate::rpc::RpcRotator;
use colored::Colorize;
/// rulează o singură trecere peste bundles definite în config
pub async fn run_bundles_once(
    cfg: &Config,
    jup: &JupiterClient,
    rpcs: &Arc<RpcRotator>,
    kp: &Arc<Keypair>,
    jito_cfg: Option<&JitoConfig>,
 ) {
    // dacă nu există secțiunea bundles în config, ieșim
    if cfg.bundles.is_none() {
        return;
    }
    let b = cfg.bundles.as_ref().unwrap();

    if let Some(j) = jito_cfg {
        println!(
            "[BUNDLES] Jito ON (tip_acct={}, default_tip={})",
            j.tip_account, j.default_tip_lamports
        );
    } else {
        println!("[BUNDLES] Jito OFF");
    }
    
    // arbs.rs, în run_bundles_once, după let b = cfg.bundles.as_ref().unwrap();
    let exec = &b.execution;
     println!(
     "[BUNDLES EXEC] simulate_first={} commit={} timeout_ms={} retries={} impact_bps_limit={} fee_buf={} min_cycle_pnl_lamports={}",
    exec.simulate_first,
    exec.commit,
    exec.timeout_ms,
    exec.retries,
    exec.price_impact_bps_limit,
    exec.fee_buffer_lamports,
    exec.min_cycle_pnl_lamports
    );

   // TWO-LEG
    for bl in &b.two_leg {
    // from/to sunt String -> ia-le ca &str direct
    let from  = bl.from.as_str();
    let to    = bl.to.as_str();
    let label = bl.label.as_deref();

    if let Err(e) = try_two_leg(cfg, jup, rpcs, kp, jito_cfg, from, to, label).await {
        eprintln!("[BUNDLE 2L] ERR {}: {:?}", label.unwrap_or("?"), e);
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // TRI-LEG
    for bl3 in &b.tri_leg {
        let label = bl3.label.as_deref();
        if let Err(e) =
            try_three_leg(cfg, jup, rpcs, kp, jito_cfg, &bl3.legs, label).await
        {
            eprintln!(
                "[BUNDLE 3L] ERR {}: {:?}",
                label.unwrap_or("?"),
                e
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn try_two_leg(
    cfg: &Config,
    jup: &JupiterClient,
    rpcs: &Arc<RpcRotator>,
    kp: &Arc<Keypair>,
    _jito_cfg: Option<&JitoConfig>,
    a_mint: &str,
    b_mint: &str,
    label: Option<&str>,
) -> Result<()> {
    let exec = &cfg.bundles.as_ref().unwrap().execution;
    let in_u: u64 = sol_to_lamports(cfg.notional_sol);
    let client: RpcClient = rpcs.client();

    // Quote A->B
    let q_fwd = jup.quote(a_mint, b_mint, in_u, Some(false)).await?;
    let out_b = parse_out(&q_fwd);
    let impact_fwd = parse_impact(&q_fwd);
    let amm_fwd = parse_amm(&q_fwd);

    println!(
        "[B2L FWD] {} {}→{} out(B)={} amm={} impact={}",
        label.unwrap_or(""),
        a_mint, b_mint, out_b, amm_fwd, impact_fwd
    );

    if out_b == 0 || bps(impact_fwd) > exec.price_impact_bps_limit as f64 {
        println!("[B2L DECISION] NO-EXEC: impact prea mare sau out=0");
        return Ok(());
    }

    // Quote B->A
    let q_rev = jup.quote(b_mint, a_mint, out_b, Some(false)).await?;
    let back_a = parse_out(&q_rev);
    let impact_rev = parse_impact(&q_rev);
    let amm_rev = parse_amm(&q_rev);

    let fee_buf = exec.fee_buffer_lamports
        + 2 * cfg.fees.lamports_per_signature
        + cfg.fees.priority_fee_lamports;

    let pnl: i128 = back_a as i128 - in_u as i128 - fee_buf as i128;

    println!(
        "[B2L REV] {} {}→{} back(A)={} amm={} impact={} | fee_buf={} pnl={} thresh={}",
        label.unwrap_or(""),
        b_mint, a_mint, back_a, amm_rev, impact_rev, fee_buf, pnl, exec.min_cycle_pnl_lamports
    );

    if bps(impact_rev) > exec.price_impact_bps_limit as f64 {
        println!("{}", "[B2L DECISION] NO-EXEC: impact prea mare".red().bold());

        return Ok(());
    }
    if pnl < exec.min_cycle_pnl_lamports as i128 {
        println!("{}", format!("[B2L DECISION] NO-EXEC: pnl {} < {}", pnl, exec.min_cycle_pnl_lamports).red().bold());
        return Ok(());
    }

    if exec.simulate_first {
        // folosim quote-ul fwd să construim tx (Jupiter ne dă direct tx b64)
        let tx_b64 = jup.swap_tx(&q_fwd, &kp.pubkey().to_string(), cfg.fees.priority_fee_lamports).await?;
        // doar asigură-te că se poate deserializa (simulare superficială)
        let _vtx: VersionedTransaction = bincode::deserialize(&BASE64_STANDARD.decode(&tx_b64)?)?;
        println!("[B2L] simulate_first OK");
    }

    if !exec.commit || cfg.dry_run {
        println!("{}", "[B2L] DRY (commit=false sau cfg.dry_run=true) — NU trimit tx".yellow());
        return Ok(());
    }

    // Trimite efectiv A->B (single leg). B->A îl va închide bucla clasică când e profitabil.
    // (pentru bundle în același slot, se va folosi ulterior JitoClient)
    let tx_b64 = jup.swap_tx(&q_fwd, &kp.pubkey().to_string(), cfg.fees.priority_fee_lamports).await?;
    let sig = send_signed(&client, kp, &tx_b64)?;
    println!(
    "{} {} sig={}",
    "[B2L EXECUTED]".green().bold(),
    label.unwrap_or(""),
    sig
    );

    Ok(())
}

async fn try_three_leg(
    cfg: &Config,
    jup: &JupiterClient,
    rpcs: &Arc<RpcRotator>,
    kp: &Arc<Keypair>,
    _jito_cfg: Option<&JitoConfig>,
    path: &Vec<String>,
    label: Option<&str>,
) -> Result<()> {
    if path.len() != 4 {
        println!("[B3L] path invalid (trebuie 4 mints A→B→C→A)");
        return Ok(());
    }
    let exec = &cfg.bundles.as_ref().unwrap().execution;
    let a = &path[0]; let b = &path[1]; let c = &path[2]; let a2 = &path[3];
    if a != a2 {
        println!("[B3L] path trebuie să se închidă în A");
        return Ok(());
    }
    let client: RpcClient = rpcs.client();
    let amt_a: u64 = sol_to_lamports(cfg.notional_sol);

    // A->B
    let q1 = jup.quote(a, b, amt_a, Some(false)).await?;
    let out_b = parse_out(&q1);
    let imp1 = parse_impact(&q1);
    if out_b == 0 || bps(imp1) > exec.price_impact_bps_limit as f64 {
        println!("[B3L] stop după A->B: impact/out invalid");
        return Ok(());
    }

    // B->C
    let q2 = jup.quote(b, c, out_b, Some(false)).await?;
    let out_c = parse_out(&q2);
    let imp2 = parse_impact(&q2);
    if out_c == 0 || bps(imp2) > exec.price_impact_bps_limit as f64 {
        println!("[B3L] stop după B->C: impact/out invalid");
        return Ok(());
    }

    // C->A
    let q3 = jup.quote(c, a, out_c, Some(false)).await?;
    let back_a = parse_out(&q3);
    let imp3 = parse_impact(&q3);

    let fee_buf = exec.fee_buffer_lamports
        + 3 * cfg.fees.lamports_per_signature
        + cfg.fees.priority_fee_lamports;

    let pnl: i128 = back_a as i128 - amt_a as i128 - fee_buf as i128;

    println!(
        "[B3L CYCLE] {} A={} B={} C={} | back(A)={} fee_buf={} pnl={} thresh={} | impacts=({:.2}bp,{:.2}bp,{:.2}bp)",
        label.unwrap_or(""),
        a, b, c, back_a, fee_buf, pnl, exec.min_cycle_pnl_lamports,
        bps(imp1), bps(imp2), bps(imp3),
    );

    if bps(imp3) > exec.price_impact_bps_limit as f64 || pnl < exec.min_cycle_pnl_lamports as i128 {
        println!("{}", "[B3L DECISION] NO-EXEC".red().bold());
        return Ok(());
    }

    if exec.simulate_first {
        let _ = bincode::deserialize::<VersionedTransaction>(&BASE64_STANDARD.decode(
            &jup.swap_tx(&q1, &kp.pubkey().to_string(), cfg.fees.priority_fee_lamports).await?,
        )?)?;
        println!("{}", "[B3L] simulate_first OK (pe prima tx)".green());
    }

    if !exec.commit || cfg.dry_run {
        println!("{}", "[B3L] DRY (commit=false sau cfg.dry_run=true) — NU trimit tx".yellow());
        return Ok(());
    }

    // Exec doar prima leg (A->B). Restul le lași pe bot să le închidă când redevine profitabil,
    // ca să eviți 400 la hop-urile următoare.
    let tx_b64 = jup.swap_tx(&q1, &kp.pubkey().to_string(), cfg.fees.priority_fee_lamports).await?;
    let sig = send_signed(&client, kp, &tx_b64)?;
    println!(
    "{} {} sig={}",
    "[B3L EXECUTED]".green().bold(),
    label.unwrap_or(""),
    sig
    );

    Ok(())
}

fn parse_out(q: &serde_json::Value) -> u64 {
    q.get("outAmount")
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

fn parse_impact(q: &serde_json::Value) -> f64 {
    q.get("priceImpactPct")
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn parse_amm(q: &serde_json::Value) -> String {
    q.get("routePlan").and_then(|rp| rp.get(0))
        .and_then(|r0| r0.get("swapInfo"))
        .and_then(|si| si.get("label"))
        .and_then(|l| l.as_str())
        .unwrap_or("?")
        .to_string()
}

fn bps(frac: f64) -> f64 {
    // Jupiter dă impact în fracție (ex: 0.0002 = 2 bps). Convertim în bps.
    frac * 10_000.0
}

fn send_signed(client: &RpcClient, kp: &Keypair, tx_b64: &str) -> Result<String> {
    let tx_bytes = BASE64_STANDARD.decode(tx_b64)?;
    let mut vtx: VersionedTransaction = bincode::deserialize(&tx_bytes)?;

    // (re)semnează local
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
    Ok(sig_str.to_string())
} 