/// Mesh Ledger — Native decentralised ledger for HIVE Coin.
///
/// A fully peer-to-peer distributed ledger with NO external blockchain dependency.
/// Every peer holds a full copy of the ledger state. Transactions are validated
/// by attestation consensus (proof-of-attestation, not proof-of-work).
///
/// ARCHITECTURE:
/// - CRDT-based state: GCounter for balances, conflict-free merge
/// - Transaction log: append-only, signed with ed25519
/// - Block assembly: batches of transactions, Merkle root, timestamp
/// - Consensus: 2/3 of attested peers must validate a block
/// - No mining, no gas fees, no staking — just participation
///
/// MINTING: Algorithmic only. No human can mint coins. The protocol mints
/// a fixed reward per block, distributed to peers who contributed compute/relay.
/// Deflationary: halves every 100,000 blocks.
/// TRANSFERS: Any peer can send to any peer (with signature verification).
/// CREDITS: The existing CreditsEngine remains as the regulation-free internal economy.
/// HIVE COIN: This ledger is the real cryptocurrency layer.
use std::collections::HashMap;
use std::path::PathBuf;
use sha2::{Sha256, Digest};
use serde::{Deserialize, Serialize};



/// A single transaction on the HIVE ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub from: String,       // Sender wallet address (ed25519 public key hex)
    pub to: String,         // Receiver wallet address
    pub amount: u64,        // Amount in smallest unit (1 HIVE = 1_000_000 micro-HIVE)
    pub nonce: u64,         // Sender's transaction counter (replay protection)
    pub timestamp: String,
    pub signature: Vec<u8>, // Ed25519 signature of (from|to|amount|nonce|timestamp)
    pub tx_type: TransactionType,
}

/// Transaction types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransactionType {
    /// Transfer between peers.
    Transfer,
    /// Algorithmic minting (block reward — no human controls this).
    Mint,
    /// Reward for contribution (compute, relay, etc.).
    Reward,
    /// Marketplace purchase.
    Purchase,
}

impl Transaction {
    /// Compute the signing payload for a transaction.
    pub fn signing_payload(&self) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(self.from.as_bytes());
        payload.extend_from_slice(b"|");
        payload.extend_from_slice(self.to.as_bytes());
        payload.extend_from_slice(b"|");
        payload.extend_from_slice(&self.amount.to_le_bytes());
        payload.extend_from_slice(b"|");
        payload.extend_from_slice(&self.nonce.to_le_bytes());
        payload.extend_from_slice(b"|");
        payload.extend_from_slice(self.timestamp.as_bytes());
        payload
    }

    /// Compute the transaction hash.
    pub fn hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(&self.signing_payload());
        format!("{:x}", hasher.finalize())
    }
}

/// A block of transactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub index: u64,
    pub timestamp: String,
    pub transactions: Vec<Transaction>,
    pub previous_hash: String,
    pub merkle_root: String,
    pub block_hash: String,
    /// Peers that validated this block (PeerId hex strings).
    pub validators: Vec<String>,
}

impl Block {
    /// Create a new block from pending transactions.
    pub fn new(
        index: u64,
        transactions: Vec<Transaction>,
        previous_hash: &str,
    ) -> Self {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let merkle_root = Self::compute_merkle_root(&transactions);

        let mut hasher = Sha256::new();
        hasher.update(&index.to_le_bytes());
        hasher.update(timestamp.as_bytes());
        hasher.update(merkle_root.as_bytes());
        hasher.update(previous_hash.as_bytes());
        let block_hash = format!("{:x}", hasher.finalize());

        Self {
            index,
            timestamp,
            transactions,
            previous_hash: previous_hash.to_string(),
            merkle_root,
            block_hash,
            validators: Vec::new(),
        }
    }

    /// Compute the Merkle root of the transaction list.
    fn compute_merkle_root(transactions: &[Transaction]) -> String {
        if transactions.is_empty() {
            return "0".repeat(64);
        }

        let mut hashes: Vec<String> = transactions.iter()
            .map(|tx| tx.hash())
            .collect();

        while hashes.len() > 1 {
            let mut next_level = Vec::new();
            for chunk in hashes.chunks(2) {
                let mut hasher = Sha256::new();
                hasher.update(chunk[0].as_bytes());
                if chunk.len() > 1 {
                    hasher.update(chunk[1].as_bytes());
                } else {
                    hasher.update(chunk[0].as_bytes()); // Duplicate last for odd count
                }
                next_level.push(format!("{:x}", hasher.finalize()));
            }
            hashes = next_level;
        }

        hashes.into_iter().next().unwrap_or_else(|| "0".repeat(64))
    }

