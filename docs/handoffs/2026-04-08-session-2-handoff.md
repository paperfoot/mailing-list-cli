# Session Handoff — Session 2

**Date:** 2026-04-08 (afternoon, ~12:45 BST)
**Session:** Phases 3-6 implementation: shipped v0.0.4 → v0.0.5 hotfix → v0.1.0 → v0.1.1, started Phase 6 (v0.1.2)
**Context usage at handoff:** ~85%

---

## TL;DR for the next session

We executed FOUR phases of the parity plan in one long session:
- **v0.0.4** Phase 3 (contacts, tags, fields, segments, filter parser, CSV import) — shipped
- **v0.0.5** Hotfix release fixing 5 critical Phase 3 bugs caught by parallel Codex code review — shipped
- **v0.1.0** Phase 4 (MJML templates, compile pipeline, lint, agent authoring guide) — shipped
- **v0.1.1** Phase 5 (broadcasts, send pipeline, HMAC unsubscribe tokens) — shipped + **validated end-to-end against real Resend domain `paperfoot.com`**
- **v0.1.2** Phase 6 (webhooks + reports) — **partially done**, Tasks 1-6 of 10 committed

**At v0.1.1 four of five user asks are shipped.** The fifth (bounce rate / click tracking / unsubscribe count per batch) lands in v0.1.2 Phase 6 Tasks 7-10. You're picking up there.

---

## Active Plans

- **Phase 6 (THIS is what to finish):** [`docs/plans/2026-04-08-phase-6-webhooks-reports.md`](../plans/2026-04-08-phase-6-webhooks-reports.md) — 2025 lines, 10 tasks. Tasks 1-6 done; Tasks 7-10 remaining.
- **Phase 5 (just shipped, reference only):** [`docs/plans/2026-04-08-phase-5-broadcasts.md`](../plans/2026-04-08-phase-5-broadcasts.md)
- **Phase 4 (shipped, reference only):** [`docs/plans/2026-04-08-phase-4-templates.md`](../plans/2026-04-08-phase-4-templates.md)
- **Phase 3 (shipped, reference only):** [`docs/plans/2026-04-08-phase-3-contacts-tags-fields-segments.md`](../plans/2026-04-08-phase-3-contacts-tags-fields-segments.md)
- **Parity plan (strategic context):** [`docs/plans/2026-04-08-parity-plan.md`](../plans/2026-04-08-parity-plan.md)
- **Design spec (authoritative):** [`docs/specs/2026-04-07-mailing-list-cli-design.md`](../specs/2026-04-07-mailing-list-cli-design.md)
- **Prior session 1 handoff:** [`docs/handoffs/2026-04-08-session-handoff.md`](./2026-04-08-session-handoff.md)

---

## Phase 6 status — exact task accounting

| Task | Title | Status | Commit |
|---|---|---|---|
| 1 | Migration 0002 — event idempotency index + kv table | ✅ done | `3b95506` |
| 2 | Dependencies (tiny_http, subtle) + ResendEvent types module | ✅ done | `ff14e73` |
| 3 | Event handler dispatch + DB helpers (event/click/suppression/soft_bounce/kv) | ✅ done | `bca25f7` (DB) + `5637763` (dispatch) |
| 4 | `event poll` — ingest via `email-cli email list` | ✅ done | `45df6d4` |
| 5 | Svix HMAC verifier + `tiny_http` listener | ✅ done | `b016f59` |
| 6 | CLI dispatch for `webhook listen/poll/test` + `report` (stub) | ✅ done | `8bd592f` |
| **7** | **Report DB aggregations** (`report_summary`, `report_links`, `report_deliverability`) | **PENDING** | — |
| **8** | **`report` CLI** — replace the stub in `src/commands/report.rs` with real handlers | **PENDING** | — |
| **9** | **Integration tests** for poll + report | **PENDING** | — |
| **10** | **agent-info update + version bump to 0.1.2 + tag** | **PENDING** | — |

