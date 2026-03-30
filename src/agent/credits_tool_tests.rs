//! Tests for the credits_tool module.

use super::*;
use std::path::PathBuf;
use std::sync::Arc;
use crate::crypto::credits::CreditsEngine;
use crate::models::scope::Scope;
use crate::models::capabilities::AgentCapabilities;

fn test_credits_engine() -> (Arc<CreditsEngine>, PathBuf) {
    let path = std::env::temp_dir()
        .join(format!("hive_credits_tool_test_{}", uuid::Uuid::new_v4()));
    let engine = CreditsEngine::new_with_path(path.clone());
    (Arc::new(engine), path)
}

fn cleanup(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn test_balance_action() {
    let (engine, path) = test_credits_engine();
    let scope = Scope::Private {
        user_id: "test_user".to_string(),
    };

    let result = execute_credits(
        "task_1".to_string(),
        "action:[balance]".to_string(),
        &scope,
        engine.clone(),
        None,
        None,
    )
    .await;

    assert_eq!(result.status, ToolStatus::Success);
    assert!(result.output.contains("Credit Balance"), "output was: {}", result.output);
    assert!(result.output.contains("test_user"), "output was: {}", result.output);
    cleanup(&path);
}

#[tokio::test]
async fn test_history_action() {
    let (engine, path) = test_credits_engine();
    let scope = Scope::Private {
        user_id: "test_user".to_string(),
    };

    // First create an account and make some transactions
    engine.get_or_create_account("test_user");
    let _ = engine.earn_compute("test_user", 1000, 1.0);

    let result = execute_credits(
        "task_2".to_string(),
        "action:[history] limit:[10]".to_string(),
        &scope,
        engine.clone(),
        None,
        None,
    )
    .await;

    assert_eq!(result.status, ToolStatus::Success);
    assert!(result.output.contains("Credit History"), "output was: {}", result.output);
    assert!(result.output.contains("test_user"), "output was: {}", result.output);
    cleanup(&path);
}

#[tokio::test]
async fn test_earn_requires_admin() {
    let (engine, path) = test_credits_engine();
    let scope = Scope::Private {
        user_id: "normal_user".to_string(),
    };

    // Create capabilities without admin user
    let mut capabilities = AgentCapabilities::default();
    capabilities.admin_users.push("admin_only".to_string());

    let result = execute_credits(
        "task_3".to_string(),
        "action:[earn] amount:[50] source:[test]".to_string(),
        &scope,
        engine.clone(),
        Some(Arc::new(capabilities)),
        None,
    )
    .await;

    assert!(matches!(result.status, ToolStatus::Failed(_)));
    assert!(result.output.contains("restricted to administrators"), "output was: {}", result.output);
    cleanup(&path);
}

#[tokio::test]
async fn test_earn_action_admin() {
    let (engine, path) = test_credits_engine();
    let scope = Scope::Private {
        user_id: "admin_user".to_string(),
    };

    // Create capabilities with admin user
    let mut capabilities = AgentCapabilities::default();
    capabilities.admin_users.push("admin_user".to_string());

    let result = execute_credits(
        "task_4".to_string(),
        "action:[earn] amount:[50] source:[test]".to_string(),
        &scope,
        engine.clone(),
        Some(Arc::new(capabilities)),
        None,
    )
    .await;

    assert_eq!(result.status, ToolStatus::Success);
    assert!(result.output.contains("Credits awarded successfully"), "output was: {}", result.output);
    assert!(result.output.contains("50.00"), "output was: {}", result.output);
    cleanup(&path);
}

#[tokio::test]
async fn test_spend_action() {
    let (engine, path) = test_credits_engine();
    let scope = Scope::Private {
        user_id: "spender".to_string(),
    };

    // Create account with welcome bonus
    engine.get_or_create_account("spender");

    let result = execute_credits(
        "task_5".to_string(),
        "action:[spend] amount:[25] service:[marketplace]".to_string(),
        &scope,
        engine.clone(),
        None,
        None,
    )
    .await;

    assert_eq!(result.status, ToolStatus::Success);
    assert!(result.output.contains("Transaction successful"), "output was: {}", result.output);
    assert!(result.output.contains("25.00"), "output was: {}", result.output);
    assert!(result.output.contains("marketplace"), "output was: {}", result.output);
    cleanup(&path);
}

#[tokio::test]
async fn test_spend_insufficient_balance() {
    let (engine, path) = test_credits_engine();
    let scope = Scope::Private {
        user_id: "poor_user".to_string(),
    };

    // Create account
    engine.get_or_create_account("poor_user");

    let result = execute_credits(
        "task_6".to_string(),
        "action:[spend] amount:[500] service:[expensive]".to_string(),
        &scope,
        engine.clone(),
        None,
        None,
    )
    .await;

    assert!(matches!(result.status, ToolStatus::Failed(_)));
    assert!(result.output.contains("failed") || result.output.contains("Failed"), "output was: {}", result.output);
    cleanup(&path);
}

#[tokio::test]
async fn test_leaderboard_action() {
    let (engine, path) = test_credits_engine();

    // Create accounts and some earnings
    engine.get_or_create_account("user_a");
    engine.get_or_create_account("user_b");
    engine.earn_compute("user_a", 10000, 1.0).unwrap();
    engine.earn_compute("user_b", 1000, 1.0).unwrap();

    let result = execute_credits(
        "task_7".to_string(),
        "action:[leaderboard] limit:[10]".to_string(),
        &Scope::Public {
            user_id: "viewer".to_string(),
            channel_id: "test".to_string(),
        },
        engine.clone(),
        None,
        None,
    )
    .await;

    assert_eq!(result.status, ToolStatus::Success);
    assert!(result.output.contains("Leaderboard"), "output was: {}", result.output);
    assert!(result.output.contains("user_a"), "output was: {}", result.output);
    cleanup(&path);
}

#[tokio::test]
async fn test_stats_action() {
    let (engine, path) = test_credits_engine();

    // Create some accounts and transactions
    engine.get_or_create_account("stats_user");
    let _ = engine.earn_compute("stats_user", 5000, 1.0);

    let result = execute_credits(
        "task_8".to_string(),
        "action:[stats]".to_string(),
        &Scope::Public {
            user_id: "viewer".to_string(),
            channel_id: "test".to_string(),
        },
        engine.clone(),
        None,
        None,
    )
    .await;

    assert_eq!(result.status, ToolStatus::Success);
    assert!(result.output.contains("Statistics"), "output was: {}", result.output);
    assert!(result.output.contains("Total Accounts"), "output was: {}", result.output);
    assert!(result.output.contains("Configuration"), "output was: {}", result.output);
    cleanup(&path);
}

#[tokio::test]
async fn test_reputation_action() {
    let (engine, path) = test_credits_engine();
    let scope = Scope::Private {
        user_id: "rep_user".to_string(),
    };

    // Create account and build some reputation
    engine.get_or_create_account("rep_user");
    let _ = engine.record_community_vote("rep_user", "voter_1", true);

    let result = execute_credits(
        "task_9".to_string(),
        "action:[reputation]".to_string(),
        &scope,
        engine.clone(),
        None,
        None,
    )
    .await;

    assert_eq!(result.status, ToolStatus::Success);
    assert!(result.output.contains("Reputation Score"), "output was: {}", result.output);
    assert!(result.output.contains("rep_user"), "output was: {}", result.output);
    cleanup(&path);
}

#[tokio::test]
async fn test_invalid_action() {
    let (engine, path) = test_credits_engine();
    let scope = Scope::Private {
        user_id: "test_user".to_string(),
    };

    let result = execute_credits(
        "task_10".to_string(),
        "action:[invalid_action]".to_string(),
        &scope,
        engine.clone(),
        None,
        None,
    )
    .await;

    assert!(matches!(result.status, ToolStatus::Failed(_)));
    assert!(result.output.contains("Unknown credits action"));
    cleanup(&path);
}

#[tokio::test]
async fn test_public_scope() {
    let (engine, path) = test_credits_engine();
    let scope = Scope::Public {
        user_id: "public_user".to_string(),
        channel_id: "test".to_string(),
    };

    let result = execute_credits(
        "task_11".to_string(),
        "action:[balance]".to_string(),
        &scope,
        engine.clone(),
        None,
        None,
    )
    .await;

    assert_eq!(result.status, ToolStatus::Success);
    assert!(result.output.contains("public_user"));
    cleanup(&path);
}

#[tokio::test]
async fn test_earn_with_invalid_amount() {
    let (engine, path) = test_credits_engine();
    let scope = Scope::Private {
        user_id: "admin_user".to_string(),
    };

    // Create capabilities with admin user
    let mut capabilities = AgentCapabilities::default();
    capabilities.admin_users.push("admin_user".to_string());

    let result = execute_credits(
        "task_12".to_string(),
        "action:[earn] amount:[invalid] source:[test]".to_string(),
        &scope,
        engine.clone(),
        Some(Arc::new(capabilities)),
        None,
    )
    .await;

    assert!(matches!(result.status, ToolStatus::Failed(_)));
    assert!(result.output.contains("amount"));
    cleanup(&path);
}

#[tokio::test]
async fn test_history_with_limit() {
    let (engine, path) = test_credits_engine();
    let scope = Scope::Private {
        user_id: "history_user".to_string(),
    };

    // Create account and multiple transactions
    engine.get_or_create_account("history_user");
    for i in 0..5 {
        let _ = engine.earn_compute("history_user", (i + 1) * 1000, 1.0);
    }

    let result = execute_credits(
        "task_13".to_string(),
        "action:[history] limit:[3]".to_string(),
        &scope,
        engine.clone(),
        None,
        None,
    )
    .await;

    assert_eq!(result.status, ToolStatus::Success);
    // The output includes the limit value
    assert!(result.output.contains("limit 3") || result.output.contains("History"), "output was: {}", result.output);
    cleanup(&path);
}

#[tokio::test]
async fn test_leaderboard_empty() {
    let (engine, path) = test_credits_engine();

    let result = execute_credits(
        "task_14".to_string(),
        "action:[leaderboard]".to_string(),
        &Scope::Public {
            user_id: "viewer".to_string(),
            channel_id: "test".to_string(),
        },
        engine.clone(),
        None,
        None,
    )
    .await;

    assert_eq!(result.status, ToolStatus::Success);
    assert!(result.output.contains("No credit earners yet"), "output was: {}", result.output);
    cleanup(&path);
}

#[tokio::test]
async fn test_system_user_cannot_bypass_admin() {
    let (engine, path) = test_credits_engine();
    let scope = Scope::Private {
        user_id: "apis_system".to_string(),
    };

    // SECURITY: System users must NOT be able to earn without admin capability.
    // In an open-source codebase, "apis_system" is just a string — any admin
    // could instruct the agent to act as apis_system to bypass restrictions.
    let result = execute_credits(
        "task_15".to_string(),
        "action:[earn] amount:[100] source:[system]".to_string(),
        &scope,
        engine.clone(),
        None,  // No capabilities = no admin
        None,
    )
    .await;

    assert!(matches!(result.status, ToolStatus::Failed(_)), "System user should NOT bypass admin gate");
    assert!(result.output.contains("restricted to administrators"), "output was: {}", result.output);
    cleanup(&path);
}
