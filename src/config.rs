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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeConfig {
    #[serde(default = "default_unsubscribe_public_url")]
    pub public_url: String,
    #[serde(default = "default_unsubscribe_secret_env")]
    pub secret_env: String,
}

impl Default for UnsubscribeConfig {
    fn default() -> Self {
        Self {
            public_url: default_unsubscribe_public_url(),
            secret_env: default_unsubscribe_secret_env(),
        }
    }
}

fn default_unsubscribe_public_url() -> String {
    "https://hooks.yourdomain.com/u".to_string()
}

fn default_unsubscribe_secret_env() -> String {
    "MLC_UNSUBSCRIBE_SECRET".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardsConfig {
    #[serde(default = "default_max_complaint_rate")]
    pub max_complaint_rate: f64,
    #[serde(default = "default_max_bounce_rate")]
    pub max_bounce_rate: f64,
    #[serde(default = "default_max_recipients_per_send")]
    pub max_recipients_per_send: usize,
    /// v0.4.5: refuse to send when the template carries error-level design
    /// findings (browser/JSX source, embedded scripts). Defaults to true so
    /// existing JSX-shaped templates already in the DB cannot ship without an
    /// explicit operator override (`--allow-design-errors`).
    #[serde(default = "default_block_design_errors")]
    pub block_design_errors: bool,
}

impl Default for GuardsConfig {
    fn default() -> Self {
        Self {
            max_complaint_rate: default_max_complaint_rate(),
            max_bounce_rate: default_max_bounce_rate(),
            max_recipients_per_send: default_max_recipients_per_send(),
            block_design_errors: default_block_design_errors(),
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
fn default_block_design_errors() -> bool {
    true
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
        assert_eq!(cfg.guards.max_complaint_rate, 0.003);
    }

    #[test]
    fn invalid_toml_returns_config_error() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "this is not = valid toml [[[").unwrap();
        let err = Config::load_from(tmp.path()).unwrap_err();
        assert_eq!(err.code(), "config_parse_failed");
    }
}