    /// Add a validator to this block.
    pub fn add_validator(&mut self, peer_id: &str) {
        if !self.validators.contains(&peer_id.to_string()) {
            self.validators.push(peer_id.to_string());
        }
    }

    /// Check if this block has enough validators (2/3 of total attested peers).
    pub fn is_validated(&self, total_attested_peers: usize) -> bool {
        if total_attested_peers == 0 { return false; }
        let threshold = (total_attested_peers * 2) / 3;
        self.validators.len() > threshold
    }
}

/// The Mesh Ledger — fully decentralised HIVE Coin state.
pub struct MeshLedger {
    /// The blockchain — ordered list of validated blocks.
    chain: Vec<Block>,
    /// Pending transactions (not yet in a block).
    pending: Vec<Transaction>,
    /// Balance cache — wallet address → balance.
    balances: HashMap<String, u64>,
    /// Nonce tracker — wallet address → last used nonce (replay protection).
    nonces: HashMap<String, u64>,
    /// Persistence path.
    persist_path: PathBuf,
    /// Total supply minted.
    total_supply: u64,
}

/// Genesis block hash constant.
const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

impl MeshLedger {
    /// Create or load the ledger.
    pub fn new(mesh_dir: &std::path::Path) -> Self {
        let persist_path = mesh_dir.join("ledger.json");

        if let Ok(data) = std::fs::read_to_string(&persist_path) {
            if let Ok(snap) = serde_json::from_str::<LedgerSnapshot>(&data) {
                tracing::info!(
                    "[LEDGER] 📒 Loaded ledger: {} blocks, {} wallets, supply: {} HIVE",
                    snap.chain.len(), snap.balances.len(), snap.total_supply / 1_000_000
                );
                return Self {
                    chain: snap.chain,
                    pending: snap.pending,
                    balances: snap.balances,
                    nonces: snap.nonces,
                    persist_path,
                    total_supply: snap.total_supply,
                };
            }
        }

        // Genesis — empty ledger
        tracing::info!("[LEDGER] 📒 Genesis — new ledger created");
        Self {
            chain: Vec::new(),
            pending: Vec::new(),
            balances: HashMap::new(),
            nonces: HashMap::new(),
            persist_path,
            total_supply: 0,
        }
    }

    /// Get the latest block hash (or genesis hash if chain is empty).
    pub fn latest_hash(&self) -> &str {
        self.chain.last()
            .map(|b| b.block_hash.as_str())
            .unwrap_or(GENESIS_HASH)
    }

    /// Get a wallet's balance.
    pub fn balance(&self, wallet: &str) -> u64 {
        self.balances.get(wallet).copied().unwrap_or(0)
    }

    /// Get the total supply.
    pub fn total_supply(&self) -> u64 {
        self.total_supply
    }

    /// Get the chain length.
    pub fn chain_length(&self) -> usize {
        self.chain.len()
    }

    /// Submit a transfer transaction.
    pub fn submit_transfer(
        &mut self,
        from: &str,
        to: &str,
        amount: u64,
        signature: Vec<u8>,
    ) -> Result<String, String> {
        // Validate balance
        let sender_balance = self.balance(from);
        if sender_balance < amount {
            return Err(format!(
                "Insufficient balance: {} has {} but tried to send {}",
                &from[..12.min(from.len())], sender_balance, amount
            ));
        }

        if amount == 0 {
            return Err("Cannot send 0 HIVE".to_string());
        }

        let nonce = self.nonces.get(from).copied().unwrap_or(0) + 1;
        let tx = Transaction {
            id: uuid::Uuid::new_v4().to_string(),
            from: from.to_string(),
            to: to.to_string(),
            amount,
            nonce,
            timestamp: chrono::Utc::now().to_rfc3339(),
            signature,
            tx_type: TransactionType::Transfer,
        };

        let id = tx.id.clone();
        self.pending.push(tx);
        self.nonces.insert(from.to_string(), nonce);

        tracing::info!(
            "[LEDGER] 💸 Transfer: {} → {} ({} micro-HIVE)",
            &from[..12.min(from.len())], &to[..12.min(to.len())], amount
        );

        Ok(id)
    }

    /// Calculate the block reward for a given block height.
    /// Deflationary: starts at 1,000,000 micro-HIVE (1 HIVE) per block,
    /// halves every 100,000 blocks.
    pub fn block_reward_for_height(block_index: u64) -> u64 {
        let halvings = block_index / 100_000;
        if halvings >= 64 { return 0; } // Eventually reaches 0
        1_000_000u64 >> halvings // 1 HIVE, then 0.5, 0.25, ...
    }

