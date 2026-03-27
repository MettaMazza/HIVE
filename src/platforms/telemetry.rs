use std::sync::Arc;
use serenity::http::Http;
use serenity::model::id::{ChannelId, MessageId};
use tokio::sync::watch::Receiver;

/// Spawns a background task that periodically updates a Discord message
/// to display live telemetry (thinking status) from the HIVE engine.
#[cfg(not(tarpaulin_include))]
pub fn spawn_telemetry_loop(
    http: Arc<Http>,
    channel_id: ChannelId,
    msg_id: u64,
    mut rx: Receiver<Option<String>>,
) {
    tokio::spawn(async move {
        let bees = ["🐝", "🍯", "🌼", "🐝"];
        let mut bee_idx = 0;
        tracing::debug!("[TELEMETRY:LOOP] 🔄 Watch loop started for msg_id={}", msg_id);
        loop {
            let text_opt = rx.borrow().clone();
            match text_opt {
                Some(text) => {
                    let is_complete = text.starts_with("✅");
                    let color = if is_complete { 0x57F287u32 } else { 0x5865F2u32 };
                    
                    let loading_blerb = if is_complete {
                        "✨ Complete".to_string()
                    } else {
                        format!("{} Processing...", bees[bee_idx % bees.len()])
                    };
                    
                    let embed = serenity::builder::CreateEmbed::new()
                        .description(format!("```\n{}\n```", text))
                        .footer(serenity::builder::CreateEmbedFooter::new(loading_blerb))
                        .color(color);
                        
                    let edit_builder = serenity::builder::EditMessage::new().embed(embed);
                    match channel_id.edit_message(&http, MessageId::new(msg_id), edit_builder).await {
                        Ok(_) => tracing::trace!("[TELEMETRY:LOOP] ✏️ Embed updated for msg_id={}", msg_id),
                        Err(e) => {
                            let err_str = e.to_string();
                            if err_str.contains("Unknown Message") {
                                tracing::warn!("[TELEMETRY:LOOP] 🗑️ Message {} was deleted — exiting loop", msg_id);
                                break;
                            }
                            tracing::warn!("[TELEMETRY:LOOP] ❌ Embed edit failed for msg_id={}: {}", msg_id, e);
                        }
                    }
                    
                    if is_complete {
                        tracing::debug!("[TELEMETRY:LOOP] ✅ Complete — exiting watch loop for msg_id={}", msg_id);
                        break;
                    }
                }
                None => {
                    tracing::debug!("[TELEMETRY:LOOP] 🔌 Watch value=None — exiting for msg_id={}", msg_id);
                    break;
                }
            }
            
            let sleep = tokio::time::sleep(tokio::time::Duration::from_secs(5));
            tokio::select! {
                _ = sleep => {
                    bee_idx += 1;
                }
                res = rx.changed() => {
                    if res.is_err() { break; }
                }
            }
        }
    });
}
