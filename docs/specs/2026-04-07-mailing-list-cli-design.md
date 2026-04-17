# mailing-list-cli — Design Spec

**Date:** 2026-04-07
**Status:** Draft v1 — **partially superseded by v0.2 (2026-04-08).** See banner below.
**Author:** Boris Djordjevic / 199 Biotechnologies

---

> ## ⚠️ SUPERSEDED SECTIONS (v0.2, 2026-04-08)
>
> This spec was written for the v0.1 architecture. **v0.2** aggressively rearchitected the template system and dropped several subsystems wholesale after a three-way review (Claude + Codex gpt-5.4 xhigh + Gemini 3.1-pro-preview). Read this document for background and architectural intent, but treat the following sections as **historical, not current**:
>
> - **§4.5 Templates** — command surface changed: `template preview --out-dir <dir>` **added** as the iteration primitive (writes rendered HTML/text to disk); `template render` now returns a JSON envelope with embedded html/text (same role, new shape); `template edit` and `template guidelines` **removed** (violated "no interactive prompts, ever" and replaced by the built-in scaffold respectively). See `src/commands/template.rs`.
> - **§4.9 Webhook ingestion** — `webhook listen` and `webhook test` **removed** (violated AGENTS.md: email-cli owns the listener). `webhook poll` and `webhook process` remain.
> - **§6 Filter Expression Language** — the PEST grammar and `segment create` textual DSL were **deleted**. Segments are now JSON AST only; agents pass `--filter-json <json>` or `--filter-json-file <path>`.
> - **§7 Templates** — MJML + Handlebars + YAML frontmatter + 20-rule lint → **deleted**. v0.2 uses plain HTML with a hand-rolled `{{ var }}` / `{{{ allowlist }}}` / `{{#if}}` substituter and **6 lint rules**. See below and `src/template/{subst,render}.rs`.
> - **§10 Webhook Listener** — removed entirely in v0.2. email-cli owns the webhook endpoint; mailing-list-cli consumes events via `webhook poll`.
> - **§16 Appendix: Embedded Template Authoring Guide** — `assets/template-authoring.md` was **deleted**. The built-in template scaffold in `src/commands/template.rs::SCAFFOLD` is now the only template documentation an agent sees.
>
> **Canonical v0.2 reference:** [`docs/plans/2026-04-08-phase-7-v0.2-rearchitecture.md`](../plans/2026-04-08-phase-7-v0.2-rearchitecture.md)
>
> **v0.2 shipped in:** `v0.2.0` (6ea71d4) → `v0.2.1` (9d8c6d1, real-Resend validated) → `v0.2.2` (5bad68a, current stable)
>
> **v0.2 net effect:** 23 → 14 runtime crates (-39%), ~9500 → ~5500 LoC (-42%), 20 → 6 template lint rules (-70%). `mrml`, `handlebars`, `css-inline`, `html2text`, `serde_yaml`, `pest`, `pest_derive`, `tiny_http` all dropped.

---

## 1. Overview