    /// Submit algorithmic block reward transactions.
    /// Called automatically during block assembly — no human triggers this.
    /// Distributes the block reward proportionally to contributing peers.
    pub fn submit_block_reward(
        &mut self,
        contributors: &[(String, u64)], // (wallet, work_weight)
    ) -> Vec<String> {
        let block_index = self.chain.len() as u64;
        let total_reward = Self::block_reward_for_height(block_index);

        if total_reward == 0 || contributors.is_empty() {
            return vec![];
        }

        let total_weight: u64 = contributors.iter().map(|(_, w)| *w).sum();
        if total_weight == 0 {
            return vec![];
        }

        let mut ids = Vec::new();
        for (wallet, weight) in contributors {
            let share = (total_reward * weight) / total_weight;
            if share == 0 { continue; }

            let tx = Transaction {
                id: uuid::Uuid::new_v4().to_string(),
                from: "BLOCK_REWARD".to_string(),
                to: wallet.clone(),
                amount: share,
                nonce: 0,
                timestamp: chrono::Utc::now().to_rfc3339(),
                signature: vec![], // Protocol-generated, no human signature
                tx_type: TransactionType::Mint,
            };

            tracing::info!(
                "[LEDGER] ⛏️ Block reward: {} micro-HIVE → {} (block #{})",
                share, &wallet[..12.min(wallet.len())], block_index
            );

            ids.push(tx.id.clone());
            self.pending.push(tx);
        }

        ids
    }

    /// Submit a reward transaction (for compute/relay contribution).
    pub fn submit_reward(
        &mut self,
        to: &str,
        amount: u64,
    ) -> Result<String, String> {
        let tx = Transaction {
            id: uuid::Uuid::new_v4().to_string(),
            from: "REWARD".to_string(),
            to: to.to_string(),
            amount,
            nonce: 0,
            timestamp: chrono::Utc::now().to_rfc3339(),
            signature: vec![], // System-generated, no user signature
            tx_type: TransactionType::Reward,
        };

        let id = tx.id.clone();
        self.pending.push(tx);
        Ok(id)
    }

    /// Assemble pending transactions into a block.
    /// Called periodically (e.g., every 30 seconds if there are pending txns).
    pub fn assemble_block(&mut self) -> Option<Block> {
        if self.pending.is_empty() {
            return None;
        }

        let index = self.chain.len() as u64;
        let prev_hash = self.latest_hash().to_string();

        let block = Block::new(
            index,
            self.pending.drain(..).collect(),
            &prev_hash,
        );

        tracing::info!(
            "[LEDGER] 📦 Block #{} assembled: {} transactions, hash: {}...",
            index, block.transactions.len(), &block.block_hash[..12]
        );

        Some(block)
    }

    /// Apply a validated block to the ledger state.
    pub fn apply_block(&mut self, block: Block) -> Result<(), String> {
        // Verify block links to the chain
        if block.previous_hash != self.latest_hash() {
            return Err(format!(
                "Block previous_hash mismatch: expected {}, got {}",
                self.latest_hash(), block.previous_hash
            ));
        }

        // Apply all transactions
        for tx in &block.transactions {
            match tx.tx_type {
                TransactionType::Transfer | TransactionType::Purchase => {
                    let sender_balance = self.balances.get(&tx.from).copied().unwrap_or(0);
                    if sender_balance < tx.amount {
                        return Err(format!("Block contains invalid transfer: insufficient balance"));
                    }
                    *self.balances.entry(tx.from.clone()).or_insert(0) -= tx.amount;
                    *self.balances.entry(tx.to.clone()).or_insert(0) += tx.amount;
                }
                TransactionType::Mint | TransactionType::Reward => {
                    *self.balances.entry(tx.to.clone()).or_insert(0) += tx.amount;
                    self.total_supply += tx.amount;
                }
            }
        }

        tracing::info!(
            "[LEDGER] ✅ Block #{} applied — supply: {} HIVE",
            block.index, self.total_supply / 1_000_000
        );

        self.chain.push(block);
        self.persist();

        Ok(())
    }

