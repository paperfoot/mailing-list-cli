# Session Handoff — Session 5 (post-v0.3.0 planning)

**Date:** 2026-04-09 (late evening, post-release)
**Session:** Post-v0.3.0 research + planning. **No code shipped.** SendGrid + Beehiiv feature comparison, first-principles analysis of mailing-list primitives, X social signal review, monetization-in-newsletter brainstorm, v0.4 scope decision. Successor to `2026-04-09-session-4-handoff.md` (which covered v0.2.3 + v0.3.0 implementation).

**Context usage at handoff:** roughly 80%

---

## TL;DR

v0.3.0 ("production-grade 10k") shipped earlier in the day. The user then asked the right strategic question: "how close are we to enterprise grade for 10k+ recipients, and what about tracking?" — that triggered the SendGrid + Beehiiv comparison and X social search using the `search` CLI. Over four turns, we converged on a focused v0.4 plan called **"Operator superpowers"**: zero-acquisition-coupling polish + monetization foundations + the biggest hidden compliance gap (content snapshots).

**No commits this session.** The next session's first job is to write the v0.4 plan file via `superpowers:writing-plans`, then execute it.

**Critical scope deferrals (user explicit):**
- **Subscription HTTP surface** — out of scope, user said "we will create a backend or website api etc for users to sign up. basically for later." Do NOT include in v0.4.
- **Drip / automation / sequences** — same, deferred to a separate later release. Do NOT include in v0.4.

The v0.4 scope below is deliberately built around what's actionable WITHOUT touching either of those.

---

## Active Plan

**v0.3 plan (already executed, frozen):** [`docs/plans/2026-04-09-phase-8-v0.3-production-grade-10k.md`](../plans/2026-04-09-phase-8-v0.3-production-grade-10k.md)
- Status: **fully executed** in session 4. All 7 tasks + release task shipped as commits `2a07374` → `f30fd42`. Tag `v0.3.0` pushed to `paperfoot/mailing-list-cli`.

**v0.4 plan (NOT YET WRITTEN — first action for next session):**
- Filename: `docs/plans/2026-04-09-phase-9-v0.4-operator-superpowers.md`
- Status: **scope decided, plan file not yet written**
- Use `superpowers:writing-plans` skill to create it
- Same TDD format as the v0.3 plan
- Target effort: ~5–6 working days
- Target test delta: 177 → ~195

### v0.4 "Operator superpowers" — agreed scope (4 phases)

