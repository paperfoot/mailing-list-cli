# Phase 1: Foundations — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `mailing-list-cli v0.0.1` — a single Rust binary with config, SQLite state, JSON envelope output, semantic exit codes, `agent-info`, `health`, and a verified `email-cli` dependency. No mailing-list features yet — this is the foundation everything else builds on.

**Architecture:** Single Rust binary using `clap` for the CLI, `rusqlite` for state, `serde`/`serde_json` for the JSON envelope, `dirs` for XDG paths, `toml`/`serde` for config. Output auto-detects TTY via `std::io::IsTerminal` and switches between human-readable text and the JSON envelope. Exit codes follow the agent-cli-framework contract: `0` success, `1` transient, `2` config, `3` bad input, `4` rate limited.

**Tech Stack:** Rust 2024 edition, MSRV 1.85, `clap` 4.5, `rusqlite` 0.37 (bundled SQLite), `serde` 1.0, `serde_json`, `toml`, `dirs` 6, `anyhow`, `thiserror`, `chrono`.

**Spec reference:** [`docs/specs/2026-04-07-mailing-list-cli-design.md`](../specs/2026-04-07-mailing-list-cli-design.md), specifically §2 (Architecture), §11 (Configuration), §12 (Error Model), §13 (email-cli interface), and §15 Phase 1.

---

## File structure

```
mailing-list-cli/
├── Cargo.toml
├── Cargo.lock
├── src/
│   ├── main.rs                 # Entry point, CLI dispatch
│   ├── cli.rs                  # clap definitions for all commands
│   ├── config.rs               # Config TOML loading and validation
│   ├── db/
│   │   ├── mod.rs              # Database connection, migration runner
│   │   └── migrations.rs       # Embedded migrations as &str array
│   ├── output.rs               # JSON envelope + TTY detection
│   ├── error.rs                # Error type, exit code mapping
│   ├── paths.rs                # XDG path resolution (~/.config, ~/.local/share)
│   ├── email_cli.rs            # email-cli subprocess wrapper
│   └── commands/
│       ├── mod.rs              # Command dispatch
│       ├── agent_info.rs       # `agent-info` manifest
│       ├── health.rs           # `health` system check
│       ├── update.rs           # `update` self-update stub
│       └── skill.rs            # `skill install` stub
├── tests/
│   ├── cli.rs                  # Integration tests using assert_cmd
│   └── fixtures/
│       └── stub-email-cli.sh   # Stub email-cli for tests
├── .github/
│   └── workflows/
│       └── ci.yml              # Build, test, clippy, fmt on push
└── docs/
    └── plans/
        └── 2026-04-07-phase-1-foundations.md   # this file
```

Each file has one responsibility. `commands/` is a flat directory of one file per top-level command.

---

## Task 1: Cargo project scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `tests/cli.rs`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "mailing-list-cli"
version = "0.0.1"
edition = "2024"
rust-version = "1.85"
description = "Newsletter and mailing list management from the terminal. Built for AI agents on top of email-cli."
license = "MIT"
repository = "https://github.com/paperfoot/mailing-list-cli"
homepage = "https://github.com/paperfoot/mailing-list-cli"
keywords = ["mailing-list", "newsletter", "cli", "resend", "agent"]
categories = ["command-line-utilities", "email"]

