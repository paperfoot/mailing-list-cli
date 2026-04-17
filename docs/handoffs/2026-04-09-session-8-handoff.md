# Session Handoff — Session 8 (v0.3.1 → v0.3.2 → v0.3.3 → v0.4.0)

**Date:** 2026-04-09
**Session:** The marathon session. Started from v0.3.0 at the end of the session-5 planning handoff. Shipped 4 releases in one sitting: three hardening patches from GPT Pro's review, then v0.4.0 "Operator Superpowers" with the features that actually matter.

---

## What shipped

| Tag | Theme | Key additions |
|---|---|---|
| v0.3.1 | Concurrency + subprocess safety | Broadcast lock CAS, email-cli timeout, schema version check, agent-info sync |
| v0.3.2 | Crash recovery + security | Write-ahead send_attempt log, unsubscribe secret hard-fail, multi-profile health check |
| v0.3.3 | Agent contract discipline | Partial send exit code, health single-output, error code consistency, stale help cleanup |
| **v0.4.0** | **Operator superpowers** | Content snapshots, dry-run, UTM links, Stripe injection, revenue tracking, report revenue/ltv, idempotent broadcast create, subscriber integration docs |

## Current state

- **Branch:** `main`
- **Commit:** `97edc1b chore: bump to v0.4.0 — operator superpowers`
- **Tag:** `v0.4.0`
- **Tests:** 142 unit + 65 integration = **207 passing**
- **Codebase:** ~15K lines of Rust (12.5K src + 2.5K tests)
- **Runtime deps:** 14 (unchanged since v0.2.0 — 9 releases on the same budget)
- **Working tree:** clean
- **Pushed:** main + v0.4.0 tag to `paperfoot/mailing-list-cli`
- **crates.io:** v0.4.0 published
- **Homebrew:** formula updated to v0.4.0

---

## What to do next: refactor and split

The user's explicit direction: "split it a little bit and refactor the code in a nicer, cleaner, and easier way to maintain and keep developing."

### The problem

The codebase grew from ~3K lines (v0.0.3) to ~15K lines (v0.4.0) in 8 sessions. Most of the growth landed in a few files that do too much. The biggest offenders:

| File | Lines | What's wrong |
|---|---|---|
| `src/db/mod.rs` | 3,286 | The god file. Every table's queries live here: lists, contacts, tags, fields, segments, templates, broadcasts, broadcast_recipients, events, suppression, revenue, send_attempts, plus migrations runner, plus 80+ unit tests. Adding a new table means scrolling past 3K lines. A split by domain is overdue. |
| `src/template/render.rs` | 1,004 | Render + lint + UTM/Stripe link rewriter + HTML-to-text + entity unescape + 30+ tests. The link rewriter and the renderer are distinct concerns. |
| `src/email_cli.rs` | 1,000 | Subprocess wrapper + retry logic + timeout helper + 8 call methods + tests. The timeout/retry infra could be a separate module; the method-per-command pattern is clean but the file is long. |
| `src/broadcast/pipeline.rs` | 928 | Send loop + write-ahead reconcile + lock acquire + dry-run + preflight + target resolution. The write-ahead reconcile block alone is ~80 lines that could be its own function or module. |
| `src/cli.rs` | 636 | All clap-derive definitions for every subcommand. Fine for now but will cross 1K when more revenue/report args are added. |

### Recommended split plan

**Priority 1: `src/db/mod.rs` → domain modules** (the biggest single win)

