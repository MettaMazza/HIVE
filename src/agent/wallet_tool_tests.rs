//! Tests for the wallet tool — admin gate, create, balance, send, dedup, history, mint.

#[cfg(test)]
mod wallet_tool_tests {
    use std::sync::Arc;
    use tempfile::TempDir;
    use crate::crypto::keystore::{Keystore, WalletRole};
    use crate::crypto::solana::{HiveSolanaClient, WalletMode};
    use crate::models::scope::Scope;
    use crate::models::capabilities::AgentCapabilities;

    fn setup() -> (Arc<Keystore>, Arc<HiveSolanaClient>, TempDir) {
        let dir = TempDir::new().unwrap();
        let ks = Arc::new(Keystore::new_with_secret(
            dir.path().join("wallets"),
            "wallet_tool_test_secret_long!!!".into(),
        ));
        let client = Arc::new(HiveSolanaClient::new_with_config(
            WalletMode::Simulation,
            dir.path().join("ledger.json"),
        ));
        (ks, client, dir)
    }

    fn admin_scope(user_id: &str) -> Scope {
        Scope::Public {
            user_id: user_id.to_string(),
            channel_id: "test_channel".into(),
        }
    }

    fn admin_caps(admin_id: &str) -> Option<Arc<AgentCapabilities>> {
        let mut caps = AgentCapabilities::default();
        caps.admin_users = vec![admin_id.to_string()];
        Some(Arc::new(caps))
    }

    #[tokio::test]
    async fn test_non_admin_blocked() {
        let (ks, client, _dir) = setup();
        let scope = admin_scope("random_user");
        let caps = admin_caps("actual_admin");

        let result = crate::agent::wallet_tool::execute_wallet(
            "test1".into(), "action:[create]".into(),
            &scope, ks, client, caps, None,
        ).await;

        assert!(matches!(result.status, crate::models::tool::ToolStatus::Failed(_)));
        assert!(result.output.contains("administrators only"));
    }

    #[tokio::test]
    async fn test_admin_can_create_wallet() {
        let (ks, client, _dir) = setup();
        let scope = admin_scope("admin_user");
        let caps = admin_caps("admin_user");

        let result = crate::agent::wallet_tool::execute_wallet(
            "test2".into(), "action:[create]".into(),
            &scope, ks.clone(), client, caps, None,
        ).await;

        assert!(matches!(result.status, crate::models::tool::ToolStatus::Success));
        assert!(result.output.contains("Wallet created"));
        assert!(ks.wallet_exists("admin_user"));
    }

    #[tokio::test]
    async fn test_check_balance() {
        let (ks, client, _dir) = setup();
        let scope = admin_scope("admin_user");
        let caps = admin_caps("admin_user");

        // Create wallet first
        ks.create_wallet("admin_user", WalletRole::User).unwrap();

        let result = crate::agent::wallet_tool::execute_wallet(
            "test3".into(), "action:[balance]".into(),
            &scope, ks, client, caps, None,
        ).await;

        assert!(matches!(result.status, crate::models::tool::ToolStatus::Success));
        assert!(result.output.contains("HIVE:"));
    }

    #[tokio::test]
    async fn test_send_requires_to_and_amount() {
        let (ks, client, _dir) = setup();
        let scope = admin_scope("admin_user");
        let caps = admin_caps("admin_user");
        ks.create_wallet("admin_user", WalletRole::User).unwrap();

        // Missing 'to:'
        let result = crate::agent::wallet_tool::execute_wallet(
            "test4".into(), "action:[send] amount:[10]".into(),
            &scope, ks.clone(), client.clone(), caps.clone(), None,
        ).await;
        assert!(matches!(result.status, crate::models::tool::ToolStatus::Failed(_)));

        // Missing 'amount:'
        let result = crate::agent::wallet_tool::execute_wallet(
            "test5".into(), "action:[send] to:[someone]".into(),
            &scope, ks, client, caps, None,
        ).await;
        assert!(matches!(result.status, crate::models::tool::ToolStatus::Failed(_)));
    }

    #[tokio::test]
    async fn test_send_with_balance() {
        let (ks, client, _dir) = setup();
        let scope = admin_scope("admin_user");
        let caps = admin_caps("admin_user");

        // Create wallets
        let sender_pk = ks.create_wallet("admin_user", WalletRole::User).unwrap();
        let receiver_pk = ks.create_wallet("receiver", WalletRole::User).unwrap();

        // Create system wallet for minting
        ks.create_wallet("apis_system", WalletRole::System).unwrap();

        // Mint some HIVE to sender
        client.mint_hive(&ks, "apis_system", &sender_pk, 100.0).unwrap();

        // Send 25 HIVE
        let result = crate::agent::wallet_tool::execute_wallet(
            "test6".into(),
            format!("action:[send] to:[receiver] amount:[25]"),
            &scope, ks.clone(), client.clone(), caps, None,
        ).await;

        assert!(matches!(result.status, crate::models::tool::ToolStatus::Success));
        assert!(result.output.contains("Transfer successful"));

        // Verify balances
        let sender_bal = client.get_balance(&sender_pk).unwrap();
        let receiver_bal = client.get_balance(&receiver_pk).unwrap();
        assert_eq!(sender_bal.hive, 75.0);
        assert_eq!(receiver_bal.hive, 25.0);
    }

    #[tokio::test]
    async fn test_receive_shows_address() {
        let (ks, client, _dir) = setup();
        let scope = admin_scope("admin_user");
        let caps = admin_caps("admin_user");
        let pubkey = ks.create_wallet("admin_user", WalletRole::User).unwrap();

        let result = crate::agent::wallet_tool::execute_wallet(
            "test7".into(), "action:[receive]".into(),
            &scope, ks, client, caps, None,
        ).await;

        assert!(matches!(result.status, crate::models::tool::ToolStatus::Success));
        assert!(result.output.contains(&pubkey));
    }

    #[tokio::test]
    async fn test_system_wallet_can_mint() {
        let (ks, client, _dir) = setup();
        let scope = Scope::Private { user_id: "apis_autonomy".into() };

        ks.create_wallet("apis_system", WalletRole::System).unwrap();
        let user_pk = ks.create_wallet("test_user", WalletRole::User).unwrap();

        let result = crate::agent::wallet_tool::execute_wallet(
            "test8".into(),
            format!("action:[mint] to:[test_user] amount:[50]"),
            &scope, ks, client.clone(), None, None,
        ).await;

        assert!(matches!(result.status, crate::models::tool::ToolStatus::Success));
        assert!(result.output.contains("Minted"));

        let bal = client.get_balance(&user_pk).unwrap();
        assert_eq!(bal.hive, 50.0);
    }
}
