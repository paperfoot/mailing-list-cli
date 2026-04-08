<div align="center">

<img src="./assets/og-card.png" alt="mailing-list-cli — newsletter campaigns from your terminal" width="100%" />

# Mailing List CLI

**Newsletter campaigns from your terminal. Built for AI agents.**

<br />

[![Star this repo](https://img.shields.io/github/stars/199-biotechnologies/mailing-list-cli?style=for-the-badge&logo=github&label=%E2%AD%90%20Star%20this%20repo&color=yellow)](https://github.com/199-biotechnologies/mailing-list-cli/stargazers)
&nbsp;&nbsp;
[![Follow @longevityboris](https://img.shields.io/badge/Follow_%40longevityboris-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/longevityboris)

<br />

[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85+-orange?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Status: v0.1.2 email-cli v0.6](https://img.shields.io/badge/Status-v0.1.2_email--cli_v0.6-orange?style=for-the-badge)](#status)
[![Built on Resend](https://img.shields.io/badge/Built_on-Resend-000000?style=for-the-badge)](https://resend.com)

---

A single Rust binary that gives an AI agent (or a human at a terminal) a real mailing list to run. Campaigns, segments, A/B tests, click tracking, double opt-in, hard-bounce auto-suppression, one-click unsubscribe — all driven by JSON-emitting commands the agent can pick up without an MCP server, schema file, or browser dashboard.

`mailing-list-cli` is the orchestration layer. It owns campaigns, segments, templates, suppression, double opt-in, A/B testing, and analytics. It does **not** talk to [Resend](https://resend.com) directly — every send, every audience operation, every webhook event flows through its sister tool [`email-cli`](https://github.com/199-biotechnologies/email-cli), which is the sole Resend API client. Two binaries, one job each.

Think Beehiiv or MailChimp, except it lives at `~/.local/bin/mailing-list-cli` and an agent uses it the same way you'd use `git`.

[Why](#why-this-exists) | [Status](#status) | [Planned Commands](#planned-commands) | [Architecture](#architecture) | [Sister Project](#sister-project) | [Research](#research)

</div>

## Why This Exists

AI agents can already send single emails. Running a mailing list is a different sport.

Sending one newsletter to fifty thousand people involves things one-off email tools never touch: deduplicating against a global suppression list, honoring unsubscribes within minutes, watching the soft-bounce counter, throttling the burst so the ESP doesn't suspend you, A/B testing two subject lines on a five-percent slice and promoting the winner, segmenting by tag and engagement, signing the one-click unsubscribe header per RFC 8058, and writing every send result back to local state so the next campaign knows who not to email.

The existing options for an agent are bad:

- **MailChimp / Beehiiv / Klaviyo** — browser-first. Their APIs exist but were designed for Zapier and websites, not for an agent shelling out forty times per second.
- **Resend's own dashboard** — fine for humans, but the Broadcasts API alone doesn't cover the full list-management surface (no bulk import, no programmatic suppression list, no double opt-in workflow, no A/B testing, no segments-by-engagement).
- **MCP servers wrapping the above** — a 32× context overhead per call versus the same operation as a CLI command, and the agent has to learn a new tool schema for every platform.

`mailing-list-cli` is the missing layer. It owns the campaign / segmentation / template / suppression / opt-in / A/B / analytics surface. For the actual SMTP-side work — sending, audience CRUD, webhook ingestion, Resend API authentication — it shells out to [`email-cli`](https://github.com/199-biotechnologies/email-cli). An agent runs `mailing-list-cli agent-info` once, learns every command, and gets to work.

## Status

> **v0.0.3 — migrated to `email-cli` v0.6 (audiences → segments).**
>
> The `email-cli` team shipped all three of our architectural asks within a day of the [gap analysis](./docs/email-cli-gap-analysis.md) being committed: `contact create --properties <json>`, `email list --after <id>`, and the `broadcast` noun. They also retired the deprecated `audience` noun. `mailing-list-cli` v0.0.3 migrated to the new surface: lists now back onto Resend segments instead of audiences, contacts live in the flat `/contacts` namespace, and the gap analysis is marked **FULFILLED**.
>
> Lists, contacts, and tags work end-to-end against real `email-cli` v0.6.2+. 30 tests pass, clippy clean, CI green. Templates, segments, broadcasts, suppression, opt-in, A/B testing, and analytics land in subsequent v0.0.x and v0.1.x releases per the [pinned roadmap issue](https://github.com/199-biotechnologies/mailing-list-cli/issues/1).
>
> Read [the research](./research), the [design spec](./docs/specs/2026-04-07-mailing-list-cli-design.md), or the [Phase 1 plan](./docs/plans/2026-04-07-phase-1-foundations.md) to see what we're building toward. Star the repo to follow along.

## Planned Commands

Synthesized from the research swarm. Directional, not final — every entry below is grounded in a feature real list operators rely on day-to-day.

### Lists, Contacts, Tags

| Command | What it does |
|---|---|
| `list create <name>` | Create a list (Resend audience) |
| `list ls` | Show all lists with subscriber counts |
| `contact add <email> --list <id>` | Add a contact |
| `contact import <file.csv> --list <id>` | Bulk import with rate-limit-aware chunking |
| `contact tag <email> <tag>` | Tag a contact |
| `contact ls --filter <expr>` | Filter contacts by tag, list, status, engagement |
| `contact erase <email>` | GDPR hard-delete (PII removed, suppression entry retained) |

### Segments

| Command | What it does |
|---|---|
| `segment create <name> --filter <expr>` | Save a dynamic segment from a filter expression |
| `segment ls` | All segments with live member counts |
| `segment members <id>` | List currently-matching contacts |

Filter expressions are boolean: `tag:vip AND opened_last:30d AND NOT bounced`. Segments re-evaluate at send time so a "last 30 days engaged" segment is always fresh.

### Templates

| Command | What it does |
|---|---|
| `template create <name>` | Scaffold a new MJML template with merge tag schema |
| `template ls` | List local templates |
| `template show <name>` | Render to HTML for inspection |
| `template lint <name>` | Validate MJML, check merge tag schema, render preview |
| `template guidelines` | Print the agent authoring guide (rules, examples, gotchas) |

Templates live in a local SQLite store. The CLI ships explicit guidelines an agent loads before authoring, written so an LLM produces output that actually renders in Outlook desktop.

### Broadcasts (Campaigns)

| Command | What it does |
|---|---|
| `broadcast create --template <name> --to <segment>` | Stage a broadcast |
| `broadcast preview <id> --to <email>` | Send a single test |
| `broadcast schedule <id> --at <time>` | Schedule for later |
| `broadcast send <id>` | Send now |
| `broadcast cancel <id>` | Cancel a scheduled broadcast |
| `broadcast ab <id> --vary subject --variants 2 --winner-by opens` | Configure A/B test |
| `broadcast ls` | Recent broadcasts and their statuses |

### Analytics

| Command | What it does |
|---|---|
| `report show <broadcast-id>` | Opens, clicks, bounces, unsubscribes, complaints, CTR |
| `report links <broadcast-id>` | Click count per link |
| `report engagement --segment <id>` | Engagement scores across a segment |
| `report deliverability` | Domain health: bounce rate, complaint rate, DMARC pass rate |

### Compliance & Hygiene

| Command | What it does |
|---|---|
| `optin start <email> --list <id>` | Send a double opt-in confirmation |
| `optin verify <token>` | Confirm an opt-in |
| `unsubscribe <email>` | Honor an unsubscribe (writes to global suppression) |
| `suppression ls` | View the global suppression list |
| `suppression import <file>` | Import suppressions from another platform |
| `dnscheck <domain>` | Verify SPF / DKIM / DMARC alignment before first send |

### Webhook ingestion

| Command | What it does |
|---|---|
| `webhook listen --port <n>` | Run a local receiver that mirrors Resend events into local state |
| `webhook backfill` | Backfill recent events from Resend's API |

### Agent tooling

| Command | What it does |
|---|---|
| `agent-info` | Self-describing JSON manifest of every command, flag, and exit code |
| `skill install` | Drop the embedded skill file into Claude / Codex / Gemini paths |
| `update` | Self-update from GitHub Releases |

## Architecture

Three layers, each replaceable.

```
┌──────────────────────────────────────────┐
│             Your Agent / You             │
│         (Claude, Codex, Gemini)          │
└────────────────┬─────────────────────────┘
                 │  CLI commands, JSON in/out
                 ▼
┌──────────────────────────────────────────┐
│             mailing-list-cli             │
│   campaigns · segments · A/B · opt-in    │
│   suppression · analytics · templates    │
└────────────┬─────────────────┬───────────┘
             │                 │
             │  shells out     │  reads/writes
             │  for sending    │  local state
             ▼                 ▼
   ┌──────────────────┐  ┌────────────┐
   │     email-cli    │  │   SQLite   │
   │ • Resend API     │  │ templates  │
   │ • send / batch   │  │ campaigns  │
   │ • audiences      │  │ suppression│
   │ • contacts       │  │ events     │
   │ • events / hooks │  │ optin tok. │
   └─────────┬────────┘  └────────────┘
             │
             ▼
       ┌──────────┐
       │  Resend  │
       └──────────┘
```

- **`mailing-list-cli` is the orchestration layer.** It composes campaigns, computes segments, renders templates, enforces suppression, runs A/B tests, and aggregates analytics. It has zero Resend code.
- **`email-cli` is the transport layer.** It is the only binary that talks to Resend's API. `mailing-list-cli` shells out to it for every send, every audience operation, and every event read.
- **Local SQLite** stores the things `email-cli` doesn't track: templates, campaign metadata, the suppression list, double opt-in tokens, segment definitions, engagement aggregates, and a mirror of recent events polled from `email-cli`.
- **MJML + Handlebars** for templates, compiled in-process via the [`mrml`](https://crates.io/crates/mrml) Rust crate — no Node, no external runtime. Merge tags are Mustache-style `{{ first_name }}` because that's the syntax LLMs author most reliably. Templates render to HTML locally before being passed to `email-cli` as a send body.

Built following the [agent-cli-framework](https://github.com/199-biotechnologies/agent-cli-framework) patterns: structured JSON output (auto-detected via `IsTerminal`), semantic exit codes (`0/1/2/3/4`), self-describing `agent-info`, no interactive prompts, ever.

## Sister Project

[`email-cli`](https://github.com/199-biotechnologies/email-cli) — the 1:1 messaging counterpart. Send, reply, draft, sync. Same conventions, same agent-friendly philosophy. Use both: `email-cli` for personal correspondence, `mailing-list-cli` for newsletters and campaigns.

## Research

Five research dossiers ground the design. Read them in [/research](./research):

1. [Modern creator newsletters](./research/01-modern-creator-newsletters.md) — Beehiiv, Buttondown, Substack
2. [Marketing platforms](./research/02-marketing-platforms.md) — MailChimp, MailerLite, Kit
3. [Resend native capabilities](./research/03-resend-native.md) — what's already there vs the gap to fill
4. [Deliverability and compliance](./research/04-deliverability-compliance.md) — the non-negotiables for safe scale
5. [Email templates for agents](./research/05-templates.md) — format choice, merge syntax, authoring guidelines

## Contributing

The spec isn't written yet. If you want to shape it, open a discussion or comment on the research files. Once the binary lands, contributions to commands, tests, and docs are welcome.

## License

MIT — see [LICENSE](LICENSE).

---

<div align="center">

Built by [Boris Djordjevic](https://github.com/longevityboris) at [Paperfoot AI](https://paperfoot.com)

<br />

**If this is useful or interesting:**

[![Star this repo](https://img.shields.io/github/stars/199-biotechnologies/mailing-list-cli?style=for-the-badge&logo=github&label=%E2%AD%90%20Star%20this%20repo&color=yellow)](https://github.com/199-biotechnologies/mailing-list-cli/stargazers)
&nbsp;&nbsp;
[![Follow @longevityboris](https://img.shields.io/badge/Follow_%40longevityboris-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/longevityboris)

</div>
