    use super::*;

    #[test]
    fn test_decode_message_ignore_self() {
        let json = r#"{
            "id": "1",
            "channel_id": "2",
            "author": { "id": "999", "username": "bot", "discriminator": "0000", "avatar": null, "bot": true },
            "content": "hello",
            "timestamp": "2024-01-01T00:00:00Z",
            "edited_timestamp": null,
            "tts": false,
            "mention_everyone": false,
            "mentions": [],
            "mention_roles": [],
            "attachments": [],
            "embeds": [],
            "pinned": false,
            "type": 0
        }"#;

        if let Ok(msg) = serde_json::from_str::<Message>(json) {
            let caps = crate::models::capabilities::AgentCapabilities::default();
            let action = decode_message(&msg, Some(serenity::model::id::UserId::new(999)), false, &caps);
            assert_eq!(action, MessageAction::IgnoreSelf);
        }
    }

    #[test]
    fn test_decode_message_new_session() {
        let json = r#"{
            "id": "1",
            "channel_id": "2",
            "author": { "id": "456", "username": "user", "discriminator": "0000", "avatar": null },
            "content": "/new",
            "timestamp": "2024-01-01T00:00:00Z",
            "edited_timestamp": null,
            "tts": false,
            "mention_everyone": false,
            "mentions": [],
            "mention_roles": [],
            "attachments": [],
            "embeds": [],
            "pinned": false,
            "type": 0
        }"#;

        if let Ok(msg) = serde_json::from_str::<Message>(json) {
            let mut caps = crate::models::capabilities::AgentCapabilities::default();
            caps.admin_users.push("456".into()); // Make user admin so they can use /new
            let action = decode_message(&msg, Some(serenity::model::id::UserId::new(999)), false, &caps);
            if let MessageAction::NewSession { user_id, user_name, channel_id, guild_id } = action {
                assert_eq!(user_id, 456);
                assert_eq!(user_name, "user");
                assert_eq!(channel_id, 2);
                assert_eq!(guild_id, None);
            } else {
                panic!("Expected NewSession action");
            }
        }
    }

    #[test]
    fn test_decode_message_dm_restricted() {
        let json = r#"{
            "id": "1",
            "channel_id": "2",
            "author": { "id": "789", "username": "stranger", "discriminator": "0000", "avatar": null },
            "content": "Hello Apis",
            "timestamp": "2024-01-01T00:00:00Z",
            "edited_timestamp": null,
            "tts": false,
            "mention_everyone": false,
            "mentions": [],
            "mention_roles": [],
            "attachments": [],
            "embeds": [],
            "pinned": false,
            "type": 0
        }"#;

        if let Ok(msg) = serde_json::from_str::<Message>(json) {
            let caps = crate::models::capabilities::AgentCapabilities::default(); // No admins
            let action = decode_message(&msg, Some(serenity::model::id::UserId::new(999)), false, &caps);
            assert_eq!(action, MessageAction::DmRestricted);
        }
    }
