<div align="center">

<img src="./assets/og-card.png" alt="mailing-list-cli — newsletter campaigns from your terminal" width="100%" />

# Mailing List CLI

**Newsletter campaigns from your terminal. Built for AI agents.**

<br />

[![Star this repo](https://img.shields.io/github/stars/paperfoot/mailing-list-cli?style=for-the-badge&logo=github&label=%E2%AD%90%20Star%20this%20repo&color=yellow)](https://github.com/paperfoot/mailing-list-cli/stargazers)
&nbsp;&nbsp;
[![Follow @longevityboris](https://img.shields.io/badge/Follow_%40longevityboris-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/longevityboris)

<br />

[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85+-orange?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Status: v0.4.5 email-cli v0.6](https://img.shields.io/badge/Status-v0.4.5_email--cli_v0.6-orange?style=for-the-badge)](#status)
[![Built on Resend](https://img.shields.io/badge/Built_on-Resend-000000?style=for-the-badge)](https://resend.com)

---

A single Rust binary that gives an AI agent (or a human at a terminal) a real mailing list to run. Campaigns, segments, A/B tests, click tracking, double opt-in, hard-bounce auto-suppression, one-click unsubscribe — all driven by JSON-emitting commands the agent can pick up without an MCP server, schema file, or browser dashboard.

`mailing-list-cli` is the orchestration layer. It owns campaigns, segments, templates, suppression, double opt-in, A/B testing, and analytics. It does **not** talk to [Resend](https://resend.com) directly — every send, every audience operation, every webhook event flows through its sister tool [`email-cli`](https://github.com/paperfoot/email-cli), which is the sole Resend API client. Two binaries, one job each.

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

`mailing-list-cli` is the missing layer. It owns the campaign / segmentation / template / suppression / opt-in / A/B / analytics surface. For the actual SMTP-side work — sending, audience CRUD, webhook ingestion, Resend API authentication — it shells out to [`email-cli`](https://github.com/paperfoot/email-cli). An agent runs `mailing-list-cli agent-info` once, learns every command, and gets to work.

## Status

> **v0.4.5 — design-gate enforcement on top of v0.4.4.**
>
> `template create --from-file` now refuses browser/React/JSX handoffs and
> lint-error sources by default. The verdict comes from `template inspect`,
> which used to be advisory only. Override with `--force` for deliberate
> incremental editing.
>
> `broadcast send` re-runs the same design check at preflight and refuses
> error-level findings (`browser_or_jsx_source`, `browser_script_dependency`)
> before a single email-cli call. Override with `--allow-design-errors` or
> set `[guards].block_design_errors = false` in `config.toml`.
>
> The JSX heuristic now catches modern frameworks without an explicit React
> import (Next 13+, Vite, `export default function`, `<Capitalized` component
> tags) so the gate fires on the handoffs people are actually shipping in
> 2026, not just `import React from 'react'`.
>
> Everything else from v0.4.4 still applies: `--confirm`-gated sends,
> resumable batch chunks of 100, RFC 8058 one-click unsubscribe headers,
> body unsubscribe links opt out of UTM rewriting, plain-text alternatives
> preserve anchor URLs as `Label (URL)`, integrated `event poll` tracking,
> bundled agent skill via `skill install`, and the explicit email design
> rules in `agent-info` and the embedded skill.

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
| `segment create <name> --filter-json <json>` | Save a dynamic segment from a JSON AST filter |
| `segment ls` | All segments with live member counts |
| `segment members <id>` | List currently-matching contacts |

Filter expressions are a JSON AST (v0.2 dropped the string DSL — agents emit JSON directly). Example: `{"kind":"and","children":[{"kind":"atom","atom":{"type":"tag","pred":{"kind":"has","name":"vip"}}},{"kind":"atom","atom":{"type":"engagement","atom":{"kind":"opened_last","duration":{"value":30,"unit":"days"}}}}]}`. See `src/segment/ast.rs` for the full shape. Segments re-evaluate at send time.

### Templates

| Command | What it does |
|---|---|
| `template create <name> --subject "..." [--from-file <path>] [--force]` | Create a plain-HTML template (or scaffold). `--from-file` enforces the design + lint gate; `--force` overrides for deliberate non-final imports |
| `template ls` | List local templates |
| `template show <name>` | Print the raw HTML source |
| `template render <name> --with-data <file>` | Render to a JSON envelope; sendable HTML is in `.data.html` |
| `template preview <name> --with-data <file> [--out-dir <path>] [--open]` | Write preview to disk and optionally open in the browser |
| `template inspect <name>` / `template inspect --from-file <path>` | Classify stored templates or design handoff files as email-ready, lint-fixable, or browser/React prototypes that need conversion |
| `template lint <name>` | 6-rule compliance check (CAN-SPAM + size + XSS allowlist + forbidden tags) |

Templates are plain HTML with `{{ var }}` merge tags and `{{#if }}` conditionals. Triple-brace `{{{ name }}}` is an allowlisted XSS-safe escape hatch, reserved for `unsubscribe_link` and `physical_address_footer` only. The send pipeline hard-fails on any unresolved placeholder before a single email goes out.

`template render` is for machine inspection and always prints the full CLI JSON envelope. Do not pass its whole stdout to `email-cli --html`; use `template preview` for rendered files, `broadcast preview` for test emails, or extract `jq -r '.data.html'` after checking `lint_errors == 0`.

Rendered plain-text alternatives preserve links as `Label (URL)`. Generated unsubscribe anchors include `data-utm="off"` so the compliance link in the body is not rewritten with tracking parameters, while normal CTA links still receive campaign UTM tags.

`template lint` warns on fragile semantic layout tags such as `<main>` and on
unstyled text links, because email clients may collapse browser-style layout
and fall back to default blue/purple hyperlinks.

For designer handoffs and browser prototypes, run `template inspect --from-file
<path>` before importing. It detects React/JSX/Babel/script dependencies,
external CSS, style blocks, flex/grid layout, missing table structure, and
missing compliance placeholders. A `browser_prototype_needs_conversion` verdict
means the file is design direction only; convert it into standalone static
email HTML before `template create` or any broadcast send.

v0.4.5 enforces the same check at the import boundary and at the send
boundary. `template create --from-file` refuses imports whose verdict is
`browser_prototype_needs_conversion` or whose lint reports any errors
(error codes `template_create_design_blocked` / `template_create_lint_blocked`,
override with `--force`). `broadcast send` re-runs the design scanner at
preflight and refuses error-level findings (error code
`template_has_design_errors`, override with `--allow-design-errors`). The two
override flags exist because capable agents may have a deliberate reason to
land a half-finished template or to ship something that the heuristic misclassifies;
they are not for routine use.

### Broadcasts (Campaigns)

| Command | What it does |
|---|---|
| `broadcast create --template <name> --to <segment>` | Stage a broadcast |
| `broadcast preview <id> --to <email>` | Send a single test |
| `broadcast schedule <id> --at <time>` | Schedule for later |
| `broadcast send <id> --dry-run [--allow-design-errors]` | Project recipient counts and preflight checks without sending |
| `broadcast send <id> --confirm [--force-unlock] [--allow-design-errors]` | Send now, after explicit approval |
| `broadcast cancel <id>` | Cancel a scheduled broadcast |
| `broadcast ab <id> --vary subject --variants 2 --winner-by opens` | Configure A/B test |
| `broadcast ls` | Recent broadcasts and their statuses |

Large broadcasts are sent in chunks of 100 through `email-cli batch send`.
Each chunk is recorded in `broadcast_send_attempt` before the ESP call and
applied after acknowledgement, so resume skips already-sent recipients instead
of repeating them. To test a 1,000-recipient slice, target a list or segment
with those 1,000 recipients, run `broadcast send <id> --dry-run`, then send
that separate test broadcast with `--confirm`.

### Analytics

| Command | What it does |
|---|---|
| `report show <broadcast-id>` | Opens, clicks, bounces, unsubscribes, complaints, CTR |
| `report links <broadcast-id>` | Click count per link |
| `report engagement --segment <id>` | Engagement scores across a segment |
| `report deliverability` | Domain health: bounce rate, complaint rate, DMARC pass rate |

Click counting is integrated through `event poll`. Per-link CTA reporting is
recorded when the upstream `email-cli email list` row includes `click.link` or
`link`; if the upstream row only exposes `last_event=clicked`, the aggregate
`clicked_count` updates but `report links` cannot infer the clicked URL.

Tracking is a local mirror, not a direct Resend API call from this binary:

1. `mailing-list-cli webhook poll` (alias: `event poll`) asks `email-cli email list` for recent email rows.
2. `email-cli` is the only tool that talks to Resend. It returns each email id plus `last_event` and, when available, click payloads such as `click.link`.
3. `mailing-list-cli` matches the returned Resend email id to `broadcast_recipient.resend_email_id`, writes an idempotent row to the local `event` table, stores CTA link rows in `click` when the URL is present, and updates the broadcast counters.
4. Agents read the mirror with `report show <broadcast-id>`, `report links <broadcast-id>`, `report engagement`, and `report deliverability`.

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
| `webhook poll` / `event poll` | Poll `email-cli email list` for new delivery/bounce/click events and mirror them locally |

v0.2 dropped the long-running HTTP listener (`tiny_http` + Svix HMAC verifier) — running an inbound HTTP server behind NAT is hostile to a local CLI. Polling via `email-cli email list` covers the same use case without the tunneling requirement.

### Agent tooling

| Command | What it does |
|---|---|
| `agent-info` | Self-describing JSON manifest of every command, flag, and exit code |
| `skill install` | Drop the embedded skill file into Claude / Codex / Gemini paths |
| `skill status` | Show whether installed skill copies match the binary |
| `update` | Self-update from GitHub Releases |

Release automation is documented in [docs/release.md](./docs/release.md). This
is a Rust binary: `cargo` and Homebrew are the supported package channels; there
are no `uv` or `bun` artifacts.

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
- **Plain HTML + hand-rolled `{{ var }}` substitution** for templates. v0.2 dropped MJML, Handlebars, css-inline, html2text, and the YAML frontmatter variable schema — all designed-for-humans safety nets that the agent-loop preview renders unnecessary. Merge tags are Mustache-style `{{ first_name }}` (HTML-escaped) with a hard-coded triple-brace allowlist for `{{{ unsubscribe_link }}}` and `{{{ physical_address_footer }}}`. The compile pipeline is ~500 lines of Rust across `src/template/{subst,render}.rs` with 14 runtime crate dependencies total.

Built following the [agent-cli-framework](https://github.com/paperfoot/agent-cli-framework) patterns: structured JSON output (auto-detected via `IsTerminal`), semantic exit codes (`0/1/2/3/4`), self-describing `agent-info`, no interactive prompts, ever.

## Sister Project

[`email-cli`](https://github.com/paperfoot/email-cli) — the 1:1 messaging counterpart. Send, reply, draft, sync. Same conventions, same agent-friendly philosophy. Use both: `email-cli` for personal correspondence, `mailing-list-cli` for newsletters and campaigns.

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

[![Star this repo](https://img.shields.io/github/stars/paperfoot/mailing-list-cli?style=for-the-badge&logo=github&label=%E2%AD%90%20Star%20this%20repo&color=yellow)](https://github.com/paperfoot/mailing-list-cli/stargazers)
&nbsp;&nbsp;
[![Follow @longevityboris](https://img.shields.io/badge/Follow_%40longevityboris-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/longevityboris)

</div>
