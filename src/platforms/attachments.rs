use serenity::builder::CreateAttachment;

/// Parses markdown attachment tags like `[ATTACH_IMAGE](path)`, strips them out of the given text,
/// and returns a list of local files formatted as Discord CreateAttachments.
#[cfg(not(tarpaulin_include))]
pub async fn extract_attachments(parsed_text: &mut String) -> Vec<CreateAttachment> {
    let mut attachments = Vec::new();
    let tags = vec!["[ATTACH_IMAGE](", "[ATTACH_FILE](", "[ATTACH_AUDIO]("];

    for tag in tags {
        while let Some(start_idx) = parsed_text.find(tag) {
            // Find the closing parenthesis relative to the start of the path
            let path_start_idx = start_idx + tag.len();
            if let Some(rel_end_idx) = parsed_text[path_start_idx..].find(')') {
                let end_idx = path_start_idx + rel_end_idx;
                let file_path = &parsed_text[path_start_idx..end_idx];
                
                match serenity::builder::CreateAttachment::path(file_path).await {
                    Ok(attachment) => attachments.push(attachment),
                    Err(e) => tracing::error!("[Discord Platform] Failed to attach local file at {}: {}", file_path, e),
                }
                
                // Strip the exact tag [TAG](path) from the text
                let before = &parsed_text[..start_idx];
                let after = &parsed_text[end_idx + 1..];
                *parsed_text = format!("{}{}", before, after);
            } else {
                // Malformed tag, break to avoid infinite loop
                break;
            }
        }
    }

    attachments
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn test_extract_attachments() {
        // Create a dummy file to attach
        let mut tmp_file = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp_file, "dummy content").unwrap();
        let path = tmp_file.path().to_str().unwrap().to_string();

        let mut text = format!(
            "Hello world! [ATTACH_IMAGE]({}) This is a test. [ATTACH_FILE]({}) Ending text.",
            path, path
        );

        let attachments = extract_attachments(&mut text).await;

        assert_eq!(attachments.len(), 2);
        assert_eq!(text, "Hello world!  This is a test.  Ending text.");
    }
}
