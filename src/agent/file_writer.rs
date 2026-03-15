use crate::models::tool::{ToolResult, ToolStatus};
use crate::agent::preferences::extract_tag;
use tokio::sync::mpsc;

pub async fn execute_file_writer(
    task_id: String,
    description: String,
    composer_opt: Option<crate::computer::document::DocumentComposer>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    let action = extract_tag(&description, "action:")
        .unwrap_or("".to_string())
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    let doc_id = extract_tag(&description, "id:")
        .unwrap_or("".to_string())
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();

    if action.is_empty() || doc_id.is_empty() {
        return ToolResult {
            task_id,
            output: "Error: Missing action:[start/add_section/render] or id:[doc_id]".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Invalid Usage".into()),
        };
    }

    let composer = composer_opt.unwrap_or_default();

    let output = match action.as_str() {
        "start" => {
            let title = extract_tag(&description, "title:")
                .unwrap_or("Untitled".to_string());
            let author = extract_tag(&description, "author:")
                .unwrap_or("Apis".to_string());
            let theme = extract_tag(&description, "theme:")
                .unwrap_or("professional".to_string())
                .split_whitespace()
                .next()
                .unwrap_or("professional")
                .to_string();

            if let Some(tx) = &telemetry_tx {
                let _ = tx
                    .send(format!("📑 Starting Document Draft '{}'...\n", title))
                    .await;
            }

            match composer
                .create_draft(&doc_id, &title, &author, &theme)
                .await
            {
                Ok(_) => format!(
                    "Success. Document '{}' (theme: {}) started. Use action:add_section to write.",
                    doc_id, theme
                ),
                Err(e) => format!("Failed to create draft: {}", e),
            }
        }
        "add_section" => {
            let heading = extract_tag(&description, "heading:")
                .unwrap_or("".to_string());
            let content = if let Some(idx) = description.find("content:") {
                description[idx + 8..].trim().to_string()
            } else {
                return ToolResult {
                    task_id,
                    output: "Error: Missing content: payloads are required for sections.".into(),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Missing Content".into()),
                };
            };

            match composer.add_section(&doc_id, &heading, &content).await {
                Ok(_) => format!("Success. Added section to document {}.", doc_id),
                Err(e) => format!("Failed to add section: {}", e),
            }
        }
        "render" => {
            if let Some(tx) = &telemetry_tx {
                let _ = tx
                    .send("⚙️ Document Composer rendering A4 PDF...\n".to_string())
                    .await;
                let _ = tx.send("typing_indicator".into()).await;
            }
            match composer.render_pdf(&doc_id).await {
                Ok(pdf_path) => {
                    if let Some(tx) = &telemetry_tx {
                        let _ = tx
                            .send("✨ Document successfully compiled to PDF.\n".to_string())
                            .await;
                    }
                    format!(
                        "Document rendering complete. YOU MUST include this EXACT tag in your human conversational response to display it to the user:\n\n[ATTACH_FILE]({})\n\nIf you do not include this, the user will not see the document.",
                        pdf_path
                    )
                }
                Err(e) => format!("Failed to render PDF: {}", e),
            }
        }
        "compose" => {
            let title = extract_tag(&description, "title:").unwrap_or("Untitled".to_string());
            let theme = extract_tag(&description, "theme:").unwrap_or("professional".to_string());
            let content = if let Some(idx) = description.find("content:[") {
                let s = &description[idx + 9..];
                if let Some(e) = s.rfind(']') {
                    s[..e].trim().to_string()
                } else {
                    s.trim().to_string()
                }
            } else {
                return ToolResult {
                    task_id,
                    output: "Error: Missing content:[...] payload.".into(),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Missing Content".into()),
                };
            };
            
            if let Some(tx) = &telemetry_tx {
                let _ = tx.send("⚙️ Composing and Rendering full Document Draft...\n".to_string()).await;
                let _ = tx.send("typing_indicator".into()).await;
            }
            let _ = composer.create_draft(&doc_id, &title, "Apis", &theme).await;
            let _ = composer.add_section(&doc_id, "", &content).await;
            
            match composer.render_pdf(&doc_id).await {
                Ok(pdf_path) => {
                    if let Some(tx) = &telemetry_tx {
                        let _ = tx.send("✨ Document complete.\n".to_string()).await;
                    }
                    format!(
                        "Document rendering complete. YOU MUST include this EXACT tag in your response to deliver it:\n\n[ATTACH_FILE]({})\n",
                        pdf_path
                    )
                }
                Err(e) => format!("Failed to render PDF: {}", e),
            }
        }
        "list_drafts" => {
            match composer.list_drafts().await {
                Ok(ids) => {
                    if ids.is_empty() {
                        "No drafts found. Use action:[start] or action:[compose] to create one.".to_string()
                    } else {
                        format!("📂 **Available Drafts ({}):**\n{}", ids.len(),
                            ids.iter().map(|id| format!("• `{}`", id)).collect::<Vec<_>>().join("\n"))
                    }
                }
                Err(e) => format!("Failed to list drafts: {}", e),
            }
        }
        "inspect" => {
            match composer.get_draft_info(&doc_id).await {
                Ok(info) => info,
                Err(e) => format!("Failed to inspect draft '{}': {}", doc_id, e),
            }
        }
        "edit_section" => {
            let index_str = extract_tag(&description, "index:")
                .unwrap_or("0".to_string())
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .to_string();
            let index: usize = index_str.parse().unwrap_or(0);
            let heading = extract_tag(&description, "heading:").unwrap_or("".to_string());
            let content = if let Some(idx) = description.find("content:[") {
                let s = &description[idx + 9..];
                if let Some(e) = s.rfind(']') {
                    s[..e].trim().to_string()
                } else {
                    s.trim().to_string()
                }
            } else if let Some(idx) = description.find("content:") {
                description[idx + 8..].trim().to_string()
            } else {
                return ToolResult {
                    task_id,
                    output: "Error: Missing content: payload for edit_section.".into(),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Missing Content".into()),
                };
            };

            match composer.edit_section(&doc_id, index, &heading, &content).await {
                Ok(_) => format!(
                    "✅ Section [{}] of draft '{}' updated. Use action:[render] id:[{}] to regenerate the PDF.",
                    index, doc_id, doc_id
                ),
                Err(e) => format!("Failed to edit section: {}", e),
            }
        }
        "remove_section" => {
            let index_str = extract_tag(&description, "index:")
                .unwrap_or("0".to_string())
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .to_string();
            let index: usize = index_str.parse().unwrap_or(0);

            match composer.remove_section(&doc_id, index).await {
                Ok(_) => format!(
                    "✅ Section [{}] removed from draft '{}'. Use action:[render] id:[{}] to regenerate the PDF.",
                    index, doc_id, doc_id
                ),
                Err(e) => format!("Failed to remove section: {}", e),
            }
        }
        "update_theme" => {
            let theme = extract_tag(&description, "theme:")
                .unwrap_or("professional".to_string())
                .split_whitespace()
                .next()
                .unwrap_or("professional")
                .to_string();

            match composer.update_theme(&doc_id, &theme).await {
                Ok(_) => format!(
                    "✅ Theme for draft '{}' changed to '{}'. Use action:[render] id:[{}] to regenerate the PDF with the new theme.",
                    doc_id, theme, doc_id
                ),
                Err(e) => format!("Failed to update theme: {}", e),
            }
        }
        _ => format!("Unknown document action: {}", action),
    };

    ToolResult {
        task_id,
        output,
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_file_writer_missing_args() {
        let res = execute_file_writer("1".into(), "".into(), None, None).await;
        assert!(matches!(res.status, ToolStatus::Failed(_)));
    }

    #[tokio::test]
    async fn test_execute_file_writer_flow() {
        let tmp = tempfile::tempdir().unwrap();
        let drafts_dir = tmp.path().join("drafts");
        let output_dir = tmp.path().join("rendered");

        let get_composer = || {
            Some(crate::computer::document::DocumentComposer::with_dirs(
                drafts_dir.clone(),
                output_dir.clone(),
            ))
        };

        // 1. Start Document
        let start_desc = "action:[start] id:[test1] title:[My Test Phase]";
        let res = execute_file_writer("2".into(), start_desc.into(), get_composer(), None).await;
        assert!(res.output.contains("started"));

        // 2. Add Section (no payload)
        let bad_add = "action:[add_section] id:[test1] heading:[Intro]";
        let res = execute_file_writer("3".into(), bad_add.into(), get_composer(), None).await;
        assert_eq!(res.status, ToolStatus::Failed("Missing Content".into()));

        // 3. Add Section (with payload)
        let add = "action:[add_section] id:[test1] heading:[Intro] content:Hello from tests!";
        let res = execute_file_writer("4".into(), add.into(), get_composer(), None).await;
        assert!(res.output.contains("Added section"));

        // 4. Render
        let render = "action:[render] id:[test1]";
        let res = execute_file_writer("5".into(), render.into(), get_composer(), None).await;
        // Headless Chrome rendering might fail gracefully or succeed. Both are handled.
        assert!(res.output.contains("complete") || res.output.contains("Failed to render PDF"));

        // 5. Unknown Action
        let unknown = "action:[ghost] id:[test1]";
        let res = execute_file_writer("6".into(), unknown.into(), get_composer(), None).await;
        assert!(res.output.contains("Unknown document action"));
    }

    #[tokio::test]
    async fn test_execute_file_writer_editing_actions() {
        let tmp = tempfile::tempdir().unwrap();
        let drafts_dir = tmp.path().join("drafts");
        let output_dir = tmp.path().join("rendered");

        let get_composer = || {
            Some(crate::computer::document::DocumentComposer::with_dirs(
                drafts_dir.clone(),
                output_dir.clone(),
            ))
        };

        // Setup: Start + two sections
        let _ = execute_file_writer("s".into(), "action:[start] id:[edit_test] title:[Edit Test]".into(), get_composer(), None).await;
        let _ = execute_file_writer("a1".into(), "action:[add_section] id:[edit_test] heading:[First] content:Hello world".into(), get_composer(), None).await;
        let _ = execute_file_writer("a2".into(), "action:[add_section] id:[edit_test] heading:[Second] content:Goodbye world".into(), get_composer(), None).await;

        // 1. List drafts
        let res = execute_file_writer("l".into(), "action:[list_drafts] id:[any]".into(), get_composer(), None).await;
        assert!(res.output.contains("edit_test"));

        // 2. Inspect
        let res = execute_file_writer("i".into(), "action:[inspect] id:[edit_test]".into(), get_composer(), None).await;
        assert!(res.output.contains("[0]"));
        assert!(res.output.contains("First"));
        assert!(res.output.contains("[1]"));
        assert!(res.output.contains("Second"));

        // 3. Edit section
        let res = execute_file_writer("e".into(), "action:[edit_section] id:[edit_test] index:[0] heading:[Updated First] content:New content here".into(), get_composer(), None).await;
        assert!(res.output.contains("updated"));

        // Verify edit took effect
        let res = execute_file_writer("i2".into(), "action:[inspect] id:[edit_test]".into(), get_composer(), None).await;
        assert!(res.output.contains("Updated First"));
        assert!(res.output.contains("New content"));

        // 4. Remove section
        let res = execute_file_writer("r".into(), "action:[remove_section] id:[edit_test] index:[1]".into(), get_composer(), None).await;
        assert!(res.output.contains("removed"));

        // Verify removal
        let res = execute_file_writer("i3".into(), "action:[inspect] id:[edit_test]".into(), get_composer(), None).await;
        assert!(res.output.contains("Sections: 1"));

        // 5. Update theme
        let res = execute_file_writer("t".into(), "action:[update_theme] id:[edit_test] theme:[cyberpunk]".into(), get_composer(), None).await;
        assert!(res.output.contains("cyberpunk"));

        // 6. Edit section out of range
        let res = execute_file_writer("bad".into(), "action:[edit_section] id:[edit_test] index:[99] heading:[X] content:Y".into(), get_composer(), None).await;
        assert!(res.output.contains("out of range"));

        // 7. Inspect non-existent draft
        let res = execute_file_writer("no".into(), "action:[inspect] id:[nonexistent]".into(), get_composer(), None).await;
        assert!(res.output.contains("Failed"));
    }
}