`mailing-list-cli` is a single Rust binary that gives an AI agent (or a human at a terminal) a complete mailing list manager. It owns the orchestration layer — campaigns, segments, templates, suppression, double opt-in, A/B testing, analytics — while shelling out to its sister tool [`email-cli`](https://github.com/199-biotechnologies/email-cli) for every actual Resend API touchpoint. Two binaries, one job each.

It follows the [agent-cli-framework](https://github.com/199-biotechnologies/agent-cli-framework) patterns: structured JSON output auto-detected via `IsTerminal`, semantic exit codes (`0`/`1`/`2`/`3`/`4`), self-describing `agent-info`, no interactive prompts, ever.

### 1.1 Goals

1. **An AI agent can run a 50,000-subscriber newsletter from the terminal** without ever opening a browser, learning a tool schema up front, or installing an MCP server.
2. **Every Resend touchpoint goes through `email-cli`.** `mailing-list-cli` has zero Resend HTTP code.
3. **Compliance is non-skippable.** RFC 8058 one-click unsubscribe, global suppression list, double opt-in by default, CAN-SPAM physical address footer, GDPR erasure — all enforced at the dispatch boundary, no flag to disable.
4. **Local-first.** All campaign metadata, suppression history, templates, and event mirrors live in SQLite under `~/.local/share/mailing-list-cli/`. The cache directory is always safe to delete.
5. **Templates are first-class.** A local template store, MJML compiled in-process via [`mrml`](https://crates.io/crates/mrml), Mustache merge tags via [`handlebars`](https://crates.io/crates/handlebars), explicit authoring guidelines surfaced via `template guidelines`.
6. **Cold start under 10 ms.** Single static Rust binary, no Node, no JIT.

### 1.2 Non-goals (v0.1)

- A web dashboard. The terminal IS the dashboard.
- Visual block editor. Templates are MJML or HTML files; the agent can author both.
- Multi-channel (SMS, push, in-app). Email only.
- Predictive analytics, AI subject-line suggesters, RFM scoring. Out of scope until v1.
- Hosted unsubscribe page (Resend already provides one — we use it).
- Inbound parsing. `email-cli` covers received mail.
- Direct competition with the SaaS tier of MailChimp/Beehiiv. We are not a marketing platform — we are an automation surface.

### 1.3 Reference: research dossiers

Five research dossiers under [`/research`](../../research/) ground every decision in this spec:

1. [`01-modern-creator-newsletters.md`](../../research/01-modern-creator-newsletters.md) — Beehiiv, Buttondown, Substack, the 80/20 of what creators with 5k–100k lists actually use.
2. [`02-marketing-platforms.md`](../../research/02-marketing-platforms.md) — MailChimp, MailerLite, Kit data models and command surfaces.
3. [`03-resend-native.md`](../../research/03-resend-native.md) — exact Resend API endpoints and what they cover.
4. [`04-deliverability-compliance.md`](../../research/04-deliverability-compliance.md) — non-negotiables for safe scale operation.
5. [`05-templates.md`](../../research/05-templates.md) — MJML + handlebars + agent authoring guidelines.

---

## 2. Architecture

```
┌──────────────────────────────────────────┐
│             Your Agent / You             │
│        (Claude, Codex, Gemini, …)        │
└────────────────┬─────────────────────────┘
                 │  CLI commands, JSON in/out
                 ▼
┌──────────────────────────────────────────┐
│             mailing-list-cli             │
│   campaigns · segments · A/B · opt-in    │
│   suppression · analytics · templates    │
│   webhook listener (own)                 │
└────────┬──────────────────┬──────────────┘
         │  shells out      │  reads/writes
         │  for sending     │  local state
         ▼                  ▼
┌──────────────────┐  ┌────────────┐
│     email-cli    │  │   SQLite   │
│ • send / batch   │  │ templates  │
│ • audiences      │  │ campaigns  │
│ • contacts       │  │ suppression│
│ • domain config  │  │ events     │
│ • outbox / retry │  │ optin tok. │
└─────────┬────────┘  └────────────┘
          │
          ▼
     ┌──────────┐         ┌──────────┐
     │  Resend  │ ◄──────►│ Webhooks │
     └──────────┘         └────┬─────┘
                               │  (mailing-list-cli's port)
                               ▼
                         (event ingestion
                          mirrors to local DB)
```

### 2.1 Layer responsibilities

**`mailing-list-cli`** (this binary)

- All campaign/list/segment/template/suppression/opt-in/A/B/analytics state
- Webhook listener bound to a port the user chooses (default `8081`)
- Local SQLite at `~/.local/share/mailing-list-cli/state.db`
- Shells out to `email-cli` for every Resend API call

**`email-cli`** (hard dependency)

- Sole Resend API client. Manages profile/API-key, sender accounts, domain config, the outbox/retry queue
- `email-cli send` for transactional 1:1 sends (used by `optin start` and `broadcast preview`)
- `email-cli batch send --file <json>` for chunked broadcast sends
- `email-cli audience create/list/delete` for the Resend audience that backs each `mailing-list-cli` list
- `email-cli contact create/update/delete --audience <id>` for syncing contact email/first/last names to Resend so the hosted unsubscribe flow works
- `email-cli domain update --open-tracking true --click-tracking true` for enabling tracking
- `email-cli profile test` for health checks

**Resend** (the underlying ESP)

- Delivery, hosted unsubscribe page, basic open/click tracking, automatic hard-bounce/complaint suppression at the account level, per-broadcast `{{{RESEND_UNSUBSCRIBE_URL}}}` rendering

### 2.2 Why the orchestration layer is its own binary

A 1:1 messaging tool and a mailing list tool are different products that happen to share a transport. Cramming both into `email-cli` would (a) double its surface area, (b) couple the durable list-state to the inbox-state, (c) force every `email-cli` consumer to learn list semantics they don't need, and (d) make the CLI less learnable by an LLM agent because there are too many top-level nouns.

Splitting along the natural fault line keeps each binary focused and lets each evolve at its own pace.

### 2.3 What `mailing-list-cli` does NOT do

- It does not open an HTTP connection to Resend, ever.
- It does not store API keys. `email-cli` has them.
- It does not synthesize or replay inbox events.
- It does not host a UI of any kind.

---

## 3. Data Model

All state lives in a single SQLite database at `~/.local/share/mailing-list-cli/state.db`. Migrations are versioned and applied automatically on first run.

### 3.1 Tables

```sql
-- ─── Lists ─────────────────────────────────────────────────────────────
CREATE TABLE list (
    id              INTEGER PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    description     TEXT,
    resend_audience_id  TEXT NOT NULL UNIQUE,  -- backed by an email-cli audience
    created_at      TEXT NOT NULL,
    archived_at     TEXT
);

-- ─── Contacts (the local source of truth) ───────────────────────────────
CREATE TABLE contact (
    id              INTEGER PRIMARY KEY,
    email           TEXT NOT NULL UNIQUE COLLATE NOCASE,
    first_name      TEXT,
    last_name       TEXT,
    status          TEXT NOT NULL CHECK (status IN ('pending', 'active', 'unsubscribed', 'bounced', 'complained', 'cleaned', 'erased')),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    -- Consent record
    consent_source  TEXT,         -- form URL, import filename, manual, etc.
    consent_ip      TEXT,
    consent_user_agent  TEXT,
    consent_text    TEXT,         -- exact opt-in language shown to user
    consent_at      TEXT,
    confirmed_at    TEXT          -- when double opt-in was confirmed
);

CREATE INDEX idx_contact_email ON contact(email);
CREATE INDEX idx_contact_status ON contact(status);

-- ─── List membership (n:m) ──────────────────────────────────────────────
CREATE TABLE list_membership (
    list_id         INTEGER NOT NULL REFERENCES list(id) ON DELETE CASCADE,
    contact_id      INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
    joined_at       TEXT NOT NULL,
    PRIMARY KEY (list_id, contact_id)
);

-- ─── Tags (n:m with contacts) ───────────────────────────────────────────
CREATE TABLE tag (
    id              INTEGER PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE
);

CREATE TABLE contact_tag (
    contact_id      INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
    tag_id          INTEGER NOT NULL REFERENCES tag(id) ON DELETE CASCADE,
    applied_at      TEXT NOT NULL,
    PRIMARY KEY (contact_id, tag_id)
);

-- ─── Custom fields ──────────────────────────────────────────────────────
CREATE TABLE field (
    id              INTEGER PRIMARY KEY,
    key             TEXT NOT NULL UNIQUE,    -- snake_case
    type            TEXT NOT NULL CHECK (type IN ('text', 'number', 'date', 'bool', 'select')),
    options_json    TEXT,                    -- for select type
    created_at      TEXT NOT NULL
);

CREATE TABLE contact_field_value (
    contact_id      INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
    field_id        INTEGER NOT NULL REFERENCES field(id) ON DELETE CASCADE,
    value_text      TEXT,
    value_number    REAL,
    value_date      TEXT,
    value_bool      INTEGER,
    PRIMARY KEY (contact_id, field_id)
);

-- ─── Segments (saved filters) ───────────────────────────────────────────
CREATE TABLE segment (
    id              INTEGER PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    filter_json     TEXT NOT NULL,           -- serialized SegmentExpr AST
    created_at      TEXT NOT NULL
);

-- ─── Templates ──────────────────────────────────────────────────────────
CREATE TABLE template (
    id              INTEGER PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,    -- snake_case
    subject         TEXT NOT NULL,           -- can contain merge tags
    mjml_source     TEXT NOT NULL,
    schema_json     TEXT NOT NULL,           -- declared variable schema
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

-- ─── Broadcasts (campaigns) ─────────────────────────────────────────────
CREATE TABLE broadcast (
    id              INTEGER PRIMARY KEY,
    name            TEXT NOT NULL,
    template_id     INTEGER NOT NULL REFERENCES template(id),
    target_kind     TEXT NOT NULL CHECK (target_kind IN ('list', 'segment')),
    target_id       INTEGER NOT NULL,
    status          TEXT NOT NULL CHECK (status IN ('draft', 'scheduled', 'sending', 'sent', 'cancelled', 'failed')),
    scheduled_at    TEXT,
    sent_at         TEXT,
    created_at      TEXT NOT NULL,
    -- A/B test fields
    ab_variant_of   INTEGER REFERENCES broadcast(id),
    ab_winner_pick  TEXT CHECK (ab_winner_pick IN ('opens', 'clicks', 'manual')),
    ab_sample_pct   INTEGER,
    ab_decided_at   TEXT,
    -- Stats (denormalized for fast read)
    recipient_count INTEGER DEFAULT 0,
    delivered_count INTEGER DEFAULT 0,
    bounced_count   INTEGER DEFAULT 0,
    opened_count    INTEGER DEFAULT 0,
    clicked_count   INTEGER DEFAULT 0,
    unsubscribed_count INTEGER DEFAULT 0,
    complained_count   INTEGER DEFAULT 0
);

-- ─── Per-broadcast recipient log ────────────────────────────────────────
CREATE TABLE broadcast_recipient (
    id              INTEGER PRIMARY KEY,
    broadcast_id    INTEGER NOT NULL REFERENCES broadcast(id) ON DELETE CASCADE,
    contact_id      INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
    resend_email_id TEXT,                    -- assigned after send
    status          TEXT NOT NULL CHECK (status IN ('pending', 'sent', 'delivered', 'bounced', 'complained', 'failed', 'suppressed')),
    sent_at         TEXT,
    last_event_at   TEXT,
    UNIQUE (broadcast_id, contact_id)
);

CREATE INDEX idx_recipient_broadcast ON broadcast_recipient(broadcast_id);
CREATE INDEX idx_recipient_resend ON broadcast_recipient(resend_email_id);

-- ─── Suppression list (the global authority) ────────────────────────────
CREATE TABLE suppression (
    email           TEXT PRIMARY KEY COLLATE NOCASE,
    reason          TEXT NOT NULL CHECK (reason IN (
        'unsubscribed', 'hard_bounced', 'soft_bounced_repeat',
        'complained', 'manually_blocked', 'spam_trap_hit',
        'gdpr_erasure', 'inactive_sunsetted', 'role_account'
    )),
    suppressed_at   TEXT NOT NULL,
    source_broadcast_id INTEGER REFERENCES broadcast(id) ON DELETE SET NULL,
    notes           TEXT
);

-- ─── Soft bounce streak counter ─────────────────────────────────────────
CREATE TABLE soft_bounce_count (
    contact_id      INTEGER PRIMARY KEY REFERENCES contact(id) ON DELETE CASCADE,
    consecutive     INTEGER NOT NULL DEFAULT 0,
    last_bounce_at  TEXT NOT NULL,
    last_subtype    TEXT
);

-- ─── Webhook event mirror ───────────────────────────────────────────────
CREATE TABLE event (
    id              INTEGER PRIMARY KEY,
    type            TEXT NOT NULL,           -- 'email.bounced', 'email.opened', etc
    resend_email_id TEXT NOT NULL,
    broadcast_id    INTEGER REFERENCES broadcast(id) ON DELETE SET NULL,
    contact_id      INTEGER REFERENCES contact(id) ON DELETE SET NULL,
    payload_json    TEXT NOT NULL,
    received_at     TEXT NOT NULL
);

CREATE INDEX idx_event_email_id ON event(resend_email_id);
CREATE INDEX idx_event_type ON event(type);
CREATE INDEX idx_event_broadcast ON event(broadcast_id);

-- ─── Click events (one row per click for per-link analytics) ────────────
CREATE TABLE click (
    id              INTEGER PRIMARY KEY,
    broadcast_id    INTEGER NOT NULL REFERENCES broadcast(id) ON DELETE CASCADE,
    contact_id      INTEGER REFERENCES contact(id),
    link            TEXT NOT NULL,
    ip_address      TEXT,
    user_agent      TEXT,
    clicked_at      TEXT NOT NULL
);

CREATE INDEX idx_click_broadcast ON click(broadcast_id);
CREATE INDEX idx_click_link ON click(link);

-- ─── Double opt-in tokens ───────────────────────────────────────────────
CREATE TABLE optin_token (
    token           TEXT PRIMARY KEY,        -- HMAC of email+nonce+timestamp
    contact_id      INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
    list_id         INTEGER REFERENCES list(id) ON DELETE SET NULL,
    issued_at       TEXT NOT NULL,
    expires_at      TEXT NOT NULL,           -- 7 days from issued_at
    redeemed_at     TEXT
);
```

### 3.2 Why everything lives locally

- **Custom fields, tags, segments** — Resend's contacts API only supports a free-form `properties` object, and `email-cli`'s `contact create` command doesn't even surface that. So the local DB is the source of truth and the Resend audience is just a backup of email addresses (so the hosted unsubscribe flow works).
- **Suppression** — Resend's suppression list is dashboard-only with no API. We keep our own and enforce it at every send.
- **Templates** — We compile MJML locally with `mrml`, so templates never leave the binary.
- **Events** — Resend's events API is per-message-id only (via email-cli's wrapper), so we run our own webhook listener and mirror events into the local DB for fast querying.
- **Opt-in tokens** — These are short-lived secrets; storing them in Resend would be both architecturally wrong and a security smell.

### 3.3 The `Contact` is local; the Resend mirror is downstream

When a new contact is added:

1. Insert into `contact` table locally
2. Insert into `list_membership` for the requested list
3. Asynchronously call `email-cli contact create --audience <resend_audience_id> --email --first-name --last-name`
4. If the call fails (rate limit, transient error), retry with backoff; the local state remains the source of truth
5. The Resend-side contact exists purely so the hosted unsubscribe page knows about the address

When a contact is deleted (`contact erase` for GDPR):

1. Mark local `contact.status = 'erased'`, clear PII fields, retain `id` for foreign keys
2. Add the email to `suppression` with reason `gdpr_erasure`
3. Call `email-cli contact delete --audience <resend_audience_id> <id>`
4. Hard-delete from `event`, `click`, `optin_token` tables
5. Keep the suppression entry forever (it's an empty row from a PII standpoint)

---

## 4. Command Surface

The full command surface, grouped by noun. Every command supports `--json` for forced JSON output (also auto-enabled when stdout is not a TTY).

### 4.1 Lists

| Command | Description |
|---|---|
| `list create <name> [--description <text>]` | Create a list. Also creates a backing Resend audience via `email-cli`. |
| `list ls` | List all lists with subscriber counts. |
| `list show <id>` | Show one list's metadata. |
| `list rename <id> <new-name>` | Rename. |
| `list archive <id>` | Soft-delete: mark `archived_at`, leave history intact. |
| `list rm <id> --confirm` | Hard-delete. Refuses unless `--confirm` is passed. |

### 4.2 Contacts

| Command | Description |
|---|---|
| `contact add <email> --list <id> [--first-name X --last-name Y --field key=val ...]` | Add a single contact. |
| `contact import <file.csv> --list <id> [--double-opt-in]` | Bulk import. Rate-limit-aware (5 req/sec to Resend via email-cli). Streams the file, deduplicates against suppression list, returns counts. |
| `contact tag <email> <tag>` | Apply a tag. Creates the tag if it doesn't exist. |
| `contact untag <email> <tag>` | Remove a tag. |
| `contact set <email> <field> <value>` | Set a custom field value. |
| `contact ls [--list <id>] [--filter <expr>] [--limit N] [--cursor C]` | List contacts. Filter syntax described in §6. |
| `contact show <email>` | Full contact details: status, fields, tags, list memberships, recent events. |
| `contact erase <email> --confirm` | GDPR Article 17 hard erasure. Requires `--confirm`. |
| `contact resubscribe <email> --confirm` | Move from `unsubscribed` back to `active`. Refused unless suppression entry is removed first. |

### 4.3 Tags & Fields

| Command | Description |
|---|---|
| `tag ls` | List all tags with member counts. |
| `tag rm <name> --confirm` | Delete a tag (removes from all contacts). |
| `field create <key> --type <text\|number\|date\|bool\|select> [--options "a,b,c"]` | Create a custom field. |
| `field ls` | List all custom fields. |
| `field rm <key> --confirm` | Delete a custom field. |

### 4.4 Segments

| Command | Description |
|---|---|
| `segment create <name> --filter <expr>` | Save a dynamic segment. The filter is parsed at create time and stored as JSON AST. |
| `segment ls` | List all segments with current member counts. |
| `segment show <name>` | Show the filter expression and a sample of matching contacts. |
| `segment members <name> [--limit N]` | List the contacts currently matching the segment. |
| `segment rm <name> --confirm` | Delete a segment definition (does not affect contacts). |

Filter expressions are described in detail in §6.

### 4.5 Templates

| Command | Description |
|---|---|
| `template create <name> [--from-file <path>]` | Scaffold a new template (or import an existing MJML file). |
| `template ls` | List all templates with last-modified time. |
| `template show <name>` | Print MJML source. |
| `template render <name> [--with-data <file.json>]` | Compile to HTML and print. With `--with-data`, also fills merge tags. |
| `template lint <name>` | Validate MJML syntax, check declared schema vs actual usage, run cross-client warnings (no flexbox, no CSS grid, etc). Exits non-zero on error. |
| `template edit <name>` | Open in `$EDITOR`. (One of the few commands that touches an interactive tool — but no prompts inside `mailing-list-cli` itself.) |
| `template rm <name> --confirm` | Delete. |
| `template guidelines` | Print the embedded agent authoring guide (rules, examples, gotchas). The full content of the guide lives in §9 and is compiled into the binary as `include_str!`. |

### 4.6 Broadcasts (campaigns)

| Command | Description |
|---|---|
| `broadcast create --name <n> --template <tpl> --to <list-or-segment>` | Stage a campaign in `draft` status. |
| `broadcast preview <id> --to <test-email>` | Send a single test email via `email-cli send`. Uses the first contact in the target as the merge-tag source, or a placeholder if `--with-data` is passed. |
| `broadcast schedule <id> --at <time>` | Move from `draft` to `scheduled`. Time accepts ISO 8601 or natural language (`"in 1h"`, `"tomorrow 9am"`). |
| `broadcast send <id>` | Send immediately. Resolves segment, applies suppression filter, renders templates, writes a JSON batch file, calls `email-cli batch send --file …` in chunks of 100. |
| `broadcast cancel <id>` | Cancel a scheduled broadcast. Only works in `draft` or `scheduled` state. |
| `broadcast ls [--status <s>] [--limit N]` | List recent broadcasts. |
| `broadcast show <id>` | Status, recipient count, current stats. |
| `broadcast ab <id> --vary <subject\|body> --variants 2 --sample-pct 10 --winner-by <opens\|clicks> --decide-after <duration>` | Configure A/B test on a draft broadcast. |
| `broadcast ab-promote <id>` | Manually promote the winner if `--winner-by manual` was used. |

### 4.7 Reports (analytics)

| Command | Description |
|---|---|
| `report show <broadcast-id>` | Per-broadcast: opens, clicks, bounces, unsubscribes, complaints, CTR, suppression hits, time-series. |
| `report links <broadcast-id>` | Click count per link in the broadcast. |
| `report engagement [--list <id>\|--segment <name>] [--days N]` | Engagement scores across a list or segment. |
| `report deliverability [--days N]` | Domain health: bounce rate, complaint rate, DMARC pass rate (queried via `email-cli` health endpoints). |

### 4.8 Compliance & opt-in

| Command | Description |
|---|---|
| `optin start <email> --list <id>` | Begin double opt-in: insert as `pending`, generate token, send confirmation via `email-cli send` with the embedded `confirmation` template. |
| `optin verify <token>` | Mark the contact as `active`, record `confirmed_at`. Returns the contact JSON. |
| `optin pending` | List all `pending` contacts with token expiry. |
| `unsubscribe <email> [--reason <text>]` | Honor an unsubscribe. Adds to `suppression` with reason `unsubscribed`, marks contact `unsubscribed`, calls `email-cli contact update --unsubscribed true`. |
| `suppression ls [--reason <r>] [--limit N]` | View the global suppression list. |
| `suppression add <email> --reason <r> [--notes <text>]` | Manually suppress an address. |
| `suppression rm <email> --confirm` | Remove from suppression. Refused unless reason allows reactivation (`unsubscribed` only). |
| `suppression import <file>` | Import suppressions from a CSV (e.g. when migrating from another platform). |
| `suppression export [--reason <r>]` | Export to CSV. |
| `dnscheck <domain>` | Verify SPF, DKIM, DMARC, FCrDNS, TLS configuration on a sending domain via `email-cli`. |

### 4.9 Webhook ingestion

| Command | Description |
|---|---|
| `webhook listen [--port 8081]` | Run a long-lived HTTP server that receives Resend webhook events and mirrors them into the local DB. Honors signature verification using the configured webhook secret. |
| `webhook backfill --since <time>` | Best-effort backfill: queries `email-cli events list` per-message for any sent messages without recent event mirroring. (Limited utility because email-cli's events API is per-message; this is mostly a recovery tool.) |
| `webhook test --to <listener-url> --event <type>` | Generate a sample event payload and POST it to a listener. Useful for testing local development. |

### 4.10 Daemon

| Command | Description |
|---|---|
| `daemon start [--port 8081]` | Convenience wrapper: runs `webhook listen` + the scheduled-broadcast tick + soft-bounce sweep + sunset evaluator in one process. |
| `daemon status` | Show running daemon PID, uptime, last event received. |
| `daemon stop` | Gracefully shut down. |

### 4.11 Agent tooling

| Command | Description |
|---|---|
| `agent-info` | Self-describing JSON manifest of every command, flag, and exit code. Always pure JSON, never wrapped in the envelope. |
| `skill install` | Drop the embedded skill file into Claude / Codex / Gemini paths. |
| `skill status` | Show which platforms have the skill installed. |
| `update [--check]` | Self-update from GitHub Releases. |
| `completions <shell>` | Generate shell completions (bash / zsh / fish / powershell). |
| `health` | One-shot health check: `email-cli` on PATH, profile reachable, DB writable, webhook port available, sending domain authenticated. Returns a JSON envelope with everything that's wrong. |

---

## 5. The Send Pipeline (how a broadcast actually flows)

This is the most important sequence in the binary. It is the place where most platforms fail compliance.

```
broadcast send <id>
  │
  ├─ 1. Load broadcast metadata + template + target (list or segment)
  │
  ├─ 2. Resolve target → list of contact_ids
  │     - If target_kind = 'list': SELECT contact_id FROM list_membership WHERE list_id = ?
  │     - If target_kind = 'segment': evaluate filter AST against contacts table
  │
  ├─ 3. Pre-flight invariant checks (any failure = exit 2, refuse to send):
  │     ✓ Sender domain has DKIM/SPF/DMARC pass (via `email-cli profile test`)
  │     ✓ Template lints clean (no broken merge tags)
  │     ✓ Physical address footer is configured in config.toml
  │     ✓ Account-level complaint rate over last 7 days < 0.30%
  │     ✓ Recipient count ≤ configured `max_recipients_per_send`
  │
  ├─ 4. Suppression filter (the dispatch boundary):
  │     - DELETE FROM the recipient list any email in `suppression`
  │     - DELETE FROM the recipient list any contact with status != 'active'
  │     - Log a `suppression_hit` row for each filtered contact (visible in report)
  │
  ├─ 5. Render templates per recipient (in chunks of 100):
  │     - Build the merge data dict from contact.first_name, contact.last_name, custom fields
  │     - Run `mrml` to compile MJML → HTML
  │     - Run `handlebars` to substitute merge tags
  │     - Run `css-inline` to inline CSS for Outlook
  │     - Run `html2text` to generate plain-text alternative
  │     - Inject CAN-SPAM physical address footer (cannot be skipped)
  │     - Inject `List-Unsubscribe` and `List-Unsubscribe-Post` headers (RFC 8058)
  │     - Sign the unsubscribe URL with HMAC for one-click validation
  │
  ├─ 6. Write a JSON batch file to a tmp path:
  │     [
  │       {
  │         "from": "newsletter@yourdomain.com",
  │         "to": ["alice@example.com"],
  │         "subject": "Hi Alice",
  │         "html": "<rendered-html>",
  │         "text": "<plain-text-alt>",
  │         "headers": {
  │           "List-Unsubscribe": "<https://yourdomain/u/SIGNED_TOKEN>, <mailto:unsubscribe+SIGNED_TOKEN@yourdomain.com>",
  │           "List-Unsubscribe-Post": "List-Unsubscribe=One-Click"
  │         },
  │         "tags": [{"name": "broadcast_id", "value": "42"}]
  │       },
  │       ... up to 100 entries per file ...
  │     ]
  │
  ├─ 7. Call `email-cli batch send --file <path> --json`
  │     - Capture the response (array of {id, to})
  │     - For each id, INSERT INTO broadcast_recipient (broadcast_id, contact_id, resend_email_id, status='sent', sent_at=...)
  │     - On chunk failure: do NOT retry blindly; mark the chunk as failed and surface to the operator
  │
  ├─ 8. Mark broadcast.status = 'sent', set sent_at
  │
  └─ 9. Output JSON envelope with summary stats: sent, suppressed, failed
```

### 5.1 Why steps 3 and 4 are non-skippable

Steps 3 (pre-flight invariants) and 4 (suppression filter) are the dispatch boundary. They are the only place a non-active contact can be excluded. There is no `--force` flag. If you need to send to a suppressed address, you remove the suppression entry first (with `suppression rm`), and that requires explicit confirmation.

This is the correction for the #1 cause of mailing list disasters at scale: a bug elsewhere in the system that lets a suppressed address through.

### 5.2 Chunking and rate limits

Resend's batch send API accepts up to 100 emails per call. `email-cli batch send` wraps this. `mailing-list-cli` always chunks at 100 and never exceeds the configured `max_concurrent_chunks` (default 1). The 5 req/sec Resend rate limit means a 50,000-recipient broadcast takes ~100 seconds at 1 chunk/sec, which is acceptable.

A future optimization: parallelize chunks via the email-cli outbox if `max_concurrent_chunks > 1`. Defer until the simple path is proven.

### 5.3 Failure semantics

- **Whole broadcast fails before sending any chunks**: `broadcast.status = 'failed'`, no recipient rows written, error JSON returned with exit code 2.
- **Some chunks succeed, some fail**: `broadcast.status = 'sent'` but with a `partial_failure` field in the result JSON listing failed chunks. Operator can manually `broadcast retry-chunk <id> <chunk-index>` (a future command).
- **Network drops mid-call**: the email-cli outbox handles retry automatically; we trust it.

---

## 6. Filter Expression Language

Used by `contact ls --filter`, `segment create --filter`, and `report engagement`. Designed to be simple enough for an LLM to author and a Rust parser to handle.

### 6.1 Grammar

```
expr     := or_expr
or_expr  := and_expr ('OR' and_expr)*
and_expr := not_expr ('AND' not_expr)*
not_expr := 'NOT'? atom
atom     := condition | '(' expr ')'

condition := key ':' op ':' value
           | key ':' value           // implicit '=' op
           | engagement_atom
           | tag_atom
           | list_atom

engagement_atom := 'opened_last:' duration
                 | 'clicked_last:' duration
                 | 'sent_last:' duration
                 | 'never_opened'
                 | 'inactive_for:' duration

tag_atom  := 'tag:' tag_name
           | 'has_tag:' tag_name
           | 'no_tag:' tag_name

list_atom := 'list:' list_name
           | 'in_list:' list_name
           | 'not_in_list:' list_name

duration := <integer><unit>     // e.g. '30d', '6h', '2w', '90d'
unit     := 'd' | 'h' | 'w' | 'm' (months)
op       := '=' | '!=' | '~' | '!~' | '>' | '<' | '>=' | '<='
```

### 6.2 Examples

```
tag:vip
tag:vip AND opened_last:30d
list:newsletter AND NOT bounced
status:active AND city:Berlin AND opened_last:90d
has_tag:premium AND (clicked_last:7d OR opened_last:14d)
inactive_for:180d AND NOT has_tag:do_not_sunset
```

### 6.3 Implementation

Parsed via the [`pest`](https://crates.io/crates/pest) crate with a `.pest` grammar file. The parsed expression is converted to a typed `SegmentExpr` Rust enum, which is both:

1. Serialized to JSON for storage in the `segment.filter_json` column
2. Compiled to a SQL `WHERE` clause at evaluation time

Engagement atoms (`opened_last:30d`, etc.) translate to subqueries against the `event` table, which is why we mirror events locally.

---

## 7. Templates

> **⚠️ SUPERSEDED in v0.2 (2026-04-08).** Canonical reference: [`docs/plans/2026-04-08-phase-7-v0.2-rearchitecture.md`](../plans/2026-04-08-phase-7-v0.2-rearchitecture.md).
>
> This section documents the v0.1 MJML + Handlebars + YAML-frontmatter architecture. All of it was deleted in v0.2.0 after a three-way review (Claude + Codex + Gemini) converged on an agent-native simplification thesis. **The current v0.2 template system is:**
>
> - **Source format:** plain HTML (no MJML, no `mrml`).
> - **Variable substitution:** a hand-rolled `{{ var }}` / `{{{ allowlist }}}` / `{{#if}}` / `{{#unless}}` substituter in `src/template/subst.rs` (~400 lines). No Handlebars.
> - **Frontmatter:** removed. There is no declared variable schema. Unresolved placeholders are detected and **hard-fail at send time** in `broadcast send` via strict-mode `render()` (`src/template/render.rs`, called from `src/broadcast/pipeline.rs`).
> - **Lint rules:** **6** (was 20). Unsubscribe link present, physical-address footer present, size under Gmail clip limit, no forbidden tags (`<script>`, `<iframe>`, `<object>`, etc.), unresolved placeholders in preview mode only, triple-brace XSS allowlist honored.
> - **CSS inlining:** removed (`css-inline` dropped). Write inline styles directly. Modern email clients handle `<style>` blocks fine.
> - **Plain-text fallback:** generated by a hand-rolled HTML-to-text stripper in `src/template/render.rs`. No `html2text` dep.
> - **`template preview` is the iteration primitive.** Agents render templates to disk via `template preview <name> --with-data <file> --out-dir <dir>` and loop until happy. The v0.1 `template guidelines` command and `template edit` were removed.
> - **The scaffold IS the documentation.** `assets/template-authoring.md` was deleted; its role is now played by the built-in `SCAFFOLD` constant in `src/commands/template.rs`, emitted by `template create`.
>
> **Why:** agents have preview + iteration loops. Declare-time variable schemas, 20 lint rules, and a 153-line embedded doctrine were designed for blind human authors. Once you commit to agent-with-preview, all of it becomes dead weight. -42% LoC, -39% runtime deps, first-try smoke test against real Resend (paperfoot.com, us-east-1) passed end-to-end.
>
> **Don't resurrect** any of: MJML, Handlebars, YAML frontmatter, `template render`, `template edit`, `template guidelines`, `css-inline`, `html2text`, declared variable schemas, or the 14 deleted lint rules. See the "Gotchas" section of `docs/handoffs/2026-04-08-session-3-handoff.md` for the full don't-do list.
>
> **The text below is preserved for historical context only.** Do not rely on it when implementing or reviewing v0.2+ code.

### 7.1 Format

**MJML** as the source format, parsed and rendered by [`mrml`](https://crates.io/crates/mrml). Variables use **Mustache-style `{{ snake_case }}`** rendered by [`handlebars`](https://crates.io/crates/handlebars). YAML frontmatter declares the variable schema.

Example template (`welcome.mjml.hbs`):

```mjml
---
name: welcome
subject: "Welcome to {{ list_name }}, {{ first_name }}"
variables:
  - name: first_name
    type: string
    required: true
  - name: list_name
    type: string
    required: true
  - name: confirmation_url
    type: string
    required: true
---
<mjml>
  <mj-head>
    <mj-title>Welcome</mj-title>
    <mj-preview>Confirm your email to get started.</mj-preview>
  </mj-head>
  <mj-body background-color="#f4f4f4">
    <mj-section background-color="#ffffff" padding="20px">
      <mj-column>
        <mj-text font-size="24px" font-weight="700">
          Welcome, {{ first_name }}.
        </mj-text>
        <mj-text>
          Click the button below to confirm your subscription to {{ list_name }}.
        </mj-text>
        <mj-button href="{{ confirmation_url }}" background-color="#000000">
          Confirm subscription
        </mj-button>
        <mj-text font-size="12px" color="#666666">
          You're receiving this because you signed up at our website.
          {{{ unsubscribe_link }}}
        </mj-text>
      </mj-column>
    </mj-section>
  </mj-body>
</mjml>
```

The triple-brace `{{{ unsubscribe_link }}}` is unescaped (so the HTML link tag passes through). Everything else is HTML-escaped automatically.

### 7.2 Compilation pipeline

```
template.mjml.hbs
  │
  ├─ Parse YAML frontmatter → variable schema
  ├─ Validate variable schema against the data dict
  │
  ├─ Render Handlebars merge tags → MJML with placeholders filled
  │
  ├─ Compile MJML → HTML via mrml
  │
  ├─ Inline CSS via css-inline (because mrml doesn't yet inline mj-style[inline])
  │
  ├─ Inject CAN-SPAM physical address footer into the last <mj-section>
  │
  ├─ Inject one-click unsubscribe link replacement for {{{ unsubscribe_link }}}
  │
  ├─ Generate plain-text alternative via html2text
  │
  └─ Output { html: String, text: String, subject: String }
```

### 7.3 Lint rules

`template lint <name>` runs all of:

| Check | Severity |
|---|---|
| MJML parses without errors (`mrml::parse`) | error |
| All declared variables are used in the template body or subject | warning |
| All used variables are declared in the schema | error |
| No `<style>` block uses `flex`, `grid`, `float:`, or `position:` | error |
| No raw `<table>`, `<div>` outside of an `<mj-raw>` escape block | warning |
| Final HTML size after inlining is under 102 KB (Gmail clipping limit) | warning |
| Plain-text version is non-empty | error |
| Subject is non-empty and < 100 characters | warning |
| Mandatory `{{{ unsubscribe_link }}}` placeholder is present | error |
| Mandatory `{{{ physical_address_footer }}}` placeholder is present | error |

### 7.4 The agent authoring guide

Printed by `template guidelines`. Stored as `assets/template-authoring.md` and compiled into the binary via `include_str!`. The guide is a single page that an LLM agent reads before authoring its first template. The full content is in §9 below.

---

## 8. Suppression List Semantics

The suppression list is the single most important data structure in the binary. The send pipeline (§5) cannot bypass it. Every send filters every recipient against it before any HTTP call.

### 8.1 Sources of suppression entries

1. **Webhook events** — `email.bounced` (if `type=Permanent`), `email.complained`, `email.suppressed` all auto-create entries.
2. **Soft bounce streak** — 5 consecutive `email.delivery_delayed` for the same contact with no successful delivery in between → auto-suppress with reason `soft_bounced_repeat`. Reset counter on any successful delivery.
3. **One-click unsubscribe** — `webhook listen` exposes a `POST /u/<token>` endpoint that validates the HMAC and adds an `unsubscribed` entry.
4. **Manual** — `suppression add <email> --reason <r>`.
5. **GDPR erasure** — `contact erase <email>` adds a `gdpr_erasure` entry and hard-deletes PII.
6. **Sunset evaluator** — the daemon runs a daily sweep that adds `inactive_sunsetted` entries for contacts that completed a re-engagement sequence without engagement.
7. **Import** — `suppression import <file>` for migration from another platform.

### 8.2 Reactivation

Only `unsubscribed` entries can be removed. All other reasons are permanent. Attempts to `suppression rm` a `hard_bounced` or `complained` entry return exit code 3 with a suggestion to investigate the underlying cause first.

### 8.3 Cross-list scope

The suppression list is **global across all lists and segments**. A user who unsubscribes from "Newsletter" cannot receive "Product Updates" either. This is the CAN-SPAM and GDPR position and it is the default. There is no per-list suppression in v0.1.

---

## 9. Compliance Boundaries

### 9.1 Non-negotiable invariants

These are enforced at the dispatch boundary (§5 step 3) and cannot be disabled by any flag:

1. **Global suppression list filter** before every send.
2. **`List-Unsubscribe` header** with HTTPS and mailto values.
3. **`List-Unsubscribe-Post: List-Unsubscribe=One-Click`** header.
4. **`unsubscribe_link` placeholder** must be present in every broadcast template.
5. **`physical_address_footer` placeholder** must be present in every broadcast template.
6. **`physical_address` config field** must be set before any `broadcast send` succeeds.
7. **Domain authentication check** before the first send to a domain (DKIM, SPF, DMARC).
8. **Complaint rate kill switch**: refuse to send if rolling 7-day complaint rate > 0.30% until operator runs `health --override-complaint-check` (which is logged).
9. **Bounce rate kill switch**: refuse to send if rolling 7-day bounce rate > 4%.
10. **Recipient count cap** per send (default 50,000; configurable, but logged on every override).

### 9.2 GDPR erasure flow

```
contact erase <email> --confirm
  │
  ├─ Look up contact, capture id
  ├─ INSERT INTO suppression (email, reason='gdpr_erasure', ...)
  ├─ UPDATE contact SET status='erased', email='<id>@erased.local',
  │       first_name=NULL, last_name=NULL, consent_*=NULL
  ├─ DELETE FROM contact_field_value WHERE contact_id = ?
  ├─ DELETE FROM contact_tag WHERE contact_id = ?
  ├─ DELETE FROM list_membership WHERE contact_id = ?
  ├─ DELETE FROM event WHERE contact_id = ?
  ├─ DELETE FROM click WHERE contact_id = ?
  ├─ DELETE FROM optin_token WHERE contact_id = ?
  ├─ Call `email-cli contact delete --audience <a> <id>` for every audience the contact was in
  └─ Log the erasure in an append-only audit log at ~/.local/share/mailing-list-cli/audit.log
```

The reason for the email-rewrite (`<id>@erased.local`) instead of NULL is that the `contact` row has to remain to preserve referential integrity for `broadcast_recipient`, but it must contain no PII.

### 9.3 Consent record

Every contact added via `contact add`, `contact import`, or `optin start` records:
- `consent_source` — form URL, file path, or `manual`
- `consent_ip` — when known
- `consent_user_agent` — when known
- `consent_text` — the exact opt-in language shown to the user
- `consent_at` — ISO 8601
- `confirmed_at` — ISO 8601 (only set after `optin verify`)

`contact import` requires every CSV row to include a `consent_source` column or it refuses to run, unless `--unsafe-no-consent` is passed (which logs a warning and tags every imported contact with `imported_without_consent` for downstream filtering).

---

## 10. Webhook Listener

### 10.1 Endpoints

| Path | Method | Purpose |
|---|---|---|
| `/webhook` | POST | Receive Resend webhook events |
| `/u/<token>` | GET, POST | One-click unsubscribe endpoint (RFC 8058) |
| `/optin/<token>` | GET | Double opt-in confirmation |
| `/health` | GET | Liveness check (returns `{"status":"ok"}`) |

### 10.2 Resend webhook events handled

| Event type | Action |
|---|---|
| `email.sent` | UPDATE broadcast_recipient SET status='sent' |
| `email.delivered` | UPDATE broadcast_recipient SET status='delivered'; INCR broadcast.delivered_count |
| `email.delivery_delayed` | INSERT/UPDATE soft_bounce_count; check threshold |
| `email.bounced` | If `type=Permanent`: INSERT INTO suppression, UPDATE contact.status='bounced', UPDATE broadcast_recipient |
| `email.complained` | INSERT INTO suppression with reason='complained', UPDATE contact.status='complained' |
| `email.suppressed` | INSERT INTO suppression (already-suppressed sends count as a hit) |
| `email.opened` | INSERT INTO event; INCR broadcast.opened_count (idempotent on duplicate) |
| `email.clicked` | INSERT INTO event AND click; INCR broadcast.clicked_count |
| `email.failed` | UPDATE broadcast_recipient SET status='failed' |
| `email.scheduled` | (informational, no action) |

Other event types (`contact.*`, `domain.*`, `email.received`) are ignored — they belong to `email-cli`'s domain.

### 10.3 Signature verification

Resend signs webhooks with Svix-style HMAC. The webhook listener verifies every payload against the configured secret. Invalid signatures return 401. The secret is configured via:

```toml
# ~/.config/mailing-list-cli/config.toml
[webhook]
secret_env = "RESEND_WEBHOOK_SECRET"
```

### 10.4 Why we run our own listener instead of polling email-cli

`email-cli`'s `events list` command takes only `--message <id>`, which would require us to poll per-message. That doesn't scale. So `mailing-list-cli` runs its own listener bound to a port the user picks. The user configures Resend to POST events to two webhook URLs: one for `email-cli` (inbox events on the existing port) and one for `mailing-list-cli` (delivery events on the new port). Resend supports multiple webhooks per account.

If `email-cli` later adds a `events list --since <ts> --type <t>` command, we can switch to polling and remove the listener. That's a future simplification.

---

## 11. Configuration

### 11.1 Config file

`~/.config/mailing-list-cli/config.toml`:

```toml
# ~/.config/mailing-list-cli/config.toml

[sender]
# The default `from` for broadcasts. Must be a verified sender on Resend (managed by email-cli).
from = "newsletter@yourdomain.com"
reply_to = "hello@yourdomain.com"

# CAN-SPAM physical address — required, refuses to send without it
physical_address = """
Paperfoot AI (SG) Pte. Ltd.
123 Example Street, #01-23
Singapore 123456
"""

[webhook]
# Port for the local listener
port = 8081
# Resend signs webhooks; supply the secret via env or directly
secret_env = "RESEND_WEBHOOK_SECRET"
# Public URL where Resend should POST events (used by `webhook configure`)
public_url = "https://hooks.yourdomain.com/webhook"

[unsubscribe]
# Public URL prefix for the one-click unsubscribe endpoint
public_url = "https://hooks.yourdomain.com/u"
# HMAC secret for signing tokens
secret_env = "MLC_UNSUBSCRIBE_SECRET"

[guards]
# Refuse to send if the rolling 7-day complaint rate exceeds this
max_complaint_rate = 0.003   # 0.30%
max_bounce_rate = 0.04       # 4%
max_recipients_per_send = 50000

[email_cli]
# Path to the email-cli binary (defaults to `email-cli` on PATH)
path = "email-cli"
# Profile name to use
profile = "default"
```

### 11.2 Environment variables

| Name | Purpose | Required |
|---|---|---|
| `RESEND_WEBHOOK_SECRET` | Resend webhook signing secret | yes (for `webhook listen`) |
| `MLC_UNSUBSCRIBE_SECRET` | HMAC secret for unsubscribe tokens | yes (for any `broadcast send`) |
| `MLC_DB_PATH` | Override the SQLite path | no |
| `MLC_CONFIG_PATH` | Override the config file path | no |

### 11.3 Filesystem layout

```
~/.config/mailing-list-cli/
    config.toml

~/.local/share/mailing-list-cli/
    state.db                    # the only durable file
    audit.log                   # append-only erasure / override log
    daemon.pid                  # when daemon is running
    daemon.sock                 # control socket

~/.cache/mailing-list-cli/
    template-renders/           # rendered HTML for previews — safe to delete
    batch-files/                # JSON batch files awaiting `email-cli batch send` — also safe to delete (but check first)
```

`rm -rf ~/.config/mailing-list-cli ~/.local/share/mailing-list-cli ~/.cache/mailing-list-cli` resets the binary to factory state.

---

## 12. Error Model

### 12.1 JSON envelope

Success:
```json
{
  "version": "1",
  "status": "success",
  "data": { ... }
}
```

Error:
```json
{
  "version": "1",
  "status": "error",
  "error": {
    "code": "suppression_blocked",
    "message": "Cannot send to alice@example.com — address is on the global suppression list",
    "suggestion": "Run `mailing-list-cli suppression ls --reason unsubscribed` to see why; use `suppression rm` if you intend to override"
  }
}
```

### 12.2 Exit codes

| Code | Meaning | Agent action |
|---|---|---|
| `0` | Success | Continue |
| `1` | Transient (network, IO, email-cli unavailable) | Retry with backoff |
| `2` | Config error (missing physical_address, missing webhook secret, email-cli not on PATH) | Fix setup, do not retry |
| `3` | Bad input (invalid filter expression, unknown contact, malformed CSV) | Fix arguments |
| `4` | Rate limited (Resend rate limit propagated through email-cli) | Wait and retry |

### 12.3 Errors as instructions

Every error envelope carries a `suggestion` field that is a literal command the agent can run to either fix the issue or get more context. A suggestion that doesn't work is a P0 bug. The framework tests this contract.

---

## 13. The `email-cli` Interface (exact commands invoked)

These are the only `email-cli` invocations `mailing-list-cli` makes. Every one of them is auto-tested against a stub `email-cli` binary in CI.

**Minimum `email-cli` version: 0.6.0.** The `audience` noun was retired in v0.6; contacts live in the flat `/contacts` namespace and lists back onto Resend segments (Resend renamed Audiences → Segments in November 2025).

### 13.1 Health & config

```bash
email-cli --json agent-info
email-cli --json profile test default
```

### 13.2 Segments (one per `mailing-list-cli` list)

```bash
email-cli --json segment create --name "<list-name>"
email-cli --json segment list
email-cli --json segment delete <id>
email-cli --json segment contact-add    --contact <id-or-email> --segment <seg-id>
email-cli --json segment contact-remove --contact <id-or-email> --segment <seg-id>
```

### 13.3 Contacts (flat /contacts namespace)

```bash
email-cli --json contact create \
    --email <e> \
    --first-name <f> --last-name <l> \
    [--segments seg_abc,seg_def] \
    [--properties '{"company":"Acme","plan":"pro"}']

email-cli --json contact update <id_or_email> \
    --unsubscribed true \
    [--properties '{"plan":"enterprise"}']

email-cli --json contact delete <id_or_email>
email-cli --json contact list [--limit N] [--after <cursor>]
email-cli --json contact get <id_or_email>
```

### 13.4 Sending

```bash
# Single send (used by optin start and broadcast preview)
email-cli --json send --account <a> --to <e> --subject <s> --html <h> --text <t>

# Native broadcast (preferred path once mailing-list-cli's broadcast
# feature lands — gives us auto-wired per-recipient unsubscribe URLs)
email-cli --json broadcast create \
    --segment-id <seg-id> \
    --from "Name <sender@example.com>" \
    --subject <subject> \
    --html <html> \
    [--scheduled-at <iso>] \
    [--topic-id <topic-id>] \
    [--send]
email-cli --json broadcast send <broadcast-id>
email-cli --json broadcast delete <broadcast-id>   # doubles as cancel for scheduled
email-cli --json broadcast get <broadcast-id>

# Batch send fallback (chunks of 100, used only when broadcast path is unavailable)
email-cli --json batch send --file <path-to-json>
```

### 13.5 Delivery status polling

```bash
# Replaces mailing-list-cli's second webhook listener (see §10.4).
# Poll every N seconds, advance the cursor to the latest seen id.
email-cli --json email list --limit 100 --after <latest-seen-email-id>
```

Each row Resend returns includes a `last_event` field (`"sent"`, `"delivered"`, `"bounced"`, etc) which is enough for the bounce / suppression path. The full engagement stream (open → click → complaint) still comes from webhook events mirrored by `email-cli webhook listen` or by `mailing-list-cli`'s own listener.

### 13.6 Domain config

```bash
email-cli --json domain update <id> --open-tracking true --click-tracking true
email-cli --json domain list
```

### 13.7 Contact properties schema (optional)

```bash
# Define a typed custom-field schema at the Resend level so that
# `contact create --properties '{"company":"Acme"}'` is accepted.
email-cli --json contact-property create --key company --property-type string
email-cli --json contact-property list
email-cli --json contact-property delete <id>
```

This is only needed if you want custom fields to be visible on Resend's hosted preference center and in Resend-side template merge tags. If you're happy with local-only custom fields, skip it.

### 13.8 No other email-cli commands are invoked.

In particular, `mailing-list-cli` does NOT use:
- `email-cli inbox` (not its job)
- `email-cli draft` (not its job)
- `email-cli sync` (not its job)
- `email-cli webhook listen` (we run our own — see §10.4; or we skip it and poll `email list` instead)
- `email-cli events list` (per-message-only, kept for email-cli's own debugging)
- `email-cli template` (we compile MJML in-process via `mrml`)
- `email-cli topic` (deferred until we add preference center support)

---

## 14. Open questions and deferred decisions

1. **Drip / sequence automations** — out of scope for v0.1. Will land in v0.6 as a `automation` noun with a DAG model. Spec to follow separately.
2. **Forms / signup widgets** — out of scope for v0.1. Operators are expected to host their own form and call `optin start` from a webhook handler.
3. **Per-recipient send-time optimization** (best time to send) — out of scope. Resend doesn't expose per-recipient timezone enrichment.
4. **Multivariate testing** — v0.1 ships subject + body A/B only. Multivariate is v1+.
5. **Geo / device analytics** — webhook payloads carry IP and user-agent only. Enrichment via `maxminddb` is a v0.5+ enhancement.
6. **Markdown body shells** — the research dossier on templates recommends a hybrid "MJML shell + Markdown body" path. Defer to v0.3 — v0.2 ships pure MJML.
7. **Custom domain for the unsubscribe page** — v0.1 uses Resend's hosted page. v0.5+ adds support for self-hosting via the existing webhook listener.
8. **Multi-tenant operation** — v0.1 is single-tenant. Multi-tenant (multiple `[sender]` blocks, scoped state) is v1+.

---

## 15. Roadmap to v0.1

The phases below match the pinned [roadmap issue](https://github.com/paperfoot/mailing-list-cli/issues/1).

| Phase | Tag | Scope |
|---|---|---|
| **Phase 1 — Foundations** | v0.0.1 | Cargo scaffold, config, SQLite migrations, JSON envelope, exit codes, `agent-info`, `health`, `email-cli` dependency check |
| **Phase 2 — Lists & contacts** | v0.0.2 | `list`, `contact`, `tag`, `field` (excluding `import` for v0.0.2; that lands in v0.0.3) |
| **Phase 3 — Segments** | v0.0.3 | Filter expression parser, `segment` commands |
| **Phase 4 — Templates** | v0.1.0 | MJML compilation, lint, `template guidelines`, the embedded authoring guide |
| **Phase 5 — Broadcasts (no A/B)** | v0.1.1 | `broadcast create/preview/schedule/send/cancel/ls/show`, the full send pipeline |
| **Phase 6 — Webhook listener + reports** | v0.1.2 | `webhook listen`, event ingestion, `report show/links/engagement` |
| **Phase 7 — Compliance + opt-in** | v0.1.3 | `optin start/verify`, `unsubscribe`, `suppression`, `dnscheck`, `contact erase` |
| **Phase 8 — A/B + daemon** | v0.1.4 | `broadcast ab`, `daemon start/stop` |
| **Phase 9 — Polish + release** | v0.1.5 | Homebrew tap, prebuilt binaries, shell completions, agent-info contract test |

---

## 16. Appendix: Embedded Template Authoring Guide

> **⚠️ DELETED in v0.2 (2026-04-08).** `assets/template-authoring.md` no longer exists, and `template guidelines` is no longer a command. The built-in `SCAFFOLD` constant in `src/commands/template.rs` (emitted by `template create <name>`) is the only template documentation an agent sees in v0.2+. See the banner at the top of this spec and §7 above for the v0.2 thesis. The text below is preserved for historical context only and **does not apply to v0.2+**.

The full text of `template guidelines` (also in `assets/template-authoring.md`):

````markdown
# Template Authoring for mailing-list-cli

Read this once before authoring your first template. Two minutes here saves
hours of debugging Outlook desktop.

## The single most important rule

Use only `<mj-*>` tags. Never write raw `<table>`, `<div>`, `<style>`, or any
CSS that uses `flex`, `grid`, `float`, or `position`. The MJML compiler emits
the gnarly nested-table HTML that Outlook desktop needs. If you hand-write
HTML, your email will break in Outlook for 30% of recipients.

## Template structure

Every template has YAML frontmatter declaring its variable schema, followed by
MJML markup with Mustache merge tags.

```mjml
---
name: snake_case_name
subject: "Subject line with {{ first_name }}"
variables:
  - name: first_name
    type: string
    required: true
  - name: company
    type: string
    required: false
---
<mjml>
  <mj-head>
    <mj-title>Email title (shows in tab)</mj-title>
    <mj-preview>Preview text (shows in inbox list)</mj-preview>
  </mj-head>
  <mj-body>
    <mj-section>
      <mj-column>
        <mj-text>Hello {{ first_name }}</mj-text>
        <mj-button href="https://example.com">Click here</mj-button>
        {{{ unsubscribe_link }}}
        {{{ physical_address_footer }}}
      </mj-column>
    </mj-section>
  </mj-body>
</mjml>
```

## Required placeholders

Every template MUST include both of these in the body, or `template lint`
will refuse to save it:

- `{{{ unsubscribe_link }}}` — replaced at send time with a one-click
  unsubscribe link bound to the recipient
- `{{{ physical_address_footer }}}` — replaced with your CAN-SPAM physical
  address from `config.toml`

The triple braces are mandatory: they tell Handlebars not to HTML-escape the
output, because these placeholders inject HTML.

## MJML components you can use

- `<mj-section>` — a horizontal stripe; the top-level layout primitive
- `<mj-column>` — a vertical column inside a section; up to 4 per section
- `<mj-text>` — paragraph text
- `<mj-button>` — a button with `href`, `background-color`, etc.
- `<mj-image>` — an image with `src`, `alt`, `width`, etc.
- `<mj-divider>` — a horizontal line
- `<mj-spacer>` — vertical whitespace
- `<mj-social>` / `<mj-social-element>` — social media link rows
- `<mj-table>` — a real data table (if you genuinely need one)
- `<mj-raw>` — escape hatch for hand-written HTML; only use if you know
  exactly what you're doing

Every component supports `padding`, `background-color`, `color`, `font-size`,
`font-family`, and other standard email-safe attributes.

## Merge tags

Use Mustache syntax with snake_case variable names:

```
{{ first_name }}
{{ company }}
{{ unsubscribe_link }}     ← HTML-escaped (almost never what you want for links)
{{{ unsubscribe_link }}}   ← raw HTML (what you want for the unsubscribe link)
```

Conditionals:

```
{{#if company}}
  <mj-text>From {{ company }}</mj-text>
{{else}}
  <mj-text>From a friend</mj-text>
{{/if}}
```

Loops are NOT supported in v0.1.

## Variables built into every template

You don't have to declare these — they're injected by the send pipeline:

- `first_name` (string, may be empty)
- `last_name` (string, may be empty)
- `email` (string, always present)
- `unsubscribe_link` (HTML, use triple braces)
- `physical_address_footer` (HTML, use triple braces)
- `current_year` (number)
- `broadcast_id` (number)

Any custom field you've created with `field create <key>` is also available as
`{{ key }}` if the recipient has a value for it.

## Common gotchas

1. **Don't link to `mailto:` for unsubscribe.** Use `{{{ unsubscribe_link }}}`.
   The send pipeline injects a real one-click unsubscribe URL.
2. **Don't omit the preview text.** It dramatically affects open rate.
3. **Don't put images larger than 600px wide.** Most email clients render at
   600px. Larger images get scaled, ugly.
4. **Don't use background images for important content.** Outlook desktop
   strips them.
5. **Don't write subject lines longer than 50 characters.** They get truncated
   on mobile.
6. **Don't forget the plain-text alternative.** It's auto-generated by the
   pipeline, but if your HTML is junk, the plain text will be too.

## Validation

Run `mailing-list-cli template lint <name>` before sending. It will catch
missing placeholders, broken merge tags, dangerous CSS, and gives you a
preview of the rendered HTML.

## Preview

Run `mailing-list-cli template render <name> --with-data sample.json` to
get the rendered HTML printed to stdout.

Run `mailing-list-cli broadcast preview <broadcast-id> --to your-test@email.com`
to send a real test through Resend.

## When in doubt

Read [mjml.io/try-it-live](https://mjml.io/try-it-live) — the official MJML
playground. Every component is documented there.
````

---

*End of spec.*
