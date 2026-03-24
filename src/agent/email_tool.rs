use crate::models::tool::{ToolResult, ToolStatus};
use tokio::sync::mpsc;
use lettre::message::header::ContentType;
use lettre::{Message, SmtpTransport, Transport};
use lettre::transport::smtp::authentication::Credentials;
use crate::agent::preferences::extract_tag;

pub async fn execute_email(
    task_id: String,
    description: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    let action = extract_tag(&description, "action:").unwrap_or_else(|| "send_email".to_string());
    
    macro_rules! telemetry {
        ($tx:expr, $msg:expr) => {
            if let Some(ref tx) = $tx {
                let _ = tx.send($msg).await;
            }
        };
    }

    if action == "send_email" {
        telemetry!(telemetry_tx, "  → Compiling outbound SMTP payload...\n".into());
        
        let to = extract_tag(&description, "to:").unwrap_or_default();
        let subject = extract_tag(&description, "subject:").unwrap_or_else(|| "HIVE Transmission".to_string());
        let body = extract_tag(&description, "body:").unwrap_or_default();

        if to.is_empty() || body.is_empty() {
            return ToolResult {
                task_id,
                output: "Error: Missing 'to:' or 'body:' fields.".into(),
                tokens_used: 0,
                status: ToolStatus::Failed("Missing params".into()),
            };
        }

        let smtp_user = std::env::var("SMTP_USER").unwrap_or_default();
        let smtp_pass = std::env::var("SMTP_PASS").unwrap_or_default();
        let smtp_host = std::env::var("SMTP_HOST").unwrap_or_else(|_| "smtp.gmail.com".into());
        let smtp_port = std::env::var("SMTP_PORT").unwrap_or_else(|_| "587".into());

        if smtp_user.is_empty() || smtp_pass.is_empty() {
            return ToolResult {
                task_id,
                output: "Error: Active SMTP credentials missing from native Environment Variables.".into(),
                tokens_used: 0,
                status: ToolStatus::Failed("No Credentials".into()),
            };
        }

        telemetry!(telemetry_tx, format!("  → Booting TLS tunnel to {}...\n", smtp_host));
        
        // Lettre blocking calls must be isolated
        let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
            let email = Message::builder()
                .from(format!("Apis <{}>", smtp_user).parse::<lettre::message::Mailbox>().map_err(|e| e.to_string())?)
                .to(to.parse::<lettre::message::Mailbox>().map_err(|e| e.to_string())?)
                .subject(subject)
                .header(ContentType::TEXT_PLAIN)
                .body(body)
                .map_err(|e| e.to_string())?;

            let creds = Credentials::new(smtp_user, smtp_pass);
            let port: u16 = smtp_port.parse().unwrap_or(587);

            let mailer = if port == 465 {
                SmtpTransport::relay(&smtp_host)
                    .map_err(|e| e.to_string())?
                    .credentials(creds)
                    .build()
            } else {
                SmtpTransport::relay(&smtp_host)
                    .map_err(|e| e.to_string())?
                    .credentials(creds)
                    .port(port)
                    .build()
            };

            mailer.send(&email).map_err(|e| e.to_string())?;
            Ok(())
        }).await;

        match result {
            Ok(Ok(_)) => {
                telemetry!(telemetry_tx, "  ✅ Biological SMTP transmission complete.\n".into());
                return ToolResult {
                     task_id,
                     output: "Successfully sent outbound email.".into(),
                     tokens_used: 0,
                     status: ToolStatus::Success,
                };
            }
            Ok(Err(e)) => {
                telemetry!(telemetry_tx, format!("  ❌ Native SMTP Error: {}\n", e));
                return ToolResult { task_id, output: e, tokens_used: 0, status: ToolStatus::Failed("SMTP Error".into()) };
            }
            Err(e) => {
                return ToolResult { task_id, output: format!("Tokio Panic: {:?}", e), tokens_used: 0, status: ToolStatus::Failed("Thread Panic".into()) };
            }
        }
    }

    ToolResult {
        task_id,
        output: "Error: Unrecognized action in email tool.".into(),
        tokens_used: 0,
        status: ToolStatus::Failed("Bad Action".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_missing_params() {
        let r = execute_email("1".into(), "action:[send_email]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Missing"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_missing_smtp_creds() {
        // Ensure env vars are unset for this test
        unsafe {
            std::env::remove_var("SMTP_USER");
            std::env::remove_var("SMTP_PASS");
        }
        let r = execute_email("1".into(), "action:[send_email] to:[test@test.com] body:[hello]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("SMTP credentials") || r.output.contains("Missing"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_bad_action() {
        let r = execute_email("1".into(), "action:[explode]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }
}
