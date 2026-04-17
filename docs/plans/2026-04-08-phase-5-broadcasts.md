# Phase 5: Broadcasts + Send Pipeline — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `mailing-list-cli v0.1.1` — named, targeted broadcasts + the full send pipeline from spec §5. After this phase, `broadcast create --name "Q4 newsletter" --template welcome --to segment:engaged` stages a campaign, and `broadcast send <id>` puts real mail in real inboxes via `email-cli batch send`, with per-recipient merge tags, per-recipient unsubscribe tokens, CAN-SPAM footers, and all the compliance guardrails enforced at the dispatch boundary. This ships the user's explicit ask: **"segment batches and give them names."**

**Architecture:** A new `src/broadcast/` module owns the send pipeline. `broadcast create` stores a draft row in the `broadcast` table (schema already in place from migration 0001). `broadcast send` walks the pipeline: resolve target → pre-flight invariants → suppression filter → per-recipient render → write JSON batch file → shell out to `email-cli batch send --file <path>` in chunks of 100 → update `broadcast_recipient` rows. Per-recipient merge tags mean we render the template N times, not once — `css-inline` runs once on the raw template (post-mrml) and handlebars substitutes on the inlined HTML for each recipient. Unsubscribe URLs are HMAC-signed tokens (RFC 8058-compatible) baked into each recipient's rendered copy via the `{{{ unsubscribe_link }}}` placeholder.

**Tech Stack:** Rust 2024 edition. New deps: `hmac 0.12`, `sha2 0.10`, `base64 0.22`. Existing: `rusqlite`, `serde_json`, `chrono`, `handlebars`, `mrml`, `css-inline`, `html2text` (all from Phase 4).

**Spec reference:** [`docs/specs/2026-04-07-mailing-list-cli-design.md`](../specs/2026-04-07-mailing-list-cli-design.md) §4.6 (broadcast commands), §5 (send pipeline), §9 (compliance boundaries), §10 (webhook listener — Phase 6 scope, noted here only), §13.4 (email-cli send surface). [`docs/plans/2026-04-08-parity-plan.md`](./2026-04-08-parity-plan.md) §5 Phase 5.

**Prerequisite:** Phase 4 is complete (v0.1.0 tagged). The `broadcast`, `broadcast_recipient`, and `suppression` tables are ALREADY in place from migration 0001. No schema work in Phase 5.

---

## Locked design decisions

