# 🐝 HIVE Mesh Network — Complete Feature Parity Roadmap

> Every detail needed to make each platform a **1:1 match** with its real-world counterpart.
> Organised by platform, priority (P0=critical, P1=high, P2=medium, P3=nice-to-have), and estimated effort.

---

## 📊 Current Status (v4.7)

| Platform | Counterpart | Current Completion | Target |
|---|---|---|---|
| HivePortal | Google Homepage | ~40% | 100% |
| HiveSurface | Facebook+Reddit+Twitter+YouTube | ~35% | 100% |
| Apis Code | VS Code / Cursor | ~30% | 100% |
| HiveChat | Discord | ~25% | 100% |
| Mesh Site Builder | Squarespace | ~20% | 100% |

---

## 🏠 HivePortal (Google Homepage) — `:3035`

### Currently Implemented
- [x] Hero with search bar
- [x] Core services grid with port + descriptions
- [x] User mesh sites registry
- [x] Mesh status (online/offline, compute, relays)
- [x] "Build Your Own Site" CTA

### Missing for 1:1 Parity

#### P0 — Critical
- [ ] **Bookmarks/Favourites** — users can pin frequently visited mesh sites
- [ ] **Recent Activity Feed** — show latest posts from Surface, messages from Chat, new sites
- [ ] **Quick Access Tiles** — customisable grid (like Chrome new tab tiles)
- [ ] **Mesh-wide search that actually works** — currently searches site registry only; need to search Surface posts, Chat messages, Code files, and user sites simultaneously
- [ ] **Live service health indicators** — ping each port and show green/red dot per service

#### P1 — High
- [ ] **User authentication / identity** — persistent peer identity across all platforms (username, avatar, bio)
- [ ] **Notification centre** — aggregated notifications from all platforms (new messages, replies, mentions)
- [ ] **Weather/time widget** — mesh uptime, local time, peer connection time
- [ ] **Trending bar** — horizontal scroll of trending posts/topics from Surface
- [ ] **Site preview thumbnails** — iframe or screenshot previews of registered mesh sites

#### P2 — Medium
- [ ] **Dark/light theme toggle** — currently dark-only
- [ ] **Customisable layout** — drag-and-drop tile arrangement
- [ ] **Keyboard shortcuts** — `/` to focus search, `1-6` for quick service launch
- [ ] **Mesh map visualisation** — graphical display of connected peers
- [ ] **Portal widgets** — clock, calculator, notes, sticky notes

#### P3 — Nice to Have
- [ ] **Custom backgrounds** — user-uploadable portal backgrounds
- [ ] **Mesh DNS** — human-readable names for mesh sites (e.g., `bob.mesh` → `localhost:9001`)
- [ ] **Multi-language support** — i18n for the portal UI
- [ ] **RSS/feed aggregator** — pull feeds from mesh sites

---

## 🌐 HiveSurface (Facebook+Reddit+Twitter+YouTube) — `:3032`

### Currently Implemented
- [x] Post creation (text, link, alert, resource, AI activity)
- [x] Real-time SSE feed streaming
- [x] Emoji reactions (6 types)
- [x] Threaded replies
- [x] Communities (subreddits)
- [x] Full-text search
- [x] Trending (engagement-ranked, 24h)
- [x] User profiles by peer ID
- [x] Emergency governance alerts
- [x] Ring buffer (10K posts) + persistence

### Missing for 1:1 Parity

#### P0 — Critical (Facebook/Twitter Core)
- [ ] **Image/media uploads** — currently text-only; need image embedding in posts (stored locally, served via file server)
- [ ] **User profiles with avatars** — profile pictures, cover photos, bio, join date, post count
- [ ] **Follow/unfollow system** — follow users, personalised feed based on follows
- [ ] **Like counts displayed** — show total reaction counts prominently
- [ ] **Share/repost** — share another user's post to your feed with attribution
- [ ] **Edit posts** — edit your own posts after creation
- [ ] **Delete posts** — delete your own posts

#### P1 — High (Reddit Core)
- [ ] **Upvote/downvote system** — Reddit-style voting (separate from reactions)
- [ ] **Nested comment threads** — deep nesting (currently only 1-level replies)
- [ ] **Community creation UI** — form to create new communities with description, rules, icon
- [ ] **Community moderation** — mods can pin posts, remove content, set rules
- [ ] **User flair** — customisable text/emoji tags per community
- [ ] **Sorting** — sort by hot/new/top/controversial
- [ ] **Karma/reputation score** — visible per-user reputation based on votes

