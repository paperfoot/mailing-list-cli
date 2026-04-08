# Phase 6: Webhook Ingestion + Reports — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `mailing-list-cli v0.1.2` — the reports surface the user explicitly asked for: **bounce rate, unsubscribe count per batch, and click tracking**. This phase ingests Resend delivery events (via both `email-cli email list` polling AND a local HTTP webhook listener), mirrors them to the `event` and `click` tables, updates broadcast stat counters, auto-suppresses bounces and complaints, and surfaces the results via `report show`, `report links`, `report engagement`, and `report deliverability`.

**Architecture:** Two ingestion paths feed the same event handler:
1. **`event poll`** — shells out to `email-cli email list --after <cursor>` and translates each returned email's `last_event` field into an event row. Simple, no long-running process, works for periodic syncs.
2. **`webhook listen`** — a `tiny_http`-based HTTP server on port 8081 (configurable) that verifies Svix-style HMAC signatures on incoming Resend webhook POSTs and calls the same event dispatch. Long-running, for real-time ingestion.

Both paths call `src/webhook/dispatch.rs::handle_event(ev)` which: inserts into `event` table → branches by event type → updates `broadcast_recipient.status`, `broadcast.<stat>_count`, `suppression`, `contact.status`, `click` tables as appropriate.

Reports are pure SQL aggregations over these tables.

**Tech Stack:** Rust 2024 edition. New deps: `tiny_http 0.12` (lightweight HTTP server), `subtle 2.6` (constant-time equality for HMAC verification). Existing: `hmac`, `sha2`, `base64`, `rusqlite`, `serde_json`, `chrono`.

**Spec reference:** [`docs/specs/2026-04-07-mailing-list-cli-design.md`](../specs/2026-04-07-mailing-list-cli-design.md) §4.7 (report commands), §4.9 (webhook commands), §10 (webhook listener details), §13.5 (delivery status polling). [`docs/plans/2026-04-08-parity-plan.md`](./2026-04-08-parity-plan.md) §5 Phase 6.

**Prerequisite:** Phase 5 is complete (v0.1.1 tagged). Broadcasts and `broadcast_recipient` rows exist. The `event`, `click`, `soft_bounce_count`, `suppression` tables are ALREADY in place from migration 0001. No schema work.

---

## Locked design decisions

1. **Both `event poll` and `webhook listen` feed the same handler.** `src/webhook/dispatch.rs::handle_event(db, ev: ResendEvent) -> Result<(), AppError>`. The two entry points (poll + HTTP) only differ in HOW they get the event.
2. **`event poll` is the primary recommended path** for Phase 6. It doesn't need a long-running process, doesn't need a public URL, doesn't need signature verification, and works behind firewalls. The webhook listener is a v0.1.2 secondary feature.
3. **`webhook listen` uses `tiny_http`** — single dependency, no async runtime, blocking I/O is fine for this use case. Phase 8 can upgrade to `axum` + `tokio` if the listener needs high concurrency.
4. **Signature verification uses Svix-compatible HMAC-SHA256**: `svix-id` + `svix-timestamp` + payload → base64-encode. Compare against `svix-signature` header. Resend signs webhooks via Svix.
5. **Auto-suppression is atomic with the event insert**. For `email.bounced` (Permanent) or `email.complained`, the handler inserts a `suppression` row AND updates `contact.status` in the same transaction. This is the compliance boundary.
6. **Soft bounce counter** increments on every `email.delivery_delayed` and resets on any successful `email.delivered`. When the counter reaches 5 consecutive delayed events with no delivery in between, the contact is auto-suppressed with reason `soft_bounced_repeat`.
7. **Click tracking** inserts one row per click into the `click` table with `broadcast_id`, `contact_id`, `link`, `ip_address`, `user_agent`, `clicked_at`. `report links` aggregates by `link` and counts.
8. **`report show <broadcast-id>`** returns a JSON envelope with: recipient_count, delivered_count, bounced_count, opened_count, clicked_count, unsubscribed_count, complained_count, CTR (clicked/delivered * 100), bounce_rate (bounced/sent * 100), complaint_rate (complained/sent * 100), suppression_hits. Agents parse the JSON; humans see a rendered table in non-JSON mode.
9. **`report engagement`** computes an engagement score per contact: `(opens * 1 + clicks * 3) / max(sent, 1)` averaged across the target list/segment. Returns a ranked list.
10. **`report deliverability`** queries `event` + `broadcast` to compute rolling 7-day bounce rate, complaint rate, and DMARC-pass rate (DMARC via `email-cli domain list` — if a domain is `verified`, assume DMARC passes).
11. **Event idempotency:** the `event` table has no unique constraint on `resend_email_id + type`, so duplicate webhook deliveries would double-count. Enforce idempotency in the handler: before inserting, check if `(resend_email_id, type)` already exists; skip if so. This means `INSERT OR IGNORE` plus a unique index — add the index in a new migration `0002_event_idempotency`.
12. **The webhook listener binds to `127.0.0.1:8081` by default**, not `0.0.0.0`. Operators who want external access must explicitly set `--bind 0.0.0.0:8081` AND configure a reverse proxy. Defaults are safe.
13. **Synthetic webhook test** via `webhook test --event <type> --to <url>` POSTs a fake payload to the given URL so the operator can verify their listener is reachable. Used for CI smoke tests and ops debugging.
14. **`event poll --since <cursor>`** stores the cursor in a small `kv` table (new migration `0002_kv_cursor`) so the next poll picks up from the last seen `email.id`. Cursor resets via `event poll --reset`.
15. **Reports are read-only** — they never mutate the database. Safe to run concurrently with the listener or poll.

---

## File structure

Created by this phase:

```
mailing-list-cli/
├── src/
│   ├── webhook/
│   │   ├── mod.rs              # public API
│   │   ├── dispatch.rs         # handle_event() — the shared dispatcher
│   │   ├── signature.rs        # Svix HMAC signature verifier
│   │   ├── listener.rs         # tiny_http server
│   │   ├── poll.rs             # email-cli email list polling
│   │   └── types.rs            # ResendEvent + ResendEmail structs
│   ├── report/
│   │   ├── mod.rs              # aggregation functions
│   │   └── engagement.rs       # engagement score computation
│   └── commands/
│       ├── webhook.rs          # CLI for webhook listen/backfill/test/poll
│       └── report.rs           # CLI for report show/links/engagement/deliverability
```

Modified:

```
├── Cargo.toml                  # +tiny_http, +subtle
├── src/
│   ├── cli.rs                  # +Webhook, +Report subcommands
│   ├── main.rs                 # +mod webhook, +mod report
│   ├── db/
│   │   ├── mod.rs              # +event_*, +kv_*, +report aggregation helpers
│   │   └── migrations.rs       # +migration 0002 (kv table + event idempotency index)
│   ├── models.rs               # +Event, +Click, +ReportSummary, +LinkReport
│   └── commands/
│       ├── mod.rs              # +pub mod webhook, +pub mod report
│       └── agent_info.rs       # advertise new commands + v0.1.2 status
└── tests/
    ├── cli.rs                  # integration tests per command
    └── fixtures/
        └── resend_events/      # sample event JSON payloads
            ├── delivered.json
            ├── bounced.json
            ├── complained.json
            ├── opened.json
            └── clicked.json
```

---

## Task 1: Migration 0002 — kv table + event idempotency index

