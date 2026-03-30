/// Embedded HTML for the HIVE Marketplace web portal.
/// Returns the full HTML page as a static string.

pub fn hive_marketplace_html() -> &'static str {
    r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>HIVE Marketplace — Peer-to-Peer Commerce</title>
<meta name="description" content="HIVE Marketplace — Buy and sell goods and services on the mesh network.">
<style>
:root {
    --bg-primary: #0a0a0f;
    --bg-secondary: #12121a;
    --bg-card: #1a1a2e;
    --bg-card-hover: #1f1f35;
    --accent-gold: #f5a623;
    --accent-gold-dim: rgba(245, 166, 35, 0.15);
    --accent-blue: #4a9eff;
    --accent-blue-dim: rgba(74, 158, 255, 0.15);
    --accent-purple: #9b59b6;
    --accent-green: #2ecc71;
    --accent-red: #e74c3c;
    --text-primary: #e8e8f0;
    --text-secondary: #8888a0;
    --text-muted: #555570;
    --border: rgba(255,255,255,0.06);
    --glow-gold: 0 0 30px rgba(245, 166, 35, 0.15);
    --glow-blue: 0 0 30px rgba(74, 158, 255, 0.15);
    --radius: 16px;
    --radius-sm: 10px;
}

* {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
}

body {
    font-family: 'Segoe UI', system-ui, -apple-system, sans-serif;
    background: var(--bg-primary);
    color: var(--text-primary);
    min-height: 100vh;
    overflow-x: hidden;
}

.bg-mesh {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 0;
    background:
        radial-gradient(ellipse at 20% 20%, rgba(245,166,35,0.06) 0%, transparent 50%),
        radial-gradient(ellipse at 80% 80%, rgba(74,158,255,0.04) 0%, transparent 50%),
        radial-gradient(ellipse at 50% 50%, rgba(155,89,182,0.03) 0%, transparent 60%);
}

.container {
    max-width: 1400px;
    margin: 0 auto;
    padding: 0 24px;
    position: relative;
    z-index: 1;
}

header {
    padding: 24px 0;
    display: flex;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid var(--border);
    background: rgba(10, 10, 15, 0.8);
    backdrop-filter: blur(10px);
    position: sticky;
    top: 0;
    z-index: 100;
}

.logo {
    display: flex;
    align-items: center;
    gap: 12px;
    cursor: pointer;
    transition: transform 0.2s ease;
}

.logo:hover {
    transform: scale(1.02);
}

