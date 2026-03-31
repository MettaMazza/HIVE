//! Tests for the crypto module — keystore, token config, and Solana client (simulation mode).

#[cfg(test)]
mod keystore_tests {
    use crate::crypto::keystore::{Keystore, WalletRole};
    use solana_sdk::signer::Signer;
    use tempfile::TempDir;

    fn test_keystore() -> (Keystore, TempDir) {
        let dir = TempDir::new().unwrap();
        let ks = Keystore::new_with_secret(dir.path().join("wallets"), "test_secret_32bytes_minimum_len!".into());
        (ks, dir)
    }

    #[test]
    fn test_create_wallet_returns_pubkey() {
        let (ks, _dir) = test_keystore();
        let pubkey = ks.create_wallet("user_123", WalletRole::User).unwrap();
        assert!(pubkey.len() >= 32 && pubkey.len() <= 44, "Invalid pubkey length: {}", pubkey.len());
    }

    #[test]
    fn test_wallet_exists() {
        let (ks, _dir) = test_keystore();
        assert!(!ks.wallet_exists("user_456"));
        ks.create_wallet("user_456", WalletRole::User).unwrap();
        assert!(ks.wallet_exists("user_456"));
    }

    #[test]
    fn test_duplicate_wallet_rejected() {
        let (ks, _dir) = test_keystore();
        ks.create_wallet("user_789", WalletRole::User).unwrap();
        let result = ks.create_wallet("user_789", WalletRole::User);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[test]
    fn test_get_public_key() {
        let (ks, _dir) = test_keystore();
        assert!(ks.get_public_key("nobody").is_none());
        let created_pubkey = ks.create_wallet("user_pub", WalletRole::User).unwrap();
        let retrieved_pubkey = ks.get_public_key("user_pub").unwrap();
        assert_eq!(created_pubkey, retrieved_pubkey);
    }

    #[test]
    fn test_get_role() {
        let (ks, _dir) = test_keystore();
        ks.create_wallet("creator_wallet", WalletRole::Creator).unwrap();
        ks.create_wallet("admin_wallet", WalletRole::Admin).unwrap();
        ks.create_wallet("user_wallet", WalletRole::User).unwrap();
        ks.create_wallet("system_wallet", WalletRole::System).unwrap();

        assert_eq!(ks.get_role("creator_wallet"), Some(WalletRole::Creator));
        assert_eq!(ks.get_role("admin_wallet"), Some(WalletRole::Admin));
        assert_eq!(ks.get_role("user_wallet"), Some(WalletRole::User));
        assert_eq!(ks.get_role("system_wallet"), Some(WalletRole::System));
        assert_eq!(ks.get_role("nonexistent"), None);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let (ks, _dir) = test_keystore();
        ks.create_wallet("roundtrip_user", WalletRole::User).unwrap();

        let keypair = ks.get_keypair("roundtrip_user").unwrap();
        let pubkey_from_keypair = keypair.pubkey().to_string();
        let pubkey_from_file = ks.get_public_key("roundtrip_user").unwrap();
        assert_eq!(pubkey_from_keypair, pubkey_from_file);
    }

    #[test]
    fn test_wrong_secret_fails_decryption() {
        let dir = TempDir::new().unwrap();
        let wallet_path = dir.path().join("wallets");

        let ks1 = Keystore::new_with_secret(&wallet_path, "correct_secret_must_be_long!!!".into());
        ks1.create_wallet("victim", WalletRole::User).unwrap();

        let ks2 = Keystore::new_with_secret(&wallet_path, "wrong_secret_trying_to_hack!!".into());
        let result = ks2.get_keypair("victim");
        assert!(result.is_err(), "Decryption should fail with wrong secret");
    }

    #[test]
    fn test_list_wallets() {
        let (ks, _dir) = test_keystore();
        ks.create_wallet("alice", WalletRole::User).unwrap();
        ks.create_wallet("bob", WalletRole::User).unwrap();
        ks.create_wallet("apis_system", WalletRole::System).unwrap();

        let mut wallets = ks.list_wallets();
        wallets.sort();
        assert_eq!(wallets, vec!["alice", "apis_system", "bob"]);
    }

    #[test]
    fn test_directory_traversal_sanitisation() {
        let (ks, _dir) = test_keystore();
        let result = ks.create_wallet("../../../etc/passwd", WalletRole::User);
        assert!(result.is_ok());
        assert!(ks.wallet_exists("etcpasswd"));
    }

    #[test]
    fn test_wallet_survives_reload() {
        let dir = TempDir::new().unwrap();
        let wallet_path = dir.path().join("wallets");
        let secret = "persistent_test_secret_long!!!".to_string();

        let ks1 = Keystore::new_with_secret(&wallet_path, secret.clone());
        let pubkey = ks1.create_wallet("persistent_user", WalletRole::User).unwrap();
        drop(ks1);

        let ks2 = Keystore::new_with_secret(&wallet_path, secret);
        assert!(ks2.wallet_exists("persistent_user"));
        assert_eq!(ks2.get_public_key("persistent_user").unwrap(), pubkey);
        let keypair = ks2.get_keypair("persistent_user").unwrap();
        assert_eq!(keypair.pubkey().to_string(), pubkey);
    }
}

#[cfg(test)]
mod token_tests {
    use crate::crypto::token::{Rarity, Rewards, to_base_units, from_base_units};

