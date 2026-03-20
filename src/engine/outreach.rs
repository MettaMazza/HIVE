/// OutreachGate — per-user outreach settings, LLM timing gate, and delivery routing.
///
/// Per-user state persisted to: memory/outreach/{user_id}.json
///
/// Frequency tiers (hours between messages):
///   low=24h  medium=12h  high=3h  unlimited=always
///
/// Delivery policies:
///   dm      → DM only
///   public  → public outreach channel tag
///   both    → DM + public
///   none    → blocked entirely
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum OutreachFrequency {
    Low,
    #[default]
    Medium,
    High,
    Unlimited,
}

impl OutreachFrequency {
    /// Minimum hours required between outreach messages.
    pub fn min_hours(&self) -> f64 {
        match self {
            OutreachFrequency::Low => 24.0,
            OutreachFrequency::Medium => 12.0,
            OutreachFrequency::High => 3.0,
            OutreachFrequency::Unlimited => 0.0,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            OutreachFrequency::Low => "low",
            OutreachFrequency::Medium => "medium",
            OutreachFrequency::High => "high",
            OutreachFrequency::Unlimited => "unlimited",
        }
    }
}


impl std::str::FromStr for OutreachFrequency {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "unlimited" => Ok(Self::Unlimited),
            other => Err(format!("Unknown frequency: {}", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum OutreachDelivery {
    #[default]
    Dm,
    Public,
    Both,
    None,
}


impl std::str::FromStr for OutreachDelivery {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "dm" => Ok(Self::Dm),
            "public" => Ok(Self::Public),
            "both" => Ok(Self::Both),
            "none" => Ok(Self::None),
            other => Err(format!("Unknown delivery: {}", other)),
        }
    }
}

impl std::fmt::Display for OutreachDelivery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutreachDelivery::Dm => write!(f, "dm"),
            OutreachDelivery::Public => write!(f, "public"),
            OutreachDelivery::Both => write!(f, "both"),
            OutreachDelivery::None => write!(f, "none"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserOutreachSettings {
    pub frequency: OutreachFrequency,
    pub delivery: OutreachDelivery,
    pub last_outreach: Option<DateTime<Utc>>,
    pub interaction_count: u64,
    pub relationship_strength: u8, // 0–100
}

impl Default for UserOutreachSettings {
    fn default() -> Self {
        Self {
            frequency: OutreachFrequency::default(),
            delivery: OutreachDelivery::default(),
            last_outreach: None,
            interaction_count: 0,
            relationship_strength: 50,
        }
    }
}

fn outreach_path(project_root: &str, user_id: &str) -> PathBuf {
    PathBuf::from(project_root)
        .join("memory/outreach")
        .join(format!("{}.json", user_id))
}

pub struct OutreachGate {
    project_root: String,
    provider: Arc<dyn crate::providers::Provider>,
}

impl OutreachGate {
    pub fn new(project_root: &str, provider: Arc<dyn crate::providers::Provider>) -> Self {
        Self {
            project_root: project_root.to_string(),
            provider,
        }
    }

    fn load(&self, user_id: &str) -> UserOutreachSettings {
        let path = outreach_path(&self.project_root, user_id);
        if path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&path) {
                if let Ok(s) = serde_json::from_str::<UserOutreachSettings>(&raw) {
                    return s;
                }
            }
        }
        UserOutreachSettings::default()
    }

    fn save(&self, user_id: &str, settings: &UserOutreachSettings) {
        let path = outreach_path(&self.project_root, user_id);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(settings) {
            let _ = std::fs::write(path, json);
        }
    }

    /// Check timing + consent for outreach to a user.
    /// Returns (allowed, reason).
    pub async fn can_outreach(&self, user_id: &str) -> (bool, String) {
        tracing::debug!("[ENGINE:Outreach] ▶ can_outreach for user_id={}", user_id);
        let s = self.load(user_id);

        if s.delivery == OutreachDelivery::None {
            tracing::debug!("[ENGINE:Outreach] ◀ Outreach blocked for user_id={} (delivery=none)", user_id);
            return (false, "Outreach disabled for this user.".into());
        }
        if s.frequency == OutreachFrequency::Unlimited {
            return (true, "Frequency set to unlimited.".into());
        }
        let Some(last) = s.last_outreach else {
            return (true, "First outreach to this user.".into());
        };

        let hours_since = (Utc::now() - last).num_seconds() as f64 / 3600.0;

        // LLM timing gate
        self.ai_outreach_decision(
            user_id,
            &s.frequency,
            hours_since,
            s.relationship_strength,
            s.interaction_count,
        ).await
    }