.logo-icon {
    width: 40px;
    height: 40px;
    background: linear-gradient(135deg, var(--accent-gold), #e8941e);
    clip-path: polygon(50% 0%, 100% 25%, 100% 75%, 50% 100%, 0% 75%, 0% 25%);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 18px;
    font-weight: 700;
    color: #000;
}

.logo-text {
    font-size: 20px;
    font-weight: 700;
}

.logo-text span {
    color: var(--accent-gold);
}

.header-actions {
    display: flex;
    gap: 12px;
    align-items: center;
}

.btn {
    padding: 10px 20px;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-size: 14px;
    font-weight: 500;
    transition: all 0.2s ease;
    font-family: inherit;
}

.btn-primary {
    background: linear-gradient(135deg, var(--accent-gold), #e8941e);
    color: #000;
}

.btn-primary:hover {
    transform: translateY(-2px);
    box-shadow: var(--glow-gold);
}

.btn-secondary {
    background: var(--bg-card);
    color: var(--text-primary);
    border: 1px solid var(--border);
}

.btn-secondary:hover {
    border-color: var(--accent-gold);
    color: var(--accent-gold);
}

.main-content {
    display: grid;
    grid-template-columns: 280px 1fr;
    gap: 24px;
    margin: 32px 0;
}

/* Sidebar */
.sidebar {
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 24px;
    height: fit-content;
    position: sticky;
    top: 100px;
}

.sidebar-section {
    margin-bottom: 24px;
}

.sidebar-title {
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    color: var(--text-secondary);
    letter-spacing: 1px;
    margin-bottom: 12px;
}

.category-item {
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-size: 14px;
    color: var(--text-secondary);
    transition: all 0.2s ease;
    display: flex;
    justify-content: space-between;
    align-items: center;
}

.category-item:hover {
    background: var(--bg-card-hover);
    color: var(--text-primary);
}

.category-item.active {
    background: var(--accent-gold-dim);
    color: var(--accent-gold);
}

.category-count {
    font-size: 12px;
    background: var(--bg-primary);
    padding: 2px 8px;
    border-radius: 4px;
}

/* Search */
.search-box {
    width: 100%;
    padding: 12px 16px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font-size: 14px;
    margin-bottom: 24px;
}

.search-box:focus {
    outline: none;
    border-color: var(--accent-gold);
    box-shadow: var(--glow-gold);
}

/* Listings Grid */
.listings-container {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 20px;
}

.listing-card {
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    transition: all 0.3s ease;
    cursor: pointer;
    display: flex;
    flex-direction: column;
}

.listing-card:hover {
    border-color: var(--accent-gold);
    transform: translateY(-4px);
    box-shadow: var(--glow-gold);
}

.listing-image {
    width: 100%;
    height: 200px;
    background: linear-gradient(135deg, rgba(245,166,35,0.1), rgba(74,158,255,0.1));
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 48px;
}

.listing-body {
    padding: 20px;
    flex: 1;
    display: flex;
    flex-direction: column;
}

.listing-category {
    font-size: 11px;
    color: var(--accent-blue);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 8px;
    font-weight: 600;
}

.listing-title {
    font-size: 16px;
    font-weight: 600;
    margin-bottom: 8px;
    line-height: 1.3;
    min-height: 40px;
}

.listing-description {
    font-size: 13px;
    color: var(--text-secondary);
    margin-bottom: 12px;
    line-height: 1.4;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
}

.listing-meta {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 12px;
    font-size: 12px;
    color: var(--text-secondary);
}

.listing-rating {
    display: flex;
    gap: 4px;
    align-items: center;
}

.star {
    color: var(--accent-gold);
}

.listing-price {
    font-size: 18px;
    font-weight: 700;
    color: var(--accent-green);
    margin-bottom: 12px;
}

.listing-price-unit {
    font-size: 12px;
    color: var(--text-secondary);
    margin-left: 4px;
}

.listing-actions {
    display: flex;
    gap: 8px;
}

.btn-buy {
    flex: 1;
    padding: 10px;
    background: linear-gradient(135deg, var(--accent-gold), #e8941e);
    color: #000;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-weight: 600;
    font-size: 13px;
    transition: all 0.2s ease;
}

.btn-buy:hover {
    transform: translateY(-2px);
    box-shadow: var(--glow-gold);
}

.btn-details {
    flex: 1;
    padding: 10px;
    background: var(--bg-primary);
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-weight: 600;
    font-size: 13px;
    transition: all 0.2s ease;
}

.btn-details:hover {
    border-color: var(--accent-blue);
    color: var(--accent-blue);
}

/* Modal */
.modal {
    display: none;
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.8);
    z-index: 1000;
    align-items: center;
    justify-content: center;
    padding: 20px;
}

.modal.open {
    display: flex;
}

.modal-content {
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    max-width: 600px;
    width: 100%;
    max-height: 90vh;
    overflow-y: auto;
    padding: 32px;
}

.modal-close {
    position: absolute;
    top: 20px;
    right: 20px;
    width: 36px;
    height: 36px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 50%;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 20px;
    color: var(--text-secondary);
    transition: all 0.2s ease;
}

.modal-close:hover {
    color: var(--accent-gold);
    border-color: var(--accent-gold);
}

.modal-header {
    font-size: 24px;
    font-weight: 700;
    margin-bottom: 24px;
}

.modal-section {
    margin-bottom: 24px;
}

.modal-label {
    font-size: 12px;
    font-weight: 700;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 1px;
    margin-bottom: 8px;
    display: block;
}

.form-input,
.form-textarea,
.form-select {
    width: 100%;
    padding: 12px 16px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font-size: 14px;
    font-family: inherit;
    margin-bottom: 12px;
}

.form-input:focus,
.form-textarea:focus,
.form-select:focus {
    outline: none;
    border-color: var(--accent-gold);
    box-shadow: 0 0 0 3px var(--accent-gold-dim);
}

.form-textarea {
    resize: vertical;
    min-height: 100px;
}

.form-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
}

.stats-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 16px;
    margin-bottom: 32px;
}