    #[test]
    fn test_base_unit_conversion() {
        assert_eq!(to_base_units(1.0), 1_000_000);
        assert_eq!(to_base_units(0.5), 500_000);
        assert_eq!(to_base_units(100.0), 100_000_000);
        assert!((from_base_units(1_000_000) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_rarity_from_confidence() {
        assert_eq!(Rarity::from_confidence(1.0), Rarity::Legendary);
        assert_eq!(Rarity::from_confidence(0.95), Rarity::Legendary);
        assert_eq!(Rarity::from_confidence(0.94), Rarity::Rare);
        assert_eq!(Rarity::from_confidence(0.85), Rarity::Rare);
        assert_eq!(Rarity::from_confidence(0.84), Rarity::Uncommon);
        assert_eq!(Rarity::from_confidence(0.70), Rarity::Uncommon);
        assert_eq!(Rarity::from_confidence(0.69), Rarity::Common);
        assert_eq!(Rarity::from_confidence(0.0), Rarity::Common);
    }

    #[test]
    fn test_rarity_display() {
        assert_eq!(format!("{}", Rarity::Legendary), "⭐ Legendary");
        assert_eq!(format!("{}", Rarity::Common), "⚪ Common");
    }

    #[test]
    fn test_reward_amounts_positive() {
        assert!(Rewards::daily_engagement() > 0.0);
        assert!(Rewards::tool_usage() > 0.0);
        assert!(Rewards::autonomy_contribution() > 0.0);
        assert!(Rewards::governance_vote() > 0.0);
        assert!(Rewards::content_contribution() > 0.0);
    }
}

#[cfg(test)]
mod solana_simulation_tests {
    use crate::crypto::keystore::{Keystore, WalletRole};
    use crate::crypto::solana::{HiveSolanaClient, WalletMode};
    use tempfile::TempDir;

    fn setup() -> (Keystore, HiveSolanaClient, TempDir) {
        let dir = TempDir::new().unwrap();
        let ks = Keystore::new_with_secret(
            dir.path().join("wallets"),
            "sim_test_secret_must_be_long!!!".into(),
        );
        let client = HiveSolanaClient::new_with_config(
            WalletMode::Simulation,
            dir.path().join("ledger.json"),
        );
        (ks, client, dir)
    }

    #[test]
    fn test_simulation_mode_default() {
        let (_, client, _dir) = setup();
        assert_eq!(*client.mode(), WalletMode::Simulation);
    }

    #[test]
    fn test_zero_balance_for_new_wallet() {
        let (ks, client, _dir) = setup();
        let pubkey = ks.create_wallet("new_user", WalletRole::User).unwrap();
        let balance = client.get_balance(&pubkey).unwrap();
        assert_eq!(balance.sol, 0.0);
        assert_eq!(balance.hive, 0.0);
    }

    #[test]
    fn test_mint_and_check_balance() {
        let (ks, client, _dir) = setup();
        let _creator_pubkey = ks.create_wallet("creator", WalletRole::Creator).unwrap();
        let user_pubkey = ks.create_wallet("user1", WalletRole::User).unwrap();

        // Mint 100 HIVE to user
        let tx = client.mint_hive(&ks, "creator", &user_pubkey, 100.0).unwrap();
        assert!(tx.starts_with("sim_"));

        // Check balance
        let balance = client.get_balance(&user_pubkey).unwrap();
        assert_eq!(balance.hive, 100.0);

        // Check total supply
        assert_eq!(client.total_supply(), 100.0);
    }

    #[test]
    fn test_transfer_between_wallets() {
        let (ks, client, _dir) = setup();
        let _creator_pubkey = ks.create_wallet("creator", WalletRole::Creator).unwrap();
        let alice_pubkey = ks.create_wallet("alice", WalletRole::User).unwrap();
        let bob_pubkey = ks.create_wallet("bob", WalletRole::User).unwrap();

        // Mint 100 HIVE to Alice
        client.mint_hive(&ks, "creator", &alice_pubkey, 100.0).unwrap();

        // Alice sends 30 to Bob
        let tx = client.transfer_hive(&ks, "alice", &bob_pubkey, 30.0).unwrap();
        assert!(tx.starts_with("sim_"));

        // Check balances
        let alice_bal = client.get_balance(&alice_pubkey).unwrap();
        let bob_bal = client.get_balance(&bob_pubkey).unwrap();
        assert_eq!(alice_bal.hive, 70.0);
        assert_eq!(bob_bal.hive, 30.0);
    }

    #[test]
    fn test_overdraft_rejected() {
        let (ks, client, _dir) = setup();
        let _creator_pubkey = ks.create_wallet("creator", WalletRole::Creator).unwrap();
        let alice_pubkey = ks.create_wallet("alice", WalletRole::User).unwrap();
        let bob_pubkey = ks.create_wallet("bob", WalletRole::User).unwrap();

        // Alice has 10 HIVE
        client.mint_hive(&ks, "creator", &alice_pubkey, 10.0).unwrap();

        // Try to send 50
        let result = client.transfer_hive(&ks, "alice", &bob_pubkey, 50.0);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Insufficient balance"));

        // Balance unchanged
        let alice_bal = client.get_balance(&alice_pubkey).unwrap();
        assert_eq!(alice_bal.hive, 10.0);
    }

    #[test]
    fn test_only_creator_can_mint() {
        let (ks, client, _dir) = setup();
        ks.create_wallet("creator", WalletRole::Creator).unwrap();
        let user_pubkey = ks.create_wallet("regular_user", WalletRole::User).unwrap();
        ks.create_wallet("admin", WalletRole::Admin).unwrap();

        // User cannot mint
        let result = client.mint_hive(&ks, "regular_user", &user_pubkey, 1000.0);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Only the creator"));

        // Admin cannot mint
        let result = client.mint_hive(&ks, "admin", &user_pubkey, 1000.0);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Only the creator"));

        // Creator can mint
        let result = client.mint_hive(&ks, "creator", &user_pubkey, 50.0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_negative_transfer_rejected() {
        let (ks, client, _dir) = setup();
        let creator_pubkey = ks.create_wallet("creator", WalletRole::Creator).unwrap();
        let alice_pubkey = ks.create_wallet("alice", WalletRole::User).unwrap();

        client.mint_hive(&ks, "creator", &alice_pubkey, 100.0).unwrap();

        let result = client.transfer_hive(&ks, "alice", &creator_pubkey, -50.0);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("positive"));
    }

    #[test]
    fn test_transaction_history() {
        let (ks, client, _dir) = setup();
        let _creator_pubkey = ks.create_wallet("creator", WalletRole::Creator).unwrap();
        let user_pubkey = ks.create_wallet("user", WalletRole::User).unwrap();

        client.mint_hive(&ks, "creator", &user_pubkey, 100.0).unwrap();
        client.mint_hive(&ks, "creator", &user_pubkey, 50.0).unwrap();

        let history = client.get_transaction_history(&user_pubkey, 10).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].tx_type, "hive_mint"); // Most recent first
    }

    #[test]
    fn test_airdrop_simulation() {
        let (ks, client, _dir) = setup();
        let pubkey = ks.create_wallet("airdrop_user", WalletRole::User).unwrap();

        client.request_airdrop(&pubkey, 1.0).unwrap();

        let balance = client.get_balance(&pubkey).unwrap();
        assert_eq!(balance.sol, 1.0);
    }

    #[test]
    fn test_ledger_persistence() {
        let dir = TempDir::new().unwrap();
        let ledger_path = dir.path().join("ledger.json");
        let ks = Keystore::new_with_secret(
            dir.path().join("wallets"),
            "persist_test_secret_long_enough!".into(),
        );

        // Create and mint
        let _creator_pubkey = ks.create_wallet("creator", WalletRole::Creator).unwrap();
        let user_pubkey = ks.create_wallet("user", WalletRole::User).unwrap();
        {
            let client = HiveSolanaClient::new_with_config(
                WalletMode::Simulation,
                ledger_path.clone(),
            );
            client.mint_hive(&ks, "creator", &user_pubkey, 42.0).unwrap();
        }

        // Reload and verify
        let client2 = HiveSolanaClient::new_with_config(
            WalletMode::Simulation,
            ledger_path,
        );
        let balance = client2.get_balance(&user_pubkey).unwrap();
        assert_eq!(balance.hive, 42.0);
        assert_eq!(client2.total_supply(), 42.0);
    }
}
