    use super::*;

    fn peer(id: &str) -> PeerId { PeerId(id.to_string()) }

    fn test_relay(id: &str, latency: u64) -> RelayPeer {
        RelayPeer {
            peer_id: peer(id),
            latency_ms: latency,
            requests_served: 0,
            last_seen: chrono::Utc::now().to_rfc3339(),
            available: true,
        }
    }

    #[test]
    fn test_web_pool_round_robin() {
        let mut pool = WebConnectionPool::new();
        pool.update_relay(test_relay("relay_a", 10));
        pool.update_relay(test_relay("relay_b", 20));
        pool.update_relay(test_relay("relay_c", 30));

        let r1 = pool.pick_relay("user_1").unwrap();
        let r2 = pool.pick_relay("user_1").unwrap();
        let r3 = pool.pick_relay("user_1").unwrap();

        // Round-robin should cycle through all three
        assert_ne!(r1, r2);
        assert_ne!(r2, r3);
    }

    #[test]
    fn test_web_pool_no_relays() {
        let mut pool = WebConnectionPool::new();
        let result = pool.pick_relay("user_1");
        assert!(result.is_err());
    }

    #[test]
    fn test_web_pool_rate_limiting() {
        let mut pool = WebConnectionPool {
            available_relays: vec![test_relay("relay_a", 10)],
            request_log: VecDeque::new(),
            max_requests_per_hour: 3,
            next_relay_idx: 0,
        };

        assert!(pool.pick_relay("spammer").is_ok());
        assert!(pool.pick_relay("spammer").is_ok());
        assert!(pool.pick_relay("spammer").is_ok());
        assert!(pool.pick_relay("spammer").is_err()); // Rate limited
    }

    #[test]
    fn test_compute_pool_heartbeat() {
        let mut pool = ComputePool::new();
        pool.handle_heartbeat(peer("gpu_node"), "qwen3.5:32b".to_string(), 4, 512.0, 0);

        assert_eq!(pool.node_count(), 1);
        assert_eq!(pool.total_slots(), 4);
    }

    #[test]
    fn test_compute_pool_pick_lowest_queue() {
        let mut pool = ComputePool::new();
        pool.handle_heartbeat(peer("busy_node"), "qwen3.5:32b".to_string(), 2, 128.0, 5);
        pool.handle_heartbeat(peer("idle_node"), "qwen3.5:32b".to_string(), 4, 512.0, 0);

        let selected = pool.pick_compute("qwen3.5:32b", "requester").unwrap();
        assert_eq!(selected, peer("idle_node")); // Lowest queue depth wins
    }

    #[test]
    fn test_compute_pool_no_slots() {
        let mut pool = ComputePool::new();
        pool.handle_heartbeat(peer("full_node"), "qwen3.5:32b".to_string(), 0, 128.0, 5);

        let result = pool.pick_compute("qwen3.5:32b", "requester");
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_job_lifecycle() {
        let mut pool = ComputePool::new();
        pool.handle_heartbeat(peer("gpu"), "qwen3.5:32b".to_string(), 2, 512.0, 0);

        pool.start_job("job_1", peer("gpu"), peer("eph_user"), "qwen3.5:32b");
        assert_eq!(pool.available_nodes[0].available_slots, 1); // Slot consumed

        pool.complete_job("job_1", 500);
        assert_eq!(pool.available_nodes[0].available_slots, 2); // Slot freed
        assert_eq!(pool.available_nodes[0].tokens_served, 500);
    }

    #[test]
    fn test_ephemeral_id_unique() {
        let a = PoolManager::ephemeral_id();
        let b = PoolManager::ephemeral_id();
        assert_ne!(a, b);
        assert!(a.0.starts_with("eph_"));
    }

    #[test]
    fn test_pool_defaults_enabled() {
        // Both should be enabled by default (equality)
        let pool = PoolManager::new(peer("local"));
        assert!(pool.web_share_enabled);
        assert!(pool.compute_share_enabled);
    }

    #[tokio::test]
    async fn test_pool_stats() {
        let pool = PoolManager::new(peer("local"));
        let stats = pool.stats().await;
        assert_eq!(stats["web_share_enabled"], true);
        assert_eq!(stats["compute_share_enabled"], true);
        assert_eq!(stats["web_relays_available"], 0);
    }

    #[test]
    fn test_relay_update_existing() {
        let mut pool = WebConnectionPool::new();
        pool.update_relay(test_relay("relay_a", 10));
        pool.update_relay(RelayPeer {
            peer_id: peer("relay_a"),
            latency_ms: 50,
            requests_served: 0,
            last_seen: chrono::Utc::now().to_rfc3339(),
            available: true,
        });

        assert_eq!(pool.available_relays.len(), 1);
        assert_eq!(pool.available_relays[0].latency_ms, 50);
    }

    #[test]
    fn test_compute_remove_node() {
        let mut pool = ComputePool::new();
        pool.handle_heartbeat(peer("node_a"), "model".to_string(), 4, 64.0, 0);
        pool.handle_heartbeat(peer("node_b"), "model".to_string(), 2, 32.0, 0);
        assert_eq!(pool.node_count(), 2);

        pool.remove_node(&peer("node_a"));
        assert_eq!(pool.node_count(), 1);
    }

    #[test]
    fn test_equality_both_enabled_passes() {
        let pool = PoolManager::new(peer("contributor"));
        assert!(pool.verify_equality());
    }

    #[test]
    fn test_equality_both_disabled_fails() {
        let mut pool = PoolManager::new(peer("freeloader"));
        pool.web_share_enabled = false;
        pool.compute_share_enabled = false;
        assert!(!pool.verify_equality()); // DENIED — no freeloading
    }

    #[test]
    fn test_equality_one_enabled_passes() {
        let mut pool = PoolManager::new(peer("partial"));
        pool.web_share_enabled = false;
        pool.compute_share_enabled = true;
        assert!(pool.verify_equality()); // OK — contributing compute

        pool.web_share_enabled = true;
        pool.compute_share_enabled = false;
        assert!(pool.verify_equality()); // OK — contributing web relay
    }

    #[test]
    fn test_pool_integrity_verification() {
        // Should not panic and should return true
        assert!(PoolManager::verify_pool_integrity());
    }

    #[test]
    fn test_token_quota_reset() {
        let mut pool = ComputePool::new();
        pool.token_usage.insert("heavy_user".to_string(), 999999);
        // Force window expiry
        pool.token_window_start = std::time::Instant::now() - std::time::Duration::from_secs(7200);
        pool.reset_if_window_expired();
        assert!(pool.token_usage.is_empty());
    }
