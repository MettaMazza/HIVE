use std::sync::Arc;
use crate::memory::MemoryStore;
use crate::models::scope::Scope;
use crate::models::message::Event;

pub async fn synthesize_50_turn(
    provider: Arc<dyn crate::providers::Provider>,
    memory: Arc<MemoryStore>,
    scope: Scope,
    drives: Option<Arc<crate::engine::drives::DriveSystem>>,
) -> Result<(), String> {
    tracing::info!("[SYNTHESIS] ▶ 50-turn synthesis for scope='{}'", scope.to_key());
    let timeline_data = memory.timeline.read_timeline(&scope).await.unwrap_or_default();
    let history_str = String::from_utf8_lossy(&timeline_data);
    let recent_lines: Vec<&str> = history_str.lines().rev().take(100).collect();
    let recent_events = recent_lines.into_iter().rev().collect::<Vec<_>>().join("\n");

    let prompt = format!(
        "You are the HIVE Timeline Synthesizer.\n\
         Review the following recent chronological timeline of events (JSONL):\n\
         {}\n\n\
         Based on these events, write a concise, single-paragraph narrative summary of what has happened over the last 50 conversational turns. Focus on the progression of tasks, major insights, user decisions, or shifts in context. Write from a high-level analytical perspective. DO NOT output headers or conversational filler.",
         recent_events
    );

    let dummy_event = Event {
        platform: "system:timeline".into(),
        scope: scope.clone(),
        author_name: "Synthesizer".into(),
        author_id: "system".into(),
        content: "Initiate 50-Turn Synthesis".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
    };

    let result = provider.generate(&prompt, &[], &dummy_event, "", None, None).await.map_err(|e| e.to_string())?;

    let mut timelines = memory.timelines.read(&scope).await;
    timelines.last_50_turns = Some(crate::memory::TurnSummary {
        narrative: result.trim().to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
    });

    memory.timelines.write(&scope, &timelines).await.map_err(|e| e.to_string())?;
    tracing::info!("[SYNTHESIS] ✅ 50-turn synthesis complete for scope='{}'", scope.to_key());

    if let Some(d) = drives {
        d.modify_drive("uncertainty", -5.0).await;
        tracing::debug!("[EPIPHYTE] 50-turn condensed. Native Drive uncertainty drops natively by -5.0");
    }

    Ok(())
}

pub async fn synthesize_24_hr(
    provider: Arc<dyn crate::providers::Provider>,
    memory: Arc<MemoryStore>,
    scope: Scope,
    drives: Option<Arc<crate::engine::drives::DriveSystem>>,
) -> Result<(), String> {
    tracing::info!("[SYNTHESIS] ▶ 24-hour synthesis for scope='{}'", scope.to_key());
    let timeline_data = memory.timeline.read_timeline(&scope).await.unwrap_or_default();
    let history_str = String::from_utf8_lossy(&timeline_data);
    let recent_lines: Vec<&str> = history_str.lines().rev().take(800).collect();
    let recent_events = recent_lines.into_iter().rev().collect::<Vec<_>>().join("\n");

    let prompt = format!(
        "You are the HIVE Timeline Synthesizer.\n\
         Review the following log of events from approximately the last 24 hours:\n\
         {}\n\n\
         Write a high-level summary of the day's events. Focus on overarching goals achieved, major discussions held, and any significant shifts in user behavior or system state. Output a single cohesive narrative paragraph.",
         recent_events
    );

    let dummy_event = Event {
        platform: "system:timeline".into(),
        scope: scope.clone(),
        author_name: "Synthesizer".into(),
        author_id: "system".into(),
        content: "Initiate 24-Hour Synthesis".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
    };

    let result = provider.generate(&prompt, &[], &dummy_event, "", None, None).await.map_err(|e| e.to_string())?;

    let mut timelines = memory.timelines.read(&scope).await;
    timelines.last_24_hours = Some(crate::memory::DailySummary {
        narrative: result.trim().to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
    });

    memory.timelines.write(&scope, &timelines).await.map_err(|e| e.to_string())?;
    tracing::info!("[SYNTHESIS] ✅ 24-hour synthesis complete for scope='{}'", scope.to_key());

    if let Some(d) = drives {
        d.modify_drive("uncertainty", -15.0).await;
        d.modify_drive("system_health", 5.0).await;
        tracing::debug!("[EPIPHYTE] 24-hr condensed. Uncertainty drops -15.0, health rises +5.0");
    }

    Ok(())
}