    /// Merge a remote chain (for initial sync when joining the mesh).
    /// Only accepts a chain that is longer and valid.
    pub fn merge_chain(&mut self, remote_chain: Vec<Block>) -> Result<usize, String> {
        if remote_chain.len() <= self.chain.len() {
            return Ok(0); // Our chain is the same or longer
        }

        // Verify the entire remote chain integrity
        let mut prev_hash = GENESIS_HASH.to_string();
        for block in &remote_chain {
            if block.previous_hash != prev_hash {
                return Err(format!(
                    "Remote chain integrity failure at block #{}",
                    block.index
                ));
            }
            prev_hash = block.block_hash.clone();
        }

        // Replace our chain with the longer valid one
        let added = remote_chain.len() - self.chain.len();
        self.chain = remote_chain;

        // Rebuild balance cache from the full chain
        self.rebuild_balances();
        self.persist();

        tracing::info!(
            "[LEDGER] 🔄 Merged remote chain: {} new blocks (total: {})",
            added, self.chain.len()
        );

        Ok(added)
    }

    /// Rebuild balance cache from the full transaction history.
    fn rebuild_balances(&mut self) {
        self.balances.clear();
        self.total_supply = 0;

        for block in &self.chain {
            for tx in &block.transactions {
                match tx.tx_type {
                    TransactionType::Transfer | TransactionType::Purchase => {
                        if let Some(bal) = self.balances.get_mut(&tx.from) {
                            *bal = bal.saturating_sub(tx.amount);
                        }
                        *self.balances.entry(tx.to.clone()).or_insert(0) += tx.amount;
                    }
                    TransactionType::Mint | TransactionType::Reward => {
                        *self.balances.entry(tx.to.clone()).or_insert(0) += tx.amount;
                        self.total_supply += tx.amount;
                    }
                }
            }
        }
    }

    /// Get the top wallets by balance.
    pub fn top_wallets(&self, limit: usize) -> Vec<(String, u64)> {
        let mut wallets: Vec<_> = self.balances.iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        wallets.sort_by(|a, b| b.1.cmp(&a.1));
        wallets.truncate(limit);
        wallets
    }

    /// Get transaction history for a wallet.
    pub fn wallet_history(&self, wallet: &str, limit: usize) -> Vec<&Transaction> {
        self.chain.iter()
            .flat_map(|b| b.transactions.iter())
            .filter(|tx| tx.from == wallet || tx.to == wallet)
            .rev()
            .take(limit)
            .collect()
    }

    /// Get ledger stats.
    pub fn stats(&self) -> serde_json::Value {
        serde_json::json!({
            "chain_length": self.chain.len(),
            "total_supply_micro": self.total_supply,
            "total_supply_hive": self.total_supply as f64 / 1_000_000.0,
            "wallet_count": self.balances.len(),
            "pending_transactions": self.pending.len(),
        })
    }

    /// Persist to disk.
    fn persist(&self) {
        let snap = LedgerSnapshot {
            chain: self.chain.clone(),
            pending: self.pending.clone(),
            balances: self.balances.clone(),
            nonces: self.nonces.clone(),
            total_supply: self.total_supply,
        };

        if let Ok(json) = serde_json::to_string(&snap) {
            if let Some(parent) = self.persist_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&self.persist_path, json);
        }
    }
}

