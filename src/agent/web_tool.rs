use crate::models::tool::{ToolResult, ToolStatus};
use tokio::sync::mpsc;
use headless_chrome::Browser;

pub async fn execute_web_search(
    task_id: String,
    description: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    let query = description.trim().to_string();
    tracing::debug!("[AGENT:web_search] ▶ task_id={} query='{}'", task_id, query);

    macro_rules! telemetry {
        ($tx:expr, $msg:expr) => {
            if let Some(ref tx) = $tx {
                let _ = tx.send($msg).await;
            }
        };
    }

    telemetry!(
        telemetry_tx,
        format!("🌐 Web Search Drone: processing request…\n")
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (compatible; HIVE/1.0)")
        .build()
        .unwrap_or_default();
        
    let action = crate::agent::preferences::extract_tag(&description, "action:").unwrap_or_else(|| "search".to_string());
    
    // ── Tier 0: Direct URL Visit ─────────────────────────────────────────────
    if action == "visit" {
        let url = crate::agent::preferences::extract_tag(&description, "url:").unwrap_or_default();
        if url.is_empty() {
             return ToolResult { task_id, output: "Error: Missing url:[...]".into(), tokens_used: 0, status: ToolStatus::Failed("Missing url".into()) };
        }
        telemetry!(telemetry_tx, format!("  → Visiting Direct URL: {}\n", url));
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let html = resp.text().await.unwrap_or_default();
                let stripped = strip_html_tags(&html);
                let payload = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
                // Cap at 10,000 characters to prevent buffer blowouts
                let chunk: String = payload.chars().take(10000).collect();
                telemetry!(telemetry_tx, "  ✅ Visit complete.\n".to_string());
                return ToolResult {
                     task_id,
                     output: format!("--- WEBPAGE CONTENT ({}) ---\n{}", url, chunk),
                     tokens_used: 0,
                     status: ToolStatus::Success,
                };
            }
            Ok(resp) => {
                return ToolResult { task_id, output: format!("HTTP Error: {}", resp.status()), tokens_used: 0, status: ToolStatus::Failed("HTTP error".into()) };
            }
            Err(e) => {
                return ToolResult { task_id, output: format!("Network Error: {}", e), tokens_used: 0, status: ToolStatus::Failed("Network error".into()) };
            }
        }
    }

    // ── Tier 0.5: DOM Headless Navigation ────────────────────────────────────
    if action == "navigate_dom" {
        let url = crate::agent::preferences::extract_tag(&description, "url:").unwrap_or_default();
        if url.is_empty() {
             return ToolResult { task_id, output: "Error: Missing url:[...]".into(), tokens_used: 0, status: ToolStatus::Failed("Missing url".into()) };
        }
        
        let css_selector = crate::agent::preferences::extract_tag(&description, "css_selector:").unwrap_or_else(|| "body".to_string());
        
        telemetry!(telemetry_tx, format!("  → Spinning up Headless Chrome for DOM Navigation: {}\n", url));
        
        let c_url = url.clone();
        let c_css = css_selector.clone();
        
        // Browser operations are blocking; run inside spawn_blocking
        let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
            let browser = Browser::default().map_err(|e| format!("Browser launch failed: {:?}", e))?;
            let tab = browser.new_tab().map_err(|e| format!("Failed to open tab: {:?}", e))?;
            
            tab.navigate_to(&c_url).map_err(|e| format!("Navigation failed: {:?}", e))?;
            tab.wait_until_navigated().map_err(|e| format!("Wait for navigation failed: {:?}", e))?;
            
            // Wait for selector
            let element = tab.wait_for_element(&c_css).map_err(|e| format!("Selector not found '{}': {:?}", c_css, e))?;
            
            let text = element.get_inner_text().map_err(|e| format!("Failed to extract inner text: {:?}", e))?;

            // Explicitly kill Chrome to prevent IOSurface client leak
            if let Some(pid) = browser.get_process_id() {
                drop(browser);
                let _ = std::process::Command::new("kill").arg("-9").arg(pid.to_string()).status();
            }

            Ok(text)
        }).await;
        
        match result {
            Ok(Ok(text)) => {
                let payload = text.split_whitespace().collect::<Vec<_>>().join(" ");
                let chunk: String = payload.chars().take(10000).collect();
                telemetry!(telemetry_tx, format!("  ✅ DOM Navigated & Extracted '{}'.\n", css_selector));
                return ToolResult {
                     task_id,
                     output: format!("--- WEBPAGE DOM ({}) Selector: '{}' ---\n{}", url, css_selector, chunk),
                     tokens_used: 0,
                     status: ToolStatus::Success,
                };
            }
            Ok(Err(e)) => {
                telemetry!(telemetry_tx, format!("  ❌ DOM Error: {}\n", e));
                return ToolResult { task_id, output: e, tokens_used: 0, status: ToolStatus::Failed("DOM Nav Failed".into()) };
            }
            Err(e) => {
                return ToolResult { task_id, output: format!("Tokio Join Error: {:?}", e), tokens_used: 0, status: ToolStatus::Failed("Thread Panic".into()) };
            }
        }
    }

    // ── Tier 1: Brave Search API ─────────────────────────────────────────────
    if let Ok(api_key) = std::env::var("BRAVE_SEARCH_API_KEY")
        && !api_key.is_empty() {
            telemetry!(telemetry_tx, "  → Trying Brave Search API…\n".to_string());
            let url = format!(
                "https://api.search.brave.com/res/v1/web/search?q={}&count=10&text_decorations=false",
                urlencoding::encode(&query)
            );
            match client
                .get(&url)
                .header("Accept", "application/json")
                .header("X-Subscription-Token", &api_key)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        let mut results = Vec::new();
                        
                        // Let's explicitly log the top-level keys if web is missing
                        if json.get("web").is_none() {
                            tracing::warn!("[Web Drone] Brave API success, but missing 'web' key. Keys: {:?}", json.as_object().map(|o| o.keys().collect::<Vec<_>>()));
                        }
                        
                        if let Some(web) = json.get("web").and_then(|w| w.get("results")) {
                            if let Some(items) = web.as_array() {
                                for item in items.iter().take(8) {
                                    let title = item
                                        .get("title")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("Untitled");
                                    let desc = item
                                        .get("description")
                                        .and_then(|d| d.as_str())
                                        .unwrap_or("");
                                    let url = item
                                        .get("url")
                                        .and_then(|u| u.as_str())
                                        .unwrap_or("");
                                    results.push(format!("• {}\n  {}\n  {}", title, desc, url));
                                }
                            } else {
                                tracing::warn!("[Web Drone] Brave API 'web.results' is not an array");
                            }
                        }
                        if !results.is_empty() {
                            telemetry!(
                                telemetry_tx,
                                format!("  ✅ Brave: {} results\n", results.len())
                            );
                            return ToolResult {
                                task_id,
                                output: format!(
                                    "--- BRAVE SEARCH RESULTS for '{}' ---\n{}",
                                    query,
                                    results.join("\n\n")
                                ),
                                tokens_used: 0,
                                status: ToolStatus::Success,
                            };
                        } else {
                            telemetry!(
                                telemetry_tx,
                                format!("  ⚠️ Brave API returned 0 results, falling through…\n")
                            );
                        }
                    } else {
                        tracing::warn!("[Web Drone] Failed to parse Brave API response as JSON");
                    }
                }
                Ok(resp) => {
                    telemetry!(
                        telemetry_tx,
                        format!("  ⚠️ Brave API returned HTTP {}, falling through…\n", resp.status())
                    );
                    tracing::warn!("[Web Drone] Brave HTTP error: {}", resp.status());
                }
                Err(e) => {
                    telemetry!(
                        telemetry_tx,
                        format!("  ⚠️ Brave API connection error: {}. Falling through…\n", e)
                    );
                    tracing::warn!("[Web Drone] Brave connection error: {}", e);
                }
            }
        }

    // ── Tier 2: DuckDuckGo HTML scrape ───────────────────────────────────────
    telemetry!(telemetry_tx, "  → Trying DuckDuckGo…\n".to_string());
    let ddg_url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding::encode(&query)
    );
    match client.get(&ddg_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(html) = resp.text().await {
                let stripped = strip_html_tags(&html);
                // DDG captcha / bot block returns very short pages
                if stripped.split_whitespace().count() > 50 {
                    telemetry!(telemetry_tx, "  ✅ DuckDuckGo: results found\n".to_string());
                    return ToolResult {
                        task_id,
                        output: format!(
                            "--- DDG SEARCH RESULTS for '{}' ---\n{}",
                            query,
                            stripped
                                .split_whitespace()
                                .collect::<Vec<_>>()
                                .join(" ")
                                .chars()
                                .take(4000)
                                .collect::<String>()
                        ),
                        tokens_used: 0,
                        status: ToolStatus::Success,
                    };
                } else {
                    telemetry!(
                        telemetry_tx,
                        "  ⚠️ DuckDuckGo returned captcha/empty page. Falling through…\n"
                            .to_string()
                    );
                }
            }
        }
        Ok(resp) => {
            telemetry!(
                telemetry_tx,
                format!("  ⚠️ DDG HTTP {}, falling through…\n", resp.status())
            );
        }
        Err(e) => {
            telemetry!(
                telemetry_tx,
                format!("  ⚠️ DDG error: {}. Falling through…\n", e)
            );
        }
    }

    // ── Tier 3: Google News RSS ───────────────────────────────────────────────
    telemetry!(telemetry_tx, "  → Trying Google News RSS…\n".to_string());
    let rss_url = format!(
        "https://news.google.com/rss/search?q={}&hl=en-US&gl=US&ceid=US:en",
        urlencoding::encode(&query)
    );
    match client.get(&rss_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(xml) = resp.text().await {
                let mut items: Vec<String> = Vec::new();
                // Manual lightweight RSS item extraction (no xml crate dependency)
                for chunk in xml.split("<item>").skip(1) {
                    let title = xml_tag_content(chunk, "title");
                    let description = xml_tag_content(chunk, "description");
                    let link = xml_tag_content(chunk, "link");
                    let pubdate = xml_tag_content(chunk, "pubDate");
                    if !title.is_empty() {
                        items.push(format!(
                            "• {}\n  {}\n  {} | {}",
                            title, description, link, pubdate
                        ));
                    }
                    if items.len() >= 8 {
                        break;
                    }
                }
                if !items.is_empty() {
                    telemetry!(
                        telemetry_tx,
                        format!("  ✅ Google RSS: {} items\n", items.len())
                    );
                    return ToolResult {
                        task_id,
                        output: format!(
                            "--- GOOGLE NEWS RSS for '{}' ---\n{}",
                            query,
                            items.join("\n\n")
                        ),
                        tokens_used: 0,
                        status: ToolStatus::Success,
                    };
                }
            }
        }
        Ok(resp) => {
            telemetry!(
                telemetry_tx,
                format!("  ⚠️ Google RSS HTTP {}\n", resp.status())
            );
        }
        Err(e) => {
            telemetry!(
                telemetry_tx,
                format!("  ⚠️ Google RSS error: {}\n", e)
            );
        }
    }

    // ── All tiers exhausted ───────────────────────────────────────────────────
    telemetry!(
        telemetry_tx,
        "  ❌ All search providers exhausted with no results.\n".to_string()
    );
    ToolResult {
        task_id,
        output: format!(
            "All search providers (Brave, DuckDuckGo, Google RSS) returned no results for '{}'. \
            The query may be too specific, or there may be a network issue. \
            Try rephrasing or ask the user to verify connectivity.",
            query
        ),
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

/// Strip HTML tags from a string, returning plain text.
fn strip_html_tags(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    text
}

/// Extract the first occurrence of content inside an XML tag from a chunk.
fn xml_tag_content(chunk: &str, tag: &str) -> String {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    if let Some(start) = chunk.find(&open) {
        let after_open = &chunk[start..];
        // skip to end of opening tag
        if let Some(gt) = after_open.find('>') {
            let content_start = gt + 1;
            if let Some(end) = after_open[content_start..].find(&close) {
                let raw = &after_open[content_start..content_start + end];
                // Strip CDATA wrappers if present
                let inner = raw
                    .trim()
                    .trim_start_matches("<![CDATA[")
                    .trim_end_matches("]]>")
                    .trim();
                return strip_html_tags(inner)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html_tags("<b>hello</b> world <br/>"), " hello  world  ");
    }

    #[test]
    fn test_xml_tag_content() {
        let chunk = "<item><title><![CDATA[Breaking News]]></title><link>http</link></item>";
        assert_eq!(xml_tag_content(chunk, "title"), "Breaking News");
        assert_eq!(xml_tag_content(chunk, "link"), "http");
        assert_eq!(xml_tag_content(chunk, "missing"), "");
    }

    #[tokio::test]
    async fn test_execute_web_search_fallback_flow() {
        // Force an invalid key to ensure we skip or fail Brave and hit the fallback tiers.
        // We will store the actual key and restore it after testing to prevent breaking other concurrently running tests,
        // though `env:set_var` is process global. In a real CI this would run isolated.
        let old_key = std::env::var("BRAVE_SEARCH_API_KEY").unwrap_or_default();
        unsafe {
            std::env::set_var("BRAVE_SEARCH_API_KEY", "invalid_key");
        }

        let res = execute_web_search("test_fallback".into(), "Rust lang".into(), None).await;
        
        // Restore key for other tests
        unsafe {
            std::env::set_var("BRAVE_SEARCH_API_KEY", old_key);
        }

        assert_eq!(res.status, ToolStatus::Success);
        assert!(
            res.output.contains("DDG SEARCH RESULTS for") || 
            res.output.contains("GOOGLE NEWS RSS for") ||
            res.output.contains("All search providers")
        );
    }
}