The stub `src/commands/report.rs` returns `BadInput { code: "report_not_implemented" }` for every report subcommand. Task 8 replaces the stub.

---

## What was accomplished this session

### v0.0.4 — Phase 3 ships
- Filter expression language: pest grammar + AST + SQL compiler (`src/segment/`)
- Tag CRUD + contact tag/untag
- Field CRUD with snake_case validation + select options
- Typed `contact set` + `contact add --field key=val ...`
- `contact show` (full details with tags/fields/list memberships)
- `segment create/ls/show/members/rm --confirm`
- `contact ls --filter --list --limit --cursor` + parity test with `segment members`
- CSV import with consent enforcement, idempotent replay, rate limiting (200ms throttle)
- Email-cli duplicate-contact path fix (calls `segment contact-add` on dup)
- 26 commits, ~71 new tests

### v0.0.5 — Hotfix (5 critical bugs found by Codex code review)
1. **Custom field filter typing** — compiler was picking column by literal value shape; now resolves field type from DB
2. **CSV import atomicity** — wrapped per-row writes in a transaction with pre-validation
3. **Consent persistence** — `consent_source` is now actually written to the contact table; unsafe-no-consent preserves prior consent
4. **NOT NOT grammar** — `not_expr = { not_op* ~ term }` allows repeated NOT
5. **Date type accepts plain `YYYY-MM-DD`** in addition to RFC 3339

### v0.1.0 — Phase 4 ships
- MJML template subsystem via `mrml 5.1` (pure Rust, no Node)
- Frontmatter parser (manual split + `serde_yaml`, NOT `gray_matter`)
- Compile pipeline: Handlebars → mrml → css-inline → html2text
- Lint module with 20 rules (forbidden tags, triple-brace allowlist, dangerous CSS, size thresholds, etc.)
- `template create/ls/show/render/lint/edit/rm/guidelines` CLI
- `template edit` is the only interactive command (TTY guard, $EDITOR/$VISUAL required, no shell)
- Embedded authoring guide compiled in via `include_str!("../../assets/template-authoring.md")`
- Macro recursion limit raised to 512 in `src/main.rs` to fit the agent-info manifest

