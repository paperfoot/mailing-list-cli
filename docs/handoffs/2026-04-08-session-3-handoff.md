# Session Handoff — Session 3

**Date:** 2026-04-08 (late evening)
**Session:** v0.1.3 Codex-reviewed fixes → v0.2.0 agent-native rearchitecture → v0.2.1 real-Resend validation + state leak fix → v0.2.2 race-free test fix + CI green
**Context usage at handoff:** roughly 85%

> **Note:** This file was rewritten in place at the end of the session to supersede the earlier handoff that was committed at `388bca8`. The earlier version stopped at v0.2.1 and did not cover the CI flake discovery, the paths race fix, or the v0.2.2 tag. Git history has the original if needed.

---

## TL;DR

Four tagged releases shipped this session on top of v0.1.2:

| Tag | Commit | What | CI |
|---|---|---|---|
| `v0.1.3` | `4465fdd` | Codex-reviewed template lint fixes (unified variable extractor, per-offender line numbers, realistic placeholder sizes) | ✓ |
| `v0.2.0` | `6ea71d4` | **The big one.** Three-way review (Claude + Codex gpt-5.4 xhigh + Gemini 3.1-pro-preview) drove an aggressive agent-native rearchitecture. Dropped MJML, Handlebars, css-inline, html2text, serde_yaml, YAML frontmatter schemas, 14 of 20 lint rules, PEST segment DSL, `webhook listen`, `webhook test`, `template edit`, `template guidelines`. Added hand-rolled `{{ var }}` substituter, `template preview`, JSON-AST segment filters. 23 → 14 crates. ~9500 → ~5500 LoC. | ✓ |
| `v0.2.1` | `9d8c6d1` | **Real-Resend smoke test against paperfoot.com PASSED end-to-end** (first time since v0.1.1). Bug found during smoke test: `broadcast send` left the status stuck in `sending` after a render error. Fixed. Docs audit for README + AGENTS.md. | ⚠ CI flaked on the pre-existing paths race |
| `v0.2.2` | `5bad68a` | **Latest stable.** Race-free env-var tests via in-module Mutex. CI pinned to `--test-threads=1` as belt+suspenders. CI green on the tagged commit. | **✓** |

**Current state at handoff:**
- Test count: 97 unit + 62 integration = **159 tests passing** (parallel in 1.48s with the mutex fix)
- Dependencies: **14 runtime crates** (-39% from v0.1.3)
- Rust LoC: **~5500** (-42% from v0.1.3)
- Template lint rules: **6** (was 20, -70%)
- `target/release/mailing-list-cli` built and validated against real Resend
- Smoke test DB preserved at `/tmp/mlc-smoke-v0.2.0/state.db` (1 sent + 1 failed broadcast)

---

## Active Plan

**Plan file:** [`docs/plans/2026-04-08-phase-7-v0.2-rearchitecture.md`](../plans/2026-04-08-phase-7-v0.2-rearchitecture.md)
**Plan status:** **FULLY EXECUTED.** All three phases shipped in v0.2.0, validated in v0.2.1, race-fixed in v0.2.2. The plan document itself is frozen for historical reference.

**Next plan:** None written yet. Outstanding items below are loose tasks, not a structured plan. If v0.3 ends up being large (template versioning + real migration 0003 + DMARC checks + daemon), it deserves its own plan file.

---

## Three-way review artifacts (preserved for reference)

All at `~/.claude/subagent-results/`:
- `rearch-brief-1775664888.md` — the structured brief all three reviewers answered
- `codex-output-1775664888-rearch.md` — Codex gpt-5.4 xhigh review
- `gemini-output-1775664888-rearch.md` — Gemini 3.1-pro-preview review
- `claude-analysis-1775664888-rearch.md` — my own independent analysis
- Earlier v0.1.3 Codex review: `codex-output-1775657343-template-gaps.md`

These files are critical context — if this handoff is ambiguous, read the raw reviews.

---

## What was accomplished this session