#### P2 — Medium (Twitter Features)
- [ ] **Character limit option** — optional tweet-length mode for quick posts
- [ ] **Hashtags** — clickable hashtags that filter posts
- [ ] **Mentions** — `@username` that links to profile and sends notification
- [ ] **Quote tweets** — repost with your own comment
- [ ] **Bookmarks** — save posts for later
- [ ] **Lists** — curated lists of users to follow
- [ ] **Polls** — create polls with 2-4 options, live results

#### P3 — Nice to Have (YouTube-like)
- [ ] **Video upload & playback** — embedded video player for mesh-hosted content
- [ ] **Live streaming** — peer-to-peer live video broadcast
- [ ] **Playlists** — curated media collections
- [ ] **Subscriptions feed** — chronological feed from subscribed creators
- [ ] **Comments with timestamps** — link comments to specific moments in media

---

## 💻 Apis Code (VS Code / Cursor) — `:3033`

### Currently Implemented
- [x] File explorer (recursive tree, icons, expand/collapse)
- [x] Multi-tab code editor with syntax highlighting
- [x] Line numbers
- [x] Tab indentation
- [x] Ctrl+S save
- [x] Unsaved indicator (•)
- [x] Integrated terminal with history
- [x] AI assistant panel (Apis chat)
- [x] File search (grep)
- [x] New file creation
- [x] Mesh Site Builder wizard

### Missing for 1:1 Parity

#### P0 — Critical
- [ ] **Find and Replace** — Ctrl+F in-editor search with regex support, replace, replace all
- [ ] **Multiple cursors** — Ctrl+D to select next occurrence, multi-cursor editing
- [ ] **Undo/Redo** — proper undo stack (currently relies on textarea)
- [ ] **File rename** — right-click rename in explorer
- [ ] **File drag-and-drop** — move files between folders
- [ ] **Auto-save** — configurable auto-save interval
- [ ] **Bracket matching** — highlight matching brackets/parens
- [ ] **Auto-close brackets** — auto-insert closing bracket/quote

#### P1 — High
- [ ] **Minimap** — VS Code-style minimap scrollbar on the right
- [ ] **Command Palette** — Ctrl+Shift+P command palette with fuzzy search
- [ ] **Go to file** — Ctrl+P quick file open with fuzzy search
- [ ] **Go to line** — Ctrl+G jump to line number
- [ ] **Code folding** — collapse/expand code blocks
- [ ] **Git integration** — show modified files, diffs, staged changes, commit
- [ ] **Split editor** — side-by-side file editing
- [ ] **Integrated debugger** — set breakpoints, step through code
- [ ] **Language Server Protocol** — real-time error checking, completions, go-to-definition
- [ ] **File type associations** — proper language detection and highlighting

#### P2 — Medium
- [ ] **Extensions/plugins** — installable editor extensions
- [ ] **Snippets** — code snippet insertion (e.g., `fn` → full function template)
- [ ] **Emmet support** — HTML/CSS shorthand expansion
- [ ] **Diff viewer** — side-by-side file comparison
- [ ] **Breadcrumbs** — file path breadcrumb navigation
- [ ] **Multiple terminals** — tabbed terminals, split terminal
- [ ] **Sticky scroll** — sticky function/class headers while scrolling
- [ ] **Settings UI** — graphical editor settings (theme, font size, tab size)
- [ ] **Workspace settings** — per-project configuration files

#### P3 — Nice to Have
- [ ] **Collaborative editing** — real-time CRDT-based multi-user editing
- [ ] **AI inline completions** — ghost text suggestions (Copilot-style)
- [ ] **AI refactoring** — select code → "Refactor with Apis"
- [ ] **AI commit messages** — auto-generate commit messages
- [ ] **Notebook mode** — Jupyter-style interactive code cells
- [ ] **Remote SSH** — edit files on remote mesh peers
- [ ] **Test runner UI** — visual test runner with pass/fail indicators

---

## 💬 HiveChat (Discord) — `:3034`