1. **Primary send path is `email-cli batch send --file <path>`**, not `email-cli broadcast create`. Reason: batch send gives us per-recipient HTML control (distinct merge data per recipient), while `broadcast create` takes one HTML blob and expects Resend to handle per-recipient substitution via its own tag syntax. Spec §5 describes batch send. `email-cli broadcast create` is deferred to Phase 5.5 or v0.2.
2. **Chunk size is 100** (Resend's batch send limit). Configurable via `config.toml` `[guards].batch_chunk_size` with default 100.
3. **Rate limit** comes from the existing `EmailCli::throttle()` (200ms per subprocess call). A 10k recipient send takes 10000/100 * 0.2s = 20s minimum just for throttle waits, plus the actual API time. Acceptable.
4. **Per-recipient render is serialized**, not parallelized. Rust `rayon` would parallelize the CPU-bound work but complicates error handling. 10k renders at ~5ms each = 50s. Still acceptable. Defer parallelization to v0.2+ when someone has a 100k list.
5. **CSS inlining runs ONCE per send** on the template's post-mrml HTML (before per-recipient handlebars substitution). This is the Gemini review point from Phase 4 — inlining is expensive and deterministic, so we do it once. Per-recipient rendering happens on the already-inlined HTML.
6. **Unsubscribe token = HMAC-SHA256(secret, `{contact_id}:{broadcast_id}:{issued_at}`)**, base64url-encoded. The secret comes from `[unsubscribe].secret_env` in config.toml (an env var name; the actual secret is read from the env at send time). Token is embedded in the one-click unsubscribe URL as `{public_url}/{token}`.
7. **Pre-flight invariant failures return exit 2 (Config)** because the fix is operator action, not input fix. The invariants (spec §9.1):
   - Physical address configured in `config.toml`
   - Sender domain authenticated (shell out to `email-cli profile test`)
   - Complaint rate < 0.30% in last 7 days (skip in Phase 5 — requires event mirror from Phase 6; stub the check to always pass)
   - Bounce rate < 4% in last 7 days (same — stub pass)
   - Recipient count ≤ `max_recipients_per_send` (default 50000)
   - Template lints clean (all errors = 0)
8. **Suppression filter reads `suppression` table** via case-insensitive email match. Phase 5 is read-only against `suppression`; Phase 7 adds CRUD. The filter is a hard boundary — if a recipient email is in `suppression`, the row is rewritten as `status='suppressed'` in `broadcast_recipient` and excluded from the batch file.
9. **`broadcast preview <id> --to <email>`** calls the render pipeline once with a synthetic single-recipient merge data dict, writes a one-entry batch file, and calls `email-cli send` (the transactional endpoint, not batch). This gives the operator a real inbox copy without committing the full broadcast.
10. **`broadcast send` is idempotent up to the suppression filter**: on a crash mid-send, `broadcast_recipient` has the rows that made it into batch files, and re-running `broadcast send <id>` on a `sending` status broadcast picks up from where it left off by skipping already-`sent` recipients. Partial failures get per-chunk retry (`broadcast retry-chunk`) in Phase 5.5 — Phase 5 just marks `status='failed'` on whole-batch failure.
11. **`broadcast ls`** shows draft + scheduled + sending + sent + cancelled + failed. `broadcast show <id>` returns all 12 denormalized stat columns (recipient_count, delivered_count, bounced_count, etc.) even though most are zero until Phase 6 webhooks.
12. **The real-Resend smoke test (Task 15)** uses the `paperfoot.com` domain (verified, both sending + receiving in the user's Resend account), `email-cli profile: local`, and sends to `smoke-test-v0.1.1@paperfoot.com` (a receiver on the same domain). After the send, `email-cli inbox poll` verifies the message arrived. This is the empirical test that v0.1.1 actually works end-to-end.
13. **Physical address footer injection happens post-handlebars, pre-batch-write**. The injected HTML is the content of `config.sender.physical_address` wrapped in a centered, small-gray `<mj-text>` styled `div`. Plain-text alt gets the address as a trailing line.
14. **List-Unsubscribe headers** are added to every batch entry's `headers` field:
    - `List-Unsubscribe`: `<https://hooks.yourdomain.com/u/{token}>, <mailto:unsubscribe+{token}@yourdomain.com>`
    - `List-Unsubscribe-Post`: `List-Unsubscribe=One-Click`
    The `https://` URL is the one the webhook listener (Phase 6) will handle. Phase 5 writes the header; Phase 6 implements the receiving endpoint.

---

## File structure

Created by this phase:

```
mailing-list-cli/
├── src/
│   ├── broadcast/
│   │   ├── mod.rs              # public API: send(), preview(), pipeline()
│   │   ├── pipeline.rs         # the 10-step send pipeline
│   │   ├── unsubscribe.rs      # HMAC token signer
│   │   └── batch.rs            # JSON batch file writer
│   └── commands/
│       └── broadcast.rs        # CLI dispatch for all broadcast subcommands
```

Modified:

```
├── Cargo.toml                  # +hmac, +sha2, +base64
├── src/
│   ├── cli.rs                  # +Broadcast subcommand + BroadcastAction
│   ├── main.rs                 # mod broadcast; + dispatch
│   ├── config.rs               # +unsubscribe section (secret_env, public_url)
│   ├── db/mod.rs               # +broadcast_*, broadcast_recipient_* helpers
│   ├── email_cli.rs            # +batch_send() method, +send() method
│   ├── models.rs               # +Broadcast, BroadcastRecipient structs
│   └── commands/
│       ├── mod.rs              # +pub mod broadcast
│       └── agent_info.rs       # advertise broadcast commands + v0.1.1 status
└── tests/
    ├── cli.rs                  # new integration tests per command
    └── fixtures/
        └── stub-email-cli.sh   # mock batch send, send, profile test
```

---

## Task 1: Dependencies + config extension

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/config.rs`

- [ ] **Step 1: Add crypto deps**

Append to `[dependencies]` in `Cargo.toml` (alphabetical):

```toml
base64 = "0.22"
hmac = "0.12"
sha2 = "0.10"
```

- [ ] **Step 2: Extend the config schema**

Edit `src/config.rs` to add an `unsubscribe` section. The existing `Config` struct has `sender` and `email_cli`; add:

```rust
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct UnsubscribeConfig {
    #[serde(default = "default_unsubscribe_public_url")]
    pub public_url: String,
    #[serde(default = "default_unsubscribe_secret_env")]
    pub secret_env: String,
}

fn default_unsubscribe_public_url() -> String {
    "https://hooks.yourdomain.com/u".to_string()
}

fn default_unsubscribe_secret_env() -> String {
    "MLC_UNSUBSCRIBE_SECRET".to_string()
}
```

Add `pub unsubscribe: UnsubscribeConfig` to the `Config` struct with `#[serde(default)]`. Update the test config.toml used in integration tests to include an `[unsubscribe]` block (or rely on defaults — they're fine for tests).

- [ ] **Step 3: Verify + commit**

```bash
cargo build
cargo clippy --all-targets -- -D warnings
git add Cargo.toml Cargo.lock src/config.rs
git commit -m "feat(deps): hmac/sha2/base64 for unsubscribe tokens + config.unsubscribe section"
```

---

## Task 2: HMAC unsubscribe token signer

**Files:**
- Create: `src/broadcast/mod.rs`
- Create: `src/broadcast/unsubscribe.rs`
- Modify: `src/main.rs` (add `mod broadcast;`)

- [ ] **Step 1: Create the broadcast module root**

Create `src/broadcast/mod.rs`:

```rust
//! Broadcast module: campaigns, send pipeline, unsubscribe tokens, batch writer.

pub mod batch;
pub mod pipeline;
pub mod unsubscribe;

pub use batch::{BatchEntry, write_batch_file};
pub use pipeline::{PipelineError, PipelineResult, send_broadcast, preview_broadcast};
pub use unsubscribe::{TokenError, sign_token, verify_token};
```

(Empty stubs for `batch.rs` and `pipeline.rs` are created in later tasks — Task 2 just needs `unsubscribe.rs` to land + the module to compile. Create `batch.rs` and `pipeline.rs` as 1-line doc comments for now to unblock the re-exports. We'll fill them in Tasks 4, 5, 6.)

Create stub `src/broadcast/batch.rs`:

```rust
//! JSON batch file writer for `email-cli batch send`. Full impl in Task 4.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct BatchEntry {
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub html: String,
    pub text: String,
    pub headers: serde_json::Value,
    pub tags: Vec<serde_json::Value>,
}

pub fn write_batch_file(
    _entries: &[BatchEntry],
    _path: &std::path::Path,
) -> Result<(), crate::error::AppError> {
    Err(crate::error::AppError::Transient {
        code: "batch_not_implemented".into(),
        message: "write_batch_file not yet implemented".into(),
        suggestion: "Task 4 implements this".into(),
    })
}
```

Create stub `src/broadcast/pipeline.rs`:

```rust
//! Broadcast send pipeline. Full impl in Task 6.

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("pipeline error: {0}")]
    Generic(String),
}

pub struct PipelineResult {
    pub sent_count: usize,
    pub suppressed_count: usize,
    pub failed_count: usize,
}

pub fn send_broadcast(_id: i64) -> Result<PipelineResult, crate::error::AppError> {
    Err(crate::error::AppError::Transient {
        code: "pipeline_not_implemented".into(),
        message: "send_broadcast not yet implemented".into(),
        suggestion: "Task 6 implements this".into(),
    })
}

pub fn preview_broadcast(
    _id: i64,
    _to: &str,
) -> Result<PipelineResult, crate::error::AppError> {
    Err(crate::error::AppError::Transient {
        code: "preview_not_implemented".into(),
        message: "preview_broadcast not yet implemented".into(),
        suggestion: "Task 6 implements this".into(),
    })
}
```

- [ ] **Step 2: Implement the HMAC token signer**

Create `src/broadcast/unsubscribe.rs`:

```rust
//! HMAC-SHA256 unsubscribe token signer.
//!
//! Token format: base64url(HMAC-SHA256(secret, "contact_id:broadcast_id:issued_at"))
//! followed by "." and the payload base64url-encoded for verification.
//!
//! Final token: "<payload_b64>.<sig_b64>"

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("hmac key error: {0}")]
    Key(String),
    #[error("invalid token format")]
    InvalidFormat,
    #[error("signature mismatch")]
    BadSignature,
    #[error("base64 decode error: {0}")]
    Base64(String),
}

pub fn sign_token(
    secret: &[u8],
    contact_id: i64,
    broadcast_id: i64,
    issued_at: i64,
) -> Result<String, TokenError> {
    let payload = format!("{contact_id}:{broadcast_id}:{issued_at}");
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.as_bytes());

    let mut mac = HmacSha256::new_from_slice(secret).map_err(|e| TokenError::Key(e.to_string()))?;
    mac.update(payload.as_bytes());
    let sig = mac.finalize().into_bytes();
    let sig_b64 = URL_SAFE_NO_PAD.encode(sig);

    Ok(format!("{payload_b64}.{sig_b64}"))
}

/// Returns `(contact_id, broadcast_id, issued_at)` on success.
pub fn verify_token(secret: &[u8], token: &str) -> Result<(i64, i64, i64), TokenError> {
    let (payload_b64, sig_b64) = token.split_once('.').ok_or(TokenError::InvalidFormat)?;
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|e| TokenError::Base64(e.to_string()))?;
    let sig_bytes = URL_SAFE_NO_PAD
        .decode(sig_b64)
        .map_err(|e| TokenError::Base64(e.to_string()))?;

    let mut mac = HmacSha256::new_from_slice(secret).map_err(|e| TokenError::Key(e.to_string()))?;
    mac.update(&payload_bytes);
    mac.verify_slice(&sig_bytes).map_err(|_| TokenError::BadSignature)?;

    let payload = std::str::from_utf8(&payload_bytes).map_err(|_| TokenError::InvalidFormat)?;
    let parts: Vec<&str> = payload.split(':').collect();
    if parts.len() != 3 {
        return Err(TokenError::InvalidFormat);
    }
    let contact_id: i64 = parts[0].parse().map_err(|_| TokenError::InvalidFormat)?;
    let broadcast_id: i64 = parts[1].parse().map_err(|_| TokenError::InvalidFormat)?;
    let issued_at: i64 = parts[2].parse().map_err(|_| TokenError::InvalidFormat)?;
    Ok((contact_id, broadcast_id, issued_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_then_verify_round_trip() {
        let secret = b"test_secret_0123456789";
        let token = sign_token(secret, 42, 7, 1234567890).unwrap();
        let (cid, bid, ts) = verify_token(secret, &token).unwrap();
        assert_eq!(cid, 42);
        assert_eq!(bid, 7);
        assert_eq!(ts, 1234567890);
    }

    #[test]
    fn verify_rejects_wrong_secret() {
        let token = sign_token(b"secret_a", 1, 1, 0).unwrap();
        let err = verify_token(b"secret_b", &token).unwrap_err();
        matches!(err, TokenError::BadSignature);
    }

    #[test]
    fn verify_rejects_malformed_token() {
        assert!(verify_token(b"x", "notatoken").is_err());
    }

    #[test]
    fn tokens_are_url_safe() {
        let token = sign_token(b"secret", 1, 1, 0).unwrap();
        assert!(token.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-'));
    }
}
```

- [ ] **Step 3: Wire the module**

Add `mod broadcast;` to `src/main.rs` alphabetically after `mod batch` (there is none — add alongside other `mod` declarations after `mod paths;`).

- [ ] **Step 4: Run tests + commit**

```bash
cargo test broadcast::unsubscribe
cargo clippy --all-targets -- -D warnings
git add src/broadcast/ src/main.rs
git commit -m "feat(broadcast): HMAC-SHA256 unsubscribe token signer"
```

Expected: 4 new unit tests pass.

---

## Task 3: Broadcast DB helpers + models

**Files:**
- Modify: `src/models.rs`
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Add `Broadcast` and `BroadcastRecipient` model structs**

Append to `src/models.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct Broadcast {
    pub id: i64,
    pub name: String,
    pub template_id: i64,
    pub target_kind: String, // "list" | "segment"
    pub target_id: i64,
    pub status: String,      // draft/scheduled/sending/sent/cancelled/failed
    pub scheduled_at: Option<String>,
    pub sent_at: Option<String>,
    pub created_at: String,
    pub recipient_count: i64,
    pub delivered_count: i64,
    pub bounced_count: i64,
    pub opened_count: i64,
    pub clicked_count: i64,
    pub unsubscribed_count: i64,
    pub complained_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BroadcastRecipient {
    pub id: i64,
    pub broadcast_id: i64,
    pub contact_id: i64,
    pub resend_email_id: Option<String>,
    pub status: String, // pending/sent/delivered/bounced/complained/failed/suppressed
    pub sent_at: Option<String>,
    pub last_event_at: Option<String>,
}
```

- [ ] **Step 2: Add `broadcast_*` DB helpers**

Append to `impl Db` in `src/db/mod.rs`:

```rust
    // ─── Broadcast operations ──────────────────────────────────────────

    pub fn broadcast_create(
        &self,
        name: &str,
        template_id: i64,
        target_kind: &str,
        target_id: i64,
    ) -> Result<i64, AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO broadcast (name, template_id, target_kind, target_id, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'draft', ?5)",
                params![name, template_id, target_kind, target_id, now],
            )
            .map_err(query_err)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn broadcast_all(&self, status_filter: Option<&str>, limit: usize) -> Result<Vec<crate::models::Broadcast>, AppError> {
        let (sql, has_status) = if status_filter.is_some() {
            (
                "SELECT id, name, template_id, target_kind, target_id, status, scheduled_at, sent_at, created_at,
                        recipient_count, delivered_count, bounced_count, opened_count, clicked_count,
                        unsubscribed_count, complained_count
                 FROM broadcast WHERE status = ?1 ORDER BY id DESC LIMIT ?2",
                true,
            )
        } else {
            (
                "SELECT id, name, template_id, target_kind, target_id, status, scheduled_at, sent_at, created_at,
                        recipient_count, delivered_count, bounced_count, opened_count, clicked_count,
                        unsubscribed_count, complained_count
                 FROM broadcast ORDER BY id DESC LIMIT ?1",
                false,
            )
        };
        let mut stmt = self.conn.prepare(sql).map_err(query_err)?;

        let row_mapper = |row: &rusqlite::Row| {
            Ok(crate::models::Broadcast {
                id: row.get(0)?,
                name: row.get(1)?,
                template_id: row.get(2)?,
                target_kind: row.get(3)?,
                target_id: row.get(4)?,
                status: row.get(5)?,
                scheduled_at: row.get(6)?,
                sent_at: row.get(7)?,
                created_at: row.get(8)?,
                recipient_count: row.get(9)?,
                delivered_count: row.get(10)?,
                bounced_count: row.get(11)?,
                opened_count: row.get(12)?,
                clicked_count: row.get(13)?,
                unsubscribed_count: row.get(14)?,
                complained_count: row.get(15)?,
            })
        };

        let rows: Vec<crate::models::Broadcast> = if has_status {
            stmt.query_map(params![status_filter.unwrap(), limit as i64], row_mapper)
                .map_err(query_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(query_err)?
        } else {
            stmt.query_map(params![limit as i64], row_mapper)
                .map_err(query_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(query_err)?
        };
        Ok(rows)
    }

    pub fn broadcast_get(&self, id: i64) -> Result<Option<crate::models::Broadcast>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, template_id, target_kind, target_id, status, scheduled_at, sent_at, created_at,
                        recipient_count, delivered_count, bounced_count, opened_count, clicked_count,
                        unsubscribed_count, complained_count
                 FROM broadcast WHERE id = ?1",
            )
            .map_err(query_err)?;
        let row = stmt.query_row(params![id], |row| {
            Ok(crate::models::Broadcast {
                id: row.get(0)?,
                name: row.get(1)?,
                template_id: row.get(2)?,
                target_kind: row.get(3)?,
                target_id: row.get(4)?,
                status: row.get(5)?,
                scheduled_at: row.get(6)?,
                sent_at: row.get(7)?,
                created_at: row.get(8)?,
                recipient_count: row.get(9)?,
                delivered_count: row.get(10)?,
                bounced_count: row.get(11)?,
                opened_count: row.get(12)?,
                clicked_count: row.get(13)?,
                unsubscribed_count: row.get(14)?,
                complained_count: row.get(15)?,
            })
        });
        match row {
            Ok(b) => Ok(Some(b)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    pub fn broadcast_set_status(
        &self,
        id: i64,
        status: &str,
        sent_at: Option<&str>,
    ) -> Result<(), AppError> {
        self.conn
            .execute(
                "UPDATE broadcast SET status = ?1, sent_at = COALESCE(?2, sent_at) WHERE id = ?3",
                params![status, sent_at, id],
            )
            .map_err(query_err)?;
        Ok(())
    }

    pub fn broadcast_set_scheduled(
        &self,
        id: i64,
        scheduled_at: &str,
    ) -> Result<(), AppError> {
        self.conn
            .execute(
                "UPDATE broadcast SET status = 'scheduled', scheduled_at = ?1 WHERE id = ?2",
                params![scheduled_at, id],
            )
            .map_err(query_err)?;
        Ok(())
    }

    pub fn broadcast_update_counts(
        &self,
        id: i64,
        recipient_count: i64,
    ) -> Result<(), AppError> {
        self.conn
            .execute(
                "UPDATE broadcast SET recipient_count = ?1 WHERE id = ?2",
                params![recipient_count, id],
            )
            .map_err(query_err)?;
        Ok(())
    }

    // ─── Broadcast recipient operations ────────────────────────────────

    pub fn broadcast_recipient_insert(
        &self,
        broadcast_id: i64,
        contact_id: i64,
        status: &str,
    ) -> Result<i64, AppError> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO broadcast_recipient (broadcast_id, contact_id, status)
                 VALUES (?1, ?2, ?3)",
                params![broadcast_id, contact_id, status],
            )
            .map_err(query_err)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn broadcast_recipient_mark_sent(
        &self,
        broadcast_id: i64,
        contact_id: i64,
        resend_email_id: &str,
    ) -> Result<(), AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE broadcast_recipient
                 SET status = 'sent', resend_email_id = ?1, sent_at = ?2
                 WHERE broadcast_id = ?3 AND contact_id = ?4",
                params![resend_email_id, now, broadcast_id, contact_id],
            )
            .map_err(query_err)?;
        Ok(())
    }

    pub fn broadcast_recipient_count_by_status(
        &self,
        broadcast_id: i64,
        status: &str,
    ) -> Result<i64, AppError> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM broadcast_recipient WHERE broadcast_id = ?1 AND status = ?2",
                params![broadcast_id, status],
                |r| r.get(0),
            )
            .map_err(query_err)?;
        Ok(count)
    }

    /// Check if an email is on the global suppression list.
    pub fn is_email_suppressed(&self, email: &str) -> Result<bool, AppError> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM suppression WHERE email = ?1 COLLATE NOCASE",
                params![email],
                |r| r.get(0),
            )
            .map_err(query_err)?;
        Ok(count > 0)
    }
```

- [ ] **Step 3: Add DB tests**

Append to the `tests` module:

```rust
    #[test]
    fn broadcast_crud_round_trip() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        // Need a template to satisfy FK
        let tid = db.template_upsert("t", "Hi", "<mjml></mjml>", "{}").unwrap();
        let list_id = db.list_create("news", None, "seg_x").unwrap();

        let bid = db.broadcast_create("Q1", tid, "list", list_id).unwrap();
        assert!(bid > 0);
        let b = db.broadcast_get(bid).unwrap().unwrap();
        assert_eq!(b.name, "Q1");
        assert_eq!(b.status, "draft");

        db.broadcast_set_status(bid, "sending", None).unwrap();
        db.broadcast_set_status(bid, "sent", Some("2026-04-08T12:00:00Z")).unwrap();
        let b = db.broadcast_get(bid).unwrap().unwrap();
        assert_eq!(b.status, "sent");
        assert_eq!(b.sent_at.as_deref(), Some("2026-04-08T12:00:00Z"));

        let all = db.broadcast_all(None, 100).unwrap();
        assert_eq!(all.len(), 1);
        let sent = db.broadcast_all(Some("sent"), 100).unwrap();
        assert_eq!(sent.len(), 1);
        let draft = db.broadcast_all(Some("draft"), 100).unwrap();
        assert_eq!(draft.len(), 0);
    }

    #[test]
    fn broadcast_recipient_crud() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let tid = db.template_upsert("t", "Hi", "<mjml></mjml>", "{}").unwrap();
        let list_id = db.list_create("news", None, "seg_x").unwrap();
        let bid = db.broadcast_create("Q1", tid, "list", list_id).unwrap();
        let cid = db.contact_upsert("alice@example.com", None, None).unwrap();

        db.broadcast_recipient_insert(bid, cid, "pending").unwrap();
        db.broadcast_recipient_mark_sent(bid, cid, "em_abc").unwrap();
        assert_eq!(db.broadcast_recipient_count_by_status(bid, "sent").unwrap(), 1);
    }

    #[test]
    fn suppression_read_check() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        db.conn
            .execute(
                "INSERT INTO suppression (email, reason, suppressed_at) VALUES ('blocked@example.com', 'hard_bounced', '2026-01-01')",
                [],
            )
            .unwrap();
        assert!(db.is_email_suppressed("blocked@example.com").unwrap());
        assert!(db.is_email_suppressed("BLOCKED@example.com").unwrap()); // COLLATE NOCASE
        assert!(!db.is_email_suppressed("alice@example.com").unwrap());
    }
```

- [ ] **Step 4: Run tests + commit**

```bash
cargo test db::tests::broadcast db::tests::suppression_read
cargo clippy --all-targets -- -D warnings
git add src/models.rs src/db/mod.rs
git commit -m "feat(db): broadcast_* and broadcast_recipient_* helpers + suppression read"
```

Expected: 3 new unit tests pass.

---

## Task 4: JSON batch file writer

**Files:**
- Modify: `src/broadcast/batch.rs`

- [ ] **Step 1: Implement `write_batch_file`**

Replace `src/broadcast/batch.rs` with:

```rust
//! JSON batch file writer for `email-cli batch send`.
//!
//! Resend's batch send takes up to 100 entries per call. Each entry is a full
//! send request: from, to, subject, html, text, headers, tags.

use crate::error::AppError;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct BatchEntry {
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub html: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    pub headers: serde_json::Value,
    pub tags: Vec<serde_json::Value>,
}

pub fn write_batch_file(entries: &[BatchEntry], path: &Path) -> Result<(), AppError> {
    let json = serde_json::to_string_pretty(entries).map_err(|e| AppError::Transient {
        code: "batch_serialize_failed".into(),
        message: format!("could not serialize batch entries: {e}"),
        suggestion: "Check for non-UTF-8 bytes in template output".into(),
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Transient {
            code: "batch_mkdir_failed".into(),
            message: format!("could not create batch dir {}: {e}", parent.display()),
            suggestion: "Check ~/.cache/mailing-list-cli/ permissions".into(),
        })?;
    }
    std::fs::write(path, json).map_err(|e| AppError::Transient {
        code: "batch_write_failed".into(),
        message: format!("could not write batch file {}: {e}", path.display()),
        suggestion: "Check filesystem write permissions".into(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn writes_batch_file_as_json_array() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let entry = BatchEntry {
            from: "sender@example.com".into(),
            to: vec!["alice@example.com".into()],
            subject: "Hi".into(),
            html: "<p>hello</p>".into(),
            text: "hello".into(),
            reply_to: None,
            headers: json!({"List-Unsubscribe": "<https://x/u/tok>"}),
            tags: vec![json!({"name": "broadcast_id", "value": "1"})],
        };
        write_batch_file(&[entry], tmp.path()).unwrap();
        let content = std::fs::read_to_string(tmp.path()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed[0]["from"], "sender@example.com");
        assert_eq!(parsed[0]["to"][0], "alice@example.com");
    }
}
```

- [ ] **Step 2: Test + commit**

```bash
cargo test broadcast::batch
git add src/broadcast/batch.rs
git commit -m "feat(broadcast): JSON batch file writer"
```

---

## Task 5: Extend `EmailCli` with `batch_send` and `send`

**Files:**
- Modify: `src/email_cli.rs`
- Modify: `tests/fixtures/stub-email-cli.sh`

- [ ] **Step 1: Add `batch_send` and `send` methods**

Add to `impl EmailCli` in `src/email_cli.rs`:

```rust
    /// Shell out to `email-cli batch send --file <path>`. Returns a Vec of
    /// `(recipient_email, resend_email_id)` pairs from the response.
    pub fn batch_send(&self, batch_file: &std::path::Path) -> Result<Vec<(String, String)>, AppError> {
        self.throttle();
        let output = Command::new(&self.path)
            .args([
                "--json",
                "batch",
                "send",
                "--file",
                batch_file.to_str().unwrap_or(""),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Config {
                code: "email_cli_invoke_failed".into(),
                message: format!("could not run email-cli batch send: {e}"),
                suggestion: "Check that email-cli is on PATH (v0.6+ required)".into(),
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Transient {
                code: "batch_send_failed".into(),
                message: format!(
                    "email-cli batch send failed: {}",
                    stderr.lines().next().unwrap_or("(no stderr)")
                ),
                suggestion: "Run `email-cli profile test` to verify Resend connectivity".into(),
            });
        }
        let parsed: Value =
            serde_json::from_slice(&output.stdout).map_err(|e| AppError::Transient {
                code: "batch_send_parse".into(),
                message: format!("invalid JSON from email-cli batch send: {e}"),
                suggestion: "Check email-cli version (v0.6+ required)".into(),
            })?;
        // Response shape: {"data": [{"id": "em_...", "to": "alice@..."}]}
        let items = parsed
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| AppError::Transient {
                code: "batch_send_no_data".into(),
                message: "email-cli batch send response has no `data` array".into(),
                suggestion: "Check email-cli version compatibility".into(),
            })?;
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let id = item
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let to = item
                .get("to")
                .and_then(|v| v.as_str())
                .or_else(|| item.get("to").and_then(|v| v.as_array()).and_then(|a| a.first()).and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string();
            out.push((to, id));
        }
        Ok(out)
    }

    /// Shell out to `email-cli send` for single-recipient transactional sends.
    pub fn send(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        html: &str,
        text: &str,
    ) -> Result<String, AppError> {
        self.throttle();
        let output = Command::new(&self.path)
            .args([
                "--json", "send", "--account", &self.profile, "--to", to, "--from", from,
                "--subject", subject, "--html", html, "--text", text,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Config {
                code: "email_cli_invoke_failed".into(),
                message: format!("could not run email-cli send: {e}"),
                suggestion: "Check that email-cli is on PATH".into(),
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Transient {
                code: "send_failed".into(),
                message: format!(
                    "email-cli send failed: {}",
                    stderr.lines().next().unwrap_or("(no stderr)")
                ),
                suggestion: "Run `email-cli profile test` to verify Resend connectivity".into(),
            });
        }
        let parsed: Value =
            serde_json::from_slice(&output.stdout).map_err(|e| AppError::Transient {
                code: "send_parse".into(),
                message: format!("invalid JSON from email-cli send: {e}"),
                suggestion: "Check email-cli version compatibility".into(),
            })?;
        let id = parsed
            .get("data")
            .and_then(|d| d.get("id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Transient {
                code: "send_no_id".into(),
                message: "email-cli send response missing data.id".into(),
                suggestion: "Check email-cli version compatibility".into(),
            })?;
        Ok(id.to_string())
    }
```

- [ ] **Step 2: Extend stub email-cli with batch and send mocks**

Edit `tests/fixtures/stub-email-cli.sh`, adding to the top-level case statement:

```sh
    "batch")
        if [ "$2" = "send" ]; then
            # Return a canned success with one per-recipient entry
            echo '{"version":"1","status":"success","data":[{"id":"em_stub_1","to":"alice@example.com"}]}'
            exit 0
        fi
        ;;
    "send")
        echo '{"version":"1","status":"success","data":{"id":"em_stub_tx_1"}}'
        exit 0
        ;;
```

- [ ] **Step 3: Add unit test for batch_send wrapper via stub**

Skip — stub-based tests are integration-level. The pipeline integration test (Task 8) exercises this path.

- [ ] **Step 4: Commit**

```bash
cargo build
cargo clippy --all-targets -- -D warnings
git add src/email_cli.rs tests/fixtures/stub-email-cli.sh
git commit -m "feat(email_cli): batch_send and send wrappers; stub batch/send mocks"
```

---

## Task 6: Send pipeline — pre-flight, suppression filter, per-recipient render, dispatch

**Files:**
- Modify: `src/broadcast/pipeline.rs`
- Modify: `src/config.rs` (if needed for `Sender::physical_address` access)

- [ ] **Step 1: Implement the full pipeline**

Replace `src/broadcast/pipeline.rs` with:

```rust
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
use crate::segment::{SegmentExpr, compiler, parser};
use crate::template::{compile_with_placeholders, lint};
use serde_json::{Value, json};

#[derive(Debug, thiserror::Error)]
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

pub fn send_broadcast(id: i64) -> Result<PipelineResult, AppError> {
    let config = Config::load()?;
    let db = Db::open()?;
    let cli = EmailCli::new(&config.email_cli.path, &config.email_cli.profile);

    // 1. Load broadcast + template
    let broadcast = db
        .broadcast_get(id)?
        .ok_or_else(|| AppError::BadInput {
            code: "broadcast_not_found".into(),
            message: format!("no broadcast with id {id}"),
            suggestion: "Run `mailing-list-cli broadcast ls` to see existing broadcasts".into(),
        })?;
    if !matches!(broadcast.status.as_str(), "draft" | "scheduled" | "sending") {
        return Err(AppError::BadInput {
            code: "broadcast_bad_status".into(),
            message: format!("broadcast '{}' is in status '{}' — cannot send", broadcast.name, broadcast.status),
            suggestion: "Only draft, scheduled, or sending broadcasts can be sent".into(),
        });
    }
    let template = db
        .template_all()?
        .into_iter()
        .find(|t| t.id == broadcast.template_id)
        .ok_or_else(|| AppError::BadInput {
            code: "template_not_found".into(),
            message: format!("template id {} not found (was it deleted?)", broadcast.template_id),
            suggestion: "Re-create the broadcast with a valid template".into(),
        })?;

    // 2. Resolve target → contact IDs
    let recipients = resolve_target(&db, &broadcast)?;

    // 3. Pre-flight invariants
    preflight_checks(&config, &template.mjml_source, recipients.len())?;

    // Mark sending
    db.broadcast_set_status(id, "sending", None)?;

    // 4. Suppression filter + insert pending broadcast_recipient rows
    let mut to_send: Vec<Contact> = Vec::with_capacity(recipients.len());
    let mut suppressed_count = 0;
    for recipient in recipients {
        if db.is_email_suppressed(&recipient.email)? {
            db.broadcast_recipient_insert(id, recipient.id, "suppressed")?;
            suppressed_count += 1;
            continue;
        }
        db.broadcast_recipient_insert(id, recipient.id, "pending")?;
        to_send.push(recipient);
    }
    db.broadcast_update_counts(id, to_send.len() as i64)?;

    // 5-7. Per-recipient render (done inside the chunk loop below)
    // 8. Write JSON batch file + shell out in chunks of 100
    let unsubscribe_secret =
        std::env::var(&config.unsubscribe.secret_env).unwrap_or_else(|_| "mlc-unsubscribe-dev".to_string());
    let now_epoch = chrono::Utc::now().timestamp();
    let from = format!(
        "{}{}",
        config.sender.from,
        "" // reply_to is a separate header
    );
    let public_url = config.unsubscribe.public_url.clone();

    let mut sent_count = 0;
    let mut failed_count = 0;
    let cache_dir = crate::paths::cache_dir().join("batch-files");

    for (chunk_idx, chunk) in to_send.chunks(CHUNK_SIZE).enumerate() {
        let mut entries = Vec::with_capacity(chunk.len());
        for contact in chunk {
            let token = sign_token(
                unsubscribe_secret.as_bytes(),
                contact.id,
                id,
                now_epoch,
            )
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
            let footer_html = format!(
                "<div style=\"color:#666;font-size:11px;text-align:center;margin-top:20px\">{}</div>",
                html_escape(config.sender.physical_address.trim())
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
            let rendered = compile_with_placeholders(&template.mjml_source, &merge_data)?;
            entries.push(BatchEntry {
                from: config.sender.from.clone(),
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

        match cli.batch_send(&batch_path) {
            Ok(results) => {
                for (email, resend_id) in results {
                    if let Some(contact) = chunk.iter().find(|c| c.email.eq_ignore_ascii_case(&email)) {
                        db.broadcast_recipient_mark_sent(id, contact.id, &resend_id)?;
                        sent_count += 1;
                    }
                }
            }
            Err(e) => {
                failed_count += chunk.len();
                eprintln!("chunk {chunk_idx} failed: {}", e.message());
            }
        }
    }

    // 10. Mark broadcast.status = 'sent' (or 'failed' if everything failed)
    let final_status = if failed_count == to_send.len() && to_send.len() > 0 {
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

pub fn preview_broadcast(id: i64, to: &str) -> Result<PipelineResult, AppError> {
    let config = Config::load()?;
    let db = Db::open()?;
    let cli = EmailCli::new(&config.email_cli.path, &config.email_cli.profile);

    let broadcast = db
        .broadcast_get(id)?
        .ok_or_else(|| AppError::BadInput {
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

    let unsubscribe_secret = std::env::var(&config.unsubscribe.secret_env)
        .unwrap_or_else(|_| "mlc-unsubscribe-dev".to_string());
    let now_epoch = chrono::Utc::now().timestamp();
    let preview_token = sign_token(unsubscribe_secret.as_bytes(), 0, id, now_epoch)
        .map_err(|e| AppError::Transient {
            code: "token_sign_failed".into(),
            message: format!("HMAC sign failed: {e}"),
            suggestion: "Set MLC_UNSUBSCRIBE_SECRET".into(),
        })?;
    let unsubscribe_url = format!("{}/{}", config.unsubscribe.public_url, preview_token);
    let footer_html = format!(
        "<div style=\"color:#666;font-size:11px;text-align:center;margin-top:20px\">{}</div>",
        html_escape(config.sender.physical_address.trim())
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
    let rendered = compile_with_placeholders(&template.mjml_source, &merge_data)?;
    let subject = format!("[PREVIEW] {}", rendered.subject);

    let _resend_id = cli.send(&config.sender.from, to, &subject, &rendered.html, &rendered.text)?;

    Ok(PipelineResult {
        broadcast_id: id,
        sent_count: 1,
        suppressed_count: 0,
        failed_count: 0,
    })
}

fn resolve_target(db: &Db, broadcast: &Broadcast) -> Result<Vec<Contact>, AppError> {
    match broadcast.target_kind.as_str() {
        "list" => {
            // Read all contacts in the list
            let contacts = db.contact_list_in_list(broadcast.target_id, 100_000)?;
            Ok(contacts)
        }
        "segment" => {
            let segment = db
                .segment_get_by_name_or_id(broadcast.target_id)?
                .ok_or_else(|| AppError::BadInput {
                    code: "segment_not_found".into(),
                    message: format!("segment id {} not found", broadcast.target_id),
                    suggestion: "Run `mailing-list-cli segment ls`".into(),
                })?;
            let expr: SegmentExpr = serde_json::from_str(&segment.filter_json)
                .map_err(|e| AppError::Transient {
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

fn preflight_checks(config: &Config, template_source: &str, recipient_count: usize) -> Result<(), AppError> {
    // Invariant 1: physical address must be set
    if config.sender.physical_address.trim().is_empty() {
        return Err(AppError::Config {
            code: "physical_address_required".into(),
            message: "[sender].physical_address is empty in config.toml".into(),
            suggestion: "Edit ~/.config/mailing-list-cli/config.toml and add a CAN-SPAM physical address".into(),
        });
    }
    // Invariant 2: template lints clean
    let outcome = lint(template_source);
    if outcome.has_errors() {
        return Err(AppError::BadInput {
            code: "template_has_lint_errors".into(),
            message: format!("template has {} lint errors", outcome.error_count),
            suggestion: "Fix the template via `template edit <name>` or inspect `template lint <name>`".into(),
        });
    }
    // Invariant 3: recipient count cap
    let cap = config.guards.max_recipients_per_send.unwrap_or(50_000);
    if recipient_count > cap {
        return Err(AppError::BadInput {
            code: "recipient_count_exceeds_cap".into(),
            message: format!(
                "recipient count {recipient_count} exceeds max_recipients_per_send {cap}"
            ),
            suggestion: "Raise the cap in config.toml [guards] or target a smaller segment".into(),
        });
    }
    // Invariant 4/5: complaint and bounce rate — Phase 6 job. Stubbed pass in Phase 5.
    Ok(())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
```

- [ ] **Step 2: Add required helpers**

`segment_get_by_name_or_id` needs to resolve by id. If it doesn't exist yet, add it to `src/db/mod.rs`:

```rust
    pub fn segment_get_by_name_or_id(&self, id: i64) -> Result<Option<crate::models::Segment>, AppError> {
        let row = self.conn.query_row(
            "SELECT id, name, filter_json, created_at FROM segment WHERE id = ?1",
            params![id],
            |row| {
                Ok(crate::models::Segment {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    filter_json: row.get(2)?,
                    created_at: row.get(3)?,
                    member_count: 0,
                })
            },
        );
        match row {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }
```

Also ensure `config.guards.max_recipients_per_send: Option<usize>` exists on the config. If not, add it (default 50000).

- [ ] **Step 3: Build + test + commit**

```bash
cargo build
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1 2>&1 | grep "test result"
git add src/broadcast/pipeline.rs src/db/mod.rs src/config.rs
git commit -m "feat(broadcast): full send pipeline with pre-flight, suppression, per-recipient render"
```

---

## Task 7: `broadcast create/ls/show/cancel/schedule` CLI

**Files:**
- Modify: `src/cli.rs`
- Create: `src/commands/broadcast.rs`
- Modify: `src/commands/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add `Broadcast` subcommand to `src/cli.rs`**

Add to `enum Command`:

```rust
    /// Manage named, targeted broadcasts (campaigns)
    Broadcast {
        #[command(subcommand)]
        action: BroadcastAction,
    },
```

At the bottom of the file:

```rust
#[derive(Subcommand, Debug)]
pub enum BroadcastAction {
    /// Stage a new broadcast in draft status
    Create(BroadcastCreateArgs),
    /// Send a single test copy via email-cli send
    Preview(BroadcastPreviewArgs),
    /// Move a draft broadcast into scheduled status
    Schedule(BroadcastScheduleArgs),
    /// Send the broadcast now (runs the full pipeline)
    Send(BroadcastSendArgs),
    /// Cancel a draft or scheduled broadcast
    Cancel(BroadcastCancelArgs),
    /// List recent broadcasts
    #[command(visible_alias = "ls")]
    List(BroadcastListArgs),
    /// Show full details for a broadcast
    Show(BroadcastShowArgs),
}

#[derive(Args, Debug)]
pub struct BroadcastCreateArgs {
    /// Broadcast name (agents use this as a memorable identifier)
    #[arg(long)]
    pub name: String,
    /// Template name to send
    #[arg(long)]
    pub template: String,
    /// Target: `list:<name>` or `segment:<name>`
    #[arg(long)]
    pub to: String,
}

#[derive(Args, Debug)]
pub struct BroadcastPreviewArgs {
    pub id: i64,
    #[arg(long)]
    pub to: String,
}

#[derive(Args, Debug)]
pub struct BroadcastScheduleArgs {
    pub id: i64,
    /// RFC 3339 timestamp (e.g. 2026-04-09T12:00:00Z)
    #[arg(long)]
    pub at: String,
}

#[derive(Args, Debug)]
pub struct BroadcastSendArgs {
    pub id: i64,
}

#[derive(Args, Debug)]
pub struct BroadcastCancelArgs {
    pub id: i64,
    #[arg(long)]
    pub confirm: bool,
}

#[derive(Args, Debug)]
pub struct BroadcastListArgs {
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long, default_value = "50")]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct BroadcastShowArgs {
    pub id: i64,
}
```

- [ ] **Step 2: Create the command dispatch module**

Create `src/commands/broadcast.rs`:

```rust
use crate::broadcast::pipeline;
use crate::cli::{
    BroadcastAction, BroadcastCancelArgs, BroadcastCreateArgs, BroadcastListArgs,
    BroadcastPreviewArgs, BroadcastScheduleArgs, BroadcastSendArgs, BroadcastShowArgs,
};
use crate::db::Db;
use crate::error::AppError;
use crate::output::{self, Format};
use serde_json::json;

pub fn run(format: Format, action: BroadcastAction) -> Result<(), AppError> {
    match action {
        BroadcastAction::Create(args) => create(format, args),
        BroadcastAction::Preview(args) => preview(format, args),
        BroadcastAction::Schedule(args) => schedule(format, args),
        BroadcastAction::Send(args) => send(format, args),
        BroadcastAction::Cancel(args) => cancel(format, args),
        BroadcastAction::List(args) => list(format, args),
        BroadcastAction::Show(args) => show(format, args),
    }
}

fn create(format: Format, args: BroadcastCreateArgs) -> Result<(), AppError> {
    let db = Db::open()?;

    // Resolve template
    let template = db
        .template_get_by_name(&args.template)?
        .ok_or_else(|| AppError::BadInput {
            code: "template_not_found".into(),
            message: format!("no template named '{}'", args.template),
            suggestion: "Run `mailing-list-cli template ls`".into(),
        })?;

    // Parse `list:<name>` or `segment:<name>`
    let (kind, name) = args
        .to
        .split_once(':')
        .ok_or_else(|| AppError::BadInput {
            code: "bad_target_syntax".into(),
            message: format!("--to '{}' must be `list:<name>` or `segment:<name>`", args.to),
            suggestion: "Example: --to list:newsletter or --to segment:vips".into(),
        })?;

    let (target_kind, target_id) = match kind {
        "list" => {
            let list = db.list_get_by_name(name)?.ok_or_else(|| AppError::BadInput {
                code: "list_not_found".into(),
                message: format!("no list named '{name}'"),
                suggestion: "Run `mailing-list-cli list ls`".into(),
            })?;
            ("list", list.id)
        }
        "segment" => {
            let segment = db.segment_get_by_name(name)?.ok_or_else(|| AppError::BadInput {
                code: "segment_not_found".into(),
                message: format!("no segment named '{name}'"),
                suggestion: "Run `mailing-list-cli segment ls`".into(),
            })?;
            ("segment", segment.id)
        }
        other => {
            return Err(AppError::BadInput {
                code: "bad_target_kind".into(),
                message: format!("target kind '{other}' not recognized"),
                suggestion: "Use `list:<name>` or `segment:<name>`".into(),
            });
        }
    };

    let id = db.broadcast_create(&args.name, template.id, target_kind, target_id)?;
    let broadcast = db.broadcast_get(id)?.expect("just-created broadcast must exist");
    output::success(
        format,
        &format!("broadcast '{}' created as draft", args.name),
        json!({ "broadcast": broadcast }),
    );
    Ok(())
}

fn preview(format: Format, args: BroadcastPreviewArgs) -> Result<(), AppError> {
    let result = pipeline::preview_broadcast(args.id, &args.to)?;
    output::success(
        format,
        &format!("preview sent to {}", args.to),
        json!({ "broadcast_id": args.id, "to": args.to, "result": result }),
    );
    Ok(())
}

fn schedule(format: Format, args: BroadcastScheduleArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let _ = chrono::DateTime::parse_from_rfc3339(&args.at).map_err(|e| AppError::BadInput {
        code: "bad_scheduled_at".into(),
        message: format!("'{}' is not a valid RFC 3339 timestamp: {e}", args.at),
        suggestion: "Use e.g. 2026-04-09T12:00:00Z".into(),
    })?;
    let b = db.broadcast_get(args.id)?.ok_or_else(|| AppError::BadInput {
        code: "broadcast_not_found".into(),
        message: format!("no broadcast with id {}", args.id),
        suggestion: "Run `mailing-list-cli broadcast ls`".into(),
    })?;
    if b.status != "draft" {
        return Err(AppError::BadInput {
            code: "broadcast_bad_status".into(),
            message: format!("broadcast is in status '{}' — only draft can be scheduled", b.status),
            suggestion: "Re-create or use `broadcast cancel` first".into(),
        });
    }
    db.broadcast_set_scheduled(args.id, &args.at)?;
    output::success(
        format,
        &format!("broadcast {} scheduled for {}", args.id, args.at),
        json!({ "id": args.id, "scheduled_at": args.at }),
    );
    Ok(())
}

fn send(format: Format, args: BroadcastSendArgs) -> Result<(), AppError> {
    let result = pipeline::send_broadcast(args.id)?;
    output::success(
        format,
        &format!("broadcast {} sent", args.id),
        json!({
            "broadcast_id": result.broadcast_id,
            "sent": result.sent_count,
            "suppressed": result.suppressed_count,
            "failed": result.failed_count
        }),
    );
    Ok(())
}

fn cancel(format: Format, args: BroadcastCancelArgs) -> Result<(), AppError> {
    if !args.confirm {
        return Err(AppError::BadInput {
            code: "confirmation_required".into(),
            message: format!("cancelling broadcast {} requires --confirm", args.id),
            suggestion: format!("rerun with `mailing-list-cli broadcast cancel {} --confirm`", args.id),
        });
    }
    let db = Db::open()?;
    let b = db.broadcast_get(args.id)?.ok_or_else(|| AppError::BadInput {
        code: "broadcast_not_found".into(),
        message: format!("no broadcast with id {}", args.id),
        suggestion: "Run `mailing-list-cli broadcast ls`".into(),
    })?;
    if !matches!(b.status.as_str(), "draft" | "scheduled") {
        return Err(AppError::BadInput {
            code: "broadcast_bad_status".into(),
            message: format!("broadcast is in status '{}' — only draft/scheduled can be cancelled", b.status),
            suggestion: "Already-sent broadcasts cannot be cancelled".into(),
        });
    }
    db.broadcast_set_status(args.id, "cancelled", None)?;
    output::success(
        format,
        &format!("broadcast {} cancelled", args.id),
        json!({ "id": args.id, "status": "cancelled" }),
    );
    Ok(())
}

fn list(format: Format, args: BroadcastListArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let broadcasts = db.broadcast_all(args.status.as_deref(), args.limit)?;
    let count = broadcasts.len();
    output::success(
        format,
        &format!("{count} broadcast(s)"),
        json!({ "broadcasts": broadcasts, "count": count }),
    );
    Ok(())
}

fn show(format: Format, args: BroadcastShowArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let broadcast = db.broadcast_get(args.id)?.ok_or_else(|| AppError::BadInput {
        code: "broadcast_not_found".into(),
        message: format!("no broadcast with id {}", args.id),
        suggestion: "Run `mailing-list-cli broadcast ls`".into(),
    })?;
    output::success(
        format,
        &format!("broadcast: {}", broadcast.name),
        json!({ "broadcast": broadcast }),
    );
    Ok(())
}
```

- [ ] **Step 2b: Wire the module**

Edit `src/commands/mod.rs`:

```rust
pub mod broadcast;
```

(Add alphabetically at the top.)

Edit `src/main.rs` to add the dispatch:

```rust
        Command::Broadcast { action } => commands::broadcast::run(format, action),
```

- [ ] **Step 3: Commit**

```bash
cargo build
cargo clippy --all-targets -- -D warnings
git add src/cli.rs src/commands/broadcast.rs src/commands/mod.rs src/main.rs
git commit -m "feat(broadcast): create/preview/schedule/send/cancel/ls/show CLI"
```

---

## Task 8: Integration tests for broadcast commands

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add integration tests**

Append to `tests/cli.rs`:

```rust
const SIMPLE_TEMPLATE: &str = r#"---
name: simple_ad
subject: "Hi {{ first_name }}"
variables:
  - name: first_name
    type: string
    required: true
---
<mjml>
  <mj-head>
    <mj-title>Hi</mj-title>
    <mj-preview>Hello there</mj-preview>
  </mj-head>
  <mj-body>
    <mj-section>
      <mj-column>
        <mj-text>Hi {{ first_name }}</mj-text>
        <mj-button href="https://example.com/cta">Click me</mj-button>
        {{{ unsubscribe_link }}}
        {{{ physical_address_footer }}}
      </mj-column>
    </mj-section>
  </mj-body>
</mjml>
"#;

fn seed_broadcast_env() -> (TempDir, PathBuf, PathBuf) {
    let (tmp, config_path, db_path) = stub_env();
    // Create list + contact + template
    let template_path = tmp.path().join("simple.mjml.hbs");
    std::fs::write(&template_path, SIMPLE_TEMPLATE).unwrap();

    for args in [
        vec!["--json", "list", "create", "news"],
        vec!["--json", "contact", "add", "alice@example.com", "--list", "1", "--first-name", "Alice"],
        vec!["--json", "template", "create", "simple_ad", "--from-file", template_path.to_str().unwrap()],
    ] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args(&args);
        cmd.assert().success();
    }
    (tmp, config_path, db_path)
}

#[test]
fn broadcast_create_list_target_and_show() {
    let (_tmp, config_path, db_path) = seed_broadcast_env();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "broadcast", "create", "--name", "Q1 ad",
            "--template", "simple_ad", "--to", "list:news",
        ]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["broadcast"]["name"], "Q1 ad");
    assert_eq!(v["data"]["broadcast"]["status"], "draft");

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "broadcast", "ls"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["count"], 1);
}