| Phase | Item | Effort | Why |
|---|---|---|---|
| **A** | P0.1 README v0.3 docs update | 0.5d | `broadcast resume`, `contact erase`, `--raw` flag, retry semantics, `sender_domain_verified` health check are undocumented in the README command tables |
| **A** | P0.2 Idempotent `broadcast create` | 0.1d | Currently `broadcast create --name foo` twice creates two broadcasts. Add UNIQUE on `broadcast.name` or detect-and-fail. |
| **A** | P0.4 `broadcast send --dry-run` | 0.3d | Preflight + render every chunk + report counts WITHOUT calling `email-cli batch send`. Operators want to know what a 10k send WILL do before doing it. |
| **A** | **BG-1 Content snapshot at send time (migration 0004)** | 1.0d | **THE single most important item in v0.4.** When `broadcast send` completes, store `snapshot_html`, `snapshot_text`, `snapshot_subject` columns on `broadcast` (or new `broadcast_snapshot` table). Foundation for compliance, audit, A/B variant comparison, reproducibility. Without it, editing a template after sending destroys the audit trail. |
| **B** | MON-1 Auto-injected UTM tags on every outbound link | 1.0d | Walk HTML at render time, append `?utm_source=mailing-list-cli&utm_medium=email&utm_campaign={broadcast_name}&utm_content={link_index}`. Override per-link via `data-utm-content="..."` attribute, disable via `data-utm="off"`. Lives in `src/template/render.rs`. Foundation for ALL attribution. |
| **B** | MON-2 Stripe `client_reference_id` injection | 0.5d | Same link rewriter recognizes `https://buy.stripe.com/*` and `https://checkout.stripe.com/*`, auto-appends `?client_reference_id=mlc_b{broadcast_id}_c{contact_id}`. Operator's existing Stripe webhook then matches payments back to broadcast+contact. |
| **B** | MON-3 `revenue` table + `revenue add` + `revenue import` + `report revenue` | 1.0d | New migration column-set: `revenue (id, broadcast_id, contact_id, amount_cents, currency, source, external_id, recorded_at)`. Manual record + Stripe CSV bulk import + per-broadcast aggregation. **The killer report** — operators can finally answer "did this newsletter pay for itself?" |
| **B** | MON-4 Affiliate link auto-wrapping | 0.5d | `config.toml` declares `[[affiliates]]` patterns (host + param + value). Link rewriter auto-injects them. Same code path as MON-1. |
| **B** | MON-5 Surface `revenue_attributed_cents` in `report show` | 0.1d | One-line addition to existing report. |
| **B** | MON-6 `report ltv` | 0.4d | Aggregate `revenue` table by `contact_id` over a window. `--top 100` shows highest-value subscribers. |
| **C** | P0.3 Subject-line spam preview | 0.3d | Local rule-based 0–10 score. ALL CAPS ratio, exclamation count, dollar signs, "FREE", emoji density. No external deps, no API. |
| **C** | P0.5 CSV export from `report show` / `report links` / `report engagement` | 0.3d | `--format csv` flag. |
| **C** | P1.5 `contact sunset --inactive-since 90d --tag dormant --confirm` | 0.6d | Bulk-tag (or bulk-suppress with reason `inactive_sunsetted`) any contact with no `email.opened` event in the window. **Highest-leverage list-hygiene op** per X signal — `@0Venkata`'s example saw open rate jump from 41% → 55–60% after sunsetting 3,800 dormant subs. |
| **C** | P1.3 `bounce show <broadcast_id>` | 0.3d | Drill into bounces with full Resend bounce subtype + message. Today bounces just disappear into the suppression table. |
| **D** | Release: bump Cargo.toml + README badge + agent-info to v0.4.0 | 0.2d | |
| **D** | **Mandatory paperfoot.com smoke test** (now 17 steps) | 0.3d | Adds: `revenue add` → `report revenue` → `contact sunset --confirm` → `bounce show 1` to the existing 13-step v0.3.0 flow. |
| **D** | Tag v0.4.0, push, write session-6 handoff | 0.1d | |

**Sum:** 7.6 days, but tasks are independent and the user has historically been comfortable parallelizing via inline TDD.

### Deliberately deferred to v0.5 / later (do NOT include in v0.4)

