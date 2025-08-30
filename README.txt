---

SolProRunner – Educational Research Tool

SolProRunner is a trading research tool designed for educational and experimental purposes.
It demonstrates how arbitrage and routing logic can be implemented on Solana using:

Jupiter v6 API for quoting & swapping

Jito bundles for transaction inclusion

Two-leg & three-leg bundle execution

Config-driven architecture (config.json)



---

⚠ Disclaimer
This software is provided for educational purposes only.
It does not constitute financial advice.
Use at your own risk.


---

Project Structure

src/ – Rust source code (arbitrage, bundles, Jito integration)

config.json – Configuration file (tokens, pairs, bundles, thresholds, RPCs)

Cargo.toml – Rust dependencies

README.txt – This document



---

How It Works

1. Pairs & Bundles

Two-leg bundles (A → B → A)

Three-leg bundles (A → B → C → A)


2. Execution

Configurable thresholds for min profit & slippage

Auto-unwind (convert non-SOL tokens back into SOL)

Jito bundles for improved inclusion


3. Logging

✅ Executed (green)

❌ No-Exec (red)

⚠ Impact Too High (yellow)



---

Usage

1. Install Solana CLI

This bot requires Solana CLI to be installed.

Install from the official docs:
https://docs.solana.com/cli/install-solana-cli-tools

Verify installation:

solana --version

Open PowerShell in the bot folder and generate a new wallet:

solana-keygen new -o wallet.json

This creates wallet.json directly inside the project folder.

If you already have a wallet, copy your existing wallet.json into the bot folder.



---

2. Install Rust

Download Rust from: https://www.rust-lang.org/tools/install


---

3. Setup

1. Unzip the project into a folder.


2. Ensure wallet.json is present in the project root.


3. Edit config.json with your own parameters:

wallet_keypair_path: "wallet.json"

rpcs: your Solana mainnet RPC URLs

tokens: token mint addresses you want to trade

pairs: trading pairs you want to enable





---

4. Run the Bot

cargo run --release


---

Important Notes

The bot runs on Solana mainnet.

You must provide your own wallet.json and valid RPC endpoints.

By default, the bot will not execute trades until thresholds in config.json are met.



---