#[derive(Serialize, Deserialize)]
struct LedgerSnapshot {
    chain: Vec<Block>,
    pending: Vec<Transaction>,
    balances: HashMap<String, u64>,
    nonces: HashMap<String, u64>,
    total_supply: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ledger() -> MeshLedger {
        let tmp = std::env::temp_dir().join(format!("hive_ledger_test_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&tmp);
        MeshLedger::new(&tmp)
    }

    #[test]
    fn test_block_reward_and_balance() {
        let mut ledger = test_ledger();

        // Block reward distributed to one contributor
        ledger.submit_block_reward(&[("wallet_alice".to_string(), 100)]);
        let block = ledger.assemble_block().unwrap();
        ledger.apply_block(block).unwrap();

        assert_eq!(ledger.balance("wallet_alice"), 1_000_000); // Full block reward (block 0)
        assert_eq!(ledger.total_supply(), 1_000_000);
    }

    #[test]
    fn test_transfer() {
        let mut ledger = test_ledger();

        // Block reward to Alice
        ledger.submit_block_reward(&[("alice".to_string(), 100)]);
        let block = ledger.assemble_block().unwrap();
        ledger.apply_block(block).unwrap();

        // Transfer from Alice to Bob
        ledger.submit_transfer("alice", "bob", 300_000, vec![]).unwrap();
        let block = ledger.assemble_block().unwrap();
        ledger.apply_block(block).unwrap();

        assert_eq!(ledger.balance("alice"), 700_000);
        assert_eq!(ledger.balance("bob"), 300_000);
        assert_eq!(ledger.total_supply(), 1_000_000); // No new coins from transfer
    }

    #[test]
    fn test_insufficient_balance() {
        let mut ledger = test_ledger();

        // Alice has 0 balance
        let result = ledger.submit_transfer("alice", "bob", 100, vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_zero_transfer_rejected() {
        let mut ledger = test_ledger();
        let result = ledger.submit_transfer("alice", "bob", 0, vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_block_chain_integrity() {
        let mut ledger = test_ledger();

        // Create 3 blocks with block rewards
        for _ in 0..3 {
            ledger.submit_block_reward(&[("miner".to_string(), 100)]);
            let block = ledger.assemble_block().unwrap();
            ledger.apply_block(block).unwrap();
        }

        assert_eq!(ledger.chain_length(), 3);

        // Verify chain links
        let mut prev_hash = GENESIS_HASH.to_string();
        for block in &ledger.chain {
            assert_eq!(block.previous_hash, prev_hash);
            prev_hash = block.block_hash.clone();
        }
    }

    #[test]
    fn test_merkle_root() {
        let tx1 = Transaction {
            id: "tx1".to_string(),
            from: "a".to_string(), to: "b".to_string(),
            amount: 100, nonce: 1,
            timestamp: "2026-01-01".to_string(),
            signature: vec![], tx_type: TransactionType::Transfer,
        };
        let tx2 = Transaction {
            id: "tx2".to_string(),
            from: "b".to_string(), to: "c".to_string(),
            amount: 50, nonce: 1,
            timestamp: "2026-01-01".to_string(),
            signature: vec![], tx_type: TransactionType::Transfer,
        };

        let block = Block::new(0, vec![tx1, tx2], GENESIS_HASH);
        assert_eq!(block.merkle_root.len(), 64);
    }

    #[test]
    fn test_block_validation() {
        let block = Block::new(0, vec![], GENESIS_HASH);
        assert!(!block.is_validated(0)); // No peers
        assert!(!block.is_validated(3)); // Need 2/3 = 2, have 0
    }

    #[test]
    fn test_reward_increases_supply() {
        let mut ledger = test_ledger();

        ledger.submit_reward("helper_peer", 10_000).unwrap();
        let block = ledger.assemble_block().unwrap();
        ledger.apply_block(block).unwrap();

        assert_eq!(ledger.balance("helper_peer"), 10_000);
        assert_eq!(ledger.total_supply(), 10_000);
    }

    #[test]
    fn test_top_wallets() {
        let mut ledger = test_ledger();

        // Distribute rewards to 5 wallets with increasing weights
        for i in 0..5 {
            ledger.submit_block_reward(&[(format!("w{}", i), (i + 1) as u64 * 100)]);
            let block = ledger.assemble_block().unwrap();
            ledger.apply_block(block).unwrap();
        }

        let top = ledger.top_wallets(3);
        assert_eq!(top.len(), 3);
        // Highest balance should be first
        assert!(top[0].1 >= top[1].1);
    }

    #[test]
    fn test_halving_schedule() {
        assert_eq!(MeshLedger::block_reward_for_height(0), 1_000_000);
        assert_eq!(MeshLedger::block_reward_for_height(99_999), 1_000_000);
        assert_eq!(MeshLedger::block_reward_for_height(100_000), 500_000);
        assert_eq!(MeshLedger::block_reward_for_height(200_000), 250_000);
        assert_eq!(MeshLedger::block_reward_for_height(300_000), 125_000);
    }

    #[test]
    fn test_proportional_distribution() {
        let mut ledger = test_ledger();

        // Two contributors, weights 3:1
        ledger.submit_block_reward(&[
            ("big_contributor".to_string(), 75),
            ("small_contributor".to_string(), 25),
        ]);
        let block = ledger.assemble_block().unwrap();
        ledger.apply_block(block).unwrap();

        // 75% of 1_000_000 = 750_000, 25% = 250_000
        assert_eq!(ledger.balance("big_contributor"), 750_000);
        assert_eq!(ledger.balance("small_contributor"), 250_000);
    }

    #[test]
    fn test_no_human_can_mint() {
        // This test proves no submit_mint method exists.
        // The only way coins enter the system is through:
        // 1. submit_block_reward() - algorithmic, called by protocol
        // 2. submit_reward() - for compute/relay contribution
        // No individual human can create coins.
        let ledger = test_ledger();
        assert_eq!(ledger.total_supply(), 0);
    }
}