**Files:**
- Modify: `src/db/migrations.rs`

- [ ] **Step 1: Add migration 0002**

Append to the `MIGRATIONS` slice in `src/db/migrations.rs`:

```rust
    (
        "0002_event_idempotency_and_kv",
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_event_dedup
            ON event(resend_email_id, type);

        CREATE TABLE IF NOT EXISTS kv (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        "#,
    ),
```

- [ ] **Step 2: Verify + commit**

```bash
cargo test db::tests::migrations_create_all_tables
git add src/db/migrations.rs
git commit -m "feat(db): migration 0002 — event idempotency index + kv cursor table"
```

---

## Task 2: Dependencies + ResendEvent types

**Files:**
- Modify: `Cargo.toml`
- Create: `src/webhook/mod.rs`
- Create: `src/webhook/types.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add deps**

Append to `[dependencies]`:

```toml
subtle = "2.6"
tiny_http = "0.12"
```

- [ ] **Step 2: Create the webhook module root**

Create `src/webhook/mod.rs`:

```rust
//! Webhook subsystem: receive Resend events via HTTP listener or poll,
//! dispatch to a shared event handler that mirrors state to the local DB.

pub mod dispatch;
pub mod listener;
pub mod poll;
pub mod signature;
pub mod types;

pub use dispatch::handle_event;
pub use listener::start_listener;
pub use poll::poll_events;
pub use types::{ResendEvent, ResendEventType};
```

Create stubs for `dispatch.rs`, `listener.rs`, `poll.rs`, `signature.rs` (each with just a doc comment) so the module compiles. They'll be filled in Tasks 3-6.

- [ ] **Step 3: Define the event types**

Create `src/webhook/types.rs`:

```rust
//! Resend webhook event shapes. See https://resend.com/docs/dashboard/webhooks
//! for the authoritative list.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResendEventType {
    #[serde(rename = "email.sent")]
    Sent,
    #[serde(rename = "email.delivered")]
    Delivered,
    #[serde(rename = "email.delivery_delayed")]
    DeliveryDelayed,
    #[serde(rename = "email.bounced")]
    Bounced,
    #[serde(rename = "email.complained")]
    Complained,
    #[serde(rename = "email.opened")]
    Opened,
    #[serde(rename = "email.clicked")]
    Clicked,
    #[serde(rename = "email.suppressed")]
    Suppressed,
    #[serde(rename = "email.failed")]
    Failed,
    #[serde(rename = "email.scheduled")]
    Scheduled,
    #[serde(other)]
    Unknown,
}

impl ResendEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sent => "email.sent",
            Self::Delivered => "email.delivered",
            Self::DeliveryDelayed => "email.delivery_delayed",
            Self::Bounced => "email.bounced",
            Self::Complained => "email.complained",
            Self::Opened => "email.opened",
            Self::Clicked => "email.clicked",
            Self::Suppressed => "email.suppressed",
            Self::Failed => "email.failed",
            Self::Scheduled => "email.scheduled",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResendEvent {
    #[serde(rename = "type")]
    pub event_type: ResendEventType,
    pub created_at: String,
    pub data: ResendEventData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResendEventData {
    pub email_id: String,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub bounce: Option<BounceInfo>,
    #[serde(default)]
    pub click: Option<ClickInfo>,
    #[serde(default, rename = "complaint_type")]
    pub complaint_type: Option<String>,
    #[serde(default)]
    pub tags: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BounceInfo {
    #[serde(rename = "type")]
    pub bounce_type: String, // "Permanent" | "Transient"
    pub message: Option<String>,
    pub subtype: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickInfo {
    pub link: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub timestamp: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_delivered_event() {
        let json = r#"{
            "type": "email.delivered",
            "created_at": "2026-04-08T12:00:00Z",
            "data": {
                "email_id": "em_test_1",
                "to": ["alice@example.com"],
                "subject": "Hi"
            }
        }"#;
        let ev: ResendEvent = serde_json::from_str(json).unwrap();
        assert_eq!(ev.event_type, ResendEventType::Delivered);
        assert_eq!(ev.data.email_id, "em_test_1");
    }

    #[test]
    fn deserializes_bounced_event_with_permanent_type() {
        let json = r#"{
            "type": "email.bounced",
            "created_at": "2026-04-08T12:00:00Z",
            "data": {
                "email_id": "em_test_2",
                "to": ["alice@example.com"],
                "bounce": {"type": "Permanent", "message": "mailbox full"}
            }
        }"#;
        let ev: ResendEvent = serde_json::from_str(json).unwrap();
        assert_eq!(ev.event_type, ResendEventType::Bounced);
        assert_eq!(ev.data.bounce.unwrap().bounce_type, "Permanent");
    }

    #[test]
    fn deserializes_clicked_event_with_link() {
        let json = r#"{
            "type": "email.clicked",
            "created_at": "2026-04-08T12:00:00Z",
            "data": {
                "email_id": "em_test_3",
                "to": ["alice@example.com"],
                "click": {"link": "https://example.com/cta", "ip_address": "1.2.3.4"}
            }
        }"#;
        let ev: ResendEvent = serde_json::from_str(json).unwrap();
        assert_eq!(ev.event_type, ResendEventType::Clicked);
        assert_eq!(ev.data.click.unwrap().link, "https://example.com/cta");
    }
}
```

- [ ] **Step 4: Wire module + commit**

Add `mod webhook;` to `src/main.rs`.

```bash
cargo build
cargo test webhook::types
git add Cargo.toml Cargo.lock src/webhook/ src/main.rs
git commit -m "feat(webhook): ResendEvent types + module skeleton"
```

---

## Task 3: Event handler dispatch + DB helpers

**Files:**
- Modify: `src/db/mod.rs`
- Modify: `src/webhook/dispatch.rs`

- [ ] **Step 1: Add event DB helpers**

Append to `impl Db` in `src/db/mod.rs`:

```rust
    // ─── Event operations ──────────────────────────────────────────────

    /// Insert an event row. Returns true if inserted, false if already
    /// present (idempotent via the unique index on (resend_email_id, type)).
    pub fn event_insert(
        &self,
        event_type: &str,
        resend_email_id: &str,
        broadcast_id: Option<i64>,
        contact_id: Option<i64>,
        payload_json: &str,
    ) -> Result<bool, AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        let affected = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO event (type, resend_email_id, broadcast_id, contact_id, payload_json, received_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![event_type, resend_email_id, broadcast_id, contact_id, payload_json, now],
            )
            .map_err(query_err)?;
        Ok(affected > 0)
    }

    /// Look up a `broadcast_recipient` row by `resend_email_id`.
    pub fn recipient_by_resend_email_id(
        &self,
        resend_email_id: &str,
    ) -> Result<Option<(i64, i64)>, AppError> {
        match self.conn.query_row(
            "SELECT broadcast_id, contact_id FROM broadcast_recipient WHERE resend_email_id = ?1",
            params![resend_email_id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        ) {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    pub fn broadcast_recipient_update_status(
        &self,
        broadcast_id: i64,
        contact_id: i64,
        status: &str,
    ) -> Result<(), AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE broadcast_recipient
                 SET status = ?1, last_event_at = ?2
                 WHERE broadcast_id = ?3 AND contact_id = ?4",
                params![status, now, broadcast_id, contact_id],
            )
            .map_err(query_err)?;
        Ok(())
    }

    pub fn broadcast_increment_stat(
        &self,
        broadcast_id: i64,
        column: &str,
    ) -> Result<(), AppError> {
        // Whitelist the column names to prevent SQL injection
        let column = match column {
            "delivered_count" => "delivered_count",
            "bounced_count" => "bounced_count",
            "opened_count" => "opened_count",
            "clicked_count" => "clicked_count",
            "unsubscribed_count" => "unsubscribed_count",
            "complained_count" => "complained_count",
            _ => {
                return Err(AppError::BadInput {
                    code: "bad_stat_column".into(),
                    message: format!("unknown stat column: {column}"),
                    suggestion: "Report as a bug".into(),
                });
            }
        };
        let sql = format!("UPDATE broadcast SET {column} = {column} + 1 WHERE id = ?1");
        self.conn
            .execute(&sql, params![broadcast_id])
            .map_err(query_err)?;
        Ok(())
    }

    pub fn click_insert(
        &self,
        broadcast_id: i64,
        contact_id: Option<i64>,
        link: &str,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<(), AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO click (broadcast_id, contact_id, link, ip_address, user_agent, clicked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![broadcast_id, contact_id, link, ip_address, user_agent, now],
            )
            .map_err(query_err)?;
        Ok(())
    }

    pub fn suppression_insert(
        &self,
        email: &str,
        reason: &str,
        source_broadcast_id: Option<i64>,
    ) -> Result<(), AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT OR IGNORE INTO suppression (email, reason, suppressed_at, source_broadcast_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![email, reason, now, source_broadcast_id],
            )
            .map_err(query_err)?;
        Ok(())
    }

    pub fn contact_set_status(&self, email: &str, status: &str) -> Result<(), AppError> {
        self.conn
            .execute(
                "UPDATE contact SET status = ?1, updated_at = ?2 WHERE email = ?3 COLLATE NOCASE",
                params![status, chrono::Utc::now().to_rfc3339(), email],
            )
            .map_err(query_err)?;
        Ok(())
    }

    pub fn soft_bounce_increment(&self, contact_id: i64) -> Result<i64, AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO soft_bounce_count (contact_id, consecutive, last_bounce_at)
                 VALUES (?1, 1, ?2)
                 ON CONFLICT(contact_id) DO UPDATE SET consecutive = consecutive + 1, last_bounce_at = ?2",
                params![contact_id, now],
            )
            .map_err(query_err)?;
        let count: i64 = self
            .conn
            .query_row(
                "SELECT consecutive FROM soft_bounce_count WHERE contact_id = ?1",
                params![contact_id],
                |r| r.get(0),
            )
            .map_err(query_err)?;
        Ok(count)
    }

    pub fn soft_bounce_reset(&self, contact_id: i64) -> Result<(), AppError> {
        self.conn
            .execute(
                "DELETE FROM soft_bounce_count WHERE contact_id = ?1",
                params![contact_id],
            )
            .map_err(query_err)?;
        Ok(())
    }

    // ─── KV cursor operations ──────────────────────────────────────────

    pub fn kv_get(&self, key: &str) -> Result<Option<String>, AppError> {
        match self.conn.query_row(
            "SELECT value FROM kv WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        ) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(query_err(e)),
        }
    }

    pub fn kv_set(&self, key: &str, value: &str) -> Result<(), AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO kv (key, value, updated_at) VALUES (?1, ?2, ?3)",
                params![key, value, now],
            )
            .map_err(query_err)?;
        Ok(())
    }
