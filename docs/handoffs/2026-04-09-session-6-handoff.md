# Session Handoff — Session 6 (v0.3.1 emergency hardening)

**Date:** 2026-04-09 (early morning, post-release)
**Session:** v0.3.1 emergency hardening — 4 outage-class fixes + crates.io first publish + Homebrew formula. Successor to `2026-04-09-session-5-planning-handoff.md` (which was research-only + v0.4 scope decision).

**Context usage at handoff:** roughly 75%

---

## TL;DR

**v0.3.1 shipped.** Four outage-class fixes confirmed by GPT Pro on 2026-04-09 landed as 4 sequential commits on main, plus the version bump. Tag pushed. **First-ever crates.io publish for `mailing-list-cli`** (the crate didn't exist before). **First-ever Homebrew formula** for the binary added to `199-biotechnologies/homebrew-tap`.

| Tag | Commit | What | CI |
|---|---|---|---|
| `v0.3.1` | `480e9b5` | Emergency hardening: lock CAS, subprocess timeout, schema version check, agent-info sync. Plus the 4 prep commits (spec, plan, then one per fix). | (in flight at handoff) |

**Current state at handoff:**
- Branch: `main`
- Latest commit: `480e9b5 chore: bump to v0.3.1 — emergency hardening release`
- Latest tag: `v0.3.1`
- Tests: 126 unit + 63 integration = **189 passing** (was 177 at start of session)
- Release binary: `target/release/mailing-list-cli` reports `0.3.1`
- Working tree clean
- Pushed: main + v0.3.1 tag pushed to `paperfoot/mailing-list-cli`
- crates.io: `https://crates.io/crates/mailing-list-cli` — **published v0.3.1** (first version on crates.io)
- Homebrew: `Formula/mailing-list-cli.rb` added to `199-biotechnologies/homebrew-tap` (commit `10824bd` in that repo)
- Smoke test DB preserved at `/tmp/mlc-smoke-v0.3.1/state.db`

---

## v0.3.1 scope (as shipped)

Four hardening fixes landed as independent commits between the spec and the version bump:

| # | Commit | Task | Effect |
|---|---|---|---|
| 1 | `45c8013` | Schema version safety check in `Db::open` | A v0.3.0 binary opening a state.db last touched by a v0.4.0 binary used to silently run SELECT queries against tables with new columns it didn't know about, producing confusing rusqlite errors. Now `Db::open` returns `AppError::Config { code: 'db_schema_too_new' }` (exit 2) when MAX(schema_version) is newer than the last known migration. ~25 lines + 2 tests. |
| 2 | `74a7285` | email-cli subprocess timeout | Every `Command::output()` call site in `email_cli.rs` was blocking with no timeout. A hung email-cli subprocess froze mailing-list-cli indefinitely. New `run_with_timeout` helper using `Child::try_wait()` in a 50ms poll loop. Default 120s, override via `MLC_EMAIL_CLI_TIMEOUT_SEC`. On timeout, kills the child via SIGKILL and returns `email_cli_timeout` transient error. Existing retry classifier already treats stderr containing "timeout" as retryable, so timed-out chunks feed back into batch_send's exponential backoff. All 9 call sites migrated. Closure-based spawn-error mapping preserves per-context error specificity. **Zero new dependencies.** +3 tests. |
| 3 | `df1bad2` | Broadcast lock CAS | Two simultaneous `mailing-list-cli broadcast send 1` invocations used to both flip draft→sending and double-send every recipient. Migration 0004 adds `broadcast.locked_by_pid` + `locked_at` columns. New `broadcast_try_acquire_send_lock(id, pid, stale_after, force_unlock)` does atomic CAS via UPDATE inside a `BEGIN IMMEDIATE` transaction. Predicate satisfied if status is draft/scheduled, OR same-PID resume, OR locked_at > 30 min stale, OR force_unlock. Pipeline calls acquire at start, fails fast with `broadcast_lock_held` (exit 1) on AlreadyHeld. Lock cleared in the final status transition via the new `broadcast_set_status_and_clear_lock`. New CLI flag: `broadcast send <id> --force-unlock` for the operator escape hatch. +6 tests. |
| 4 | `fc47414` | agent-info + AGENTS.md sync | agent-info was missing v0.3.0 commands (`contact erase`, `broadcast resume`) and the new `--force-unlock` + `--raw` flags. AGENTS.md still said v0.2.0. Manual rewrite of both files. Adds `tests/cli.rs::agent_info_includes_v0_3_1_surface` — a regression guard that asserts known-current command prefixes exist, version matches `CARGO_PKG_VERSION`, and `env_vars` contains `MLC_EMAIL_CLI_TIMEOUT_SEC`. Drift caught by `cargo test`. Spec was wrong about `--json` not being a real flag — it IS a real global flag (`#[arg(long, global = true)]` in cli.rs:11-13), kept in the manifest. +1 test. |

Plus the version bump:
- `480e9b5` — Cargo.toml 0.3.0 → 0.3.1, README badge, agent-info status string already set in fc47414.

---

## Plan & spec files

- **Spec**: `docs/specs/2026-04-09-v0.3.1-emergency-hardening.md` — committed at `3cb5512`. Written via the brainstorming skill before any code was touched. Status: **fully executed**, frozen for historical reference.
- **Plan**: `docs/plans/2026-04-09-phase-9-v0.3.1-emergency-hardening.md` — committed at `79b1dd6`. Written via the writing-plans skill. 5 tasks (4 implementation + release). Status: **fully executed**.

The plan itself has notes that need updating before someone uses it to learn from:
- The plan assumed tests in `tests/` could import internal modules. There's no `src/lib.rs`, so all unit tests live in `#[cfg(test)] mod tests` blocks inside source files. The plan steps still work — just mentally substitute "add to existing test mod" for "create new test file". Schema check tests + lock tests went into `src/db/mod.rs:1750+`. Timeout tests went into `src/email_cli.rs:639+`. The agent-info drift test is the only one in `tests/cli.rs`.
- The plan said `--json` was "false claim" in agent-info. It is **NOT a false claim** — `--json` IS a real global flag. The plan was based on a wrong assumption from the spec. The actual fix in Task 4 KEPT the `--json` entry and just fleshed out its description. The drift test does NOT assert that `--json` is absent — it asserts the v0.3.1 commands and env_vars exist.

## Smoke test results (v0.3.1, paperfoot.com, us-east-1)

13-step v0.3.0 regression flow plus 2 new v0.3.1-specific steps:

| Step | Result |
|---|---|
| 1. `health` (with `sender_domain_verified` + new schema check) | all 5 checks ok; paperfoot.com verified |
| 2. `list create smoke-v031` | id=1, Resend segment created |
| 3. `contact add smoke-test-v031@paperfoot.com` | 1 contact added |
| 4. `template create smoke_tpl_v031` + `template lint` | scaffold clean 0/0 |
| 5. `broadcast create --name smoke-bcast-v031 --template smoke_tpl_v031 --to list:smoke-v031` | id=1, draft |
| 6. `broadcast preview 1 --to smoke-test-v031@paperfoot.com` | sent=1 |
| 7. `broadcast send 1` | sent=1, failed=0 |
| 8. `broadcast show 1` | status=sent, sent_at populated, **lock columns NULL after completion** ✓ |
| **14a NEW** | Manually injected fake live lock (pid 88888) on a fresh broadcast → `broadcast send` exited with code **1** + JSON error code `broadcast_lock_held` ✓ |
| **14c NEW** | Same broadcast, `broadcast send N --force-unlock` → exit 0, sent=1 ✓ |
| **15 NEW** | Copied state.db, manually `INSERT INTO schema_version VALUES ('9999_imaginary_future', ...)`, ran `health` against it → exit code **2**, `db_schema_too_new` error message with the exact future version name ✓ |

Smoke DB preserved at `/tmp/mlc-smoke-v0.3.1/state.db`.

## crates.io publish

This was the **first publish for `mailing-list-cli`**. The crate did not exist before today. The publish:
1. `cargo publish --dry-run` ran clean (no warnings about missing fields)
2. `cargo publish` uploaded successfully — "Published mailing-list-cli v0.3.1 at registry crates-io"
3. Visible at `https://crates.io/crates/mailing-list-cli`

Future releases just need `cargo publish` — no extra setup.

## Homebrew formula

Created `Formula/mailing-list-cli.rb` in `199-biotechnologies/homebrew-tap` (commit `10824bd`). Same shape as `email-cli.rb` (the sister binary's formula): source-tarball-based build via `cargo install`. SHA256 of the v0.3.1 tag tarball: `36f4bd2f77f93f17e3986df52fe0a567982588c080cb916c3d991027ac18facf`.

Install command for users:
```bash
brew tap 199-biotechnologies/tap
brew install mailing-list-cli
```

For future releases, the formula update flow is:
1. Tag and push the new version
2. Compute SHA256: `curl -sL https://github.com/paperfoot/mailing-list-cli/archive/refs/tags/vX.Y.Z.tar.gz | shasum -a 256`
3. Update `url` and `sha256` in `Formula/mailing-list-cli.rb`
4. Commit and push to homebrew-tap

---

## Migration impact (for the v0.4 plan to absorb)

| Migration | Status |
|---|---|
| 0001_initial | shipped pre-v0.2 |
| 0002_event_idempotency_and_kv | shipped v0.2 |
| 0003_template_html_source | shipped v0.2 (no-op) |
| **0004_broadcast_locks** | **shipped v0.3.1 (this session)** — adds `broadcast.locked_by_pid INTEGER`, `broadcast.locked_at TEXT` |
| 0005_content_snapshots_and_revenue | **planned for v0.4** — was originally numbered 0004 in the session-5 handoff, must be renumbered |

**ACTION REQUIRED for the v0.4 plan author:** when writing the v0.4 plan, the content-snapshot + revenue migration MUST be `0005_content_snapshots_and_revenue`, NOT `0004_*`. The session-5 handoff used "0004" generically; v0.3.1 has now claimed that slot. Update the v0.4 plan accordingly.

## Test count history (carry-over)

| Version | Tests | Notes |
|---|---|---|
| v0.0.3 (session 1 start) | 30 | |
| v0.0.4 Phase 3 | 101 | |
| v0.1.0 Phase 4 | 135 | |
| v0.1.1 Phase 5 | 148 | |
| v0.1.2 Phase 6 | 167 | |
| v0.1.3 Codex gap fixes | 173 | |
| v0.2.0 rearchitecture | 158 | |
| v0.2.1 real-Resend + state fix | 158 | |
| v0.2.2 race + CI green | 159 | |
| v0.2.3 blind-test polish | 165 | |
| v0.3.0 production-grade 10k | 177 | |
| **v0.3.1 emergency hardening** | **189** | +12 (schema check 2, timeout 3, lock 6, drift 1) |
| v0.4.0 target | ~225 | +36 |

---

## What's still pending (handoff to next session)

### 1. The v0.4 plan is STILL not written
This was supposed to be job #1 of session 5, then session 6 detoured into emergency hardening. The v0.4 scope (16 items in 4 phases: Foundations / Monetization / Polish / Release) is decided in `2026-04-09-session-5-planning-handoff.md`. Use `superpowers:writing-plans` to draft `docs/plans/2026-04-09-phase-10-v0.4-operator-superpowers.md` (note: phase 10 now, since v0.3.1 took phase 9). **Migration 0005 = content_snapshots_and_revenue, NOT 0004.** Otherwise the scope is unchanged from session 5.

### 2. GPT Pro full hardening review is still in flight
A 31KB structured prompt + 144KB tar.gz package was prepared at `~/Documents/GPT Pro Analysis/mailing-list-cli-hardening-2026-04-09/` and copied to clipboard for the user to paste into ChatGPT Pro. The user has not yet returned the full GPT Pro report. When it comes back:
- Save it to `docs/handoffs/2026-04-09-gpt-pro-hardening-review.md` with header "Source: GPT-5 Pro, 2026-04-09"
- Compare findings against v0.4 scope and v0.3.1 fixes (this session already addressed 4 outage-class gaps the user pre-identified)
- Cherry-pick high-leverage findings into the v0.5+ roadmap

### 3. Two known-defer items from the spec
- **Auto-generation of agent-info from clap introspection.** v0.4 refactor candidate. Current state: hand-written + drift test catches missing entries.
- **Per-call timeout overrides** (vs the single global env var). v0.4+ if anyone asks. Current state: one knob, one default.

### 4. CI verification on the tag (in flight at handoff)
At handoff, the `v0.3.1` push had triggered a CI run that was still `in_progress`. Check with:
```bash
gh run list --branch v0.3.1 --limit 5
```
If green, no action needed. If it flakes, investigate immediately — the v0.2.1 CI flake was a real bug that required v0.2.2 to fix.

---

## Files modified this session

| File | Lines | Purpose |
|---|---|---|
| `docs/specs/2026-04-09-v0.3.1-emergency-hardening.md` | +493 | new — design spec |
| `docs/plans/2026-04-09-phase-9-v0.3.1-emergency-hardening.md` | +1258 | new — TDD plan |
| `src/db/mod.rs` | +200 | LockAcquireResult enum, broadcast_try_acquire_send_lock, broadcast_set_status_and_clear_lock, schema check in run_migrations, 8 unit tests |
| `src/db/migrations.rs` | +18 | migration 0004_broadcast_locks |
| `src/email_cli.rs` | +245 -118 | run_with_timeout helper, 9 call sites migrated, 3 unit tests |
| `src/broadcast/pipeline.rs` | +44 -8 | acquire lock at start, BrokeStale logging, AlreadyHeld error path, broadcast_set_status_and_clear_lock at completion + failure paths |
| `src/cli.rs` | +7 | --force-unlock flag on BroadcastSendArgs |
| `src/commands/broadcast.rs` | +1 -1 | pass force_unlock to pipeline |
| `src/commands/agent_info.rs` | +20 -10 | sync to v0.3.1 surface, env_vars block, expanded exit codes |
| `AGENTS.md` | +14 -3 | version → v0.3.1, "Production hardening (v0.3.x)" section |
| `tests/cli.rs` | +57 -1 | agent_info_includes_v0_3_1_surface drift test, update existing phase_4 test for --raw |
| `tests/support/stub_email_cli.sh` | +5 | STUB_EMAIL_CLI_SLEEP_SEC mode |
| `Cargo.toml` | +1 -1 | version → 0.3.1 |
| `Cargo.lock` | regenerated | |
| `README.md` | +1 -1 | status badge → v0.3.1 |
| `docs/handoffs/2026-04-09-session-6-handoff.md` | +this file | session handoff |

**Total commits this session:** 7 (spec, plan, schema check, timeout, lock, agent-info sync, version bump). Plus this handoff = 8.

---

## Session 6 entry-point summary (compressed)

> Read `2026-04-09-session-5-planning-handoff.md` then this file. v0.3.1 is current stable on crates.io + Homebrew. **The v0.4 plan is still NOT written** — that's job #1 next session, with the migration renumbered to 0005. Otherwise scope is unchanged from session 5. The GPT Pro full hardening review is still pending; when it returns, save to `docs/handoffs/2026-04-09-gpt-pro-hardening-review.md` and fold high-leverage findings into v0.5+ roadmap. Hard rule still applies: every tagged release goes through the 13-step paperfoot.com smoke flow plus any new step the release introduces.
>
> **Critical reminders carried from session 5:**
> - User explicitly deferred subscription HTTP surface AND drip/automation/sequences. Do NOT include either in v0.4.
> - Monetization is operator-embeds-attribution-aware-links + revenue ingestion, NOT a marketplace. We never touch payment processing.
> - BG-1 content snapshot is the highest-leverage item in v0.4 — foundation for compliance, audit, A/B comparison, reproducibility.
> - Hold the line at 14 runtime deps. v0.3.1 shipped zero new ones. v0.4 should ship zero new ones too unless `dnscheck` (in v0.4 Phase A per session-5 decision) needs a DNS resolver crate.

---

*End of session 6 emergency-hardening handoff.*
