use serenity::prelude::*;
use serenity::model::channel::Message;
use crate::models::message::Event;
use crate::models::scope::Scope;

#[derive(Debug, PartialEq)]
pub enum MessageAction {
    IgnoreSelf,
    BufferBot { chunk: String, author_name: String, author_id: u64, channel_id: u64, msg_id: u64 },
    ToggleAiComms { user_id: u64, author_name: String },
    LinkDevice { user_id: u64, user_name: String, code: String },
    Sweep { user_id: u64, channel_id: u64 },
    NewSession { user_id: u64, user_name: String, channel_id: u64, guild_id: Option<u64> },
    TendingBusy,
    DmRestricted,
    Event {
        author_name: String,
        author_id: u64,
        channel_id: u64,
        message_id: u64,
        guild_id: Option<u64>,
    },
    Ignore,
}

pub fn decode_message(msg: &Message, bot_user_id: Option<serenity::model::id::UserId>, is_tending: bool, capabilities: &crate::models::capabilities::AgentCapabilities) -> MessageAction {
    if let Some(bot_id) = bot_user_id {
        if bot_id == msg.author.id {
            return MessageAction::IgnoreSelf;
        }
    }

    if msg.author.bot {
        if msg.content.trim().is_empty() {
            return MessageAction::Ignore;
        }
        return MessageAction::BufferBot {
            chunk: msg.content.clone(),
            author_name: msg.author.name.clone(),
            author_id: msg.author.id.get(),
            channel_id: msg.channel_id.get(),
            msg_id: msg.id.get(),
        };
    }

    if msg.content.trim() == "/aicoms" {
        return MessageAction::ToggleAiComms {
            user_id: msg.author.id.get(),
            author_name: msg.author.name.clone(),
        };
    }

    if let Some(code) = msg.content.trim().strip_prefix("/link ") {
        let code = code.trim();
        if code.len() == 6 && code.chars().all(|c| c.is_ascii_digit()) {
            return MessageAction::LinkDevice {
                user_id: msg.author.id.get(),
                user_name: msg.author.name.clone(),
                code: code.to_string(),
            };
        } else {
            return MessageAction::LinkDevice {
                user_id: msg.author.id.get(),
                user_name: msg.author.name.clone(),
                code: "invalid".to_string(),
            };
        }
    }

    if msg.content.trim() == "/sweep" {
        return MessageAction::Sweep {
            user_id: msg.author.id.get(),
            channel_id: msg.channel_id.get(),
        };
    }

    if msg.content.trim() == "/new" {
        return MessageAction::NewSession {
            user_id: msg.author.id.get(),
            user_name: msg.author.name.clone(),
            channel_id: msg.channel_id.get(),
            guild_id: msg.guild_id.map(|g| g.get()),
        };
    }

    let is_dm = msg.guild_id.is_none();
    let author_id_str = msg.author.id.get().to_string();
    let is_admin = capabilities.admin_users.contains(&author_id_str);

    if is_dm && !is_admin {
        return MessageAction::DmRestricted;
    }

    if is_dm && is_admin && is_tending && !capabilities.admin_users.first().map_or(false, |primary| *primary == author_id_str) {
        return MessageAction::TendingBusy;
    }

    let chat_channel: u64 = std::env::var("HIVE_CHAT_CHANNEL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let is_target_channel = msg.channel_id.get() == chat_channel;
    
    let is_mentioned = if let Some(bot_id) = bot_user_id {
        msg.mentions_user_id(bot_id)
    } else {
        false
    };

    if !is_dm && !is_target_channel && !is_mentioned {
        return MessageAction::Ignore;
    }

    MessageAction::Event {
        author_name: msg.author.name.clone(),
        author_id: msg.author.id.get(),
        channel_id: msg.channel_id.get(),
        message_id: msg.id.get(),
        guild_id: msg.guild_id.map(|g| g.get()),
    }
}

pub async fn handle_message(handler: &super::Handler, ctx: Context, msg: Message) {
    let bot_id = *handler.bot_user_id.lock().await;
    let is_tending = handler.is_tending.load(std::sync::atomic::Ordering::SeqCst);
    let action = decode_message(&msg, bot_id, is_tending, &handler.capabilities);

    match action {
        MessageAction::IgnoreSelf | MessageAction::Ignore => return,
        MessageAction::BufferBot { chunk, author_name, author_id, channel_id, msg_id } => {
            let aicoms_on = handler.aicoms_enabled.load(std::sync::atomic::Ordering::SeqCst);
            if !aicoms_on { return; }

            let debounce_key = format!("{}:{}", channel_id, author_id);
            let generation: u64;

            {
                let mut buffer = handler.bot_debounce.lock().await;
                let entry = buffer.entry(debounce_key.clone()).or_insert_with(|| {
                    super::BotDebounceEntry {
                        chunks: Vec::new(),
                        author_name: author_name.clone(),
                        author_id: author_id.to_string(),
                        channel_id,
                        generation: 0,
                    }
                });
                entry.chunks.push(chunk);
                entry.generation += 1;
                generation = entry.generation;
                tracing::info!("[AICOMS] Buffered chunk {} from bot '{}' (gen {})", entry.chunks.len(), author_name, generation);
            }

            let debounce_buf = handler.bot_debounce.clone();
            let event_sender = handler.event_sender.clone();
            let active_telemetry = handler.active_telemetry.clone();
            let http = ctx.http.clone();

            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

                let flush_data = {
                    let mut buffer = debounce_buf.lock().await;
                    if let Some(entry) = buffer.get(&debounce_key) {
                        if entry.generation == generation {
                            let data = (
                                entry.chunks.join("\n"),
                                entry.author_name.clone(),
                                entry.author_id.clone(),
                                entry.channel_id,
                                entry.chunks.len(),
                            );
                            buffer.remove(&debounce_key);
                            Some(data)
                        } else { None }
                    } else { None }
                };

                if let Some((combined_text, a_name, a_id, chan_id, chunk_count)) = flush_data {
                    tracing::info!("[AICOMS] Flushing {} chunks from bot '{}' as one event ({} chars)", chunk_count, a_name, combined_text.len());

                    let scope = Scope::Public { channel_id: chan_id.to_string(), user_id: a_id.clone() };

                    let embed = serenity::builder::CreateEmbed::new()
                        .description("```\n⏳ Processing bot message...\n```")
                        .footer(serenity::builder::CreateEmbedFooter::new("🤖 Reading bot chunks..."))
                        .color(0x5865F2);

                    let thinking_msg_id = {
                        let builder = serenity::builder::CreateMessage::new().embed(embed);
                        if let Ok(sent_msg) = serenity::model::id::ChannelId::new(chan_id).send_message(&http, builder).await {
                            let msg_id_u64 = sent_msg.id.get();
                            let (tx, rx) = tokio::sync::watch::channel(Some("⏳ Processing bot message...".to_string()));
                            {
                                let mut map = active_telemetry.lock().await;
                                map.insert(msg_id_u64, tx);
                            }
                            crate::platforms::telemetry::spawn_telemetry_loop(http.clone(), serenity::model::id::ChannelId::new(chan_id), msg_id_u64, rx);
                            Some(msg_id_u64.to_string())
                        } else { None }
                    };

                    let platform_id = format!("discord:{}:{}:{}", chan_id, thinking_msg_id.unwrap_or_default(), msg_id);

                    let ev = Event {
                        platform: platform_id,
                        scope,
                        author_name: a_name,
                        author_id: a_id,
                        content: combined_text,
                        timestamp: Some(chrono::Utc::now().to_rfc3339()),
                        message_index: None,
                    };

                    let _ = event_sender.send(ev).await;
                }
            });
        }
        MessageAction::ToggleAiComms { user_id, author_name } => {
            if handler.capabilities.admin_users.contains(&user_id.to_string()) {
                let current = handler.aicoms_enabled.load(std::sync::atomic::Ordering::SeqCst);
                handler.aicoms_enabled.store(!current, std::sync::atomic::Ordering::SeqCst);
                let state_str = if !current { "**enabled** 🤖✅" } else { "**disabled** 🤖❌" };
                let _ = msg.reply(&ctx.http, format!("🤖 AI Comms toggled: Bot-to-bot communication is now {}.", state_str)).await;
                tracing::info!("[AICOMS] Toggled to {} by {}", if !current { "ON" } else { "OFF" }, author_name);
            } else {
                let _ = msg.reply(&ctx.http, "🚫 Only administrators can toggle AI communications.").await;
            }
        }
        MessageAction::LinkDevice { user_id: _, user_name, code } => {
            if code != "invalid" {
                let discord_id = msg.author.id.get().to_string();
                match crate::platforms::glasses::link::claim_link_code(&code, &discord_id, &user_name).await {
                    Ok((device_token, platform_id)) => {
                        let _ = msg.reply(&ctx.http, format!(
                            "🔗 **Glasses linked!** Your glasses session is now connected to your Discord identity. All conversations through your glasses will use your private memory scope.\n\n🔑 Device token: `{}...` (stored on your device for auto-reconnect)",
                            &device_token[..8]
                        )).await;
                        tracing::info!("[LINK] ✅ Discord user {} linked glasses {} via /link command", user_name, platform_id);
                    }
                    Err(reason) => {
                        let _ = msg.reply(&ctx.http, format!("❌ {}", reason)).await;
                    }
                }
            } else {
                let _ = msg.reply(&ctx.http, "❌ Invalid code format. Use `/link <6-digit code>` from your glasses.").await;
            }
        }
        MessageAction::Sweep { user_id, channel_id } => {
            if handler.capabilities.admin_users.contains(&user_id.to_string()) {
                let _ = msg.react(&ctx.http, serenity::model::channel::ReactionType::Unicode("🧹".to_string())).await;
                let c_id = serenity::model::id::ChannelId::new(channel_id);
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
                                for bulk_msg in bulk {
                                    let _ = bulk_msg.delete(&http).await;
                                }
                            }
                        }

                        if !single.is_empty() {
                            let mut handles = Vec::new();
                            for single_msg in single {
                                let http_clone = http.clone();
                                handles.push(tokio::spawn(async move {
                                    let _ = single_msg.delete(&http_clone).await;
                                }));
                            }
                            for handle in handles {
                                let _ = handle.await;
                            }
                        }
                        
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                });
            }
        }
        MessageAction::NewSession { user_id, user_name, channel_id, guild_id } => {
            let _ = msg.react(&ctx.http, serenity::model::channel::ReactionType::Unicode("🔄".to_string())).await;

            let scope = if guild_id.is_none() {
                Scope::Private { user_id: user_id.to_string() }
            } else {
                Scope::Public { channel_id: channel_id.to_string(), user_id: user_id.to_string() }
            };

            let memory = handler.memory.clone();
            let _ = memory.check_and_trigger_autosave(&scope).await;
            memory.working.clear(&scope).await;

            // Send a telemetry "processing" embed so the user sees activity
            let embed = serenity::builder::CreateEmbed::new()
                .description("```\n🔄 New session starting...\n```")
                .footer(serenity::builder::CreateEmbedFooter::new("🐝 Resetting..."))
                .color(0x5865F2);
            let thinking_msg_id = if let Ok(sent_msg) = serenity::model::id::ChannelId::new(channel_id)
                .send_message(&ctx.http, serenity::builder::CreateMessage::new().embed(embed)).await {
                let mid = sent_msg.id.get();
                let (tx, rx) = tokio::sync::watch::channel(Some("🔄 New session starting...".to_string()));
                {
                    let mut map = handler.active_telemetry.lock().await;
                    map.insert(mid, tx);
                }
                crate::platforms::telemetry::spawn_telemetry_loop(
                    ctx.http.clone(),
                    serenity::model::id::ChannelId::new(channel_id),
                    mid, rx,
                );
                Some(mid.to_string())
            } else {
                None
            };

            let platform_id = format!("discord:{}:{}:0", channel_id, thinking_msg_id.unwrap_or_default());

            let continuity_event = Event {
                platform: platform_id,
                scope: scope.clone(),
                author_name: "System".to_string(),
                author_id: "system_welcome".into(),
                content: format!(
                    "*** NEW SESSION ***\n\n\
                    User {} initiated a new session via /new.\n\
                    Previous conversation has been archived to persistent memory.\n\
                    You are now operating in a fresh context window.\n\n\
                    YOU MUST ALWAYS use ALL of these memory tools to recall who they are before greeting:\n\
                    - manage_user_preferences (read their profile)\n\
                    - search_timeline action:[recent] (paginate through recent conversation history)\n\
                    - search_timeline action:[semantic] (semantic recall by meaning)\n\
                    - operate_synaptic_graph (search for stored facts about them)\n\
                    - manage_scratchpad (check for any persistent notes)\n\
                    Then greet them.",
                    user_name
                ),
                timestamp: Some(chrono::Utc::now().to_rfc3339()),
                message_index: None,
            };
            // Add to memory AND send through event_sender so the engine processes it
            memory.add_event(continuity_event.clone()).await;
            let _ = handler.event_sender.send(continuity_event).await;

            let _ = msg.reply(&ctx.http, "🔄 **Session saved and reset.** Starting fresh — Apis is ready for a new conversation.").await;
            tracing::info!("[SESSION] /new triggered by {} — working memory archived and cleared.", user_name);
        }
        MessageAction::TendingBusy => {
            let _ = msg.reply(&ctx.http, "Sorry I'm away right now doing testing but we develop publicly so you can watch in the public channel, I'll be back soon!").await;
        }
        MessageAction::DmRestricted => {
            let target_ch = std::env::var("HIVE_CHAT_CHANNEL").ok().and_then(|v| v.parse::<u64>().ok());
            let channel_msg = if let Some(ch) = target_ch {
                format!("\n\nHowever, you are more than welcome to interact with me in my public channel: <#{}>!", ch)
            } else {
                String::new()
            };
            let response = format!(
                "🐝 **Access Restricted** 🌼\n\nGreetings! I've noticed you're attempting to establish a private uplink. My direct neural pathways are currently reserved for administrative overrides only.{}\n\nPrefer total sovereignty? You can download my entire program and run your own independent HIVE on your own hardware with zero restrictions: https://github.com/MettaMazza/HIVE",
                channel_msg
            );
            let _ = msg.reply(&ctx.http, response).await;
        }
        MessageAction::Event { author_name, author_id, channel_id, message_id, guild_id } => {
            let scope = if guild_id.is_none() {
                Scope::Private { user_id: author_id.to_string() }
            } else {
                Scope::Public { channel_id: channel_id.to_string(), user_id: author_id.to_string() }
            };

            let embed = serenity::builder::CreateEmbed::new()
                .description("```\n⏳ Processing...\n```")
                .footer(serenity::builder::CreateEmbedFooter::new("🐝 Analyzing..."))
                .color(0x5865F2);
                
            let builder = serenity::builder::CreateMessage::new().reference_message(&msg).embed(embed);
            let thinking_msg_id = if let Ok(sent_msg) = serenity::model::id::ChannelId::new(channel_id).send_message(&ctx.http, builder).await {
                let msg_id_u64 = sent_msg.id.get();
                let (tx, rx) = tokio::sync::watch::channel(Some("⏳ Processing...".to_string()));
                {
                    let mut map = handler.active_telemetry.lock().await;
                    map.insert(msg_id_u64, tx);
                }
                let http_clone = ctx.http.clone();

                crate::platforms::telemetry::spawn_telemetry_loop(http_clone, serenity::model::id::ChannelId::new(channel_id), msg_id_u64, rx);

                Some(msg_id_u64.to_string())
            } else {
                None
            };

            let platform_id = format!("discord:{}:{}:{}", channel_id, thinking_msg_id.unwrap_or_default(), message_id);

            let mut enriched_content = msg.content.clone();

            if let Some(ref_msg) = &msg.referenced_message {
                enriched_content.push_str(&format!(
                    "\n\n[REPLY_CONTEXT from {}]:\n{}",
                    ref_msg.author.name, ref_msg.content
                ));
            } else if let Some(reference) = &msg.message_reference {
                if let Some(m_id) = reference.message_id {
                    let c_id = serenity::model::id::ChannelId::new(channel_id);
                    if let Ok(ref_msg) = c_id.message(&ctx.http, m_id).await {
                        enriched_content.push_str(&format!(
                            "\n\n[FORWARDED_CONTEXT from {}]:\n{}",
                            ref_msg.author.name, ref_msg.content
                        ));
                    }
                }
            }

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
                author_name,
                author_id: author_id.to_string(),
                content: enriched_content,
                timestamp: Some(chrono::Utc::now().to_rfc3339()),
                message_index: None,
            };

            let _ = handler.event_sender.send(ev).await;
        }
    }
}


#[cfg(test)]
#[path = "message_tests.rs"]
mod tests;
