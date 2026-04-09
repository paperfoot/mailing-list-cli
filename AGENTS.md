# Agents Guide

`mailing-list-cli` is a single Rust binary for running a mailing list from the
terminal. Campaigns, segments, templates, suppression, analytics, webhook
ingestion — all exposed as JSON-emitting subcommands so an agent can drive
them without an MCP server, schema file, or browser dashboard.

## Current state

- **Version**: v0.3.1 (emergency hardening on top of v0.3.0 production-grade 10k foundations)
- **Research**: see [/research](./research) for the five dossiers that informed the original design
- **Recent plans**:
  - [v0.2 rearchitecture](./docs/plans/2026-04-08-phase-7-v0.2-rearchitecture.md) (shipped as v0.2.0)
  - [v0.3 production-grade 10k](./docs/plans/2026-04-09-phase-8-v0.3-production-grade-10k.md) (shipped as v0.3.0)
  - [v0.3.1 emergency hardening](./docs/plans/2026-04-09-phase-9-v0.3.1-emergency-hardening.md) (shipped as v0.3.1)

## Production hardening (v0.3.x)

What "production-grade" means in this codebase:

- **Send pipeline reliability**: 429/5xx retry with exponential backoff [500ms, 1s, 2s, 4s], up to 4 retries; per-chunk DB transactions; preloaded suppression `HashSet` for O(1) lookups; resumable sends with atomic broadcast lock CAS (no double-send race even on concurrent invocation); **v0.3.2 write-ahead `broadcast_send_attempt` table** that records ESP acceptance BEFORE the local recipient UPDATE, so a crash between the two cannot cause duplicate sends on resume — it reconciles from the stored response.
- **Subprocess safety**: every `email-cli` call has a 120-second default timeout (`MLC_EMAIL_CLI_TIMEOUT_SEC` env var to override). Hung subprocesses are killed via SIGKILL and surfaced as `email_cli_timeout` transient errors that feed the existing retry path.
- **Schema safety**: `Db::open` fails fast (exit code 2, `db_schema_too_new`) when the on-disk schema version is newer than what this binary supports — no more silent column-mismatch errors after a binary downgrade.
- **Unsubscribe link security**: `MLC_UNSUBSCRIBE_SECRET` is required (v0.3.2) — `broadcast send` refuses to sign tokens with a dev fallback. HMAC-SHA256.
- **GDPR compliance**: `contact erase --confirm` writes a `gdpr_erasure` suppression tombstone before deleting the contact row (atomic transaction; the email is never momentarily absent from both).
- **Operator escape hatches**: `broadcast send <id> --force-unlock` overrides a held send lock when the previous process is confirmed dead (use only after `ps aux | grep mailing-list-cli`).
- **Honest limitations**: the 30-day complaint/bounce rate guards in `broadcast send` preflight are computed from the local `event` table, which is populated by `webhook poll` paginating `email-cli email list` by email ID and reading only `last_event` per row. The guards still fire and remain useful safety nets, but the exact percentages are best-effort approximations — see `agent-info → known_limitations` and the docstring on `historical_send_rates`. Proper fix is v0.5+ pending an upstream change to email-cli or a rolling-window snapshot diff. Source: GPT Pro F3.2 from 2026-04-09 hardening review.

## Conventions

This project follows the [agent-cli-framework](https://github.com/199-biotechnologies/agent-cli-framework) patterns:

- Structured JSON output, auto-detected via `IsTerminal`
- Semantic exit codes: `0` success, `1` transient (retry), `2` config (fix setup), `3` bad input, `4` rate limited
- Self-describing via `agent-info` — one command returns the full capability manifest
- **No interactive prompts, ever.** v0.2 removed the v0.1 `template edit` command because it violated this invariant; agents use `Write`/`Edit` tools directly on files passed via `template create --from-file`.
- Local-first state under `~/.local/share/mailing-list-cli/`
- Config under `~/.config/mailing-list-cli/config.toml`
- Cache under `~/.cache/mailing-list-cli/` (always safe to `rm -rf`)
- **Integrated preview.** `template preview <name>` writes rendered HTML to disk and optionally opens it in the default browser. This is the core iteration primitive — it replaces every "catch the mistake upfront" safety net the v0.1 system had.

## Discovery

```bash
mailing-list-cli agent-info
```

Returns a JSON manifest of every subcommand, every flag, every exit code. No documentation drift, no MCP server, no schema file an agent has to load up front.

## Required dependency: email-cli

`mailing-list-cli` does **not** talk to Resend directly. Every send, every audience operation, every event read goes through [`email-cli`](https://github.com/199-biotechnologies/email-cli), which is the sole Resend API client. Both binaries must be on `$PATH`.

This split exists so neither tool has to do the other's job:

- `email-cli` owns the Resend API surface, accounts, profiles, transports, the inbox, the webhook listener.
- `mailing-list-cli` owns campaigns, segmentation, templates, suppression, double opt-in, A/B testing, analytics.

For an agent: use `email-cli` for personal correspondence, `mailing-list-cli` for newsletters and campaigns. They cooperate on the same Resend account but each one stays in its lane.