.stat-card {
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 24px;
    text-align: center;
}

.stat-value {
    font-size: 28px;
    font-weight: 700;
    color: var(--accent-gold);
    margin-bottom: 8px;
}

.stat-label {
    font-size: 12px;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 1px;
}

.empty-state {
    text-align: center;
    padding: 48px 24px;
    color: var(--text-secondary);
}

.empty-state-icon {
    font-size: 48px;
    margin-bottom: 16px;
    opacity: 0.5;
}

.status-badge {
    display: inline-block;
    padding: 4px 12px;
    border-radius: 20px;
    font-size: 12px;
    font-weight: 600;
}

.status-active {
    background: rgba(46, 204, 113, 0.2);
    color: var(--accent-green);
}

.status-sold {
    background: rgba(231, 76, 60, 0.2);
    color: var(--accent-red);
}

@media (max-width: 900px) {
    .main-content {
        grid-template-columns: 1fr;
    }

    .sidebar {
        position: static;
    }

    .listings-container {
        grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    }
}

@media (max-width: 640px) {
    .listing-card {
        flex-direction: row;
    }

    .listing-image {
        width: 120px;
        height: 120px;
        flex-shrink: 0;
    }

    .listings-container {
        grid-template-columns: 1fr;
    }

    .form-row {
        grid-template-columns: 1fr;
    }

    header {
        flex-direction: column;
        gap: 12px;
    }

    .header-actions {
        width: 100%;
    }

    .btn {
        flex: 1;
    }
}
</style>
</head>
<body>
<div class="bg-mesh"></div>

<header>
    <div class="logo" onclick="resetView()">
        <div class="logo-icon">🏪</div>
        <div class="logo-text">HIVE <span>Marketplace</span></div>
    </div>
    <div class="header-actions">
        <button class="btn btn-secondary" onclick="showCreateForm()">Create Listing</button>
        <button class="btn btn-primary" onclick="showStats()">Stats</button>
    </div>
</header>

<div class="container">
    <div class="main-content">
        <!-- Sidebar -->
        <aside class="sidebar">
            <div class="sidebar-section">
                <div class="sidebar-title">Search</div>
                <input type="text" class="search-box" id="searchInput" placeholder="Search listings..." onkeyup="handleSearch()">
            </div>

            <div class="sidebar-section">
                <div class="sidebar-title">Categories</div>
                <div id="categoriesList"></div>
            </div>

            <div class="sidebar-section">
                <button class="btn btn-secondary" style="width: 100%;" onclick="clearFilters()">Clear Filters</button>
            </div>
        </aside>

        <!-- Main content -->
        <main>
            <div id="listingsContainer" class="listings-container"></div>
            <div id="emptyState" class="empty-state" style="display: none;">
                <div class="empty-state-icon">📭</div>
                <p>No listings found</p>
            </div>
        </main>
    </div>
</div>

<!-- Detail Modal -->
<div id="detailModal" class="modal">
    <div style="position: relative; width: 100%; max-width: 600px;">
        <button class="modal-close" onclick="closeModal()">&times;</button>
        <div class="modal-content" id="detailContent"></div>
    </div>
</div>

