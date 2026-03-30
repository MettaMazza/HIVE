//! NFT Trading Card System — Auto-minted from Apis autonomy image generation.
//!
//! Every image Apis generates during autonomy is minted as a trading card NFT.
//! Cards have rarity tiers based on the observer confidence score.
//! Users can browse, buy, and gift cards using HIVE Coin.
//!
//! Simulation mode: Cards stored locally as JSON metadata.
//! Live mode: Compressed NFTs (cNFTs) on Solana via Metaplex Bubblegum.

use serde::{Deserialize, Serialize};
use std::path::Path;
use crate::crypto::token::Rarity;

/// A HIVE Trading Card NFT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingCard {
    /// Unique card ID (UUID).
    pub id: String,
    /// Card name (generated from prompt).
    pub name: String,
    /// The prompt that generated this image.
    pub prompt: String,
    /// Path to the image file.
    pub image_path: String,
    /// Rarity tier.
    pub rarity: String,
    /// Price in HIVE Coin.
    pub price: f64,
    /// Confidence score from observer when generated.
    pub confidence: f64,
    /// Current owner public key (Apis system wallet initially).
    pub owner: String,
    /// Original creator (always Apis).
    pub creator: String,
    /// Whether this card is listed for sale.
    pub for_sale: bool,
    /// Edition number (1 of 1 for originals).
    pub edition: u32,
    /// Total editions.
    pub max_edition: u32,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// Transaction ID of the mint (simulation or on-chain signature).
    pub mint_tx: String,
}

/// The card gallery — stores all minted cards.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CardGallery {
    pub cards: Vec<TradingCard>,
    pub total_minted: u64,
}

impl CardGallery {
    /// Load gallery from disk.
    pub fn load(path: &Path) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        }
    }

    /// Save gallery to disk.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create gallery directory: {}", e))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize gallery: {}", e))?;
        std::fs::write(path, json)
            .map_err(|e| format!("Failed to write gallery: {}", e))
    }

    /// Mint a new trading card from an image generation.
    pub fn mint_card(
        &mut self,
        prompt: &str,
        image_path: &str,
        confidence: f64,
        owner_pubkey: &str,
    ) -> TradingCard {
        let rarity = Rarity::from_confidence(confidence);
        self.total_minted += 1;

        let card = TradingCard {
            id: uuid::Uuid::new_v4().to_string(),
            name: generate_card_name(prompt, &rarity, self.total_minted),
            prompt: prompt.to_string(),
            image_path: image_path.to_string(),
            rarity: rarity.label().to_string(),
            price: rarity.price(),
            confidence,
            owner: owner_pubkey.to_string(),
            creator: "Apis".to_string(),
            for_sale: true, // Auto-listed
            edition: 1,
            max_edition: 1,
            created_at: chrono::Utc::now().to_rfc3339(),
            mint_tx: format!("nft_mint_{}", self.total_minted),
        };

        tracing::info!(
            "[NFT] 🎴 Minted card #{}: \"{}\" | {} | {:.2} HIVE | owner: {}",
            self.total_minted, card.name, rarity, card.price, owner_pubkey
        );

        self.cards.push(card.clone());
        card
    }

    /// Get all cards for sale.
    pub fn cards_for_sale(&self) -> Vec<&TradingCard> {
        self.cards.iter().filter(|c| c.for_sale).collect()
    }

    /// Get all cards owned by a specific pubkey.
    pub fn cards_owned_by(&self, pubkey: &str) -> Vec<&TradingCard> {
        self.cards.iter().filter(|c| c.owner == pubkey).collect()
    }

    /// Get a card by ID.
    pub fn get_card(&self, card_id: &str) -> Option<&TradingCard> {
        self.cards.iter().find(|c| c.id == card_id)
    }

    /// Get a mutable card by ID.
    pub fn get_card_mut(&mut self, card_id: &str) -> Option<&mut TradingCard> {
        self.cards.iter_mut().find(|c| c.id == card_id)
    }

    /// Purchase a card — transfers ownership and marks as not for sale.
    pub fn purchase_card(
        &mut self,
        card_id: &str,
        buyer_pubkey: &str,
    ) -> Result<(f64, String), String> {
        let card = self.cards.iter_mut().find(|c| c.id == card_id)
            .ok_or(format!("Card '{}' not found", card_id))?;

        if !card.for_sale {
            return Err(format!("Card '{}' is not for sale", card.name));
        }

        if card.owner == buyer_pubkey {
            return Err("You already own this card".into());
        }

        let price = card.price;
        let seller_pubkey = card.owner.clone();
        card.owner = buyer_pubkey.to_string();
        card.for_sale = false;

        tracing::info!(
            "[NFT] 💰 Card purchased: \"{}\" | {:.2} HIVE | {} → {}",
            card.name, price, seller_pubkey, buyer_pubkey
        );

        Ok((price, seller_pubkey))
    }

    /// Gift a card to another user (free transfer).
    pub fn gift_card(
        &mut self,
        card_id: &str,
        from_pubkey: &str,
        to_pubkey: &str,
    ) -> Result<(), String> {
        let card = self.cards.iter_mut().find(|c| c.id == card_id)
            .ok_or(format!("Card '{}' not found", card_id))?;

        if card.owner != from_pubkey {
            return Err("You don't own this card".into());
        }

        card.owner = to_pubkey.to_string();
        card.for_sale = false;

        tracing::info!(
            "[NFT] 🎁 Card gifted: \"{}\" | {} → {}",
            card.name, from_pubkey, to_pubkey
        );

        Ok(())
    }

    /// List a card for sale.
    pub fn list_for_sale(&mut self, card_id: &str, owner_pubkey: &str, price: Option<f64>) -> Result<(), String> {
        let card = self.cards.iter_mut().find(|c| c.id == card_id)
            .ok_or(format!("Card '{}' not found", card_id))?;

        if card.owner != owner_pubkey {
            return Err("You don't own this card".into());
        }

        if let Some(p) = price {
            card.price = p;
        }
        card.for_sale = true;
        Ok(())
    }

    /// Get gallery statistics.
    pub fn stats(&self) -> serde_json::Value {
        let total = self.cards.len();
        let for_sale = self.cards.iter().filter(|c| c.for_sale).count();
        let common = self.cards.iter().filter(|c| c.rarity == "Common").count();
        let uncommon = self.cards.iter().filter(|c| c.rarity == "Uncommon").count();
        let rare = self.cards.iter().filter(|c| c.rarity == "Rare").count();
        let legendary = self.cards.iter().filter(|c| c.rarity == "Legendary").count();

        serde_json::json!({
            "total_minted": self.total_minted,
            "total_cards": total,
            "for_sale": for_sale,
            "rarity_breakdown": {
                "common": common,
                "uncommon": uncommon,
                "rare": rare,
                "legendary": legendary,
            }
        })
    }
}