#[test]
fn broadcast_send_via_stub_updates_status_to_sent() {
    let (_tmp, config_path, db_path) = seed_broadcast_env();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "broadcast", "create", "--name", "test",
            "--template", "simple_ad", "--to", "list:news",
        ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_UNSUBSCRIBE_SECRET", "test-secret-long-enough")
        .args(["--json", "broadcast", "send", "1"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["sent"], 1);

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "broadcast", "show", "1"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["broadcast"]["status"], "sent");
}

#[test]
fn broadcast_cancel_without_confirm_fails() {
    let (_tmp, config_path, db_path) = seed_broadcast_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "broadcast", "create", "--name", "test",
            "--template", "simple_ad", "--to", "list:news",
        ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "broadcast", "cancel", "1"]);
    cmd.assert().failure().code(3);
}

#[test]
fn broadcast_preview_via_stub_sends_single() {
    let (_tmp, config_path, db_path) = seed_broadcast_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json", "broadcast", "create", "--name", "test",
            "--template", "simple_ad", "--to", "list:news",
        ]);
    cmd.assert().success();

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_UNSUBSCRIBE_SECRET", "test-secret-long-enough")
        .args(["--json", "broadcast", "preview", "1", "--to", "preview@example.com"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["to"], "preview@example.com");
}
```

- [ ] **Step 2: Run + commit**

```bash
cargo test broadcast -- --test-threads=1 2>&1 | tail -20
cargo test -- --test-threads=1 2>&1 | grep "test result"
cargo clippy --all-targets -- -D warnings
cargo fmt --check
git add tests/cli.rs
git commit -m "test(broadcast): integration tests for create/send/preview/cancel"
```

---

## Task 9: Update agent-info + version bump + tag v0.1.1

**Files:**
- Modify: `src/commands/agent_info.rs`
- Modify: `Cargo.toml`
- Modify: `README.md` (badge)

- [ ] **Step 1: Add broadcast commands to agent-info**

Append to the `commands` block in `src/commands/agent_info.rs`:

```rust
            "broadcast create --name <n> --template <tpl> --to <list:name|segment:name>": "Stage a named broadcast in draft status",
            "broadcast preview <id> --to <email>": "Send a single test copy via email-cli send",
            "broadcast schedule <id> --at <rfc3339>": "Move a draft broadcast to scheduled",
            "broadcast send <id>": "Run the full send pipeline and dispatch via email-cli batch send",
            "broadcast cancel <id> --confirm": "Cancel a draft or scheduled broadcast",
            "broadcast ls [--status <s>] [--limit N]": "List recent broadcasts",
            "broadcast show <id>": "Show broadcast details including recipient + stat counts",
