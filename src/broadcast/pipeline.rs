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
use sha2::{Digest, Sha256};

/// v0.3.2 (F2.1): SHA-256 of the batch file bytes, used as the
/// `request_sha256` column in `broadcast_send_attempt`. Lets us detect
/// "same chunk replayed" idempotently across crashes.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

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
pub fn send_broadcast(id: i64, force_unlock: bool) -> Result<PipelineResult, AppError> {
    let config = Config::load()?;
    // v0.3: mut because the per-chunk transaction blocks need
    // &mut Connection for conn.transaction(). v0.3.1: also needed for
    // broadcast_try_acquire_send_lock which uses a BEGIN IMMEDIATE
    // transaction.
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

    // v0.3.1: atomic lock acquire BEFORE any other work. Two simultaneous
    // `broadcast send 1` invocations both used to flip draft→sending and
    // double-send every recipient. Now exactly one acquires the lock; the
    // other gets `broadcast_lock_held` (exit code 1, transient).
    {
        use crate::db::LockAcquireResult;
        let pid = std::process::id() as i64;
        let stale_after = chrono::Duration::minutes(30);
        match db.broadcast_try_acquire_send_lock(id, pid, stale_after, force_unlock)? {
            LockAcquireResult::Acquired => {
                // Normal path. status was already flipped to 'sending' inside
                // the acquire's UPDATE, so the explicit broadcast_set_status
                // call further down is no longer needed.
            }
            LockAcquireResult::BrokeStale {
                previous_pid,
                locked_at,
            } => {
                eprintln!(
                    "warning: breaking stale lock from pid {previous_pid} (locked_at {locked_at}, > 30 min ago)"
                );
            }
            LockAcquireResult::AlreadyHeld {
                pid: holder,
                locked_at,
            } => {
                return Err(AppError::Transient {
                    code: "broadcast_lock_held".into(),
                    message: format!(
                        "broadcast {id} is already being sent by process {holder}, started {locked_at}"
                    ),
                    suggestion: format!(
                        "Wait for the other process to finish, OR if you're sure it died, run `broadcast send {id} --force-unlock`"
                    ),
                });
            }
        }
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
        &db,
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

    // v0.3.1: status is already 'sending' from broadcast_try_acquire_send_lock
    // above. The old `db.broadcast_set_status(id, "sending", None)?;` line was
    // redundant and has been removed.

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

    // v0.3: resume support — drop recipients already marked 'sent' from
    // a previous interrupted run. Combined with Task 3's per-chunk
    // transactions, this makes a mid-send crash cleanly recoverable via
    // `broadcast resume <id>` (or `broadcast send <id>`, same handler).
    let already_sent = db.broadcast_recipient_already_sent_ids(id)?;
    let resume_skipped = already_sent.len();
    if resume_skipped > 0 {
        eprintln!(
            "broadcast {id}: resume mode — {resume_skipped} recipient(s) already sent, skipping"
        );
        to_send.retain(|c| !already_sent.contains(&c.id));
    }
    db.broadcast_update_counts(id, to_send.len() as i64)?;

    // v0.3.2 (F2.1): write-ahead reconciliation — handle any send attempts
    // from a previous (crashed) run BEFORE processing new chunks.
    //
    // Two cases to handle:
    //   1. `esp_acked` rows: email-cli succeeded but the local recipient
    //      UPDATE never committed. We have the response_json stored, so we
    //      can re-apply it locally without re-calling email-cli (no
    //      duplicate sends). Mark applied.
    //   2. `prepared` rows: email-cli MAY have succeeded but the response
    //      was never recorded. We CANNOT determine whether Resend received
    //      the chunk. Refuse to proceed and surface the chunks for operator
    //      decision (the alternative — silently retrying — risks duplicate
    //      sends; the alternative — silently skipping — risks missed sends.
    //      Both are bad outcomes; the operator must choose).
    {
        let acked = db.broadcast_send_attempts_in_state(id, "esp_acked")?;
        for attempt in acked {
            eprintln!(
                "broadcast {id}: reconciling chunk {} from previous run (esp_acked → applied)",
                attempt.chunk_index
            );
            // Parse the stored applied_pairs and re-run the recipient UPDATE
            // in the same transaction that flips the attempt to 'applied'.
            let response: serde_json::Value = serde_json::from_str(
                attempt.esp_response_json.as_deref().unwrap_or("{}"),
            )
            .map_err(|e| AppError::Transient {
                code: "send_attempt_response_parse".into(),
                message: format!(
                    "could not parse stored esp_response_json for attempt {}: {e}",
                    attempt.id
                ),
                suggestion: format!(
                    "Inspect broadcast_send_attempt id={} manually: SELECT * FROM broadcast_send_attempt WHERE id={};",
                    attempt.id, attempt.id
                ),
            })?;
            let pairs = response
                .get("applied_pairs")
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default();
            let now = chrono::Utc::now().to_rfc3339();
            let tx = db.conn.transaction().map_err(|e| AppError::Transient {
                code: "tx_begin_failed".into(),
                message: format!("begin reconcile transaction failed: {e}"),
                suggestion: "Retry — the DB is probably busy".into(),
            })?;
            for pair in &pairs {
                let contact_id = pair.get("contact_id").and_then(|v| v.as_i64()).unwrap_or(0);
                let resend_id = pair.get("resend_id").and_then(|v| v.as_str()).unwrap_or("");
                if contact_id == 0 || resend_id.is_empty() {
                    continue;
                }
                tx.execute(
                    "UPDATE broadcast_recipient
                     SET status = 'sent', resend_email_id = ?1, sent_at = ?2
                     WHERE broadcast_id = ?3 AND contact_id = ?4",
                    rusqlite::params![resend_id, now, id, contact_id],
                )
                .map_err(|e| AppError::Transient {
                    code: "recipient_reconcile_failed".into(),
                    message: format!("reconcile recipient update: {e}"),
                    suggestion: "Check DB disk space".into(),
                })?;
            }
            tx.commit().map_err(|e| AppError::Transient {
                code: "tx_commit_failed".into(),
                message: format!("commit reconcile transaction failed: {e}"),
                suggestion: "Retry — the DB is probably busy".into(),
            })?;
            db.broadcast_send_attempt_mark_applied(attempt.id)?;
            // Update local already_sent so we don't try to re-send these
            // contacts in the new chunk loop.
            for pair in &pairs {
                if let Some(cid) = pair.get("contact_id").and_then(|v| v.as_i64()) {
                    to_send.retain(|c| c.id != cid);
                }
            }
        }

        let prepared = db.broadcast_send_attempts_in_state(id, "prepared")?;
        if !prepared.is_empty() {
            let chunk_indices: Vec<String> =
                prepared.iter().map(|a| a.chunk_index.to_string()).collect();
            return Err(AppError::Transient {
                code: "broadcast_attempt_indeterminate".into(),
                message: format!(
                    "broadcast {id} has {} chunk(s) in indeterminate state from a previous run (chunks: {}). The previous process crashed between starting an email-cli call and recording the response — we cannot tell if Resend received the chunk(s) or not.",
                    prepared.len(),
                    chunk_indices.join(", ")
                ),
                suggestion: format!(
                    "Inspect Resend dashboard or `email-cli email list` filtered by tag broadcast_id={id} to determine if the chunk(s) shipped. Then either: (a) manually mark applied with `sqlite3 STATE.DB \"UPDATE broadcast_send_attempt SET state='applied' WHERE id IN (...);\"` and re-run, or (b) mark failed with the same SQL pattern and re-run to retry the chunk. Refusing to auto-retry to avoid duplicate sends."
                ),
            });
        }
    }

    // 5-7. Per-recipient render (done inside the chunk loop below)
    // 8. Write JSON batch file + shell out in chunks of 100
    // v0.3.2 F13.1: hard-fail when MLC_UNSUBSCRIBE_SECRET is unset.
    // The previous fallback was "mlc-unsubscribe-dev" — a publicly-known
    // value committed to the source — meaning anyone running without the
    // env var was shipping forgeable unsubscribe tokens. Production must
    // refuse to sign tokens with a dev key.
    let unsubscribe_secret = std::env::var(&config.unsubscribe.secret_env)
        .map_err(|_| AppError::Config {
            code: "missing_unsubscribe_secret".into(),
            message: format!(
                "environment variable `{}` is not set; cannot sign unsubscribe links",
                config.unsubscribe.secret_env
            ),
            suggestion: format!(
                "Export {} with at least 16 bytes of random data (e.g. `export {}=$(openssl rand -hex 32)`) before running broadcast send",
                config.unsubscribe.secret_env, config.unsubscribe.secret_env
            ),
        })?;
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
                    // v0.3.1: clear the lock on failure so the broadcast can
                    // be inspected / re-tried without lock confusion.
                    let _ = db.broadcast_set_status_and_clear_lock(id, "failed", None);
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

        // v0.3.2 (F2.1): write-ahead attempt row BEFORE calling email-cli.
        // The previous pipeline called email-cli first; a crash between the
        // ESP ack and the local recipient UPDATE caused resume to resend the
        // chunk. Now we record the attempt so resume can reconcile.
        let batch_bytes = std::fs::read(&batch_path).map_err(|e| AppError::Transient {
            code: "batch_file_read".into(),
            message: format!("could not read batch file for hash: {e}"),
            suggestion: "Retry the command".into(),
        })?;
        let request_sha256 = sha256_hex(&batch_bytes);
        let attempt_id = db.broadcast_send_attempt_insert(
            id,
            chunk_idx as i64,
            &request_sha256,
            batch_path.to_str().unwrap_or(""),
        )?;

        // Pass the recipient emails in input order so the wrapper can correlate
        // by index when the real Resend response omits the `to` field.
        let recipients_in_order: Vec<String> = chunk.iter().map(|c| c.email.clone()).collect();
        match cli.batch_send(&batch_path, &recipients_in_order) {
            Ok(results) => {
                // v0.3.2 (F2.1): build the (contact_id, resend_id) correlation
                // BEFORE the local UPDATE. Store it as the canonical
                // applied_pairs in the attempt row, then mark esp_acked. This
                // is the recovery point: if we crash between this mark and the
                // UPDATE below, resume will reconcile from these stored pairs.
                let mut applied_pairs: Vec<serde_json::Value> = Vec::with_capacity(results.len());
                for (email, resend_id) in &results {
                    if let Some(contact) =
                        chunk.iter().find(|c| c.email.eq_ignore_ascii_case(email))
                    {
                        applied_pairs.push(json!({
                            "contact_id": contact.id,
                            "resend_id": resend_id
                        }));
                    }
                }
                let response_json = serde_json::to_string(&json!({
                    "applied_pairs": applied_pairs
                }))
                .unwrap_or_else(|_| "{}".to_string());
                db.broadcast_send_attempt_mark_esp_acked(attempt_id, &response_json)?;

                // v0.3: wrap the per-chunk mark-sent updates in ONE
                // transaction so the 100 fsyncs collapse into one.
                let tx = db.conn.transaction().map_err(|e| AppError::Transient {
                    code: "tx_begin_failed".into(),
                    message: format!("begin mark-sent transaction failed: {e}"),
                    suggestion: "Retry — the DB is probably busy".into(),
                })?;
                let now = chrono::Utc::now().to_rfc3339();
                for pair in &applied_pairs {
                    let contact_id = pair.get("contact_id").and_then(|v| v.as_i64()).unwrap_or(0);
                    let resend_id = pair.get("resend_id").and_then(|v| v.as_str()).unwrap_or("");
                    if contact_id == 0 || resend_id.is_empty() {
                        continue;
                    }
                    tx.execute(
                        "UPDATE broadcast_recipient
                         SET status = 'sent', resend_email_id = ?1, sent_at = ?2
                         WHERE broadcast_id = ?3 AND contact_id = ?4",
                        rusqlite::params![resend_id, now, id, contact_id],
                    )
                    .map_err(|e| AppError::Transient {
                        code: "recipient_mark_sent_failed".into(),
                        message: format!("mark sent: {e}"),
                        suggestion: "Check DB disk space".into(),
                    })?;
                    sent_count += 1;
                }
                tx.commit().map_err(|e| AppError::Transient {
                    code: "tx_commit_failed".into(),
                    message: format!("commit mark-sent transaction failed: {e}"),
                    suggestion: "Retry — the DB is probably busy".into(),
                })?;

                // v0.3.2 (F2.1): mark the attempt applied as the final step.
                // If we crash between the tx.commit above and this mark, the
                // attempt stays in esp_acked and the next reconcile pass will
                // replay the (now-already-applied) UPDATE — which is a no-op
                // because the recipient rows are already in 'sent' state. Safe.
                db.broadcast_send_attempt_mark_applied(attempt_id)?;
            }
            Err(e) => {
                failed_count += chunk.len();
                let _ = db.broadcast_send_attempt_mark_failed(attempt_id);
                eprintln!("chunk {chunk_idx} failed: {}", e.message());
            }
        }
    }

    // 10. Mark broadcast.status = 'sent' (or 'failed' if everything failed).
    // v0.3.1: clear the lock columns in the same UPDATE so subsequent
    // `broadcast resume` / `broadcast send` invocations don't see a stale
    // lock for a completed broadcast.
    let final_status = if failed_count == to_send.len() && !to_send.is_empty() {
        "failed"
    } else {
        "sent"
    };
    let now_rfc = chrono::Utc::now().to_rfc3339();
    db.broadcast_set_status_and_clear_lock(id, final_status, Some(&now_rfc))?;

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

    // v0.3.2 F13.1: hard-fail when MLC_UNSUBSCRIBE_SECRET is unset.
    // The previous fallback was "mlc-unsubscribe-dev" — a publicly-known
    // value committed to the source — meaning anyone running without the
    // env var was shipping forgeable unsubscribe tokens. Production must
    // refuse to sign tokens with a dev key.
    let unsubscribe_secret = std::env::var(&config.unsubscribe.secret_env)
        .map_err(|_| AppError::Config {
            code: "missing_unsubscribe_secret".into(),
            message: format!(
                "environment variable `{}` is not set; cannot sign unsubscribe links",
                config.unsubscribe.secret_env
            ),
            suggestion: format!(
                "Export {} with at least 16 bytes of random data (e.g. `export {}=$(openssl rand -hex 32)`) before running broadcast send",
                config.unsubscribe.secret_env, config.unsubscribe.secret_env
            ),
        })?;
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

/// Minimum delivered events in the 30-day window before the
/// complaint/bounce rate guards are enforced. Prevents a brand-new account
/// from being blocked by its first 10 bounces where the denominator is
/// statistically meaningless.
const MIN_DELIVERED_FOR_RATE_CHECK: i64 = 100;

/// Window (in days) for the complaint/bounce rate guards. Matches the
/// de-facto standard that Gmail/Yahoo use for reputation scoring.
const RATE_WINDOW_DAYS: i64 = 30;

#[allow(dead_code)]
fn preflight_checks(
    db: &Db,
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
    // Invariant 4+5 (v0.3): complaint and bounce rate over the last 30 days.
    // Statistically-meaningful threshold: require at least 100 delivered
    // events in the window before enforcing, so a brand-new account isn't
    // blocked by its first 10 bounces.
    let (complaint_rate, bounce_rate, delivered) = db.historical_send_rates(RATE_WINDOW_DAYS)?;
    if delivered >= MIN_DELIVERED_FOR_RATE_CHECK {
        if complaint_rate > config.guards.max_complaint_rate {
            return Err(AppError::BadInput {
                code: "complaint_rate_exceeds_guard".into(),
                message: format!(
                    "historical complaint rate {:.4}% over last {} days exceeds max_complaint_rate {:.4}% (delivered={delivered})",
                    complaint_rate * 100.0,
                    RATE_WINDOW_DAYS,
                    config.guards.max_complaint_rate * 100.0,
                ),
                suggestion: "Investigate recent complaint sources before sending more; consider pruning the list or pausing sends for 7 days. Override by raising [guards].max_complaint_rate in config.toml (NOT recommended — Gmail/Yahoo will block you at 0.3%).".into(),
            });
        }
        if bounce_rate > config.guards.max_bounce_rate {
            return Err(AppError::BadInput {
                code: "bounce_rate_exceeds_guard".into(),
                message: format!(
                    "historical bounce rate {:.4}% over last {} days exceeds max_bounce_rate {:.4}% (delivered={delivered})",
                    bounce_rate * 100.0,
                    RATE_WINDOW_DAYS,
                    config.guards.max_bounce_rate * 100.0,
                ),
                suggestion: "Prune hard-bounced contacts before sending more; most have already been auto-suppressed but list hygiene matters. Override by raising [guards].max_bounce_rate (NOT recommended).".into(),
            });
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
