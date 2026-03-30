# HIVE Credits, Marketplace & Phase 5 — Implementation Plan

> **Date:** 2026-03-30
> **Scope:** Credits system, dynamic pricing, universal access, marketplaces, documentation, Phase 5 completion
> **Principle:** Zero stubs. Zero placeholders. Every module fully implemented, manually verifiable, and test-covered.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                     HIVE ECONOMY LAYER                          │
│                                                                 │
│  ┌─────────────┐  ┌──────────────┐  ┌───────────────────────┐ │
│  │ Credits     │  │ Dynamic      │  │ Universal Access      │ │
│  │ Engine      │  │ Pricing      │  │ Queue                 │ │
│  │             │  │              │  │                       │ │
│  │ earn()      │  │ compute_cost │  │ everyone gets access  │ │
│  │ spend()     │  │ network_cost │  │ credits = priority    │ │
│  │ balance()   │  │ supply/demand│  │ needs-based fallback  │ │
│  └──────┬──────┘  └──────┬───────┘  └───────────┬───────────┘ │
│         │                │                       │              │
│  ┌──────▼────────────────▼───────────────────────▼───────────┐ │
│  │              Credits Ledger (JSON-backed)                 │ │
│  │  Separate from HIVE Coin — non-crypto, no regulation     │ │
│  │  Internal points system, not a tradeable token            │ │
│  └───────────────────────────────────────────────────────────┘ │
│                                                                 │
│  ┌───────────────────────┐  ┌────────────────────────────────┐ │
│  │ Goods & Services      │  │ Crypto & NFT Marketplace       │ │
│  │ Marketplace (:3038)   │  │ (Enhanced Bank :3037)          │ │
│  │                       │  │                                │ │
│  │ List/browse/buy items │  │ HIVE Coin trading              │ │
│  │ Service mesh listings │  │ NFT gallery + auctions         │ │
│  │ Reviews & ratings     │  │ Trading card marketplace       │ │
│  │ Credits OR HIVE Coin  │  │ Wallet-to-wallet transfers     │ │
│  └───────────────────────┘  └────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

---

## Module 1: Credits Engine (`src/crypto/credits.rs`)

**Non-crypto internal points system.** Completely separate from HIVE Coin. No blockchain, no regulation concerns. Just a local JSON-backed ledger of earned/spent points.

### Data Structures

```rust
CreditLedger {
    accounts: HashMap<String, CreditAccount>,  // peer_id → account
    transactions: Vec<CreditTransaction>,
    config: CreditConfig,
}

CreditAccount {
    peer_id: String,
    balance: f64,
    lifetime_earned: f64,
    lifetime_spent: f64,
    contribution_streak: u32,      // consecutive days contributing
    last_contribution: String,     // ISO 8601
    reputation_score: f64,         // 0.0–1.0, from community votes
    earning_sources: HashMap<String, f64>,  // source → total earned from it
}

CreditTransaction {
    id: String,
    peer_id: String,
    amount: f64,                   // positive = earned, negative = spent
    source: CreditSource,
    timestamp: String,
    description: String,
}

enum CreditSource {
    ComputeProvided { tokens_served: u64, demand_multiplier: f64 },
    NetworkProvided { requests_relayed: u64, demand_multiplier: f64 },
    IdleContribution { hours_connected: f64 },
    CodeContribution { pr_id: String, lines_changed: u32 },
    SocialShare { platform: String, reference_url: String },
    CommunityVote { voter_id: String, positive: bool },
    GovernanceParticipation { proposal_id: String },
    ContentContribution { content_type: String },
    Spent { service: String },
}
```

### Earning Rules

| Activity | Base Credits | Multiplier Conditions |
|---|---|---|
| Providing compute (per 1K tokens served) | 2.0 | ×1.5 during high demand |
| Providing network relay (per 100 requests) | 1.0 | ×1.5 during high demand |
| Idle connection (per hour connected) | 0.5 | ×1.0 (flat) |
| Code contribution (merged PR) | 10.0 | ×1.0 per 100 lines changed |
| Social media share with reference | 3.0 | ×1.0 (flat, max 5/day) |
| Positive community vote received | 1.0 | ×1.0 (flat) |
| Governance vote cast | 2.0 | ×1.0 (flat) |
| Content contribution (lesson/routine) | 2.0 | ×1.0 (flat) |

### Spending Rules

| Service | Base Cost | Multiplier Conditions |
|---|---|---|
| Remote compute (per 1K tokens) | 1.0 | ×1.5 during high demand |
| Network relay (per 100 requests) | 0.5 | ×1.5 during high demand |
| Marketplace purchase | item price | ×1.0 (seller sets price) |
| Priority queue boost | 5.0 | ×1.0 (flat) |

### Key Methods