<!-- Create Listing Modal -->
<div id="createModal" class="modal">
    <div style="position: relative; width: 100%; max-width: 600px;">
        <button class="modal-close" onclick="closeCreateModal()">&times;</button>
        <div class="modal-content">
            <div class="modal-header">Create Listing</div>
            <form onsubmit="submitListing(event)">
                <div class="modal-section">
                    <label class="modal-label">Your Peer ID</label>
                    <input type="text" class="form-input" id="sellerPeerId" placeholder="your-peer-id" required>
                </div>

                <div class="modal-section">
                    <label class="modal-label">Title</label>
                    <input type="text" class="form-input" id="listingTitle" placeholder="Item name" required>
                </div>

                <div class="modal-section">
                    <label class="modal-label">Description</label>
                    <textarea class="form-textarea" id="listingDesc" placeholder="Describe your item..." required></textarea>
                </div>

                <div class="modal-section">
                    <label class="modal-label">Category</label>
                    <select class="form-select" id="listingCategory" required>
                        <option value="DigitalGoods">Digital Goods</option>
                        <option value="Services">Services</option>
                        <option value="ComputeTime">Compute Time</option>
                        <option value="StorageSpace">Storage Space</option>
                        <option value="MeshSites">Mesh Sites</option>
                        <option value="Other">Other</option>
                    </select>
                </div>

                <div class="form-row">
                    <div>
                        <label class="modal-label">Price (Credits)</label>
                        <input type="number" class="form-input" id="priceCredits" placeholder="0.00" step="0.01">
                    </div>
                    <div>
                        <label class="modal-label">Price (HIVE)</label>
                        <input type="number" class="form-input" id="priceHive" placeholder="0.00" step="0.01">
                    </div>
                </div>

                <div class="modal-section">
                    <label class="modal-label">Tags (comma separated)</label>
                    <input type="text" class="form-input" id="listingTags" placeholder="tag1, tag2, tag3">
                </div>

                <button type="submit" class="btn btn-primary" style="width: 100%; padding: 14px;">Create Listing</button>
            </form>
        </div>
    </div>
</div>

<!-- Stats Modal -->
<div id="statsModal" class="modal">
    <div style="position: relative; width: 100%; max-width: 600px;">
        <button class="modal-close" onclick="closeStatsModal()">&times;</button>
        <div class="modal-content">
            <div class="modal-header">Marketplace Stats</div>
            <div id="statsContent"></div>
        </div>
    </div>
</div>

<script>
let currentListings = [];
let currentFilter = null;
let currentSearchQuery = null;

// Initialize
document.addEventListener('DOMContentLoaded', () => {
    loadListings();
    loadCategories();
});

async function loadListings(category = null) {
    try {
        let url = '/api/listings';
        if (category) {
            url += `?category=${encodeURIComponent(category)}`;
        }
        const response = await fetch(url);
        const data = await response.json();
        currentListings = data.listings;
        renderListings(currentListings);
    } catch (error) {
        console.error('Error loading listings:', error);
        showEmpty();
    }
}

async function loadCategories() {
    try {
        const response = await fetch('/api/categories');
        const data = await response.json();
        const categoriesList = document.getElementById('categoriesList');
        categoriesList.innerHTML = data.categories.map(cat => `
            <div class="category-item" onclick="filterByCategory('${cat.name}')">
                <span>${cat.name}</span>
                <span class="category-count">${cat.count}</span>
            </div>
        `).join('');
    } catch (error) {
        console.error('Error loading categories:', error);
    }
}

function renderListings(listings) {
    const container = document.getElementById('listingsContainer');
    const emptyState = document.getElementById('emptyState');

    if (listings.length === 0) {
        container.innerHTML = '';
        emptyState.style.display = 'block';
        return;
    }

    emptyState.style.display = 'none';
    container.innerHTML = listings.map(listing => `
        <div class="listing-card">
            <div class="listing-image">
                ${getCategoryEmoji(listing.category)}
            </div>
            <div class="listing-body">
                <div class="listing-category">${listing.category}</div>
                <h3 class="listing-title">${listing.title}</h3>
                <p class="listing-description">${listing.description}</p>
                <div class="listing-meta">
                    <div class="listing-rating">
                        <span>${listing.reviews.length} reviews</span>
                        ${listing.reviews.length > 0 ? `<span class="star">★</span><span>${listing.average_rating.toFixed(1)}</span>` : ''}
                    </div>
                    <span class="status-badge status-active">Active</span>
                </div>
                ${listing.price_credits ? `<div class="listing-price">${listing.price_credits.toFixed(2)}<span class="listing-price-unit">credits</span></div>` : ''}
                ${listing.price_hive ? `<div class="listing-price">${listing.price_hive.toFixed(2)}<span class="listing-price-unit">HIVE</span></div>` : ''}
                <div class="listing-actions">
                    <button class="btn-buy" onclick="showPurchaseFlow('${listing.id}')">Purchase</button>
                    <button class="btn-details" onclick="showListingDetail('${listing.id}')">Details</button>
                </div>
            </div>
        </div>
    `).join('');
}