Split into:
```
src/db/
  mod.rs          — Db struct, open, run_migrations, schema check, query_err helper
  migrations.rs   — already split (keep as-is)
  list.rs         — list_create, list_all, list_get_by_name, list_get
  contact.rs      — contact_upsert, contact_list, contact_show, contact_erase, ...
  tag.rs          — tag_get_or_create, tag_add, tag_remove, tag_list, ...
  field.rs        — field_create, field_list, field_rm, coerce_*
  segment.rs      — segment_create, segment_list, segment_get_by_name, ...
  template.rs     — template_upsert, template_all, template_get_by_name, ...
  broadcast.rs    — broadcast_create, broadcast_get, broadcast_all, broadcast_set_status, lock methods, ...
  event.rs        — event_insert, event_dedupe, historical_send_rates, ...
  suppression.rs  — suppression_insert, suppression_all_emails, is_email_suppressed, ...
  revenue.rs      — revenue_insert, revenue_list, revenue_aggregate, revenue_ltv_top
  report.rs       — report_summary, report_links, report_deliverability, ...
```

Each file is `impl Db { ... }` blocks — Rust lets you spread impl blocks across files in the same module. The `Db` struct stays in `mod.rs`, the `conn` field stays `pub(crate)`, and each domain file adds methods to `Db`. Tests move to each domain file's `#[cfg(test)]` block.

Migration risk: **zero public API change**. All methods are still `db.foo()`. Only the file they live in changes. Tests import `Db` the same way.

**Priority 2: `src/template/render.rs` → render + links**

Split into:
```
src/template/
  mod.rs          — re-exports
  render.rs       — render, render_preview, render_inner, lint, Rendered, LintFinding
  links.rs        — inject_utm_params, inject_stripe_ref, percent_encode_simple + tests
  text.rs         — html_to_text, unescape_entities, collapse_whitespace, strip_html_comments + tests
  subst.rs        — already split (keep as-is)
```

**Priority 3: `src/broadcast/pipeline.rs` → pipeline + reconcile**

Extract the write-ahead reconcile block into `src/broadcast/reconcile.rs`. Also extract `preflight_checks` and `resolve_target` into `src/broadcast/preflight.rs`. The main `send_broadcast` function shrinks from 928 lines to ~300 (the core chunk loop).

**Priority 4: `src/email_cli.rs` → email_cli + timeout**

Extract `run_with_timeout` and the retry constants into `src/subprocess.rs` (generic, reusable). `email_cli.rs` stays as the method-per-command wrapper.

### What NOT to refactor

- Don't add abstractions. No traits, no generics, no Repository pattern. Just move methods to smaller files. The codebase is deliberately concrete.
- Don't change public behavior. The split is purely internal organization. Every command, flag, exit code, JSON shape stays identical.
- Don't add new deps. The `db/mod.rs` split in particular tempts you to add a query builder or ORM. Don't. Raw rusqlite SQL is doctrine.
- Don't batch the refactor with feature work. Ship the split as its own release (v0.4.1 or v0.5.0 depending on how you feel about semver). Zero behavior change, just file moves + `mod` declarations.

### Execution recommendation

The split is **purely mechanical** — no new tests needed, no new behavior, just cut-paste-and-compile. A session with a focused agent can do the db/mod.rs split in ~1 hour. The template and pipeline splits are smaller. Ship as one commit per split target (4 commits + version bump).

Test the split by running `cargo test -- --test-threads=1` after each file move. If the test count stays at 207 and clippy is clean, the move was correct.

---

## GPT Pro findings still outstanding

The GPT Pro hardening review from 2026-04-09 had ~35 findings. Here's what's been addressed vs what remains:

### Addressed (v0.3.1 through v0.4.0)

Broadcast lock CAS, subprocess timeout, schema version check, write-ahead attempt log, unsubscribe secret hard-fail, multi-profile health check, partial send exit code, health double-output, error code consistency, stale CLI help/stubs, report engagement error swallowing.

### Remaining for v0.5+ (in rough priority order)

