<div align="center">

# Mailing List CLI

**Run your newsletter from the terminal. One Rust binary an AI agent can drive end to end.**

[![Star this repo](https://img.shields.io/github/stars/paperfoot/mailing-list-cli?style=for-the-badge&logo=github&label=%E2%AD%90%20Star%20this%20repo&color=yellow)](https://github.com/paperfoot/mailing-list-cli/stargazers)
&nbsp;&nbsp;
[![Follow @longevityboris](https://img.shields.io/badge/Follow_%40longevityboris-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/longevityboris)

<br />

[![crates.io](https://img.shields.io/crates/v/mailing-list-cli?style=for-the-badge&logo=rust&logoColor=white)](https://crates.io/crates/mailing-list-cli)
[![CI](https://img.shields.io/github/actions/workflow/status/paperfoot/mailing-list-cli/ci.yml?style=for-the-badge&label=CI)](https://github.com/paperfoot/mailing-list-cli/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](LICENSE)
[![Rust](https://img.shields.io/crates/msrv/mailing-list-cli?style=for-the-badge&logo=rust&logoColor=white&label=Rust)](https://www.rust-lang.org/)

Email marketing tooling was built for humans at dashboards. Agents get browser UIs they can't click, MCP servers that spend context on every call, and raw APIs with no guard rails against a bad send. `mailing-list-cli` puts the whole job in one binary: subscribers, segments, templates, broadcasts, analytics, and compliance, with JSON output on every command.

[Quick Start](#quick-start) · [How It Works](#how-it-works) · [Features](#features) · [Built for AI Agents](#built-for-ai-agents) · [Roadmap](#roadmap) · [Docs](#documentation)

</div>

## Why This Exists

Sending one email is easy; running a list is not. A real newsletter needs suppression checks, consent records, unsubscribe law, segments, and batch sends that survive a crash. Mailchimp and beehiiv put all of that behind dashboards, so this is the open-source alternative shaped like the tool agents already know: a CLI on top of [Resend](https://resend.com).

## Quick Start

Pick an install channel (Rust 1.85+ for source builds):

```sh
cargo install mailing-list-cli
# or
brew install 199-biotechnologies/tap/mailing-list-cli
# or unreleased main
cargo install --git https://github.com/paperfoot/mailing-list-cli --locked
```

Sending goes through [email-cli](https://github.com/paperfoot/email-cli), the sister binary that owns the Resend connection. Put both on `$PATH`, give email-cli your Resend key, and verify the wiring:

```sh
mailing-list-cli health
```

Then run your first campaign:

```sh
mailing-list-cli list create "Newsletter"
mailing-list-cli contact add alice@example.com --list 1 --first-name Alice
mailing-list-cli template create welcome --subject "Welcome, {{ first_name }}"
mailing-list-cli broadcast create --name launch --template welcome --to list:Newsletter
mailing-list-cli broadcast send 1 --dry-run     # recipient counts + preflight, sends nothing
mailing-list-cli broadcast send 1 --confirm     # the real send
```

`template create` scaffolds a compliant starting template; add `--from-file your.html` to import your own.

## How It Works

Two binaries, one job each. `mailing-list-cli` owns list logic and local state, and shells out to [email-cli](https://github.com/paperfoot/email-cli), the sole Resend API client. Zero Resend code lives in this crate.

```
┌───────────────────────────┐
│      you / your agent     │
│  (Claude, Codex, Gemini)  │
└─────────────┬─────────────┘
              │  commands in · JSON envelopes + exit codes out
              ▼
┌───────────────────────────┐      ┌──────────────────────────────┐
│      mailing-list-cli     │◀────▶│ SQLite: contacts, segments,  │
│  lists · templates        │      │ templates, broadcasts,       │
│  broadcasts · analytics   │      │ suppression, events, revenue │
└─────────────┬─────────────┘      └──────────────────────────────┘
              │  shells out for every send, audience op, event read
              ▼
┌───────────────────────────┐
│         email-cli         │──────▶  Resend API
└───────────────────────────┘
```

The agent loop:

| Step | Mechanism | What happens |
|---|---|---|
| Discover | `agent-info` | One JSON manifest lists every command, flag, exit code, and known limitation |
| Act | any command | Structured JSON out; errors carry a stable `code` plus a `suggestion` the agent can act on |
| Branch | exit codes | `0` continue · `1` retry · `2` fix config · `3` fix input · `4` back off |
| Stay safe | guard rails | Real sends need `--confirm`, imports need consent, templates pass a design + lint gate |

## Features

| Area | What you get |
|---|---|
| Audience | Lists, contacts, tags, custom fields, dynamic segments from JSON filter expressions |
| Templates | Plain HTML with `{{ var }}` merge tags, a 6-rule lint, and `template inspect` to classify designer or React handoffs before they cost you a send |
| Design gate | `template create --from-file` and `broadcast send` preflight refuse browser/JSX prototypes and lint errors; explicit override flags exist for deliberate exceptions |
| Broadcasts | Draft, test-send, schedule, send. `--dry-run` projects counts, `--confirm` gates the real thing, atomic locks prevent double sends, resume skips already-sent recipients in chunks of 100 |
| Analytics | Opens, clicks, bounces, per-link clicks, engagement scores, rolling deliverability windows via `report` |
| Revenue | Attribute payments to broadcasts and contacts, import Stripe Checkout CSVs, rank subscribers by lifetime value |
| GDPR | `contact erase --confirm` deletes PII atomically and leaves a suppression tombstone so the address can't sneak back in |
| Hosted unsubscribe | RFC 8058 one-click headers point at the SharpClap companion; `unsubscribe sync` mirrors hosted opt-outs into local suppression |

Full command reference: run `mailing-list-cli agent-info`.

### Hosted unsubscribe

Unsubscribe links land on SharpClap ([`web/`](./web), Next.js on Vercel), which serves the public `/u/<token>` page and the RFC 8058 one-click POST endpoint a local CLI can't. Run `unsubscribe sync` before real sends to pull those opt-outs into local suppression.

```toml
[unsubscribe]
public_url = "https://sharpclap.com/u"
secret_env = "MLC_UNSUBSCRIBE_SECRET"
```

`MLC_UNSUBSCRIBE_SECRET` must match the web app's secret. The sync key is read from `MLC_UNSUBSCRIBE_SYNC_KEY`, falling back to `SYNC_API_KEY`.

## Built for AI Agents

`mailing-list-cli` is agent-native email marketing: no MCP server, no schema file, no interactive prompts, ever. The binary describes itself, and every response is machine-parseable.

**Discovery in one call.** `mailing-list-cli agent-info` returns the full capability manifest: every command, every flag, every exit code, plus template design rules and known limitations. No documentation drift.

**JSON envelopes.** Output is structured JSON, auto-detected when piped (force with `--json`). Every error carries a machine-readable `code` and a `suggestion`; a suggestion that doesn't work is treated as a P0 bug.

**Semantic exit codes.** The agent branches on the code instead of parsing prose:

| Exit code | Meaning | Agent response |
|---|---|---|
| `0` | Success | Continue |
| `1` | Transient error | Retry with backoff |
| `2` | Config error | Fix setup (email-cli missing, bad key); don't retry |
| `3` | Bad input | Fix arguments; don't retry |
| `4` | Rate limited | Back off, then retry |

**Embedded skill.** `mailing-list-cli skill install` writes the bundled skill into Claude, Codex, and Gemini skill roots, so your agent starts with the workflow instead of the man page. `skill status` reports drift between installed copies and the binary.

## Roadmap

**v0.5 (planned, not shipped):** a deliverability guard with a send-ramp governor and a bounce/complaint circuit breaker — [plan](./docs/plans/2026-07-19-v0.5-deliverability-guard.md).

## Documentation

| Doc | Covers |
|---|---|
| [docs/release.md](./docs/release.md) | Install channels, release automation, required secrets |
| [docs/subscriber-integration.md](./docs/subscriber-integration.md) | Wiring signup forms (Next.js, Express, Flask) to the CLI |
| [web/README.md](./web/README.md) | SharpClap, the hosted unsubscribe companion app |
| [assets/mailing-list-cli-skill.md](./assets/mailing-list-cli-skill.md) | Source of the embedded agent skill (`skill install`) |
| [research/](./research) | Five dossiers on newsletter platforms, deliverability, and email templates that shaped the design |

## Contributing

Bug reports and focused PRs are welcome. Run `cargo test` and `cargo clippy` before pushing, and update `agent-info` whenever you change the command surface. Details in [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE).

---

<div align="center">

Built by [Boris Djordjevic](https://github.com/longevityboris) at [Paperfoot AI](https://paperfoot.com)

**If this is useful to you:**

[![Star this repo](https://img.shields.io/github/stars/paperfoot/mailing-list-cli?style=for-the-badge&logo=github&label=%E2%AD%90%20Star%20this%20repo&color=yellow)](https://github.com/paperfoot/mailing-list-cli/stargazers)
&nbsp;&nbsp;
[![Follow @longevityboris](https://img.shields.io/badge/Follow_%40longevityboris-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/longevityboris)

</div>