function getCategoryEmoji(category) {
    const emojis = {
        'DigitalGoods': '💿',
        'Services': '🔧',
        'ComputeTime': '⚙️',
        'StorageSpace': '💾',
        'MeshSites': '🌐',
        'Other': '📦'
    };
    return emojis[category] || '📦';
}

async function showListingDetail(id) {
    try {
        const response = await fetch(`/api/listings/${id}`);
        const data = await response.json();
        if (!data.success) {
            alert('Listing not found');
            return;
        }

        const listing = data.listing;
        const detailContent = document.getElementById('detailContent');
        detailContent.innerHTML = `
            <div class="modal-header">${listing.title}</div>

            <div class="modal-section">
                <div style="text-align: center; font-size: 48px; margin: 20px 0;">
                    ${getCategoryEmoji(listing.category)}
                </div>
            </div>

            <div class="modal-section">
                <label class="modal-label">Category</label>
                <p>${listing.category}</p>
            </div>

            <div class="modal-section">
                <label class="modal-label">Description</label>
                <p>${listing.description}</p>
            </div>

            <div class="modal-section">
                <label class="modal-label">Seller</label>
                <p><code>${listing.seller_peer_id}</code></p>
            </div>

            <div class="modal-section">
                <label class="modal-label">Pricing</label>
                <p>
                    ${listing.price_credits ? `Credits: <strong>${listing.price_credits.toFixed(2)}</strong><br>` : ''}
                    ${listing.price_hive ? `HIVE: <strong>${listing.price_hive.toFixed(2)}</strong><br>` : ''}
                </p>
            </div>

            <div class="modal-section">
                <label class="modal-label">Tags</label>
                <p>${listing.tags.map(t => `<code>${t}</code>`).join(' ')}</p>
            </div>

            <div class="modal-section">
                <label class="modal-label">Reviews (${listing.reviews.length})</label>
                ${listing.reviews.length > 0 ? `
                    <div style="max-height: 200px; overflow-y: auto;">
                        ${listing.reviews.map(r => `
                            <div style="padding: 12px; background: var(--bg-primary); border-radius: 8px; margin-bottom: 8px;">
                                <div style="display: flex; justify-content: space-between;">
                                    <strong>${r.reviewer_peer_id}</strong>
                                    <span class="star">${'★'.repeat(r.rating)}</span>
                                </div>
                                <p style="font-size: 12px; margin-top: 4px; color: var(--text-secondary);">${r.comment}</p>
                            </div>
                        `).join('')}
                    </div>
                ` : '<p style="color: var(--text-secondary);">No reviews yet</p>'}
            </div>

            <div class="modal-section">
                <button class="btn btn-primary" style="width: 100%; padding: 12px;" onclick="showPurchaseFlow('${listing.id}')">
                    Purchase This Item
                </button>
            </div>
        `;

        document.getElementById('detailModal').classList.add('open');
    } catch (error) {
        console.error('Error loading listing:', error);
        alert('Failed to load listing');
    }
}

function showPurchaseFlow(id) {
    const buyer_peer_id = prompt('Enter your Peer ID:');
    if (!buyer_peer_id) return;

    const payment_type = confirm('Click OK for Credits, Cancel for HIVE');

    buyListing(id, buyer_peer_id, payment_type ? 'credits' : 'hive');
}

async function buyListing(id, buyer_peer_id, payment_type) {
    try {
        const response = await fetch(`/api/listings/${id}/buy`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                buyer_peer_id,
                payment_type
            })
        });

        const data = await response.json();
        if (data.success) {
            alert(`Purchase successful!\nPrice: ${data.price} ${data.payment_type}`);
            closeModal();
            loadListings();
        } else {
            alert(`Error: ${data.error}`);
        }
    } catch (error) {
        console.error('Error purchasing:', error);
        alert('Failed to complete purchase');
    }
}