| Item | Why deferred |
|---|---|
| P1.2 A/B testing commands | Schema exists, but pairs better with `broadcast diff` (P2.2) — ship both together in v0.5 |
| P1.4 `event tail` (with `--follow`) | Pairs with `daemon` work that's also deferred |
| P1.6 Geo + device parsing of click events | Adds first new runtime dep (MaxMind GeoLite2 + woothee user-agent parser, ~6MB+). Should be a deliberate decision in its own release, not bundled. |
| P2.1 Audit log | Bigger compliance pass deserves its own focus |
| P2.3 DNS MX check on `contact add` | Defensive, niche |
| P2.4 Engagement cohort view | Niche analytics |
| P2.5 Reply-to override per broadcast | Niche use case |
| P2.6 `account` subcommand | Quality of life |
| BG-3 Sponsor table | Pairs better with content snapshots being mature first |
| BG-4 Content blocks library / partials | Substituter extension deserves its own release |
| MON-7 Per-recipient Stripe coupon generation | First HTTP dep should be deliberate (`reqwest` is heavy) |
| MON-8 Per-recipient first-party redirects | Requires operator-hosted redirect infra |
| BG-9 Multi-attachment / inline image support | 2 days, low priority unless asked |
| BG-10 `--from` override per broadcast | Niche |
| BG-11 Warmup mode for new domains | Niche |
| BG-12 Unsubscribe reason capture | Depends on unsubscribe redirect hosting |
| `dnscheck` (SPF/DKIM/DMARC) | Originally planned for v0.4. Adds a DNS resolver dep. Could fit in v0.4 OR move to v0.5. **DECISION REQUIRED next session** — see "What to do next" item 0. |
| `daemon` subcommand for poll loop | Originally planned for v0.4. Tightly coupled with future `sequence`/automation work which is deferred per user. Move to v0.5 or v0.6. |
| Template versioning (Gap #2 from v0.1.3) | Original v0.4 candidate per session-3 handoff. Ship after content snapshots (BG-1) which are a strict prerequisite. |

---

## What was accomplished this session

**Zero code commits.** Pure research and planning.

1. **SendGrid feature inventory** — pulled the 2026 pricing page via WebFetch + corroborating GMass review. Captured both Email API tier features (Free Trial / Essentials $19.95 / Pro $89.95 / Premier custom) and Marketing Campaigns tier features (Free Trial / Basic $15 / Advanced $60 / Custom). Notable: dedicated IP + auto-warmup, dynamic templates, Email Validation paid add-on, signup forms.

2. **Beehiiv feature inventory** — pulled the features page. Notable: Editor + AI + audio + polls; Boosts + Referral Program + Subscribe Forms + Pop-ups + Magic Links + Recommendations; Ad Network ($1M+/mo to publishers) + Paid Subscriptions + Direct Sponsorships + Digital Products; A/B Testing + Verified Clicks + Surveys; Web Builder.

3. **X social search via `search` CLI in social mode** — three queries against xAI Grok:
   - "newsletter operator complaints email tool features missing 10000 subscribers" — surfaced migration horror stories (`@Maurizio_Isendo`: "42,000 subscribers stopped hearing from her")
   - "beehiiv vs sendgrid vs ghost vs convertkit feature comparison" — surfaced the **load-bearing insight** from `@tibo_maker` (465 likes, 85 replies, Aug 2024): "the big guys (Beehiiv, Convertkit, prob others too...) are not even sending emails by themselves. They rely on Sendgrid". Confirmed the layered architecture: layer-1 = ESP (Resend/SendGrid/Mailgun), layer-2 = orchestration (Beehiiv/ConvertKit/mailing-list-cli).
   - "newsletter growth subscribers what features actually matter" — surfaced the **tier-1 social signals**:
     - `@sharyph_`: removed inactives, gained 255 engaged subs in 30 days
     - `@0Venkata`: founder with 6,200 subs, 41% open rate. Suppressed 3,800 non-openers who hadn't opened in 6+ months. Open rate jumped to 55–60%.
     - `@strobyai`: "5K-subscriber newsletter with 55% open rates can outperform a 100K list"
     - `@LeCodeBusiness`: "Subscribe to my newsletter" → 3% opt-in. "Get the free 7-step launch checklist" → 22% opt-in.

4. **First-principles framework: 8 mailing-list primitives.** Derived from "what does it take to run a successful list" rather than from copying ESP feature taxonomy:
   1. Reach the inbox
   2. Get permission honestly
   3. Know who's listening
   4. Compose the right message
   5. Send at the right moment
   6. Measure and learn
   7. Keep the house clean
   8. Make it pay (optional)

5. **Comparison matrix per primitive: SendGrid vs Beehiiv vs mailing-list-cli v0.3.0.** Captured in the conversation. Verdict: v0.3.0 is **strong** on primitives 1, 3, 5, 7 (the "delivery infrastructure + compliance" half); **acceptable** on 4 + 6; **weak** on 2 + 8 by design.

6. **User explicit deferrals** clarified scope:
   - "subscription and automated posts, we will work on that later — we will create a backend or website api etc for users to sign up. basically for later" → primitives 2 + 5's drip half are out of scope.
   - Monetization framing: "we will include links, payments or whatever in the mailing list, not sure how, but think about it" → primitive 8 is IN scope, but as **operator embeds attribution-aware links**, NOT as a multi-tenant ad marketplace.

7. **Monetization brainstorm** produced 8 primitives (MON-1 through MON-8), of which MON-1 through MON-6 are in v0.4 scope. The "operator hosts the redirect" pattern (MON-8) and "Stripe API for per-recipient coupons" (MON-7) are deferred because both add operator infra requirements.

8. **9 "big gaps" surfaced during the comparison work** that the user hadn't previously named:
   - **BG-1 Content snapshot at send time** — this is the single biggest hole and is now in v0.4 scope.
   - BG-2 Revenue tracking (covered by MON-3)
   - BG-3 Sponsor placement records — deferred to v0.5
   - BG-4 Content blocks / partials library — deferred
   - BG-5 Bounce drill-down — covered by P1.3 in v0.4
   - BG-6 `event tail` — deferred (pairs with daemon)
   - BG-7 Subject-line spam preview — covered by P0.3 in v0.4
   - BG-8 Idempotent commands — partially covered by P0.2
   - BG-9 Multi-attachment / inline image — deferred, niche
   - BG-10 Per-broadcast `--from` override — deferred, niche
   - BG-11 Warmup mode for new domains — deferred, niche
   - BG-12 Unsubscribe reason capture — deferred, depends on operator infra

9. **Final v0.4 scope agreed** — "Operator superpowers", 4-phase structure documented above.

---

## Key decisions made this session (not in code or plan files)

1. **Layer-2 positioning is correct, validated by X data.** The `@tibo_maker` post is the load-bearing evidence — Beehiiv and ConvertKit, the platforms with the loudest "we're a newsletter platform" branding, both delegate actual email delivery to SendGrid (or equivalents). mailing-list-cli does the same with Resend via email-cli. This is not a weakness; it's the **standard architecture**. Don't try to compete with the layer-1 ESPs on delivery infrastructure — compete with the layer-2 orchestrators (Beehiiv/ConvertKit) on agent-native ergonomics.

2. **Open rate is the canonical engagement metric** — every successful operator on X talks about it. List hygiene (suppressing inactives) is the #1 lever to move it. This is why **`contact sunset` is in v0.4 Phase C** despite being a small feature — it's the highest-ROI list-hygiene op per real-world signal.

3. **Niche > scale** is the meta-rule. A 5k newsletter with 55% open rate beats a 100k list with bad targeting. Implication: every feature we ship should make it EASIER to maintain a tight, engaged list rather than a sprawling one. Sunset, complaint guard (already shipped in v0.3.0), suppression HashSet (v0.3.0) all align. Adding "list growth at any cost" features would violate this rule.

4. **Monetization belongs in the link layer, not as a marketplace.** The user's framing is exactly right: "we will include links, payments or whatever in the mailing list." The right architecture is to make EVERY link in a broadcast trackable + attributable + auto-decorated with affiliate/UTM/Stripe metadata at render time, then ingest revenue events back via the existing webhook pipeline. We never touch payment processing — Stripe / GitHub Sponsors / Buy Me a Coffee already do that. Our job is to be the world's best link decorator.

5. **Content snapshot (BG-1) is the single most leverage-rich feature in v0.4** — it's a precondition for compliance, audit logs, A/B variant comparison, template versioning, and reproducibility debugging. Without it, editing a template after a send destroys forensic evidence. It's also small (1 day) and migration-only. Highest ROI per line of code in the entire v0.4 plan.

6. **First HTTP dependency should be deferred.** MON-7 (Stripe API for unique coupons) would require `reqwest` or similar — the first new runtime dep since v0.2 dropped 9 of them. We've held the line at 14 crates for 5 releases. Don't break it casually. v0.4 uses zero new deps; if MON-7 is added later, that's the moment to deliberately discuss the dep budget.

7. **No subagent-driven execution this session.** The session-4 implementation already revealed that subagent dispatch hits usage limits mid-task; inline TDD execution worked fine. v0.4 should use the same inline TDD approach, OR — if dispatching subagents — be prepared to fall back to inline mid-plan. Either way, every task should commit independently with TDD discipline.

8. **The reduced Task 7 from v0.3 (sender_domain_verified instead of full tracking config)** is a good template for future blocked features: ship the strictly-additive piece you CAN, defer the rest to v0.3.x with a clear pointer at the upstream blocker. Tracking-config full surfacing is still pending an email-cli upstream fix.

---

## Current state

- **Branch:** `main`
- **Last commit:** `97eaa1b docs: session 4 handoff — v0.2.3 polish + v0.3.0 production-grade 10k`
- **Latest tag:** `v0.3.0`
- **Uncommitted changes:** none (this session was research-only)
- **Tests passing:** 115 unit + 62 integration = **177 passing** (snapshot of v0.3.0 state)
- **Build status:** clean (verified at v0.3.0 release)
- **CI status:** v0.3.0 commit was running at end of session 4; presumed green (no notification of failure during session 5 research)
- **Pushed:** main + v0.3.0 + v0.2.3 + v0.2.2 + v0.2.1 + v0.2.0 + v0.1.x tags pushed to `paperfoot/mailing-list-cli`
- **Smoke test DB:** `/tmp/mlc-smoke-v0.3.0/` preserved

---

## What to do next

**Read `docs/handoffs/2026-04-09-session-4-handoff.md` first** (the implementation handoff for v0.2.3 + v0.3.0). Then this file. Together they're the full session-4 + session-5 picture.

Then, in order:

### 0. (Optional, before plan-writing) Decide on `dnscheck` placement
The session-4 handoff originally placed `dnscheck` (SPF/DKIM/DMARC verification) in v0.4. Session 5 reframed v0.4 around polish + monetization and the dnscheck slot moved to v0.5. **Confirm with the user** before writing the v0.4 plan: should `dnscheck` go in v0.4 (adds ~1 day + 1 dep), or stay deferred to v0.5 with the daemon and audit log?

### 1. Write the v0.4 plan file
Use the `superpowers:writing-plans` skill. Save to `docs/plans/2026-04-09-phase-9-v0.4-operator-superpowers.md`. The plan should include all 16 items from the 4-phase table above, structured as TDD tasks with bite-sized steps. Use the v0.3 plan at `docs/plans/2026-04-09-phase-8-v0.3-production-grade-10k.md` as a structural template.

Specific requirements for the v0.4 plan content:
- **Phase A** (Foundations): each task ~2-10 steps, BG-1 (content snapshot) gets the most detail because it's the load-bearing item. Migration `0004_content_snapshots_and_revenue` creates BOTH the broadcast snapshot columns AND the revenue table in one shot.
- **Phase B** (Monetization): MON-1 through MON-6. Each task should include the link-rewriter unit tests for UTM injection edge cases (already-has-query-string, fragment-only, mailto:, javascript:, data:). MON-3 needs 3 commands (`revenue add`, `revenue import --from-stripe-csv`, `report revenue`) plus a `revenue ls` for completeness.
- **Phase C** (Polish): P0.3 + P0.5 + P1.5 + P1.3. Each is small and standalone; can land in any order within the phase.
- **Phase D** (Release): the smoke test must include the new v0.4 flows. List the 17 specific steps (the existing 13 + 4 new: `revenue add`, `report revenue`, `contact sunset --confirm`, `bounce show 1`).

### 2. After plan approval, execute it via `superpowers:subagent-driven-development` OR inline TDD
Same fallback pattern as v0.3 — start with subagents, fall back to inline if usage limits hit.

### 3. After all phase tasks land, do the release (Phase D)
1. Bump `Cargo.toml` 0.3.0 → 0.4.0
2. Update README badge
3. Update `src/commands/agent_info.rs` status string
4. `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test -- --test-threads=1`
5. `cargo build --release && ./target/release/mailing-list-cli --version`
6. **Run the 17-step paperfoot.com smoke test** (mandatory hard rule)
7. Commit version bump
8. `git tag v0.4.0 HEAD -m "v0.4.0 — operator superpowers"`
9. `git push origin main && git push origin v0.4.0`
10. Write `docs/handoffs/YYYY-MM-DD-session-6-handoff.md`

### 4. After v0.4.0 ships, decide on v0.5 scope
The deferred items list is in this handoff under "Deliberately deferred to v0.5 / later". Likely v0.5 candidates:
- A/B testing commands (P1.2) + `broadcast diff` (P2.2)
- `dnscheck` for SPF/DKIM/DMARC
- Audit log (P2.1)
- `daemon` subcommand for poll loop
- Geo + device parsing (P1.6) — first new runtime dep

---

## Files to review first

1. **`docs/handoffs/2026-04-09-session-4-handoff.md`** — the implementation handoff for v0.2.3 + v0.3.0. Sets the technical context.
2. **`docs/plans/2026-04-09-phase-8-v0.3-production-grade-10k.md`** — the v0.3 plan that was just executed. Use as a structural template for the v0.4 plan.
3. **`src/broadcast/pipeline.rs`** — where MON-1 (UTM injection) and MON-2 (Stripe ref injection) will eventually live (or in render.rs — see decision below). The `send_broadcast` function shows the chunk loop pattern. ~470 lines.
4. **`src/template/render.rs`** — where the link rewriter MUST live so it runs both for `template preview` and for the broadcast send path. The existing `strip_html_comments()` function is the closest analog: a render-time HTML transform. ~600 lines including the new tests.
5. **`src/db/mod.rs`** — where the `revenue` table queries and `broadcast_snapshot` columns will live. ~2300 lines, find by grep.
6. **`src/db/migrations.rs`** — where migration 0004 will live. Currently has 0001 (initial), 0002 (event idempotency + kv), 0003 (sentinel no-op for v0.1→v0.2 schema).
7. **`tests/support/stub_email_cli.sh`** — the bash stub email-cli for tests. v0.4 may need to extend it with a `STUB_REVENUE_RESPONSE` mode if MON-3's CSV import gets integration tests.

---

## Gotchas & warnings

### Implementation specifics to remember when writing the v0.4 plan

- **The link rewriter for MON-1/MON-2/MON-4 belongs in `src/template/render.rs`**, NOT in `src/broadcast/pipeline.rs`. Reason: the rewriter should also affect `template preview` output so operators can see the UTM-decorated links during iteration. Putting it in the pipeline only would surprise them at send time.
- **The link rewriter MUST run AFTER substitution**, not before. Reason: a `{{ landing_page_url }}` merge tag that resolves to `https://example.com` should get UTM params on the resolved URL, not get its `{{ }}` syntax mangled. Order: substitute → strip-comments → rewrite-links → lint.
- **Existing `strip_html_comments()` function is the structural template** for the link rewriter — same `&str → String` shape, same byte-walking pattern, same place in `render_inner()`.
- **UTM merge needs to handle existing query strings.** Don't blindly append `?foo=bar` — check for `?` first and use `&` if present. Don't double-encode existing params.
- **Excluded URL schemes** for the link rewriter: `mailto:`, `javascript:`, `tel:`, `sms:`, `data:`. Test cases should cover each.
- **`<a>` tag detection** needs to handle attributes in any order, with single OR double quotes, with whitespace inside the tag. Don't write a regex; do a manual byte scanner like `strip_html_comments` does, OR use a lightweight HTML walker (no, don't add a dep, keep it manual).
- **The `data-utm-content="X"` per-link override** should be parsed from the `<a>` tag's attributes during the rewrite pass. Same for `data-utm="off"` to disable rewriting on a specific link.