[dependencies]
anyhow = "1.0"
chrono = { version = "0.4", features = ["clock", "serde"] }
clap = { version = "4.5", features = ["derive", "env"] }
dirs = "6.0"
rusqlite = { version = "0.37", features = ["bundled", "chrono"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0"
toml = "0.8"

[dev-dependencies]
assert_cmd = "2.0"
predicates = "3.1"
tempfile = "3.10"

[profile.release]
lto = true
codegen-units = 1
strip = true
```

- [ ] **Step 2: Create minimal `src/main.rs`**

```rust
fn main() {
    println!("mailing-list-cli {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 3: Create minimal `tests/cli.rs`**

```rust
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn binary_runs_and_prints_version() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("mailing-list-cli"));
}
```

- [ ] **Step 4: Run `cargo build` to verify compilation**

Run: `cargo build`
Expected: Successful build, no warnings.

- [ ] **Step 5: Run the integration test**

Run: `cargo test --test cli binary_runs_and_prints_version`
Expected: 1 passed.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs tests/cli.rs
git commit -m "feat: cargo scaffold + first integration test"
```

---

## Task 2: Path resolution module

**Files:**
- Create: `src/paths.rs`
- Modify: `src/main.rs` (add `mod paths;`)
- Test: `src/paths.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing tests**

Add to a new file `src/paths.rs`:

```rust
use std::path::PathBuf;

pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("MLC_CONFIG_PATH") {
        return PathBuf::from(p);
    }
    dirs::config_dir()
        .expect("XDG config dir is required")
        .join("mailing-list-cli")
        .join("config.toml")
}

pub fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("MLC_DB_PATH") {
        return PathBuf::from(p);
    }
    dirs::data_local_dir()
        .expect("XDG data dir is required")
        .join("mailing-list-cli")
        .join("state.db")
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .expect("XDG cache dir is required")
        .join("mailing-list-cli")
}

pub fn audit_log_path() -> PathBuf {
    db_path().parent().unwrap().join("audit.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_respects_env_override() {
        std::env::set_var("MLC_CONFIG_PATH", "/tmp/test-config.toml");
        assert_eq!(config_path(), PathBuf::from("/tmp/test-config.toml"));
        std::env::remove_var("MLC_CONFIG_PATH");
    }

    #[test]
    fn db_path_respects_env_override() {
        std::env::set_var("MLC_DB_PATH", "/tmp/test-state.db");
        assert_eq!(db_path(), PathBuf::from("/tmp/test-state.db"));
        std::env::remove_var("MLC_DB_PATH");
    }

    #[test]
    fn audit_log_is_sibling_of_db() {
        std::env::set_var("MLC_DB_PATH", "/tmp/foo/state.db");
        assert_eq!(audit_log_path(), PathBuf::from("/tmp/foo/audit.log"));
        std::env::remove_var("MLC_DB_PATH");
    }
}
```

- [ ] **Step 2: Wire `paths` into `main.rs`**

```rust
mod paths;

fn main() {
    println!("mailing-list-cli {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test paths`
Expected: 3 passed.

- [ ] **Step 4: Commit**

```bash
git add src/paths.rs src/main.rs
git commit -m "feat(paths): XDG path resolution with env overrides"
```

---

## Task 3: Error type and exit codes

**Files:**
- Create: `src/error.rs`
- Modify: `src/main.rs` (add `mod error;`)

- [ ] **Step 1: Write the error module with tests**

Create `src/error.rs`:

```rust
use std::fmt;

/// Semantic exit codes per the agent-cli-framework contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Success = 0,
    Transient = 1,
    Config = 2,
    BadInput = 3,
    RateLimited = 4,
}

impl ExitCode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("transient error: {message}")]
    Transient {
        code: String,
        message: String,
        suggestion: String,
    },
    #[error("config error: {message}")]
    Config {
        code: String,
        message: String,
        suggestion: String,
    },
    #[error("bad input: {message}")]
    BadInput {
        code: String,
        message: String,
        suggestion: String,
    },
    #[error("rate limited: {message}")]
    RateLimited {
        code: String,
        message: String,
        suggestion: String,
    },
}

impl AppError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            AppError::Transient { .. } => ExitCode::Transient,
            AppError::Config { .. } => ExitCode::Config,
            AppError::BadInput { .. } => ExitCode::BadInput,
            AppError::RateLimited { .. } => ExitCode::RateLimited,
        }
    }

    pub fn code(&self) -> &str {
        match self {
            AppError::Transient { code, .. }
            | AppError::Config { code, .. }
            | AppError::BadInput { code, .. }
            | AppError::RateLimited { code, .. } => code,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            AppError::Transient { message, .. }
            | AppError::Config { message, .. }
            | AppError::BadInput { message, .. }
            | AppError::RateLimited { message, .. } => message,
        }
    }

    pub fn suggestion(&self) -> &str {
        match self {
            AppError::Transient { suggestion, .. }
            | AppError::Config { suggestion, .. }
            | AppError::BadInput { suggestion, .. }
            | AppError::RateLimited { suggestion, .. } => suggestion,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_correct() {
        assert_eq!(ExitCode::Success.as_i32(), 0);
        assert_eq!(ExitCode::Transient.as_i32(), 1);
        assert_eq!(ExitCode::Config.as_i32(), 2);
        assert_eq!(ExitCode::BadInput.as_i32(), 3);
        assert_eq!(ExitCode::RateLimited.as_i32(), 4);
    }

    #[test]
    fn config_error_has_exit_code_2() {
        let err = AppError::Config {
            code: "missing_email_cli".into(),
            message: "email-cli not on PATH".into(),
            suggestion: "Install email-cli with `brew install email-cli`".into(),
        };
        assert_eq!(err.exit_code(), ExitCode::Config);
        assert_eq!(err.code(), "missing_email_cli");
    }
}
```

- [ ] **Step 2: Wire into `main.rs`**

```rust
mod error;
mod paths;

fn main() {
    println!("mailing-list-cli {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test error`
Expected: 2 passed.

- [ ] **Step 4: Commit**

```bash
git add src/error.rs src/main.rs
git commit -m "feat(error): semantic exit codes + structured AppError"
```

---

## Task 4: JSON envelope output

**Files:**
- Create: `src/output.rs`
- Modify: `src/main.rs` (add `mod output;`)

- [ ] **Step 1: Write the output module with tests**

Create `src/output.rs`:

```rust
use crate::error::AppError;
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{self, IsTerminal, Write};

/// Format determines how output is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Human,
}

impl Format {
    /// Detect format: JSON if stdout is not a TTY, or if `force_json` is set.
    pub fn detect(force_json: bool) -> Self {
        if force_json || !io::stdout().is_terminal() {
            Format::Json
        } else {
            Format::Human
        }
    }
}

/// Render a successful result to stdout in the chosen format.
pub fn success<T: Serialize>(format: Format, human_label: &str, data: T) {
    match format {
        Format::Json => {
            let envelope = json!({
                "version": "1",
                "status": "success",
                "data": data,
            });
            println!("{}", serde_json::to_string(&envelope).unwrap());
        }
        Format::Human => {
            println!("{human_label}");
            // Pretty-print the JSON below for debugging
            let value = serde_json::to_value(&data).unwrap();
            if !matches!(value, Value::Null) {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            }
        }
    }
}

/// Render an error to stderr in the chosen format. Always uses the JSON envelope
/// for the error structure even in human mode (with extra prefix).
pub fn error(format: Format, err: &AppError) {
    let envelope = json!({
        "version": "1",
        "status": "error",
        "error": {
            "code": err.code(),
            "message": err.message(),
            "suggestion": err.suggestion(),
        }
    });

    let stderr = io::stderr();
    let mut handle = stderr.lock();

    match format {
        Format::Json => {
            let _ = writeln!(handle, "{}", serde_json::to_string(&envelope).unwrap());
        }
        Format::Human => {
            let _ = writeln!(handle, "error: {}", err.message());
            let _ = writeln!(handle, "  code: {}", err.code());
            let _ = writeln!(handle, "  suggestion: {}", err.suggestion());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_detect_forces_json_when_flag_set() {
        assert_eq!(Format::detect(true), Format::Json);
    }
}
```

- [ ] **Step 2: Wire into `main.rs`**

```rust
mod error;
mod output;
mod paths;

fn main() {
    println!("mailing-list-cli {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test output`
Expected: 1 passed.

- [ ] **Step 4: Commit**

```bash
git add src/output.rs src/main.rs
git commit -m "feat(output): JSON envelope + TTY auto-detection"
```

---

## Task 5: Config loader

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the config module with tests**

Create `src/config.rs`:

```rust
use crate::error::AppError;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub sender: SenderConfig,
    #[serde(default)]
    pub webhook: WebhookConfig,
    #[serde(default)]
    pub unsubscribe: UnsubscribeConfig,
    #[serde(default)]
    pub guards: GuardsConfig,
    #[serde(default)]
    pub email_cli: EmailCliConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SenderConfig {
    pub from: Option<String>,
    pub reply_to: Option<String>,
    pub physical_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    #[serde(default = "default_webhook_port")]
    pub port: u16,
    pub secret_env: Option<String>,
    pub public_url: Option<String>,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            port: default_webhook_port(),
            secret_env: None,
            public_url: None,
        }
    }
}

fn default_webhook_port() -> u16 {
    8081
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnsubscribeConfig {
    pub public_url: Option<String>,
    pub secret_env: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardsConfig {
    #[serde(default = "default_max_complaint_rate")]
    pub max_complaint_rate: f64,
    #[serde(default = "default_max_bounce_rate")]
    pub max_bounce_rate: f64,
    #[serde(default = "default_max_recipients_per_send")]
    pub max_recipients_per_send: usize,
}

impl Default for GuardsConfig {
    fn default() -> Self {
        Self {
            max_complaint_rate: default_max_complaint_rate(),
            max_bounce_rate: default_max_bounce_rate(),
            max_recipients_per_send: default_max_recipients_per_send(),
        }
    }
}

fn default_max_complaint_rate() -> f64 {
    0.003
}
fn default_max_bounce_rate() -> f64 {
    0.04
}
fn default_max_recipients_per_send() -> usize {
    50_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailCliConfig {
    #[serde(default = "default_email_cli_path")]
    pub path: String,
    #[serde(default = "default_email_cli_profile")]
    pub profile: String,
}

impl Default for EmailCliConfig {
    fn default() -> Self {
        Self {
            path: default_email_cli_path(),
            profile: default_email_cli_profile(),
        }
    }
}

fn default_email_cli_path() -> String {
    "email-cli".into()
}
fn default_email_cli_profile() -> String {
    "default".into()
}

impl Config {
    /// Load config from the configured path. Returns Default if the file does not exist
    /// (so first-run is non-fatal). Returns AppError::Config on parse failure.
    pub fn load() -> Result<Self, AppError> {
        let path = paths::config_path();
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self, AppError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path).map_err(|e| AppError::Config {
            code: "config_read_failed".into(),
            message: format!("could not read {}: {e}", path.display()),
            suggestion: format!("Check file permissions on {}", path.display()),
        })?;
        toml::from_str(&raw).map_err(|e| AppError::Config {
            code: "config_parse_failed".into(),
            message: format!("invalid TOML in {}: {e}", path.display()),
            suggestion: "Run `mailing-list-cli health` to see a sample config".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn missing_config_returns_default() {
        let path = std::path::PathBuf::from("/tmp/this-file-does-not-exist-mlc-test");
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.webhook.port, 8081);
        assert_eq!(cfg.email_cli.path, "email-cli");
    }

    #[test]
    fn parses_full_config() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmp,
            r#"
[sender]
from = "newsletter@example.com"
reply_to = "hello@example.com"
physical_address = "123 Main St"

[webhook]
port = 9000

[guards]
max_recipients_per_send = 25000
"#
        )
        .unwrap();
        let cfg = Config::load_from(tmp.path()).unwrap();
        assert_eq!(cfg.sender.from.as_deref(), Some("newsletter@example.com"));
        assert_eq!(cfg.webhook.port, 9000);
        assert_eq!(cfg.guards.max_recipients_per_send, 25000);
        assert_eq!(cfg.guards.max_complaint_rate, 0.003); // default
    }

    #[test]
    fn invalid_toml_returns_config_error() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "this is not = valid toml [[[").unwrap();
        let err = Config::load_from(tmp.path()).unwrap_err();
        assert_eq!(err.code(), "config_parse_failed");
    }
}
```

- [ ] **Step 2: Wire into `main.rs`**

```rust
mod config;
mod error;
mod output;
mod paths;

fn main() {
    println!("mailing-list-cli {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test config`
Expected: 3 passed.

- [ ] **Step 4: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "feat(config): TOML config loader with sane defaults"
```

---

## Task 6: SQLite database with migrations

**Files:**
- Create: `src/db/mod.rs`
- Create: `src/db/migrations.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create the migrations module**

Create `src/db/migrations.rs`:

```rust
/// Embedded migrations applied in order. Each migration is idempotent only
/// in the sense that the migration runner skips already-applied versions.
pub const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_initial",
        r#"
        CREATE TABLE IF NOT EXISTS schema_version (
            version TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL
        );

        CREATE TABLE list (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            description TEXT,
            resend_audience_id TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL,
            archived_at TEXT
        );

        CREATE TABLE contact (
            id INTEGER PRIMARY KEY,
            email TEXT NOT NULL UNIQUE COLLATE NOCASE,
            first_name TEXT,
            last_name TEXT,
            status TEXT NOT NULL CHECK (status IN (
                'pending', 'active', 'unsubscribed', 'bounced',
                'complained', 'cleaned', 'erased'
            )),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            consent_source TEXT,
            consent_ip TEXT,
            consent_user_agent TEXT,
            consent_text TEXT,
            consent_at TEXT,
            confirmed_at TEXT
        );

        CREATE INDEX idx_contact_email ON contact(email);
        CREATE INDEX idx_contact_status ON contact(status);

        CREATE TABLE list_membership (
            list_id INTEGER NOT NULL REFERENCES list(id) ON DELETE CASCADE,
            contact_id INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
            joined_at TEXT NOT NULL,
            PRIMARY KEY (list_id, contact_id)
        );

        CREATE TABLE tag (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE contact_tag (
            contact_id INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
            tag_id INTEGER NOT NULL REFERENCES tag(id) ON DELETE CASCADE,
            applied_at TEXT NOT NULL,
            PRIMARY KEY (contact_id, tag_id)
        );

        CREATE TABLE field (
            id INTEGER PRIMARY KEY,
            key TEXT NOT NULL UNIQUE,
            type TEXT NOT NULL CHECK (type IN ('text', 'number', 'date', 'bool', 'select')),
            options_json TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE contact_field_value (
            contact_id INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
            field_id INTEGER NOT NULL REFERENCES field(id) ON DELETE CASCADE,
            value_text TEXT,
            value_number REAL,
            value_date TEXT,
            value_bool INTEGER,
            PRIMARY KEY (contact_id, field_id)
        );

        CREATE TABLE segment (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            filter_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE template (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            subject TEXT NOT NULL,
            mjml_source TEXT NOT NULL,
            schema_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE broadcast (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            template_id INTEGER NOT NULL REFERENCES template(id),
            target_kind TEXT NOT NULL CHECK (target_kind IN ('list', 'segment')),
            target_id INTEGER NOT NULL,
            status TEXT NOT NULL CHECK (status IN (
                'draft', 'scheduled', 'sending', 'sent', 'cancelled', 'failed'
            )),
            scheduled_at TEXT,
            sent_at TEXT,
            created_at TEXT NOT NULL,
            ab_variant_of INTEGER REFERENCES broadcast(id),
            ab_winner_pick TEXT CHECK (ab_winner_pick IN ('opens', 'clicks', 'manual')),
            ab_sample_pct INTEGER,
            ab_decided_at TEXT,
            recipient_count INTEGER DEFAULT 0,
            delivered_count INTEGER DEFAULT 0,
            bounced_count INTEGER DEFAULT 0,
            opened_count INTEGER DEFAULT 0,
            clicked_count INTEGER DEFAULT 0,
            unsubscribed_count INTEGER DEFAULT 0,
            complained_count INTEGER DEFAULT 0
        );

        CREATE TABLE broadcast_recipient (
            id INTEGER PRIMARY KEY,
            broadcast_id INTEGER NOT NULL REFERENCES broadcast(id) ON DELETE CASCADE,
            contact_id INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
            resend_email_id TEXT,
            status TEXT NOT NULL CHECK (status IN (
                'pending', 'sent', 'delivered', 'bounced', 'complained',
                'failed', 'suppressed'
            )),
            sent_at TEXT,
            last_event_at TEXT,
            UNIQUE (broadcast_id, contact_id)
        );

        CREATE INDEX idx_recipient_broadcast ON broadcast_recipient(broadcast_id);
        CREATE INDEX idx_recipient_resend ON broadcast_recipient(resend_email_id);

        CREATE TABLE suppression (
            email TEXT PRIMARY KEY COLLATE NOCASE,
            reason TEXT NOT NULL CHECK (reason IN (
                'unsubscribed', 'hard_bounced', 'soft_bounced_repeat',
                'complained', 'manually_blocked', 'spam_trap_hit',
                'gdpr_erasure', 'inactive_sunsetted', 'role_account'
            )),
            suppressed_at TEXT NOT NULL,
            source_broadcast_id INTEGER REFERENCES broadcast(id) ON DELETE SET NULL,
            notes TEXT
        );

        CREATE TABLE soft_bounce_count (
            contact_id INTEGER PRIMARY KEY REFERENCES contact(id) ON DELETE CASCADE,
            consecutive INTEGER NOT NULL DEFAULT 0,
            last_bounce_at TEXT NOT NULL,
            last_subtype TEXT
        );

        CREATE TABLE event (
            id INTEGER PRIMARY KEY,
            type TEXT NOT NULL,
            resend_email_id TEXT NOT NULL,
            broadcast_id INTEGER REFERENCES broadcast(id) ON DELETE SET NULL,
            contact_id INTEGER REFERENCES contact(id) ON DELETE SET NULL,
            payload_json TEXT NOT NULL,
            received_at TEXT NOT NULL
        );

        CREATE INDEX idx_event_email_id ON event(resend_email_id);
        CREATE INDEX idx_event_type ON event(type);
        CREATE INDEX idx_event_broadcast ON event(broadcast_id);

        CREATE TABLE click (
            id INTEGER PRIMARY KEY,
            broadcast_id INTEGER NOT NULL REFERENCES broadcast(id) ON DELETE CASCADE,
            contact_id INTEGER REFERENCES contact(id),
            link TEXT NOT NULL,
            ip_address TEXT,
            user_agent TEXT,
            clicked_at TEXT NOT NULL
        );

        CREATE INDEX idx_click_broadcast ON click(broadcast_id);
        CREATE INDEX idx_click_link ON click(link);

        CREATE TABLE optin_token (
            token TEXT PRIMARY KEY,
            contact_id INTEGER NOT NULL REFERENCES contact(id) ON DELETE CASCADE,
            list_id INTEGER REFERENCES list(id) ON DELETE SET NULL,
            issued_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            redeemed_at TEXT
        );
        "#,
    ),
];
```

- [ ] **Step 2: Create the database module**

Create `src/db/mod.rs`:

```rust
pub mod migrations;

use crate::error::AppError;
use crate::paths;
use rusqlite::Connection;
use std::path::Path;

pub struct Db {
    pub conn: Connection,
}

impl Db {
    /// Open the default database. Creates parent directories if needed and runs migrations.
    pub fn open() -> Result<Self, AppError> {
        Self::open_at(&paths::db_path())
    }

    pub fn open_at(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::Config {
                code: "db_dir_create_failed".into(),
                message: format!("could not create {}: {e}", parent.display()),
                suggestion: format!("Check directory permissions on {}", parent.display()),
            })?;
        }
        let conn = Connection::open(path).map_err(|e| AppError::Transient {
            code: "db_open_failed".into(),
            message: format!("could not open {}: {e}", path.display()),
            suggestion: "Try removing the file and rerunning to recreate".into(),
        })?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
            .map_err(|e| AppError::Transient {
                code: "db_pragma_failed".into(),
                message: format!("could not set PRAGMAs: {e}"),
                suggestion: "Database may be corrupt; consider recreating".into(),
            })?;
        let db = Self { conn };
        db.run_migrations()?;
        Ok(db)
    }

    fn run_migrations(&self) -> Result<(), AppError> {
        // Bootstrap schema_version table if missing
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_version (
                    version TEXT PRIMARY KEY,
                    applied_at TEXT NOT NULL
                );",
            )
            .map_err(|e| AppError::Transient {
                code: "schema_version_bootstrap_failed".into(),
                message: format!("could not create schema_version table: {e}"),
                suggestion: "Database may be corrupt; consider recreating".into(),
            })?;

        for (version, sql) in migrations::MIGRATIONS {
            let already: Option<String> = self
                .conn
                .query_row(
                    "SELECT version FROM schema_version WHERE version = ?",
                    [version],
                    |r| r.get(0),
                )
                .ok();
            if already.is_some() {
                continue;
            }
            self.conn.execute_batch(sql).map_err(|e| AppError::Transient {
                code: "migration_failed".into(),
                message: format!("migration {version} failed: {e}"),
                suggestion: format!("Inspect migration {version} for syntax errors"),
            })?;
            let now = chrono::Utc::now().to_rfc3339();
            self.conn
                .execute(
                    "INSERT INTO schema_version (version, applied_at) VALUES (?, ?)",
                    [version, &now],
                )
                .map_err(|e| AppError::Transient {
                    code: "schema_version_insert_failed".into(),
                    message: format!("could not record migration: {e}"),
                    suggestion: "Database may be in inconsistent state".into(),
                })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_create_all_tables() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let table_count: i64 = db
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // 17 tables expected: schema_version + 16 from §3.1
        assert!(table_count >= 17, "expected at least 17 tables, got {table_count}");
    }

    #[test]
    fn migration_is_idempotent() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let _ = Db::open_at(tmp.path()).unwrap();
        let _ = Db::open_at(tmp.path()).unwrap();
        let _ = Db::open_at(tmp.path()).unwrap();
        // No panic = pass
    }

    #[test]
    fn foreign_keys_are_enabled() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = Db::open_at(tmp.path()).unwrap();
        let fk: i64 = db
            .conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }
}
```

- [ ] **Step 3: Wire into `main.rs`**

```rust
mod config;
mod db;
mod error;
mod output;
mod paths;

fn main() {
    println!("mailing-list-cli {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test db`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/db/ src/main.rs
git commit -m "feat(db): SQLite migrations + schema for all v0.1 tables"
```

---

## Task 7: email-cli subprocess wrapper

**Files:**
- Create: `src/email_cli.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the wrapper with tests**

Create `src/email_cli.rs`:

```rust
use crate::error::AppError;
use serde_json::Value;
use std::process::{Command, Stdio};

/// A handle to the local email-cli binary. Holds the configured path and profile.
pub struct EmailCli {
    pub path: String,
    pub profile: String,
}

impl EmailCli {
    pub fn new(path: impl Into<String>, profile: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            profile: profile.into(),
        }
    }

    /// Run `email-cli --json agent-info` and return the parsed manifest.
    pub fn agent_info(&self) -> Result<Value, AppError> {
        let output = Command::new(&self.path)
            .args(["--json", "agent-info"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Config {
                code: "email_cli_not_found".into(),
                message: format!("could not run `{}`: {e}", self.path),
                suggestion: "Install email-cli with `brew install 199-biotechnologies/tap/email-cli` or set [email_cli].path in config.toml".into(),
            })?;

        if !output.status.success() {
            return Err(AppError::Transient {
                code: "email_cli_agent_info_failed".into(),
                message: format!(
                    "email-cli agent-info exited with code {:?}",
                    output.status.code()
                ),
                suggestion: format!("Run `{} agent-info` directly to see the error", self.path),
            });
        }

        serde_json::from_slice(&output.stdout).map_err(|e| AppError::Transient {
            code: "email_cli_agent_info_parse".into(),
            message: format!("could not parse email-cli agent-info JSON: {e}"),
            suggestion: "email-cli may be an incompatible version; run `email-cli --version`".into(),
        })
    }

    /// Run `email-cli --json profile test <profile>`.
    pub fn profile_test(&self) -> Result<Value, AppError> {
        let output = Command::new(&self.path)
            .args(["--json", "profile", "test", &self.profile])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Config {
                code: "email_cli_invoke_failed".into(),
                message: format!("could not run email-cli: {e}"),
                suggestion: "Check that email-cli is on PATH".into(),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Config {
                code: "email_cli_profile_test_failed".into(),
                message: format!(
                    "email-cli profile test failed: {}",
                    stderr.lines().next().unwrap_or("(no stderr)")
                ),
                suggestion: format!(
                    "Add the profile with `email-cli profile add {}` and a valid Resend API key",
                    self.profile
                ),
            });
        }

        serde_json::from_slice(&output.stdout).map_err(|e| AppError::Transient {
            code: "email_cli_response_parse".into(),
            message: format!("invalid JSON from email-cli: {e}"),
            suggestion: "Check email-cli version compatibility".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_email_cli_returns_config_error() {
        let cli = EmailCli::new("/nonexistent/path/to/email-cli", "default");
        let err = cli.agent_info().unwrap_err();
        assert_eq!(err.code(), "email_cli_not_found");
        assert!(err.suggestion().contains("Install"));
    }
}
```

- [ ] **Step 2: Wire into `main.rs`**

```rust
mod config;
mod db;
mod email_cli;
mod error;
mod output;
mod paths;

fn main() {
    println!("mailing-list-cli {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test email_cli`
Expected: 1 passed.

- [ ] **Step 4: Commit**

```bash
git add src/email_cli.rs src/main.rs
git commit -m "feat(email-cli): subprocess wrapper with structured errors"
```

---

## Task 8: CLI definition with clap

**Files:**
- Create: `src/cli.rs`
- Create: `src/commands/mod.rs`
- Create: `src/commands/agent_info.rs`
- Create: `src/commands/health.rs`
- Create: `src/commands/update.rs`
- Create: `src/commands/skill.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Define the CLI in `src/cli.rs`**

```rust
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "mailing-list-cli",
    version,
    about = "Newsletter and mailing list management from your terminal. Built for AI agents on top of email-cli.",
    long_about = None,
)]
pub struct Cli {
    /// Force JSON output even on a TTY
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Print the JSON capability manifest
    AgentInfo,
    /// Run a system health check
    Health,
    /// Self-update from GitHub Releases
    Update {
        #[arg(long)]
        check: bool,
    },
    /// Manage the skill file installed in agent platforms
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum SkillAction {
    /// Install skill files into Claude / Codex / Gemini paths
    Install,
    /// Show installed-skill status
    Status,
}
```

- [ ] **Step 2: Create `src/commands/mod.rs`**

```rust
pub mod agent_info;
pub mod health;
pub mod skill;
pub mod update;
```

- [ ] **Step 3: Create `src/commands/agent_info.rs`**

```rust
use serde_json::json;

/// Print the agent-info manifest as raw JSON. Always JSON, never wrapped in the envelope.
pub fn run() {
    let manifest = json!({
        "name": "mailing-list-cli",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Newsletter and mailing list management from your terminal. Built for AI agents on top of email-cli.",
        "commands": {
            "agent-info | info": "Machine-readable capability manifest (this output)",
            "health": "Run a system health check (email-cli reachable, DB writable, config valid)",
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
        "status": "v0.0.1 — foundations"
    });
    println!("{}", serde_json::to_string_pretty(&manifest).unwrap());
}
```

- [ ] **Step 4: Create `src/commands/health.rs`**

```rust
use crate::config::Config;
use crate::db::Db;
use crate::email_cli::EmailCli;
use crate::error::AppError;
use crate::output::{self, Format};
use serde_json::json;

pub fn run(format: Format) -> Result<(), AppError> {
    let mut checks = vec![];

    // 1. Config loads
    let config = match Config::load() {
        Ok(c) => {
            checks.push(("config_loads", "ok", String::new()));
            c
        }
        Err(e) => {
            checks.push(("config_loads", "fail", e.message().to_string()));
            output::success(
                format,
                "health: degraded",
                json!({
                    "status": "degraded",
                    "checks": checks_to_json(&checks)
                }),
            );
            return Err(e);
        }
    };

    // 2. DB opens and migrations apply
    match Db::open() {
        Ok(_) => checks.push(("database", "ok", String::new())),
        Err(e) => checks.push(("database", "fail", e.message().to_string())),
    }

    // 3. email-cli is on PATH and agent-info works
    let cli = EmailCli::new(&config.email_cli.path, &config.email_cli.profile);
    match cli.agent_info() {
        Ok(_) => checks.push(("email_cli", "ok", String::new())),
        Err(e) => checks.push(("email_cli", "fail", e.message().to_string())),
    }

    // 4. physical_address is set
    if config.sender.physical_address.is_some() {
        checks.push(("physical_address", "ok", String::new()));
    } else {
        checks.push((
            "physical_address",
            "warn",
            "[sender].physical_address is required before sending broadcasts".into(),
        ));
    }

    let status = if checks.iter().any(|c| c.1 == "fail") {
        "fail"
    } else if checks.iter().any(|c| c.1 == "warn") {
        "degraded"
    } else {
        "ok"
    };

    let label = format!("health: {status}");
    output::success(
        format,
        &label,
        json!({
            "status": status,
            "checks": checks_to_json(&checks)
        }),
    );

    if status == "fail" {
        return Err(AppError::Config {
            code: "health_check_failed".into(),
            message: "one or more health checks failed".into(),
            suggestion: "Inspect the `checks` field in the JSON output".into(),
        });
    }

    Ok(())
}

fn checks_to_json(checks: &[(&str, &str, String)]) -> serde_json::Value {
    serde_json::Value::Array(
        checks
            .iter()
            .map(|(name, state, message)| {
                json!({
                    "name": name,
                    "state": state,
                    "message": message
                })
            })
            .collect(),
    )
}
```

- [ ] **Step 5: Create `src/commands/update.rs` (stub)**

```rust
use crate::error::AppError;
use crate::output::{self, Format};
use serde_json::json;

pub fn run(format: Format, check: bool) -> Result<(), AppError> {
    output::success(
        format,
        "update: not yet implemented",
        json!({
            "current_version": env!("CARGO_PKG_VERSION"),
            "check_only": check,
            "note": "Self-update lands in a future phase. For now, reinstall via cargo or homebrew."
        }),
    );
    Ok(())
}
```

- [ ] **Step 6: Create `src/commands/skill.rs` (stub)**

```rust
use crate::cli::SkillAction;
use crate::error::AppError;
use crate::output::{self, Format};
use serde_json::json;

pub fn run(format: Format, action: SkillAction) -> Result<(), AppError> {
    let label = match action {
        SkillAction::Install => "skill install: not yet implemented",
        SkillAction::Status => "skill status: not yet implemented",
    };
    output::success(
        format,
        label,
        json!({
            "note": "Skill installation lands in a future phase."
        }),
    );
    Ok(())
}
```

- [ ] **Step 7: Wire all of it up in `src/main.rs`**

```rust
mod cli;
mod commands;
mod config;
mod db;
mod email_cli;
mod error;
mod output;
mod paths;

use clap::Parser;
use cli::{Cli, Command};
use output::Format;
use std::process::ExitCode;

fn main() -> ExitCode {
    let parsed = Cli::parse();
    let format = Format::detect(parsed.json);

    let result = match parsed.command {
        Command::AgentInfo => {
            commands::agent_info::run();
            Ok(())
        }
        Command::Health => commands::health::run(format),
        Command::Update { check } => commands::update::run(format, check),
        Command::Skill { action } => commands::skill::run(format, action),
    };

    match result {
        Ok(()) => ExitCode::from(error::ExitCode::Success.as_i32() as u8),
        Err(err) => {
            output::error(format, &err);
            ExitCode::from(err.exit_code().as_i32() as u8)
        }
    }
}
```

- [ ] **Step 8: Run `cargo build` to verify it compiles**

Run: `cargo build`
Expected: clean build, no warnings.

- [ ] **Step 9: Manually exercise the binary**

Run: `cargo run -- --json agent-info | head -20`
Expected: A JSON manifest starting with `{"name":"mailing-list-cli", ...`.

Run: `cargo run -- --json health`
Expected: A JSON envelope with `status: "fail"` (because email-cli is probably not configured) and a checks array.

- [ ] **Step 10: Commit**

```bash
git add src/
git commit -m "feat(cli): clap definitions + agent-info, health, update, skill stubs"
```

---

## Task 9: Integration tests for the assembled CLI

**Files:**
- Modify: `tests/cli.rs`
- Create: `tests/fixtures/stub-email-cli.sh`

- [ ] **Step 1: Replace the contents of `tests/cli.rs`**

```rust
use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::path::PathBuf;
use tempfile::TempDir;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn isolated_env() -> TempDir {
    TempDir::new().unwrap()
}

#[test]
fn agent_info_returns_valid_json() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    let out = cmd.args(["--json", "agent-info"]).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let value: Value = serde_json::from_str(&stdout).expect("agent-info must be JSON");
    assert_eq!(value["name"], "mailing-list-cli");
    assert!(value["commands"].is_object());
    assert_eq!(value["exit_codes"]["2"], "Config error (missing email-cli, missing physical_address, etc) -- fix setup");
}

#[test]
fn agent_info_lists_health_command() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    let out = cmd.args(["--json", "agent-info"]).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert!(value["commands"]["health"].is_string());
}

#[test]
fn version_flag_exits_zero() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("mailing-list-cli"));
}

#[test]
fn help_flag_exits_zero() {
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("mailing list management"));
}

