#![allow(clippy::field_reassign_with_default)]
use headless_chrome::{Browser, LaunchOptions};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

use super::pdf_styles::get_theme;

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentSection {
    pub heading: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentDraft {
    pub id: String,
    pub title: String,
    pub author: String,
    pub theme: String,
    #[serde(default)]
    pub custom_css: String,
    pub sections: Vec<DocumentSection>,
}

#[derive(Clone)]
pub struct DocumentComposer {
    drafts_dir: PathBuf,
    output_dir: PathBuf,
}

impl Default for DocumentComposer {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentComposer {
    pub fn new() -> Self {
        Self {
            drafts_dir: PathBuf::from("memory/core/docs/drafts"),
            output_dir: PathBuf::from("memory/core/docs/rendered"),
        }
    }

    pub fn with_dirs(drafts: PathBuf, rendered: PathBuf) -> Self {
        Self {
            drafts_dir: drafts,
            output_dir: rendered,
        }
    }

    pub async fn create_draft(
        &self,
        id: &str,
        title: &str,
        author: &str,
        theme: &str,
    ) -> std::io::Result<()> {
        fs::create_dir_all(&self.drafts_dir).await?;

        let draft = DocumentDraft {
            id: id.to_string(),
            title: title.to_string(),
            author: author.to_string(),
            theme: theme.to_string(),
            custom_css: String::new(),
            sections: Vec::new(),
        };

        let path = self.drafts_dir.join(format!("{}.json", id));
        let json = serde_json::to_string_pretty(&draft)?;
        fs::write(path, json).await?;
        Ok(())
    }

    pub async fn add_section(&self, id: &str, heading: &str, content: &str) -> std::io::Result<()> {
        let path = self.drafts_dir.join(format!("{}.json", id));
        if !path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Draft not found",
            ));
        }

        let json = fs::read_to_string(&path).await?;
        let mut draft: DocumentDraft = serde_json::from_str(&json)?;

        draft.sections.push(DocumentSection {
            heading: heading.to_string(),
            content: content.to_string(),
        });

        let updated_json = serde_json::to_string_pretty(&draft)?;
        fs::write(path, updated_json).await?;
        Ok(())
    }

    pub async fn render_pdf(&self, id: &str) -> std::io::Result<(String, String)> {
        let draft_path = self.drafts_dir.join(format!("{}.json", id));
        if !draft_path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Draft not found",
            ));
        }

        let json = fs::read_to_string(&draft_path).await?;
        let draft: DocumentDraft = serde_json::from_str(&json)?;

        let css = get_theme(&draft.theme);

        // Build HTML
        let mut html = String::new();
        html.push_str("<!DOCTYPE html><html><head><meta charset=\"UTF-8\">");
        html.push_str(&format!(
            "<style>{}</style>",
            crate::computer::pdf_styles::BASE_CSS
        ));
        html.push_str(&format!("<style>{}</style>", css));
        // Inject custom CSS overrides (user-specified styling)
        if !draft.custom_css.is_empty() {
            html.push_str(&format!("<style>{}</style>", draft.custom_css));
        }

        html.push_str("</head><body><div class=\"document-container\">");

        // Header
        html.push_str("<div class=\"header\">");
        html.push_str(&format!("<h1>{}</h1>", html_escape(&draft.title)));
        html.push_str(&format!(
            "<div class=\"author\">Written by {}</div>",
            html_escape(&draft.author)
        ));
        html.push_str("</div>");

        // Sections
        for section in &draft.sections {
            html.push_str("<div class=\"section\">");
            if !section.heading.is_empty() {
                html.push_str(&format!("<h2>{}</h2>", html_escape(&section.heading)));
            }
            // Basic Markdown to HTML conversion
            let content_html = convert_markdown(&section.content);
            html.push_str(&content_html);
            html.push_str("</div>");
        }

        html.push_str("</div></body></html>");

        // Inline local images as base64 data URIs so headless Chrome can render them
        // (Chrome blocks file:// cross-origin loads even from file:// pages)
        let mut final_html = html;
        let needle = r#"<img src="file://"#;
        while let Some(start) = final_html.find(needle) {
            // Find the closing quote after file://
            let src_start = start + r#"<img src=""#.len(); // points to "file://..."
            if let Some(quote_end) = final_html[src_start..].find('"') {
                let file_url = final_html[src_start..src_start + quote_end].to_string();
                let file_path = file_url.strip_prefix("file://").unwrap_or(&file_url).to_string();
                
                if let Ok(img_bytes) = std::fs::read(&file_path) {
                    let ext = std::path::Path::new(file_path.as_str())
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("png");
                    let mime = match ext {
                        "jpg" | "jpeg" => "image/jpeg",
                        "gif" => "image/gif",
                        "webp" => "image/webp",
                        "svg" => "image/svg+xml",
                        _ => "image/png",
                    };
                    use base64::Engine;
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&img_bytes);
                    let old_src = format!(r#"<img src="{}""#, file_url);
                    let new_src = format!(r#"<img src="data:{};base64,{}""#, mime, b64);
                    final_html = final_html.replacen(&old_src, &new_src, 1);
                    tracing::info!("[PDF] 🖼️ Inlined image: {} ({} bytes)", file_path, img_bytes.len());
                } else {
                    tracing::warn!("[PDF] ⚠️ Could not read image: {}", file_path);
                    break;
                }
            } else {
                break;
            }
        }

        // Save HTML to temp file for headless chrome
        let temp_html_path = self.drafts_dir.join(format!("{}.html", id));
        fs::write(&temp_html_path, &final_html).await?;

        // Render PDF
        fs::create_dir_all(&self.output_dir).await?;
        let output_pdf_path = self.output_dir.join(format!("{}.pdf", id));
        let output_png_path = self.output_dir.join(format!("{}_preview.png", id));

        // Headless Chrome requires running in a blocking thread
        let pdf_path_clone = output_pdf_path.clone();
        let png_path_clone = output_png_path.clone();
        let html_path_clone = temp_html_path.clone();

        let render_result = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            // Retry loop for Browser::new() — Chrome 146+ has a timing race where
            // the DevTools WebSocket server isn't ready when the crate tries to connect,
            // causing "WebSocket protocol error: Handshake not finished".
            let mut browser_result = None;
            for attempt in 0..3u32 {
                if attempt > 0 {
                    let delay = std::time::Duration::from_secs(1 << attempt); // 2s, 4s
                    tracing::warn!("[PDF] Chrome WebSocket retry #{} after {:?}", attempt + 1, delay);
                    std::thread::sleep(delay);
                }
                match Browser::new(LaunchOptions {
                    headless: true,
                    sandbox: false,
                    idle_browser_timeout: std::time::Duration::from_secs(30),
                    ..Default::default()
                }) {
                    Ok(b) => {
                        browser_result = Some(b);
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("[PDF] Chrome launch attempt {} failed: {}", attempt + 1, e);
                        if attempt == 2 {
                            return Err(std::io::Error::other(format!(
                                "Chrome failed after 3 attempts: {}", e
                            )));
                        }
                    }
                }
            }
            let browser = browser_result.unwrap();

            let tab = browser
                .new_tab()
                .map_err(|e| std::io::Error::other(e.to_string()))?;

            let file_url = format!(
                "file://{}",
                html_path_clone.canonicalize()?.to_string_lossy()
            );
            tab.navigate_to(&file_url)
                .map_err(|e| std::io::Error::other(e.to_string()))?;

            tab.wait_until_navigated()
                .map_err(|e| std::io::Error::other(e.to_string()))?;

            // Capture PDF
            let mut pdf_options = headless_chrome::types::PrintToPdfOptions::default();
            pdf_options.prefer_css_page_size = Some(true); // Force CSS @page bounds
            pdf_options.paper_width = Some(8.27); // A4 width in inches
            pdf_options.paper_height = Some(11.69); // A4 height in inches
            pdf_options.print_background = Some(true);
            pdf_options.margin_top = Some(0.0);
            pdf_options.margin_bottom = Some(0.0);
            pdf_options.margin_left = Some(0.0);
            pdf_options.margin_right = Some(0.0);
            let pdf_data = tab
                .print_to_pdf(Some(pdf_options))
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            std::fs::write(&pdf_path_clone, pdf_data)?;

            // Capture PNG preview (viewport screenshot for visual QA)
            let png_data = tab
                .capture_screenshot(
                    headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
                    None,
                    None,
                    true,
                )
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            std::fs::write(&png_path_clone, png_data)?;

            Ok(())
        })
        .await?;

        render_result?;

        // Cleanup temp HTML
        let _ = fs::remove_file(temp_html_path).await;

        let absolute_pdf_path = output_pdf_path
            .canonicalize()?
            .to_string_lossy()
            .to_string();

        let absolute_png_path = output_png_path
            .canonicalize()
            .unwrap_or(output_png_path)
            .to_string_lossy()
            .to_string();

        Ok((absolute_pdf_path, absolute_png_path))
    }

    /// Lists all draft IDs in the drafts directory.
    pub async fn list_drafts(&self) -> std::io::Result<Vec<String>> {
        let mut ids = Vec::new();
        if !self.drafts_dir.exists() {
            return Ok(ids);
        }
        let mut reader = fs::read_dir(&self.drafts_dir).await?;
        while let Some(entry) = reader.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    ids.push(stem.to_string());
                }
        }
        Ok(ids)
    }

    /// Returns a formatted summary of a draft's metadata and sections.
    pub async fn get_draft_info(&self, id: &str) -> std::io::Result<String> {
        let path = self.drafts_dir.join(format!("{}.json", id));
        if !path.exists() {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Draft not found"));
        }
        let json = fs::read_to_string(&path).await?;
        let draft: DocumentDraft = serde_json::from_str(&json)?;

        let mut info = format!("📄 **Draft: {}**\nTitle: {}\nAuthor: {}\nTheme: {}\nSections: {}\n\n",
            draft.id, draft.title, draft.author, draft.theme, draft.sections.len());

        for (i, section) in draft.sections.iter().enumerate() {
            let preview: String = section.content.chars().take(120).collect();
            let heading_display = if section.heading.is_empty() { "(no heading)" } else { &section.heading };
            info.push_str(&format!("[{}] **{}**: {}...\n", i, heading_display, preview.replace('\n', " ")));
        }
        Ok(info)
    }

    /// Replaces a section at the given index with new heading and content.
    pub async fn edit_section(&self, id: &str, index: usize, new_heading: &str, new_content: &str) -> std::io::Result<()> {
        let path = self.drafts_dir.join(format!("{}.json", id));
        if !path.exists() {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Draft not found"));
        }
        let json = fs::read_to_string(&path).await?;
        let mut draft: DocumentDraft = serde_json::from_str(&json)?;

        if index >= draft.sections.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,
                format!("Section index {} out of range (draft has {} sections)", index, draft.sections.len())));
        }

        draft.sections[index] = DocumentSection {
            heading: new_heading.to_string(),
            content: new_content.to_string(),
        };

        let updated = serde_json::to_string_pretty(&draft)?;
        fs::write(path, updated).await?;
        Ok(())
    }

    /// Removes a section at the given index.
    pub async fn remove_section(&self, id: &str, index: usize) -> std::io::Result<()> {
        let path = self.drafts_dir.join(format!("{}.json", id));
        if !path.exists() {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Draft not found"));
        }
        let json = fs::read_to_string(&path).await?;
        let mut draft: DocumentDraft = serde_json::from_str(&json)?;

        if index >= draft.sections.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,
                format!("Section index {} out of range (draft has {} sections)", index, draft.sections.len())));
        }

        draft.sections.remove(index);
        let updated = serde_json::to_string_pretty(&draft)?;
        fs::write(path, updated).await?;
        Ok(())
    }

    /// Updates the theme of an existing draft.
    pub async fn update_theme(&self, id: &str, new_theme: &str) -> std::io::Result<()> {
        let path = self.drafts_dir.join(format!("{}.json", id));
        if !path.exists() {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Draft not found"));
        }
        let json = fs::read_to_string(&path).await?;
        let mut draft: DocumentDraft = serde_json::from_str(&json)?;
        draft.theme = new_theme.to_string();
        let updated = serde_json::to_string_pretty(&draft)?;
        fs::write(path, updated).await?;
        Ok(())
    }

    /// Sets custom CSS overrides on a draft (applied on top of the theme).
    pub async fn set_custom_css(&self, id: &str, css: &str) -> std::io::Result<()> {
        let path = self.drafts_dir.join(format!("{}.json", id));
        if !path.exists() {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Draft not found"));
        }
        let json = fs::read_to_string(&path).await?;
        let mut draft: DocumentDraft = serde_json::from_str(&json)?;
        draft.custom_css = css.to_string();
        let updated = serde_json::to_string_pretty(&draft)?;
        fs::write(path, updated).await?;
        Ok(())
    }

    /// Renders the draft as plain text (no styling).
    pub async fn render_text(&self, id: &str) -> std::io::Result<String> {
        let draft = self.load_draft(id).await?;
        fs::create_dir_all(&self.output_dir).await?;
        let mut text = format!("{}\nBy {}\n\n", draft.title, draft.author);
        for section in &draft.sections {
            if !section.heading.is_empty() {
                text.push_str(&format!("== {} ==\n\n", section.heading));
            }
            text.push_str(&section.content);
            text.push_str("\n\n");
        }
        let out_path = self.output_dir.join(format!("{}.txt", id));
        fs::write(&out_path, &text).await?;
        Ok(out_path.canonicalize()?.to_string_lossy().to_string())
    }

    /// Renders the draft as a Markdown file.
    pub async fn render_markdown(&self, id: &str) -> std::io::Result<String> {
        let draft = self.load_draft(id).await?;
        fs::create_dir_all(&self.output_dir).await?;
        let mut md = format!("# {}\n*By {}*\n\n", draft.title, draft.author);
        for section in &draft.sections {
            if !section.heading.is_empty() {
                md.push_str(&format!("## {}\n\n", section.heading));
            }
            md.push_str(&section.content);
            md.push_str("\n\n");
        }
        let out_path = self.output_dir.join(format!("{}.md", id));
        fs::write(&out_path, &md).await?;
        Ok(out_path.canonicalize()?.to_string_lossy().to_string())
    }

    /// Renders the draft as a standalone HTML file (no Chrome, instant).
    pub async fn render_html(&self, id: &str) -> std::io::Result<String> {
        let draft = self.load_draft(id).await?;
        fs::create_dir_all(&self.output_dir).await?;
        let css = get_theme(&draft.theme);
        let mut html = String::new();
        html.push_str("<!DOCTYPE html><html><head><meta charset=\"UTF-8\">");
        html.push_str(&format!("<style>{}</style>", crate::computer::pdf_styles::BASE_CSS));
        html.push_str(&format!("<style>{}</style>", css));
        html.push_str("</head><body><div class=\"document-container\">");
        html.push_str("<div class=\"header\">");
        html.push_str(&format!("<h1>{}</h1>", html_escape(&draft.title)));
        html.push_str(&format!("<div class=\"author\">Written by {}</div>", html_escape(&draft.author)));
        html.push_str("</div>");
        for section in &draft.sections {
            html.push_str("<div class=\"section\">");
            if !section.heading.is_empty() {
                html.push_str(&format!("<h2>{}</h2>", html_escape(&section.heading)));
            }
            html.push_str(&convert_markdown(&section.content));
            html.push_str("</div>");
        }
        html.push_str("</div></body></html>");
        let out_path = self.output_dir.join(format!("{}.html", id));
        fs::write(&out_path, &html).await?;
        Ok(out_path.canonicalize()?.to_string_lossy().to_string())
    }

    /// Renders the draft as CSV (heading, content per row).
    pub async fn render_csv(&self, id: &str) -> std::io::Result<String> {
        let draft = self.load_draft(id).await?;
        fs::create_dir_all(&self.output_dir).await?;
        let mut csv = String::from("heading,content\n");
        for section in &draft.sections {
            let h = section.heading.replace('"', "\"\"");
            let c = section.content.replace('"', "\"\"");
            csv.push_str(&format!("\"{}\",\"{}\"\n", h, c));
        }
        let out_path = self.output_dir.join(format!("{}.csv", id));
        fs::write(&out_path, &csv).await?;
        Ok(out_path.canonicalize()?.to_string_lossy().to_string())
    }

    /// Exports the raw draft JSON as a formatted file.
    pub async fn render_json(&self, id: &str) -> std::io::Result<String> {
        let draft = self.load_draft(id).await?;
        fs::create_dir_all(&self.output_dir).await?;
        let json = serde_json::to_string_pretty(&draft)?;
        let out_path = self.output_dir.join(format!("{}.json", id));
        fs::write(&out_path, &json).await?;
        Ok(out_path.canonicalize()?.to_string_lossy().to_string())
    }

    /// Internal helper to load a draft by ID.
    async fn load_draft(&self, id: &str) -> std::io::Result<DocumentDraft> {
        let path = self.drafts_dir.join(format!("{}.json", id));
        if !path.exists() {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Draft not found"));
        }
        let json = fs::read_to_string(&path).await?;
        Ok(serde_json::from_str(&json)?)
    }
}

