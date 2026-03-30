/// Embedded HTML/CSS/JS for the HiveChat messaging platform.
///
/// Extracted from hive_chat.rs for module size management.

pub(crate) const CHAT_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>HiveChat — Decentralised Messaging</title>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap" rel="stylesheet">
    <style>
        *{margin:0;padding:0;box-sizing:border-box}
        :root{--bg:#1a1a2e;--surface:#16213e;--panel:#0f3460;--card:#1a1a40;--border:rgba(255,255,255,0.08);--text:#e8e8f0;--text-dim:#8888aa;--text-muted:#555577;--accent:#e94560;--accent-dim:rgba(233,69,96,0.15);--green:#53d769;--yellow:#f5c542;--blue:#5b7fff;--radius:12px}
        body{font-family:'Inter',sans-serif;background:var(--bg);color:var(--text);height:100vh;overflow:hidden;display:flex}

        /* Server List */
        .server-list{width:72px;background:#0a0a1a;display:flex;flex-direction:column;align-items:center;padding:12px 0;gap:8px;border-right:1px solid var(--border);overflow-y:auto;flex-shrink:0}
        .server-icon{width:48px;height:48px;border-radius:16px;background:var(--card);display:flex;align-items:center;justify-content:center;font-size:20px;cursor:pointer;transition:all .2s;border:2px solid transparent}
        .server-icon:hover{border-radius:12px;border-color:var(--accent)}
        .server-icon.active{border-color:var(--accent);border-radius:12px;background:var(--accent-dim)}
        .server-add{width:48px;height:48px;border-radius:50%;background:transparent;border:2px dashed var(--border);display:flex;align-items:center;justify-content:center;font-size:20px;color:var(--text-muted);cursor:pointer}
        .server-add:hover{border-color:var(--green);color:var(--green)}
        .server-divider{width:32px;height:2px;background:var(--border);border-radius:1px}

        /* Channel Sidebar */
        .channel-sidebar{width:240px;background:var(--surface);display:flex;flex-direction:column;border-right:1px solid var(--border);flex-shrink:0}
        .server-header{padding:14px 16px;font-weight:700;font-size:14px;border-bottom:1px solid var(--border);display:flex;justify-content:space-between;align-items:center}
        .server-header button{background:none;border:none;color:var(--text-dim);cursor:pointer;font-size:16px}
        .channel-list{flex:1;overflow-y:auto;padding:8px}
        .channel-category{font-size:10px;font-weight:700;color:var(--text-muted);text-transform:uppercase;letter-spacing:1px;padding:16px 8px 4px;display:flex;align-items:center;justify-content:space-between}
        .channel-item{padding:6px 8px;border-radius:6px;cursor:pointer;display:flex;align-items:center;gap:6px;font-size:13px;color:var(--text-dim);transition:background .15s}
        .channel-item:hover{background:rgba(255,255,255,0.05);color:var(--text)}
        .channel-item.active{background:var(--accent-dim);color:var(--text)}
        .channel-hash{color:var(--text-muted);font-weight:500}

        /* User Panel */
        .user-panel{padding:10px;border-top:1px solid var(--border);background:rgba(0,0,0,0.2);display:flex;align-items:center;gap:8px}
        .user-avatar{width:32px;height:32px;border-radius:50%;background:linear-gradient(135deg,var(--accent),var(--blue));display:flex;align-items:center;justify-content:center;font-size:14px;font-weight:700}
        .user-info{flex:1}
        .user-info .name{font-size:12px;font-weight:600}
        .user-info .status{font-size:10px;color:var(--green);display:flex;align-items:center;gap:4px}
        .status-dot{width:6px;height:6px;border-radius:50%;background:var(--green)}

        /* Chat Area */
        .chat-area{flex:1;display:flex;flex-direction:column;overflow:hidden}
        .chat-header{padding:12px 16px;border-bottom:1px solid var(--border);display:flex;align-items:center;gap:8px;background:rgba(0,0,0,0.1)}
        .chat-header .channel-name{font-weight:600;font-size:15px}
        .chat-header .topic{font-size:12px;color:var(--text-dim);margin-left:8px;border-left:1px solid var(--border);padding-left:8px}

        .messages{flex:1;overflow-y:auto;padding:16px}
        .message{display:flex;gap:12px;padding:4px 0;margin-bottom:4px;border-radius:8px;transition:background .15s}
        .message:hover{background:rgba(255,255,255,0.02)}
        .msg-avatar{width:40px;height:40px;border-radius:50%;background:linear-gradient(135deg,var(--accent),#ff6b6b);display:flex;align-items:center;justify-content:center;font-weight:700;font-size:14px;flex-shrink:0}
        .msg-body{flex:1;min-width:0}
        .msg-header{display:flex;align-items:baseline;gap:8px}
        .msg-author{font-weight:600;font-size:14px;color:var(--accent)}
        .msg-time{font-size:11px;color:var(--text-muted)}
        .msg-content{font-size:14px;line-height:1.5;color:var(--text);margin-top:2px;word-break:break-word;white-space:pre-wrap}
        .msg-reactions{display:flex;gap:4px;margin-top:4px;flex-wrap:wrap}
        .msg-react-btn{padding:2px 8px;border-radius:6px;border:1px solid var(--border);background:transparent;color:var(--text-dim);cursor:pointer;font-size:12px;transition:all .15s}
        .msg-react-btn:hover{background:var(--accent-dim);border-color:var(--accent)}
        .msg-reply{font-size:11px;color:var(--text-muted);padding:4px 8px;border-left:2px solid var(--accent);margin-bottom:4px}

        /* Message Input */
        .msg-input-area{padding:12px 16px;border-top:1px solid var(--border)}
        .msg-input-wrap{display:flex;align-items:center;background:var(--card);border-radius:8px;border:1px solid var(--border);padding:4px}
        .msg-input{flex:1;background:transparent;border:none;color:var(--text);font-family:inherit;font-size:14px;padding:8px 12px;outline:none}
        .msg-send{padding:8px 16px;border-radius:6px;border:none;background:var(--accent);color:#fff;font-weight:600;cursor:pointer;font-family:inherit;font-size:13px}
        .msg-send:hover{opacity:.9}

        /* Members */
        .member-list{width:240px;background:var(--surface);border-left:1px solid var(--border);padding:12px;overflow-y:auto;flex-shrink:0}
        .member-category{font-size:10px;font-weight:700;color:var(--text-muted);text-transform:uppercase;letter-spacing:1px;padding:8px 0 4px}
        .member-item{display:flex;align-items:center;gap:8px;padding:4px 0;cursor:pointer;border-radius:4px}
        .member-item:hover{background:rgba(255,255,255,0.03)}
        .member-avatar{width:28px;height:28px;border-radius:50%;background:var(--card);display:flex;align-items:center;justify-content:center;font-size:11px;font-weight:600;position:relative}
        .member-avatar .dot{position:absolute;bottom:-1px;right:-1px;width:10px;height:10px;border-radius:50%;border:2px solid var(--surface)}
        .member-name{font-size:13px;color:var(--text-dim)}
        .discord-badge{font-size:9px;color:var(--blue);background:rgba(91,127,255,0.15);padding:1px 4px;border-radius:3px}

        /* Day Separator */
        .day-sep{text-align:center;padding:16px 0;font-size:11px;color:var(--text-muted);position:relative}
        .day-sep span{background:var(--bg);padding:0 12px;position:relative;z-index:1}
        .day-sep::before{content:'';position:absolute;left:0;right:0;top:50%;height:1px;background:var(--border)}

        /* Scrollbar */
        ::-webkit-scrollbar{width:6px}::-webkit-scrollbar-track{background:transparent}::-webkit-scrollbar-thumb{background:var(--border);border-radius:3px}

        @media(max-width:900px){.member-list{display:none}}
        @media(max-width:700px){.channel-sidebar{display:none}}
    </style>
</head>
<body>
    <!-- Server List -->
    <div class="server-list" id="server-list"></div>

    <!-- Channel Sidebar -->
    <div class="channel-sidebar">
        <div class="server-header">
            <span id="server-name">🐝 HIVE</span>
            <button onclick="createChannel()" title="New Channel">+</button>
        </div>
        <div class="channel-list" id="channel-list"></div>
        <div class="user-panel">
            <div class="user-avatar" id="user-initial">?</div>
            <div class="user-info">
                <div class="name" id="user-name">Loading...</div>
                <div class="status"><div class="status-dot"></div> Online</div>
            </div>
            <button style="background:none;border:none;color:var(--text-muted);cursor:pointer;font-size:14px" onclick="linkDiscord()" title="Link Discord">🔗</button>
        </div>
    </div>

    <!-- Chat Area -->
    <div class="chat-area">
        <div class="chat-header">
            <span class="channel-hash">#</span>
            <span class="channel-name" id="current-channel-name">general</span>
            <span class="topic" id="current-topic">General discussion — say hi!</span>
        </div>
        <div class="messages" id="messages"></div>
        <div class="msg-input-area">
            <div class="msg-input-wrap">
                <input class="msg-input" id="msg-input" placeholder="Message #general" onkeydown="if(event.key==='Enter')sendMsg()">
                <button class="msg-send" onclick="sendMsg()">Send</button>
            </div>
        </div>
    </div>

    <!-- Members -->
    <div class="member-list" id="member-list"></div>

<script>
let currentServer = 'hive-main';
let currentChannel = 'hive-general';
let servers = [];
let channels = [];

async function loadServers() {
    const res = await fetch('/api/servers');
    const data = await res.json();
    servers = data.servers || [];
    const list = document.getElementById('server-list');
    list.innerHTML = servers.map(s => `
        <div class="server-icon ${s.id===currentServer?'active':''}" onclick="switchServer('${s.id}')" title="${esc(s.name)}">
            ${s.icon || s.name[0]}
        </div>
    `).join('') + '<div class="server-divider"></div><div class="server-add" onclick="createServer()">+</div>';
}

async function switchServer(id) {
    currentServer = id;
    const s = servers.find(x=>x.id===id);
    document.getElementById('server-name').textContent = s ? s.name : id;
    loadServers();
    await loadChannels();
    if (channels.length) switchChannel(channels[0].id);
}

async function loadChannels() {
    const res = await fetch(`/api/server/${currentServer}/channels`);
    const data = await res.json();
    channels = data.channels || [];
    const list = document.getElementById('channel-list');
    list.innerHTML = '<div class="channel-category">Text Channels</div>' +
        channels.map(c => `
            <div class="channel-item ${c.id===currentChannel?'active':''}" onclick="switchChannel('${c.id}')">
                <span class="channel-hash">#</span> ${esc(c.name)}
            </div>
        `).join('');
}

function switchChannel(id) {
    currentChannel = id;
    const ch = channels.find(c=>c.id===id);
    document.getElementById('current-channel-name').textContent = ch ? ch.name : id;
    document.getElementById('current-topic').textContent = ch ? ch.topic : '';
    document.getElementById('msg-input').placeholder = `Message #${ch ? ch.name : 'general'}`;
    loadChannels();
    loadMessages();
}

async function loadMessages() {
    const res = await fetch(`/api/channel/${currentChannel}/messages?limit=100`);
    const data = await res.json();
    const container = document.getElementById('messages');
    const msgs = data.messages || [];
    if (!msgs.length) {
        container.innerHTML = '<div style="text-align:center;padding:40px;color:var(--text-muted)"><p style="font-size:40px">👋</p><p>No messages yet. Say something!</p></div>';
        return;
    }

    container.innerHTML = '<div class="day-sep"><span>Today</span></div>' +
        msgs.map(m => {
            const init = (m.author_name||'?')[0].toUpperCase();
            const reactions = Object.entries(m.reactions||{}).map(([e,v]) =>
                `<button class="msg-react-btn" onclick="reactMsg('${m.id}','${e}')">${e} ${v.length}</button>`
            ).join('');
            const reply = m.reply_to ? `<div class="msg-reply">↩ reply</div>` : '';
            return `${reply}<div class="message">
                <div class="msg-avatar">${init}</div>
                <div class="msg-body">
                    <div class="msg-header">
                        <span class="msg-author">${esc(m.author_name)}</span>
                        <span class="msg-time">${timeAgo(m.timestamp)}</span>
                    </div>
                    <div class="msg-content">${esc(m.content)}</div>
                    <div class="msg-reactions">${reactions}
                        <button class="msg-react-btn" onclick="reactMsg('${m.id}','👍')">+</button>
                    </div>
                </div>
            </div>`;
        }).join('');
    container.scrollTop = container.scrollHeight;
}

async function sendMsg() {
    const input = document.getElementById('msg-input');
    const content = input.value.trim();
    if (!content) return;
    input.value = '';
    await fetch(`/api/channel/${currentChannel}/message`, {
        method: 'POST', headers: {'Content-Type':'application/json'},
        body: JSON.stringify({ content })
    });
}

async function reactMsg(msgId, emoji) {
    await fetch(`/api/message/${currentChannel}/${msgId}/react`, {
        method: 'POST', headers: {'Content-Type':'application/json'},
        body: JSON.stringify({ emoji })
    });
    loadMessages();
}

async function loadMembers() {
    const res = await fetch(`/api/server/${currentServer}/members`);
    const data = await res.json();
    const list = document.getElementById('member-list');
    const members = data.members || [];
    const online = members.filter(m=>m.status==='online');
    const offline = members.filter(m=>m.status!=='online');
    list.innerHTML = `<div class="member-category">Online — ${online.length}</div>` +
        online.map(m => memberHtml(m, true)).join('') +
        (offline.length ? `<div class="member-category">Offline — ${offline.length}</div>` +
        offline.map(m => memberHtml(m, false)).join('') : '');
}

function memberHtml(m, online) {
    const badge = m.discord_link ? `<span class="discord-badge">🔗 ${esc(m.discord_link)}</span>` : '';
    return `<div class="member-item">
        <div class="member-avatar">${(m.display_name||'?')[0].toUpperCase()}
            <div class="dot" style="background:${online?'var(--green)':'var(--text-muted)'}"></div>
        </div>
        <div><div class="member-name">${esc(m.display_name)}</div>${badge}</div>
    </div>`;
}

function createServer() {
    const name = prompt('Server name:');
    if (!name) return;
    const icon = prompt('Server icon (emoji):', '🌐') || '🌐';
    fetch('/api/servers', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({name, icon}) })
        .then(() => loadServers());
}

function createChannel() {
    const name = prompt('Channel name:');
    if (!name) return;
    fetch(`/api/server/${currentServer}/channels`, { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({name}) })
        .then(() => loadChannels());
}

function linkDiscord() {
    const username = prompt('Your Discord username (e.g. user#1234):');
    if (!username) return;
    fetch('/api/link-discord', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({discord_username: username}) })
        .then(() => loadMembers());
}

// SSE
const evtSource = new EventSource('/api/stream');
evtSource.onmessage = (e) => {
    try {
        const data = JSON.parse(e.data);
        if (data.type === 'message' && data.channel_id === currentChannel) loadMessages();
    } catch(err) {}
};

function timeAgo(ts) {
    const s = Math.floor((Date.now()-new Date(ts))/1000);
    if(s<60)return 'just now';if(s<3600)return Math.floor(s/60)+'m ago';
    if(s<86400)return Math.floor(s/3600)+'h ago';return Math.floor(s/86400)+'d ago';
}
function esc(t){if(!t)return'';const d=document.createElement('div');d.textContent=t;return d.innerHTML}

async function loadStatus() {
    const res = await fetch('/api/status');
    const data = await res.json();
    document.getElementById('user-name').textContent = data.display_name || 'Unknown';
    document.getElementById('user-initial').textContent = (data.display_name||'?')[0].toUpperCase();
}

// Boot
loadServers(); loadChannels().then(()=>loadMessages()); loadMembers(); loadStatus();
</script>
</body>
</html>"##;
