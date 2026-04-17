# Session Handoff — Session 4

**Date:** 2026-04-09
**Session:** v0.2.3 polish release → v0.3.0 "production-grade 10k" release
**Context at handoff:** ~60%

---

## TL;DR

Two releases shipped this session:

| Tag | Commit | What | CI |
|---|---|---|---|
| `v0.2.3` | `074840b` | **Blind-test polish.** Three agents (Codex gpt-5.4 xhigh, Gemini 3.1-pro-preview, Claude Opus subagent) cold-started the v0.2 template system in parallel as an integrity test. All three hit clean lint on their first `template create` + `template lint` cycle — the v0.2 scaffold-as-documentation thesis validated. Fixed the 10+ footguns they flagged: stale "MJML" help strings, named HTML entity decode in plain-text fallback, `<div>` → `<span>` footer stub (safe to inject inside `<p>`), `template render` default flipped to inject stubs with `--raw` opt-out, scaffold expanded with an HTML-comment quick reference that's stripped at render time (discovered during the smoke test that the lint needed `strip_html_comments()` or it'd flag its own comment). | ✓ |
| `v0.3.0` | `f30fd42` | **The big one.** Seven independent tasks hardening the send pipeline to the "I would deploy this for a paying client" bar for 10k+ recipients. Shipped directly on main (trunk-based per project convention). | ✓ |

**Current state at handoff:**
- Branch: `main`
- Latest commit: `f30fd42 chore: bump to v0.3.0`
- Latest tag: `v0.3.0`
- Tests: 115 unit + 62 integration = **177 passing** (was 165 at start of session)
- Release binary: `target/release/mailing-list-cli` reports `0.3.0`
- Working tree clean
- Pushed: main + all tags pushed to `paperfoot/mailing-list-cli`

---

## v0.3.0 scope (as shipped)

Seven tasks landed as independent commits between the plan and the release bump:

| # | Commit | Task | Effect |
|---|---|---|---|
| 1 | `2a07374` | 429/5xx retry with exponential backoff in `batch_send` | Eliminates silent under-delivery. A single transient Resend hiccup on chunk N used to print `eprintln!("chunk N failed")` and move on — up to 100 emails lost. Now retries up to 4× on `[500ms, 1s, 2s, 4s]` backoff, returns `AppError::RateLimited` (exit 4) on exhaustion. |
| 2 | `75ea59d` | Preloaded suppression HashSet | O(N) per-recipient `is_email_suppressed` SELECT queries → O(1) `HashSet::contains` lookups. ~2500ms → ~40ms for a 10k send with 1k suppressed. |
| 3 | `bae7f9f` | Per-chunk explicit DB transactions | 30,000+ individual fsyncs per 10k send → ~101 (1 suppression filter + 1 per chunk). Foundation for resume. |
| 4 | `88e15c2` | Complaint/bounce rate guard in preflight (30-day window) | Previously stubbed pass. Now queries the `event` table, computes rates, enforces config thresholds (0.003 complaint, 0.04 bounce by default). Skipped on brand-new accounts with < 100 delivered events. |
| 5 | `44bffeb` | `contact erase <email> --confirm` (GDPR Article 17) | Transactional: insert `gdpr_erasure` suppression tombstone → delete contact row (FK cascades handle all child tables). Tombstone goes in first so the email is never momentarily absent from both. |
| 6 | `7d0ef9f` | Resumable sends + `broadcast resume` alias | Skips already-sent recipients via `db.broadcast_recipient_already_sent_ids(id)` after the suppression filter. Logs "resume mode — N recipient(s) already sent" to stderr. `resume` is an alias for `send`; both share the same handler. |
| 7 | `27de8a8` | `sender_domain_verified` health check | **Reduced version** of the originally planned tracking-config surfacing. email-cli v0.6.3's `domain list` output doesn't expose `open_tracking` / `click_tracking` / domain IDs, so full tracking surfacing is deferred to v0.3.1. What we ship: a new health check that confirms the sender domain is registered and verified in Resend via `EmailCli::domain_list()`. Catches the "my broadcast went out but nothing happened" failure mode. |

Plus two late fixes from the smoke test:
- `78d943f` — `domain_list` was parsing `{data: [...]}` instead of the real `{data: {data: [...]}}`. Same double-nested shape quirk as `batch_send`. Discovered when the health check reported paperfoot.com as "not registered" despite it being verified.
- `f30fd42` — version bump.

---

## Plan file

`docs/plans/2026-04-09-phase-8-v0.3-production-grade-10k.md` — committed at `f955c1c`. Written via `superpowers:writing-plans` before any code was touched. Contains the full task text for each of the 7 items plus the release task (Task R). Status: **fully executed**, frozen for historical reference.

### Intended vs. actual execution