```

Update status:

```rust
        "status": "v0.1.1 — broadcasts, send pipeline, HMAC unsubscribe tokens"
```

- [ ] **Step 2: Bump version + README**

```bash
# Cargo.toml: version = "0.1.1"
# README.md: update badge from v0.1.0 to v0.1.1
```

- [ ] **Step 3: Add agent-info test for v0.1.1**

Append to `tests/cli.rs`:

```rust
#[test]
fn agent_info_lists_phase_5_commands() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    let out = cmd.args(["--json", "agent-info"]).assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    let commands = v["commands"].as_object().unwrap();
    for key in [
        "broadcast create --name <n> --template <tpl> --to <list:name|segment:name>",
        "broadcast send <id>",
        "broadcast preview <id> --to <email>",
    ] {
        assert!(commands.contains_key(key), "agent-info missing {key}");
    }
    assert!(v["status"].as_str().unwrap().starts_with("v0.1.1"));
}
```

- [ ] **Step 4: Full verification + tag**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1 2>&1 | grep "test result"

git add Cargo.toml Cargo.lock src/commands/agent_info.rs tests/cli.rs README.md
git commit -m "chore: bump to v0.1.1 — phase 5 broadcasts"
git push origin main
git tag -a v0.1.1 -m "v0.1.1 — broadcasts, send pipeline, HMAC unsubscribe tokens"
git push origin v0.1.1
gh run list --repo paperfoot/mailing-list-cli --limit 1
```