| Area | What | Effort |
|---|---|---|
| Event source | webhook poll reads only `last_event` per email — lossy, not a real event stream. Complaint/bounce guards are approximate. Needs upstream email-cli change or a rolling-window snapshot diff. | l |
| Read-modify-write races | `schedule`, `cancel`, `contact_upsert`, `tag_get_or_create`, `template_upsert`, `list_create` all have check-then-act patterns without transactions. Should be UPSERTs or CAS. | m |
| Webhook handler atomicity | Event row inserted, derived writes (suppression, stats) are separate calls. Crash between = "event exists but state never updated." Wrap in one transaction. | s |
| Structured logging | All logs are `eprintln!` prose. Add JSON-line stderr logging behind `MLC_LOG=level` env var. No new deps — use existing serde_json. | s |
| Error class collapse | Every rusqlite error → `Transient/db_query_failed`. Should classify Busy→Transient, Constraint→BadInput, Corrupt→Config. | m |
| Strict email-cli response parsing | `batch_send` parser accepts mismatched array lengths, empty IDs, wrong `to` fields. Should be strict. | s |
| Profile pass-through | `email_cli.profile` is dead for normal operations (email-cli has no `--profile` flag). Upstream issue needed. | xs (doc only) |
| Doctrine tests | Every command under `--json` should emit exactly one JSON object. Every `agent-info` command string should match a real clap path. No `unwrap()` in non-test code. | s |
| Chaos/fault tests | kill -9 mid-send, SQLITE_BUSY, full disk, malformed email-cli responses, clock skew. | l |
| File permissions | config.toml and state.db should be 0600 on Unix. Directories 0700. | s |
| `invocation_id` propagation | Single correlation ID threaded through logs, locks, attempts, audit. | m |
| Audit log table | GDPR Article 30. Append-only `audit_log` table with pseudonymized PII. | l |
| db/mod.rs split | The refactor described above. | m |
| Streaming pipeline at 100k+ | Page recipients from DB instead of loading full Vec. Anti-join suppression in SQL. | l |

---

## v0.4.x polish items (smaller, still unshipped from the original v0.4 plan)

- `--format csv` flag on report commands (P0.5)
- Subject-line spam preview — local rule-based 0-10 score (P0.3)
- `contact sunset --inactive-since 90d --tag dormant --confirm` (P1.5)
- `bounce show <broadcast_id>` (P1.3)
- Affiliate link auto-wrapping from config.toml `[[affiliates]]` (MON-4)
- Surface `revenue_attributed_cents` in `report show` (MON-5)
- README full update (P0.1 — still partially stale per GPT Pro)

These are all small (xs-s effort each) and can be batched into v0.4.1 or folded into the refactor release.

---

## Migration accounting

| Migration | Release |
|---|---|
| 0001_initial | pre-v0.2 |
| 0002_event_idempotency_and_kv | v0.2 |
| 0003_template_html_source | v0.2 (no-op) |
| 0004_broadcast_locks | v0.3.1 |
| 0005_broadcast_send_attempt | v0.3.2 |
| 0006_content_snapshots_and_revenue | v0.4.0 |

---

## Test count history

| Version | Tests |
|---|---|
| v0.0.3 | 30 |
| v0.1.0 | 135 |
| v0.2.0 | 158 |
| v0.3.0 | 177 |
| v0.3.1 | 189 |
| v0.3.2 | 196 |
| v0.3.3 | 196 |
| **v0.4.0** | **207** |

---

## Files to review first

1. **`src/db/mod.rs`** (3,286 lines) — the god file. This is where the refactor starts.
2. **`src/template/render.rs`** (1,004 lines) — the link rewriter + renderer + lint.
3. **`src/broadcast/pipeline.rs`** (928 lines) — the send loop + reconcile + preflight.
4. **This handoff** — the split plan above is the recommended approach.

---

## Entry point summary

> v0.4.0 is current stable on GitHub, crates.io, and Homebrew. The codebase is ~15K lines of Rust, 207 tests passing, 14 runtime deps. The user wants to refactor and split before adding more features. The db/mod.rs split (3,286 lines → 12 domain files) is the #1 priority. No behavior changes, no new deps, no new abstractions — just file moves. Ship as v0.4.1 (or v0.5.0 if you want a minor bump for the internal restructure). The GPT Pro hardening roadmap is in this handoff and the session-7 handoff for future feature planning.

---

*End of session 8 handoff.*