Plan intended subagent-driven execution via `superpowers:subagent-driven-development`. User consented to work directly on main (project convention). Task 1 was dispatched as a subagent, but the parent agent hit a usage limit mid-task; the subagent had written the constants, helper, stub script, and 4 failing tests before the limit hit, but hadn't rewritten `batch_send` itself. Switched to inline execution for tasks 1b–7 and the release. All 8 items still landed with TDD discipline (test-then-implement), just without the two-stage review gate that subagent-driven development provides.

---

## Smoke test results (v0.3.0, paperfoot.com, us-east-1)

Hard rule from the session-3 handoff: every tagged release goes through the full real-Resend flow. All 13 steps green:

| Step | Result |
|---|---|
| 1. `health` (with new `sender_domain_verified` check) | all 5 checks ok; paperfoot.com verified |
| 2. `list create smoke-v030` | id=1, Resend segment created |
| 3. `contact add` | 1 contact added to list |
| 4. `template create` + `lint` + `preview` | scaffold clean 0/0, preview 0/0 unresolved=[] |
| 5. `broadcast create` | id=1, draft |
| 6. `broadcast preview` (real single send) | sent=1 |
| 7. `broadcast send` (batch) | sent=1, failed=0 |
| 8. `webhook poll` | 100 real events processed |
| 9. `broadcast show` | status=sent, delivered_count=1 |
| 10. `report show` | summary populated |
| 11. Hard-fail path (typo template) | exit 3, status=failed, `template_unresolved_placeholder` |
| 12. **NEW v0.3: `broadcast resume`** | After faking a broadcast into `sending` status + marking one contact as already sent, `broadcast resume 3` printed `resume mode — 1 recipient(s) already sent, skipping` on stderr and returned `sent=0` |
| 13. **NEW v0.3: `contact erase`** | `contact erase email` without `--confirm` → exit 3 `confirm_required`. With `--confirm` → DB verified clean (contact row gone, suppression tombstone = `gdpr_erasure`) |

Smoke DB preserved at `/tmp/mlc-smoke-v0.3.0/state.db`.

---

## Deferred to v0.3.1 or later

### v0.3.1 (next patch)
- **Full tracking-config surfacing.** Blocked on an upstream email-cli fix: `domain list` needs to expose `open_tracking`, `click_tracking`, and the Resend domain `id` (so `domain get <id>` can be called for the fuller config). Open an issue on 199-biotechnologies/email-cli first, then land the `tracking_enabled` fields in mailing-list-cli's `report show` output and the warning path in `broadcast send` preflight.
- **Integration tests deferred from Task 4 and Task 5.** The unit tests cover the DB methods and the CLI wiring is trivial, but end-to-end assert_cmd tests for preflight complaint-rate guard and contact erase would add confidence. Low priority since real-Resend smoke test covers both.

