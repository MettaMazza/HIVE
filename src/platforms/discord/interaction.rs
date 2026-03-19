use serenity::prelude::*;
use serenity::model::application::Interaction;
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};
use crate::models::message::Event;
use crate::models::scope::Scope;

pub async fn handle_interaction(handler: &super::Handler, ctx: Context, interaction: Interaction) {
    if let Interaction::Command(command) = &interaction {
        if command.data.name.as_str() == "clean" || command.data.name.as_str() == "clear" {
            // Instantly reply so the Discord interaction doesn't fail
            let data = CreateInteractionResponseMessage::new()
                .content("```\n⏳ Initiating Factory Wipe...\n```")
                .ephemeral(true); // Only the admin sees this
            let builder = CreateInteractionResponse::Message(data);
            if let Err(why) = command.create_response(&ctx.http, builder).await {
                tracing::error!("Cannot respond to slash command: {why}");
            }

            // Push a special hidden command to the core engine.
            // It comes attached to the exact Admin's Discord UID so the Engine RBAC matches it.
            let ev = Event {
                platform: format!("discord:{}:0", command.channel_id.get()),
                scope: Scope::Public { channel_id: command.channel_id.get().to_string(), user_id: command.user.id.get().to_string() }, 
                author_name: command.user.name.clone(),
                author_id: command.user.id.get().to_string(),
                content: "/clean".to_string(), // The hardcoded command the Engine looks for
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
            };

            let _ = handler.event_sender.send(ev).await;
        } else if command.data.name.as_str() == "sweep" {
            // Hardcoded Admin ID Check
            if command.user.id.get() != 1299810741984956449 {
                let data = CreateInteractionResponseMessage::new()
                    .content("❌ You do not have permission to use this command.")
                    .ephemeral(true);
                let builder = CreateInteractionResponse::Message(data);
                let _ = command.create_response(&ctx.http, builder).await;
                return;
            }

            // Instantly reply so the Discord interaction doesn't fail
            let data = CreateInteractionResponseMessage::new()
                .content("```\n🧹 Sweeping channel... This may take a while for older messages.\n```")
                .ephemeral(true); // Only the admin sees this
            let builder = CreateInteractionResponse::Message(data);
            if let Err(why) = command.create_response(&ctx.http, builder).await {
                tracing::error!("Cannot respond to slash command: {why}");
            }

            let channel_id = command.channel_id;
            let http = ctx.http.clone();

            tokio::spawn(async move {
                let fourteen_days_ago = chrono::Utc::now() - chrono::Duration::days(14);
                
                loop {
                    let messages = match channel_id.messages(&http, serenity::builder::GetMessages::new().limit(100)).await {
                        Ok(msgs) => msgs,
                        Err(_) => break,
                    };

                    if messages.is_empty() {
                        break;
                    }

                    let (bulk, single): (Vec<_>, Vec<_>) = messages.into_iter().partition(|m| m.timestamp.with_timezone(&chrono::Utc) > fourteen_days_ago);

                    if !bulk.is_empty() {
                        if channel_id.delete_messages(&http, &bulk).await.is_err() {
                            // Fallback to single if bulk delete fails for some edge case
                            let mut handles = Vec::new();
                            for msg in bulk {
                                let http_clone = http.clone();
                                handles.push(tokio::spawn(async move {
                                    let _ = msg.delete(&http_clone).await;
                                }));
                            }
                            for handle in handles {
                                let _ = handle.await;
                            }
                        }
                    }

                    // Concurrently delete older messages (past 14 days)
                    if !single.is_empty() {
                        let mut handles = Vec::new();
                        for msg in single {
                            let http_clone = http.clone();
                            handles.push(tokio::spawn(async move {
                                let _ = msg.delete(&http_clone).await;
                            }));
                        }
                        for handle in handles {
                            let _ = handle.await;
                        }
                    }
                    
                    // Prevent aggressive rate limit tripping
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            });
        } else if command.data.name.as_str() == "tending" {
            // Hardcoded Admin ID Check
            if command.user.id.get() != 1299810741984956449 {
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
        } else if command.data.name.as_str() == "proxy" {
            // Hardcoded Admin ID Check
            if command.user.id.get() != 1299810741984956449 {
                let data = CreateInteractionResponseMessage::new()
                    .content("❌ You do not have permission to use this command.")
                    .ephemeral(true);
                let builder = CreateInteractionResponse::Message(data);
                let _ = command.create_response(&ctx.http, builder).await;
                return;
            }

            let mut target_channel = String::new();
            let mut message_content = String::new();

            for option in &command.data.options {
                if option.name == "channel_id" {
                    if let serenity::model::application::CommandDataOptionValue::String(val) = &option.value {
                        target_channel = val.clone();
                    }
                } else if option.name == "message" {
                    if let serenity::model::application::CommandDataOptionValue::String(val) = &option.value {
                        message_content = val.clone();
                    }
                }
            }

            if let Ok(cid) = target_channel.parse::<u64>() {
                let channel = serenity::model::id::ChannelId::new(cid);
                let http = ctx.http.clone();
                let msg = message_content.clone();
                tokio::spawn(async move {
                    let _ = channel.send_message(&http, serenity::builder::CreateMessage::new().content(msg)).await;
                });
                
                let data = CreateInteractionResponseMessage::new()
                    .content(format!("✅ Proxied message to <#{}>.", cid))
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

    if let Interaction::Component(component) = &interaction {
        if component.data.custom_id == "tts_generate" {
            // Check if message already has an audio attachment
            let has_audio = component.message.attachments.iter().any(|a| a.filename.ends_with(".wav") || a.filename.ends_with(".mp3"));

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

                let mut text_to_speak = component.message.content.clone();
                {
                    let cache = handler.tts_cache.lock().await;
                    if let Some(full_text) = cache.get(&component.message.id.get()) {
                        text_to_speak = full_text.clone();
                    }
                }

                // Strip markdown ATTACH tags from spoken text
                while let Some(start_idx) = text_to_speak.find("[ATTACH_IMAGE](") {
                    if let Some(end_idx) = text_to_speak[start_idx..].find(")") {
                        let before = &text_to_speak[..start_idx];
                        let after = &text_to_speak[start_idx + end_idx + 1..];
                        text_to_speak = format!("{}{}", before, after);
                    } else {
                        break;
                    }
                }

                let http = ctx.http.clone();
                let mut msg = component.message.clone();

                if text_to_speak.trim().is_empty() {
                    return;
                }

                tokio::spawn(async move {
                    if let Ok(tts) = crate::voice::kokoro::KokoroTTS::new().await {
                        if let Ok(path) = tts.get_audio_path(&text_to_speak).await {
                            if let Ok(attachment) = serenity::builder::CreateAttachment::path(&path).await {
                                let edit = serenity::builder::EditMessage::new()
                                    .attachments(serenity::builder::EditAttachments::new().add(attachment));
                                let _ = msg.edit(&http, edit).await;
                            }
                        } else {
                            // Silent fallback on failure
                        }
                    }
                });
            }
        }
    }

    if let Interaction::Component(component) = &interaction {
        let cid = &component.data.custom_id;
        if cid == "continue_yes" || cid == "continue_no" {
            let user_wants_continue = cid == "continue_yes";
            let btn_label = if user_wants_continue { "✅ Continuing..." } else { "🛑 Wrapping up..." };
            let data = CreateInteractionResponseMessage::new()
                .content(btn_label)
                .ephemeral(true);
            let builder = CreateInteractionResponse::Message(data);
            let _ = component.create_response(&ctx.http, builder).await;

            // Resolve the oneshot
            let msg_id = component.message.id.get();
            let mut map = handler.continue_responses.lock().await;
            if let Some(tx) = map.remove(&msg_id) {
                let _ = tx.send(user_wants_continue);
            }

            // Edit the checkpoint message to show the choice
            let edit_text = if user_wants_continue {
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
}