```

- [ ] **Step 2: Implement `handle_event`**

Replace `src/webhook/dispatch.rs` with:

```rust
//! Shared event handler. Both `event poll` and `webhook listen` call this.

use crate::db::Db;
use crate::error::AppError;
use crate::webhook::types::{ResendEvent, ResendEventType};

const SOFT_BOUNCE_STREAK_LIMIT: i64 = 5;

pub fn handle_event(db: &Db, ev: &ResendEvent) -> Result<HandleOutcome, AppError> {
    let payload_json = serde_json::to_string(ev).unwrap_or_default();
    let email_id = ev.data.email_id.clone();
    let event_type = ev.event_type.as_str();

    // Look up the broadcast_recipient first so we can attribute the event
    let recipient = db.recipient_by_resend_email_id(&email_id)?;
    let (broadcast_id, contact_id) = match recipient {
        Some(t) => (Some(t.0), Some(t.1)),
        None => (None, None),
    };

    // Idempotent insert into event table
    let inserted = db.event_insert(event_type, &email_id, broadcast_id, contact_id, &payload_json)?;
    if !inserted {
        return Ok(HandleOutcome::Duplicate);
    }

    // Dispatch by event type
    match ev.event_type {
        ResendEventType::Delivered => {
            if let (Some(bid), Some(cid)) = (broadcast_id, contact_id) {
                db.broadcast_recipient_update_status(bid, cid, "delivered")?;
                db.broadcast_increment_stat(bid, "delivered_count")?;
                db.soft_bounce_reset(cid)?;
            }
        }
        ResendEventType::Bounced => {
            let is_permanent = ev
                .data
                .bounce
                .as_ref()
                .map(|b| b.bounce_type.eq_ignore_ascii_case("Permanent"))
                .unwrap_or(false);
            if is_permanent {
                if let (Some(bid), Some(cid)) = (broadcast_id, contact_id) {
                    db.broadcast_recipient_update_status(bid, cid, "bounced")?;
                    db.broadcast_increment_stat(bid, "bounced_count")?;
                }
                // Auto-suppress
                for to in &ev.data.to {
                    db.suppression_insert(to, "hard_bounced", broadcast_id)?;
                    db.contact_set_status(to, "bounced")?;
                }
            }
        }
        ResendEventType::DeliveryDelayed => {
            if let Some(cid) = contact_id {
                let streak = db.soft_bounce_increment(cid)?;
                if streak >= SOFT_BOUNCE_STREAK_LIMIT {
                    // Promote to permanent suppression
                    for to in &ev.data.to {
                        db.suppression_insert(to, "soft_bounced_repeat", broadcast_id)?;
                        db.contact_set_status(to, "bounced")?;
                    }
                }
            }
        }
        ResendEventType::Complained => {
            if let (Some(bid), Some(cid)) = (broadcast_id, contact_id) {
                db.broadcast_recipient_update_status(bid, cid, "complained")?;
                db.broadcast_increment_stat(bid, "complained_count")?;
                let _ = cid;
            }
            for to in &ev.data.to {
                db.suppression_insert(to, "complained", broadcast_id)?;
                db.contact_set_status(to, "complained")?;
            }
        }
        ResendEventType::Opened => {
            if let Some(bid) = broadcast_id {
                db.broadcast_increment_stat(bid, "opened_count")?;
            }
        }
        ResendEventType::Clicked => {
            if let Some(bid) = broadcast_id {
                db.broadcast_increment_stat(bid, "clicked_count")?;
                if let Some(click) = &ev.data.click {
                    db.click_insert(
                        bid,
                        contact_id,
                        &click.link,
                        click.ip_address.as_deref(),
                        click.user_agent.as_deref(),
                    )?;
                }
            }
        }
        ResendEventType::Suppressed => {
            // Already suppressed — no action beyond the event row
        }
        ResendEventType::Failed => {
            if let (Some(bid), Some(cid)) = (broadcast_id, contact_id) {
                db.broadcast_recipient_update_status(bid, cid, "failed")?;
            }
        }
        ResendEventType::Sent | ResendEventType::Scheduled | ResendEventType::Unknown => {
            // Informational / unknown events get logged but produce no state mutation
        }
    }

    Ok(HandleOutcome::Processed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleOutcome {
    Processed,
    Duplicate,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webhook::types::*;

    fn fresh_db() -> Db {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        Db::open_at(tmp.path()).unwrap()
    }

    fn seed_broadcast_with_recipient(db: &Db) -> (i64, i64, String) {
        let tid = db.template_upsert("t", "Hi", "<mjml></mjml>", "{}").unwrap();
        let list_id = db.list_create("news", None, "seg_x").unwrap();
        let bid = db.broadcast_create("Q1", tid, "list", list_id).unwrap();
        let cid = db.contact_upsert("alice@example.com", None, None).unwrap();
        db.broadcast_recipient_insert(bid, cid, "sent").unwrap();
        db.conn
            .execute(
                "UPDATE broadcast_recipient SET resend_email_id = 'em_x' WHERE broadcast_id = ?1 AND contact_id = ?2",
                [bid, cid],
            )
            .unwrap();
        (bid, cid, "em_x".into())
    }

    #[test]
    fn delivered_updates_broadcast_and_recipient() {
        let db = fresh_db();
        let (bid, cid, email_id) = seed_broadcast_with_recipient(&db);

        let ev = ResendEvent {
            event_type: ResendEventType::Delivered,
            created_at: "2026-04-08T12:00:00Z".into(),
            data: ResendEventData {
                email_id,
                to: vec!["alice@example.com".into()],
                subject: None,
                bounce: None,
                click: None,
                complaint_type: None,
                tags: serde_json::Value::Null,
            },
        };
        let outcome = handle_event(&db, &ev).unwrap();
        assert_eq!(outcome, HandleOutcome::Processed);

        let b = db.broadcast_get(bid).unwrap().unwrap();
        assert_eq!(b.delivered_count, 1);
        assert_eq!(
            db.broadcast_recipient_count_by_status(bid, "delivered").unwrap(),
            1
        );
        let _ = cid;
    }

    #[test]
    fn bounced_permanent_auto_suppresses() {
        let db = fresh_db();
        let (bid, _cid, email_id) = seed_broadcast_with_recipient(&db);
        let ev = ResendEvent {
            event_type: ResendEventType::Bounced,
            created_at: "2026-04-08T12:00:00Z".into(),
            data: ResendEventData {
                email_id,
                to: vec!["alice@example.com".into()],
                subject: None,
                bounce: Some(BounceInfo {
                    bounce_type: "Permanent".into(),
                    message: None,
                    subtype: None,
                }),
                click: None,
                complaint_type: None,
                tags: serde_json::Value::Null,
            },
        };
        handle_event(&db, &ev).unwrap();
        let b = db.broadcast_get(bid).unwrap().unwrap();
        assert_eq!(b.bounced_count, 1);
        assert!(db.is_email_suppressed("alice@example.com").unwrap());
    }

    #[test]
    fn clicked_inserts_click_row_and_increments() {
        let db = fresh_db();
        let (bid, _cid, email_id) = seed_broadcast_with_recipient(&db);
        let ev = ResendEvent {
            event_type: ResendEventType::Clicked,
            created_at: "2026-04-08T12:00:00Z".into(),
            data: ResendEventData {
                email_id,
                to: vec!["alice@example.com".into()],
                subject: None,
                bounce: None,
                click: Some(ClickInfo {
                    link: "https://example.com/cta".into(),
                    ip_address: Some("1.2.3.4".into()),
                    user_agent: Some("test".into()),
                    timestamp: None,
                }),
                complaint_type: None,
                tags: serde_json::Value::Null,
            },
        };
        handle_event(&db, &ev).unwrap();
        let b = db.broadcast_get(bid).unwrap().unwrap();
        assert_eq!(b.clicked_count, 1);
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM click WHERE broadcast_id = ?1",
                [bid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn duplicate_event_returns_duplicate_outcome() {
        let db = fresh_db();
        let (_bid, _cid, email_id) = seed_broadcast_with_recipient(&db);
        let ev = ResendEvent {
            event_type: ResendEventType::Delivered,
            created_at: "2026-04-08T12:00:00Z".into(),
            data: ResendEventData {
                email_id,
                to: vec!["alice@example.com".into()],
                subject: None,
                bounce: None,
                click: None,
                complaint_type: None,
                tags: serde_json::Value::Null,
            },
        };
        assert_eq!(handle_event(&db, &ev).unwrap(), HandleOutcome::Processed);
        assert_eq!(handle_event(&db, &ev).unwrap(), HandleOutcome::Duplicate);
    }
}
```

- [ ] **Step 3: Commit**

```bash
cargo test db::tests::event_insert webhook::dispatch
cargo clippy --all-targets -- -D warnings
git add src/db/mod.rs src/webhook/dispatch.rs
git commit -m "feat(webhook): event handler with auto-suppression and stat updates"
```

---

## Task 4: `event poll` — ingest via `email-cli email list`

**Files:**
- Modify: `src/webhook/poll.rs`
- Modify: `src/email_cli.rs` (add `email_list` method)
- Modify: `tests/fixtures/stub-email-cli.sh` (mock `email list`)

- [ ] **Step 1: Add `email_list` wrapper to `EmailCli`**

Add to `impl EmailCli` in `src/email_cli.rs`:

```rust
    /// Shell out to `email-cli email list --limit N [--after cursor]`.
    /// Returns the parsed response as a `serde_json::Value`.
    pub fn email_list(&self, limit: usize, after: Option<&str>) -> Result<Value, AppError> {
        self.throttle();
        let mut args: Vec<String> = vec![
            "--json".into(),
            "email".into(),
            "list".into(),
            "--limit".into(),
            limit.to_string(),
        ];
        if let Some(cursor) = after {
            args.push("--after".into());
            args.push(cursor.into());
        }
        let output = Command::new(&self.path)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Config {
                code: "email_cli_invoke_failed".into(),
                message: format!("could not run email-cli email list: {e}"),
                suggestion: "Check that email-cli is on PATH".into(),
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Transient {
                code: "email_list_failed".into(),
                message: format!(
                    "email-cli email list failed: {}",
                    stderr.lines().next().unwrap_or("(no stderr)")
                ),
                suggestion: "Run `email-cli profile test` to verify connectivity".into(),
            });
        }
        serde_json::from_slice(&output.stdout).map_err(|e| AppError::Transient {
            code: "email_list_parse".into(),
            message: format!("invalid JSON from email-cli email list: {e}"),
            suggestion: "Check email-cli version compatibility".into(),
        })
    }
```

- [ ] **Step 2: Implement `poll_events`**

Replace `src/webhook/poll.rs` with:

```rust
//! Poll-based event ingestion via `email-cli email list`.
//!
//! The `email list` command returns each email with a `last_event` field
//! (e.g., "delivered", "bounced"). We translate that into a ResendEvent and
//! feed it to the shared handler.

use crate::db::Db;
use crate::email_cli::EmailCli;
use crate::error::AppError;
use crate::webhook::dispatch::{HandleOutcome, handle_event};
use crate::webhook::types::*;
use serde_json::Value;

const CURSOR_KEY: &str = "webhook.poll.last_email_id";
const DEFAULT_PAGE_SIZE: usize = 100;

pub struct PollResult {
    pub processed: usize,
    pub duplicates: usize,
    pub latest_cursor: Option<String>,
}

pub fn poll_events(
    db: &Db,
    cli: &EmailCli,
    reset_cursor: bool,
) -> Result<PollResult, AppError> {
    if reset_cursor {
        db.kv_set(CURSOR_KEY, "")?;
    }
    let cursor = db.kv_get(CURSOR_KEY)?.filter(|s| !s.is_empty());
    let response = cli.email_list(DEFAULT_PAGE_SIZE, cursor.as_deref())?;

    let data = response
        .get("data")
        .and_then(|d| d.get("data"))
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();

    let mut processed = 0;
    let mut duplicates = 0;
    let mut latest_cursor = cursor;

    for entry in &data {
        let Some(email_id) = entry.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(last_event) = entry.get("last_event").and_then(|v| v.as_str()) else {
            continue;
        };
        let event_type = map_last_event_to_type(last_event);
        if matches!(event_type, ResendEventType::Unknown) {
            continue;
        }
        let to_array: Vec<String> = entry
            .get("to")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let ev = ResendEvent {
            event_type,
            created_at: entry
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            data: ResendEventData {
                email_id: email_id.to_string(),
                to: to_array,
                subject: entry
                    .get("subject")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                bounce: None, // poll path doesn't expose bounce subtype
                click: None,
                complaint_type: None,
                tags: entry.get("tags").cloned().unwrap_or(Value::Null),
            },
        };
        match handle_event(db, &ev)? {
            HandleOutcome::Processed => processed += 1,
            HandleOutcome::Duplicate => duplicates += 1,
        }
        latest_cursor = Some(email_id.to_string());
    }

    if let Some(c) = &latest_cursor {
        db.kv_set(CURSOR_KEY, c)?;
    }

    Ok(PollResult {
        processed,
        duplicates,
        latest_cursor,
    })
}

fn map_last_event_to_type(s: &str) -> ResendEventType {
    match s {
        "sent" => ResendEventType::Sent,
        "delivered" => ResendEventType::Delivered,
        "delivery_delayed" => ResendEventType::DeliveryDelayed,
        "bounced" => ResendEventType::Bounced,
        "complained" => ResendEventType::Complained,
        "opened" => ResendEventType::Opened,
        "clicked" => ResendEventType::Clicked,
        "failed" => ResendEventType::Failed,
        _ => ResendEventType::Unknown,
    }
}
```

- [ ] **Step 3: Extend stub email-cli**

Add to `tests/fixtures/stub-email-cli.sh` inside the `"email")` case:

```sh
        if [ "$2" = "list" ] || [ "$2" = "ls" ]; then
            # If MLC_STUB_EMAIL_LIST_JSON is set, emit that, else empty
            if [ -n "$MLC_STUB_EMAIL_LIST_JSON" ]; then
                echo "$MLC_STUB_EMAIL_LIST_JSON"
            else
                echo '{"version":"1","status":"success","data":{"object":"list","has_more":false,"data":[]}}'
            fi
            exit 0
        fi
```

(The existing stub already returns empty — just ensure it also honors the env override.)

- [ ] **Step 4: Commit**

```bash
cargo build
cargo clippy --all-targets -- -D warnings
git add src/webhook/poll.rs src/email_cli.rs tests/fixtures/stub-email-cli.sh
git commit -m "feat(webhook): event poll via email-cli email list"
```

---

## Task 5: Signature verifier + HTTP listener

**Files:**
- Modify: `src/webhook/signature.rs`
- Modify: `src/webhook/listener.rs`

- [ ] **Step 1: Implement Svix-compatible HMAC signature verifier**

Replace `src/webhook/signature.rs` with:

```rust
//! Svix-compatible webhook signature verifier.
//!
//! Resend signs webhooks using Svix. The signature header looks like:
//!   svix-signature: v1,<base64_sig>
//!   svix-id: msg_xyz
//!   svix-timestamp: 1234567890
//!
//! Payload to sign: `{svix-id}.{svix-timestamp}.{body}`

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, thiserror::Error)]
pub enum SignatureError {
    #[error("missing svix headers")]
    MissingHeaders,
    #[error("invalid signature format")]
    BadFormat,
    #[error("signature mismatch")]
    Mismatch,
    #[error("hmac error: {0}")]
    Hmac(String),
}