    async fn ai_outreach_decision(
        &self,
        user_id: &str,
        frequency: &OutreachFrequency,
        hours_since: f64,
        strength: u8,
        interactions: u64,
    ) -> (bool, String) {
        let prompt = format!(
            "ROLE: Relationship Timing Advisor\n\n\
             CONTEXT:\n\
             - User ID: {user_id}\n\
             - Frequency Preference: {freq} (low=less often, high=more often)\n\
             - Hours Since Last Contact: {hours:.1}\n\
             - Relationship Strength: {strength}/100\n\
             - Total Interactions: {interactions}\n\n\
             TASK: Should Apis reach out to this user now?\n\
             - \"low\" preference users want 24+ hours between contacts\n\
             - \"medium\" preference users are OK with 8-12 hour gaps\n\
             - \"high\" preference users welcome frequent contact (3-6 hours)\n\
             - Stronger relationships allow more flexibility\n\
             - Always err on the side of respecting boundaries\n\n\
             RESPOND WITH ONLY ONE WORD: YES or NO",
            user_id = user_id,
            freq = frequency.as_str(),
            hours = hours_since,
            strength = strength,
            interactions = interactions,
        );

        let event = crate::models::message::Event {
            platform: "system:outreach".into(),
            scope: crate::models::scope::Scope::Private {
                user_id: "apis_self".into(),
            },
            author_name: "OutreachGate".into(),
            author_id: "apis_self".into(),
            content: prompt.clone(),
        };

        match self.provider.generate(&prompt, &[], &event, "", None, None).await {
            Ok(response) => {
                let upper = response.trim().to_uppercase();
                if upper.contains("YES") {
                    tracing::debug!("[ENGINE:Outreach] AI approved outreach for user_id={} ({:.1}h since last)", user_id, hours_since);
                    (true, format!("AI approved: appropriate timing ({:.1}h since last)", hours_since))
                } else {
                    tracing::debug!("[ENGINE:Outreach] AI denied outreach for user_id={} ({:.1}h since last)", user_id, hours_since);
                    (false, format!("AI: not appropriate time ({:.1}h since last)", hours_since))
                }
            }
            Err(_) => {
                tracing::warn!("[ENGINE:Outreach] AI timing gate failed for user_id={}, using fallback threshold", user_id);
                // Fallback: hard threshold
                let min = frequency.min_hours();
                if hours_since >= min {
                    (true, format!("Fallback: {:.1}h elapsed, threshold {:.0}h", hours_since, min))
                } else {
                    (false, format!("Fallback: wait {:.1}h more (threshold {}h)", min - hours_since, min as u32))
                }
            }
        }
    }

    /// Record that outreach was successfully sent.
    pub fn record_outreach(&self, user_id: &str) {
        tracing::debug!("[ENGINE:Outreach] Recording outreach sent for user_id={}", user_id);
        let mut s = self.load(user_id);
        s.last_outreach = Some(Utc::now());
        self.save(user_id, &s);
    }

    /// Update cached relationship data (called on each incoming user message).
    pub fn record_interaction(&self, user_id: &str, relationship_strength: u8) {
        tracing::debug!("[ENGINE:Outreach] Recording interaction for user_id={} (strength={})", user_id, relationship_strength);
        let mut s = self.load(user_id);
        s.interaction_count += 1;
        s.relationship_strength = relationship_strength;
        self.save(user_id, &s);
    }

    pub fn set_frequency(&self, user_id: &str, freq: OutreachFrequency) -> String {
        tracing::debug!("[ENGINE:Outreach] Setting frequency for user_id={} to '{}'", user_id, freq.as_str());
        let mut s = self.load(user_id);
        let label = freq.as_str().to_string();
        let hours = freq.min_hours();
        s.frequency = freq;
        self.save(user_id, &s);
        let desc = if hours > 0.0 { format!("{}h between messages", hours as u32) } else { "no limit".into() };
        format!("✅ Outreach frequency set to '{}' ({}).", label, desc)
    }

    pub fn set_delivery(&self, user_id: &str, delivery: OutreachDelivery) -> String {
        tracing::debug!("[ENGINE:Outreach] Setting delivery for user_id={} to '{}'", user_id, delivery);
        let mut s = self.load(user_id);
        let label = delivery.to_string();
        s.delivery = delivery;
        self.save(user_id, &s);
        format!("✅ Outreach delivery set to '{}'.", label)
    }

    pub fn get_settings(&self, user_id: &str) -> UserOutreachSettings {
        self.load(user_id)
    }