### v0.1.3 — Codex-reviewed template lint fixes (session start)
Six gaps identified at the end of session 2; Codex ranked and advised. Shipped 3 code fixes + 2 docs fixes, deferred 1:

- **Gap #3** unused-var textual check — unified `extract_merge_tag_names` to cover `{{#if}}`/`{{#unless}}` arguments and normalize whitespace
- **Gap #5** alt/href break after first offender — added `line: Option<usize>` to `LintFinding`, dropped the breaks, emit per-offender
- **Gap #6** size lint underestimates real send — replaced placeholder stubs with realistic HTML matching send-time shape
- **Gap #1** mrml silent drops — docs-only (subsequently deleted in v0.2)
- **Gap #4** no composition — softened "v0.2+" promise in lint hints
- **Gap #2** template versioning — deferred (still outstanding, see below)

### v0.2.0 — agent-native rearchitecture
The user pushed back on the MJML stack itself with the "agent has preview + iteration loop" argument. I ran a three-way review (Claude + Codex + Gemini, parallel, same brief) which converged on aggressive simplification. User picked **Option A** (max aggression, 14 crates).

**Phase 1 (commit `cb5d36c`):**
- Deleted `src/segment/parser.rs` + `grammar.pest` (~640 lines). PEST was an authoring façade; segments were already stored as JSON AST internally. Agents now pass `--filter-json <json>` / `--filter-json-file <path>`.
- Deleted `src/webhook/listener.rs` + `signature.rs` (~240 lines). `webhook listen` was untested and violated AGENTS.md doctrine that email-cli owns the listener.
- Deleted `template edit` (violated "no interactive prompts, ever"), `template guidelines` (153-line doctrine replaced by the scaffold), `webhook test`.
- Deps: `pest`, `pest_derive`, `tiny_http` removed; `tempfile` moved to dev-deps.

**Phase 2+3 (commit `6ea71d4`):**
- Deleted `src/template/{compile,lint,frontmatter}.rs` and `assets/template-authoring.md` (~1350 lines)
- Added `src/template/subst.rs` (~400 lines) — hand-rolled `{{ var }}` + `{{{ allowlist }}}` + `{{#if}}`/`{{#unless}}` substituter with depth-aware nesting, HTML escaping, triple-brace XSS allowlist, unresolved-placeholder tracking
- Added `src/template/render.rs` (~500 lines) — 6 inline lint rules, HTML-to-text stripping, strict (send) vs preview render modes
- Rewrote `src/commands/template.rs` — dropped `edit`/`guidelines`, added real `preview` command with `--out-dir`/`--open`
- Updated `src/broadcast/pipeline.rs` — uses new `template::render()` in strict mode, hard-fails on any unresolved placeholder at send time (the v0.2 replacement for the v0.1 frontmatter variable schema)
- Migration 0003 — sentinel no-op (v0.1 databases are not supported; migration 0001 now creates the v0.2 shape directly)
- DB schema: `template.mjml_source` → `template.html_source`, dropped `template.schema_json`
- Deps removed: `mrml`, `handlebars`, `css-inline`, `html2text`, `serde_yaml`
- Version 0.2.0, README badge, agent-info updated

### v0.2.1 — real-Resend validation + state fix (commit `9d8c6d1`)
First real-Resend smoke test since v0.1.1. v0.1.2, v0.1.3, and the initial v0.2.0 ship all skipped this (the handoff warned three sessions ago).

Flow executed against live `paperfoot.com` (us-east-1) via `email-cli` profile `local`:
1. health — all 4 checks green
2. list create — real Resend segment `ccb0d9d9-ddb4-4c9e-aa22-c3167f9a00dc`
3. contact add — real Resend contact
4. template create + lint + preview — 0 errors, 0 warnings
5. broadcast create
6. **broadcast preview** (real single send to `smoke-test-v0.2.0@paperfoot.com`) — sent=1
7. **broadcast send** (real batch) — sent=1, failed=0
8. event poll — processed 100 real Resend events
9. broadcast show — delivered_count=1 (attributed via resend_email_id)
10. report show — full metrics populated
11. **Hard-fail path** — typo template → `broadcast send` → exit 3, `template_unresolved_placeholder`, zero email-cli calls, no batch file