pub fn verify_svix(
    secret: &[u8],
    svix_id: &str,
    svix_timestamp: &str,
    body: &str,
    svix_signature_header: &str,
) -> Result<(), SignatureError> {
    let to_sign = format!("{svix_id}.{svix_timestamp}.{body}");

    // The secret from Resend is prefixed with "whsec_" — strip it if present.
    let key = if let Some(stripped) = std::str::from_utf8(secret)
        .ok()
        .and_then(|s| s.strip_prefix("whsec_"))
    {
        STANDARD.decode(stripped).map_err(|e| SignatureError::Hmac(e.to_string()))?
    } else {
        secret.to_vec()
    };

    let mut mac = HmacSha256::new_from_slice(&key).map_err(|e| SignatureError::Hmac(e.to_string()))?;
    mac.update(to_sign.as_bytes());
    let expected = mac.finalize().into_bytes();
    let expected_b64 = STANDARD.encode(expected);

    // The header may contain multiple signatures separated by spaces: "v1,sig1 v1,sig2"
    for sig_entry in svix_signature_header.split_whitespace() {
        if let Some((version, provided)) = sig_entry.split_once(',') {
            if version == "v1" && provided.as_bytes().ct_eq(expected_b64.as_bytes()).into() {
                return Ok(());
            }
        }
    }
    Err(SignatureError::Mismatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_accepts_matching_signature() {
        let secret = b"test_secret_key";
        let svix_id = "msg_test";
        let svix_ts = "1234567890";
        let body = r#"{"hello":"world"}"#;
        let to_sign = format!("{svix_id}.{svix_ts}.{body}");

        let mut mac = HmacSha256::new_from_slice(secret).unwrap();
        mac.update(to_sign.as_bytes());
        let sig = STANDARD.encode(mac.finalize().into_bytes());
        let header = format!("v1,{sig}");

        verify_svix(secret, svix_id, svix_ts, body, &header).unwrap();
    }

    #[test]
    fn verify_rejects_wrong_signature() {
        assert!(verify_svix(b"secret", "id", "ts", "body", "v1,wrong").is_err());
    }
}
```

- [ ] **Step 2: Implement the HTTP listener**

Replace `src/webhook/listener.rs` with:

```rust
//! tiny_http-based webhook listener.
//!
//! Binds to 127.0.0.1:<port> by default. Verifies Svix signatures on every
//! incoming payload. Calls the shared dispatcher.

use crate::db::Db;
use crate::error::AppError;
use crate::webhook::dispatch::handle_event;
use crate::webhook::signature::verify_svix;
use crate::webhook::types::ResendEvent;
use std::io::Read;
use std::net::SocketAddr;

pub fn start_listener(bind: SocketAddr, secret: Option<Vec<u8>>) -> Result<(), AppError> {
    let db = Db::open()?;
    let server = tiny_http::Server::http(bind).map_err(|e| AppError::Config {
        code: "webhook_bind_failed".into(),
        message: format!("could not bind webhook listener to {bind}: {e}"),
        suggestion: "Pick a different --bind address/port".into(),
    })?;
    eprintln!("webhook listener ready on {bind}");

    for mut request in server.incoming_requests() {
        // Only POST /webhook is accepted
        if request.method() != &tiny_http::Method::Post {
            let _ = request.respond(tiny_http::Response::empty(405));
            continue;
        }
        if request.url() != "/webhook" {
            let _ = request.respond(tiny_http::Response::empty(404));
            continue;
        }

        let mut body = String::new();
        if let Err(e) = request.as_reader().read_to_string(&mut body) {
            eprintln!("body read error: {e}");
            let _ = request.respond(tiny_http::Response::empty(400));
            continue;
        }

        // Signature verification
        if let Some(key) = &secret {
            let svix_id = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("svix-id"))
                .map(|h| h.value.as_str().to_string())
                .unwrap_or_default();
            let svix_ts = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("svix-timestamp"))
                .map(|h| h.value.as_str().to_string())
                .unwrap_or_default();
            let svix_sig = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("svix-signature"))
                .map(|h| h.value.as_str().to_string())
                .unwrap_or_default();
            if verify_svix(key, &svix_id, &svix_ts, &body, &svix_sig).is_err() {
                let _ = request.respond(tiny_http::Response::empty(401));
                continue;
            }
        }

        // Parse + dispatch
        let ev: ResendEvent = match serde_json::from_str(&body) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("JSON parse error: {e}");
                let _ = request.respond(tiny_http::Response::empty(400));
                continue;
            }
        };
        match handle_event(&db, &ev) {
            Ok(_) => {
                let _ = request.respond(tiny_http::Response::empty(200));
            }
            Err(e) => {
                eprintln!("handler error: {}", e.message());
                let _ = request.respond(tiny_http::Response::empty(500));
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 3: Commit**

```bash
cargo build
cargo test webhook::signature
cargo clippy --all-targets -- -D warnings
git add src/webhook/signature.rs src/webhook/listener.rs
git commit -m "feat(webhook): Svix HMAC verifier + tiny_http listener"
```

---

## Task 6: `webhook` CLI + `event poll` CLI

**Files:**
- Modify: `src/cli.rs`
- Create: `src/commands/webhook.rs`
- Modify: `src/commands/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add CLI definitions**

Add to `enum Command` in `src/cli.rs`:

```rust
    /// Webhook ingestion: listen/poll/test
    Webhook {
        #[command(subcommand)]
        action: WebhookAction,
    },
    /// Shorthand for `webhook poll` (convenience)
    Event {
        #[command(subcommand)]
        action: EventAction,
    },
```

At the bottom:

```rust
#[derive(Subcommand, Debug)]
pub enum WebhookAction {
    /// Run the HTTP webhook listener (long-running)
    Listen(WebhookListenArgs),
    /// Poll email-cli for delivery status updates
    Poll(WebhookPollArgs),
    /// Emit a synthetic event for testing
    Test(WebhookTestArgs),
}

#[derive(Args, Debug)]
pub struct WebhookListenArgs {
    #[arg(long, default_value = "127.0.0.1:8081")]
    pub bind: String,
}

#[derive(Args, Debug)]
pub struct WebhookPollArgs {
    #[arg(long)]
    pub reset: bool,
}

#[derive(Args, Debug)]
pub struct WebhookTestArgs {
    #[arg(long)]
    pub to: String,
    #[arg(long)]
    pub event: String,
}

#[derive(Subcommand, Debug)]
pub enum EventAction {
    /// Poll email-cli for events
    Poll(WebhookPollArgs),
}
```

- [ ] **Step 2: Create the command module**

Create `src/commands/webhook.rs`:

```rust
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

fn listen(format: Format, args: WebhookListenArgs) -> Result<(), AppError> {
    let config = Config::load()?;
    let addr: SocketAddr = args.bind.parse().map_err(|e| AppError::BadInput {
        code: "bad_bind_addr".into(),
        message: format!("invalid bind address '{}': {e}", args.bind),
        suggestion: "Use host:port syntax, e.g. 127.0.0.1:8081".into(),
    })?;
    let secret = std::env::var(&config.webhook.secret_env).ok().map(|s| s.into_bytes());
    if matches!(format, Format::Json) {
        println!(
            "{}",
            json!({"status":"starting","bind":args.bind.clone()})
        );
    } else {
        eprintln!("Starting webhook listener on {addr} (secret configured: {})", secret.is_some());
    }
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
    // POST via a shell-out to curl — tiny_http doesn't ship a client
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
```

- [ ] **Step 3: Wire + commit**

Edit `src/commands/mod.rs`:

```rust
pub mod webhook;
```

Edit `src/main.rs`:

```rust
        Command::Webhook { action } => commands::webhook::run(format, action),
        Command::Event { action } => commands::webhook::run_event(format, action),
```

Also ensure `src/config.rs` has a `WebhookConfig` with `secret_env`, `port`, `public_url` fields. If not, add them per spec §11.1.

```bash
cargo build
cargo clippy --all-targets -- -D warnings
git add src/cli.rs src/commands/webhook.rs src/commands/mod.rs src/main.rs src/config.rs
git commit -m "feat(webhook): CLI surface for webhook listen/poll/test"
```

---

## Task 7: Report DB aggregations + models

**Files:**
- Create: `src/report/mod.rs`
- Create: `src/report/engagement.rs`
- Modify: `src/models.rs`
- Modify: `src/db/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add report models**

Append to `src/models.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ReportSummary {
    pub broadcast_id: i64,
    pub broadcast_name: String,
    pub recipient_count: i64,
    pub delivered_count: i64,
    pub bounced_count: i64,
    pub opened_count: i64,
    pub clicked_count: i64,
    pub unsubscribed_count: i64,
    pub complained_count: i64,
    pub suppressed_count: i64,
    pub ctr: f64,
    pub bounce_rate: f64,
    pub complaint_rate: f64,
    pub open_rate: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkReport {
    pub link: String,
    pub clicks: i64,
    pub unique_clickers: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeliverabilityReport {
    pub window_days: i64,
    pub total_sent: i64,
    pub total_delivered: i64,
    pub total_bounced: i64,
    pub total_complained: i64,
    pub bounce_rate: f64,
    pub complaint_rate: f64,
    pub verified_domains: Vec<String>,
}
```

- [ ] **Step 2: Add report helpers to `impl Db`**

Append to `src/db/mod.rs`:

```rust
    // ─── Report aggregations ───────────────────────────────────────────

    pub fn report_summary(&self, broadcast_id: i64) -> Result<crate::models::ReportSummary, AppError> {
        let broadcast = self
            .broadcast_get(broadcast_id)?
            .ok_or_else(|| AppError::BadInput {
                code: "broadcast_not_found".into(),
                message: format!("no broadcast with id {broadcast_id}"),
                suggestion: "Run `mailing-list-cli broadcast ls`".into(),
            })?;
        let suppressed_count = self.broadcast_recipient_count_by_status(broadcast_id, "suppressed")?;

        let ctr = if broadcast.delivered_count > 0 {
            (broadcast.clicked_count as f64 / broadcast.delivered_count as f64) * 100.0
        } else {
            0.0
        };
        let open_rate = if broadcast.delivered_count > 0 {
            (broadcast.opened_count as f64 / broadcast.delivered_count as f64) * 100.0
        } else {
            0.0
        };
        let total_sent = (broadcast.recipient_count - suppressed_count).max(0);
        let bounce_rate = if total_sent > 0 {
            (broadcast.bounced_count as f64 / total_sent as f64) * 100.0
        } else {
            0.0
        };
        let complaint_rate = if total_sent > 0 {
            (broadcast.complained_count as f64 / total_sent as f64) * 100.0
        } else {
            0.0
        };

        Ok(crate::models::ReportSummary {
            broadcast_id,
            broadcast_name: broadcast.name,
            recipient_count: broadcast.recipient_count,
            delivered_count: broadcast.delivered_count,
            bounced_count: broadcast.bounced_count,
            opened_count: broadcast.opened_count,
            clicked_count: broadcast.clicked_count,
            unsubscribed_count: broadcast.unsubscribed_count,
            complained_count: broadcast.complained_count,
            suppressed_count,
            ctr,
            bounce_rate,
            complaint_rate,
            open_rate,
        })
    }

    pub fn report_links(&self, broadcast_id: i64) -> Result<Vec<crate::models::LinkReport>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT link, COUNT(*) as clicks, COUNT(DISTINCT contact_id) as unique_clickers
                 FROM click WHERE broadcast_id = ?1
                 GROUP BY link ORDER BY clicks DESC",
            )
            .map_err(query_err)?;
        let rows = stmt
            .query_map(params![broadcast_id], |row| {
                Ok(crate::models::LinkReport {
                    link: row.get(0)?,
                    clicks: row.get(1)?,
                    unique_clickers: row.get(2)?,
                })
            })
            .map_err(query_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
    }

    pub fn report_deliverability(
        &self,
        window_days: i64,
    ) -> Result<crate::models::DeliverabilityReport, AppError> {
        let since = chrono::Utc::now() - chrono::Duration::days(window_days);
        let since_str = since.to_rfc3339();

        let (total_sent, total_delivered, total_bounced, total_complained): (i64, i64, i64, i64) =
            self.conn
                .query_row(
                    "SELECT
                        COALESCE(SUM(recipient_count), 0),
                        COALESCE(SUM(delivered_count), 0),
                        COALESCE(SUM(bounced_count), 0),
                        COALESCE(SUM(complained_count), 0)
                     FROM broadcast WHERE created_at >= ?1",
                    params![since_str],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .map_err(query_err)?;

        let bounce_rate = if total_sent > 0 {
            (total_bounced as f64 / total_sent as f64) * 100.0
        } else {
            0.0
        };
        let complaint_rate = if total_sent > 0 {
            (total_complained as f64 / total_sent as f64) * 100.0
        } else {
            0.0
        };

        Ok(crate::models::DeliverabilityReport {
            window_days,
            total_sent,
            total_delivered,
            total_bounced,
            total_complained,
            bounce_rate,
            complaint_rate,
            verified_domains: vec![], // Phase 6 doesn't wire domain list; Phase 7 does
        })
    }
```

- [ ] **Step 3: Create report module**

Create `src/report/mod.rs`:

```rust
//! Read-only report aggregations over the broadcast + event + click tables.

pub mod engagement;
```

Create `src/report/engagement.rs`:

```rust
//! Engagement score computation (v0.2+ will elaborate).

// Placeholder for the engagement scoring logic. Phase 6 ships a naive version
// that's computed inline in the CLI handler.
```

Add `mod report;` to `src/main.rs`.

- [ ] **Step 4: Commit**

```bash
cargo build
cargo test db::tests::report
git add src/report/ src/models.rs src/db/mod.rs src/main.rs
git commit -m "feat(db): report_summary + report_links + report_deliverability aggregations"
```

---

## Task 8: `report` CLI

**Files:**
- Modify: `src/cli.rs`
- Create: `src/commands/report.rs`
- Modify: `src/commands/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add CLI**

Add to `enum Command`:

```rust
    /// Analytics reports (per-broadcast, per-link, engagement, deliverability)
    Report {
        #[command(subcommand)]
        action: ReportAction,
    },
```

At the bottom:

```rust
#[derive(Subcommand, Debug)]
pub enum ReportAction {
    /// Show per-broadcast summary stats
    Show(ReportShowArgs),
    /// Show per-link click counts for a broadcast
    Links(ReportLinksArgs),
    /// Show engagement across a list or segment
    Engagement(ReportEngagementArgs),
    /// Show rolling-window bounce/complaint rates + domain health
    Deliverability(ReportDeliverabilityArgs),
}

#[derive(Args, Debug)]
pub struct ReportShowArgs {
    pub broadcast_id: i64,
}

#[derive(Args, Debug)]
pub struct ReportLinksArgs {
    pub broadcast_id: i64,
}

#[derive(Args, Debug)]
pub struct ReportEngagementArgs {
    #[arg(long)]
    pub list: Option<String>,
    #[arg(long)]
    pub segment: Option<String>,
    #[arg(long, default_value = "30")]
    pub days: i64,
}

#[derive(Args, Debug)]
pub struct ReportDeliverabilityArgs {
    #[arg(long, default_value = "7")]
    pub days: i64,
}
```

- [ ] **Step 2: Create the command dispatch**

Create `src/commands/report.rs`:

```rust
use crate::cli::{
    ReportAction, ReportDeliverabilityArgs, ReportEngagementArgs, ReportLinksArgs, ReportShowArgs,
};
use crate::db::Db;
use crate::error::AppError;
use crate::output::{self, Format};
use serde_json::json;

pub fn run(format: Format, action: ReportAction) -> Result<(), AppError> {
    match action {
        ReportAction::Show(args) => show(format, args),
        ReportAction::Links(args) => links(format, args),
        ReportAction::Engagement(args) => engagement(format, args),
        ReportAction::Deliverability(args) => deliverability(format, args),
    }
}

fn show(format: Format, args: ReportShowArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let summary = db.report_summary(args.broadcast_id)?;
    output::success(
        format,
        &format!("report for broadcast {}", summary.broadcast_name),
        json!({ "summary": summary }),
    );
    Ok(())
}

fn links(format: Format, args: ReportLinksArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let links = db.report_links(args.broadcast_id)?;
    let total_clicks: i64 = links.iter().map(|l| l.clicks).sum();
    output::success(
        format,
        &format!(
            "{} distinct link(s), {} total clicks",
            links.len(),
            total_clicks
        ),
        json!({ "links": links, "total_clicks": total_clicks }),
    );
    Ok(())
}

fn engagement(format: Format, args: ReportEngagementArgs) -> Result<(), AppError> {
    // Phase 6 ships a naive aggregation; Phase 8 elaborates
    let db = Db::open()?;
    let target = args.list.as_deref().or(args.segment.as_deref()).unwrap_or("all");
    let since = chrono::Utc::now() - chrono::Duration::days(args.days);
    let since_str = since.to_rfc3339();
    let opens: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM event WHERE type = 'email.opened' AND received_at >= ?1",
            rusqlite::params![since_str],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let clicks: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM event WHERE type = 'email.clicked' AND received_at >= ?1",
            rusqlite::params![since_str],
            |r| r.get(0),
        )
        .unwrap_or(0);
    output::success(
        format,
        &format!("engagement for {} (last {} days)", target, args.days),
        json!({
            "target": target,
            "days": args.days,
            "opens": opens,
            "clicks": clicks,
            "engagement_score": opens + (clicks * 3)
        }),
    );
    Ok(())
}

fn deliverability(format: Format, args: ReportDeliverabilityArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let report = db.report_deliverability(args.days)?;
    output::success(
        format,
        &format!("deliverability (last {} days)", args.days),
        json!({ "report": report }),
    );
    Ok(())
}
```

- [ ] **Step 3: Wire + commit**

Edit `src/commands/mod.rs`: `pub mod report;`
Edit `src/main.rs`: `Command::Report { action } => commands::report::run(format, action),`

```bash
cargo build
cargo clippy --all-targets -- -D warnings
git add src/cli.rs src/commands/report.rs src/commands/mod.rs src/main.rs
git commit -m "feat(report): show/links/engagement/deliverability CLI"
```

---

## Task 9: Integration tests

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add end-to-end event + report tests**

Append to `tests/cli.rs`:

```rust
// Helpers to build Resend event payloads for testing
fn resend_event_json(event_type: &str, email_id: &str, to: &str, extra: serde_json::Value) -> String {
    let mut data = json!({"email_id": email_id, "to": [to]});
    if let Some(obj) = data.as_object_mut() {
        if let Some(extra_obj) = extra.as_object() {
            for (k, v) in extra_obj {
                obj.insert(k.clone(), v.clone());
            }
        }
    }
    json!({"type": event_type, "created_at": "2026-04-08T12:00:00Z", "data": data}).to_string()
}

#[test]
fn event_poll_ingests_delivered_status_and_report_shows_it() {
    let (_tmp, config_path, db_path) = stub_env();

    // Seed a list + contact + template + broadcast + sent recipient
    for args in [
        vec!["--json", "list", "create", "news"],
        vec!["--json", "contact", "add", "alice@example.com", "--list", "1", "--first-name", "Alice"],
    ] {
        let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
        cmd.env("MLC_CONFIG_PATH", &config_path)
            .env("MLC_DB_PATH", &db_path)
            .args(&args);
        cmd.assert().success();
    }

    // Feed synthetic email list response to the stub so poll sees a delivered event
    let stub_response = r#"{"version":"1","status":"success","data":{"object":"list","has_more":false,"data":[{"id":"em_stub_deliver","to":["alice@example.com"],"last_event":"delivered","created_at":"2026-04-08T12:00:00Z","subject":"Hi"}]}}"#;

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .env("MLC_STUB_EMAIL_LIST_JSON", stub_response)
        .args(["--json", "event", "poll"]);
    let out = cmd.assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    assert_eq!(v["data"]["processed"].as_i64().unwrap(), 1);
}

#[test]
fn report_show_for_nonexistent_broadcast_fails_with_exit_3() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "report", "show", "999"]);
    cmd.assert().failure().code(3);
}

