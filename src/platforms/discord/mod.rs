#![allow(clippy::collapsible_if)]
pub mod interaction;
pub mod message;

use async_trait::async_trait;
use tokio::sync::mpsc::Sender;
use serenity::prelude::*;
use serenity::model::channel::Message;
use serenity::model::application::Interaction;
use std::sync::Arc;

use crate::models::message::{Event, Response};
use crate::platforms::{Platform, PlatformError};

pub struct Handler {
    pub(crate) event_sender: Sender<Event>,
    pub(crate) bot_user_id: Mutex<Option<serenity::model::id::UserId>>,
    pub(crate) active_telemetry: Arc<Mutex<std::collections::HashMap<u64, tokio::sync::watch::Sender<Option<String>>>>>,
    pub(crate) tts_cache: Arc<Mutex<std::collections::HashMap<u64, String>>>,
    pub(crate) continue_responses: Arc<Mutex<std::collections::HashMap<u64, tokio::sync::oneshot::Sender<bool>>>>,
    pub(crate) is_tending: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) aicoms_enabled: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) memory: Arc<crate::memory::MemoryStore>,
}

#[async_trait]
#[cfg(not(tarpaulin_include))]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: serenity::model::gateway::Ready) {
        tracing::info!("[Discord] Connected as {}", ready.user.name);
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
            
        let command_tending = serenity::builder::CreateCommand::new("tending")
            .description("ADMIN ONLY: Toggles Tending Mode (blocks DMs)")
            .default_member_permissions(serenity::model::Permissions::ADMINISTRATOR);

        let command_proxy = serenity::builder::CreateCommand::new("proxy")
            .description("ADMIN ONLY: Proxy a message through the bot to a specific channel")
            .add_option(serenity::builder::CreateCommandOption::new(
                serenity::model::application::CommandOptionType::String,
                "channel_id",
                "The target Channel ID"
            ).required(true))
            .add_option(serenity::builder::CreateCommandOption::new(
                serenity::model::application::CommandOptionType::String,
                "message",
                "The message content"
            ).required(true))
            .default_member_permissions(serenity::model::Permissions::ADMINISTRATOR);
        
        let _ = serenity::model::application::Command::create_global_command(&ctx.http, command_clean).await;
        let _ = serenity::model::application::Command::create_global_command(&ctx.http, command_clear).await;
        let _ = serenity::model::application::Command::create_global_command(&ctx.http, command_sweep).await;
        let _ = serenity::model::application::Command::create_global_command(&ctx.http, command_tending).await;
        let _ = serenity::model::application::Command::create_global_command(&ctx.http, command_proxy).await;
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        interaction::handle_interaction(self, ctx, interaction).await;
    }

    async fn message(&self, ctx: Context, msg: Message) {
        message::handle_message(self, ctx, msg).await;
    }
}

pub struct DiscordPlatform {
    token: String,
    http: Mutex<Option<Arc<serenity::http::Http>>>,
    active_telemetry: Arc<Mutex<std::collections::HashMap<u64, tokio::sync::watch::Sender<Option<String>>>>>,
    tts_cache: Arc<Mutex<std::collections::HashMap<u64, String>>>,
    continue_responses: Arc<Mutex<std::collections::HashMap<u64, tokio::sync::oneshot::Sender<bool>>>>,
    is_tending: Arc<std::sync::atomic::AtomicBool>,
    aicoms_enabled: Arc<std::sync::atomic::AtomicBool>,
    memory: Arc<crate::memory::MemoryStore>,
}

impl DiscordPlatform {
    pub fn new(token: String, memory: Arc<crate::memory::MemoryStore>) -> Self {
        Self { 
            token,
            http: Mutex::new(None),
            active_telemetry: Arc::new(Mutex::new(std::collections::HashMap::new())),
            tts_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
            continue_responses: Arc::new(Mutex::new(std::collections::HashMap::new())),
            is_tending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            aicoms_enabled: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            memory,
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
            is_tending: self.is_tending.clone(),
            aicoms_enabled: self.aicoms_enabled.clone(),
            memory: self.memory.clone(),
        };

        let mut client = Client::builder(&self.token, intents)
            .event_handler(handler)
            .await
            .map_err(|e| PlatformError::Other(e.to_string()))?;

        let http = client.http.clone();
        *self.http.lock().await = Some(http);

        tokio::spawn(async move {
            if let Err(why) = client.start().await {
                tracing::error!("[Discord] Client error: {:?}", why);
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
                let map_size = map.len();
                if let Some(tx) = map.get(&thinking_msg_id) {
                    let text_preview: String = response.text.chars().take(80).collect();
                    tracing::debug!("[TELEMETRY:DISCORD] 📨 Updating embed msg_id={} (map_size={}, text='{}')", thinking_msg_id, map_size, text_preview);
                    let _ = tx.send(Some(response.text.clone()));
                } else {
                    tracing::warn!("[TELEMETRY:DISCORD] ⚠️ msg_id={} NOT FOUND in active_telemetry map (map_size={}, keys={:?})", thinking_msg_id, map_size, map.keys().collect::<Vec<_>>());
                }
            } else if response.platform != "discord:1480192647657427044" {
                tracing::warn!("[TELEMETRY:DISCORD] ⚠️ thinking_msg_id=0 — cannot route telemetry (platform='{}')", response.platform);
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
                let attachments = crate::platforms::attachments::extract_attachments(&mut parsed_text).await;

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
                    Err(e) => tracing::error!("[Discord Platform] Error sending response chunk: {:?}", e),
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
        
        tracing::info!("[Discord] ✅ Added reaction {} to message {} in channel {}", emoji, message_id, channel_id);
        Ok(())
    }

    #[cfg(not(tarpaulin_include))]
    async fn ask_continue(&self, channel_id: u64, turn: usize) -> bool {
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

                tracing::info!("[CHECKPOINT] 🐝 Sent continue prompt at turn {} (msg {})", turn, msg_id);

                // Wait for user response with 5 minute timeout
                match tokio::time::timeout(tokio::time::Duration::from_secs(300), rx).await {
                    Ok(Ok(response)) => {
                        tracing::info!("[CHECKPOINT] User responded: {}", if response { "continue" } else { "wrap up" });
                        response
                    }
                    _ => {
                        // Timeout or channel error — default to wrapping up
                        tracing::warn!("[CHECKPOINT] ⏰ Timed out waiting for user. Wrapping up.");
                        let edit = serenity::builder::EditMessage::new()
                            .content(format!("🐝 **Checkpoint — Turn {}** — ⏰ No response, wrapping up.", turn))
                            .components(vec![]);
                        let _ = channel.edit_message(&http, serenity::model::id::MessageId::new(msg_id), edit).await;
                        false
                    }
                }
            }
            Err(e) => {
                tracing::error!("[CHECKPOINT] Failed to send continue prompt: {}", e);
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
        let discord = DiscordPlatform::new("".to_string(), Arc::new(crate::memory::MemoryStore::default()));
        assert_eq!(discord.name(), "discord");
    }

    #[tokio::test]
    async fn test_discord_send_invalid_platform_id() {
        let discord = DiscordPlatform::new("".to_string(), Arc::new(crate::memory::MemoryStore::default()));
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
        let discord = DiscordPlatform::new("".to_string(), Arc::new(crate::memory::MemoryStore::default()));
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