---

## Task 10: Real-Resend smoke test (manual, post-tag)

**Goal:** Prove the v0.1.1 release actually sends real mail via the user's Resend account. This is a manual verification step, not a committed test — it touches the user's real Resend infrastructure.

**Prerequisite:** user's `email-cli profile test local` passes. `paperfoot.com` is a verified sending domain.

- [ ] **Step 1: Build release and set up test config**

```bash
cd /Users/biobook/Projects/mailing-list-cli
cargo build --release 2>&1 | tail -5
mkdir -p /tmp/mlc-smoke-v0.1.1
cat > /tmp/mlc-smoke-v0.1.1/config.toml <<'EOF'
[sender]
from = "test@paperfoot.com"
physical_address = """
Paperfoot AI (SG) Pte. Ltd.
123 Example Street, #01-23
Singapore 123456
"""

[email_cli]
path = "email-cli"
profile = "local"

[unsubscribe]
public_url = "https://hooks.paperfoot.com/u"
secret_env = "MLC_UNSUBSCRIBE_SECRET"

[guards]
max_recipients_per_send = 10

[webhook]
port = 8081
secret_env = "RESEND_WEBHOOK_SECRET"
public_url = "https://hooks.paperfoot.com/webhook"
EOF

export MLC_CONFIG_PATH=/tmp/mlc-smoke-v0.1.1/config.toml
export MLC_DB_PATH=/tmp/mlc-smoke-v0.1.1/state.db
export MLC_UNSUBSCRIBE_SECRET=smoke-secret-at-least-16-bytes
MLC=./target/release/mailing-list-cli
```

