/// Embedded HTML for the HIVE Bank web portal.
/// Returns the full HTML page as a static string.

pub fn hive_bank_html() -> &'static str {
    r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>HIVE Bank — Decentralised Finance</title>
<meta name="description" content="HIVE Coin Banking Portal — wallet management, NFT trading cards, and decentralised finance on the mesh network.">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
<style>
:root {
    --bg-primary: #0a0a0f;
    --bg-secondary: #12121a;
    --bg-card: #1a1a2e;
    --bg-card-hover: #1f1f35;
    --accent-gold: #f5a623;
    --accent-gold-dim: rgba(245, 166, 35, 0.15);
    --accent-blue: #4a9eff;
    --accent-purple: #9b59b6;
    --accent-green: #2ecc71;
    --accent-red: #e74c3c;
    --text-primary: #e8e8f0;
    --text-secondary: #8888a0;
    --text-muted: #555570;
    --border: rgba(255,255,255,0.06);
    --glow-gold: 0 0 30px rgba(245, 166, 35, 0.15);
    --radius: 16px;
    --radius-sm: 10px;
}
* { margin: 0; padding: 0; box-sizing: border-box; }
body {
    font-family: 'Inter', -apple-system, BlinkMacSystemFont, sans-serif;
    background: var(--bg-primary);
    color: var(--text-primary);
    min-height: 100vh;
    overflow-x: hidden;
}
.bg-mesh {
    position: fixed; top: 0; left: 0; right: 0; bottom: 0; z-index: 0;
    background:
        radial-gradient(ellipse at 20% 20%, rgba(245,166,35,0.06) 0%, transparent 50%),
        radial-gradient(ellipse at 80% 80%, rgba(74,158,255,0.04) 0%, transparent 50%),
        radial-gradient(ellipse at 50% 50%, rgba(155,89,182,0.03) 0%, transparent 60%);
}
.container { max-width: 1200px; margin: 0 auto; padding: 0 24px; position: relative; z-index: 1; }
header {
    padding: 28px 0; display: flex; justify-content: space-between; align-items: center;
    border-bottom: 1px solid var(--border);
}
.logo { display: flex; align-items: center; gap: 14px; }
.logo-hex {
    width: 44px; height: 44px; background: linear-gradient(135deg, var(--accent-gold), #e8941e);
    clip-path: polygon(50% 0%, 100% 25%, 100% 75%, 50% 100%, 0% 75%, 0% 25%);
    display: flex; align-items: center; justify-content: center;
    font-size: 20px; color: #000; font-weight: 700;
}
.logo-text { font-size: 22px; font-weight: 700; letter-spacing: -0.5px; }
.logo-text span { color: var(--accent-gold); }
.nav-pills { display: flex; gap: 6px; background: var(--bg-secondary); border-radius: 12px; padding: 4px; }
.nav-pill {
    padding: 10px 20px; border-radius: 10px; cursor: pointer; font-size: 14px;
    font-weight: 500; color: var(--text-secondary); transition: all 0.3s ease;
    border: none; background: none; font-family: inherit;
}
.nav-pill:hover { color: var(--text-primary); }
.nav-pill.active { background: var(--bg-card); color: var(--accent-gold); box-shadow: var(--glow-gold); }
.status-bar {
    display: flex; gap: 24px; align-items: center; font-size: 13px; color: var(--text-muted);
}
.status-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--accent-green); display: inline-block; }

/* Hero Section */
.hero { text-align: center; padding: 60px 0 40px; }
.hero h1 { font-size: 48px; font-weight: 700; letter-spacing: -1.5px; margin-bottom: 16px; }
.hero h1 .gold { background: linear-gradient(135deg, var(--accent-gold), #ffd700); -webkit-background-clip: text; -webkit-text-fill-color: transparent; }
.hero p { color: var(--text-secondary); font-size: 18px; max-width: 550px; margin: 0 auto; line-height: 1.6; }

/* Stats Grid */
.stats-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 16px; margin: 40px 0; }
.stat-card {
    background: var(--bg-card); border: 1px solid var(--border); border-radius: var(--radius);
    padding: 24px; text-align: center; transition: all 0.3s ease;
}
.stat-card:hover { border-color: rgba(245,166,35,0.2); transform: translateY(-2px); box-shadow: var(--glow-gold); }
.stat-value { font-size: 32px; font-weight: 700; font-family: 'JetBrains Mono', monospace; }
.stat-value.gold { color: var(--accent-gold); }
.stat-value.blue { color: var(--accent-blue); }
.stat-value.purple { color: var(--accent-purple); }
.stat-value.green { color: var(--accent-green); }
.stat-label { font-size: 13px; color: var(--text-secondary); margin-top: 8px; text-transform: uppercase; letter-spacing: 1px; }

/* Sections */
.section { margin: 48px 0; }
.section-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 24px; }
.section-title { font-size: 22px; font-weight: 600; }
.section-title .emoji { margin-right: 10px; }

