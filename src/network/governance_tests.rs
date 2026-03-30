    use super::*;

    fn peer(id: &str) -> PeerId { PeerId(id.to_string()) }

    #[tokio::test]
    async fn test_ban_proposal_creation() {
        let gov = GovernanceEngine::new();
        let id = gov.propose_ban(peer("bad_actor"), "Spamming", "hash123", peer("reporter")).await;
        assert!(!id.is_empty());
        assert_eq!(gov.active_proposals().await.len(), 1);
    }

    #[tokio::test]
    async fn test_ban_voting_supermajority() {
        let gov = GovernanceEngine::new();
        let id = gov.propose_ban(peer("target"), "Abuse", "hash", peer("p1")).await;

        // 4 out of 5 vote for ban (80% = supermajority)
        gov.vote(&id, peer("v1"), true, 5).await.unwrap();
        gov.vote(&id, peer("v2"), true, 5).await.unwrap();
        gov.vote(&id, peer("v3"), true, 5).await.unwrap();
        let outcome = gov.vote(&id, peer("v4"), true, 5).await.unwrap();

        assert_eq!(outcome, Some(BanOutcome::Banned));
        assert!(gov.is_banned(&peer("target")).await);
    }

    #[tokio::test]
    async fn test_ban_voting_acquittal() {
        let gov = GovernanceEngine::new();
        let id = gov.propose_ban(peer("target"), "False accusation", "hash", peer("p1")).await;

        gov.vote(&id, peer("v1"), false, 5).await.unwrap();
        gov.vote(&id, peer("v2"), false, 5).await.unwrap();
        let outcome = gov.vote(&id, peer("v3"), false, 5).await.unwrap();

        assert_eq!(outcome, Some(BanOutcome::Acquitted));
        assert!(!gov.is_banned(&peer("target")).await);
    }

    #[tokio::test]
    async fn test_double_vote_rejected() {
        let gov = GovernanceEngine::new();
        let id = gov.propose_ban(peer("target"), "reason", "hash", peer("p1")).await;

        gov.vote(&id, peer("v1"), true, 5).await.unwrap();
        let result = gov.vote(&id, peer("v1"), true, 5).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_emergency_alert() {
        let gov = GovernanceEngine::new();
        let id = gov.issue_alert(
            AlertSeverity::Critical,
            CrisisCategory::ConnectivityLost,
            "ISP backbone failure detected in region EU-West",
            peer("monitor_node"),
        ).await;

        gov.acknowledge_alert(&id, peer("ack_peer")).await;

        let alerts = gov.recent_alerts(10).await;
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].acknowledged_by.len(), 1);
    }

    #[tokio::test]
    async fn test_resource_directory() {
        let gov = GovernanceEngine::new();

        gov.advertise_resource(peer("relay_node"), ResourceType::InternetRelay, "100Mbps fiber").await;
        gov.advertise_resource(peer("storage_node"), ResourceType::Storage, "500GB available").await;

        let relays = gov.find_resources(&ResourceType::InternetRelay).await;
        assert_eq!(relays.len(), 1);
        assert_eq!(relays[0].capacity, "100Mbps fiber");
    }

    #[tokio::test]
    async fn test_osint_submission_and_confirmation() {
        let gov = GovernanceEngine::new();

        let id = gov.submit_osint("blocked_ips", "192.168.1.100 - compromised relay", peer("reporter")).await;

        // Two peers confirm
        gov.confirm_osint(&id, peer("confirmer_1")).await;
        gov.confirm_osint(&id, peer("confirmer_2")).await;

        let reports = gov.osint_by_category("blocked_ips").await;
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].confirmations.len(), 2);
        assert!(reports[0].confidence > 0.5);
    }

    #[tokio::test]
    async fn test_osint_confidence_increases() {
        let mut entry = OSINTEntry::new("test", "data", peer("issuer"));
        assert_eq!(entry.confidence, 0.5);

        entry.confirm(peer("c1"));
        assert!(entry.confidence > 0.5);

        entry.confirm(peer("c2"));
        entry.confirm(peer("c3"));
        assert!(entry.confidence > 0.7);
    }

    #[tokio::test]
    async fn test_governance_stats() {
        let gov = GovernanceEngine::new();
        gov.propose_ban(peer("t1"), "r", "h", peer("p")).await;
        gov.issue_alert(AlertSeverity::Info, CrisisCategory::ResourceAvailable, "test", peer("n")).await;

        let stats = gov.stats().await;
        assert_eq!(stats["active_proposals"], 1);
        assert_eq!(stats["alerts"], 1);
    }