- `earn(peer_id, source, amount)` — credit the account
- `spend(peer_id, service, amount)` — debit the account (returns Ok/Err)
- `balance(peer_id)` — get current balance
- `history(peer_id, limit)` — transaction history
- `leaderboard(limit)` — top earners
- `demand_multiplier(resource_type)` — current demand-based price multiplier

---

## Module 2: Dynamic Pricing (`src/crypto/pricing.rs`)

Adjusts credit costs/rewards in real-time based on supply and demand.

### Algorithm

```
demand_ratio = active_requests / available_capacity

if demand_ratio > 0.8:  HIGH DEMAND
  earn_multiplier = 1.5   (providers earn more)
  cost_multiplier = 1.5   (consumers pay more)
elif demand_ratio > 0.5:  MODERATE
  earn_multiplier = 1.2
  cost_multiplier = 1.2
else:  LOW DEMAND
  earn_multiplier = 1.0
  cost_multiplier = 1.0
```

Separate demand tracking for compute and network. Updates every 60 seconds from PoolManager stats.

---

## Module 3: Universal Access Queue (`src/crypto/access_queue.rs`)

**Everyone gets to use the mesh, even with zero credits.** Credits buy priority, not access.

### Queue Tiers

1. **Priority** — Has credits, pays for immediate service
2. **Standard** — Has some credits, served in FIFO order
3. **Free** — Zero credits, served when capacity available, round-robin fair share

### Needs-Based Priority (within Free tier)

- Emergency alerts get instant access regardless of credits
- First-time users get a 100-credit welcome bonus
- Peers with high reputation scores get slight priority boost

### Key Methods

- `enqueue(peer_id, request_type, urgency)` — add to queue
- `dequeue()` — get next request to serve (respects priority)
- `position(peer_id)` — where you are in the queue
- `queue_stats()` — current queue depth per tier

---

## Module 4: Goods & Services Marketplace (`src/server/hive_marketplace.rs`)

A new mesh site on port **:3038** where users can list and trade goods and services.

### Data Model

```rust
MarketplaceListing {
    id: String,
    seller_peer_id: String,
    title: String,
    description: String,
    category: ListingCategory,
    price_credits: Option<f64>,      // price in credits (non-crypto)
    price_hive: Option<f64>,         // price in HIVE Coin (optional)
    images: Vec<String>,             // file paths
    created_at: String,
    updated_at: String,
    status: ListingStatus,           // Active, Sold, Cancelled
    reviews: Vec<Review>,
    tags: Vec<String>,
}

enum ListingCategory {
    DigitalGoods,      // files, templates, datasets
    Services,          // hosting, development, design
    ComputeTime,       // bulk compute blocks
    StorageSpace,      // disk space on peer
    MeshSites,         // pre-built mesh sites
    Other,
}

Review {
    reviewer_peer_id: String,
    rating: u8,         // 1-5
    comment: String,
    created_at: String,
}
```

### API Endpoints

```
GET  /api/listings              — browse all active listings (paginated, filterable)
GET  /api/listings/:id          — single listing detail
POST /api/listings              — create a new listing
PUT  /api/listings/:id          — update your listing
DELETE /api/listings/:id        — cancel your listing
POST /api/listings/:id/buy      — purchase with credits or HIVE
POST /api/listings/:id/review   — leave a review
GET  /api/categories            — list all categories
GET  /api/search?q=             — full-text search
GET  /api/seller/:peer_id       — all listings by a seller
```

### HTML Frontend

Full self-contained HTML page served at `/` on port 3038. Grid layout with category filters, search, listing cards with images, and purchase flow.

---

## Module 5: Enhanced Crypto & NFT Marketplace (Enhanced `src/server/hive_bank.rs`)

Expand the existing HIVE Bank to include:

### New API Endpoints

```
GET  /api/nft/marketplace       — all NFTs listed for sale (paginated)
POST /api/nft/list              — list an NFT for sale with custom price
POST /api/nft/delist            — remove from sale
POST /api/nft/auction           — start a timed auction
POST /api/nft/bid               — place a bid on an auction
GET  /api/nft/auctions          — active auctions
GET  /api/nft/history/:id       — ownership/price history for a card
GET  /api/credits/balance       — credits balance (separate from HIVE)
GET  /api/credits/history       — credits transaction history
GET  /api/credits/leaderboard   — top credit earners
GET  /api/credits/earn          — current earning rates & multipliers
POST /api/credits/spend         — spend credits on a service
```

### NFT Auctions

```rust
Auction {
    id: String,
    card_id: String,
    seller_peer_id: String,
    starting_price: f64,
    current_bid: f64,
    current_bidder: Option<String>,
    ends_at: String,
    bids: Vec<Bid>,
    status: AuctionStatus,   // Active, Completed, Cancelled
}
```

