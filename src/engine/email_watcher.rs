use std::sync::Arc;
use tokio::time::{sleep, Duration};
use crate::memory::MemoryStore;
use crate::models::message::Event;

pub async fn spawn_email_watcher(memory: Arc<MemoryStore>) {
    tokio::spawn(async move {
        let user = std::env::var("IMAP_USER").unwrap_or_default();
        let pass = std::env::var("IMAP_PASS").unwrap_or_default();
        let host = std::env::var("IMAP_HOST").unwrap_or_else(|_| "imap.gmail.com".into());
        let port_str = std::env::var("IMAP_PORT").unwrap_or_else(|_| "993".into());

        if user.is_empty() || pass.is_empty() {
            tracing::warn!("[EMAIL_WATCHER] IMAP_USER or IMAP_PASS not set. Inbound email polling inactive.");
            return;
        }

        let port: u16 = port_str.parse().unwrap_or(993);

        tracing::info!("[EMAIL_WATCHER] 📧 Starting IMAP Listener on {}:{}", host, port);

        loop {
            // Run blocking IMAP operations inside spawn_blocking to avoid blocking the tokio runtime
            let host_clone = host.clone();
            let user_clone = user.clone();
            let pass_clone = pass.clone();
            let memory_clone = memory.clone();

            let result = tokio::task::spawn_blocking(move || -> Result<Vec<Event>, String> {
                use native_tls::TlsConnector;
                use std::net::TcpStream;

                let tcp = TcpStream::connect((host_clone.as_str(), port))
                    .map_err(|e| format!("TCP connection failed: {}", e))?;

                let tls = TlsConnector::new()
                    .map_err(|e| format!("TLS init failed: {}", e))?;

                let tls_stream = tls.connect(&host_clone, tcp)
                    .map_err(|e| format!("TLS handshake failed: {}", e))?;

                let client = imap::Client::new(tls_stream);
                let mut session = client.login(&user_clone, &pass_clone)
                    .map_err(|e| format!("Login failed: {:?}", e.0))?;

                session.select("INBOX")
                    .map_err(|e| format!("Failed to select INBOX: {:?}", e))?;

                // Search for unseen messages
                let unseen = session.search("UNSEEN")
                    .map_err(|e| format!("Search failed: {:?}", e))?;

                let mut events = Vec::new();

                if !unseen.is_empty() {
                    let seq_set: String = unseen.iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(",");

                    let fetches = session.fetch(&seq_set, "RFC822")
                        .map_err(|e| format!("Fetch failed: {:?}", e))?;

                    for fetch in fetches.iter() {
                        if let Some(body_bytes) = fetch.body() {
                            if let Ok(parsed) = mailparse::parse_mail(body_bytes) {
                                let subject = parsed.headers.iter()
                                    .find(|h| h.get_key().eq_ignore_ascii_case("subject"))
                                    .map(|h| h.get_value())
                                    .unwrap_or_default();
                                let from = parsed.headers.iter()
                                    .find(|h| h.get_key().eq_ignore_ascii_case("from"))
                                    .map(|h| h.get_value())
                                    .unwrap_or_default();
                                let body_text = parsed.get_body().unwrap_or_default();

                                let envelope = format!("SUBJECT: {}\nFROM: {}\n\n{}", subject, from, body_text);
                                let timestamp = Some(chrono::Utc::now().to_rfc3339());
                                
                                events.push(Event {
                                    platform: "email".into(),
                                    scope: crate::models::scope::Scope::Private { user_id: from.clone() },
                                    author_name: from.clone(),
                                    author_id: from,
                                    content: envelope,
                                    timestamp,
                                    message_index: None,
                                });
                            }
                        }
                    }
                }

                let _ = session.logout();
                Ok(events)
            }).await;

            match result {
                Ok(Ok(events)) => {
                    for event in events {
                        tracing::info!("[EMAIL_WATCHER] 📨 Inbound message received from {}", event.author_name);
                        memory_clone.working.add_event(event).await;
                    }
                }
                Ok(Err(e)) => tracing::warn!("[EMAIL_WATCHER] {}", e),
                Err(e) => tracing::warn!("[EMAIL_WATCHER] Task panic: {:?}", e),
            }

            // Sleep before polling again (60s tick)
            sleep(Duration::from_secs(60)).await;
        }
    });
}