// Simple HTML Escaper
fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// Very basic Markdown-to-HTML implementation for essential tags (Bold, Italics, Lists, Code)
fn convert_markdown(md: &str) -> String {
    let mut html = String::new();
    let mut in_code_block = false;
    let mut in_list = false;

    for line in md.lines() {
        let trimmed = line.trim();

        // Code blocks
        if trimmed.starts_with("```") {
            if in_code_block {
                html.push_str("</code></pre>\n");
                in_code_block = false;
            } else {
                html.push_str("<pre><code>");
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            html.push_str(&html_escape(line));
            html.push('\n');
            continue;
        }

        // Unordered Lists
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            if !in_list {
                html.push_str("<ul>\n");
                in_list = true;
            }
            let item = &trimmed[2..];
            html.push_str(&format!("<li>{}</li>\n", apply_inline_md(item)));
            continue;
        } else if in_list {
            html.push_str("</ul>\n");
            in_list = false;
        }

        // Headings inside sections (check longest prefix first)
        if let Some(stripped) = trimmed.strip_prefix("#### ") {
            html.push_str(&format!("<h4>{}</h4>\n", apply_inline_md(stripped)));
            continue;
        }
        if let Some(stripped) = trimmed.strip_prefix("### ") {
            html.push_str(&format!("<h3>{}</h3>\n", apply_inline_md(stripped)));
            continue;
        }
        if let Some(stripped) = trimmed.strip_prefix("## ") {
            html.push_str(&format!("<h2>{}</h2>\n", apply_inline_md(stripped)));
            continue;
        }
        if let Some(stripped) = trimmed.strip_prefix("# ") {
            html.push_str(&format!("<h1>{}</h1>\n", apply_inline_md(stripped)));
            continue;
        }

        // Paragraphs
        if trimmed.is_empty() {
            html.push_str("<br>\n");
        } else {
            html.push_str(&format!("<p>{}</p>\n", apply_inline_md(trimmed)));
        }
    }

    if in_list {
        html.push_str("</ul>\n");
    }

    html
}