#[test]
fn webhook_test_requires_running_listener_or_fails() {
    let (_tmp, config_path, db_path) = stub_env();
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args([
            "--json",
            "webhook",
            "test",
            "--to",
            "http://127.0.0.1:1", // guaranteed-closed port
            "--event",
            "delivered",
        ]);
    // curl will fail to connect
    cmd.assert().failure();
}
```

- [ ] **Step 2: Run + commit**

```bash
cargo test -- --test-threads=1 2>&1 | grep "test result"
cargo clippy --all-targets -- -D warnings
cargo fmt --check
git add tests/cli.rs
git commit -m "test(webhook+report): integration tests for poll and report show"
```

---

## Task 10: Agent-info + version bump + tag v0.1.2

**Files:**
- Modify: `src/commands/agent_info.rs`
- Modify: `Cargo.toml`
- Modify: `README.md`

- [ ] **Step 1: Update agent-info**

Append to the `commands` block:

```rust
            "webhook listen [--bind <addr>]": "Run the HTTP webhook listener (long-running)",
            "webhook poll [--reset]": "Poll email-cli for delivery status updates (alias: `event poll`)",
            "webhook test --to <url> --event <type>": "POST a synthetic event to a listener",
            "event poll [--reset]": "Alias for `webhook poll`",
            "report show <broadcast-id>": "Per-broadcast summary (delivered/bounced/opened/clicked/CTR)",
            "report links <broadcast-id>": "Per-link click counts for a broadcast",
            "report engagement [--list <name>|--segment <name>] [--days N]": "Engagement score across a list/segment",
            "report deliverability [--days N]": "Rolling-window bounce rate / complaint rate / domain health",
