use serde_json::json;

/// Print the agent-info manifest as raw JSON. Always JSON, never wrapped in the envelope.
pub fn run() {
    let manifest = json!({
        "name": "mailing-list-cli",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Newsletter and mailing list management from your terminal. Built for AI agents on top of email-cli.",
        "commands": {
            "agent-info": "Machine-readable capability manifest (this output)",
            "health": "Run a system health check (config, email-cli on PATH, sender_domain_verified, db reachable, schema_version current)",
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
            "template render <name> [--with-data <file.json>] [--with-placeholders] [--raw]": "Compile template → JSON { subject, html, text }. --raw skips automatic injection of unsubscribe-link / physical-address-footer stubs (for advanced template authors who provide their own)",
            "template preview <name> [--with-data <file>] [--out-dir <path>] [--open]": "Write rendered preview to disk and optionally open in browser",
            "template lint <name>": "Run the lint rule set; exit 3 on errors",
            "template rm <name> --confirm": "Delete a template",
            "broadcast create --name <n> --template <tpl> --to <list:name|segment:name>": "Stage a named broadcast in draft status",
            "broadcast preview <id> --to <email>": "Send a single test copy via email-cli send",
            "broadcast schedule <id> --at <rfc3339>": "Move a draft broadcast to scheduled",
            "broadcast send <id> [--force-unlock]": "Run the full send pipeline. v0.3.1: acquires an atomic broadcast lock to prevent double-send race; --force-unlock overrides a held lock (use only when previous process is confirmed dead). Resumable — already-sent recipients are skipped",
            "broadcast resume <id> [--force-unlock]": "Alias of `broadcast send` with explicit resume semantics. Skips already-sent recipients via the broadcast_recipient table",
            "broadcast cancel <id> --confirm": "Cancel a draft or scheduled broadcast",
            "broadcast ls [--status <s>] [--limit N]": "List recent broadcasts",
            "broadcast show <id>": "Show broadcast details including recipient + stat counts",
            "webhook poll [--reset]": "Poll email-cli for delivery status updates (alias: `event poll`)",
            "event poll [--reset]": "Alias for `webhook poll`",
            "report show <broadcast-id>": "Per-broadcast summary (delivered/bounced/opened/clicked/CTR)",
            "report links <broadcast-id>": "Per-link click counts for a broadcast",
            "report engagement [--list <name>|--segment <name>] [--days N]": "Engagement score across a list/segment",
            "report deliverability [--days N]": "Rolling-window bounce rate / complaint rate / domain health",
            "update [--check]": "Self-update from GitHub Releases",
            "skill install": "Install skill files into Claude / Codex / Gemini paths",
            "skill status": "Show which platforms have the skill installed"
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
        "status": "v0.3.1 — emergency hardening: atomic broadcast lock CAS (no double-send race even on concurrent invocation), email-cli subprocess timeout (MLC_EMAIL_CLI_TIMEOUT_SEC, default 120s, kills hung child on deadline), schema version safety check (Db::open fails fast with exit 2 when DB is newer than binary), agent-info + AGENTS.md sync. Built on v0.3.0 production-grade 10k foundations."
    });
    println!("{}", serde_json::to_string_pretty(&manifest).unwrap());
}