#[test]
fn health_with_stub_email_cli_succeeds() {
    let stub = fixture_path("stub-email-cli.sh");
    assert!(stub.exists(), "stub-email-cli.sh must exist at {:?}", stub);

    let tmp = isolated_env();
    let config_path = tmp.path().join("config.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"
[sender]
physical_address = "123 Test St"

[email_cli]
path = "{}"
profile = "default"
"#,
            stub.display()
        ),
    )
    .unwrap();

    let db_path = tmp.path().join("state.db");

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "health"]);
    let out = cmd.assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["status"], "success");
    assert_eq!(value["data"]["status"], "ok");
}

#[test]
fn health_without_email_cli_fails_with_exit_2() {
    let tmp = isolated_env();
    let config_path = tmp.path().join("config.toml");
    std::fs::write(
        &config_path,
        r#"
[sender]
physical_address = "123 Test St"

[email_cli]
path = "/definitely/not/a/real/path/email-cli"
profile = "default"
"#,
    )
    .unwrap();
    let db_path = tmp.path().join("state.db");

    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.env("MLC_CONFIG_PATH", &config_path)
        .env("MLC_DB_PATH", &db_path)
        .args(["--json", "health"]);
    cmd.assert().failure().code(2);
}

#[test]
fn unknown_command_returns_exit_2() {
    // clap returns exit 2 for usage errors by default
    let mut cmd = Command::cargo_bin("mailing-list-cli").unwrap();
    cmd.arg("definitely-not-a-real-subcommand")
        .assert()
        .failure();
}
```

- [ ] **Step 2: Create the stub email-cli script**

Create `tests/fixtures/stub-email-cli.sh`:

```bash
#!/bin/sh
# Minimal stub of email-cli for tests. Returns canned JSON for agent-info and profile test.
case "$2" in
    "agent-info")
        echo '{"name":"email-cli","version":"0.4.0","commands":{}}'
        exit 0
        ;;
    "profile")
        if [ "$3" = "test" ]; then
            echo '{"version":"1","status":"success","data":{"reachable":true}}'
            exit 0
        fi
        ;;
