/// PWA — Progressive Web App support for HIVE mesh platforms.
///
/// Provides shared manifest generation, service worker, and
/// responsive meta tags for all mesh services.

use axum::{
    routing::get,
    Router,
    Json,
};
use serde_json::{Value, json};

/// Generate a PWA manifest for a given service.
pub fn manifest(name: &str, short_name: &str, icon_emoji: &str, color: &str, start_url: &str) -> Value {
    json!({
        "name": format!("HIVE — {}", name),
        "short_name": short_name,
        "description": format!("{} on the HIVE mesh network", name),
        "start_url": start_url,
        "display": "standalone",
        "background_color": "#0a0a0f",
        "theme_color": color,
        "orientation": "any",
        "icons": [
            {
                "src": format!("data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'><text y='.9em' font-size='90'>{}</text></svg>", icon_emoji),
                "sizes": "any",
                "type": "image/svg+xml",
                "purpose": "any maskable"
            }
        ],
        "categories": ["productivity", "social"],
        "lang": "en",
    })
}

/// Service worker JS that caches the app shell.
pub fn service_worker_js() -> &'static str {
    r#"
const CACHE = 'hive-mesh-v2';
const PRECACHE = ['/', '/api/status'];

self.addEventListener('install', e => {
    e.waitUntil(caches.open(CACHE).then(c => c.addAll(PRECACHE)));
    self.skipWaiting();
});

self.addEventListener('activate', e => {
    e.waitUntil(
        caches.keys().then(keys =>
            Promise.all(keys.filter(k => k !== CACHE).map(k => caches.delete(k)))
        )
    );
    self.clients.claim();
});

self.addEventListener('fetch', e => {
    if (e.request.method !== 'GET') return;
    // Network-first for API, cache-first for static
    if (e.request.url.includes('/api/')) {
        e.respondWith(
            fetch(e.request).then(r => {
                const clone = r.clone();
                caches.open(CACHE).then(c => c.put(e.request, clone));
                return r;
            }).catch(() => caches.match(e.request))
        );
    } else {
        e.respondWith(
            caches.match(e.request).then(r => r || fetch(e.request))
        );
    }
});
"#
}

/// HTML meta tags for responsive PWA behavior.
/// Insert this into the <head> of any platform SPA.
pub fn pwa_meta_tags(name: &str, color: &str) -> String {
    format!(r#"<meta name="viewport" content="width=device-width,initial-scale=1.0,maximum-scale=1.0,user-scalable=no">
<meta name="theme-color" content="{color}">
<meta name="apple-mobile-web-app-capable" content="yes">
<meta name="apple-mobile-web-app-status-bar-style" content="black-translucent">
<meta name="apple-mobile-web-app-title" content="{name}">
<link rel="manifest" href="/manifest.json">"#, name=name, color=color)
}

/// Shared responsive CSS utilities for all platforms.
pub fn responsive_css() -> &'static str {
    r#"
/* ─── HIVE Responsive Utilities ─── */
@media (max-width: 768px) {
    .sidebar, .side-panel { display: none !important; }
    .main-content { margin-left: 0 !important; width: 100% !important; }
    .grid-3 { grid-template-columns: 1fr !important; }
    .grid-2 { grid-template-columns: 1fr !important; }
    .hero h1 { font-size: 28px !important; }
    .topbar { padding: 8px 12px !important; }
    .hide-mobile { display: none !important; }
}
@media (max-width: 480px) {
    body { font-size: 14px; }
    .card, .post, .feature { padding: 16px !important; }
    .hero { padding: 40px 12px !important; }
}
@media (min-width: 769px) {
    .show-mobile-only { display: none !important; }
}
/* Touch-friendly tap targets */
button, a, .clickable { min-height: 44px; min-width: 44px; }
/* Safe area for notched devices */
body { padding: env(safe-area-inset-top) env(safe-area-inset-right) env(safe-area-inset-bottom) env(safe-area-inset-left); }
"#
}

/// Create PWA routes (manifest + service worker) for a service.
pub fn pwa_routes(name: &str, short_name: &str, icon: &str, color: &str) -> Router {
    let manifest_data = manifest(name, short_name, icon, color, "/");
    let sw_js = service_worker_js().to_string();

    Router::new()
        .route("/manifest.json", get(move || {
            let m = manifest_data.clone();
            async move { Json(m) }
        }))
        .route("/sw.js", get(move || {
            let js = sw_js.clone();
            async move {
                (
                    [("content-type", "application/javascript")],
                    js
                )
            }
        }))
        .route("/pwa/status", get(|| async {
            Json(json!({"pwa": true, "cache": "hive-mesh-v2"}))
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_structure() {
        let m = manifest("HiveChat", "Chat", "💬", "#ffc107", "/");
        assert_eq!(m["short_name"], "Chat");
        assert!(m["name"].as_str().unwrap().contains("HiveChat"));
        assert_eq!(m["display"], "standalone");
        assert_eq!(m["background_color"], "#0a0a0f");
        assert!(m["icons"].as_array().unwrap().len() > 0);
    }

    #[test]
    fn test_service_worker_not_empty() {
        let sw = service_worker_js();
        assert!(sw.len() > 100);
        assert!(sw.contains("caches"));
        assert!(sw.contains("fetch"));
        assert!(sw.contains("install"));
    }

    #[test]
    fn test_pwa_meta_tags() {
        let tags = pwa_meta_tags("TestApp", "#ff0000");
        assert!(tags.contains("theme-color"));
        assert!(tags.contains("#ff0000"));
        assert!(tags.contains("manifest.json"));
        assert!(tags.contains("apple-mobile-web-app-capable"));
    }

    #[test]
    fn test_responsive_css_contains_breakpoints() {
        let css = responsive_css();
        assert!(css.contains("768px"));
        assert!(css.contains("480px"));
        assert!(css.contains("safe-area-inset"));
    }

    #[test]
    fn test_manifest_self_contained() {
        let m = manifest("App", "A", "🐝", "#ffc107", "/");
        let icons = m["icons"].as_array().unwrap();
        // Icon must be data URI, not external
        let icon_src = icons[0]["src"].as_str().unwrap();
        assert!(icon_src.starts_with("data:"), "Icon must be inline data URI");
    }
}