**Bug caught by step 11:** broadcast stuck in `sending` status after render error instead of being reverted to `failed`. Fixed in `src/broadcast/pipeline.rs` — the render call now explicitly matches on the error, calls `broadcast_set_status(id, "failed", None)`, then returns. Integration test `broadcast_send_hard_fails_on_unresolved_placeholder` now asserts the failed state.

**Docs audit:** README command tables updated for v0.2; AGENTS.md stale "not yet written" text replaced.

### v0.2.2 — race fix + green CI (commits `8f06937` + `5bad68a`)
**v0.2.1 CI failed** on the pre-existing `paths::tests` race — cargo's default thread pool races on process-global env vars. The handoff has warned about this since session 1 ("always use `--test-threads=1`") but CI was running without the flag and we got lucky on v0.1.x → v0.2.0. v0.2.1 lost the race.

**Fix in `src/paths.rs`:** added a file-scope `static ENV_MUTEX: Mutex<()>` that each env-mutating test locks. Restructured each test to read the value and remove the env var BEFORE the assertion, so a failing assertion can't leak state into sibling tests. Poisoned-mutex recovery via `unwrap_or_else(|e| e.into_inner())`.

**Belt+suspenders in `.github/workflows/ci.yml`:** added `-- --test-threads=1` to the test step. The mutex alone is sufficient for current tests, but single-threaded testing protects against future env-var tests landing before someone wraps them in the same mutex. Negligible cost (~1.5s on a 159-test suite).

v0.2.2 tag push → CI **green** (`24160755176`).

---

## Key decisions made this session (not in code or plan)

1. **Three-way review is the right pattern for architectural decisions.** Codex + Gemini + Claude in parallel on the same brief, then synthesize. All three converged on the agent-native thesis; where they disagreed (JSON AST vs raw SQL for segments), the disagreement was load-bearing and I resolved by the cheapest implementation (JSON AST — Codex caught that segments were already stored that way internally, making removal trivial).

2. **"Agent-native CLI" is the framing that unlocks the simplification.** Assumptions that made sense for a blind-human author (declare schema upfront, lint every possible mistake, embed a 153-line doctrine) become dead weight once you commit to agent-with-preview. Test: v0.2 end-to-end works and the code is 42% smaller.

3. **Migration 0003 as a sentinel no-op is a defensible shortcut.** Zero production users means clean-slate upgrade is fine. If a production user ever emerges, write a real migration then. Documented in the risk register. Anyone upgrading a v0.1.x database in place will hit SQL errors.

4. **Real-Resend smoke testing is not optional.** Three releases in a row (v0.1.2, v0.1.3, initial v0.2.0) skipped it. The v0.2.1 run caught a bug that the stub couldn't see. This is now a hard rule: every tagged release goes through the paperfoot.com smoke test before being declared done.

5. **Scaffold IS the documentation.** The v0.2 scaffold at `src/commands/template.rs::SCAFFOLD` is the only template docs an agent sees. It has to be self-explanatory because there's no separate guide anymore. Smoke test passed on the first try, but the blind-test re-run (see "What to do next" below) will tell us for real.

6. **CLAUDE.md rule: single-agent sessions don't use TaskCreate.** Held throughout. Tracking mentally worked fine for a multi-phase refactor.

7. **The v0.2.1 CI flake is a code smell, not a fluke.** The race had been latent since session 1 but only started biting once commits landed in rapid succession. Fixing it properly (mutex + CI pin) is cheaper than another post-tag surprise.

8. **Tag v0.2.2 instead of moving v0.2.1.** v0.2.1's commit is functionally correct; only its CI run flaked. Moving a tag is usually worse than bumping. v0.2.2 is the latest "CI-green guaranteed" tag to point people at.

---

## Current state