### Currently Implemented
- [x] Server list (create/join)
- [x] Text channels
- [x] Send messages
- [x] Emoji reactions
- [x] Reply/thread indicator
- [x] Direct messages
- [x] Member list with online/offline
- [x] SSE real-time streaming
- [x] Discord account linking
- [x] Welcome message
- [x] Content filter integration

### Missing for 1:1 Parity

#### P0 — Critical
- [ ] **Message editing** — edit your own messages
- [ ] **Message deletion** — delete your own messages
- [ ] **Message history pagination** — scroll up to load older messages (infinite scroll)
- [ ] **Typing indicators** — "Alice is typing..." in real-time
- [ ] **Unread message indicators** — bold channel name + badge count when unread
- [ ] **Channel switching preserves scroll** — maintain scroll position per channel
- [ ] **Notifications** — browser notifications for mentions and DMs
- [ ] **User avatars** — profile pictures (upload or generated from initials)
- [ ] **Timestamps per message** — full date/time on hover

#### P1 — High
- [ ] **Voice channels** — WebRTC-based voice chat (indicator in channel list)
- [ ] **Server invite links** — generate invite codes/links for others
- [ ] **Channel categories** — collapsible categories grouping channels
- [ ] **Roles and permissions** — admin, moderator, member roles with granular perms
- [ ] **Server settings** — name, icon, description, default channel
- [ ] **User settings** — notification preferences, theme, status (online/idle/DND/invisible)
- [ ] **Pinned messages** — pin important messages to channel
- [ ] **Search messages** — search within a server/channel
- [ ] **File uploads** — send images, files, documents in chat
- [ ] **Rich embeds** — link previews with title, description, image
- [ ] **Thread channels** — Discord-style thread creation from messages

#### P2 — Medium
- [ ] **Markdown support** — bold, italic, code blocks, spoilers in messages
- [ ] **Emoji picker** — visual grid of emojis (not just typing emoji characters)
- [ ] **Custom emojis** — server-specific custom emoji uploads
- [ ] **User presence** — "Playing a game", "Listening to...", custom status
- [ ] **Server boost** — mesh equivalent (contribute compute = boost perks)
- [ ] **Slow mode** — configurable rate limit per channel
- [ ] **NSFW channel flag** — age-gated channels
- [ ] **Channel permissions** — per-channel role overrides
- [ ] **Audit log** — log of server admin actions
- [ ] **Webhooks** — incoming webhooks for bot integration

#### P3 — Nice to Have
- [ ] **Video calls** — peer-to-peer video chat
- [ ] **Screen sharing** — share screen in voice channels
- [ ] **Stage channels** — audience/speaker style events
- [ ] **Forum channels** — Reddit-like threaded discussions within Discord
- [ ] **Scheduled events** — event creation with RSVP
- [ ] **AutoMod** — automatic moderation rules
- [ ] **Stickers/GIF search** — search and send GIFs
- [ ] **Server discovery** — browse public servers on the mesh
- [ ] **Nitro equivalent** — mesh contribution tiers with perks (larger uploads, custom emojis)
- [ ] **Bot framework** — create chat bots that respond to commands

---

## 🔧 Mesh Site Builder (Squarespace) — In Apis Code

### Currently Implemented
- [x] AI-powered site generation
- [x] 7 site types (blog, portfolio, forum, shop, landing, docs, gallery)
- [x] Auto-saves to mesh_sites/ folder
- [x] Publish to HivePortal
- [x] Self-contained HTML (no CDN)

### Missing for 1:1 Parity