### Stripe-specific gotchas for MON-2 / MON-3

- **Stripe payment link URLs** can appear in two forms: `https://buy.stripe.com/test_xxx` and `https://buy.stripe.com/xxx` (live mode). Detect by host match, not by path prefix.
- **Stripe Checkout Session URLs** are `https://checkout.stripe.com/c/pay/cs_test_xxx#fid=...`. The `#fid=...` fragment must be preserved. Append the `client_reference_id` BEFORE the fragment.
- **`client_reference_id` is limited to 200 characters and must be alphanumeric + underscores + hyphens.** Our format `mlc_b{broadcast_id}_c{contact_id}` is fine for any reasonable id range.
- **The Stripe webhook fires `checkout.session.completed`** containing `client_reference_id`. We do NOT receive this webhook directly — the operator does, and they call `mailing-list-cli revenue add --from-stripe-webhook payload.json` (manual recording) or `mailing-list-cli revenue import --from-stripe-csv stripe.csv` (bulk).
- **Stripe's CSV export** has columns including `client_reference_id`, `amount_total`, `currency`, `id` (session id). The MON-3 importer needs to parse those, match the `mlc_b{N}_c{M}` pattern, and insert rows.

### Revenue table semantics

- **`amount_cents` is INTEGER, not REAL.** Currency precision matters; never store money as floating point. Store the smallest unit (cents for USD/EUR, satoshi for BTC, etc.) and the currency code.
- **`recorded_at` vs `paid_at`.** `recorded_at` is when WE recorded the row. The actual payment time should be a separate `paid_at` column from the Stripe event. Both are RFC3339 strings to match the rest of the schema.
- **`source` is an enum-shaped string** ('stripe', 'manual', 'paypal', 'github_sponsors', etc.). Use a CHECK constraint, OR leave it free-form for v0.4 and tighten later.
- **`external_id` is the foreign key into the source system.** For Stripe it's the checkout session id `cs_test_xxx`. For manual entries it's NULL. UNIQUE on `(source, external_id) WHERE external_id IS NOT NULL` so re-imports of the same Stripe CSV don't create duplicates.