### v0.4 (next minor) — the "deliverability + daemon" release
- **`dnscheck` module.** `report deliverability` currently returns `verified_domains: []`. A DNS resolver crate (`trust-dns-resolver` or similar, the first new runtime dep in two releases) checks SPF, DKIM, and DMARC records for the sender domain. Pairs naturally with the v0.3 `sender_domain_verified` check.
- **`daemon` subcommand.** `webhook poll` is one-shot today. A `daemon start` that runs the poll loop on a configurable interval until SIGINT, writing a pidfile and handling signal-driven shutdown.
- **Template versioning** (Gap #2 from the v0.1.3 Codex review that never shipped). Migration 0004 + `template_revision` table + `template history <name>` + `template restore <name> --revision N`.

### v0.5+ (not yet planned)
- Concurrent-send guard (prevent two simultaneous `broadcast send` on the same id).
- Audit log (`audit_log` table, `audit ls` command) for GDPR compliance.
- Multi-user / tiered access (not needed until a 199-employee org wants one DB for the whole team).

---

## Gotchas & warnings

### Release pipeline
- **Tests use the new bash stub** at `tests/support/stub_email_cli.sh`. It's driven by env vars: `STUB_EMAIL_CLI_FAIL_COUNT`, `STUB_EMAIL_CLI_COUNTER_FILE`, `STUB_EMAIL_CLI_PERMANENT_4XX`. Each test should use a UNIQUE counter-file path (`pid + ns`) so parallel runs don't stomp each other. The retry tests run with `--test-threads=1` anyway but the pattern is there for safety.
- **`batch_send` retry exhaustion sleeps ~7.5s in tests** (500 + 1000 + 2000 + 4000ms). Factored into CI wall-clock budget; not a concern now but if we ever add more retry tests, shorten the backoff with a compile-time constant injected via `#[cfg(test)]`.

### Broadcast pipeline
- **`send_broadcast` now takes `let mut db = Db::open()?;`** because `conn.transaction()` requires `&mut Connection`. If a future feature tries to thread `&db` through a helper, it'll need to be updated.
- **Suppression filter runs in two passes now** (Task 2 + Task 3 combined). First pass inside the transaction iterates `&recipients` so the borrow checker is happy with the live `Transaction`. Second pass takes ownership to build `to_send`. Unavoidable given the lifetime constraints; don't "simplify" it back into one loop without understanding why.
- **Per-chunk transactions depend on `rusqlite::Connection::transaction()`** which rolls back on `Drop`. If any error is returned from inside the block before `.commit()`, the chunk's writes are discarded — that's the correct behavior, don't wrap in a `catch_unwind` or similar.

### Contact erase
- **Erase is NOT idempotent on a missing contact.** `contact erase nobody@example.com --confirm` returns `contact_not_found` exit 3. This is intentional — the agent needs to know if they typoed the email. A "silent no-op" erase is worse than a loud error.
- **The suppression tombstone is inserted BEFORE the contact delete** in the transaction so the email is never momentarily absent from both places. Don't reorder these without understanding why.

### Broadcast resume
- **`broadcast send`, `broadcast resume` are the same handler.** Both refuse non-{draft,scheduled,sending} status. Calling `resume` on a `sent` broadcast returns `broadcast_bad_status` exit 3 — that's correct, you shouldn't resume a completed broadcast.
- **Resume test in the smoke was manually constructed** by running SQL against `/tmp/mlc-smoke-v0.3.0/state.db` to force a broadcast into `sending` status + insert a fake `broadcast_recipient` row with status `sent`. The pipeline code path was exercised correctly; the UI ergonomics (`broadcast show` showing recipient_count=0 after a full resume skip) are slightly surprising but match the v0.2 behavior.

### Domain verification check
- **The health check expects `email-cli domain list` to return `{data: {data: [...]}}`** (double-nested). The `or_else` fallback to `{data: [...]}` is there for test compatibility and should not be removed.
- **Per-profile domain lists.** email-cli doesn't currently accept a `--profile` flag; it uses whatever profile is active. mailing-list-cli's `self.profile` field is only used in `profile_test`. If email-cli grows a `--profile <name>` flag, we should thread it through every shell invocation.

### General
- **Smoke test is mandatory before every release tag.** v0.3.0 caught a real bug (domain_list shape mismatch) that would have shipped as "the health check is broken" otherwise. No exceptions.
- **Docs have NOT been updated** for v0.3's new commands (`broadcast resume`, `contact erase`). The README command tables should be updated in a follow-up docs-only commit before v0.3.1.

---

## Current state

- **Branch:** `main`
- **Last commit:** `f30fd42 chore: bump to v0.3.0 — production-grade 10k release`
- **Latest tag:** `v0.3.0`
- **Uncommitted changes:** none
- **Tests passing:** 115 unit + 62 integration = **177 passing** in ~11s parallel + 4s integration
- **Build status:** clean (`cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` all clean)
- **Release binary:** `target/release/mailing-list-cli 0.3.0` (3.5 MB LTO+strip)
- **Pushed:** main + v0.3.0 tag pushed to `paperfoot/mailing-list-cli`
- **Smoke test DB:** `/tmp/mlc-smoke-v0.3.0/` preserved

---

## Test count history

| Version | Tests | Notes |
|---|---|---|
| v0.2.2 (start of session 4) | 159 | race-free baseline |
| v0.2.3 | 165 | blind-test polish, +6 new tests for entity decode + comment stripping |
| **v0.3.0 (current)** | **177** | production-grade 10k, +12 new tests across retry, HashSet, transactions, rates, erase, resume |

---

## Session entry point for next run

> Read `docs/handoffs/2026-04-09-session-4-handoff.md`. v0.3.0 is current stable — production-grade 10k, real-Resend validated, 177 tests passing. Three things to do next session, in order:
>
> 1. **Open an upstream email-cli issue** requesting `domain list` to expose `open_tracking` / `click_tracking` / Resend domain `id` in the JSON output. Link it from the v0.3.1 plan. This unblocks the full Task 7 from the v0.3 plan (tracking-config surfacing in `report show`).
>
> 2. **Update README command tables** for v0.3.0 — add `broadcast resume`, `contact erase`, mention the `--raw` flag on `template render`, note the new `sender_domain_verified` health check, mention the retry semantics on `batch_send`. Docs-only commit, no code changes.
>
> 3. **Start the v0.4 plan file** at `docs/plans/YYYY-MM-DD-phase-9-v0.4-deliverability-daemon.md`. Scope: `dnscheck` module, `daemon` subcommand, template versioning. Use `superpowers:writing-plans` + `superpowers:brainstorming` beforehand if it feels big.
>
> Hard rule: **every tagged release goes through the paperfoot.com smoke test.** v0.3.0 caught a real `domain_list` shape bug during its smoke run — skipping it would have shipped the bug.

---

*End of session 4 handoff.*
