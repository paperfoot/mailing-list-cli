use crate::cli::{EventAction, WebhookAction, WebhookListenArgs, WebhookPollArgs, WebhookTestArgs};
use crate::config::Config;
use crate::db::Db;
use crate::email_cli::EmailCli;
use crate::error::AppError;
use crate::output::{self, Format};
use crate::webhook::{listener, poll};
use serde_json::json;
use std::net::SocketAddr;

pub fn run(format: Format, action: WebhookAction) -> Result<(), AppError> {
    match action {
        WebhookAction::Listen(args) => listen(format, args),
        WebhookAction::Poll(args) => poll_once(format, args),
        WebhookAction::Test(args) => test(format, args),
    }
}

pub fn run_event(format: Format, action: EventAction) -> Result<(), AppError> {
    match action {
        EventAction::Poll(args) => poll_once(format, args),
    }
}

fn listen(_format: Format, args: WebhookListenArgs) -> Result<(), AppError> {
    let config = Config::load()?;
    let addr: SocketAddr = args.bind.parse().map_err(|e| AppError::BadInput {
        code: "bad_bind_addr".into(),
        message: format!("invalid bind address '{}': {e}", args.bind),
        suggestion: "Use host:port syntax, e.g. 127.0.0.1:8081".into(),
    })?;
    let secret = config
        .webhook
        .secret_env
        .as_ref()
        .and_then(|env_name| std::env::var(env_name).ok())
        .map(|s| s.into_bytes());
    eprintln!(
        "Starting webhook listener on {addr} (secret configured: {})",
        secret.is_some()
    );
    listener::start_listener(addr, secret)
}

fn poll_once(format: Format, args: WebhookPollArgs) -> Result<(), AppError> {
    let config = Config::load()?;
    let db = Db::open()?;
    let cli = EmailCli::new(&config.email_cli.path, &config.email_cli.profile);
    let result = poll::poll_events(&db, &cli, args.reset)?;
    output::success(
        format,
        &format!(
            "polled: {} processed, {} duplicates",
            result.processed, result.duplicates
        ),
        json!({
            "processed": result.processed,
            "duplicates": result.duplicates,
            "latest_cursor": result.latest_cursor
        }),
    );
    Ok(())
}

fn test(format: Format, args: WebhookTestArgs) -> Result<(), AppError> {
    let payload = match args.event.as_str() {
        "delivered" => json!({
            "type": "email.delivered",
            "created_at": chrono::Utc::now().to_rfc3339(),
            "data": {"email_id": "em_test_synthetic", "to": ["test@example.com"]}
        }),
        "bounced" => json!({
            "type": "email.bounced",
            "created_at": chrono::Utc::now().to_rfc3339(),
            "data": {
                "email_id": "em_test_synthetic",
                "to": ["test@example.com"],
                "bounce": {"type": "Permanent", "message": "mailbox not found"}
            }
        }),
        "clicked" => json!({
            "type": "email.clicked",
            "created_at": chrono::Utc::now().to_rfc3339(),
            "data": {
                "email_id": "em_test_synthetic",
                "to": ["test@example.com"],
                "click": {"link": "https://example.com/cta", "ip_address": "1.2.3.4"}
            }
        }),
        other => {
            return Err(AppError::BadInput {
                code: "unknown_event_type".into(),
                message: format!("unknown event type '{other}'"),
                suggestion: "Use one of: delivered, bounced, clicked".into(),
            });
        }
    };
    let body = payload.to_string();
    // POST via curl — tiny_http doesn't ship a client
    let status = std::process::Command::new("curl")
        .args([
            "-sS",
            "-X",
            "POST",
            "-H",
            "content-type: application/json",
            "-d",
            &body,
            &args.to,
        ])
        .status()
        .map_err(|e| AppError::Config {
            code: "curl_not_found".into(),
            message: format!("could not run curl: {e}"),
            suggestion: "Install curl".into(),
        })?;
    if !status.success() {
        return Err(AppError::Transient {
            code: "curl_post_failed".into(),
            message: format!("curl POST to {} failed", args.to),
            suggestion: "Verify the listener URL is reachable".into(),
        });
    }
    output::success(
        format,
        &format!("synthetic {} event POSTed to {}", args.event, args.to),
        json!({"event": args.event, "to": args.to}),
    );
    Ok(())
}