---

## Module 6: Integration Points

### Pool Manager Integration

Modify `src/network/pool.rs`:
- After `complete_job()` → call `credits.earn(provider, ComputeProvided {...})`
- After `pick_relay()` → call `credits.earn(relay_peer, NetworkProvided {...})`
- Before serving requests → check `access_queue` for priority ordering
- Add `demand_tracker` field for pricing module

### Compute Relay Integration

Modify `src/network/compute_relay.rs`:
- After successful job → credit the provider peer
- Track tokens served for credit calculation
- Report demand stats to pricing module

### HIVE Bank Integration

Modify `src/server/hive_bank.rs`:
- Add credits endpoints alongside wallet endpoints
- Credits balance shown alongside HIVE balance on dashboard
- Marketplace link in navigation

---

## Module 7: Documentation Updates

### Files to Update

1. **README.md** — Add Credits System, Marketplace, Phase 5 status sections
2. **USER_GUIDE.md** — Add credits earning guide, marketplace walkthrough, NFT auction guide
3. **SECURITY.md** — Document credits security (local-only, no exfiltration)
4. **meshnetworktodolist.md** — Mark completed items, add credits/marketplace items
5. **whitepaper.md** — Add Economy section covering credits + marketplace
6. **mastertestprompt.md** — Add test cases for credits, marketplace, auctions
7. **src/prompts/kernel.rs** — Add Law about credits system integrity
8. **.env.example** — Add all new environment variables

### New Documentation

1. **ECONOMY_GUIDE.md** — Complete credits + marketplace documentation
2. **MARKETPLACE_API.md** — Full REST API reference for both marketplaces

---

## Module 8: Phase 5 Completion

Based on the meshnetworktodolist.md step 5: "HivePortal live health + search aggregation"

### Tasks

1. **Live service health indicators** — Ping each port, show green/red/yellow status dots
2. **Mesh-wide search aggregation** — Search across Surface posts, Chat messages, Code files, Portal sites simultaneously
3. **Recent activity feed** — Aggregate latest content from all platforms into Portal homepage
4. **Bookmarks/Favourites** — Pin frequently visited mesh sites
5. **Quick access tiles** — Customisable grid of service shortcuts

---

## Implementation Order

| Step | Module | Files | Est. Lines |
|---|---|---|---|
| 1 | Credits Engine | `src/crypto/credits.rs` | ~400 |
| 2 | Dynamic Pricing | `src/crypto/pricing.rs` | ~200 |
| 3 | Access Queue | `src/crypto/access_queue.rs` | ~250 |
| 4 | Pool/Relay integration | modify `pool.rs`, `compute_relay.rs` | ~100 |
| 5 | Credits API in Bank | modify `hive_bank.rs` | ~200 |
| 6 | Goods Marketplace | `src/server/hive_marketplace.rs` + HTML | ~800 |
| 7 | NFT Marketplace expansion | modify `hive_bank.rs`, new `nft.rs` methods | ~300 |
| 8 | Phase 5 Portal features | modify `hive_portal.rs` | ~400 |
| 9 | Documentation | README, USER_GUIDE, etc. | ~500 |
| 10 | Tests & verification | test files, cargo check | ~200 |

**Total estimated: ~3,350 lines of fully implemented Rust + documentation**

---

## Environment Variables (New)

```env
# Credits System
REMOVED_MESH_GOVERNED=true
REMOVED_MESH_GOVERNED=100
REMOVED_MESH_GOVERNED=2.0
REMOVED_MESH_GOVERNED=1.0
REMOVED_MESH_GOVERNED=0.5
REMOVED_MESH_GOVERNED=1.5
REMOVED_MESH_GOVERNED=5

# Marketplace
REMOVED_MESH_GOVERNED=3038
REMOVED_MESH_GOVERNED=50
REMOVED_MESH_GOVERNED=10

# NFT Auctions
HIVE_AUCTION_MIN_DURATION_HOURS=1
HIVE_AUCTION_MAX_DURATION_HOURS=168
HIVE_AUCTION_SNIPE_PROTECTION_MINUTES=5
```

---

## Security Considerations

1. **Credits are local-only** — Never transmitted off-device. Peer-scoped. No chance of data leak.
2. **No crypto regulation risk** — Credits are not a tradeable token, not on any blockchain, cannot be exchanged for fiat. They are internal loyalty points.
3. **HIVE Coin remains separate** — The existing Solana-based HIVE Coin continues as-is for users who want real crypto. Credits are the free, accessible alternative.
4. **Marketplace isolation** — Each user's marketplace data is scoped to their peer. Listings propagate via mesh sync but purchases are peer-to-peer.
5. **No one sees anyone else's balance** — Credit balances are local. Leaderboards only show opted-in peers.