fn apply_inline_md(input: &str) -> String {
    let mut s = html_escape(input);
    
    // Parse images `![alt](src)`
    // Must loop until no more matches are found
    loop {
        let mut found = false;
        if let Some(start) = s.find("![")
            && let Some(mid) = s[start + 2..].find("](") {
                let mid_abs = start + 2 + mid;
                if let Some(end) = s[mid_abs + 2..].find(')') {
                    let end_abs = mid_abs + 2 + end;
                    let alt = &s[start + 2..mid_abs];
                    let raw_src = &s[mid_abs + 2..end_abs];
                    
                    // Prefix with file:// if it's an absolute local path for Chromium
                    let src = if raw_src.starts_with('/') {
                        format!("file://{}", raw_src)
                    } else {
                        raw_src.to_string()
                    };

                    s = format!(
                        "{}<img src=\"{}\" alt=\"{}\" />{}",
                        &s[..start],
                        src,
                        alt,
                        &s[end_abs + 1..]
                    );
                    found = true;
                }
            }
        if !found {
            break;
        }
    }

    // Rough bold
    while let Some(start) = s.find("**") {
        if let Some(end) = s[start + 2..].find("**") {
            let inner = &s[start + 2..start + 2 + end];
            s = format!(
                "{}<strong>{}</strong>{}",
                &s[..start],
                inner,
                &s[start + 4 + end..]
            );
        } else {
            break;
        }
    }
    // Rough italic
    while let Some(start) = s.find('*') {
        if let Some(end) = s[start + 1..].find('*') {
            let inner = &s[start + 1..start + 1 + end];
            s = format!("{}<em>{}</em>{}", &s[..start], inner, &s[start + 2 + end..]);
        } else {
            break;
        }
    }
    // Rough inline code
    while let Some(start) = s.find('`') {
        if let Some(end) = s[start + 1..].find('`') {
            let inner = &s[start + 1..start + 1 + end];
            s = format!(
                "{}<code>{}</code>{}",
                &s[..start],
                inner,
                &s[start + 2 + end..]
            );
        } else {
            break;
        }
    }
    s
}


#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;
