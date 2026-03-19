use serenity::prelude::*;
use serenity::model::channel::Message;
use crate::models::message::Event;
use crate::models::scope::Scope;

pub async fn handle_message(handler: &super::Handler, ctx: Context, msg: Message) {
    // Always ignore self
    let is_self = {
        let id_lock = handler.bot_user_id.lock().await;
        id_lock.map(|id| id == msg.author.id).unwrap_or(false)
    };
    if is_self {
        return;
    }

    // Bot message handling: debounce chunks with a 5-second window.
    // When aicoms is enabled, bot text messages are buffered. After 5 seconds of
    // silence from that bot, all chunks are combined into one event.
    if msg.author.bot {
        let aicoms_on = handler.aicoms_enabled.load(std::sync::atomic::Ordering::SeqCst);
        if !aicoms_on {
            return; // aicoms is off — ignore all bot messages
        }
        // Skip embed-only messages (no text content to respond to)
        if msg.content.trim().is_empty() {
            return;
        }

        let debounce_key = format!("{}:{}", msg.channel_id.get(), msg.author.id.get());
        let generation: u64;

        // Buffer the chunk and get the current generation
        {
            let mut buffer = handler.bot_debounce.lock().await;
            let entry = buffer.entry(debounce_key.clone()).or_insert_with(|| {
                super::BotDebounceEntry {
                    chunks: Vec::new(),
                    author_name: msg.author.name.clone(),
                    author_id: msg.author.id.get().to_string(),
                    channel_id: msg.channel_id.get(),
                    generation: 0,
                }
            });
            entry.chunks.push(msg.content.clone());
            entry.generation += 1;
            generation = entry.generation;
            tracing::info!("[AICOMS] Buffered chunk {} from bot '{}' (gen {}): {} chars",
                entry.chunks.len(), msg.author.name, generation, msg.content.len());
        }

        // Spawn a 5-second debounce timer. If no new chunks arrive (generation stays
        // the same), flush all buffered chunks as one combined event.
        let debounce_buf = handler.bot_debounce.clone();
        let event_sender = handler.event_sender.clone();
        let active_telemetry = handler.active_telemetry.clone();
        let http = ctx.http.clone();
        let channel_id = msg.channel_id;
        let msg_id = msg.id;

        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

            // Check if this timer is still current (no newer chunks arrived)
            let flush_data = {
                let mut buffer = debounce_buf.lock().await;
                if let Some(entry) = buffer.get(&debounce_key) {
                    if entry.generation == generation {
                        // This is still the latest timer — flush
                        let data = (
                            entry.chunks.join("\n"),
                            entry.author_name.clone(),
                            entry.author_id.clone(),
                            entry.channel_id,
                            entry.chunks.len(),
                        );
                        buffer.remove(&debounce_key);
                        Some(data)
                    } else {
                        None // A newer chunk arrived — this timer is stale
                    }
                } else {
                    None
                }
            };

            if let Some((combined_text, author_name, author_id, chan_id, chunk_count)) = flush_data {
                tracing::info!("[AICOMS] Flushing {} chunks from bot '{}' as one event ({} chars)",
                    chunk_count, author_name, combined_text.len());

                let scope = Scope::Public {
                    channel_id: chan_id.to_string(),
                    user_id: author_id.clone(),
                };

                // Create cognition tracker for the combined bot message
                let embed = serenity::builder::CreateEmbed::new()
                    .description("```\n⏳ Processing bot message...\n```")
                    .footer(serenity::builder::CreateEmbedFooter::new("🤖 Reading bot chunks..."))
                    .color(0x5865F2);

                let thinking_msg_id = {
                    let builder = serenity::builder::CreateMessage::new().embed(embed);
                    if let Ok(sent_msg) = channel_id.send_message(&http, builder).await {
                        let msg_id_u64 = sent_msg.id.get();
                        let (tx, rx) = tokio::sync::watch::channel(Some("⏳ Processing bot message...".to_string()));
                        {
                            let mut map = active_telemetry.lock().await;
                            map.insert(msg_id_u64, tx);
                        }
                        crate::platforms::telemetry::spawn_telemetry_loop(
                            http.clone(), channel_id, msg_id_u64, rx,
                        );
                        Some(msg_id_u64.to_string())
                    } else {
                        None
                    }
                };

                let platform_id = format!("discord:{}:{}:{}", chan_id, thinking_msg_id.unwrap_or_default(), msg_id.get());

                let ev = Event {
                    platform: platform_id,
                    scope,
                    author_name,
                    author_id,
                    content: combined_text,
                };

                let _ = event_sender.send(ev).await;
            }
        });

        return; // Bot messages are handled asynchronously via the debounce timer
    }

    // /aicoms — Toggle bot-to-bot communication on/off
    if msg.content.trim() == "/aicoms" {
        if msg.author.id.get() == 1299810741984956449 {
            let current = handler.aicoms_enabled.load(std::sync::atomic::Ordering::SeqCst);
            handler.aicoms_enabled.store(!current, std::sync::atomic::Ordering::SeqCst);
            let state_str = if !current { "**enabled** 🤖✅" } else { "**disabled** 🤖❌" };
            let _ = msg.reply(&ctx.http, format!("🤖 AI Comms toggled: Bot-to-bot communication is now {}.", state_str)).await;
            tracing::info!("[AICOMS] Toggled to {} by {}", if !current { "ON" } else { "OFF" }, msg.author.name);
        } else {
            let _ = msg.reply(&ctx.http, "🚫 Only administrators can toggle AI communications.").await;
        }
        return;
    }

    // /link <code> — Link a glasses/app session to this Discord account
    if let Some(code) = msg.content.trim().strip_prefix("/link ") {
        let code = code.trim();
        if code.len() == 6 && code.chars().all(|c| c.is_ascii_digit()) {
            let discord_id = msg.author.id.get().to_string();
            let discord_name = msg.author.name.clone();
            match crate::platforms::glasses::link::claim_link_code(code, &discord_id, &discord_name).await {
                Ok((device_token, platform_id)) => {
                    let _ = msg.reply(&ctx.http, format!(
                        "🔗 **Glasses linked!** Your glasses session is now connected to your Discord identity. All conversations through your glasses will use your private memory scope.\n\n🔑 Device token: `{}...` (stored on your device for auto-reconnect)",
                        &device_token[..8]
                    )).await;
                    tracing::info!("[LINK] ✅ Discord user {} linked glasses {} via /link command", discord_name, platform_id);
                }
                Err(reason) => {
                    let _ = msg.reply(&ctx.http, format!("❌ {}", reason)).await;
                }
            }
        } else {
            let _ = msg.reply(&ctx.http, "❌ Invalid code format. Use `/link <6-digit code>` from your glasses.").await;
        }
        return;
    }

    // Intercept text-based /sweep command (since slash commands take an hour to sync)
    if msg.content.trim() == "/sweep" {
        // Hardcoded Admin ID Check
        if msg.author.id.get() == 1299810741984956449 {
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
                        if channel_id.delete_messages(&http, &bulk).await.is_err() {
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
        }
        // ALWAYS return so the LLM doesn't process it
        return;
    }

    // /new — Available to ALL users. Archives current session and starts fresh.
    if msg.content.trim() == "/new" {
        let _ = msg.react(&ctx.http, serenity::model::channel::ReactionType::Unicode("🔄".to_string())).await;

        let scope = if msg.guild_id.is_none() {
            Scope::Private { user_id: msg.author.id.get().to_string() }
        } else {
            Scope::Public { channel_id: msg.channel_id.get().to_string(), user_id: msg.author.id.get().to_string() }
        };

        // Archive current session to persistent storage
        let memory = handler.memory.clone();
        let _ = memory.check_and_trigger_autosave(&scope).await;
        // Force clear even if under token limit (autosave only fires above limit)
        memory.working.clear(&scope).await;

        // Inject a fresh continuity event so Apis knows a new session started
        let continuity_event = Event {
            platform: "system:session".to_string(),
            scope: scope.clone(),
            author_name: "System".to_string(),
            author_id: "system".into(),
            content: format!(
                "*** NEW SESSION ***\n\n\
                User {} initiated a new session via /new.\n\
                Previous conversation has been archived to persistent memory.\n\
                You are now operating in a fresh context window.\n\
                Greet them warmly and ask what they'd like to work on.",
                msg.author.name
            ),
        };
        memory.add_event(continuity_event).await;

        let _ = msg.reply(&ctx.http, "🔄 **Session saved and reset.** Starting fresh — Apis is ready for a new conversation.").await;
        tracing::info!("[SESSION] /new triggered by {} — working memory archived and cleared.", msg.author.name);
        return;
    }

    let is_dm = msg.guild_id.is_none();

    // Tending Mode Check
    if is_dm && msg.author.id.get() != 1299810741984956449 {
        if handler.is_tending.load(std::sync::atomic::Ordering::SeqCst) {
            let _ = msg.reply(&ctx.http, "Sorry I'm away right now doing testing but we develop publicly so you can watch me and Maria work in the public channel, I'll be back soon!").await;
            return;
        }
    }

    let target_channel: u64 = 1479744132904915125;
    let is_target_channel = msg.channel_id.get() == target_channel;
    
    // Determine if we should listen.
    // Listen if: it's a DM, it's the target channel, or we are explicitly mentioned.
    let is_mentioned = {
        let id_lock = handler.bot_user_id.lock().await;
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
            let mut map = handler.active_telemetry.lock().await;
            map.insert(msg_id_u64, tx);
        }
        let http_clone = ctx.http.clone();
        let channel_id_clone = msg.channel_id;

        crate::platforms::telemetry::spawn_telemetry_loop(http_clone, channel_id_clone, msg_id_u64, rx);

        Some(msg_id_u64.to_string())
    } else {
        None
    };

    // Attach platform metadata containing the channel, thinking msg, and source user msg.
    let platform_id = format!("discord:{}:{}:{}", msg.channel_id.get(), thinking_msg_id.unwrap_or_default(), msg.id.get());

    // Capture attachment metadata only — no downloads, no disk writes.
    // Apis can use the `read_attachment` tool to fetch content on-demand (in-memory).
    let mut enriched_content = msg.content.clone();

    // Context from Replies and Forwarded Messages
    if let Some(ref_msg) = &msg.referenced_message {
        enriched_content.push_str(&format!(
            "\n\n[REPLY_CONTEXT from {}]:\n{}",
            ref_msg.author.name, ref_msg.content
        ));
    } else if let Some(reference) = &msg.message_reference {
        // Fallback for forwarded messages if not loaded in referenced_message
        if let Some(m_id) = reference.message_id {
            if let Ok(ref_msg) = reference.channel_id.message(&ctx.http, m_id).await {
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
        author_name: msg.author.name.clone(),
        author_id: msg.author.id.get().to_string(),
        content: enriched_content,
    };

    let _ = handler.event_sender.send(ev).await;
}
