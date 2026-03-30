//! Solana Client — Dual-mode: Simulation (local ledger) and Live (real blockchain).
//!
//! Simulation mode is a 1:1 realistic replica of Solana token operations.
//! Same API, same data structures, same validation — but backed by a local
//! JSON ledger instead of the blockchain. When everything is proven, flip
//! HIVE_WALLET_MODE=live to go on-chain.
//!
//! Admin-only: only instance owners (admins) can create and use wallet features.

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::Signer;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::crypto::token::{self, TokenConfig};
use crate::crypto::keystore::Keystore;

/// Balance information for a wallet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletBalance {
    /// SOL balance (for transaction fees).
    pub sol: f64,
    /// HIVE token balance.
    pub hive: f64,
}

/// A transaction record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub id: String,
    pub from: String,
    pub to: String,
    pub amount: f64,
    pub tx_type: String,
    pub timestamp: String,
    pub status: String,
}

/// Operating mode — simulation or live blockchain.
#[derive(Debug, Clone, PartialEq)]
pub enum WalletMode {
    /// Local JSON ledger — 1:1 realistic simulation.
    Simulation,
    /// Real Solana blockchain via RPC.
    Live,
}

impl WalletMode {
    pub fn from_env() -> Self {
        match std::env::var("HIVE_WALLET_MODE").unwrap_or_else(|_| "simulation".into()).to_lowercase().as_str() {
            "live" | "mainnet" | "devnet" => WalletMode::Live,
            _ => WalletMode::Simulation,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  SIMULATED LEDGER — Local JSON-backed 1:1 replica
// ═══════════════════════════════════════════════════════════════════════

/// The local ledger state — persisted to disk as JSON.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimulatedLedger {
    /// Balances indexed by public key.
    pub balances: HashMap<String, WalletBalance>,
    /// Full transaction history.
    pub transactions: Vec<TransactionRecord>,
    /// Monotonic transaction counter for unique IDs.
    pub tx_counter: u64,
    /// Total HIVE ever minted.
    pub total_supply: f64,
}

impl SimulatedLedger {
    fn load(path: &Path) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        }
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create ledger directory: {}", e))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize ledger: {}", e))?;
        std::fs::write(path, json)
            .map_err(|e| format!("Failed to write ledger: {}", e))
    }

    fn next_tx_id(&mut self) -> String {
        self.tx_counter += 1;
        // Generate a realistic-looking Solana signature (base58, ~88 chars)
        format!("sim_{:012}_{}", self.tx_counter, uuid::Uuid::new_v4().to_string().replace("-", ""))
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  UNIFIED CLIENT — Same API for both modes
// ═══════════════════════════════════════════════════════════════════════

/// The HIVE Solana client. Operates in simulation or live mode.
pub struct HiveSolanaClient {
    mode: WalletMode,
    ledger_path: PathBuf,
    config: TokenConfig,
}

impl HiveSolanaClient {
    /// Create a new client. Mode determined by HIVE_WALLET_MODE env var.
    pub fn new() -> Self {
        let mode = WalletMode::from_env();
        let config = TokenConfig::from_env();
        let ledger_path = PathBuf::from("data/wallets/ledger.json");

        tracing::info!(
            "[SOLANA] 🌐 Mode: {} | RPC: {}",
            match &mode {
                WalletMode::Simulation => "🔬 SIMULATION (local ledger)",
                WalletMode::Live => "🔴 LIVE (real blockchain)",
            },
            config.rpc_url,
        );

        Self { mode, ledger_path, config }
    }

    /// Create with explicit config (for testing).
    pub fn new_with_config(mode: WalletMode, ledger_path: PathBuf) -> Self {
        let config = TokenConfig::from_env();
        Self { mode, ledger_path, config }
    }

    /// Get SOL and HIVE balance for a public key.
    pub fn get_balance(&self, pubkey_str: &str) -> Result<WalletBalance, String> {
        match &self.mode {
            WalletMode::Simulation => self.sim_get_balance(pubkey_str),
            WalletMode::Live => self.live_get_balance(pubkey_str),
        }
    }

    /// Transfer HIVE tokens between wallets.
    pub fn transfer_hive(
        &self,
        keystore: &Keystore,
        from_user_id: &str,
        to_pubkey_str: &str,
        amount: f64,
    ) -> Result<String, String> {
        if amount <= 0.0 {
            return Err("Transfer amount must be positive".into());
        }

        match &self.mode {
            WalletMode::Simulation => self.sim_transfer(keystore, from_user_id, to_pubkey_str, amount),
            WalletMode::Live => self.live_transfer(keystore, from_user_id, to_pubkey_str, amount),
        }
    }

    /// Mint new HIVE tokens (creator/system only).
    pub fn mint_hive(
        &self,
        keystore: &Keystore,
        mint_authority_id: &str,
        to_pubkey_str: &str,
        amount: f64,
    ) -> Result<String, String> {
        // Verify role
        let role = keystore.get_role(mint_authority_id)
            .ok_or("Mint authority wallet not found")?;

        match role {
            crate::crypto::keystore::WalletRole::Creator |
            crate::crypto::keystore::WalletRole::System => {},
            _ => return Err("Only the creator or system wallet can mint HIVE".into()),
        }

        if amount <= 0.0 {
            return Err("Mint amount must be positive".into());
        }

        match &self.mode {
            WalletMode::Simulation => self.sim_mint(mint_authority_id, to_pubkey_str, amount),
            WalletMode::Live => self.live_mint(keystore, mint_authority_id, to_pubkey_str, amount),
        }
    }

    /// Get transaction history for a wallet.
    pub fn get_transaction_history(
        &self,
        pubkey_str: &str,
        limit: usize,
    ) -> Result<Vec<TransactionRecord>, String> {
        match &self.mode {
            WalletMode::Simulation => self.sim_history(pubkey_str, limit),
            WalletMode::Live => self.live_history(pubkey_str, limit),
        }
    }

    /// Request SOL airdrop (devnet/simulation only).
    pub fn request_airdrop(&self, pubkey_str: &str, sol_amount: f64) -> Result<String, String> {
        match &self.mode {
            WalletMode::Simulation => {
                let mut ledger = SimulatedLedger::load(&self.ledger_path);
                let balance = ledger.balances.entry(pubkey_str.to_string())
                    .or_insert(WalletBalance { sol: 0.0, hive: 0.0 });
                balance.sol += sol_amount;
                let tx_id = ledger.next_tx_id();
                ledger.transactions.push(TransactionRecord {
                    id: tx_id.clone(),
                    from: "AIRDROP".into(),
                    to: pubkey_str.into(),
                    amount: sol_amount,
                    tx_type: "sol_airdrop".into(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    status: "Success".into(),
                });
                ledger.save(&self.ledger_path)?;
                Ok(tx_id)
            }
            WalletMode::Live => {
                if !self.config.is_devnet {
                    return Err("Airdrops only available on devnet".into());
                }
                let rpc = solana_client::rpc_client::RpcClient::new(self.config.rpc_url.clone());
                let pubkey = Pubkey::from_str(pubkey_str)
                    .map_err(|e| format!("Invalid pubkey: {}", e))?;
                let lamports = (sol_amount * 1_000_000_000.0) as u64;
                let sig = rpc.request_airdrop(&pubkey, lamports)
                    .map_err(|e| format!("Airdrop failed: {}", e))?;
                Ok(sig.to_string())
            }
        }
    }

    /// Get the current mode.
    pub fn mode(&self) -> &WalletMode { &self.mode }

    /// Get total supply (simulation only).
    pub fn total_supply(&self) -> f64 {
        let ledger = SimulatedLedger::load(&self.ledger_path);
        ledger.total_supply
    }

    /// Check if configured.
    pub fn is_configured(&self) -> bool {
        match &self.mode {
            WalletMode::Simulation => true, // Always ready
            WalletMode::Live => self.config.mint_address.is_some(),
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    //  SIMULATION IMPLEMENTATIONS
    // ═══════════════════════════════════════════════════════════════════

    fn sim_get_balance(&self, pubkey_str: &str) -> Result<WalletBalance, String> {
        let ledger = SimulatedLedger::load(&self.ledger_path);
        Ok(ledger.balances.get(pubkey_str)
            .cloned()
            .unwrap_or(WalletBalance { sol: 0.0, hive: 0.0 }))
    }

    fn sim_transfer(
        &self,
        keystore: &Keystore,
        from_user_id: &str,
        to_pubkey_str: &str,
        amount: f64,
    ) -> Result<String, String> {
        let from_pubkey = keystore.get_public_key(from_user_id)
            .ok_or(format!("No wallet found for '{}'", from_user_id))?;

        let mut ledger = SimulatedLedger::load(&self.ledger_path);

        // Check sender balance
        let from_balance = ledger.balances.get(&from_pubkey)
            .ok_or(format!("No balance record for {}", from_pubkey))?;
        if from_balance.hive < amount {
            return Err(format!(
                "Insufficient balance: you have {:.2} HIVE but tried to send {:.2}",
                from_balance.hive, amount
            ));
        }

        // Deduct from sender
        ledger.balances.get_mut(&from_pubkey).unwrap().hive -= amount;

        // Credit to recipient
        let to_balance = ledger.balances.entry(to_pubkey_str.to_string())
            .or_insert(WalletBalance { sol: 0.0, hive: 0.0 });
        to_balance.hive += amount;

        // Record transaction
        let tx_id = ledger.next_tx_id();
        ledger.transactions.push(TransactionRecord {
            id: tx_id.clone(),
            from: from_pubkey.clone(),
            to: to_pubkey_str.into(),
            amount,
            tx_type: "hive_transfer".into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            status: "Success".into(),
        });

        ledger.save(&self.ledger_path)?;

        tracing::info!(
            "[SOLANA:SIM] ✅ Transferred {:.2} HIVE: {} → {} (tx: {})",
            amount, from_pubkey, to_pubkey_str, tx_id
        );

        Ok(tx_id)
    }

    fn sim_mint(
        &self,
        authority_id: &str,
        to_pubkey_str: &str,
        amount: f64,
    ) -> Result<String, String> {
        let mut ledger = SimulatedLedger::load(&self.ledger_path);

        let to_balance = ledger.balances.entry(to_pubkey_str.to_string())
            .or_insert(WalletBalance { sol: 0.0, hive: 0.0 });
        to_balance.hive += amount;
        ledger.total_supply += amount;

        let tx_id = ledger.next_tx_id();
        ledger.transactions.push(TransactionRecord {
            id: tx_id.clone(),
            from: format!("MINT({})", authority_id),
            to: to_pubkey_str.into(),
            amount,
            tx_type: "hive_mint".into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            status: "Success".into(),
        });

        ledger.save(&self.ledger_path)?;

        tracing::info!(
            "[SOLANA:SIM] 🪙 Minted {:.2} HIVE to {} (total supply: {:.2}, tx: {})",
            amount, to_pubkey_str, ledger.total_supply, tx_id
        );

        Ok(tx_id)
    }

    fn sim_history(&self, pubkey_str: &str, limit: usize) -> Result<Vec<TransactionRecord>, String> {
        let ledger = SimulatedLedger::load(&self.ledger_path);
        let records: Vec<TransactionRecord> = ledger.transactions.iter()
            .rev()
            .filter(|tx| tx.from.contains(pubkey_str) || tx.to == pubkey_str)
            .take(limit)
            .cloned()
            .collect();
        Ok(records)
    }

    // ═══════════════════════════════════════════════════════════════════
    //  LIVE BLOCKCHAIN IMPLEMENTATIONS
    // ═══════════════════════════════════════════════════════════════════

    fn live_get_balance(&self, pubkey_str: &str) -> Result<WalletBalance, String> {
        use solana_client::rpc_client::RpcClient;
        use solana_sdk::commitment_config::CommitmentConfig;
        use spl_associated_token_account::get_associated_token_address;

        let rpc = RpcClient::new_with_commitment(
            self.config.rpc_url.clone(),
            CommitmentConfig::confirmed(),
        );
        let pubkey = Pubkey::from_str(pubkey_str)
            .map_err(|e| format!("Invalid public key: {}", e))?;

        let sol_lamports = rpc.get_balance(&pubkey)
            .map_err(|e| format!("Failed to get SOL balance: {}", e))?;
        let sol = sol_lamports as f64 / 1_000_000_000.0;

        let hive = if let Some(mint) = &self.config.mint_address {
            let ata = get_associated_token_address(&pubkey, mint);
            match rpc.get_token_account_balance(&ata) {
                Ok(balance) => balance.ui_amount.unwrap_or(0.0),
                Err(_) => 0.0,
            }
        } else {
            0.0
        };

        Ok(WalletBalance { sol, hive })
    }

    fn live_transfer(
        &self,
        keystore: &Keystore,
        from_user_id: &str,
        to_pubkey_str: &str,
        amount: f64,
    ) -> Result<String, String> {
        use solana_client::rpc_client::RpcClient;
        use solana_sdk::commitment_config::CommitmentConfig;
        use solana_sdk::transaction::Transaction;
        use spl_associated_token_account::{get_associated_token_address, instruction::create_associated_token_account};
        use spl_token::instruction as token_instruction;

        let mint = self.config.mint_address
            .ok_or("HIVE token mint not configured")?;

        let rpc = RpcClient::new_with_commitment(
            self.config.rpc_url.clone(),
            CommitmentConfig::confirmed(),
        );

        let from_keypair = keystore.get_keypair(from_user_id)?;
        let from_pubkey = from_keypair.pubkey();
        let to_pubkey = Pubkey::from_str(to_pubkey_str)
            .map_err(|e| format!("Invalid recipient: {}", e))?;

        let from_ata = get_associated_token_address(&from_pubkey, &mint);
        let to_ata = get_associated_token_address(&to_pubkey, &mint);
        let amount_base = token::to_base_units(amount);

        // Check balance
        let balance = rpc.get_token_account_balance(&from_ata)
            .map_err(|e| format!("Balance check failed: {}", e))?;
        if balance.ui_amount.unwrap_or(0.0) < amount {
            return Err(format!("Insufficient balance"));
        }

        let mut instructions = vec![];
        if rpc.get_account(&to_ata).is_err() {
            instructions.push(create_associated_token_account(
                &from_pubkey, &to_pubkey, &mint, &spl_token::id(),
            ));
        }
        instructions.push(
            token_instruction::transfer(
                &spl_token::id(), &from_ata, &to_ata, &from_pubkey, &[], amount_base,
            ).map_err(|e| format!("Transfer instruction error: {}", e))?
        );

        let blockhash = rpc.get_latest_blockhash()
            .map_err(|e| format!("Blockhash error: {}", e))?;
        let tx = Transaction::new_signed_with_payer(
            &instructions, Some(&from_pubkey), &[&from_keypair], blockhash,
        );
        let sig = rpc.send_and_confirm_transaction(&tx)
            .map_err(|e| format!("Transaction failed: {}", e))?;

        Ok(sig.to_string())
    }

    fn live_mint(
        &self,
        keystore: &Keystore,
        authority_id: &str,
        to_pubkey_str: &str,
        amount: f64,
    ) -> Result<String, String> {
        use solana_client::rpc_client::RpcClient;
        use solana_sdk::commitment_config::CommitmentConfig;
        use solana_sdk::transaction::Transaction;
        use spl_associated_token_account::{get_associated_token_address, instruction::create_associated_token_account};
        use spl_token::instruction as token_instruction;

        let mint = self.config.mint_address
            .ok_or("HIVE token mint not configured")?;

        let rpc = RpcClient::new_with_commitment(
            self.config.rpc_url.clone(),
            CommitmentConfig::confirmed(),
        );

        let auth_keypair = keystore.get_keypair(authority_id)?;
        let auth_pubkey = auth_keypair.pubkey();
        let to_pubkey = Pubkey::from_str(to_pubkey_str)
            .map_err(|e| format!("Invalid recipient: {}", e))?;

        let to_ata = get_associated_token_address(&to_pubkey, &mint);
        let amount_base = token::to_base_units(amount);

        let mut instructions = vec![];
        if rpc.get_account(&to_ata).is_err() {
            instructions.push(create_associated_token_account(
                &auth_pubkey, &to_pubkey, &mint, &spl_token::id(),
            ));
        }
        instructions.push(
            token_instruction::mint_to(
                &spl_token::id(), &mint, &to_ata, &auth_pubkey, &[], amount_base,
            ).map_err(|e| format!("Mint instruction error: {}", e))?
        );

        let blockhash = rpc.get_latest_blockhash()
            .map_err(|e| format!("Blockhash error: {}", e))?;
        let tx = Transaction::new_signed_with_payer(
            &instructions, Some(&auth_pubkey), &[&auth_keypair], blockhash,
        );
        let sig = rpc.send_and_confirm_transaction(&tx)
            .map_err(|e| format!("Mint failed: {}", e))?;

        Ok(sig.to_string())
    }

    fn live_history(&self, pubkey_str: &str, limit: usize) -> Result<Vec<TransactionRecord>, String> {
        use solana_client::rpc_client::RpcClient;

        let rpc = RpcClient::new(self.config.rpc_url.clone());
        let pubkey = Pubkey::from_str(pubkey_str)
            .map_err(|e| format!("Invalid pubkey: {}", e))?;

        let sigs = rpc.get_signatures_for_address(&pubkey)
            .map_err(|e| format!("History query failed: {}", e))?;

        let records: Vec<TransactionRecord> = sigs.into_iter()
            .take(limit)
            .map(|info| TransactionRecord {
                id: info.signature.clone(),
                from: "".into(), // On-chain txns need parsing for from/to
                to: "".into(),
                amount: 0.0,
                tx_type: "on_chain".into(),
                timestamp: info.block_time
                    .map(|t| chrono::DateTime::from_timestamp(t, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default())
                    .unwrap_or_default(),
                status: if info.err.is_some() { "Failed".into() } else { "Success".into() },
            })
            .collect();

        Ok(records)
    }
}