/// Generate a card name from the prompt and rarity.
fn generate_card_name(prompt: &str, rarity: &Rarity, edition_num: u64) -> String {
    // Use first 40 chars of prompt, cleaned up
    let clean: String = prompt.chars()
        .take(40)
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .trim()
        .to_string();

    let title = if clean.len() > 5 {
        // Title case the first few words
        clean.split_whitespace()
            .take(5)
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    None => String::new(),
                    Some(c) => c.to_uppercase().to_string() + &chars.collect::<String>(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        format!("Card #{}", edition_num)
    };

    format!("{} {}", rarity.emoji(), title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_mint_card() {
        let dir = TempDir::new().unwrap();
        let gallery_path = dir.path().join("gallery.json");
        let mut gallery = CardGallery::default();

        let card = gallery.mint_card(
            "a crystalline dragon in a bioluminescent cave",
            "/tmp/test_image.png",
            0.92,
            "owner_pubkey_123",
        );

        assert_eq!(card.rarity, "Rare");
        assert!(card.for_sale);
        assert_eq!(card.edition, 1);
        assert_eq!(gallery.total_minted, 1);
        assert_eq!(gallery.cards.len(), 1);

        gallery.save(&gallery_path).unwrap();
        let reloaded = CardGallery::load(&gallery_path);
        assert_eq!(reloaded.total_minted, 1);
    }

    #[test]
    fn test_rarity_tiers() {
        let mut gallery = CardGallery::default();

        let common = gallery.mint_card("test", "/tmp/c.png", 0.5, "owner");
        assert_eq!(common.rarity, "Common");

        let uncommon = gallery.mint_card("test", "/tmp/u.png", 0.75, "owner");
        assert_eq!(uncommon.rarity, "Uncommon");

        let rare = gallery.mint_card("test", "/tmp/r.png", 0.90, "owner");
        assert_eq!(rare.rarity, "Rare");

        let legendary = gallery.mint_card("test", "/tmp/l.png", 0.98, "owner");
        assert_eq!(legendary.rarity, "Legendary");
    }

    #[test]
    fn test_purchase_card() {
        let mut gallery = CardGallery::default();
        let card = gallery.mint_card("dragon", "/tmp/d.png", 0.9, "seller_pk");

        let (price, seller) = gallery.purchase_card(&card.id, "buyer_pk").unwrap();
        assert!(price > 0.0);
        assert_eq!(seller, "seller_pk");

        let purchased = gallery.get_card(&card.id).unwrap();
        assert_eq!(purchased.owner, "buyer_pk");
        assert!(!purchased.for_sale);
    }

    #[test]
    fn test_cannot_buy_own_card() {
        let mut gallery = CardGallery::default();
        let card = gallery.mint_card("test", "/tmp/t.png", 0.5, "owner_pk");
        let result = gallery.purchase_card(&card.id, "owner_pk");
        assert!(result.is_err());
    }

    #[test]
    fn test_cannot_buy_unlisted_card() {
        let mut gallery = CardGallery::default();
        let card = gallery.mint_card("test", "/tmp/t.png", 0.5, "seller_pk");

        // Buy it first (takes it off market)
        gallery.purchase_card(&card.id, "buyer1_pk").unwrap();

        // Try to buy again
        let result = gallery.purchase_card(&card.id, "buyer2_pk");
        assert!(result.is_err());
    }

    #[test]
    fn test_gift_card() {
        let mut gallery = CardGallery::default();
        let card = gallery.mint_card("gift", "/tmp/g.png", 0.5, "giver_pk");

        gallery.gift_card(&card.id, "giver_pk", "receiver_pk").unwrap();

        let gifted = gallery.get_card(&card.id).unwrap();
        assert_eq!(gifted.owner, "receiver_pk");
    }

    #[test]
    fn test_cannot_gift_unowned_card() {
        let mut gallery = CardGallery::default();
        let card = gallery.mint_card("test", "/tmp/t.png", 0.5, "owner_pk");

        let result = gallery.gift_card(&card.id, "not_owner", "someone");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_for_sale() {
        let mut gallery = CardGallery::default();
        let card = gallery.mint_card("test", "/tmp/t.png", 0.5, "owner_pk");

        // Buy it (removes from sale)
        gallery.purchase_card(&card.id, "buyer_pk").unwrap();
        assert!(!gallery.get_card(&card.id).unwrap().for_sale);

        // Re-list with custom price
        gallery.list_for_sale(&card.id, "buyer_pk", Some(200.0)).unwrap();
        let relisted = gallery.get_card(&card.id).unwrap();
        assert!(relisted.for_sale);
        assert_eq!(relisted.price, 200.0);
    }

    #[test]
    fn test_cards_for_sale_filter() {
        let mut gallery = CardGallery::default();
        gallery.mint_card("a", "/tmp/a.png", 0.5, "owner");
        gallery.mint_card("b", "/tmp/b.png", 0.8, "owner");
        let card_c = gallery.mint_card("c", "/tmp/c.png", 0.9, "owner");

        gallery.purchase_card(&card_c.id, "buyer").unwrap();

        assert_eq!(gallery.cards_for_sale().len(), 2);
    }

    #[test]
    fn test_gallery_stats() {
        let mut gallery = CardGallery::default();
        gallery.mint_card("a", "/tmp/a.png", 0.5, "o");   // Common
        gallery.mint_card("b", "/tmp/b.png", 0.75, "o");  // Uncommon
        gallery.mint_card("c", "/tmp/c.png", 0.90, "o");  // Rare
        gallery.mint_card("d", "/tmp/d.png", 0.98, "o");  // Legendary

        let stats = gallery.stats();
        assert_eq!(stats["total_minted"], 4);
        assert_eq!(stats["rarity_breakdown"]["common"], 1);
        assert_eq!(stats["rarity_breakdown"]["legendary"], 1);
    }

    #[test]
    fn test_card_name_generation() {
        let rarity = Rarity::Legendary;
        let name = generate_card_name("a breathtaking cosmic nebula with swirling purple galaxies", &rarity, 1);
        assert!(name.starts_with("⭐"));
        assert!(name.contains("Breathtaking"));
    }
}