### Lint hotfix (between Phase 4 ship and Phase 5)
- **`{{else}}`/`{{if}}` false positive** — the merge-tag extractor was treating `else` as an undeclared variable
- **Found by blind template authoring test:** Codex + Gemini + Claude each authored a welcome template using only `template guidelines`. Both Codex and Gemini hit the `{{else}}` trap (they followed the guide's example precisely). Claude's template passed only because it didn't use `{{else}}`.
- Fix in commit `f643656`: added `HANDLEBARS_KEYWORDS` skip-list (`else`, `if`, `unless`, `each`, `with`, `this`)
- After fix, **all 3 agents pass the lint** (verified manually)

### v0.1.1 — Phase 5 ships
- Broadcast model + DB helpers
- HMAC-SHA256 unsubscribe token signer (RFC 8058 compatible)
- JSON batch file writer
- Full send pipeline: pre-flight invariants → suppression filter → per-recipient render (handlebars + mrml + css-inline + html2text) → physical address footer injection → List-Unsubscribe header → JSON batch → `email-cli batch send`
- `broadcast create/preview/schedule/send/cancel/ls/show` CLI
- 9 commits, 12 new tests
- **Subagent crashed mid-Task 9 (API quota)** — finished Task 9 (version bump + agent-info) manually in main session

### v0.1.1 real-Resend smoke test (POST-RELEASE bug found)
The real-Resend smoke test against `paperfoot.com` caught **two bugs the stub couldn't catch**:

1. **`email-cli send` arg shape** — wrapper was passing `--account <profile> --from <email>` but real email-cli has `--from` as an alias for `--account` (so they're the same field), and `--account` takes the SENDER ACCOUNT EMAIL not the profile name. Fix: pass `--account <from-email>` only.

2. **`email-cli batch send` response shape** — wrapper expected `data[]` but real email-cli returns `data.data[]` with items containing only `id` (no `to`). Fix: support both shapes; correlate items with input recipients by index.

3. **Error parsing** — real email-cli returns errors as JSON in stdout with non-zero exit. Wrapper was reading stderr only. Fix: try parsing stdout JSON for `error.message`.

4. **`send` response `id` field** — real returns `data.id` as integer (local DB id) and `data.remote_id` as Resend UUID string. Wrapper expected `data.id` as string. Fix: prefer `remote_id`, fall back to `id` as string OR number.

All committed in `2c61152 fix(email_cli): real Resend response shapes for send + batch_send`. **After the fix, the smoke test PASSES**: real broadcast preview + real broadcast send both deliver real emails to `smoke-test-v0.1.1@paperfoot.com`.

### v0.1.2 — Phase 6 partial (Tasks 1-6 done)
- Migration 0002 (event dedup index + kv cursor table)
- ResendEvent types module
- Event handler dispatch with auto-suppression
- `event poll` via `email-cli email list`
- Svix HMAC signature verifier + `tiny_http` listener
- CLI definitions for `webhook listen/poll/test` and `report show/links/engagement/deliverability`
- `commands/webhook.rs` with full implementations
- `commands/report.rs` is a STUB returning `report_not_implemented`

**Subagent stopped mid-Task 3 due to API quota issues (twice). Finished Tasks 3-6 in main session.**

---

## Key decisions made (NOT in code or plan)

1. **Codex + Gemini parallel reviews** before each phase plan — both gave critical feedback that was folded into the plan before execution. Worth doing for Phase 6 Tasks 7-10 too.

2. **Real-Resend smoke testing is non-negotiable** — the stub email-cli script masked TWO real bugs (response shapes for `send` and `batch_send`) that only showed up against real Resend. Phase 6 should have a real smoke test against `paperfoot.com` once `report show` works.

3. **Subagent API quota is unstable** — the user hit "out of usage" twice (once in Phase 5, once in Phase 6). Reset is at 6am London time. Main session has its own budget — if quota dies again, do work in main session directly.

4. **`mrml 5.1` quirks**:
   - `mrml::parse(src)?.element.render(&RenderOptions::default())?` (note `.element` accessor, not direct `.render()` on the parse result)
   - Unknown tags are SILENTLY DROPPED, not errors. The `rejects_invalid_mjml` test uses unclosed `<mj-button>` instead.
   - Some MJML attributes that JS MJML accepts (e.g. `border-radius` on `mj-section`) are dropped by mrml. The lint can't catch all of these.

5. **`css-inline` `--default-features=false`** with `stylesheet-cache` only — the `http` feature would let it fetch remote CSS which we don't want. Without disabling, the inliner errors on `Loading external URLs requires the http feature`.

6. **Frontmatter via manual split + `serde_yaml`, NOT `gray_matter`** — Codex review pushed back on `gray_matter` as unnecessary abstraction. The hand-rolled `split_frontmatter` in `src/template/frontmatter.rs` is ~50 lines and gives deterministic errors with line numbers.

7. **Triple-brace allowlist** — only `{{{ unsubscribe_link }}}` and `{{{ physical_address_footer }}}` allowed; everything else is a lint ERROR (XSS prevention). The `extract_triple_brace_names` walker enforces this.

8. **Handlebars strict mode** when not substituting placeholders, lenient when in `compile_with_placeholders`. Critical for catching missing required vars at render time.

9. **`compile()` vs `compile_with_placeholders()`** — the former leaves `{{{ unsubscribe_link }}}` literal (send-time substitution); the latter substitutes stub values for preview. Phase 5's broadcast pipeline uses the placeholder version with REAL values injected via the merge data dict (not via the stubs).

10. **The user's 3 verified Resend domains:** `paperfoot.com` (us-east-1, sending+receiving), `livebeyond.dev` (eu-west-1, sending only), `healtrix.clinic` (eu-west-1, both). The `email-cli` profile is `local`. The default sending account is `boris@paperfoot.com`.

11. **Recursion limit 512** in `src/main.rs` is required because `serde_json::json!` macro hits the default 128 limit when expanding the agent-info manifest with 30+ command entries. Don't lower it.

12. **`tempfile` was moved from dev-dep to main dep** in Phase 4 because `template edit` needs it. Don't put it back in dev-dependencies.

13. **`paths::tests` race** — there's a pre-existing `cargo test` race when running parallel. Always use `cargo test -- --test-threads=1` for verification. Not Phase 6 scope to fix.

---

## Current state

- **Branch:** `main`
- **Last commit:** `8bd592f feat(webhook+report): wire CLI dispatch for phase 6`
- **Tags pushed:** `v0.0.1` through `v0.1.1` (latest tag is v0.1.1)
- **Uncommitted changes:** none (working tree clean)
- **Tests passing:** yes — `cargo test -- --test-threads=1` reports **105 unit + 52 integration = 157 passing**
- **Build status:** clean (`cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` all clean)
- **CI status:** v0.1.1 CI was triggered earlier (`gh run list --repo paperfoot/mailing-list-cli --limit 1`); check that it went green
- **Pushed to remote:** yes, all commits and tags pushed

---

## What to do next

### Step 0: Acclimate (5 min)

```bash
cd /Users/biobook/Projects/mailing-list-cli
git log --oneline -10
git status
cargo test -- --test-threads=1 2>&1 | grep "test result"
gh run list --repo paperfoot/mailing-list-cli --limit 3
```

You should see 157 tests green and the v0.1.1 CI run as completed.

### Step 1: Read this handoff and the Phase 6 plan

```bash
# This handoff:
docs/handoffs/2026-04-08-session-2-handoff.md   # ← you are here

# The plan to execute:
docs/plans/2026-04-08-phase-6-webhooks-reports.md
```

The plan is 2025 lines with verbatim code for every task. Tasks 1-6 are already done. **Start at Task 7.**

### Step 2: Execute Phase 6 Task 7 — Report DB aggregations

Open `docs/plans/2026-04-08-phase-6-webhooks-reports.md` and find Task 7. It adds these methods to `impl Db` in `src/db/mod.rs`:

- `report_summary(broadcast_id) -> ReportSummary` — division-by-zero-safe percentage math
- `report_links(broadcast_id) -> Vec<LinkReport>` — `GROUP BY link` aggregation over the `click` table
- `report_deliverability(window_days) -> DeliverabilityReport` — rolling window over the `broadcast` table

The model structs (`ReportSummary`, `LinkReport`, `DeliverabilityReport`) **already exist** in `src/models.rs` with `#[allow(dead_code)]` — Task 7 wires them up so the allow can be removed.

The plan also says to create `src/report/mod.rs` and `src/report/engagement.rs`. **Skip those** — the report module isn't strictly needed; the DB helpers + the existing `src/commands/report.rs` stub are sufficient. The plan was slightly over-scoped.

### Step 3: Execute Phase 6 Task 8 — Replace the report.rs stub

Replace the body of `src/commands/report.rs::run` with the real handlers from the plan. Each handler:
- Opens the DB
- Calls the relevant `Db::report_*` method
- Outputs a JSON envelope via `crate::output::success`

The CLI args (`ReportShowArgs`, `ReportLinksArgs`, etc.) are already defined in `src/cli.rs`. Don't duplicate them.

For `report engagement`, the plan does a naive aggregation directly in the handler (not via a Db method) — that's fine. Use the `chrono::Utc::now() - chrono::Duration::days(args.days)` pattern to compute the cutoff, then `db.conn.query_row` for the count.

### Step 4: Execute Phase 6 Task 9 — Integration tests

Add to `tests/cli.rs`:
- `event_poll_ingests_delivered_status_and_report_shows_it` — uses `MLC_STUB_EMAIL_LIST_JSON` env var to feed a synthetic delivered event into the stub
- `report_show_for_nonexistent_broadcast_fails_with_exit_3`
- `webhook_test_requires_running_listener_or_fails` (uses port 1, guaranteed-closed)

Plus probably:
- `report_show_returns_ctr_bounce_rate_after_synthetic_events` — seed a broadcast, inject events directly via `Db::event_insert`, verify `report show` reports correct percentages

### Step 5: Execute Phase 6 Task 10 — Tag v0.1.2

```bash
# Update agent-info status string to "v0.1.2 — webhook ingestion + reports"
# Add the new commands to the manifest:
#   webhook listen [--bind <addr>]
#   webhook poll [--reset]
#   webhook test --to <url> --event <type>
#   event poll [--reset]
#   report show <broadcast-id>
#   report links <broadcast-id>
#   report engagement [--list <name>|--segment <name>] [--days N]
#   report deliverability [--days N]

# Bump Cargo.toml: version = "0.1.2"
# Update README badge: v0.1.1 → v0.1.2

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1 2>&1 | grep "test result"

git add Cargo.toml Cargo.lock src/commands/agent_info.rs tests/cli.rs README.md
git commit -m "chore: bump to v0.1.2 — phase 6 webhooks + reports"
git push origin main
git tag -a v0.1.2 -m "v0.1.2 — webhook ingestion + reports; ships bounce/click/unsubscribe stats"
git push origin v0.1.2
gh run list --repo paperfoot/mailing-list-cli --limit 1
```

### Step 6 (post-tag): Real-Resend smoke test for v0.1.2

The user explicitly asked for real-Resend testing. After v0.1.2 ships, do a smoke test:

```bash
# Reuse the v0.1.1 smoke test setup at /tmp/mlc-smoke-v0.1.1/
# (or create a fresh /tmp/mlc-smoke-v0.1.2/)
export MLC_CONFIG_PATH=/tmp/mlc-smoke-v0.1.2/config.toml
export MLC_DB_PATH=/tmp/mlc-smoke-v0.1.2/state.db
export MLC_UNSUBSCRIBE_SECRET=smoke-secret-at-least-16-bytes
MLC=./target/release/mailing-list-cli

# 1. Send a test broadcast (re-use v0.1.1 procedure)
# 2. Wait 30s for Resend to deliver and webhook events to fire
# 3. Run event poll to ingest the events
$MLC --json event poll
# 4. Check the report
$MLC --json report show 1
# Expected: delivered_count >= 1, ctr / bounce_rate / etc populated
```

If the user's Resend account has a webhook configured pointing to a public URL, you can also test the listener path. Otherwise, polling is sufficient.

### Step 7: Re-run the blind template authoring test (final validation)

The original blind test (Phase 4 era) ran against the BUGGY lint and showed 1/3 pass. After the `f643656` lint fix, manual verification showed 3/3 pass. **Re-run a clean blind test** to produce a final report:

```bash
# The blind test files are still at /tmp/mlc-blind-test/
# Re-run with the current build:
./target/release/mailing-list-cli --json template guidelines | jq -r '.data.guide_markdown' > /tmp/template-guide.md

# Then dispatch fresh prompts to Codex + Gemini + Claude (yourself) and lint the outputs.
# See the original blind test prompt at /tmp/mlc-blind-test/full-prompt.txt
```

Document the final 3/3 pass result somewhere in the docs (maybe a `docs/blind-test-results-v0.1.x.md`).

---

## Files to review first

1. **[`docs/plans/2026-04-08-phase-6-webhooks-reports.md`](../plans/2026-04-08-phase-6-webhooks-reports.md)** — the plan you're executing
2. **[`src/commands/report.rs`](../../src/commands/report.rs)** — the stub you need to replace in Task 8
3. **[`src/db/mod.rs`](../../src/db/mod.rs)** — find the end of `impl Db` (around line 1450+) to append the report helpers in Task 7
4. **[`src/webhook/dispatch.rs`](../../src/webhook/dispatch.rs)** — to understand how events update broadcast stats (so the reports actually have data to read)
5. **[`src/models.rs`](../../src/models.rs)** — `ReportSummary`, `LinkReport`, `DeliverabilityReport` are defined here

---

## Gotchas & warnings

- **Don't use parallel `cargo test`** — there's a pre-existing race in `paths::tests` that bites at parallel default. Always `cargo test -- --test-threads=1`.

- **Don't lower the macro recursion limit** in `src/main.rs`. The `serde_json::json!` macro hits the default 128 limit because the agent-info manifest has 30+ entries. The current limit is 512.

- **Don't try to use `mrml::parse(src)?.render(...)`** — that's what the plan originally said but it's wrong for mrml 5.1. The correct invocation is `mrml::parse(src)?.element.render(&RenderOptions::default())?`.

- **Don't add `gray_matter`** as a dep. Codex review pushed back hard on it. The hand-rolled `split_frontmatter` in `src/template/frontmatter.rs` is the canonical implementation.

- **Don't forget the recursion limit when adding manifest entries**. Phase 6 Task 10 adds 8 more commands to agent-info. If you hit a recursion error, raise the limit to 1024.

- **Don't try to use the `email-cli send` `--from` flag** — it's an alias for `--account` not a separate field. The correct invocation is `--account <sender-email>` only.

- **Don't trust the stub for `email-cli batch send` shape** — the stub returns `data[]` but the real returns `data.data[]`. The wrapper supports BOTH but when adding new tests, double-check against real Resend output.

- **The `subagent quota` issue** — the user's "extra usage" budget for subagents is fragile. If you dispatch a subagent and it dies with "out of usage", do the work in the main session instead. Resets at 6am London time.

- **The `paperfoot.com` smoke test database** is at `/tmp/mlc-smoke-v0.1.1/state.db` with 1 broadcast already in `sent` status. You can reuse it for v0.1.2 reporting tests OR start fresh.

- **The blind test artifacts** are at `/tmp/mlc-blind-test/` — preserved for re-running the test against the current code.

- **Always run `cargo fmt` before committing** — the rustfmt rules are stricter than what manual editing produces (especially long type signatures and `.ok_or_else(|| ...)` chains).

- **The `webhook listen` command is long-running** and has no automated test. Test it manually only if needed: `mailing-list-cli webhook listen --bind 127.0.0.1:8081` then POST a synthetic event in another terminal.

- **`subtle::ConstantTimeEq`** is the import for HMAC verification (in `src/webhook/signature.rs`). It's already wired.

- **The `cli.rs` file is now ~525 lines** with all the Webhook/Event/Report enum + args. Don't try to refactor it into separate files mid-Phase 6 — that's a Phase 9 polish task.

---

## Test count history

| Version | Tests passing |
|---|---|
| v0.0.3 baseline (start of session) | 30 |
| v0.0.4 Phase 3 ship | 101 |
| v0.0.5 hotfix | 110 |
| v0.1.0 Phase 4 ship | 135 |
| v0.1.0 + lint hotfix | 136 |
| v0.1.1 Phase 5 ship | 148 |
| v0.1.1 + email_cli fix | 148 |
| v0.1.1 + Phase 6 Tasks 1-6 (current) | **157** |
| v0.1.2 target after Tasks 7-9 | ~166-170 |

---

## Session entry point for the next run

> Read `docs/handoffs/2026-04-08-session-2-handoff.md`. Then read `docs/plans/2026-04-08-phase-6-webhooks-reports.md` Tasks 7-10. Execute Task 7 (report DB aggregations in `src/db/mod.rs`), Task 8 (replace the stub in `src/commands/report.rs`), Task 9 (integration tests in `tests/cli.rs`), and Task 10 (bump to v0.1.2 + tag + push). Then run a real-Resend smoke test against paperfoot.com to verify `report show` returns real CTR/bounce_rate values after `event poll`. Finally re-run the blind template authoring test to get a 3/3 pass result on the current code.

---

*End of handoff.*