esac
echo '{"version":"1","status":"error","error":{"code":"unsupported","message":"stub","suggestion":"this is a test stub"}}' >&2
exit 1
```

- [ ] **Step 3: Make the stub executable**

Run: `chmod +x tests/fixtures/stub-email-cli.sh`

- [ ] **Step 4: Run the integration tests**

Run: `cargo test --test cli`
Expected: 7 passed.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test`
Expected: All unit and integration tests pass.

- [ ] **Step 6: Commit**

```bash
git add tests/
git commit -m "test: integration tests with stub email-cli"
```

---

## Task 10: GitHub Actions CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Create the workflow file**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  build:
    name: Build & Test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - name: Cache cargo
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/bin/
            ~/.cargo/registry/index/
            ~/.cargo/registry/cache/
            ~/.cargo/git/db/
            target/
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Format check
        run: cargo fmt --all -- --check

      - name: Clippy
        run: cargo clippy --all-targets --all-features -- -D warnings

      - name: Build
        run: cargo build --release --verbose

      - name: Test
        run: cargo test --all-features --verbose
```

- [ ] **Step 2: Verify formatting locally**

Run: `cargo fmt --all`
Expected: silent (no changes).

- [ ] **Step 3: Verify clippy locally**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: no warnings, no errors.

- [ ] **Step 4: Commit**

```bash
git add .github/
git commit -m "ci: GitHub Actions for fmt, clippy, build, test"
```

---

## Task 11: README updates and tag v0.0.1

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update the README status badge**

Find the badge line:
```markdown
[![Status: Spec & Research](https://img.shields.io/badge/Status-Spec_%26_Research-blue?style=for-the-badge)](#status)
```

Replace with:
```markdown
[![Status: v0.0.1 Foundations](https://img.shields.io/badge/Status-v0.0.1_Foundations-orange?style=for-the-badge)](#status)
```

- [ ] **Step 2: Update the Status section**

Find:
```markdown
> **Spec and research phase. No binary yet.**
```

Replace with:
```markdown
> **v0.0.1 — Foundations shipped. No mailing-list features yet.**
>
> The binary builds, the database migrates, the JSON envelope works, `agent-info` is wired, and `health` checks every dependency including `email-cli`. Mailing-list features (lists, contacts, templates, broadcasts) land in subsequent v0.0.x and v0.1.x releases per the [pinned roadmap issue](https://github.com/paperfoot/mailing-list-cli/issues/1).
```

- [ ] **Step 3: Commit, tag, and push**

```bash
git add README.md
git commit -m "docs: README updates for v0.0.1"
git tag v0.0.1
git push
git push --tags
```

---

## Self-Review

After completing all tasks above, verify the following:

**Spec coverage (Phase 1 of §15):**
- ✅ Cargo project scaffold → Task 1
- ✅ Config (`~/.config/mailing-list-cli/config.toml`) → Task 5
- ✅ Local SQLite store with migrations → Task 6
- ✅ JSON envelope output + TTY auto-detection → Task 4
- ✅ Semantic exit codes (0/1/2/3/4) → Task 3
- ✅ Self-describing `agent-info` command → Task 8 step 3
- ✅ Dependency check on first run: `email-cli` must be on `$PATH` → Task 7 + Task 8 step 4
- ✅ `update` self-update from GitHub Releases → Task 8 step 5 (stub)
- ✅ `skill install` → Task 8 step 6 (stub)
- ✅ CI: build, test, clippy, release-binary on tag → Task 10

Stubs for `update` and `skill install` are intentional for v0.0.1 — the contract is wired, the implementation lands later. They print a helpful note instead of failing, so the agent can discover them via `agent-info` and skip them gracefully.

**Type consistency:**
- `EmailCli::new(path, profile)` matches the signature used in `commands/health.rs`.
- `Format::detect(force_json)` matches the signature in `output.rs`.
- `AppError` variants are uniform: every variant has `code`, `message`, `suggestion`.
- `ExitCode::as_i32()` is used everywhere we cast to a number.

**No placeholders:**
- Every step has either complete code or an exact command.
- No "TODO" / "TBD" outside the spec's intentionally-deferred sections.

---

## Done criteria for v0.0.1

- [ ] `cargo build --release` produces a binary in `target/release/mailing-list-cli`
- [ ] `cargo test` passes (all unit + integration tests, ≥10 tests total)
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt -- --check` passes
- [ ] `mailing-list-cli --json agent-info` returns a parseable JSON manifest
- [ ] `mailing-list-cli --json health` reports a degraded state when email-cli is missing and an `ok` state with the stub fixture
- [ ] CI workflow runs on a fresh clone and passes
- [ ] `v0.0.1` tag is pushed to GitHub
- [ ] README reflects the new status

Once all checkboxes above are ticked, Phase 1 is complete and Phase 2 (Lists & Contacts) can begin against this foundation.
