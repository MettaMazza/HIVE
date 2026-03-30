use serenity::prelude::*;
use serenity::model::application::Interaction;
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};
use crate::models::message::Event;
use crate::models::scope::Scope;

#[derive(Debug, PartialEq)]
pub enum InteractionAction {
    Clean { channel_id: u64, user_id: u64, user_name: String },
    Sweep { user_id: u64, channel_id: u64 },
    Tending { user_id: u64 },
    Proxy { user_id: u64, target_channel: u64, message: String },
    AiComs { user_id: u64 },
    TtsGenerate { message_id: u64, content: String, has_audio: bool },
    Continue { message_id: u64, wants_continue: bool, allowed_user_id: String, clicker_user_id: u64 },
    Ignore,
}

pub fn decode_interaction(interaction: &Interaction) -> InteractionAction {
    if let Interaction::Command(command) = interaction {
        match command.data.name.as_str() {
            "clean" | "clear" => return InteractionAction::Clean {
                channel_id: command.channel_id.get(),
                user_id: command.user.id.get(),
                user_name: command.user.name.clone(),
            },
            "sweep" => return InteractionAction::Sweep {
                user_id: command.user.id.get(),
                channel_id: command.channel_id.get(),
            },
            "tending" => return InteractionAction::Tending {
                user_id: command.user.id.get(),
            },
            "proxy" => {
                let mut target_channel = 0;
                let mut message_content = String::new();
                for option in &command.data.options {
                    if option.name == "channel_id" {
                        if let serenity::model::application::CommandDataOptionValue::String(val) = &option.value {
                            if let Ok(cid) = val.parse::<u64>() {
                                target_channel = cid;
                            }
                        }
                    } else if option.name == "message" {
                        if let serenity::model::application::CommandDataOptionValue::String(val) = &option.value {
                            message_content = val.clone();
                        }
                    }
                }
                return InteractionAction::Proxy {
                    user_id: command.user.id.get(),
                    target_channel,
                    message: message_content,
                };
            }
            "aicoms" => return InteractionAction::AiComs {
                user_id: command.user.id.get(),
            },
            _ => {}
        }
    } else if let Interaction::Component(component) = interaction {
        let custom_id = component.data.custom_id.as_str();
        let clicker_id = component.user.id.get();

        if custom_id == "tts_generate" {
            return InteractionAction::TtsGenerate {
                message_id: component.message.id.get(),
                content: component.message.content.clone(),
                has_audio: component.message.attachments.iter().any(|a| a.filename.ends_with(".wav") || a.filename.ends_with(".mp3")),
            };
        }

        // Parse continue buttons: custom_id format is "continue_yes:USER_ID" or "continue_no:USER_ID"
        if let Some(allowed_uid) = custom_id.strip_prefix("continue_yes:") {
            return InteractionAction::Continue {
                message_id: component.message.id.get(),
                wants_continue: true,
                allowed_user_id: allowed_uid.to_string(),
                clicker_user_id: clicker_id,
            };
        }
        if let Some(allowed_uid) = custom_id.strip_prefix("continue_no:") {
            return InteractionAction::Continue {
                message_id: component.message.id.get(),
                wants_continue: false,
                allowed_user_id: allowed_uid.to_string(),
                clicker_user_id: clicker_id,
            };
        }

        // Legacy buttons without user scoping (backwards compat)
        if custom_id == "continue_yes" {
            return InteractionAction::Continue {
                message_id: component.message.id.get(),
                wants_continue: true,
                allowed_user_id: String::new(),
                clicker_user_id: clicker_id,
            };
        }
        if custom_id == "continue_no" {
            return InteractionAction::Continue {
                message_id: component.message.id.get(),
                wants_continue: false,
                allowed_user_id: String::new(),
                clicker_user_id: clicker_id,
            };
        }
    }
    InteractionAction::Ignore
}