    pub fn get_delivery(&self, user_id: &str) -> OutreachDelivery {
        self.load(user_id).delivery
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frequency_min_hours() {
        assert_eq!(OutreachFrequency::Low.min_hours(), 24.0);
        assert_eq!(OutreachFrequency::Medium.min_hours(), 12.0);
        assert_eq!(OutreachFrequency::High.min_hours(), 3.0);
        assert_eq!(OutreachFrequency::Unlimited.min_hours(), 0.0);
    }

    #[test]
    fn test_frequency_from_str() {
        assert_eq!("low".parse::<OutreachFrequency>().unwrap(), OutreachFrequency::Low);
        assert_eq!("unlimited".parse::<OutreachFrequency>().unwrap(), OutreachFrequency::Unlimited);
        assert!("bad".parse::<OutreachFrequency>().is_err());
    }

    #[test]
    fn test_delivery_from_str() {
        assert_eq!("dm".parse::<OutreachDelivery>().unwrap(), OutreachDelivery::Dm);
        assert_eq!("both".parse::<OutreachDelivery>().unwrap(), OutreachDelivery::Both);
        assert!("bad".parse::<OutreachDelivery>().is_err());
    }

    #[test]
    fn test_settings_default() {
        let s = UserOutreachSettings::default();
        assert_eq!(s.frequency, OutreachFrequency::Medium);
        assert_eq!(s.delivery, OutreachDelivery::Dm);
        assert!(s.last_outreach.is_none());
        assert_eq!(s.relationship_strength, 50);
    }

    #[tokio::test]
    async fn test_outreach_gate_crud() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_str().unwrap();
        
        let mut mock = crate::providers::MockProvider::new();
        mock.expect_generate().returning(|_, _, _, _, _, _| Ok("YES".into()));
        let gate = OutreachGate::new(root, Arc::new(mock));

        // 1. load nonexistent (gets default)
        let s1 = gate.load("user1");
        assert_eq!(s1.frequency, OutreachFrequency::Medium);

        // 2. set frequency
        let res = gate.set_frequency("user1", OutreachFrequency::High);
        assert!(res.contains("Outreach frequency set to 'high'"));
        let s2 = gate.load("user1");
        assert_eq!(s2.frequency, OutreachFrequency::High);

        // 3. set delivery
        let res2 = gate.set_delivery("user1", OutreachDelivery::Both);
        assert!(res2.contains("Outreach delivery set to 'both'"));
        assert_eq!(gate.get_delivery("user1"), OutreachDelivery::Both);

        // 4. record interaction
        gate.record_interaction("user1", 85);
        let s3 = gate.get_settings("user1");
        assert_eq!(s3.interaction_count, 1);
        assert_eq!(s3.relationship_strength, 85);

        // 5. record outreach timestamp
        gate.record_outreach("user1");
        let s4 = gate.get_settings("user1");
        assert!(s4.last_outreach.is_some());
    }

    #[tokio::test]
    async fn test_outreach_gate_can_outreach_logic() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_str().unwrap();
        
        // Mock that says "YES"
        let mut mock_yes = crate::providers::MockProvider::new();
        mock_yes.expect_generate().returning(|_, _, _, _, _, _| Ok("YES".into()));
        let gate_yes = OutreachGate::new(root, Arc::new(mock_yes));

        // Setup a user
        gate_yes.set_delivery("u1", OutreachDelivery::Dm);
        
        // 1. None returns false immediately
        gate_yes.set_delivery("unone", OutreachDelivery::None);
        let (can, reason) = gate_yes.can_outreach("unone").await;
        assert!(!can);
        assert!(reason.contains("Outreach disabled"));

        // 2. Unlimited returns true immediately
        gate_yes.set_frequency("ufast", OutreachFrequency::Unlimited);
        let (can_fast, _) = gate_yes.can_outreach("ufast").await;
        assert!(can_fast);

        // 3. First outreach returns true
        let (can_first, reason_first) = gate_yes.can_outreach("u1").await;
        assert!(can_first);
        assert!(reason_first.contains("First outreach"));

        // Record outreach so they aren't "first" anymore
        gate_yes.record_outreach("u1");

        // 4. Test LLM logic returning YES
        let (can_ai, reason_ai) = gate_yes.can_outreach("u1").await;
        assert!(can_ai);
        assert!(reason_ai.contains("AI approved"));
    }

    #[tokio::test]
    async fn test_outreach_gate_can_outreach_llm_rejection_and_fallback() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_str().unwrap();
        
        // Mock that says "NO"
        let mut mock_no = crate::providers::MockProvider::new();
        mock_no.expect_generate().returning(|_, _, _, _, _, _| Ok("NO".into()));
        let gate_no = OutreachGate::new(root, Arc::new(mock_no));

        gate_no.record_outreach("u2");
        let (can, reason) = gate_no.can_outreach("u2").await;
        assert!(!can);
        assert!(reason.contains("AI: not appropriate time"));

        // Mock that FAILS (tests fallback threshold logic)
        let mut mock_err = crate::providers::MockProvider::new();
        mock_err.expect_generate().returning(|_, _, _, _, _, _| Err(crate::providers::ProviderError::ConnectionError("offline".into())));
        let gate_err = OutreachGate::new(root, Arc::new(mock_err));

        gate_err.set_frequency("u3", OutreachFrequency::Low); // 24hr limit
        gate_err.record_outreach("u3"); // set to just now (0hrs ago)
        
        let (can_fb_wait, reason_fb_wait) = gate_err.can_outreach("u3").await;
        assert!(!can_fb_wait);
        assert!(reason_fb_wait.contains("Fallback: wait"));
    }
}
