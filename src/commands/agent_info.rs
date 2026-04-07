use serde_json::json;

/// Print the agent-info manifest as raw JSON. Always JSON, never wrapped in the envelope.
pub fn run() {
    let manifest = json!({
        "name": "mailing-list-cli",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Newsletter and mailing list management from your terminal. Built for AI agents on top of email-cli.",
        "commands": {
            "agent-info": "Machine-readable capability manifest (this output)",
            "health": "Run a system health check (email-cli reachable, DB writable, config valid)",
            "list create <name> [--description <text>]": "Create a list (also creates a Resend audience via email-cli)",
            "list ls": "List all lists with subscriber counts",
            "list show <id>": "Show one list's details",
            "contact add <email> --list <id> [--first-name <f> --last-name <l>]": "Add a contact to a list",
            "contact ls --list <id> [--limit N]": "List contacts in a list",
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
        "depends_on": ["email-cli"],
        "status": "v0.0.2 — lists & contacts"
    });
    println!("{}", serde_json::to_string_pretty(&manifest).unwrap());
}
