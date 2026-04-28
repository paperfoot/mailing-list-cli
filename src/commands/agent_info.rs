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
            "template create <name> [--subject <text>] [--from-file <path>] [--force]": "Create a template from an HTML file or built-in scaffold. v0.4.5: --from-file imports refuse browser/JSX handoffs and lint-error sources; pass --force to override (e.g. for incremental editing)",
            "template ls": "List all templates",
            "template show <name>": "Print a template's HTML source",
            "template render <name> [--with-data <file.json>] [--raw]": "Render to a JSON envelope. The sendable HTML is `.data.html`; do not pass the whole stdout to `email-cli --html`. --raw skips automatic injection of unsubscribe-link / physical-address-footer stubs",
            "template preview <name> [--with-data <file>] [--out-dir <path>] [--open]": "Write rendered preview to disk and optionally open in browser",
            "template inspect <name> | template inspect --from-file <path> [--subject <text>]": "Assess whether a stored template or handoff file is email-ready, needs lint fixes, or is a browser/React prototype that must be converted into static table-based inline HTML before sending. Alias: template info",
            "template lint <name>": "Run the lint rule set; exit 3 on errors",
            "template rm <name> --confirm": "Delete a template",
            "broadcast create --name <n> --template <tpl> --to <list:name|segment:name>": "Stage a named broadcast in draft status",
            "broadcast preview <id> --to <email>": "Send a single test copy via email-cli send",
            "broadcast schedule <id> --at <rfc3339>": "Move a draft broadcast to scheduled",
            "broadcast send <id> --dry-run [--allow-design-errors]": "Resolve recipients, run preflight checks (incl. v0.4.5 design-error gate), render a sample. No email-cli call, no state mutation. Use this before --confirm.",
            "broadcast send <id> --confirm [--force-unlock] [--allow-design-errors]": "Run the full send pipeline. Requires explicit --confirm; use --dry-run first for projected counts. v0.4.5: refuses templates with error-level design findings (browser/JSX, embedded scripts) unless --allow-design-errors is set or [guards].block_design_errors is false in config. v0.3.1: acquires an atomic broadcast lock to prevent double-send race; --force-unlock overrides a held lock (use only when previous process is confirmed dead). Resumable — already-sent recipients are skipped",
            "broadcast resume <id> --confirm [--force-unlock] [--allow-design-errors]": "Alias of `broadcast send` with explicit resume semantics. Requires explicit --confirm. Skips already-sent recipients via the broadcast_recipient table",
            "broadcast cancel <id> --confirm": "Cancel a draft or scheduled broadcast",
            "broadcast ls [--status <s>] [--limit N]": "List recent broadcasts",
            "broadcast show <id>": "Show broadcast details including recipient + stat counts",
            "webhook poll [--reset]": "Poll `email-cli email list` for delivery/click status updates and mirror them into local SQLite (alias: `event poll`)",
            "event poll [--reset]": "Alias for `webhook poll`",
            "report show <broadcast-id>": "Per-broadcast summary from the local event mirror (delivered/bounced/opened/clicked/CTR). Run `event poll` first to sync latest Resend state via email-cli",
            "report links <broadcast-id>": "Per-link click counts for a broadcast when email-cli exposes click.link or link payloads",
            "report engagement [--list <name>|--segment <name>] [--days N]": "Engagement score across a list/segment",
            "report deliverability [--days N]": "Rolling-window bounce rate / complaint rate / domain health",
            "update [--check]": "(stub) Self-update from GitHub Releases — not yet implemented, reinstall via cargo or homebrew",
            "skill install": "Install the embedded mailing-list-cli skill into Codex, Claude, Gemini, and .agents skill roots. Override roots with MLC_SKILL_ROOTS for custom setups.",
            "skill status": "Show whether installed skill copies match the embedded skill bundled in this binary."
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
            "MLC_UNSUBSCRIBE_SECRET": "HMAC secret for one-click unsubscribe link signatures. Required for `broadcast send`. Min 16 bytes",
            "MLC_SKILL_ROOTS": "Optional colon-separated skill root override for `skill install` and `skill status`. Each root receives mailing-list-cli/SKILL.md. Mainly for tests or custom agent setups."
        },
        "config_keys": {
            "[guards].block_design_errors": "v0.4.5: when true (default), broadcast send preflight refuses templates carrying error-level design findings (browser/JSX source, embedded scripts). Set to false to fall back to v0.4.4 advisory-only behavior",
            "[guards].max_complaint_rate": "Hard limit on 30-day complaint rate enforced at preflight (default 0.003 = 0.3%, the Gmail/Yahoo block threshold)",
            "[guards].max_bounce_rate": "Hard limit on 30-day bounce rate enforced at preflight (default 0.04 = 4%)",
            "[guards].max_recipients_per_send": "Cap on a single broadcast's recipient count (default 50000)"
        },
        "depends_on": ["email-cli >= 0.6.0"],
        "tracking": {
            "sync_command": "mailing-list-cli event poll",
            "source": "email-cli email list; email-cli is the sole Resend API client",
            "read_commands": [
                "mailing-list-cli report show <broadcast-id>",
                "mailing-list-cli report links <broadcast-id>",
                "mailing-list-cli report engagement",
                "mailing-list-cli report deliverability"
            ],
            "local_tables": ["broadcast_recipient", "event", "click", "broadcast"],
            "flow": [
                "email-cli returns each email id, last_event, and optional click payload",
                "mailing-list-cli matches email id to broadcast_recipient.resend_email_id",
                "the event handler inserts an idempotent event row, updates broadcast counters, and stores click.link rows when present",
                "report commands read the local SQLite mirror"
            ],
            "limitation": "last_event is a latest-state snapshot per email. It is enough for aggregate clicked/opened/bounced counts, but per-link CTA reporting needs click.link or link payload from email-cli."
        },
        "deliverability": {
            "headers": "broadcast send includes List-Unsubscribe and List-Unsubscribe-Post headers on every recipient payload",
            "body_unsubscribe": "generated unsubscribe body anchors include inline link style plus data-utm=\"off\" so UTM rewriting does not decorate compliance links",
            "plain_text": "the plain-text MIME alternative preserves anchor destinations as `Label (URL)` so CTA and unsubscribe URLs remain visible outside HTML clients",
            "template_quality": "template lint warns on unstyled text links and fragile semantic layout tags such as <main>; use table-based wrappers and inline link styles for email clients",
            "prototype_handoff_check": "template inspect --from-file detects browser/React/JSX handoffs, script/Babel dependencies, external CSS, style blocks, flex/grid layout, missing table layout, missing compliance placeholders, and returns a conversion checklist",
            "design_gate": "v0.4.5: `template create --from-file` enforces the same design check at import — refuses verdict `browser_prototype_needs_conversion` and lint errors unless `--force` is passed. `broadcast send` re-runs the design check at preflight to catch templates that bypassed creation (or were stored before v0.4.5); refuses error-level design findings unless `--allow-design-errors` is set",
            "operator_note": "Inbox placement still depends on DNS alignment, domain reputation, recipient engagement, content, and the provider's spam model. `mailing-list-cli health` verifies the Resend sender domain, but DMARC/SPF policy tuning and reputation monitoring are outside the local SQLite state."
        },
        "template_handoff_workflow": [
            "If a designer or agent gives you a browser prototype, JSX, React app, Canvas export, or full webpage, run `mailing-list-cli template inspect --from-file <path>` before importing it.",
            "If verdict is `browser_prototype_needs_conversion`, do not send it. Convert the visual direction into standalone static email HTML.",
            "Conversion target: 100% outer presentation table, centered 600-640px inner table, inline styles, styled text links, no scripts/imports/external CSS, one clear CTA, visible compliance footer.",
            "After conversion: inspect the converted file, create the template, lint it, preview HTML and plain.txt, send broadcast preview to an internal address, dry-run, then send with --confirm.",
            "v0.4.5: `template create --from-file` and `broadcast send` will both refuse a JSX/script source by default. The error codes are `template_create_design_blocked` and `template_has_design_errors` respectively. The override flags (`--force` and `--allow-design-errors`) exist for deliberate, agent-driven workflows; do not silence them as a default."
        ],
        "template_design_rules": [
            "Build email like email, not like a webpage: use table-based outer wrappers, centered content, visible outer padding, and inline styles.",
            "Keep content width around 600-640 px and put padding on table cells so Gmail does not render text against the edge.",
            "Style every <a href> inline; unstyled links render as default blue/purple and look unfinished.",
            "Avoid semantic layout tags such as <main>, <section>, <article>, <header>, and <footer>; use tables or simple divs for email-client compatibility.",
            "Use restrained typography, clear paragraph spacing, one obvious CTA, and a visible styled unsubscribe/address footer.",
            "Before a real send: run template lint, inspect template preview output including plain.txt, then send broadcast preview to an internal address."
        ],
        "known_limitations": [
            "email-cli profile selection is database-implicit. The `[email_cli].profile` config field is used ONLY by the health-check `profile test` call. email-cli 0.6.3 has no global `--profile <name>` flag, so other commands cannot select a profile per-invocation. Multi-profile setups are ambiguous — `mailing-list-cli health` will warn if more than one email-cli profile is configured. Track the upstream issue at paperfoot/email-cli.",
            "30-day complaint/bounce rate guards in `broadcast send` preflight are computed from the local `event` table, which is populated by `webhook poll` paginating `email-cli email list` by email ID and reading `last_event` per row. This means later state changes on already-seen emails are invisible, and only the most recent event per email is recorded. Treat the rates as approximate. The guards still fire (and are still useful safety nets), but operators should not over-trust the exact percentages. Source: GPT Pro F3.2 from 2026-04-09 hardening review. See docs/email-cli-gap-analysis.md.",
            "`report show` can count clicked emails from `last_event=clicked`; `report links` needs click link payload (`click.link` or `link`) from email-cli. The poll path stores it when present, but if upstream only exposes last_event then per-link CTA rows remain empty while clicked_count still increments."
        ],
        "status": concat!(
            "v",
            env!("CARGO_PKG_VERSION"),
            " — design-gate enforcement: `template create --from-file` refuses browser/JSX handoffs and lint-error sources by default (override with --force); `broadcast send` re-runs the design check at preflight and refuses error-level design findings unless --allow-design-errors is set. Tighter JSX heuristics catch modern frameworks without an explicit React import. Single-source design-rule scanner shared by `template inspect`, `template create`, and `broadcast send` preflight."
        )
    });
    println!("{}", serde_json::to_string_pretty(&manifest).unwrap());
}