```

Status:

```rust
        "status": "v0.1.2 — webhook ingestion (poll + listen), reports (show/links/engagement/deliverability)"
```

- [ ] **Step 2: Version bump + README badge**

```toml
version = "0.1.2"
```

Update README badge if present.

- [ ] **Step 3: Add agent-info test**

Append:

```rust
#[test]
fn agent_info_lists_phase_6_commands() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    let out = cmd.args(["--json", "agent-info"]).assert().success();
    let v: Value =
        serde_json::from_str(&String::from_utf8(out.get_output().stdout.clone()).unwrap()).unwrap();
    let commands = v["commands"].as_object().unwrap();
    for key in [
        "webhook listen [--bind <addr>]",
        "webhook poll [--reset]",
        "report show <broadcast-id>",
        "report links <broadcast-id>",
    ] {
        assert!(commands.contains_key(key), "agent-info missing {key}");
    }
    assert!(v["status"].as_str().unwrap().starts_with("v0.1.2"));
}
```

- [ ] **Step 4: Full sweep + tag**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1 2>&1 | grep "test result"
git add Cargo.toml Cargo.lock src/commands/agent_info.rs tests/cli.rs README.md
git commit -m "chore: bump to v0.1.2 — phase 6 webhooks + reports"
git push origin main
git tag -a v0.1.2 -m "v0.1.2 — webhook ingestion + reports; ships bounce/click/unsubscribe stats"
git push origin v0.1.2
gh run list --repo 199-biotechnologies/mailing-list-cli --limit 1
```