pub async fn synthesize_lifetime(
    provider: Arc<dyn crate::providers::Provider>,
    memory: Arc<MemoryStore>,
    scope: Scope,
    drives: Option<Arc<crate::engine::drives::DriveSystem>>,
) -> Result<(), String> {
    tracing::info!("[SYNTHESIS] ▶ Lifetime synthesis for scope='{}'", scope.to_key());
    let timelines = memory.timelines.read(&scope).await;
    
    let previous_lifetime = timelines.lifetime.as_ref().map(|l| l.narrative.as_str()).unwrap_or("No previous lifetime summary exists. This is the origin.");
    let recent_day = timelines.last_24_hours.as_ref().map(|d| d.narrative.as_str()).unwrap_or("No recent daily block.");

    let prompt = format!(
        "You are the HIVE Lifetime Synthesizer.\n\
         Your task is to update the agent's continuous lifetime narrative by merging the most recent daily events into the existing historic trajectory.\n\n\
         [PREVIOUS LIFETIME FRAGMENT]: {}\n\
         [RECENT 24-HOUR FRAGMENT]: {}\n\n\
         Write a completely new unified paragraph that tells the continuous story of this agent's entire operational lifetime within this scope, smoothly incorporating the recent day's events. Keep it majestic but factual. No markdown headers.",
         previous_lifetime, recent_day
    );

    let dummy_event = Event {
        platform: "system:timeline".into(),
        scope: scope.clone(),
        author_name: "Synthesizer".into(),
        author_id: "system".into(),
        content: "Initiate Lifetime Synthesis".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
    };

    let result = provider.generate(&prompt, &[], &dummy_event, "", None, None).await.map_err(|e| e.to_string())?;

    let mut timelines_mem = memory.timelines.read(&scope).await;
    timelines_mem.lifetime = Some(crate::memory::LifetimeSummary {
        narrative: result.trim().to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
    });

    memory.timelines.write(&scope, &timelines_mem).await.map_err(|e| e.to_string())?;
    tracing::info!("[SYNTHESIS] ✅ Lifetime synthesis complete for scope='{}'", scope.to_key());

    if let Some(d) = drives {
        d.modify_drive("uncertainty", -25.0).await;
        d.modify_drive("system_health", 10.0).await;
        tracing::debug!("[EPIPHYTE] Lifetime summary secured. Uncertainty drops -25.0, health rises +10.0");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::MockProvider;

    #[tokio::test]
    async fn test_synthesis_50_turn() {
        let mut mock = MockProvider::new();
        mock.expect_generate().returning(|_, _, _, _, _, _| Ok("50 turn summary".to_string()));
        let memory = Arc::new(MemoryStore::default());
        let scope = Scope::Private { user_id: "u1".into() };

        let res = synthesize_50_turn(Arc::new(mock), memory.clone(), scope.clone(), None).await;
        assert!(res.is_ok());

        let t = memory.timelines.read(&scope).await;
        assert_eq!(t.last_50_turns.unwrap().narrative, "50 turn summary");
    }

    #[tokio::test]
    async fn test_synthesis_24_hr() {
        let mut mock = MockProvider::new();
        mock.expect_generate().returning(|_, _, _, _, _, _| Ok("24 hour summary".to_string()));
        let memory = Arc::new(MemoryStore::default());
        let scope = Scope::Private { user_id: "u1".into() };

        let res = synthesize_24_hr(Arc::new(mock), memory.clone(), scope.clone(), None).await;
        assert!(res.is_ok());

        let t = memory.timelines.read(&scope).await;
        assert_eq!(t.last_24_hours.unwrap().narrative, "24 hour summary");
    }

    #[tokio::test]
    async fn test_synthesis_lifetime() {
        let mut mock = MockProvider::new();
        mock.expect_generate().returning(|_, _, _, _, _, _| Ok("Lifetime summary".to_string()));
        let memory = Arc::new(MemoryStore::default());
        let scope = Scope::Private { user_id: "u1".into() };

        let res = synthesize_lifetime(Arc::new(mock), memory.clone(), scope.clone(), None).await;
        assert!(res.is_ok());

        let t = memory.timelines.read(&scope).await;
        assert_eq!(t.lifetime.unwrap().narrative, "Lifetime summary");
    }

    #[tokio::test]
    async fn test_synthesis_failure() {
        let mut mock = MockProvider::new();
        mock.expect_generate().returning(|_, _, _, _, _, _| Err(crate::providers::ProviderError::ConnectionError("fail".into())));
        let memory = Arc::new(MemoryStore::default());
        let scope = Scope::Private { user_id: "u1".into() };

        let res = synthesize_50_turn(Arc::new(mock), memory, scope, None).await;
        assert!(res.is_err());
    }
}