- **Branch:** `main`
- **Last commit:** `5bad68a chore: bump to v0.2.2 — race fix + status string`
- **Latest tag:** `v0.2.2`
- **Uncommitted changes:** none (working tree clean)
- **Tests passing:** yes. 97 unit + 62 integration = **159 passing** in 1.48s parallel, ~3.5s with `--test-threads=1`
- **Build status:** clean (`cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` all clean)
- **CI status:** **green** on v0.2.2 commit (run `24160755176`, 2m13s)
- **Release binary:** `target/release/mailing-list-cli` (3.5 MB LTO+strip)
- **Pushed:** main + all tags pushed to `paperfoot/mailing-list-cli`
- **Smoke test DB:** `/tmp/mlc-smoke-v0.2.0/` preserved — config.toml, welcome.html, typo.html, state.db, cache/, preview/

---

## What to do next

### 1. Fix the stale design spec (HIGH PRIORITY, bounded)
**File:** `docs/specs/2026-04-07-mailing-list-cli-design.md`

§7 still documents the v0.1 MJML + frontmatter + 20-rule lint architecture. Agents reading the spec see the old design, not v0.2.

Choose one:
- **(a) Annotate** — add a prominent banner at the top of §7 saying "SUPERSEDED in v0.2 by `docs/plans/2026-04-08-phase-7-v0.2-rearchitecture.md`" with a 1-paragraph summary of what changed
- **(b) Rewrite** — replace §7 with the v0.2 shape (plain HTML, substituter, 6 lint rules, `template preview` as the iteration primitive)
- **(c) Delete** — remove the spec entirely and make the Phase 7 plan the canonical reference

Recommendation: **(a) first**, because it's fast and unambiguous; revisit (b) only if someone complains. The plan file is the canonical source of truth for v0.2 anyway.

### 2. Re-run the blind template authoring test for v0.2
**Goal:** verify the scaffold alone is enough for agents to cold-start the template system.

Setup:
```bash
# Extract the scaffold as the only documentation an agent gets
export MLC_CONFIG_PATH=/tmp/mlc-blind-v020/config.toml
export MLC_DB_PATH=/tmp/mlc-blind-v020/state.db
mkdir -p /tmp/mlc-blind-v020
# Minimal config so `template create` works
cat > $MLC_CONFIG_PATH <<'EOF'
[sender]
from = "test@example.com"
physical_address = "123 Test St"
[email_cli]
path = "/Users/biobook/.cargo/bin/email-cli"
profile = "local"
[unsubscribe]
public_url = "https://hooks.example.com/u"
secret_env = "MLC_UNSUBSCRIBE_SECRET"
EOF
./target/release/mailing-list-cli template create welcome --subject "Welcome" | jq .
./target/release/mailing-list-cli template show welcome | jq -r .data.html_source > /tmp/blind-scaffold.html
```

Then dispatch **Codex + Gemini + Claude (me)** in parallel with the same prompt:

> "You are writing an email template for mailing-list-cli v0.2.2. You have exactly these tools:
> - `template create <name> --subject <text> --from-file <path>` to save a template
> - `template lint <name>` to validate compliance (6 rules: unsubscribe link, physical address footer, size, forbidden tags, unresolved placeholders, XSS allowlist)
> - `template preview <name> --with-data <file> --out-dir <dir>` to render HTML to disk
>
> Here is the built-in scaffold as your only reference:
> ```html
> {scaffold HTML}
> ```
>
> Write a welcome email for a fictional product, iterate via preview until the HTML looks good and the lint is clean. Deliver the final template file path and a summary of how many iterations it took."

Measure:
- Did all three produce a clean-lint template on the first try?
- If not, how many preview iterations did each need?
- Did any hit a footgun that the scaffold doesn't document?

Save the outputs to `~/.claude/subagent-results/blind-test-v020-{agent}-{ts}.md` and write a summary at `docs/blind-test-results-v0.2.md`.