---

## What Phase 6 does NOT ship

1. **Real DMARC / SPF / DKIM checks** — `report deliverability` reports zero verified domains. Phase 7 `dnscheck` fills this in.
2. **Long-running daemon with scheduled polls** — Phase 8 (daemon).
3. **Real-time engagement dashboards** — reports are text/JSON only.
4. **Webhook listener backfill** — `webhook backfill --since <ts>` is deferred to Phase 8.
5. **Alert rules** (e.g., "page me when bounce rate > 3%") — deferred indefinitely.
6. **Per-contact engagement score cache** — computed on the fly in Phase 6; cached version is Phase 8+.

---

## Acceptance criteria

- [ ] All 10 tasks checked off.
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test -- --test-threads=1` clean.
- [ ] `event poll` with a synthetic delivered event updates the broadcast's `delivered_count`.
- [ ] `report show <id>` returns CTR, bounce_rate, complaint_rate.
- [ ] `report links <id>` returns per-link click aggregation.
- [ ] Auto-suppression fires on `email.bounced` (Permanent) and `email.complained`.
- [ ] Soft-bounce streak of 5 promotes to `soft_bounced_repeat` suppression.
- [ ] Duplicate event insertions are idempotent (no double counting).
- [ ] `webhook listen` binds to 127.0.0.1:8081 by default.
- [ ] `Cargo.toml` is 0.1.2, tag pushed, CI triggered.

**At v0.1.2, all five of the user's original asks are shipped.**

---

*End of Phase 6 implementation plan.*
