#![allow(clippy::collapsible_if)]
use async_trait::async_trait;
use tokio::sync::mpsc::Sender;
use serenity::prelude::*;
use serenity::model::channel::Message;
use serenity::model::application::Interaction;
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};
use std::sync::Arc;

use crate::models::message::{Event, Response};
use crate::models::scope::Scope;
use super::{Platform, PlatformError};

struct Handler {
    event_sender: Sender<Event>,
    bot_user_id: Mutex<Option<serenity::model::id::UserId>>,
    active_telemetry: Arc<Mutex<std::collections::HashMap<u64, tokio::sync::watch::Sender<Option<String>>>>>,
    tts_cache: Arc<Mutex<std::collections::HashMap<u64, String>>>,
    continue_responses: Arc<Mutex<std::collections::HashMap<u64, tokio::sync::oneshot::Sender<bool>>>>,
}

#[async_trait]
#[cfg(not(tarpaulin_include))]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: serenity::model::gateway::Ready) {
        println!("[Discord] Connected as {}", ready.user.name);
        let mut id_lock = self.bot_user_id.lock().await;
        *id_lock = Some(ready.user.id);

        // Register Global Slash Commands
        let command_clean = serenity::builder::CreateCommand::new("clean")
            .description("ADMIN ONLY: Wipes all AI Memory (Factory Reset)")
            .default_member_permissions(serenity::model::Permissions::ADMINISTRATOR);
        let command_clear = serenity::builder::CreateCommand::new("clear")
            .description("ADMIN ONLY: Wipes all AI Memory (Factory Reset)")
            .default_member_permissions(serenity::model::Permissions::ADMINISTRATOR);
        let command_sweep = serenity::builder::CreateCommand::new("sweep")
            .description("ADMIN ONLY: Delete all messages in this channel")
            .default_member_permissions(serenity::model::Permissions::ADMINISTRATOR);
        
        let _ = serenity::model::application::Command::create_global_command(&ctx.http, command_clean).await;
        let _ = serenity::model::application::Command::create_global_command(&ctx.http, command_clear).await;
        let _ = serenity::model::application::Command::create_global_command(&ctx.http, command_sweep).await;
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = &interaction {
            if command.data.name.as_str() == "clean" || command.data.name.as_str() == "clear" {
                // Instantly reply so the Discord interaction doesn't fail
                let data = CreateInteractionResponseMessage::new()
                    .content("```\n⏳ Initiating Factory Wipe...\n```")
                    .ephemeral(true); // Only the admin sees this
                let builder = CreateInteractionResponse::Message(data);
                if let Err(why) = command.create_response(&ctx.http, builder).await {
                    eprintln!("Cannot respond to slash command: {why}");
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

                let _ = self.event_sender.send(ev).await;
            } else if command.data.name.as_str() == "sweep" {
                let admin_list: Vec<String> = std::env::var("HIVE_ADMIN_USERS").unwrap_or_default().split(',').map(|s| s.trim().to_string()).collect();
                if !admin_list.contains(&command.user.id.get().to_string()) {
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
                    eprintln!("Cannot respond to slash command: {why}");
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
                            if let Err(_) = channel_id.delete_messages(&http, &bulk).await {
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
                        let cache = self.tts_cache.lock().await;
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
                let mut map = self.continue_responses.lock().await;
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
                return;
            }
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        // Ignore self
        let is_self = {
            let id_lock = self.bot_user_id.lock().await;
            id_lock.map(|id| id == msg.author.id).unwrap_or(false)
        };
        if is_self || msg.author.bot {
            return;
        }

        // Intercept text-based /sweep command (since slash commands take an hour to sync)
        if msg.content.trim() == "/sweep" {
            // Admin-only command
            let sweep_admins: Vec<String> = std::env::var("HIVE_ADMIN_USERS").unwrap_or_default().split(',').map(|s| s.trim().to_string()).collect();
            if sweep_admins.contains(&msg.author.id.get().to_string()) {
                let _ = msg.react(&ctx.http, serenity::model::channel::ReactionType::Unicode("🧹".to_string())).await;
                
                let channel_id = msg.channel_id;
            let http = ctx.http.clone();

            tokio::spawn(async move {
                let fourteen_days_ago = chrono::Utc::now() - chrono::Duration::days(14);
                loop {
                    let messages = match channel_id.messages(&http, serenity::builder::GetMessages::new().limit(100)).await {
                        Ok(msgs) => msgs,
                        Err(_) => break,
                    };

                    if messages.is_empty() { break; }

                    let (bulk, single): (Vec<_>, Vec<_>) = messages.into_iter().partition(|m| m.timestamp.with_timezone(&chrono::Utc) > fourteen_days_ago);

                    if !bulk.is_empty() {
                        if let Err(_) = channel_id.delete_messages(&http, &bulk).await {
                            for msg in bulk {
                                let _ = msg.delete(&http).await;
                            }
                        }
                    }

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
                    
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            });
            // ALWAYS return so the LLM doesn't process it
            return;
        }
        }
        
        let is_dm = msg.guild_id.is_none();
        let target_channel: u64 = std::env::var("HIVE_TARGET_CHANNEL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let is_target_channel = msg.channel_id.get() == target_channel;
        
        // Determine if we should listen.
        // Listen if: it's a DM, it's the target channel, or we are explicitly mentioned.
        let is_mentioned = {
            let id_lock = self.bot_user_id.lock().await;
            if let Some(bot_id) = *id_lock {
                msg.mentions_user_id(bot_id)
            } else {
                false
            }
        };

        if !is_dm && !is_target_channel && !is_mentioned {
            return;
        }

        let scope = if is_dm {
            Scope::Private { user_id: msg.author.id.get().to_string() }
        } else {
            Scope::Public { channel_id: msg.channel_id.get().to_string(), user_id: msg.author.id.get().to_string() }
        };

        // Create cognition tracker embed (ErnOS CognitionTracker pattern)
        let embed = serenity::builder::CreateEmbed::new()
            .description("```\n⏳ Processing...\n```")
            .footer(serenity::builder::CreateEmbedFooter::new("🐝 Analyzing..."))
            .color(0x5865F2);
            
        let builder = serenity::builder::CreateMessage::new().reference_message(&msg).embed(embed);
        let thinking_msg_id = if let Ok(sent_msg) = msg.channel_id.send_message(&ctx.http, builder).await {
            let msg_id_u64 = sent_msg.id.get();
            let (tx, rx) = tokio::sync::watch::channel(Some("⏳ Processing...".to_string()));
            {
                let mut map = self.active_telemetry.lock().await;
                map.insert(msg_id_u64, tx);
            }
            let http_clone = ctx.http.clone();
            let channel_id_clone = msg.channel_id;

            super::telemetry::spawn_telemetry_loop(http_clone, channel_id_clone, msg_id_u64, rx);

            Some(msg_id_u64.to_string())
        } else {
            None
        };

        // Attach platform metadata containing the channel, thinking msg, and source user msg.
        let platform_id = format!("discord:{}:{}:{}", msg.channel_id.get(), thinking_msg_id.unwrap_or_default(), msg.id.get());

        // Capture attachment metadata only — no downloads, no disk writes.
        // Apis can use the `read_attachment` tool to fetch content on-demand (in-memory).
        let mut enriched_content = msg.content.clone();
        if !msg.attachments.is_empty() {
            for att in &msg.attachments {
                let content_type = att.content_type.clone().unwrap_or_else(|| "unknown".to_string());
                enriched_content.push_str(&format!(
                    "\n\n[USER_ATTACHMENT: {} | type: {} | size: {} bytes | url: {}]",
                    att.filename, content_type, att.size, att.url
                ));
            }
        }

        let ev = Event {
            platform: platform_id,
            scope,
            author_name: msg.author.name.clone(),
            author_id: msg.author.id.get().to_string(),
            content: enriched_content,
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };

        let _ = self.event_sender.send(ev).await;
    }
}

pub struct DiscordPlatform {
    token: String,
    http: Mutex<Option<Arc<serenity::http::Http>>>,
    active_telemetry: Arc<Mutex<std::collections::HashMap<u64, tokio::sync::watch::Sender<Option<String>>>>>,
    tts_cache: Arc<Mutex<std::collections::HashMap<u64, String>>>,
    continue_responses: Arc<Mutex<std::collections::HashMap<u64, tokio::sync::oneshot::Sender<bool>>>>,
}

impl DiscordPlatform {
    pub fn new(token: String) -> Self {
        Self { 
            token,
            http: Mutex::new(None),
            active_telemetry: Arc::new(Mutex::new(std::collections::HashMap::new())),
            tts_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
            continue_responses: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }
}

#[async_trait]
impl Platform for DiscordPlatform {
    fn name(&self) -> &str {
        "discord"
    }
    #[cfg(not(tarpaulin_include))]
    async fn start(&self, event_sender: Sender<Event>) -> Result<(), PlatformError> {
        let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::DIRECT_MESSAGES | GatewayIntents::MESSAGE_CONTENT;
        
        let handler = Handler {
            event_sender,
            bot_user_id: Mutex::new(None),
            active_telemetry: self.active_telemetry.clone(),
            tts_cache: self.tts_cache.clone(),
            continue_responses: self.continue_responses.clone(),
        };

        let mut client = Client::builder(&self.token, intents)
            .event_handler(handler)
            .await
            .map_err(|e| PlatformError::Other(e.to_string()))?;

        let http = client.http.clone();
        *self.http.lock().await = Some(http);

        tokio::spawn(async move {
            if let Err(why) = client.start().await {
                eprintln!("[Discord] Client error: {:?}", why);
            }
        });

        Ok(())
    }

    #[cfg(not(tarpaulin_include))]
    async fn send(&self, response: Response) -> Result<(), PlatformError> {
        // Parse the platform string: discord:channel_id:msg_id
        let parts: Vec<&str> = response.platform.split(':').collect();
        if parts.len() < 2 {
            return Err(PlatformError::Other("Invalid discord platform routing ID".into()));
        }

        let channel_id: u64 = parts[1].parse().unwrap_or(0);
        let thinking_msg_id: u64 = if parts.len() >= 3 { parts[2].parse().unwrap_or(0) } else { 0 };

        let http_lock = self.http.lock().await;
        let http = http_lock.as_ref().ok_or(PlatformError::Other("Discord HTTP client not initialized".into()))?;

        let channel = serenity::model::id::ChannelId::new(channel_id);

        if response.is_telemetry {
            if thinking_msg_id > 0 {
                let map = self.active_telemetry.lock().await;
                if let Some(tx) = map.get(&thinking_msg_id) {
                    let _ = tx.send(Some(response.text.clone()));
                }
            }
        } else {
            // Discord limits messages to 2000 characters. We must chunk the final response.
            let chars: Vec<char> = response.text.chars().collect();
            let mut chunks = Vec::new();
            let mut current_pos = 0;

            while current_pos < chars.len() {
                let end_pos = std::cmp::min(current_pos + 1950, chars.len()); // 1950 to be safe
                let chunk: String = chars[current_pos..end_pos].iter().collect();
                chunks.push(chunk);
                current_pos = end_pos;
            }

            let mut final_msg_id = None;
            for (i, chunk_text) in chunks.iter().enumerate() {
                let mut parsed_text = chunk_text.to_string();
                let attachments = super::attachments::extract_attachments(&mut parsed_text).await;

                let mut builder = serenity::builder::CreateMessage::new().content(parsed_text.trim());
                for att in attachments {
                    builder = builder.add_file(att);
                }
                
                // Add the TTS button exclusively to the final message chunk
                if i == chunks.len() - 1 && !response.is_telemetry {
                    let btn = serenity::builder::CreateButton::new("tts_generate")
                        .label("🔊 Speak")
                        .style(serenity::model::application::ButtonStyle::Secondary);
                    let row = serenity::builder::CreateActionRow::Buttons(vec![btn]);
                    builder = builder.components(vec![row]);
                }

                match channel.send_message(http, builder).await {
                    Ok(msg) => {
                        if i == chunks.len() - 1 {
                            final_msg_id = Some(msg.id.get());
                        }
                    }
                    Err(e) => eprintln!("[Discord Platform] Error sending response chunk: {:?}", e),
                }
            }

            if let Some(msg_id) = final_msg_id {
                let mut cache = self.tts_cache.lock().await;
                cache.insert(msg_id, response.text.clone());
            }
        }
        
        // Always trigger the native typing indicator on any engine frame
        // This ensures the bot looks busy even during silent validation loops
        let _ = channel.broadcast_typing(http).await;

        Ok(())
    }

    #[cfg(not(tarpaulin_include))]
    async fn react(&self, channel_id: u64, message_id: u64, emoji: &str) -> Result<(), PlatformError> {
        let http_lock = self.http.lock().await;
        let http = http_lock.as_ref().ok_or(PlatformError::Other("Discord HTTP client not initialized".into()))?;
        
        let channel = serenity::model::id::ChannelId::new(channel_id);
        let msg_id = serenity::model::id::MessageId::new(message_id);
        let reaction = serenity::model::channel::ReactionType::Unicode(emoji.to_string());
        
        channel.create_reaction(http, msg_id, reaction)
            .await
            .map_err(|e| PlatformError::Other(format!("Failed to add reaction: {}", e)))?;
        
        println!("[Discord] ✅ Added reaction {} to message {} in channel {}", emoji, message_id, channel_id);
        Ok(())
    }

    #[cfg(not(tarpaulin_include))]
    async fn ask_continue(&self, channel_id: u64, turn: usize, _user_id: &str) -> bool {
        let http_lock = self.http.lock().await;
        let http = match http_lock.as_ref() {
            Some(h) => h.clone(),
            None => return true,
        };
        drop(http_lock);

        let channel = serenity::model::id::ChannelId::new(channel_id);

        let yes_btn = serenity::builder::CreateButton::new("continue_yes")
            .label("✅ Continue")
            .style(serenity::model::application::ButtonStyle::Success);
        let no_btn = serenity::builder::CreateButton::new("continue_no")
            .label("🛑 Wrap Up")
            .style(serenity::model::application::ButtonStyle::Danger);
        let row = serenity::builder::CreateActionRow::Buttons(vec![yes_btn, no_btn]);

        let builder = serenity::builder::CreateMessage::new()
            .content(format!("🐝 **Checkpoint — Turn {}**\nI've been working for {} turns. Should I keep going, or wrap up and respond with what I have?", turn, turn))
            .components(vec![row]);

        match channel.send_message(&http, builder).await {
            Ok(sent) => {
                let msg_id = sent.id.get();
                let (tx, rx) = tokio::sync::oneshot::channel();

                {
                    let mut map = self.continue_responses.lock().await;
                    map.insert(msg_id, tx);
                }

                println!("[CHECKPOINT] 🐝 Sent continue prompt at turn {} (msg {})", turn, msg_id);

                // Wait for user response with 5 minute timeout
                match tokio::time::timeout(tokio::time::Duration::from_secs(300), rx).await {
                    Ok(Ok(response)) => {
                        println!("[CHECKPOINT] User responded: {}", if response { "continue" } else { "wrap up" });
                        response
                    }
                    _ => {
                        // Timeout or channel error — default to wrapping up
                        println!("[CHECKPOINT] ⏰ Timed out waiting for user. Wrapping up.");
                        let edit = serenity::builder::EditMessage::new()
                            .content(format!("🐝 **Checkpoint — Turn {}** — ⏰ No response, wrapping up.", turn))
                            .components(vec![]);
                        let _ = channel.edit_message(&http, serenity::model::id::MessageId::new(msg_id), edit).await;
                        false
                    }
                }
            }
            Err(e) => {
                eprintln!("[CHECKPOINT] Failed to send continue prompt: {}", e);
                true // Can't ask, just continue
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
}