### 3. Decide on v0.3 scope
The handoff section below lists six deferred items. Look at them together and decide:
- Ship a v0.3 with **just** template versioning (Gap #2 from v0.1.3 Codex review)? Small and focused.
- Or bundle template versioning + real migration 0003 + `dnscheck` for DMARC/SPF/DKIM? Medium.
- Or go bigger with a `daemon` subcommand for long-running poll? Large.

Write the v0.3 plan at `docs/plans/2026-04-08-phase-8-v0.3-versioning.md` (or similar) before touching code.

---

## Files to review first

1. **`docs/plans/2026-04-08-phase-7-v0.2-rearchitecture.md`** — the plan that was just executed. Read this to understand the v0.2 thesis.
2. **`src/template/subst.rs`** — the hand-rolled substituter (~400 lines). Understand the two render modes and the triple-brace allowlist before touching templates.
3. **`src/template/render.rs`** — the 6 lint rules + strict vs preview modes. This is where send-time hard-fail lives.
4. **`src/broadcast/pipeline.rs`** — where the strict-mode render is called, and where the "mark failed on render error" fix lives (lines ~165-200).
5. **`src/commands/template.rs`** — the CLI handlers including the `preview` command and the built-in `SCAFFOLD` constant.
6. **`~/.claude/subagent-results/codex-output-1775664888-rearch.md`** — Codex's review that drove the v0.2 plan. Has the best one-line justifications for each deletion.

---

## Gotchas & warnings

### Tests
- **`cargo test` parallel now works** — the `paths::tests` race is fixed via in-module Mutex. Both `cargo test` and `cargo test -- --test-threads=1` are green.
- **CI uses `--test-threads=1`** by default (belt+suspenders). Don't remove it without understanding why.
- **Future env-var tests must lock the mutex** or the race will come back. Add a `static ENV_MUTEX: Mutex<()>` per file and lock it.

### Templates
- **Don't resurrect the frontmatter.** The whole v0.2 rewrite hinges on dropping it. If a new requirement seems to need declare-time variable validation, use the unresolved-at-send-time hard-fail instead.
- **Don't add a Handlebars dep back.** The hand-rolled substituter in `src/template/subst.rs` supports exactly what v0.2 ships (scalar, triple-brace allowlist, `{{#if}}`, `{{#unless}}`, depth-aware nesting). Adding features via Handlebars would re-explode the dep graph we just cut.
- **Don't trust `lint()` to catch unresolved placeholders.** It explicitly strips `UnresolvedPlaceholder` findings because lint is for structural issues only. The unresolved check lives in `render()` strict mode, called from the broadcast pipeline.
- **Don't add `template preview --serve <port>`.** The three-way review was explicit no. If you want live-reload, fswatch the template file yourself.
- **Migration 0003 is a sentinel no-op.** Anyone upgrading a v0.1.x database in place will hit SQL errors (`mjml_source` / `schema_json` columns still exist but code expects `html_source` only). Zero production users means this is acceptable, but document it if you ever make a release-notes page.
- **The `template preview --open` flag is not tested.** Tests can't reliably launch browsers in CI. Manual smoke testing only.

### Broadcast pipeline
- **`preflight_checks` calls `lint()` which strips unresolved findings** — so a typo template will pass preflight. The hard-fail happens in the chunk loop via `render()` strict mode. If this feels wrong, see "Should do" item 4 in this handoff's original version (which was committed at `388bca8`).
- **On render error in the chunk loop, the status is reverted to `failed`** before bubbling up. Don't break this — the integration test `broadcast_send_hard_fails_on_unresolved_placeholder` asserts it.

### Agents
- **AGENTS.md doctrine is now honored.** v0.1 was silently violating "no interactive prompts, ever" via `template edit` and "email-cli owns the webhook listener" via `webhook listen`. v0.2 dropped both. If you bring either back, fix AGENTS.md first.
- **The scaffold is the documentation.** The 153-line `template-authoring.md` was deleted. Anything that needs to be in the docs now goes in the scaffold HTML as comments or in the built-in help text. Don't bring back an embedded guide.

### Smoke test
- **The paperfoot.com smoke test DB at `/tmp/mlc-smoke-v0.2.0/state.db`** is preserved with 1 sent broadcast (id=1) and 1 failed broadcast (id=2, the typo template). You can reuse it by exporting the same env vars:
  ```bash
  export MLC_CONFIG_PATH=/tmp/mlc-smoke-v0.2.0/config.toml
  export MLC_DB_PATH=/tmp/mlc-smoke-v0.2.0/state.db
  export MLC_CACHE_DIR=/tmp/mlc-smoke-v0.2.0/cache
  export MLC_UNSUBSCRIBE_SECRET="smoke-v0.2.0-secret-at-least-16-bytes"
  ```
- **The smoke test IS mandatory before every release tag.** No more skipping it. Codex caught this in the v0.1.1 session, the handoff has been saying it since, and v0.2.1 proved it by catching a real bug.

### Docs
- **`docs/specs/2026-04-07-mailing-list-cli-design.md` §7 is stale.** Don't quote from it when explaining v0.2 — use the Phase 7 plan instead. See "What to do next" item 1.

---

## Still outstanding (deferred, not next-session-mandatory)

### Should do soon
- **Design spec §7 update** (item 1 above)
- **Blind template authoring test v0.2** (item 2 above)

### Defer to v0.3+
- **Template versioning** (Gap #2 from v0.1.3 Codex review) — migration 0004 + `template_revision` table + `template history <name>` + `template restore <name> --revision N`. Currently `template create --from-file` is a destructive overwrite.
- **Real migration 0003** — upgrade path for v0.1.x databases. Not needed with zero production users; document as "clean-slate upgrade only".
- **DMARC/SPF/DKIM checks** in `report deliverability` — still stubbed with empty `verified_domains`. Would need a `dnscheck` module.
- **Long-running poll daemon** — `event poll` is one-shot. A `daemon` subcommand running the poll loop on a schedule is a v0.3 candidate.
- **`template preview --serve`** — explicitly rejected in v0.2 review. Don't bring it back without a new review.

---

## Test count history

| Version | Tests | Notes |
|---|---|---|
| v0.0.3 (session 1 start) | 30 | |
| v0.0.4 Phase 3 | 101 | |
| v0.0.5 hotfix | 110 | |
| v0.1.0 Phase 4 | 135 | |
| v0.1.1 Phase 5 | 148 | First real-Resend validation |
| v0.1.2 Phase 6 | 167 | Webhooks + reports; **skipped real-Resend** |
| v0.1.3 Codex gap fixes | 173 | **skipped real-Resend** |
| v0.2.0 rearchitecture | 158 | Dropped tests for deleted subsystems, added new ones; **skipped real-Resend** |
| v0.2.1 real-Resend + state fix | 158 | +1 hard-fail integration test (cancelled a flaky one prior) |
| v0.2.2 **(current)** | **159** | **Real-Resend passed**; race-free tests; CI green |

---

## Session entry point for the next run

> Read `docs/handoffs/2026-04-08-session-3-handoff.md`. v0.2.2 is the current stable release — real-Resend validated, CI green, 159 tests passing. Three things to do next session, in order:
>
> 1. **Fix `docs/specs/2026-04-07-mailing-list-cli-design.md` §7** — it still documents the v0.1 MJML architecture. Recommend: add a "SUPERSEDED in v0.2" banner pointing at `docs/plans/2026-04-08-phase-7-v0.2-rearchitecture.md`. 15-minute job.
>
> 2. **Re-run the blind template authoring test** — extract the v0.2 scaffold via `template create welcome` + `template show welcome`, dispatch Codex + Gemini + Claude with fresh prompts asking each to author a clean template using ONLY `template preview` + `template lint` iteration. Document results in `docs/blind-test-results-v0.2.md`. Validates that the scaffold is self-documenting.
>
> 3. **Decide on v0.3 scope.** Six items are deferred. Pick which ones ship in v0.3, write a plan file, brainstorm structure before touching code.
>
> Hard rule: **every tagged release goes through the paperfoot.com smoke test.** The v0.1.1 handoff said this, v0.1.2 / v0.1.3 / initial v0.2.0 skipped it, v0.2.1 caught a real bug because we finally ran it. No more skipping.

---

*End of session 3 handoff (revised at v0.2.2 tag).*
