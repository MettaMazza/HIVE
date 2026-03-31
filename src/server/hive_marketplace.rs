/// HIVE Goods & Services Marketplace — Peer-to-Peer Commerce Engine.
///
/// Listing management, browsing, search, purchases, and reviews.
/// Served on localhost:3038 (configurable via REMOVED_MESH_GOVERNED).
///
/// API:
///   GET  /api/listings              — browse active listings (paginated)
///   GET  /api/listings/:id          — single listing detail
///   POST /api/listings              — create listing
///   PUT  /api/listings/:id          — update listing
///   DELETE /api/listings/:id        — cancel listing
///   POST /api/listings/:id/buy      — purchase listing
///   POST /api/listings/:id/review   — leave review
///   GET  /api/categories            — list categories with counts
///   GET  /api/search?q=term         — full-text search
///   GET  /api/seller/:peer_id       — listings by seller
///   GET  /api/stats                 — marketplace statistics

use axum::{
    routing::{get, post, put, delete},
    Router, Json,
    response::Html,
    extract::Path,
    extract::Query,
};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::fs;
use uuid::Uuid;
use chrono::Utc;

const LISTINGS_PATH: &str = "data/marketplace/listings.json";

// ─── Data Models ─────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum ListingCategory {
    DigitalGoods,
    Services,
    ComputeTime,
    StorageSpace,
    MeshSites,
    Other,
}

impl ListingCategory {
    pub fn as_str(&self) -> &str {
        match self {
            ListingCategory::DigitalGoods => "DigitalGoods",
            ListingCategory::Services => "Services",
            ListingCategory::ComputeTime => "ComputeTime",
            ListingCategory::StorageSpace => "StorageSpace",
            ListingCategory::MeshSites => "MeshSites",
            ListingCategory::Other => "Other",
        }
    }

