# What `mailing-list-cli` needs from `email-cli`

**Date:** 2026-04-07
**Audience:** the team that maintains [`email-cli`](https://github.com/199-biotechnologies/email-cli)
**Purpose:** a focused list of what to add — and what *not* to add — so that `mailing-list-cli` can wrap a complete mailing-list workflow without doing any Resend HTTP itself.

## Architectural rule

`mailing-list-cli` has **zero Resend HTTP code**. It owns campaigns, segments, templates, suppression, double opt-in, A/B testing, and analytics. Every Resend touchpoint flows through `email-cli`. That split lets each binary stay focused: `email-cli` is the Resend client, `mailing-list-cli` is the orchestration layer on top.

This document only asks for things `mailing-list-cli` actually needs. It explicitly does not ask `email-cli` to wrap features that belong to `mailing-list-cli` itself or that Resend doesn't expose.

## What `email-cli` already covers (no action needed)

| Capability | Existing email-cli command |
|---|---|
| Capability discovery | `email-cli --json agent-info` |
| Profile / API key health check | `email-cli --json profile test <name>` |
| Audience CRUD (deprecated by Resend) | `email-cli --json audience {list,get,create,delete}` |
| Contact basic CRUD | `email-cli --json contact {list,get,create,update,delete} --audience <id>` |
| Single send | `email-cli --json send --to --subject --html --text --attach` |
| Batch send | `email-cli --json batch send --file <json>` |
| Domain CRUD + tracking flags | `email-cli --json domain {list,get,create,verify,delete,update --open-tracking --click-tracking}` |
| Webhook listener (receiver) | `email-cli webhook listen --port <p>` |
| Outbox / durable retry queue | `email-cli outbox {list,retry,flush}` |
| API key management | `email-cli --json api-key {list,create,delete}` |

`mailing-list-cli v0.0.2` is already shipping against this surface. It works.

## The three asks

### 1. MUST — `contact create --properties '{"k":"v"}'`

**What:** Plumb Resend's free-form `properties` object on `/contacts` through `email-cli contact create / update`, and surface it in `contact get / list` responses.

**Why:** `mailing-list-cli` lets a user define custom fields (`field create company --type text`) and assign values to contacts. Without `--properties`, every custom field stays in `mailing-list-cli`'s local SQLite and is invisible to Resend. Resend's hosted preference center renders no extra fields, and merge tags inside Resend templates can't reference them. This is the only feature blocker for `mailing-list-cli v0.1`.

**Underlying Resend:** `POST /contacts` and `PATCH /contacts/{id_or_email}` already accept `properties` as a free-form JSON object. The endpoint exists; only the `email-cli` flag is missing.

**Effort:** small. Plumb a `HashMap<String, Value>` field through to the existing `/contacts` request body and serialize it in the JSON response on `get` / `list`.

**Suggested CLI surface:**
```bash
email-cli --json contact create --audience <id> --email alice@example.com \
    --first-name Alice --properties '{"company":"Acme","plan":"pro"}'
email-cli --json contact update --audience <id> <contact-id> \
    --properties '{"plan":"enterprise"}'
```

---

### 2. MUST — `email list [--limit N] [--after <id>]`

**What:** A new top-level subcommand wrapping Resend's `GET /emails` that returns paginated lists of recently-sent emails along with the `last_event` field on each row.

**Why:** Today `mailing-list-cli` runs its own webhook listener on a separate port to mirror delivery events into local state. That means the user has to configure two Resend webhook URLs and run two listeners. If `email-cli` exposes `email list`, `mailing-list-cli` can poll for status (`sent → delivered → bounced`) and drop its second listener entirely.

**Underlying Resend:** `GET /emails` already exists. Each row returned by Resend includes:
- `id`, `created_at`, `from`, `to`, `subject`
- `last_event` (string, e.g. `"sent"`, `"delivered"`, `"bounced"`, `"opened"`, `"clicked"`)
- `scheduled_at` (if applicable)

Pagination is by ID via `after` and `before` (not timestamp), with `limit` 1-100, default 20.

**Caveat:** `last_event` only carries the most-recent event for an email, so the full engagement history (open → click → unsubscribe → complaint) still needs the webhook stream. That's fine — `mailing-list-cli` only needs polling for the delivery / bounce / suppression path, which is enough to retire its second webhook listener. The first listener (on the `email-cli` side) keeps mirroring full engagement events into the local DB.

**Effort:** small. Thin wrapper over a single Resend endpoint.

**Suggested CLI surface:**
```bash
email-cli --json email list --limit 100
email-cli --json email list --limit 100 --after em_abc123
```

---

### 3. SHOULD — `broadcast` noun wrapping Resend's Broadcasts API

**What:** A new `broadcast` subcommand set wrapping Resend's Broadcasts endpoints.

**Why:** `mailing-list-cli` currently sends campaigns via `email-cli batch send --file <json>`. That works but bypasses three things Resend's native Broadcasts give you for free:

1. Per-recipient `{{{RESEND_UNSUBSCRIBE_URL}}}` substitution (the hosted preference page wires up automatically per recipient).
2. Server-side scheduling via `scheduled_at` and server-side cancellation via `DELETE /broadcasts/{id}`.
3. Resend's internal queue throttling for very large audiences (it handles the rate-limit math instead of `mailing-list-cli`).

If `email-cli` wraps broadcasts, `mailing-list-cli` can stop injecting its own RFC 8058 unsubscribe headers and let Resend handle it. The `mailing-list-cli` send pipeline gets simpler.

**Underlying Resend endpoints:**

| Resend | Suggested email-cli command |
|---|---|
| `POST /broadcasts` | `email-cli broadcast create --audience-id <id> --from --subject --html [--text] [--reply-to] [--scheduled-at <iso>]` |
| `POST /broadcasts/{id}/send` | `email-cli broadcast send <id> [--scheduled-at <iso>]` |
| `GET /broadcasts` | `email-cli broadcast list` |
| `GET /broadcasts/{id}` | `email-cli broadcast get <id>` |
| `DELETE /broadcasts/{id}` | `email-cli broadcast delete <id>` (also acts as cancel for scheduled broadcasts) |

**Effort:** medium — five thin commands, all 1:1 with the Resend endpoints.

---

## What NOT to build

These either belong in `mailing-list-cli` or aren't useful:

| Thing | Why not |
|---|---|
| Templates API wrapper (`POST /templates`, etc.) | `mailing-list-cli` compiles MJML in-process via `mrml`. It never calls Resend's `/templates`. |
| Automations API wrapper (`POST /automations`, etc.) | `mailing-list-cli`'s drip / sequence layer will wrap this directly when Resend takes it out of private alpha. Not `email-cli`'s job. |
| Events API wrapper (`POST /events`) | Same as Automations — currently private alpha. It pairs with the automation engine, so it ships when `mailing-list-cli` builds the automation surface. |
| Suppression list wrapper | Resend doesn't expose a suppression-list API. Nothing to wrap. `mailing-list-cli` maintains its own local mirror from webhook events. |
| Bulk contact import (`POST /contacts/import`) | Resend has no batch contact endpoint. CSV import is dashboard-only. `mailing-list-cli` chunks individual `contact create` calls under the 5 req/sec rate limit. |
| Topics API wrapper | Only useful if `mailing-list-cli` exposes a granular preference center. Defer until we explicitly ask. |
| Contact Properties CRUD (`POST /contact-properties`, etc) | The custom-field schema is local to `mailing-list-cli`. We only need ask #1 (passing values per contact) — the schema definition stays in `mailing-list-cli`. |
| Webhook configuration CRUD | Configurable via the Resend dashboard. Only revisit if `mailing-list-cli` ever needs to wire webhooks programmatically as part of `health` / `dnscheck`. |
| Segments API CRUD beyond what we have | Same — only if `mailing-list-cli` needs server-side segments. Today segments are local. |
| Logs API wrapper (`GET /logs`) | Returns API *request* logs, not delivery events. Not what we need. |

## Summary

**Three asks. One hard blocker (`contact --properties`), one duplicate-subsystem killer (`email list`), one quality-of-life upgrade (`broadcast` noun).** Anything beyond this list is scope creep — please push back on it.

Once those three land, `mailing-list-cli` ships v0.1 fully feature-complete on top of `email-cli` with no architectural workarounds.