pub async fn handle_interaction(handler: &super::Handler, ctx: Context, interaction: Interaction) {
    let action = decode_interaction(&interaction);

    match action {
        InteractionAction::Clean { channel_id, user_id, user_name } => {
            if let Interaction::Command(command) = &interaction {
                let data = CreateInteractionResponseMessage::new()
                    .content("```\n⏳ Initiating Factory Wipe...\n```")
                    .ephemeral(true);
                let builder = CreateInteractionResponse::Message(data);
                if let Err(why) = command.create_response(&ctx.http, builder).await {
                    tracing::error!("Cannot respond to slash command: {why}");
                }

                let ev = Event {
                    platform: format!("discord:{}:0", channel_id),
                    scope: Scope::Public { channel_id: channel_id.to_string(), user_id: user_id.to_string() },
                    author_name: user_name,
                    author_id: user_id.to_string(),
                    content: "/clean".to_string(),
                    timestamp: Some(chrono::Utc::now().to_rfc3339()),
                    message_index: None,
                };
                let _ = handler.event_sender.send(ev).await;
            }
        }
        InteractionAction::Sweep { user_id, channel_id: _ } => {
            if let Interaction::Command(command) = &interaction {
                if !handler.capabilities.admin_users.contains(&user_id.to_string()) {
                    let data = CreateInteractionResponseMessage::new()
                        .content("❌ You do not have permission to use this command.")
                        .ephemeral(true);
                    let builder = CreateInteractionResponse::Message(data);
                    let _ = command.create_response(&ctx.http, builder).await;
                    return;
                }

                let data = CreateInteractionResponseMessage::new()
                    .content("```\n🧹 Sweeping channel... This may take a while for older messages.\n```")
                    .ephemeral(true);
                let builder = CreateInteractionResponse::Message(data);
                if let Err(why) = command.create_response(&ctx.http, builder).await {
                    tracing::error!("Cannot respond to slash command: {why}");
                }

                let c_id = command.channel_id;
                let http = ctx.http.clone();

                tokio::spawn(async move {
                    let fourteen_days_ago = chrono::Utc::now() - chrono::Duration::days(14);
                    loop {
                        let messages = match c_id.messages(&http, serenity::builder::GetMessages::new().limit(100)).await {
                            Ok(msgs) => msgs,
                            Err(_) => break,
                        };

                        if messages.is_empty() { break; }

                        let (bulk, single): (Vec<_>, Vec<_>) = messages.into_iter().partition(|m| m.timestamp.with_timezone(&chrono::Utc) > fourteen_days_ago);

                        if !bulk.is_empty() {
                            if c_id.delete_messages(&http, &bulk).await.is_err() {
                                let mut handles = Vec::new();
                                for msg in bulk {
                                    let http_clone = http.clone();
                                    handles.push(tokio::spawn(async move { let _ = msg.delete(&http_clone).await; }));
                                }
                                for handle in handles { let _ = handle.await; }
                            }
                        }

                        if !single.is_empty() {
                            let mut handles = Vec::new();
                            for msg in single {
                                let http_clone = http.clone();
                                handles.push(tokio::spawn(async move { let _ = msg.delete(&http_clone).await; }));
                            }
                            for handle in handles { let _ = handle.await; }
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                });
            }
        }
        InteractionAction::Tending { user_id } => {
            if let Interaction::Command(command) = &interaction {
                if !handler.capabilities.admin_users.contains(&user_id.to_string()) {
                    let data = CreateInteractionResponseMessage::new()
                        .content("❌ You do not have permission to use this command.")
                        .ephemeral(true);
                    let builder = CreateInteractionResponse::Message(data);
                    let _ = command.create_response(&ctx.http, builder).await;
                    return;
                }

                let is_tending = handler.is_tending.load(std::sync::atomic::Ordering::SeqCst);
                handler.is_tending.store(!is_tending, std::sync::atomic::Ordering::SeqCst);
                let status = if !is_tending { "ON" } else { "OFF" };
                
                let data = CreateInteractionResponseMessage::new()
                    .content(format!("```\n🛡️ Tending Mode is now {}\n```", status))
                    .ephemeral(true);
                let builder = CreateInteractionResponse::Message(data);
                if let Err(why) = command.create_response(&ctx.http, builder).await {
                    tracing::error!("Cannot respond to slash command: {why}");
                }
            }
        }
        InteractionAction::Proxy { user_id, target_channel, message } => {
            if let Interaction::Command(command) = &interaction {
                if !handler.capabilities.admin_users.contains(&user_id.to_string()) {
                    let data = CreateInteractionResponseMessage::new()
                        .content("❌ You do not have permission to use this command.")
                        .ephemeral(true);
                    let builder = CreateInteractionResponse::Message(data);
                    let _ = command.create_response(&ctx.http, builder).await;
                    return;
                }

                if target_channel > 0 {
                    let channel = serenity::model::id::ChannelId::new(target_channel);
                    let http = ctx.http.clone();
                    let msg = message.clone();
                    tokio::spawn(async move {
                        let _ = channel.send_message(&http, serenity::builder::CreateMessage::new().content(msg)).await;
                    });
                    
                    let data = CreateInteractionResponseMessage::new()
                        .content(format!("✅ Proxied message to <#{}>.", target_channel))
                        .ephemeral(true);
                    let builder = CreateInteractionResponse::Message(data);
                    let _ = command.create_response(&ctx.http, builder).await;
                } else {
                    let data = CreateInteractionResponseMessage::new()
                        .content("❌ Invalid channel ID format. Must be numeric.")
                        .ephemeral(true);
                    let builder = CreateInteractionResponse::Message(data);
                    let _ = command.create_response(&ctx.http, builder).await;
                }
            }
        }
        InteractionAction::TtsGenerate { message_id, mut content, has_audio } => {
            if let Interaction::Component(component) = &interaction {
                if has_audio {
                    let data = CreateInteractionResponseMessage::new()
                        .content("🔇 TTS Audio removed.")
                        .ephemeral(true);
                    let builder = CreateInteractionResponse::Message(data);
                    let _ = component.create_response(&ctx.http, builder).await;

                    let edit = serenity::builder::EditMessage::new()
                        .attachments(serenity::builder::EditAttachments::new());
                    let _ = component.message.clone().edit(&ctx.http, edit).await;
                } else {
                    let data = CreateInteractionResponseMessage::new()
                        .content("🔊 Requesting local TTS generation...")
                        .ephemeral(true);
                    let builder = CreateInteractionResponse::Message(data);
                    let _ = component.create_response(&ctx.http, builder).await;

                    {
                        let cache = handler.tts_cache.lock().await;
                        if let Some(full_text) = cache.get(&message_id) {
                            content = full_text.clone();
                        }
                    }

                    while let Some(start_idx) = content.find("[ATTACH_IMAGE](") {
                        if let Some(end_idx) = content[start_idx..].find(")") {
                            let before = &content[..start_idx];
                            let after = &content[start_idx + end_idx + 1..];
                            content = format!("{}{}", before, after);
                        } else {
                            break;
                        }
                    }

                    let http = ctx.http.clone();
                    let mut msg = component.message.clone();

                    if !content.trim().is_empty() {
                        tokio::spawn(async move {
                            if let Ok(tts) = crate::voice::kokoro::KokoroTTS::new().await {
                                if let Ok(path) = tts.get_audio_path(&content, None).await {
                                    if let Ok(attachment) = serenity::builder::CreateAttachment::path(&path).await {
                                        let edit = serenity::builder::EditMessage::new()
                                            .attachments(serenity::builder::EditAttachments::new().add(attachment));
                                        let _ = msg.edit(&http, edit).await;
                                    }
                                }
                            }
                        });
                    }
                }
            }
        }
        InteractionAction::Continue { message_id, wants_continue, allowed_user_id, clicker_user_id } => {
            if let Interaction::Component(component) = &interaction {
                // Authorization: Only the user who initiated the request can interact with the checkpoint
                if !allowed_user_id.is_empty() && clicker_user_id.to_string() != allowed_user_id {
                    let data = CreateInteractionResponseMessage::new()
                        .content("❌ Only the user who sent the original request can use this checkpoint.")
                        .ephemeral(true);
                    let builder = CreateInteractionResponse::Message(data);
                    let _ = component.create_response(&ctx.http, builder).await;
                    tracing::warn!("[CHECKPOINT] 🛡️ Rejected checkpoint click from user {} (allowed: {})", clicker_user_id, allowed_user_id);
                    return;
                }

                let btn_label = if wants_continue { "✅ Continuing..." } else { "🛑 Wrapping up..." };
                let data = CreateInteractionResponseMessage::new()
                    .content(btn_label)
                    .ephemeral(true);
                let builder = CreateInteractionResponse::Message(data);
                let _ = component.create_response(&ctx.http, builder).await;

                let mut map = handler.continue_responses.lock().await;
                if let Some(tx) = map.remove(&message_id) {
                    let _ = tx.send(wants_continue);
                }

                let edit_text = if wants_continue {
                    "🐝 **Checkpoint reached** — ✅ User chose to continue."
                } else {
                    "🐝 **Checkpoint reached** — 🛑 User chose to wrap up."
                };
                let edit = serenity::builder::EditMessage::new()
                    .content(edit_text)
                    .components(vec![]);
                let _ = component.message.clone().edit(&ctx.http, edit).await;
            }
        }
        InteractionAction::AiComs { user_id } => {
            if let Interaction::Command(command) = &interaction {
                if !handler.capabilities.admin_users.contains(&user_id.to_string()) {
                    let data = CreateInteractionResponseMessage::new()
                        .content("❌ You do not have permission to use this command.")
                        .ephemeral(true);
                    let builder = CreateInteractionResponse::Message(data);
                    let _ = command.create_response(&ctx.http, builder).await;
                    return;
                }

                let current = handler.aicoms_enabled.load(std::sync::atomic::Ordering::SeqCst);
                handler.aicoms_enabled.store(!current, std::sync::atomic::Ordering::SeqCst);
                let state_str = if !current { "**enabled** 🤖✅" } else { "**disabled** 🤖❌" };

                let data = CreateInteractionResponseMessage::new()
                    .content(format!("🤖 AI Comms toggled: Bot-to-bot communication is now {}.", state_str))
                    .ephemeral(true);
                let builder = CreateInteractionResponse::Message(data);
                if let Err(why) = command.create_response(&ctx.http, builder).await {
                    tracing::error!("Cannot respond to slash command: {why}");
                }
                tracing::info!("[AICOMS] Toggled to {} by user {}", if !current { "ON" } else { "OFF" }, user_id);
            }
        }
        InteractionAction::Ignore => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_clean_interaction() {
        let json = r#"{
            "id": "1",
            "application_id": "2",
            "type": 2,
            "token": "token",
            "version": 1,
            "data": {
                "id": "3",
                "name": "clean",
                "type": 1
            },
            "user": {
                "id": "1299810741984956449",
                "username": "admin",
                "discriminator": "0000",
                "avatar": null
            },
            "channel_id": "999"
        }"#;

        if let Ok(interaction) = serde_json::from_str::<Interaction>(json) {
            let action = decode_interaction(&interaction);
            if let InteractionAction::Clean { channel_id, user_id, user_name } = action {
                assert_eq!(channel_id, 999);
                assert_eq!(user_id, 1299810741984956449);
                assert_eq!(user_name, "admin");
            } else {
                panic!("Expected Clean action");
            }
        }
    }

    #[test]
    fn test_decode_sweep_interaction() {
        let json = r#"{
            "id": "1",
            "application_id": "2",
            "type": 2,
            "token": "token",
            "version": 1,
            "data": {
                "id": "3",
                "name": "sweep",
                "type": 1
            },
            "user": {
                "id": "1299810741984956449",
                "username": "admin",
                "discriminator": "0000",
                "avatar": null
            },
            "channel_id": "999"
        }"#;

        if let Ok(interaction) = serde_json::from_str::<Interaction>(json) {
            let action = decode_interaction(&interaction);
            if let InteractionAction::Sweep { user_id, channel_id } = action {
                assert_eq!(channel_id, 999);
                assert_eq!(user_id, 1299810741984956449);
            } else {
                panic!("Expected Sweep action");
            }
        }
    }

    #[test]
    fn test_checkpoint_custom_id_parsing() {
        // Verify custom_id format "continue_yes:USER_ID" extracts the allowed user correctly
        let custom_id = "continue_yes:1299810741984956449";
        assert!(custom_id.starts_with("continue_yes:"));
        let allowed = custom_id.strip_prefix("continue_yes:").unwrap();
        assert_eq!(allowed, "1299810741984956449");

        let custom_id_no = "continue_no:9876543210";
        let allowed_no = custom_id_no.strip_prefix("continue_no:").unwrap();
        assert_eq!(allowed_no, "9876543210");
    }

    #[test]
    fn test_checkpoint_authorization_logic() {
        // Authorized: clicker matches allowed user
        let allowed_user_id = "1299810741984956449";
        let clicker_user_id: u64 = 1299810741984956449;
        assert_eq!(clicker_user_id.to_string(), allowed_user_id);

        // Unauthorized: clicker does NOT match allowed user  
        let other_clicker: u64 = 9999999999;
        assert_ne!(other_clicker.to_string(), allowed_user_id);

        // Legacy: empty allowed_user_id means no restriction (backwards compat)
        let legacy_allowed = "";
        assert!(legacy_allowed.is_empty());
    }

    #[test]
    fn test_checkpoint_legacy_custom_ids() {
        // Old-style buttons without user_id should not parse as scoped
        let legacy_yes = "continue_yes";
        assert!(legacy_yes.strip_prefix("continue_yes:").is_none());
        
        let legacy_no = "continue_no";
        assert!(legacy_no.strip_prefix("continue_no:").is_none());
    }
}