#### P0 — Critical
- [ ] **Visual drag-and-drop editor** — WYSIWYG editor (not just AI-generated code)
- [ ] **Live preview** — iframe preview of the site as you edit
- [ ] **Template gallery** — pre-made templates users can browse and choose
- [ ] **Component library** — headers, footers, hero sections, contact forms, galleries
- [ ] **Site hosting** — actually serve the site on a mesh port (not file:// URL)

#### P1 — High
- [ ] **Domain mapping** — assign human-readable mesh addresses
- [ ] **Multi-page sites** — generate sites with multiple linked pages
- [ ] **Form handling** — contact forms with mesh-local email/storage
- [ ] **Blog engine** — create/edit/publish blog posts with date, tags, categories
- [ ] **E-commerce integration** — product pages, cart, checkout (mesh payments)
- [ ] **SEO tools** — meta tags editor, sitemap generation
- [ ] **Analytics** — visitor count, page views, referrers

#### P2 — Medium
- [ ] **Custom CSS editor** — visual style editor (colours, fonts, spacing)
- [ ] **Image gallery builder** — upload images, arrange in grid/masonry/carousel
- [ ] **Responsive preview** — desktop/tablet/mobile preview toggle
- [ ] **Version history** — revert to previous versions of the site
- [ ] **Password protection** — password-protect specific pages
- [ ] **Custom 404 page** — custom error pages

---

## 🔒 Cross-Platform Requirements

These features span all platforms and are essential for a cohesive mesh.

#### P0 — Critical
- [ ] **Unified identity system** — single username/avatar/bio across all platforms
- [ ] **Persistent sessions** — stay logged in across browser restarts (localStorage + token)
- [ ] **Cross-platform notifications** — notification bell in every platform's header
- [ ] **Mesh peer discovery** — auto-discover peers on local network (mDNS/multicast)
- [ ] **Data persistence** — all data survives HIVE restarts (currently some stores are in-memory only: HiveChat, HivePortal site registry)
- [ ] **Data sync between peers** — messages, posts, and sites propagate across mesh peers

#### P1 — High
- [ ] **Unified navigation bar** — consistent top bar across all platforms with platform switcher
- [ ] **Mobile responsive** — all platforms work on phone screens
- [ ] **Keyboard shortcuts documentation** — accessible help panel in every platform
- [ ] **Settings page** — unified settings for all platforms (theme, notifications, privacy)
- [ ] **Offline-first architecture** — all platforms work without any peers connected
- [ ] **Import/export** — export your data (posts, messages, files) as JSON/ZIP

#### P2 — Medium
- [ ] **End-to-end encryption** — optional E2E for DMs and private channels
- [ ] **Two-factor authentication** — TOTP-based 2FA for mesh identity
- [ ] **API documentation** — OpenAPI/Swagger docs for all endpoints
- [ ] **Accessibility** — ARIA labels, screen reader support, keyboard navigation
- [ ] **Internationalisation** — support for multiple languages

---

## 🏗️ Infrastructure & Architecture

#### P0 — Critical
- [ ] **HiveChat persistence** — save messages to disk (currently in-memory, lost on restart)
- [ ] **HivePortal persistence** — save registered sites to disk
- [ ] **WebSocket upgrade** — replace SSE with WebSocket for bidirectional real-time (better for typing indicators, voice signaling)
- [ ] **File upload system** — shared file upload endpoint for images, avatars, documents across all platforms
- [ ] **Static file server for mesh sites** — serve published mesh sites on actual HTTP ports (currently file:// URLs)

#### P1 — High
- [ ] **Database layer** — replace in-memory Vecs/HashMaps with SQLite for durability
- [ ] **Rate limiting per endpoint** — prevent API abuse
- [ ] **CORS policy hardening** — currently CorsLayer::permissive() on all servers
- [ ] **Health check endpoints** — `/health` on every platform for monitoring
- [ ] **Graceful shutdown** — persist all platform state on Ctrl+C

#### P2 — Medium
- [ ] **Metrics/telemetry** — Prometheus-compatible metrics per platform
- [ ] **Load testing** — benchmark each platform under high load
- [ ] **CI/CD pipeline** — automated testing on push
- [ ] **Docker container** — single-container deployment
- [ ] **Config file** — unified TOML config instead of env vars

---

## 📋 Priority Summary

| Priority | Count | Description |
|---|---|---|
| **P0** | 47 | Must-have for usable product |
| **P1** | 52 | Expected by users, high impact |
| **P2** | 42 | Polish and competitive features |
| **P3** | 24 | Nice-to-have, future features |
| **Total** | **165** | Features to full 1:1 parity |

## 🎯 Suggested Implementation Order

1. **Cross-platform identity + persistence** (enables everything else)
2. **HiveChat message persistence + editing + deletion** (most-used feature)
3. **HiveSurface media uploads + profiles** (visual impact)
4. **Apis Code find/replace + command palette** (developer productivity)
5. **HivePortal live health + search aggregation** (usability)
6. **HiveChat voice channels** (killer Discord feature)
7. **Site Builder visual editor + hosting** (Squarespace parity)
8. **Mobile responsive across all platforms** (accessibility)
9. **Cross-platform notifications** (cohesion)
10. **E2E encryption** (security promise)