- [ ] **Step 2: Create list, add contact (test recipient), template, broadcast, send**

```bash
# 1. Create list
$MLC --json list create smoke-list --description "v0.1.1 smoke test"

# 2. Add test recipient
$MLC --json contact add smoke-v0.1.1-test@paperfoot.com --list 1 --first-name Smoke --last-name Test

# 3. Create template from file
cat > /tmp/mlc-smoke-v0.1.1/template.mjml.hbs <<'EOF'
---
name: smoke_template
subject: "v0.1.1 smoke test for {{ first_name }}"
variables:
  - name: first_name
    type: string
    required: true
---
<mjml>
  <mj-head>
    <mj-title>mailing-list-cli v0.1.1 smoke test</mj-title>
    <mj-preview>This is an automated smoke test email</mj-preview>
  </mj-head>
  <mj-body>
    <mj-section>
      <mj-column>
        <mj-text font-size="20px">Hi {{ first_name }},</mj-text>
        <mj-text>This is an automated smoke test from mailing-list-cli v0.1.1.</mj-text>
        <mj-button href="https://github.com/paperfoot/mailing-list-cli">View the repo</mj-button>
        <mj-text font-size="11px" color="#666">
          {{{ unsubscribe_link }}}<br/>
          {{{ physical_address_footer }}}
        </mj-text>
      </mj-column>
    </mj-section>
  </mj-body>
</mjml>
EOF
$MLC --json template create smoke_template --from-file /tmp/mlc-smoke-v0.1.1/template.mjml.hbs

# 4. Lint it
$MLC --json template lint smoke_template

# 5. Preview (real send via email-cli send)
$MLC --json broadcast create --name "smoke-v0.1.1" --template smoke_template --to list:smoke-list
$MLC --json broadcast preview 1 --to smoke-v0.1.1-test@paperfoot.com

# 6. Full send (real)
$MLC --json broadcast send 1
```