async function submitListing(event) {
    event.preventDefault();

    const priceCredits = document.getElementById('priceCredits').value;
    const priceHive = document.getElementById('priceHive').value;

    if (!priceCredits && !priceHive) {
        alert('Please enter at least one price');
        return;
    }

    const tags = document.getElementById('listingTags').value
        .split(',')
        .map(t => t.trim())
        .filter(t => t.length > 0);

    try {
        const response = await fetch('/api/listings', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                seller_peer_id: document.getElementById('sellerPeerId').value,
                title: document.getElementById('listingTitle').value,
                description: document.getElementById('listingDesc').value,
                category: document.getElementById('listingCategory').value,
                price_credits: priceCredits ? parseFloat(priceCredits) : null,
                price_hive: priceHive ? parseFloat(priceHive) : null,
                tags
            })
        });

        const data = await response.json();
        if (data.success) {
            alert('Listing created successfully!');
            closeCreateModal();
            loadListings();
            loadCategories();
        } else {
            alert(`Error: ${data.error}`);
        }
    } catch (error) {
        console.error('Error creating listing:', error);
        alert('Failed to create listing');
    }
}

function filterByCategory(category) {
    currentFilter = category;
    currentSearchQuery = null;
    document.getElementById('searchInput').value = '';
    loadListings(category);
}

function handleSearch() {
    const query = document.getElementById('searchInput').value;
    currentSearchQuery = query;
    currentFilter = null;

    if (!query) {
        loadListings();
        return;
    }

    const filtered = currentListings.filter(listing =>
        listing.title.toLowerCase().includes(query.toLowerCase()) ||
        listing.description.toLowerCase().includes(query.toLowerCase()) ||
        listing.tags.some(t => t.toLowerCase().includes(query.toLowerCase()))
    );

    renderListings(filtered);
}

function clearFilters() {
    currentFilter = null;
    currentSearchQuery = null;
    document.getElementById('searchInput').value = '';
    loadListings();
    loadCategories();
}

function showCreateForm() {
    document.getElementById('createModal').classList.add('open');
}

function closeCreateModal() {
    document.getElementById('createModal').classList.remove('open');
    document.getElementById('sellerPeerId').value = '';
    document.getElementById('listingTitle').value = '';
    document.getElementById('listingDesc').value = '';
    document.getElementById('priceCredits').value = '';
    document.getElementById('priceHive').value = '';
    document.getElementById('listingTags').value = '';
}

function closeModal() {
    document.getElementById('detailModal').classList.remove('open');
}

async function showStats() {
    try {
        const response = await fetch('/api/stats');
        const stats = await response.json();

        const statsContent = document.getElementById('statsContent');
        statsContent.innerHTML = `
            <div class="stats-grid">
                <div class="stat-card">
                    <div class="stat-value">${stats.total_listings}</div>
                    <div class="stat-label">Total Listings</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">${stats.active_listings}</div>
                    <div class="stat-label">Active</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">${stats.sold_count}</div>
                    <div class="stat-label">Sold</div>
                </div>
            </div>

            <div class="modal-section">
                <label class="modal-label">By Category</label>
                <div>
                    ${stats.category_breakdown.map(([cat, count]) => `
                        <div style="padding: 8px 0; display: flex; justify-content: space-between;">
                            <span>${cat}</span>
                            <strong>${count}</strong>
                        </div>
                    `).join('')}
                </div>
            </div>
        `;

        document.getElementById('statsModal').classList.add('open');
    } catch (error) {
        console.error('Error loading stats:', error);
    }
}

function closeStatsModal() {
    document.getElementById('statsModal').classList.remove('open');
}

function resetView() {
    clearFilters();
    currentListings = [];
    loadListings();
}

function showEmpty() {
    document.getElementById('listingsContainer').innerHTML = '';
    document.getElementById('emptyState').style.display = 'block';
}

// Close modals on escape
document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
        closeModal();
        closeCreateModal();
        closeStatsModal();
    }
});

// Close modals on background click
document.getElementById('detailModal').addEventListener('click', (e) => {
    if (e.target.id === 'detailModal') closeModal();
});

document.getElementById('createModal').addEventListener('click', (e) => {
    if (e.target.id === 'createModal') closeCreateModal();
});

document.getElementById('statsModal').addEventListener('click', (e) => {
    if (e.target.id === 'statsModal') closeStatsModal();
});
</script>
</body>
</html>"##
}
