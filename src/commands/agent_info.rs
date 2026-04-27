use serde_json::json;

/// Print the agent-info manifest as raw JSON. Always JSON, never wrapped in the envelope.
pub fn run() {
    let manifest = json!({
        "name": "mailing-list-cli",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Newsletter and mailing list management from your terminal. Built for AI agents on top of email-cli.",
        "commands": {
            "agent-info": "Machine-readable capability manifest (this output)",
            "health": "Run a system health check (config, db reachable, email-cli on PATH, email-cli single-profile uniqueness, sender_domain_verified, db schema version current)",
            "list create <name> [--description <text>]": "Create a list (backed by a Resend segment via email-cli)",
            "list ls": "List all lists with subscriber counts",
            "list show <id>": "Show one list's details",
            "contact add <email> --list <id> [--first-name F --last-name L --field key=val ...]": "Add a contact to a list",
            "contact ls [--list <id>] [--filter-json <json>|--filter-json-file <path>] [--limit N] [--cursor C]": "List/filter contacts",
            "contact show <email>": "Show a contact's full details (tags, fields, list memberships)",
            "contact tag <email> <tag>": "Apply a tag to a contact",
            "contact untag <email> <tag>": "Remove a tag from a contact",
            "contact set <email> <field> <value>": "Set a typed custom field value",
            "contact import <file.csv> --list <id> [--unsafe-no-consent]": "Bulk-import contacts from CSV (5 req/sec rate limit, idempotent replay)",
            "contact erase <email> --confirm": "Permanently erase a contact (GDPR Article 17). Inserts a `gdpr_erasure` suppression tombstone, then deletes the contact row; FK cascades clean child tables. Atomic transaction so the email is never momentarily absent from both",
            "tag ls": "List all tags with member counts",
            "tag rm <name> --confirm": "Delete a tag",
            "field create <key> --type <text|number|date|bool|select> [--options a,b,c]": "Create a typed custom field",
            "field ls": "List all custom fields",
            "field rm <key> --confirm": "Delete a custom field",
            "segment create <name> --filter-json <json> | --filter-json-file <path>": "Save a dynamic segment from a JSON SegmentExpr",
            "segment ls": "List all segments with member counts",
            "segment show <name>": "Show a segment's filter + 10 sample members",
            "segment members <name> [--limit N] [--cursor C]": "List contacts currently matching the segment",
            "segment rm <name> --confirm": "Delete a segment definition",
            "template create <name> [--subject <text>] [--from-file <path>]": "Create a template from an HTML file or built-in scaffold",
            "template ls": "List all templates",
            "template show <name>": "Print a template's HTML source",
            "template render <name> [--with-data <file.json>] [--raw]": "Render to a JSON envelope. The sendable HTML is `.data.html`; do not pass the whole stdout to `email-cli --html`. --raw skips automatic injection of unsubscribe-link / physical-address-footer stubs",
            "template preview <name> [--with-data <file>] [--out-dir <path>] [--open]": "Write rendered preview to disk and optionally open in browser",
            "template lint <name>": "Run the lint rule set; exit 3 on errors",
            "template rm <name> --confirm": "Delete a template",
            "broadcast create --name <n> --template <tpl> --to <list:name|segment:name>": "Stage a named broadcast in draft status",
            "broadcast preview <id> --to <email>": "Send a single test copy via email-cli send",
            "broadcast schedule <id> --at <rfc3339>": "Move a draft broadcast to scheduled",
            "broadcast send <id> --dry-run": "Resolve recipients, run preflight checks, and render a sample without calling email-cli or modifying broadcast state. Use this before --confirm.",
            "broadcast send <id> --confirm [--force-unlock]": "Run the full send pipeline. Requires explicit --confirm; use --dry-run first for projected counts. v0.3.1: acquires an atomic broadcast lock to prevent double-send race; --force-unlock overrides a held lock (use only when previous process is confirmed dead). Resumable — already-sent recipients are skipped",
            "broadcast resume <id> --confirm [--force-unlock]": "Alias of `broadcast send` with explicit resume semantics. Requires explicit --confirm. Skips already-sent recipients via the broadcast_recipient table",
            "broadcast cancel <id> --confirm": "Cancel a draft or scheduled broadcast",
            "broadcast ls [--status <s>] [--limit N]": "List recent broadcasts",
            "broadcast show <id>": "Show broadcast details including recipient + stat counts",
            "webhook poll [--reset]": "Poll email-cli for delivery status updates (alias: `event poll`)",
            "event poll [--reset]": "Alias for `webhook poll`",
            "report show <broadcast-id>": "Per-broadcast summary (delivered/bounced/opened/clicked/CTR)",
            "report links <broadcast-id>": "Per-link click counts for a broadcast",
            "report engagement [--list <name>|--segment <name>] [--days N]": "Engagement score across a list/segment",
            "report deliverability [--days N]": "Rolling-window bounce rate / complaint rate / domain health",
            "update [--check]": "(stub) Self-update from GitHub Releases — not yet implemented, reinstall via cargo or homebrew",
            "skill install": "(stub) Install skill files into Claude / Codex / Gemini paths — not yet implemented",
            "skill status": "(stub) Show which platforms have the skill installed — not yet implemented"
        },
        "flags": {
            "--json": "Force JSON output even on a TTY (global flag, applies to every subcommand). Without --json, output mode is auto-detected via IsTerminal: TTY → human, pipe/redirect → JSON envelope"
        },
        "exit_codes": {
            "0": "Success",
            "1": "Transient error (IO, network, email-cli unavailable, broadcast lock held, email-cli timeout) -- retry",
            "2": "Config error (missing email-cli, missing physical_address, db schema too new, etc) -- fix setup",
            "3": "Bad input (invalid args) -- fix arguments",
            "4": "Rate limited (Resend rate limit, batch_send retries exhausted) -- wait and retry"
        },
        "envelope": {
            "version": "1",
            "success": "{ version, status, data }",
            "error": "{ version, status, error: { code, message, suggestion } }"
        },
        "config_path": "~/.config/mailing-list-cli/config.toml",
        "state_path": "~/.local/share/mailing-list-cli/state.db",
        "auto_json_when_piped": true,
        "env_prefix": "MLC_",
        "env_vars": {
            "MLC_EMAIL_CLI_TIMEOUT_SEC": "Timeout in seconds for any single email-cli subprocess invocation. Default: 120. On timeout, the child is killed via SIGKILL and the call returns `email_cli_timeout` (transient, feeds the existing retry path)",
            "MLC_UNSUBSCRIBE_SECRET": "HMAC secret for one-click unsubscribe link signatures. Required for `broadcast send`. Min 16 bytes"
        },
        "depends_on": ["email-cli >= 0.6.0"],
        "known_limitations": [
            "email-cli profile selection is database-implicit. The `[email_cli].profile` config field is used ONLY by the health-check `profile test` call. email-cli 0.6.3 has no global `--profile <name>` flag, so other commands cannot select a profile per-invocation. Multi-profile setups are ambiguous — `mailing-list-cli health` will warn if more than one email-cli profile is configured. Track the upstream issue at paperfoot/email-cli.",
            "30-day complaint/bounce rate guards in `broadcast send` preflight are computed from the local `event` table, which is populated by `webhook poll` paginating `email-cli email list` by email ID and reading `last_event` per row. This means later state changes on already-seen emails are invisible, and only the most recent event per email is recorded. Treat the rates as approximate. The guards still fire (and are still useful safety nets), but operators should not over-trust the exact percentages. Source: GPT Pro F3.2 from 2026-04-09 hardening review. See docs/email-cli-gap-analysis.md.",
            "`report show` can count clicked emails from `last_event=clicked`; `report links` needs click link payload (`click.link` or `link`) from email-cli. The poll path stores it when present, but if upstream only exposes last_event then per-link CTA rows remain empty while clicked_count still increments."
        ],
        "status": "v0.4.0 — operator superpowers: content snapshots at send time (compliance audit trail), broadcast send --dry-run (projected counts without sending), explicit broadcast send/resume --confirm approval, UTM link auto-injection on every outbound <a> tag (utm_source/medium/campaign), Stripe client_reference_id auto-injection on buy.stripe.com/checkout.stripe.com URLs, revenue tracking (revenue add/ls/import --from-stripe-csv, report revenue, report ltv), idempotent broadcast create (detect-and-fail on duplicate names), subscriber integration docs. Built on v0.3.x hardening foundations (lock CAS, write-ahead attempt log, subprocess timeout, schema check)."
    });
    println!("{}", serde_json::to_string_pretty(&manifest).unwrap());
}
