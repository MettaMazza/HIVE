/// Embedded HTML/CSS/JS for the SafeNet Web Proxy dashboard.
///
/// Extracted from web_proxy.rs for module size management.

pub(crate) const PROXY_DASHBOARD: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>SafeNet Web Proxy</title>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600;700&display=swap" rel="stylesheet">
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: 'Inter', sans-serif;
            background: #0a0a0f;
            color: #e0e0e8;
            min-height: 100vh;
            padding: 32px;
        }
        .header {
            margin-bottom: 32px;
        }
        .header h1 {
            font-size: 28px;
            font-weight: 700;
            background: linear-gradient(135deg, #4caf50, #2196f3);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }
        .header p { color: #888; font-size: 14px; margin-top: 4px; }
        .stats {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 16px;
            margin-bottom: 32px;
        }
        .stat-card {
            background: rgba(255,255,255,0.03);
            border: 1px solid rgba(255,255,255,0.08);
            border-radius: 12px;
            padding: 20px;
        }
        .stat-card .label { font-size: 12px; color: #888; text-transform: uppercase; }
        .stat-card .value { font-size: 24px; font-weight: 600; margin-top: 4px; }
        .stat-card .value.green { color: #4caf50; }
        .stat-card .value.red { color: #f44336; }
        .stat-card .value.amber { color: #ff9800; }
        .fetch-box {
            background: rgba(255,255,255,0.03);
            border: 1px solid rgba(255,255,255,0.08);
            border-radius: 12px;
            padding: 24px;
            margin-bottom: 32px;
        }
        .fetch-box h2 { font-size: 16px; margin-bottom: 12px; color: #aaa; }
        .fetch-row {
            display: flex;
            gap: 10px;
        }
        .fetch-row input {
            flex: 1;
            padding: 10px 14px;
            background: rgba(0,0,0,0.4);
            border: 1px solid rgba(255,255,255,0.12);
            border-radius: 8px;
            color: #fff;
            font-family: monospace;
            font-size: 14px;
        }
        .fetch-row button {
            padding: 10px 20px;
            background: linear-gradient(135deg, #4caf50, #2196f3);
            border: none;
            border-radius: 8px;
            color: #fff;
            font-weight: 600;
            cursor: pointer;
        }
        #result {
            margin-top: 16px;
            padding: 16px;
            background: rgba(0,0,0,0.3);
            border-radius: 8px;
            font-family: monospace;
            font-size: 13px;
            max-height: 300px;
            overflow: auto;
            display: none;
        }
    </style>
</head>
<body>
    <div class="header">
        <h1>🌐 SafeNet Web Proxy</h1>
        <p>Censorship-resistant browsing through the HIVE mesh</p>
    </div>
    <div class="stats" id="stats">
        <div class="stat-card">
            <div class="label">Clearnet</div>
            <div class="value" id="clearnet-status">Checking...</div>
        </div>
        <div class="stat-card">
            <div class="label">Mesh Relay</div>
            <div class="value" id="relay-status">—</div>
        </div>
        <div class="stat-card">
            <div class="label">Cache</div>
            <div class="value" id="cache-count">0</div>
        </div>
        <div class="stat-card">
            <div class="label">DoH Resolver</div>
            <div class="value" id="doh-resolver" style="font-size:13px">—</div>
        </div>
    </div>
    <div class="fetch-box">
        <h2>Fetch URL</h2>
        <div class="fetch-row">
            <input type="text" id="url-input" placeholder="https://example.com" />
            <button onclick="fetchUrl()">Fetch</button>
        </div>
        <pre id="result"></pre>
    </div>
    <script>
        async function loadStatus() {
            try {
                const res = await fetch('/api/status');
                const s = await res.json();
                const cn = document.getElementById('clearnet-status');
                cn.textContent = s.clearnet_available ? '✅ Online' : '❌ Down';
                cn.className = 'value ' + (s.clearnet_available ? 'green' : 'red');
                
                const rl = document.getElementById('relay-status');
                rl.textContent = s.mesh_relay_enabled ? '✅ Active' : '❌ Off';
                rl.className = 'value ' + (s.mesh_relay_enabled ? 'green' : 'amber');
                
                document.getElementById('cache-count').textContent = s.cache_entries;
                document.getElementById('doh-resolver').textContent = s.doh_resolver.replace('https://', '');
            } catch(e) {
                document.getElementById('clearnet-status').textContent = 'Error';
            }
        }
        async function fetchUrl() {
            const url = document.getElementById('url-input').value;
            if (!url) return;
            const result = document.getElementById('result');
            result.style.display = 'block';
            result.textContent = 'Fetching...';
            try {
                const res = await fetch('/api/fetch', {
                    method: 'POST',
                    headers: {'Content-Type': 'application/json'},
                    body: JSON.stringify({ url })
                });
                const data = await res.json();
                result.textContent = `Source: ${data.source} | Status: ${data.status} | ${data.latency_ms}ms\n\n${data.body.substring(0, 2000)}`;
            } catch(e) {
                result.textContent = 'Fetch failed: ' + e.message;
            }
        }
        loadStatus();
        setInterval(loadStatus, 15000);
    </script>
</body>
</html>"##;
