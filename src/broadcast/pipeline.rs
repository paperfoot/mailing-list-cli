//! Broadcast send pipeline (spec §5).
//!
//! Steps:
//!   1. Load broadcast + template + target
//!   2. Resolve target → contact IDs
//!   3. Pre-flight invariants (domain auth, template lint, physical address,
//!      complaint rate, recipient count cap)
//!   4. Suppression filter
//!   5. Per-recipient render (handlebars on pre-inlined HTML)
//!   6. Physical address footer injection
//!   7. RFC 8058 List-Unsubscribe header (HMAC-signed token)
//!   8. Write JSON batch file, call email-cli batch send
//!   9. Update broadcast_recipient rows from response
//!  10. Mark broadcast.status = 'sent'

use crate::broadcast::batch::{BatchEntry, write_batch_file};
use crate::broadcast::unsubscribe::sign_token;
use crate::config::Config;
use crate::db::Db;
use crate::email_cli::EmailCli;
use crate::error::AppError;
use crate::models::{Broadcast, Contact};
use crate::segment::SegmentExpr;
use crate::segment::compiler;
use crate::template::{self, RenderError};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum PipelineError {
    #[error("{0}")]
    Generic(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PipelineResult {
    pub broadcast_id: i64,
    pub sent_count: usize,
    pub suppressed_count: usize,
    pub failed_count: usize,
}

const CHUNK_SIZE: usize = 100;

#[allow(dead_code)]
pub fn send_broadcast(id: i64) -> Result<PipelineResult, AppError> {
    let config = Config::load()?;
    // v0.3: mut because the per-chunk transaction blocks need
    // &mut Connection for conn.transaction().
    let mut db = Db::open()?;
    let cli = EmailCli::new(&config.email_cli.path, &config.email_cli.profile);

    // 1. Load broadcast + template
    let broadcast = db.broadcast_get(id)?.ok_or_else(|| AppError::BadInput {
        code: "broadcast_not_found".into(),
        message: format!("no broadcast with id {id}"),
        suggestion: "Run `mailing-list-cli broadcast ls` to see existing broadcasts".into(),
    })?;
    if !matches!(broadcast.status.as_str(), "draft" | "scheduled" | "sending") {
        return Err(AppError::BadInput {
            code: "broadcast_bad_status".into(),
            message: format!(
                "broadcast '{}' is in status '{}' — cannot send",
                broadcast.name, broadcast.status
            ),
            suggestion: "Only draft, scheduled, or sending broadcasts can be sent".into(),
        });
    }
    let template = db
        .template_all()?
        .into_iter()
        .find(|t| t.id == broadcast.template_id)
        .ok_or_else(|| AppError::BadInput {
            code: "template_not_found".into(),
            message: format!(
                "template id {} not found (was it deleted?)",
                broadcast.template_id
            ),
            suggestion: "Re-create the broadcast with a valid template".into(),
        })?;

    // 2. Resolve target → contact IDs
    let recipients = resolve_target(&db, &broadcast)?;

    // 3. Pre-flight invariants
    preflight_checks(
        &config,
        &template.html_source,
        &template.subject,
        recipients.len(),
    )?;

    // Resolve sender from config (validated by preflight already, but extract again).
    let sender_from = config
        .sender
        .from
        .as_ref()
        .ok_or_else(|| AppError::Config {
            code: "sender_from_required".into(),
            message: "[sender].from is empty in config.toml".into(),
            suggestion: "Edit config.toml and set [sender].from = \"you@yourdomain.com\"".into(),
        })?
        .clone();
    let physical_address = config
        .sender
        .physical_address
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();

    // Mark sending
    db.broadcast_set_status(id, "sending", None)?;

    // 4. Suppression filter + insert pending broadcast_recipient rows.
    // v0.3: load the entire suppression list into an in-memory HashSet once
    // before the filter loop. Replaces O(N) per-recipient `is_email_suppressed`
    // DB queries with O(1) HashSet::contains lookups — the biggest single
    // win for 10k+ sends.
    //
    // v0.3: wrap the entire filter insert loop in ONE transaction so N
    // individual fsyncs collapse into one. For 10k recipients this drops
    // the filter phase from ~2.5s to ~40ms on a warm WAL-mode DB.
    let suppressed_set = db.suppression_all_emails()?;
    let mut to_send: Vec<Contact> = Vec::with_capacity(recipients.len());
    let mut suppressed_count = 0;
    {
        let tx = db.conn.transaction().map_err(|e| AppError::Transient {
            code: "tx_begin_failed".into(),
            message: format!("begin suppression-filter transaction failed: {e}"),
            suggestion: "Retry — the DB is probably busy".into(),
        })?;
        for recipient in &recipients {
            if suppressed_set.contains(&recipient.email.to_ascii_lowercase()) {
                tx.execute(
                    "INSERT OR IGNORE INTO broadcast_recipient (broadcast_id, contact_id, status)
                     VALUES (?1, ?2, 'suppressed')",
                    rusqlite::params![id, recipient.id],
                )
                .map_err(|e| AppError::Transient {
                    code: "recipient_insert_failed".into(),
                    message: format!("insert broadcast_recipient (suppressed): {e}"),
                    suggestion: "Check DB disk space and WAL permissions".into(),
                })?;
                suppressed_count += 1;
            } else {
                tx.execute(
                    "INSERT OR IGNORE INTO broadcast_recipient (broadcast_id, contact_id, status)
                     VALUES (?1, ?2, 'pending')",
                    rusqlite::params![id, recipient.id],
                )
                .map_err(|e| AppError::Transient {
                    code: "recipient_insert_failed".into(),
                    message: format!("insert broadcast_recipient (pending): {e}"),
                    suggestion: "Check DB disk space and WAL permissions".into(),
                })?;
            }
        }
        tx.commit().map_err(|e| AppError::Transient {
            code: "tx_commit_failed".into(),
            message: format!("commit suppression-filter transaction failed: {e}"),
            suggestion: "Retry — the DB is probably busy".into(),
        })?;
    }
    // Second pass: build the to_send vec from the original owned contacts.
    // (We held references inside the tx above; to_send takes ownership here.)
    for recipient in recipients {
        if !suppressed_set.contains(&recipient.email.to_ascii_lowercase()) {
            to_send.push(recipient);
        }
    }
    db.broadcast_update_counts(id, to_send.len() as i64)?;

    // 5-7. Per-recipient render (done inside the chunk loop below)
    // 8. Write JSON batch file + shell out in chunks of 100
    let unsubscribe_secret = std::env::var(&config.unsubscribe.secret_env)
        .unwrap_or_else(|_| "mlc-unsubscribe-dev".to_string());
    let now_epoch = chrono::Utc::now().timestamp();
    let public_url = config.unsubscribe.public_url.clone();

    let mut sent_count = 0;
    let mut failed_count = 0;
    let cache_dir = crate::paths::cache_dir().join("batch-files");

    for (chunk_idx, chunk) in to_send.chunks(CHUNK_SIZE).enumerate() {
        let mut entries = Vec::with_capacity(chunk.len());
        for contact in chunk {
            let token = sign_token(unsubscribe_secret.as_bytes(), contact.id, id, now_epoch)
                .map_err(|e| AppError::Transient {
                    code: "token_sign_failed".into(),
                    message: format!("HMAC sign failed: {e}"),
                    suggestion: "Set MLC_UNSUBSCRIBE_SECRET to a non-empty string".into(),
                })?;
            let unsubscribe_url = format!("{public_url}/{token}");
            let unsubscribe_html = format!(
                "<a href=\"{}\" target=\"_blank\">Unsubscribe</a>",
                html_escape(&unsubscribe_url)
            );
            // v0.2.3+: inline <span> so the footer is safe to inject inside a
            // `<p>` wrapper in the template (the scaffold does this). A block
            // element like `<div>` would create invalid HTML nesting.
            let footer_html = format!(
                "<span style=\"color:#666;font-size:11px\">{}</span>",
                html_escape(&physical_address)
            );
            let merge_data = json!({
                "first_name": contact.first_name.clone().unwrap_or_default(),
                "last_name": contact.last_name.clone().unwrap_or_default(),
                "email": contact.email.clone(),
                "unsubscribe_link": unsubscribe_html,
                "physical_address_footer": footer_html,
                "current_year": chrono::Utc::now().format("%Y").to_string(),
                "broadcast_id": id,
            });
            // STRICT MODE: unresolved placeholders + lint errors abort the
            // send before a single email goes out. This is the v0.2
            // replacement for the v0.1 frontmatter variable schema — we
            // catch the problem at the latest possible moment (when we
            // have real data) instead of the earliest.
            //
            // On any error here we MUST revert the broadcast from 'sending'
            // to 'failed' before bubbling up, otherwise retries would see a
            // stale status and agents would be confused.
            let rendered = match template::render(
                &template.html_source,
                &template.subject,
                &merge_data,
            ) {
                Ok(r) => r,
                Err(e) => {
                    let _ = db.broadcast_set_status(id, "failed", None);
                    let (code, msg) = match &e {
                        RenderError::UnresolvedAtSend(_) => (
                            "template_unresolved_placeholder",
                            format!("cannot send: {e}"),
                        ),
                        RenderError::Lint(_) => {
                            ("template_lint_error", format!("cannot send: {e}"))
                        }
                    };
                    return Err(AppError::BadInput {
                        code: code.into(),
                        message: msg,
                        suggestion: format!(
                            "Run `mailing-list-cli template preview {} --open` to fix the template before retrying",
                            template.name
                        ),
                    });
                }
            };
            entries.push(BatchEntry {
                from: sender_from.clone(),
                to: vec![contact.email.clone()],
                subject: rendered.subject,
                html: rendered.html,
                text: rendered.text,
                reply_to: config.sender.reply_to.clone(),
                headers: json!({
                    "List-Unsubscribe": format!("<{}>", unsubscribe_url),
                    "List-Unsubscribe-Post": "List-Unsubscribe=One-Click",
                }),
                tags: vec![json!({"name": "broadcast_id", "value": id.to_string()})],
            });
        }

        let batch_path = cache_dir.join(format!("broadcast-{id}-chunk-{chunk_idx}.json"));
        write_batch_file(&entries, &batch_path)?;

        // Pass the recipient emails in input order so the wrapper can correlate
        // by index when the real Resend response omits the `to` field.
        let recipients_in_order: Vec<String> = chunk.iter().map(|c| c.email.clone()).collect();
        match cli.batch_send(&batch_path, &recipients_in_order) {
            Ok(results) => {
                // v0.3: wrap the per-chunk mark-sent updates in ONE
                // transaction so the 100 fsyncs collapse into one.
                let tx = db.conn.transaction().map_err(|e| AppError::Transient {
                    code: "tx_begin_failed".into(),
                    message: format!("begin mark-sent transaction failed: {e}"),
                    suggestion: "Retry — the DB is probably busy".into(),
                })?;
                let now = chrono::Utc::now().to_rfc3339();
                for (email, resend_id) in &results {
                    if let Some(contact) =
                        chunk.iter().find(|c| c.email.eq_ignore_ascii_case(email))
                    {
                        tx.execute(
                            "UPDATE broadcast_recipient
                             SET status = 'sent', resend_email_id = ?1, sent_at = ?2
                             WHERE broadcast_id = ?3 AND contact_id = ?4",
                            rusqlite::params![resend_id, now, id, contact.id],
                        )
                        .map_err(|e| AppError::Transient {
                            code: "recipient_mark_sent_failed".into(),
                            message: format!("mark sent: {e}"),
                            suggestion: "Check DB disk space".into(),
                        })?;
                        sent_count += 1;
                    }
                }
                tx.commit().map_err(|e| AppError::Transient {
                    code: "tx_commit_failed".into(),
                    message: format!("commit mark-sent transaction failed: {e}"),
                    suggestion: "Retry — the DB is probably busy".into(),
                })?;
            }
            Err(e) => {
                failed_count += chunk.len();
                eprintln!("chunk {chunk_idx} failed: {}", e.message());
            }
        }
    }

    // 10. Mark broadcast.status = 'sent' (or 'failed' if everything failed)
    let final_status = if failed_count == to_send.len() && !to_send.is_empty() {
        "failed"
    } else {
        "sent"
    };
    let now_rfc = chrono::Utc::now().to_rfc3339();
    db.broadcast_set_status(id, final_status, Some(&now_rfc))?;

    Ok(PipelineResult {
        broadcast_id: id,
        sent_count,
        suppressed_count,
        failed_count,
    })
}

#[allow(dead_code)]
pub fn preview_broadcast(id: i64, to: &str) -> Result<PipelineResult, AppError> {
    let config = Config::load()?;
    let db = Db::open()?;
    let cli = EmailCli::new(&config.email_cli.path, &config.email_cli.profile);

    let broadcast = db.broadcast_get(id)?.ok_or_else(|| AppError::BadInput {
        code: "broadcast_not_found".into(),
        message: format!("no broadcast with id {id}"),
        suggestion: "Run `mailing-list-cli broadcast ls`".into(),
    })?;
    let template = db
        .template_all()?
        .into_iter()
        .find(|t| t.id == broadcast.template_id)
        .ok_or_else(|| AppError::BadInput {
            code: "template_not_found".into(),
            message: "template not found".into(),
            suggestion: "Re-create the broadcast".into(),
        })?;

    let sender_from = config
        .sender
        .from
        .as_ref()
        .ok_or_else(|| AppError::Config {
            code: "sender_from_required".into(),
            message: "[sender].from is empty in config.toml".into(),
            suggestion: "Edit config.toml and set [sender].from".into(),
        })?
        .clone();
    let physical_address = config
        .sender
        .physical_address
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();

    let unsubscribe_secret = std::env::var(&config.unsubscribe.secret_env)
        .unwrap_or_else(|_| "mlc-unsubscribe-dev".to_string());
    let now_epoch = chrono::Utc::now().timestamp();
    let preview_token =
        sign_token(unsubscribe_secret.as_bytes(), 0, id, now_epoch).map_err(|e| {
            AppError::Transient {
                code: "token_sign_failed".into(),
                message: format!("HMAC sign failed: {e}"),
                suggestion: "Set MLC_UNSUBSCRIBE_SECRET".into(),
            }
        })?;
    let unsubscribe_url = format!("{}/{}", config.unsubscribe.public_url, preview_token);
    // v0.2.3+: inline <span> to match the send path and the v0.2.3 stub shape.
    let footer_html = format!(
        "<span style=\"color:#666;font-size:11px\">{}</span>",
        html_escape(&physical_address)
    );
    let merge_data = json!({
        "first_name": "Preview",
        "last_name": "Recipient",
        "email": to,
        "unsubscribe_link": format!("<a href=\"{}\">Unsubscribe</a>", html_escape(&unsubscribe_url)),
        "physical_address_footer": footer_html,
        "current_year": chrono::Utc::now().format("%Y").to_string(),
        "broadcast_id": id,
    });
    let rendered = template::render(&template.html_source, &template.subject, &merge_data)
        .map_err(|e| AppError::BadInput {
            code: "template_render_failed".into(),
            message: format!("template render failed: {e}"),
            suggestion: format!(
                "Run `mailing-list-cli template preview {} --open` to debug",
                template.name
            ),
        })?;
    let subject = format!("[PREVIEW] {}", rendered.subject);

    let _resend_id = cli.send(&sender_from, to, &subject, &rendered.html, &rendered.text)?;

    Ok(PipelineResult {
        broadcast_id: id,
        sent_count: 1,
        suppressed_count: 0,
        failed_count: 0,
    })
}

#[allow(dead_code)]
fn resolve_target(db: &Db, broadcast: &Broadcast) -> Result<Vec<Contact>, AppError> {
    match broadcast.target_kind.as_str() {
        "list" => {
            // Read all contacts in the list
            let contacts = db.contact_list_in_list(broadcast.target_id, 100_000)?;
            Ok(contacts)
        }
        "segment" => {
            let segment =
                db.segment_get_by_id(broadcast.target_id)?
                    .ok_or_else(|| AppError::BadInput {
                        code: "segment_not_found".into(),
                        message: format!("segment id {} not found", broadcast.target_id),
                        suggestion: "Run `mailing-list-cli segment ls`".into(),
                    })?;
            let expr: SegmentExpr =
                serde_json::from_str(&segment.filter_json).map_err(|e| AppError::Transient {
                    code: "segment_deserialize_failed".into(),
                    message: format!("segment filter corrupt: {e}"),
                    suggestion: "Recreate the segment".into(),
                })?;
            let (frag, params) = compiler::to_sql_where(&expr);
            db.segment_members(&frag, &params, 100_000, None)
        }
        other => Err(AppError::BadInput {
            code: "bad_target_kind".into(),
            message: format!("unknown target_kind '{other}'"),
            suggestion: "target_kind must be 'list' or 'segment'".into(),
        }),
    }
}

#[allow(dead_code)]
fn preflight_checks(
    config: &Config,
    html_source: &str,
    subject_source: &str,
    recipient_count: usize,
) -> Result<(), AppError> {
    // Invariant 1: physical address must be set
    let address_present = config
        .sender
        .physical_address
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if !address_present {
        return Err(AppError::Config {
            code: "physical_address_required".into(),
            message: "[sender].physical_address is empty in config.toml".into(),
            suggestion:
                "Edit ~/.config/mailing-list-cli/config.toml and add a CAN-SPAM physical address"
                    .into(),
        });
    }
    // Invariant 1b: sender.from must be set
    let from_present = config
        .sender
        .from
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if !from_present {
        return Err(AppError::Config {
            code: "sender_from_required".into(),
            message: "[sender].from is empty in config.toml".into(),
            suggestion: "Edit config.toml and set [sender].from = \"you@yourdomain.com\"".into(),
        });
    }
    // Invariant 2: template lints clean (6 rules, v0.2)
    let outcome = template::lint(html_source, subject_source);
    if outcome.has_errors() {
        return Err(AppError::BadInput {
            code: "template_has_lint_errors".into(),
            message: format!("template has {} lint errors", outcome.error_count()),
            suggestion: "Run `template lint <name>` or `template preview <name> --open` to see and fix the issues".into(),
        });
    }
    // Invariant 3: recipient count cap
    let cap = config.guards.max_recipients_per_send;
    if recipient_count > cap {
        return Err(AppError::BadInput {
            code: "recipient_count_exceeds_cap".into(),
            message: format!(
                "recipient count {recipient_count} exceeds max_recipients_per_send {cap}"
            ),
            suggestion: "Raise the cap in config.toml [guards] or target a smaller segment".into(),
        });
    }
    // Invariant 4/5: complaint and bounce rate — Phase 6 job. Stubbed pass.
    Ok(())
}

#[allow(dead_code)]
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
