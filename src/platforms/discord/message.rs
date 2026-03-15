use serenity::prelude::*;
use serenity::model::channel::Message;
use crate::models::message::Event;
use crate::models::scope::Scope;

pub async fn handle_message(handler: &super::Handler, ctx: Context, msg: Message) {
    // Ignore self
    let is_self = {
        let id_lock = handler.bot_user_id.lock().await;
        id_lock.map(|id| id == msg.author.id).unwrap_or(false)
    };
    if is_self {
        return;
    }

    let mut bot_chunked_text: Option<String> = None;
    if msg.author.bot {
        // Ignore embed-only messages from other bots
        if !msg.embeds.is_empty() && msg.content.trim().is_empty() {
            return;
        }

        // Wait 5 seconds to group chunks from chatty bots
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        // Check if there's a newer message from this bot in the channel; if so, skip this one
        if let Ok(recent) = msg.channel_id.messages(&ctx.http, serenity::builder::GetMessages::new().limit(10)).await {
            if recent.iter().any(|m| m.author.id == msg.author.id && m.id.get() > msg.id.get()) {
                return;
            }
            
            // We are the final message in the 5-second burst. Gather the recent consecutive chunks.
            let mut chunks = Vec::new();
            for m in recent.into_iter() {
                if m.author.id != msg.author.id { break; } // stop at first msg not from this bot
                if !m.content.trim().is_empty() {
                    chunks.push(m.content);
                }
            }
            chunks.reverse(); // Chronological order
            bot_chunked_text = Some(chunks.join("\n"));
        }
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
    let mut enriched_content = bot_chunked_text.unwrap_or_else(|| msg.content.clone());

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

    if !msg.author.bot {
        handler.memory.interrupt_autonomy.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    
    let _ = handler.event_sender.send(ev).await;
}
