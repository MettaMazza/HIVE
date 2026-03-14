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
            .description("ADMIN ONLY: Wipes all AI Memory (Factory Reset)");
        let command_clear = serenity::builder::CreateCommand::new("clear")
            .description("ADMIN ONLY: Wipes all AI Memory (Factory Reset)");
        
        let _ = serenity::model::application::Command::create_global_command(&ctx.http, command_clean).await;
        let _ = serenity::model::application::Command::create_global_command(&ctx.http, command_clear).await;
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = interaction {
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
                };

                let _ = self.event_sender.send(ev).await;
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

        let is_dm = msg.guild_id.is_none();
        let target_channel: u64 = 1479744132904915125;
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
            .color(0x5865F2);
            
        let builder = serenity::builder::CreateMessage::new().reference_message(&msg).embed(embed);
        let thinking_msg_id = if let Ok(sent_msg) = msg.channel_id.send_message(&ctx.http, builder).await {
            Some(sent_msg.id.get().to_string())
        } else {
            None
        };

        // Attach platform metadata containing the channel & thinking msg.
        let platform_id = format!("discord:{}:{}", msg.channel_id.get(), thinking_msg_id.unwrap_or_default());

        let ev = Event {
            platform: platform_id,
            scope,
            author_name: msg.author.name.clone(),
            author_id: msg.author.id.get().to_string(),
            content: msg.content.clone(),
        };

        let _ = self.event_sender.send(ev).await;
    }
}

pub struct DiscordPlatform {
    token: String,
    http: Mutex<Option<Arc<serenity::http::Http>>>,
}

impl DiscordPlatform {
    pub fn new(token: String) -> Self {
        Self { 
            token,
            http: Mutex::new(None)
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
        let thinking_msg_id: u64 = if parts.len() == 3 { parts[2].parse().unwrap_or(0) } else { 0 };

        let http_lock = self.http.lock().await;
        let http = http_lock.as_ref().ok_or(PlatformError::Other("Discord HTTP client not initialized".into()))?;

        let channel = serenity::model::id::ChannelId::new(channel_id);

        if response.is_telemetry {
            // TELEMETRY: Edit the cognition tracker embed in-place (ErnOS CognitionTracker pattern)
            if thinking_msg_id > 0 {
                // Determine color: blurple for processing, green for complete
                let is_complete = response.text.starts_with("✅");
                let color = if is_complete { 0x57F287u32 } else { 0x5865F2u32 };

                let embed = serenity::builder::CreateEmbed::new()
                    .description(format!("```\n{}\n```", response.text))
                    .color(color);
                let edit_builder = serenity::builder::EditMessage::new().embed(embed);
                let _ = channel
                    .edit_message(http, serenity::model::id::MessageId::new(thinking_msg_id), edit_builder)
                    .await;
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

            for chunk_text in chunks {
                let builder = serenity::builder::CreateMessage::new().content(chunk_text);
                if let Err(e) = channel.send_message(http, builder).await {
                    eprintln!("[Discord Platform] Error sending response chunk: {:?}", e);
                }
            }
        }
        
        // Always trigger the native typing indicator on any engine frame
        // This ensures the bot looks busy even during silent validation loops
        let _ = channel.broadcast_typing(http).await;

        Ok(())
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