/* Wallet Panel */
.wallet-panel {
    background: var(--bg-card); border: 1px solid var(--border); border-radius: var(--radius);
    padding: 32px; display: grid; grid-template-columns: 2fr 1fr; gap: 32px;
}
.wallet-address {
    font-family: 'JetBrains Mono', monospace; font-size: 14px; color: var(--accent-blue);
    background: rgba(74,158,255,0.08); padding: 12px 16px; border-radius: var(--radius-sm);
    word-break: break-all; cursor: pointer; transition: all 0.2s;
}
.wallet-address:hover { background: rgba(74,158,255,0.15); }
.wallet-balance-row { display: flex; justify-content: space-between; align-items: baseline; margin: 16px 0; }
.balance-big { font-size: 42px; font-weight: 700; font-family: 'JetBrains Mono'; color: var(--accent-gold); }
.balance-label { font-size: 14px; color: var(--text-secondary); }
.wallet-actions { display: flex; gap: 12px; margin-top: 20px; }
.btn {
    padding: 12px 24px; border-radius: var(--radius-sm); font-size: 14px; font-weight: 600;
    cursor: pointer; transition: all 0.3s; border: none; font-family: inherit;
}
.btn-primary { background: linear-gradient(135deg, var(--accent-gold), #e8941e); color: #000; }
.btn-primary:hover { transform: translateY(-1px); box-shadow: 0 4px 20px rgba(245,166,35,0.3); }
.btn-secondary { background: var(--bg-secondary); color: var(--text-primary); border: 1px solid var(--border); }
.btn-secondary:hover { border-color: var(--accent-gold); }
.btn-icon { display: flex; align-items: center; gap: 8px; }

/* Quick Send Form */
.send-form { display: flex; flex-direction: column; gap: 12px; }
.input-group { display: flex; flex-direction: column; gap: 6px; }
.input-group label { font-size: 12px; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 1px; }
.input-field {
    background: var(--bg-primary); border: 1px solid var(--border); border-radius: var(--radius-sm);
    padding: 12px 16px; color: var(--text-primary); font-size: 14px; font-family: inherit;
    outline: none; transition: border-color 0.2s;
}
.input-field:focus { border-color: var(--accent-gold); }
.input-field::placeholder { color: var(--text-muted); }

/* Trading Cards */
.cards-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 20px; }
.nft-card {
    background: var(--bg-card); border: 1px solid var(--border); border-radius: var(--radius);
    overflow: hidden; transition: all 0.4s ease; cursor: pointer; position: relative;
}
.nft-card:hover { transform: translateY(-4px); border-color: rgba(245,166,35,0.3); box-shadow: var(--glow-gold); }
.nft-card .rarity-stripe {
    height: 4px; background: linear-gradient(90deg, var(--accent-gold), transparent);
}
.nft-card .rarity-stripe.common { background: linear-gradient(90deg, #888, transparent); }
.nft-card .rarity-stripe.uncommon { background: linear-gradient(90deg, var(--accent-blue), transparent); }
.nft-card .rarity-stripe.rare { background: linear-gradient(90deg, var(--accent-purple), transparent); }
.nft-card .rarity-stripe.legendary { background: linear-gradient(90deg, var(--accent-gold), #ffd700, transparent); }
.nft-card-img {
    width: 100%; height: 200px; object-fit: cover; background: var(--bg-secondary);
    display: flex; align-items: center; justify-content: center; font-size: 48px;
}
.nft-card-body { padding: 16px; }
.nft-card-name { font-size: 15px; font-weight: 600; margin-bottom: 8px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.nft-card-meta { display: flex; justify-content: space-between; align-items: center; }
.nft-card-rarity { font-size: 12px; padding: 4px 10px; border-radius: 20px; background: var(--accent-gold-dim); color: var(--accent-gold); }
.nft-card-price { font-family: 'JetBrains Mono'; font-size: 14px; font-weight: 600; color: var(--accent-gold); }
.nft-card-footer {
    padding: 12px 16px; border-top: 1px solid var(--border);
    display: flex; justify-content: space-between; align-items: center;
}
.nft-card-id { font-family: 'JetBrains Mono'; font-size: 11px; color: var(--text-muted); }

/* Transaction History */
.tx-table { width: 100%; border-collapse: collapse; }
.tx-table th { font-size: 12px; color: var(--text-muted); text-transform: uppercase; letter-spacing: 1px; padding: 12px 16px; text-align: left; border-bottom: 1px solid var(--border); }
.tx-table td { padding: 14px 16px; border-bottom: 1px solid var(--border); font-size: 14px; }
.tx-table tr:hover td { background: rgba(255,255,255,0.02); }
.tx-type { padding: 4px 10px; border-radius: 6px; font-size: 12px; font-weight: 500; }
.tx-type.mint { background: rgba(46,204,113,0.15); color: var(--accent-green); }
.tx-type.send { background: rgba(231,76,60,0.15); color: var(--accent-red); }
.tx-type.receive { background: rgba(74,158,255,0.15); color: var(--accent-blue); }
.tx-amount { font-family: 'JetBrains Mono'; }
.tx-hash { font-family: 'JetBrains Mono'; font-size: 12px; color: var(--text-muted); }

/* Empty States */
.empty-state { text-align: center; padding: 60px 20px; color: var(--text-secondary); }
.empty-state .icon { font-size: 48px; margin-bottom: 16px; }
.empty-state p { font-size: 15px; line-height: 1.6; }

/* Mode Badge */
.mode-badge {
    display: inline-flex; align-items: center; gap: 8px; padding: 6px 16px;
    border-radius: 20px; font-size: 12px; font-weight: 600; text-transform: uppercase; letter-spacing: 1px;
}
.mode-badge.simulation { background: rgba(245,166,35,0.12); color: var(--accent-gold); border: 1px solid rgba(245,166,35,0.3); }
.mode-badge.live { background: rgba(46,204,113,0.12); color: var(--accent-green); border: 1px solid rgba(46,204,113,0.3); }

/* Toast */
.toast {
    position: fixed; bottom: 30px; right: 30px; padding: 14px 24px;
    background: var(--bg-card); border: 1px solid var(--accent-gold);
    border-radius: var(--radius-sm); font-size: 14px; z-index: 1000;
    transform: translateY(100px); opacity: 0; transition: all 0.4s ease;
    box-shadow: var(--glow-gold);
}
.toast.visible { transform: translateY(0); opacity: 1; }

/* Responsive */
@media (max-width: 768px) {
    .stats-grid { grid-template-columns: repeat(2, 1fr); }
    .wallet-panel { grid-template-columns: 1fr; }
    .hero h1 { font-size: 32px; }
    .nav-pills { display: none; }
}
</style>
</head>
<body>
<div class="bg-mesh"></div>
<div class="container">
    <header>
        <div class="logo">
            <div class="logo-hex">🪙</div>
            <div class="logo-text">HIVE <span>Bank</span></div>
        </div>
        <div class="nav-pills" id="nav">
            <button class="nav-pill active" data-tab="wallet">💰 Wallet</button>
            <button class="nav-pill" data-tab="gallery">🎴 Gallery</button>
            <button class="nav-pill" data-tab="history">📜 History</button>
        </div>
        <div class="status-bar">
            <span id="mode-badge" class="mode-badge simulation">🔬 Simulation</span>
            <span><span class="status-dot"></span> Mesh Online</span>
        </div>
    </header>

    <div class="hero">
        <h1>Decentralised <span class="gold">Finance</span></h1>
        <p>Manage your HIVE Coin wallet, trade NFT cards, and track transactions — all on the mesh network.</p>
    </div>

    <div class="stats-grid">
        <div class="stat-card"><div class="stat-value gold" id="stat-balance">0.00</div><div class="stat-label">HIVE Balance</div></div>
        <div class="stat-card"><div class="stat-value blue" id="stat-sol">0.0000</div><div class="stat-label">SOL Balance</div></div>
        <div class="stat-card"><div class="stat-value purple" id="stat-cards">0</div><div class="stat-label">Cards Owned</div></div>
        <div class="stat-card"><div class="stat-value green" id="stat-supply">0</div><div class="stat-label">Total Supply</div></div>
    </div>

    <!-- Wallet Tab -->
    <div id="tab-wallet" class="tab-content">
        <div class="section">
            <div class="section-header"><h2 class="section-title"><span class="emoji">💰</span>Your Wallet</h2></div>
            <div class="wallet-panel">
                <div>
                    <div class="balance-label">HIVE COIN BALANCE</div>
                    <div class="wallet-balance-row">
                        <div class="balance-big" id="wallet-hive">0.00</div>
                        <div style="color:var(--text-secondary)">HIVE</div>
                    </div>
                    <div class="wallet-balance-row" style="margin-top:0">
                        <div style="font-size:18px;font-family:'JetBrains Mono';color:var(--accent-blue)" id="wallet-sol">0.0000</div>
                        <div style="color:var(--text-secondary)">SOL</div>
                    </div>
                    <div class="wallet-address" id="wallet-address" onclick="copyAddress()" title="Click to copy">Loading...</div>
                    <div class="wallet-actions">
                        <button class="btn btn-primary btn-icon" onclick="refreshBalances()">🔄 Refresh</button>
                        <button class="btn btn-secondary btn-icon" onclick="copyAddress()">📋 Copy Address</button>
                    </div>
                </div>
                <div>
                    <h3 style="font-size:16px;margin-bottom:16px;color:var(--text-secondary)">Quick Send</h3>
                    <div class="send-form">
                        <div class="input-group">
                            <label>Recipient</label>
                            <input class="input-field" id="send-to" placeholder="User ID or Solana address" />
                        </div>
                        <div class="input-group">
                            <label>Amount (HIVE)</label>
                            <input class="input-field" id="send-amount" type="number" step="0.01" placeholder="0.00" />
                        </div>
                        <button class="btn btn-primary" onclick="sendHive()" style="margin-top:8px">Send HIVE</button>
                    </div>
                </div>
            </div>
        </div>
    </div>

    <!-- Gallery Tab -->
    <div id="tab-gallery" class="tab-content" style="display:none">
        <div class="section">
            <div class="section-header">
                <h2 class="section-title"><span class="emoji">🎴</span>Trading Card Gallery</h2>
                <div id="gallery-stats" style="color:var(--text-muted);font-size:13px"></div>
            </div>
            <div class="cards-grid" id="cards-grid">
                <div class="empty-state">
                    <div class="icon">🎴</div>
                    <p>No cards minted yet.<br>Cards are auto-minted when Apis generates images.</p>
                </div>
            </div>
        </div>
    </div>

    <!-- History Tab -->
    <div id="tab-history" class="tab-content" style="display:none">
        <div class="section">
            <div class="section-header"><h2 class="section-title"><span class="emoji">📜</span>Transaction History</h2></div>
            <div style="background:var(--bg-card);border:1px solid var(--border);border-radius:var(--radius);overflow:hidden">
                <table class="tx-table" id="tx-table">
                    <thead><tr><th>Type</th><th>Amount</th><th>From</th><th>To</th><th>TX</th></tr></thead>
                    <tbody id="tx-body">
                        <tr><td colspan="5" class="empty-state"><div class="icon">📜</div><p>No transactions yet.</p></td></tr>
                    </tbody>
                </table>
            </div>
        </div>
    </div>
</div>

<div class="toast" id="toast"></div>

<script>
const API = '';

// Tab navigation
document.querySelectorAll('.nav-pill').forEach(pill => {
    pill.addEventListener('click', () => {
        document.querySelectorAll('.nav-pill').forEach(p => p.classList.remove('active'));
        pill.classList.add('active');
        document.querySelectorAll('.tab-content').forEach(t => t.style.display = 'none');
        document.getElementById('tab-' + pill.dataset.tab).style.display = 'block';
        if (pill.dataset.tab === 'gallery') loadGallery();
        if (pill.dataset.tab === 'history') loadHistory();
    });
});

function showToast(msg) {
    const t = document.getElementById('toast');
    t.textContent = msg;
    t.classList.add('visible');
    setTimeout(() => t.classList.remove('visible'), 3000);
}

function copyAddress() {
    const addr = document.getElementById('wallet-address').textContent;
    if (addr && addr !== 'Loading...') {
        navigator.clipboard.writeText(addr).then(() => showToast('📋 Address copied!'));
    }
}

async function refreshBalances() {
    try {
        const res = await fetch(API + '/api/wallet/balance');
        const data = await res.json();
        document.getElementById('wallet-hive').textContent = data.hive.toFixed(2);
        document.getElementById('wallet-sol').textContent = data.sol.toFixed(4);
        document.getElementById('wallet-address').textContent = data.address || 'No wallet';
        document.getElementById('stat-balance').textContent = data.hive.toFixed(2);
        document.getElementById('stat-sol').textContent = data.sol.toFixed(4);
        document.getElementById('stat-supply').textContent = data.total_supply?.toFixed(0) || '0';
        if (data.mode) {
            const badge = document.getElementById('mode-badge');
            badge.className = 'mode-badge ' + data.mode;
            badge.textContent = data.mode === 'simulation' ? '🔬 Simulation' : '🟢 Live';
        }
    } catch(e) { console.error('Balance fetch failed:', e); }
}

async function loadGallery() {
    try {
        const res = await fetch(API + '/api/gallery');
        const data = await res.json();
        const grid = document.getElementById('cards-grid');
        document.getElementById('stat-cards').textContent = data.owned_count || 0;
        if (!data.cards || data.cards.length === 0) {
            grid.innerHTML = '<div class="empty-state"><div class="icon">🎴</div><p>No cards minted yet.</p></div>';
            return;
        }
        document.getElementById('gallery-stats').textContent = data.cards.length + ' cards total';
        grid.innerHTML = data.cards.map(c => {
            const rclass = c.rarity.toLowerCase();
            const emoji = {Common:'⚪',Uncommon:'🔵',Rare:'💎',Legendary:'⭐'}[c.rarity]||'❓';
            return `<div class="nft-card" onclick="buyCard('${c.id}')">
                <div class="rarity-stripe ${rclass}"></div>
                <div class="nft-card-img">${emoji}</div>
                <div class="nft-card-body">
                    <div class="nft-card-name">${c.name}</div>
                    <div class="nft-card-meta">
                        <span class="nft-card-rarity">${emoji} ${c.rarity}</span>
                        <span class="nft-card-price">${c.price.toFixed(2)} HIVE</span>
                    </div>
                </div>
                <div class="nft-card-footer">
                    <span class="nft-card-id">${c.id.substring(0,8)}</span>
                    <span style="color:${c.for_sale?'var(--accent-green)':'var(--text-muted)'}">
                        ${c.for_sale?'For Sale':'Owned'}
                    </span>
                </div>
            </div>`;
        }).join('');
    } catch(e) { console.error('Gallery fetch failed:', e); }
}

async function loadHistory() {
    try {
        const res = await fetch(API + '/api/wallet/history');
        const data = await res.json();
        const tbody = document.getElementById('tx-body');
        if (!data.transactions || data.transactions.length === 0) {
            tbody.innerHTML = '<tr><td colspan="5" class="empty-state"><p>No transactions yet.</p></td></tr>';
            return;
        }
        tbody.innerHTML = data.transactions.map(tx => {
            const typeClass = tx.tx_type.includes('mint')?'mint':tx.tx_type.includes('send')||tx.tx_type.includes('transfer')?'send':'receive';
            return `<tr>
                <td><span class="tx-type ${typeClass}">${tx.tx_type}</span></td>
                <td class="tx-amount" style="color:var(--accent-gold)">${tx.amount.toFixed(2)} HIVE</td>
                <td style="font-family:'JetBrains Mono';font-size:12px">${tx.from.substring(0,12)}...</td>
                <td style="font-family:'JetBrains Mono';font-size:12px">${tx.to.substring(0,12)}...</td>
                <td class="tx-hash">${tx.id.substring(0,12)}...</td>
            </tr>`;
        }).join('');
    } catch(e) { console.error('History fetch failed:', e); }
}

async function sendHive() {
    const to = document.getElementById('send-to').value.trim();
    const amount = parseFloat(document.getElementById('send-amount').value);
    if (!to || !amount || amount <= 0) { showToast('⚠️ Enter recipient and amount'); return; }
    try {
        const res = await fetch(API + '/api/wallet/send', {
            method: 'POST',
            headers: {'Content-Type':'application/json'},
            body: JSON.stringify({to, amount})
        });
        const data = await res.json();
        if (data.success) {
            showToast('✅ Sent ' + amount.toFixed(2) + ' HIVE!');
            document.getElementById('send-to').value = '';
            document.getElementById('send-amount').value = '';
            refreshBalances();
        } else {
            showToast('❌ ' + (data.error || 'Send failed'));
        }
    } catch(e) { showToast('❌ Network error'); }
}

async function buyCard(cardId) {
    if (!confirm('Purchase this card?')) return;
    try {
        const res = await fetch(API + '/api/gallery/buy', {
            method: 'POST',
            headers: {'Content-Type':'application/json'},
            body: JSON.stringify({card_id: cardId})
        });
        const data = await res.json();
        if (data.success) {
            showToast('🎴 Card purchased!');
            loadGallery();
            refreshBalances();
        } else {
            showToast('❌ ' + (data.error || 'Purchase failed'));
        }
    } catch(e) { showToast('❌ Network error'); }
}

// Auto-load
refreshBalances();
setInterval(refreshBalances, 30000);
</script>
</body>
</html>"##
}
