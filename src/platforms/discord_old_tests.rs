    use super::*;

    use crate::models::scope::Scope;

    #[tokio::test]
    async fn test_discord_name() {
        let discord = DiscordPlatform::new("".to_string());
        assert_eq!(discord.name(), "discord");
    }



    #[tokio::test]
    async fn test_discord_send_invalid_platform_id() {
        let discord = DiscordPlatform::new("".to_string());
        let res = Response {
            platform: "discord".to_string(),
            target_scope: Scope::Public { channel_id: "123".to_string(), user_id: "user".to_string() },
            text: "Public test".to_string(),
            is_telemetry: false,
        };
        let err = discord.send(res).await;
        assert!(matches!(err, Err(PlatformError::Other(_))));
    }

    #[tokio::test]
    async fn test_discord_send_uninitialized_http() {
        let discord = DiscordPlatform::new("".to_string());
        let res = Response {
            platform: "discord:1234:5678".to_string(),
            target_scope: Scope::Public { channel_id: "123".to_string(), user_id: "user".to_string() },
            text: "Public test".to_string(),
            is_telemetry: false,
        };
        let err = discord.send(res).await;
        assert!(matches!(err, Err(PlatformError::Other(_))));
    }