- [ ] **Step 3: Verify receipt**

```bash
# Wait ~10s for delivery
sleep 15
# Poll the inbox for the test recipient
email-cli --json inbox ls --account local --to smoke-v0.1.1-test@paperfoot.com --limit 5
```

Expected: at least 2 messages arrived (preview + real broadcast). Both contain "v0.1.1 smoke test" in the subject and "This is an automated smoke test" in the body.

- [ ] **Step 4: Report results**

If the smoke test passes: v0.1.1 ships and Phase 6 can proceed.
If it fails: diagnose (Resend API error, malformed batch, missing env, etc.), fix, and re-run.

**This task is NOT committed.** It's a post-release verification.

---

## What Phase 5 does NOT ship

1. **Native broadcast via `email-cli broadcast create`** — batch send only in v0.1.1. Native is v0.2+.
2. **Per-chunk retry** (`broadcast retry-chunk`) — if a chunk fails, the whole broadcast is marked `failed`.
3. **Scheduled-broadcast daemon** — `broadcast schedule <id>` sets `scheduled_at` but nothing fires. That's Phase 8 (daemon).
4. **Real complaint / bounce rate checks** — pre-flight invariants 4 and 5 stub-pass. Phase 6 enables real checks via the event mirror.
5. **A/B testing** — Phase 8.
6. **Dynamic merge data beyond contact fields** — you get `first_name`, `last_name`, `email`, and declared custom fields. No contextual overrides per broadcast.
7. **Send-time template re-lint failure recovery** — if lint errors appear mid-send (shouldn't happen since we check up front), the broadcast fails hard.

---

## Acceptance criteria

- [ ] All 10 tasks checked off.
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test -- --test-threads=1` all clean.
- [ ] Tests ≥ baseline (87 unit + 48 integration = 135) + ~10 new = ≥ 145.
- [ ] `broadcast create --name X --template Y --to list:Z` creates a draft row.
- [ ] `broadcast send <id>` with stub email-cli returns `sent: 1` and updates status to `sent`.
- [ ] `broadcast preview <id> --to X` calls email-cli send once.
- [ ] `broadcast cancel` requires `--confirm` and refuses non-draft/scheduled broadcasts.
- [ ] `Cargo.toml` is 0.1.1, tag pushed, CI green.
- [ ] **Real-Resend smoke test (Task 10) documented**, either run by the executor or left for manual verification.

---

*End of Phase 5 implementation plan.*
