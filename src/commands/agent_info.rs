use serde_json::json;

/// Print the agent-info manifest as raw JSON. Always JSON, never wrapped in the envelope.
pub fn run() {
    let manifest = json!({
        "name": "mailing-list-cli",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Newsletter and mailing list management from your terminal. Built for AI agents on top of email-cli.",
        "commands": {
            "agent-info": "Machine-readable capability manifest (this output)",
            "health": "Run a system health check",
            "list create <name> [--description <text>]": "Create a list (backed by a Resend segment via email-cli)",
            "list ls": "List all lists with subscriber counts",
            "list show <id>": "Show one list's details",
            "contact add <email> --list <id> [--first-name F --last-name L --field key=val ...]": "Add a contact to a list",
            "contact ls [--list <id>] [--filter <expr>] [--limit N] [--cursor C]": "List/filter contacts",
            "contact show <email>": "Show a contact's full details (tags, fields, list memberships)",
            "contact tag <email> <tag>": "Apply a tag to a contact",
            "contact untag <email> <tag>": "Remove a tag from a contact",
            "contact set <email> <field> <value>": "Set a typed custom field value",
            "contact import <file.csv> --list <id> [--unsafe-no-consent]": "Bulk-import contacts from CSV (5 req/sec rate limit, idempotent replay)",
            "tag ls": "List all tags with member counts",
            "tag rm <name> --confirm": "Delete a tag",
            "field create <key> --type <text|number|date|bool|select> [--options a,b,c]": "Create a typed custom field",
            "field ls": "List all custom fields",
            "field rm <key> --confirm": "Delete a custom field",
            "segment create <name> --filter <expr>": "Save a dynamic segment",
            "segment ls": "List all segments with member counts",
            "segment show <name>": "Show a segment's filter + 10 sample members",
            "segment members <name> [--limit N] [--cursor C]": "List contacts currently matching the segment",
            "segment rm <name> --confirm": "Delete a segment definition",
            "template create <name> [--from-file <path>]": "Create a new template (scaffold or import MJML file)",
            "template ls": "List all templates",
            "template show <name>": "Print a template's MJML source",
            "template render <name> [--with-data <file.json>] [--with-placeholders]": "Compile template → JSON { subject, html, text }",
            "template lint <name>": "Run the 13-rule lint set; exit 3 on errors",
            "template edit <name> [--force]": "Open in $EDITOR, re-lint on save",
            "template rm <name> --confirm": "Delete a template",
            "template guidelines": "Print the embedded agent authoring guide",
            "broadcast create --name <n> --template <tpl> --to <list:name|segment:name>": "Stage a named broadcast in draft status",
            "broadcast preview <id> --to <email>": "Send a single test copy via email-cli send",
            "broadcast schedule <id> --at <rfc3339>": "Move a draft broadcast to scheduled",
            "broadcast send <id>": "Run the full send pipeline and dispatch via email-cli batch send",
            "broadcast cancel <id> --confirm": "Cancel a draft or scheduled broadcast",
            "broadcast ls [--status <s>] [--limit N]": "List recent broadcasts",
            "broadcast show <id>": "Show broadcast details including recipient + stat counts",
            "update [--check]": "Self-update from GitHub Releases",
            "skill install": "Install skill files into Claude / Codex / Gemini paths",
            "skill status": "Show which platforms have the skill installed"
        },
        "flags": {
            "--json": "Force JSON output (auto-enabled when stdout is not a TTY)"
        },
        "exit_codes": {
            "0": "Success",
            "1": "Transient error (IO, network, email-cli unavailable) -- retry",
            "2": "Config error (missing email-cli, missing physical_address, etc) -- fix setup",
            "3": "Bad input (invalid args) -- fix arguments",
            "4": "Rate limited (Resend rate limit) -- wait and retry"
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
        "depends_on": ["email-cli >= 0.6.0"],
        "status": "v0.1.1 — broadcasts, send pipeline, HMAC unsubscribe tokens"
    });
    println!("{}", serde_json::to_string_pretty(&manifest).unwrap());
}
