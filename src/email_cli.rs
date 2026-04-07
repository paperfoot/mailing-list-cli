use crate::error::AppError;
use serde_json::Value;
use std::process::{Command, Stdio};

/// A handle to the local email-cli binary.
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
            suggestion: "email-cli may be an incompatible version; run `email-cli --version`"
                .into(),
        })
    }

    /// Create a Resend audience via `email-cli --json audience create --name <name>`.
    /// Returns the new audience id.
    pub fn audience_create(&self, name: &str) -> Result<String, AppError> {
        let output = Command::new(&self.path)
            .args(["--json", "audience", "create", "--name", name])
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
            return Err(AppError::Transient {
                code: "audience_create_failed".into(),
                message: format!(
                    "email-cli audience create failed: {}",
                    stderr.lines().next().unwrap_or("(no stderr)")
                ),
                suggestion: "Run `email-cli profile test default` to verify Resend connectivity"
                    .into(),
            });
        }

        let parsed: Value =
            serde_json::from_slice(&output.stdout).map_err(|e| AppError::Transient {
                code: "audience_create_parse".into(),
                message: format!("invalid JSON from email-cli audience create: {e}"),
                suggestion: "Check email-cli version compatibility".into(),
            })?;

        // Try common shapes: top-level id, data.id, or data.audience.id
        let id = parsed
            .get("data")
            .and_then(|d| d.get("id"))
            .and_then(|v| v.as_str())
            .or_else(|| {
                parsed
                    .get("data")
                    .and_then(|d| d.get("audience"))
                    .and_then(|a| a.get("id"))
                    .and_then(|v| v.as_str())
            })
            .or_else(|| parsed.get("id").and_then(|v| v.as_str()));

        id.map(|s| s.to_string())
            .ok_or_else(|| AppError::Transient {
                code: "audience_create_missing_id".into(),
                message: "email-cli audience create response had no id field".into(),
                suggestion: "email-cli may be an incompatible version".into(),
            })
    }

    /// Create a Resend contact via `email-cli --json contact create --audience <id> --email <e>`.
    pub fn contact_create(
        &self,
        audience_id: &str,
        email: &str,
        first_name: Option<&str>,
        last_name: Option<&str>,
    ) -> Result<(), AppError> {
        let mut args = vec![
            "--json".to_string(),
            "contact".to_string(),
            "create".to_string(),
            "--audience".to_string(),
            audience_id.to_string(),
            "--email".to_string(),
            email.to_string(),
        ];
        if let Some(f) = first_name {
            args.push("--first-name".to_string());
            args.push(f.to_string());
        }
        if let Some(l) = last_name {
            args.push("--last-name".to_string());
            args.push(l.to_string());
        }

        let output = Command::new(&self.path)
            .args(&args)
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
            // A duplicate-contact error from email-cli is non-fatal at this layer
            // because mailing-list-cli treats the local DB as authoritative.
            if stderr.contains("already exists") || stderr.contains("duplicate") {
                return Ok(());
            }
            return Err(AppError::Transient {
                code: "contact_create_failed".into(),
                message: format!(
                    "email-cli contact create failed: {}",
                    stderr.lines().next().unwrap_or("(no stderr)")
                ),
                suggestion:
                    "Run `email-cli contact list --audience <id>` to inspect Resend audience state"
                        .into(),
            });
        }

        Ok(())
    }

    /// Run `email-cli --json profile test <profile>`.
    #[allow(dead_code)]
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