### Content snapshot (BG-1) — the load-bearing item

- **Migration 0004 adds three nullable columns to `broadcast`:** `snapshot_html TEXT`, `snapshot_text TEXT`, `snapshot_subject TEXT`. Nullable so existing broadcasts (pre-v0.4) don't get retroactive nulls and crash on read.
- **The snapshot is written AFTER the chunk loop completes successfully**, in the final `broadcast_set_status(id, "sent", ...)` call site. Wrap it in the same final-status transition. If the broadcast fails partway, the snapshot is NOT written — that's correct, because there's no canonical "what was sent" yet.
- **For broadcasts that resume mid-flight** (v0.3.0 feature), the snapshot is written at the end of the FINAL successful run, not the first one. The snapshot reflects the template state at the time of completion.
- **Don't snapshot per-recipient.** The snapshot is per broadcast (one HTML, one text, one subject). Per-recipient variation comes from the merge data, which is reconstructable from `broadcast_recipient` + `contact` joins. Don't store 10,000 rendered HTML copies.

### General

- **The `data.data` double-nested JSON shape** discovered in `domain_list` (v0.3.0 release-day fix `78d943f`) is real for ALL email-cli list endpoints — `domain list`, `batch send` results, etc. If you add a new email-cli passthrough in v0.4 (e.g., for MON-7's coupon API later), use the same `parsed.get("data").and_then(|d| d.get("data"))` fallback pattern.
- **Smoke test is mandatory before tagging.** v0.3.0 caught a real `domain_list` shape bug DURING the smoke test. Do not skip.
- **Tests run with `--test-threads=1`** in CI (and locally for any env-mutating test). Don't write tests that depend on parallel execution semantics.
- **CLAUDE.md global rule:** ALL agents (including subagents) use Opus 4.6. Never set `model: "sonnet"` or `model: "haiku"` on Agent dispatches.
- **CLAUDE.md global rule:** "Whenever I ask you to commit to github, always commit to the **199-biotechnologies** organisation github (not personal), and always push." Already configured for this repo, but worth re-confirming if you ever clone or set a remote.
- **CLAUDE.md global rule:** Single-agent sessions don't use TaskCreate. Multi-agent sessions DO use TaskCreate. v0.4 will likely be multi-agent (subagent-driven), so use TaskCreate for the 16-task list.

### What NOT to do in v0.4

- **Don't add subscription handling.** User explicit deferral.
- **Don't add drip / sequence / automation engine.** User explicit deferral.
- **Don't add any new runtime dependency.** Hold the line at 14 crates.
- **Don't add a webhook listener.** AGENTS.md doctrine: email-cli owns the listener. The v0.2 deletion of `webhook listen` is permanent.
- **Don't bring back MJML, Handlebars, frontmatter, or any of the v0.1 template machinery.** v0.2 dropped them deliberately.
- **Don't add daemons or background processes.** `daemon` subcommand is deferred to v0.5+ pending sequence work.
- **Don't add a payment processor integration.** mailing-list-cli is a link decorator + revenue ingester, not a payment gateway. Stripe stays in the operator's control plane.
- **Don't add multi-tenant features.** Single-tenant is the design. Beehiiv-style ad networks are out of scope forever.

---

## Test count history (carry-over from session 4 handoff)

| Version | Tests | Notes |
|---|---|---|
| v0.0.3 (session 1 start) | 30 | |
| v0.0.4 Phase 3 | 101 | |
| v0.1.0 Phase 4 | 135 | |
| v0.1.1 Phase 5 | 148 | First real-Resend validation |
| v0.1.2 Phase 6 | 167 | Webhooks + reports |
| v0.1.3 Codex gap fixes | 173 | |
| v0.2.0 rearchitecture | 158 | Dropped subsystems, added new ones |
| v0.2.1 real-Resend + state fix | 158 | |
| v0.2.2 race + CI green | 159 | |
| v0.2.3 blind-test polish | 165 | |
| v0.3.0 production-grade 10k | 177 | +12 across retry/HashSet/transactions/rates/erase/resume |
| **v0.4.0 target** | **~195** | +18: link rewriter (4-6) + revenue table (4) + content snapshot (2) + sunset (2) + bounce show (2) + spam preview (2) |

---

## Session 5 entry point summary (compressed)

> Read `2026-04-09-session-4-handoff.md` then this file. v0.3.0 is current stable. v0.4 scope is **decided but the plan file is NOT yet written** — that's job #1 next session. Use `superpowers:writing-plans`. Save to `docs/plans/2026-04-09-phase-9-v0.4-operator-superpowers.md`. The 4-phase scope is in the "Active Plan" section of this handoff. After plan approval, execute with subagent-driven-development OR fall back to inline TDD as session 4 did. Hard rule: every tagged release goes through the 17-step paperfoot.com smoke test, no exceptions.
>
> **Critical scope reminders:**
> - User explicitly deferred subscription HTTP surface AND drip/automation/sequences. Do NOT include either in v0.4.
> - Monetization is operator-embeds-attribution-aware-links + revenue ingestion, NOT a marketplace. We never touch payment processing.
> - BG-1 content snapshot is the highest-leverage item in v0.4 — foundation for compliance, audit, A/B comparison, reproducibility.
> - Hold the line at 14 runtime deps. v0.4 ships zero new ones.

---

*End of session 5 planning handoff.*