    pub fn all() -> Vec<Self> {
        vec![
            ListingCategory::DigitalGoods,
            ListingCategory::Services,
            ListingCategory::ComputeTime,
            ListingCategory::StorageSpace,
            ListingCategory::MeshSites,
            ListingCategory::Other,
        ]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum ListingStatus {
    Active,
    Sold,
    Cancelled,
}

impl ListingStatus {
    pub fn as_str(&self) -> &str {
        match self {
            ListingStatus::Active => "Active",
            ListingStatus::Sold => "Sold",
            ListingStatus::Cancelled => "Cancelled",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Review {
    pub reviewer_peer_id: String,
    pub rating: u8,
    pub comment: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketplaceListing {
    pub id: String,
    pub seller_peer_id: String,
    pub title: String,
    pub description: String,
    pub category: ListingCategory,
    pub price_credits: Option<f64>,
    pub price_hive: Option<f64>,
    pub images: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub status: ListingStatus,
    pub reviews: Vec<Review>,
    pub tags: Vec<String>,
}

impl MarketplaceListing {
    pub fn new(
        seller_peer_id: String,
        title: String,
        description: String,
        category: ListingCategory,
        price_credits: Option<f64>,
        price_hive: Option<f64>,
        tags: Vec<String>,
    ) -> Self {
        let now = Utc::now().to_rfc3339();
        MarketplaceListing {
            id: Uuid::new_v4().to_string(),
            seller_peer_id,
            title,
            description,
            category,
            price_credits,
            price_hive,
            images: vec![],
            created_at: now.clone(),
            updated_at: now,
            status: ListingStatus::Active,
            reviews: vec![],
            tags,
        }
    }

    pub fn average_rating(&self) -> f64 {
        if self.reviews.is_empty() {
            0.0
        } else {
            let sum: u32 = self.reviews.iter().map(|r| r.rating as u32).sum();
            sum as f64 / self.reviews.len() as f64
        }
    }
}

pub struct MarketplaceStore {
    listings: Vec<MarketplaceListing>,
}

impl MarketplaceStore {
    pub fn new() -> Self {
        MarketplaceStore {
            listings: vec![],
        }
    }

    pub fn load(path: &PathBuf) -> Self {
        if !path.exists() {
            return MarketplaceStore::new();
        }

        match fs::read_to_string(path) {
            Ok(content) => {
                match serde_json::from_str::<Vec<MarketplaceListing>>(&content) {
                    Ok(listings) => MarketplaceStore { listings },
                    Err(e) => {
                        tracing::warn!("[MARKETPLACE] Failed to deserialize listings: {}", e);
                        MarketplaceStore::new()
                    }
                }
            }
            Err(e) => {
                tracing::warn!("[MARKETPLACE] Failed to load listings: {}", e);
                MarketplaceStore::new()
            }
        }
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.listings)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn create_listing(&mut self, listing: MarketplaceListing) -> String {
        let id = listing.id.clone();
        self.listings.push(listing);
        id
    }

    pub fn get_listing(&self, id: &str) -> Option<MarketplaceListing> {
        self.listings.iter().find(|l| l.id == id).cloned()
    }

    pub fn update_listing(&mut self, id: &str, mut updates: MarketplaceListing) -> bool {
        if let Some(pos) = self.listings.iter().position(|l| l.id == id) {
            updates.id = id.to_string();
            updates.updated_at = Utc::now().to_rfc3339();
            self.listings[pos] = updates;
            true
        } else {
            false
        }
    }

    pub fn cancel_listing(&mut self, id: &str) -> bool {
        if let Some(listing) = self.listings.iter_mut().find(|l| l.id == id) {
            listing.status = ListingStatus::Cancelled;
            listing.updated_at = Utc::now().to_rfc3339();
            true
        } else {
            false
        }
    }

    pub fn get_active_listings(&self) -> Vec<MarketplaceListing> {
        self.listings
            .iter()
            .filter(|l| l.status == ListingStatus::Active)
            .cloned()
            .collect()
    }

    pub fn get_listings_by_seller(&self, seller_peer_id: &str) -> Vec<MarketplaceListing> {
        self.listings
            .iter()
            .filter(|l| l.seller_peer_id == seller_peer_id && l.status == ListingStatus::Active)
            .cloned()
            .collect()
    }

    pub fn get_listings_by_category(&self, category: &ListingCategory) -> Vec<MarketplaceListing> {
        self.listings
            .iter()
            .filter(|l| l.category == *category && l.status == ListingStatus::Active)
            .cloned()
            .collect()
    }

    pub fn search(&self, query: &str) -> Vec<MarketplaceListing> {
        let query_lower = query.to_lowercase();
        self.listings
            .iter()
            .filter(|l| {
                l.status == ListingStatus::Active
                    && (l.title.to_lowercase().contains(&query_lower)
                        || l.description.to_lowercase().contains(&query_lower)
                        || l.tags
                            .iter()
                            .any(|t| t.to_lowercase().contains(&query_lower)))
            })
            .cloned()
            .collect()
    }

    pub fn add_review(&mut self, listing_id: &str, review: Review) -> bool {
        if let Some(listing) = self.listings.iter_mut().find(|l| l.id == listing_id) {
            listing.reviews.push(review);
            listing.updated_at = Utc::now().to_rfc3339();
            true
        } else {
            false
        }
    }

    pub fn mark_sold(&mut self, id: &str) -> bool {
        if let Some(listing) = self.listings.iter_mut().find(|l| l.id == id) {
            listing.status = ListingStatus::Sold;
            listing.updated_at = Utc::now().to_rfc3339();
            true
        } else {
            false
        }
    }

    pub fn get_stats(&self) -> Value {
        let active = self.get_active_listings();
        let sold_count = self.listings
            .iter()
            .filter(|l| l.status == ListingStatus::Sold)
            .count();

        let all_categories = ListingCategory::all();
        let category_counts: Vec<_> = all_categories
            .iter()
            .map(|cat| {
                let count = active.iter().filter(|l| l.category == *cat).count();
                (cat.as_str(), count)
            })
            .collect();

        json!({
            "total_listings": self.listings.len(),
            "active_listings": active.len(),
            "sold_count": sold_count,
            "category_breakdown": category_counts,
        })
    }
}

// ─── Server State ───────────────────────────────────────────────────

#[derive(Clone)]
struct MarketplaceState {
    store: Arc<Mutex<MarketplaceStore>>,
}

impl MarketplaceState {
    fn listings_path() -> PathBuf {
        PathBuf::from(LISTINGS_PATH)
    }

    fn new() -> Self {
        let store = MarketplaceStore::load(&Self::listings_path());
        MarketplaceState {
            store: Arc::new(Mutex::new(store)),
        }
    }

    fn save(&self) -> Result<(), String> {
        let store = self.store.lock().unwrap();
        store
            .save(&Self::listings_path())
            .map_err(|e| format!("Failed to save listings: {}", e))
    }
}

// ─── Request/Response Types ─────────────────────────────────────────

#[derive(Deserialize)]
struct CreateListingRequest {
    seller_peer_id: String,
    title: String,
    description: String,
    category: ListingCategory,
    price_credits: Option<f64>,
    price_hive: Option<f64>,
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct UpdateListingRequest {
    title: Option<String>,
    description: Option<String>,
    price_credits: Option<f64>,
    price_hive: Option<f64>,
    tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct BuyListingRequest {
    buyer_peer_id: String,
    payment_type: String, // "credits" or "hive"
}

#[derive(Deserialize)]
struct ReviewRequest {
    reviewer_peer_id: String,
    rating: u8,
    comment: String,
}

#[derive(Deserialize)]
struct PaginationParams {
    page: Option<u32>,
    limit: Option<u32>,
    category: Option<String>,
}

#[derive(Deserialize)]
struct SearchParams {
    q: String,
}

// ─── Main Server Spawn ──────────────────────────────────────────────

pub async fn spawn_hive_marketplace_server() {
    let port: u16 = std::env::var("REMOVED_MESH_GOVERNED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3038);

    let state = MarketplaceState::new();

    tokio::spawn(async move {
        tracing::info!("[MARKETPLACE] 🏪 HIVE Marketplace starting on http://0.0.0.0:{}", port);

        let app = Router::new()
            .route("/api/listings", get(api_list_listings))
            .route("/api/listings", post(api_create_listing))
            .route("/api/listings/{id}", get(api_get_listing))
            .route("/api/listings/{id}", put(api_update_listing))
            .route("/api/listings/{id}", delete(api_cancel_listing))
            .route("/api/listings/{id}/buy", post(api_buy_listing))
            .route("/api/listings/{id}/review", post(api_add_review))
            .route("/api/categories", get(api_categories))
            .route("/api/search", get(api_search))
            .route("/api/seller/{peer_id}", get(api_seller_listings))
            .route("/api/stats", get(api_stats))
            .fallback(get(serve_marketplace_page))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let addr = format!("0.0.0.0:{}", port);
        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                tracing::info!("[MARKETPLACE] 🏪 Bound on {}", addr);
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("[MARKETPLACE] ❌ Server error: {}", e);
                }
            }
            Err(e) => tracing::error!("[MARKETPLACE] ❌ Failed to bind {}: {}", addr, e),
        }
    });
}

// ─── API Handlers ───────────────────────────────────────────────────

async fn api_list_listings(
    axum::extract::State(state): axum::extract::State<MarketplaceState>,
    Query(params): Query<PaginationParams>,
) -> Json<Value> {
    let store = state.store.lock().unwrap();

    let mut listings = store.get_active_listings();

    // Filter by category if specified
    if let Some(cat_str) = params.category {
        let category = match cat_str.as_str() {
            "DigitalGoods" => ListingCategory::DigitalGoods,
            "Services" => ListingCategory::Services,
            "ComputeTime" => ListingCategory::ComputeTime,
            "StorageSpace" => ListingCategory::StorageSpace,
            "MeshSites" => ListingCategory::MeshSites,
            _ => ListingCategory::Other,
        };
        listings.retain(|l| l.category == category);
    }

    // Pagination
    let page = params.page.unwrap_or(1).max(1);
    let limit = params.limit.unwrap_or(20).min(100);
    let skip = ((page - 1) * limit) as usize;
    let paginated: Vec<_> = listings.into_iter().skip(skip).take(limit as usize).collect();

    Json(json!({
        "listings": paginated,
        "page": page,
        "limit": limit,
        "total": store.get_active_listings().len(),
    }))
}

async fn api_get_listing(
    axum::extract::State(state): axum::extract::State<MarketplaceState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let store = state.store.lock().unwrap();

    match store.get_listing(&id) {
        Some(listing) => Json(json!({
            "success": true,
            "listing": listing,
        })),
        None => Json(json!({
            "success": false,
            "error": "Listing not found",
        })),
    }
}

async fn api_create_listing(
    axum::extract::State(state): axum::extract::State<MarketplaceState>,
    Json(req): Json<CreateListingRequest>,
) -> Json<Value> {
    if req.title.trim().is_empty() || req.description.trim().is_empty() {
        return Json(json!({
            "success": false,
            "error": "Title and description are required",
        }));
    }

    if req.price_credits.is_none() && req.price_hive.is_none() {
        return Json(json!({
            "success": false,
            "error": "At least one price must be specified",
        }));
    }

    let listing = MarketplaceListing::new(
        req.seller_peer_id,
        req.title,
        req.description,
        req.category,
        req.price_credits,
        req.price_hive,
        req.tags,
    );

    let listing_id = listing.id.clone();

    {
        let mut store = state.store.lock().unwrap();
        store.create_listing(listing);
        let _ = state.save();
    }

    Json(json!({
        "success": true,
        "listing_id": listing_id,
    }))
}

async fn api_update_listing(
    axum::extract::State(state): axum::extract::State<MarketplaceState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateListingRequest>,
) -> Json<Value> {
    let mut store = state.store.lock().unwrap();

    match store.get_listing(&id) {
        Some(mut listing) => {
            if let Some(title) = req.title {
                listing.title = title;
            }
            if let Some(desc) = req.description {
                listing.description = desc;
            }
            if let Some(pc) = req.price_credits {
                listing.price_credits = Some(pc);
            }
            if let Some(ph) = req.price_hive {
                listing.price_hive = Some(ph);
            }
            if let Some(tags) = req.tags {
                listing.tags = tags;
            }

            if store.update_listing(&id, listing) {
                let _ = state.save();
                Json(json!({
                    "success": true,
                    "listing_id": id,
                }))
            } else {
                Json(json!({
                    "success": false,
                    "error": "Failed to update listing",
                }))
            }
        }
        None => Json(json!({
            "success": false,
            "error": "Listing not found",
        })),
    }
}

async fn api_cancel_listing(
    axum::extract::State(state): axum::extract::State<MarketplaceState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let mut store = state.store.lock().unwrap();

    if store.cancel_listing(&id) {
        let _ = state.save();
        Json(json!({
            "success": true,
            "listing_id": id,
        }))
    } else {
        Json(json!({
            "success": false,
            "error": "Listing not found",
        }))
    }
}

async fn api_buy_listing(
    axum::extract::State(state): axum::extract::State<MarketplaceState>,
    Path(id): Path<String>,
    Json(req): Json<BuyListingRequest>,
) -> Json<Value> {
    let mut store = state.store.lock().unwrap();

    match store.get_listing(&id) {
        Some(listing) => {
            if listing.status != ListingStatus::Active {
                return Json(json!({
                    "success": false,
                    "error": "Listing is not available for purchase",
                }));
            }

            let price = if req.payment_type == "credits" {
                listing.price_credits
            } else {
                listing.price_hive
            };

            match price {
                Some(p) => {
                    store.mark_sold(&id);
                    let _ = state.save();
                    Json(json!({
                        "success": true,
                        "listing_id": id,
                        "seller_peer_id": listing.seller_peer_id,
                        "buyer_peer_id": req.buyer_peer_id,
                        "price": p,
                        "payment_type": req.payment_type,
                    }))
                }
                None => Json(json!({
                    "success": false,
                    "error": format!("Payment type '{}' not available for this listing", req.payment_type),
                })),
            }
        }
        None => Json(json!({
            "success": false,
            "error": "Listing not found",
        })),
    }
}

async fn api_add_review(
    axum::extract::State(state): axum::extract::State<MarketplaceState>,
    Path(id): Path<String>,
    Json(req): Json<ReviewRequest>,
) -> Json<Value> {
    if req.rating < 1 || req.rating > 5 {
        return Json(json!({
            "success": false,
            "error": "Rating must be between 1 and 5",
        }));
    }

    let review = Review {
        reviewer_peer_id: req.reviewer_peer_id,
        rating: req.rating,
        comment: req.comment,
        created_at: Utc::now().to_rfc3339(),
    };

    let mut store = state.store.lock().unwrap();

    if store.add_review(&id, review) {
        let _ = state.save();
        Json(json!({
            "success": true,
            "listing_id": id,
        }))
    } else {
        Json(json!({
            "success": false,
            "error": "Listing not found",
        }))
    }
}

async fn api_categories(
    axum::extract::State(state): axum::extract::State<MarketplaceState>,
) -> Json<Value> {
    let store = state.store.lock().unwrap();
    let active = store.get_active_listings();

    let categories: Vec<Value> = ListingCategory::all()
        .iter()
        .map(|cat| {
            let count = active.iter().filter(|l| l.category == *cat).count();
            json!({
                "name": cat.as_str(),
                "count": count,
            })
        })
        .collect();

    Json(json!({
        "categories": categories,
    }))
}

async fn api_search(
    axum::extract::State(state): axum::extract::State<MarketplaceState>,
    Query(params): Query<SearchParams>,
) -> Json<Value> {
    let store = state.store.lock().unwrap();
    let results = store.search(&params.q);

    Json(json!({
        "query": params.q,
        "results": results,
        "count": results.len(),
    }))
}

async fn api_seller_listings(
    axum::extract::State(state): axum::extract::State<MarketplaceState>,
    Path(peer_id): Path<String>,
) -> Json<Value> {
    let store = state.store.lock().unwrap();
    let listings = store.get_listings_by_seller(&peer_id);

    Json(json!({
        "seller_peer_id": peer_id,
        "listings": listings,
        "count": listings.len(),
    }))
}

async fn api_stats(
    axum::extract::State(state): axum::extract::State<MarketplaceState>,
) -> Json<Value> {
    let store = state.store.lock().unwrap();
    Json(store.get_stats())
}

async fn serve_marketplace_page() -> Html<&'static str> {
    Html(super::hive_marketplace_html::hive_marketplace_html())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marketplace_store_create_listing() {
        let mut store = MarketplaceStore::new();
        let listing = MarketplaceListing::new(
            "seller1".to_string(),
            "Test Item".to_string(),
            "A test item".to_string(),
            ListingCategory::DigitalGoods,
            Some(10.0),
            None,
            vec!["test".to_string()],
        );
        let id = listing.id.clone();

        store.create_listing(listing);
        assert_eq!(store.listings.len(), 1);
        assert!(store.get_listing(&id).is_some());
    }

    #[test]
    fn test_marketplace_store_get_active_listings() {
        let mut store = MarketplaceStore::new();

        let listing1 = MarketplaceListing::new(
            "seller1".to_string(),
            "Item 1".to_string(),
            "Description 1".to_string(),
            ListingCategory::DigitalGoods,
            Some(10.0),
            None,
            vec![],
        );

        let mut listing2 = MarketplaceListing::new(
            "seller1".to_string(),
            "Item 2".to_string(),
            "Description 2".to_string(),
            ListingCategory::Services,
            None,
            Some(5.0),
            vec![],
        );
        listing2.status = ListingStatus::Sold;

        store.create_listing(listing1);
        store.create_listing(listing2);

        let active = store.get_active_listings();
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn test_marketplace_store_search() {
        let mut store = MarketplaceStore::new();

        let listing1 = MarketplaceListing::new(
            "seller1".to_string(),
            "Laptop Computer".to_string(),
            "High-performance laptop".to_string(),
            ListingCategory::DigitalGoods,
            Some(500.0),
            None,
            vec!["electronics".to_string(), "computing".to_string()],
        );

        let listing2 = MarketplaceListing::new(
            "seller2".to_string(),
            "Web Design Service".to_string(),
            "Professional website design".to_string(),
            ListingCategory::Services,
            None,
            Some(100.0),
            vec!["design".to_string(), "web".to_string()],
        );

        store.create_listing(listing1);
        store.create_listing(listing2);

        let results = store.search("laptop");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Laptop Computer");

        let results = store.search("web");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Web Design Service");
    }

    #[test]
    fn test_marketplace_store_reviews() {
        let mut store = MarketplaceStore::new();
        let listing = MarketplaceListing::new(
            "seller1".to_string(),
            "Test Item".to_string(),
            "A test item".to_string(),
            ListingCategory::DigitalGoods,
            Some(10.0),
            None,
            vec![],
        );
        let id = listing.id.clone();

        store.create_listing(listing);

        let review1 = Review {
            reviewer_peer_id: "reviewer1".to_string(),
            rating: 5,
            comment: "Excellent!".to_string(),
            created_at: Utc::now().to_rfc3339(),
        };

        let review2 = Review {
            reviewer_peer_id: "reviewer2".to_string(),
            rating: 3,
            comment: "Good".to_string(),
            created_at: Utc::now().to_rfc3339(),
        };

        store.add_review(&id, review1);
        store.add_review(&id, review2);

        let listing = store.get_listing(&id).unwrap();
        assert_eq!(listing.reviews.len(), 2);
        assert_eq!(listing.average_rating(), 4.0);
    }

    #[test]
    fn test_marketplace_store_cancel_listing() {
        let mut store = MarketplaceStore::new();
        let listing = MarketplaceListing::new(
            "seller1".to_string(),
            "Test Item".to_string(),
            "A test item".to_string(),
            ListingCategory::DigitalGoods,
            Some(10.0),
            None,
            vec![],
        );
        let id = listing.id.clone();

        store.create_listing(listing);
        assert!(store.cancel_listing(&id));

        let listing = store.get_listing(&id).unwrap();
        assert_eq!(listing.status, ListingStatus::Cancelled);
    }

    #[test]
    fn test_marketplace_store_get_listings_by_seller() {
        let mut store = MarketplaceStore::new();

        for i in 1..=3 {
            let listing = MarketplaceListing::new(
                "seller1".to_string(),
                format!("Item {}", i),
                "Description".to_string(),
                ListingCategory::DigitalGoods,
                Some(10.0),
                None,
                vec![],
            );
            store.create_listing(listing);
        }

        let listing = MarketplaceListing::new(
            "seller2".to_string(),
            "Item 4".to_string(),
            "Description".to_string(),
            ListingCategory::Services,
            None,
            Some(5.0),
            vec![],
        );
        store.create_listing(listing);

        let seller1_listings = store.get_listings_by_seller("seller1");
        assert_eq!(seller1_listings.len(), 3);

        let seller2_listings = store.get_listings_by_seller("seller2");
        assert_eq!(seller2_listings.len(), 1);
    }

    #[test]
    fn test_marketplace_store_get_listings_by_category() {
        let mut store = MarketplaceStore::new();

        let listing1 = MarketplaceListing::new(
            "seller1".to_string(),
            "Digital Good".to_string(),
            "Description".to_string(),
            ListingCategory::DigitalGoods,
            Some(10.0),
            None,
            vec![],
        );

        let listing2 = MarketplaceListing::new(
            "seller1".to_string(),
            "Service".to_string(),
            "Description".to_string(),
            ListingCategory::Services,
            None,
            Some(5.0),
            vec![],
        );

        store.create_listing(listing1);
        store.create_listing(listing2);

        let digital = store.get_listings_by_category(&ListingCategory::DigitalGoods);
        assert_eq!(digital.len(), 1);

        let services = store.get_listings_by_category(&ListingCategory::Services);
        assert_eq!(services.len(), 1);
    }

    #[test]
    fn test_listing_average_rating() {
        let mut listing = MarketplaceListing::new(
            "seller1".to_string(),
            "Test Item".to_string(),
            "Description".to_string(),
            ListingCategory::DigitalGoods,
            Some(10.0),
            None,
            vec![],
        );

        assert_eq!(listing.average_rating(), 0.0);

        listing.reviews.push(Review {
            reviewer_peer_id: "r1".to_string(),
            rating: 5,
            comment: "Great".to_string(),
            created_at: Utc::now().to_rfc3339(),
        });

        listing.reviews.push(Review {
            reviewer_peer_id: "r2".to_string(),
            rating: 3,
            comment: "Okay".to_string(),
            created_at: Utc::now().to_rfc3339(),
        });

        assert_eq!(listing.average_rating(), 4.0);
    }
}
