use crate::error::AppError;
use serde_json::Value;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration as StdDuration, Instant};

/// A handle to the local email-cli binary.
pub struct EmailCli {
    pub path: String,
    pub profile: String,
    last_call: Mutex<Instant>,
}

const MIN_INTERVAL: StdDuration = StdDuration::from_millis(200);

/// Maximum number of retries for transient batch-send failures (429 rate
/// limited, 5xx, connection reset, timeout). Total attempts = MAX_RETRIES + 1.
const MAX_RETRIES: u32 = 4;

/// Exponential backoff schedule between retry attempts, in milliseconds.
/// Indexed by retry number (0 = first retry after the initial attempt failed).
/// Schedule: 500ms, 1s, 2s, 4s — total worst-case ~7.5s per chunk.
const BACKOFF_MS: &[u64] = &[500, 1_000, 2_000, 4_000];

/// Classify a `batch send` subprocess failure as either a transient error
/// (should be retried) or a permanent one (should fail fast).
///
/// Transient signals:
/// - Exit code 4 (the agent-cli-framework convention for rate-limited).
/// - stderr containing `429`, `rate_limit`, `connection reset`, `timeout`,
///   or any 5xx HTTP marker.
///
/// Everything else (4xx auth, validation, 404, etc.) is permanent.
fn is_retryable_batch_error(exit_code: Option<i32>, stderr: &str) -> bool {
    if exit_code == Some(4) {
        return true;
    }
    let lower = stderr.to_ascii_lowercase();
    lower.contains("429")
        || lower.contains("too many requests")
        || lower.contains("rate_limit")
        || lower.contains("rate limit")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("http 5")
        || lower.contains("internal server error")
        || lower.contains("bad gateway")
        || lower.contains("service unavailable")
        || lower.contains("gateway timeout")
}

impl EmailCli {
    pub fn new(path: impl Into<String>, profile: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            profile: profile.into(),
            last_call: Mutex::new(Instant::now() - MIN_INTERVAL),
        }
    }

    /// Sleep until at least 200ms have elapsed since the last call. This
    /// enforces the 5 req/sec Resend rate limit at the subprocess layer
    /// across ALL invocations.
    fn throttle(&self) {
        let mut last = self.last_call.lock().unwrap();
        let elapsed = last.elapsed();
        if elapsed < MIN_INTERVAL {
            std::thread::sleep(MIN_INTERVAL - elapsed);
        }
        *last = Instant::now();
    }

    /// Run `email-cli --json agent-info` and return the parsed manifest.
    pub fn agent_info(&self) -> Result<Value, AppError> {
        self.throttle();
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

    /// Create a Resend segment via `email-cli --json segment create --name <name>`.
    /// Returns the new segment id.
    ///
    /// Replaces the old `audience_create` which targeted the deprecated
    /// `/audiences` endpoint. Resend renamed Audiences to Segments in
    /// November 2025 and email-cli v0.6+ removed the legacy `audience`
    /// subcommand entirely.
    pub fn segment_create(&self, name: &str) -> Result<String, AppError> {
        self.throttle();
        let output = Command::new(&self.path)
            .args(["--json", "segment", "create", "--name", name])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Config {
                code: "email_cli_invoke_failed".into(),
                message: format!("could not run email-cli: {e}"),
                suggestion: "Check that email-cli is on PATH (v0.6+ required)".into(),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Transient {
                code: "segment_create_failed".into(),
                message: format!(
                    "email-cli segment create failed: {}",
                    stderr.lines().next().unwrap_or("(no stderr)")
                ),
                suggestion: "Run `email-cli profile test default` to verify Resend connectivity"
                    .into(),
            });
        }

        let parsed: Value =
            serde_json::from_slice(&output.stdout).map_err(|e| AppError::Transient {
                code: "segment_create_parse".into(),
                message: format!("invalid JSON from email-cli segment create: {e}"),
                suggestion: "Check email-cli version (v0.6+ required)".into(),
            })?;

        // Try common response shapes: data.id, data.segment.id, or top-level id.
        let id = parsed
            .get("data")
            .and_then(|d| d.get("id"))
            .and_then(|v| v.as_str())
            .or_else(|| {
                parsed
                    .get("data")
                    .and_then(|d| d.get("segment"))
                    .and_then(|s| s.get("id"))
                    .and_then(|v| v.as_str())
            })
            .or_else(|| parsed.get("id").and_then(|v| v.as_str()));

        id.map(|s| s.to_string())
            .ok_or_else(|| AppError::Transient {
                code: "segment_create_missing_id".into(),
                message: "email-cli segment create response had no id field".into(),
                suggestion: "email-cli may be an incompatible version".into(),
            })
    }

    /// Create a Resend contact via the flat `/contacts` API (email-cli v0.6+).
    /// Optionally adds the contact to segments at creation time and/or attaches
    /// custom properties (if the contact-property schema has been defined via
    /// `email-cli contact-property create`).
    ///
    /// Treats "already exists" errors from email-cli as a no-op because
    /// mailing-list-cli's local DB is authoritative.
    pub fn contact_create(
        &self,
        email: &str,
        first_name: Option<&str>,
        last_name: Option<&str>,
        segments: &[&str],
        properties: Option<&Value>,
    ) -> Result<(), AppError> {
        self.throttle();
        let mut args: Vec<String> = vec![
            "--json".into(),
            "contact".into(),
            "create".into(),
            "--email".into(),
            email.into(),
        ];
        if let Some(f) = first_name {
            args.push("--first-name".into());
            args.push(f.into());
        }
        if let Some(l) = last_name {
            args.push("--last-name".into());
            args.push(l.into());
        }
        if !segments.is_empty() {
            args.push("--segments".into());
            args.push(segments.join(","));
        }
        if let Some(props) = properties {
            args.push("--properties".into());
            args.push(props.to_string());
        }

        let output = Command::new(&self.path)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Config {
                code: "email_cli_invoke_failed".into(),
                message: format!("could not run email-cli: {e}"),
                suggestion: "Check that email-cli is on PATH (v0.6+ required)".into(),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let is_duplicate = stderr.contains("already exists") || stderr.contains("duplicate");
            if is_duplicate {
                // The contact already exists in Resend. Our local DB is the
                // source of truth for memberships, so ensure the existing
                // Resend contact is in each requested segment.
                for seg in segments {
                    self.segment_contact_add(email, seg)?;
                }
                return Ok(());
            }
            return Err(AppError::Transient {
                code: "contact_create_failed".into(),
                message: format!(
                    "email-cli contact create failed: {}",
                    stderr.lines().next().unwrap_or("(no stderr)")
                ),
                suggestion: "Run `email-cli contact list` to inspect Resend contact state".into(),
            });
        }

        Ok(())
    }

    /// Add an existing Resend contact to a segment. Used by `contact_create`'s
    /// duplicate-handling path and by the CSV importer's re-run logic.
    pub fn segment_contact_add(
        &self,
        contact_email: &str,
        segment_id: &str,
    ) -> Result<(), AppError> {
        self.throttle();
        let output = Command::new(&self.path)
            .args([
                "--json",
                "segment",
                "contact-add",
                "--contact",
                contact_email,
                "--segment",
                segment_id,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Config {
                code: "email_cli_invoke_failed".into(),
                message: format!("could not run email-cli: {e}"),
                suggestion: "Check that email-cli is on PATH (v0.6+ required)".into(),
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // "already in segment" is a successful no-op
            if stderr.contains("already") {
                return Ok(());
            }
            return Err(AppError::Transient {
                code: "segment_contact_add_failed".into(),
                message: format!(
                    "email-cli segment contact-add failed: {}",
                    stderr.lines().next().unwrap_or("(no stderr)")
                ),
                suggestion: "Run `email-cli segment list` to verify the segment exists".into(),
            });
        }
        Ok(())
    }

    /// Shell out to `email-cli domain list` and return the array of domain
    /// objects. Each entry has `name`, `region`, `status` ('verified' |
    /// 'pending' | 'failed' | 'unverified'), and `capabilities`.
    ///
    /// v0.3: used by the `health` check and broadcast preflight to confirm
    /// the sender domain is verified before a send.
    ///
    /// Note: email-cli v0.6.3 does NOT expose open/click tracking settings
    /// or Resend domain UUIDs via `domain list`. The tracking-config
    /// surfacing originally planned for v0.3 Task 7 is therefore reduced
    /// to domain-status surfacing only; the full open/click tracking
    /// surfacing is deferred to v0.3.1 pending an upstream email-cli fix
    /// (issue: expose `open_tracking`, `click_tracking`, and `id` in the
    /// `domain list` output).
    #[allow(dead_code)]
    pub fn domain_list(&self) -> Result<Vec<Value>, AppError> {
        self.throttle();
        let output = Command::new(&self.path)
            .args(["--json", "domain", "list"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Config {
                code: "email_cli_invoke_failed".into(),
                message: format!("could not run email-cli domain list: {e}"),
                suggestion: "Check that email-cli is on PATH".into(),
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Transient {
                code: "domain_list_failed".into(),
                message: format!(
                    "email-cli domain list failed: {}",
                    stderr.lines().next().unwrap_or("(no stderr)")
                ),
                suggestion: "Run `email-cli profile test` to verify Resend connectivity".into(),
            });
        }
        let parsed: Value =
            serde_json::from_slice(&output.stdout).map_err(|e| AppError::Transient {
                code: "domain_list_parse".into(),
                message: format!("invalid JSON from email-cli domain list: {e}"),
                suggestion: "Check email-cli version compatibility".into(),
            })?;
        // Shape: {"data": [ {name, region, status, capabilities}, ... ] }
        let arr = parsed
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(arr)
    }

    /// Shell out to `email-cli batch send --file <path>`. Real email-cli
    /// returns `{"data": {"data": [{"id": "<resend-uuid>"}, ...]}}` — items
    /// match input order, no `to` field. The caller must pass the recipients
    /// in input order so we can correlate index → email.
    ///
    /// v0.3: retries on transient errors (429 rate limited, 5xx, connection
    /// reset, timeout) with exponential backoff [500ms, 1s, 2s, 4s], up to
    /// `MAX_RETRIES` (4) retries = 5 total attempts. Permanent errors (4xx
    /// validation, auth) fail fast without retrying. On exhausted retries,
    /// returns `AppError::RateLimited` so callers can distinguish silent
    /// under-delivery from permanent broadcast failures.
    #[allow(dead_code)]
    pub fn batch_send(
        &self,
        batch_file: &std::path::Path,
        recipients_in_order: &[String],
    ) -> Result<Vec<(String, String)>, AppError> {
        let mut attempt: u32 = 0;
        loop {
            self.throttle();
            let output = Command::new(&self.path)
                .args([
                    "--json",
                    "batch",
                    "send",
                    "--file",
                    batch_file.to_str().unwrap_or(""),
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .map_err(|e| AppError::Config {
                    code: "email_cli_invoke_failed".into(),
                    message: format!("could not run email-cli batch send: {e}"),
                    suggestion: "Check that email-cli is on PATH (v0.6+ required)".into(),
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code();
                let retryable = is_retryable_batch_error(exit_code, &stderr);
                if retryable && attempt < MAX_RETRIES {
                    let sleep_ms = BACKOFF_MS[attempt as usize];
                    eprintln!(
                        "batch_send attempt {} failed (retryable): {}; retrying in {}ms",
                        attempt + 1,
                        stderr.lines().next().unwrap_or("(no stderr)"),
                        sleep_ms
                    );
                    std::thread::sleep(StdDuration::from_millis(sleep_ms));
                    attempt += 1;
                    continue;
                }
                // Either not retryable, or we exhausted MAX_RETRIES.
                let first_line = stderr.lines().next().unwrap_or("(no stderr)").to_string();
                if retryable {
                    return Err(AppError::RateLimited {
                        code: "batch_send_retries_exhausted".into(),
                        message: format!(
                            "email-cli batch send failed after {} attempt(s): {}",
                            attempt + 1,
                            first_line
                        ),
                        suggestion:
                            "Resend is rate-limiting or unreachable. Wait and resume the broadcast with `broadcast resume <id>`, or raise your Resend plan limits."
                                .into(),
                    });
                }
                return Err(AppError::Transient {
                    code: "batch_send_failed".into(),
                    message: format!("email-cli batch send failed: {first_line}"),
                    suggestion: "Run `email-cli profile test` to verify Resend connectivity".into(),
                });
            }

            // Successful exit — parse JSON response.
            let parsed: Value =
                serde_json::from_slice(&output.stdout).map_err(|e| AppError::Transient {
                    code: "batch_send_parse".into(),
                    message: format!("invalid JSON from email-cli batch send: {e}"),
                    suggestion: "Check email-cli version (v0.6+ required)".into(),
                })?;
            // Real shape: {"data": {"data": [{"id": "..."}]}}
            // Test stub shape: {"data": [{"id": "...", "to": "..."}]}  (legacy)
            // We support both: try data.data first, fall back to data.
            let items = parsed
                .get("data")
                .and_then(|d| d.get("data"))
                .and_then(|d| d.as_array())
                .or_else(|| parsed.get("data").and_then(|d| d.as_array()))
                .ok_or_else(|| AppError::Transient {
                    code: "batch_send_no_data".into(),
                    message: "email-cli batch send response has no `data` array".into(),
                    suggestion:
                        "Check email-cli version compatibility (expected data.data[] or data[])"
                            .into(),
                })?;
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                let id = item
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                // Real responses have no `to` field — use the input order. The stub
                // does include `to`, so prefer that when present (test compat).
                let to = item
                    .get("to")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or_else(|| {
                        item.get("to")
                            .and_then(|v| v.as_array())
                            .and_then(|a| a.first())
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    })
                    .unwrap_or_else(|| recipients_in_order.get(i).cloned().unwrap_or_default());
                out.push((to, id));
            }
            return Ok(out);
        }
    }

    /// Shell out to `email-cli send` for single-recipient transactional sends.
    /// `from` is used as the `--account` argument (which is an email address
    /// matching one of the configured sender accounts in email-cli, NOT the
    /// profile name).
    #[allow(dead_code)]
    pub fn send(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        html: &str,
        text: &str,
    ) -> Result<String, AppError> {
        self.throttle();
        let output = Command::new(&self.path)
            .args([
                "--json",
                "send",
                "--account",
                from,
                "--to",
                to,
                "--subject",
                subject,
                "--html",
                html,
                "--text",
                text,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Config {
                code: "email_cli_invoke_failed".into(),
                message: format!("could not run email-cli send: {e}"),
                suggestion: "Check that email-cli is on PATH".into(),
            })?;
        // Real email-cli returns errors as JSON in stdout AND non-zero exit.
        // Try to parse the stdout JSON either way so we can surface the actual
        // error message.
        let parsed_result: Result<Value, _> = serde_json::from_slice(&output.stdout);
        if !output.status.success() {
            let detail = parsed_result
                .as_ref()
                .ok()
                .and_then(|p| p.get("error"))
                .and_then(|e| e.get("message"))
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| {
                    String::from_utf8_lossy(&output.stderr)
                        .lines()
                        .next()
                        .unwrap_or("(no error detail)")
                        .to_string()
                });
            return Err(AppError::Transient {
                code: "send_failed".into(),
                message: format!("email-cli send failed: {detail}"),
                suggestion: "Run `email-cli profile test` to verify Resend connectivity, or check that the sender email is configured as an account in email-cli".into(),
            });
        }
        let parsed = parsed_result.map_err(|e| AppError::Transient {
            code: "send_parse".into(),
            message: format!("invalid JSON from email-cli send: {e}"),
            suggestion: "Check email-cli version compatibility".into(),
        })?;
        // Prefer `remote_id` (Resend UUID) over `id` (local DB id).
        // `id` may be either a string or a number depending on email-cli version.
        let id = parsed
            .get("data")
            .and_then(|d| {
                d.get("remote_id")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or_else(|| d.get("id").and_then(|v| v.as_str()).map(String::from))
                    .or_else(|| d.get("id").and_then(|v| v.as_i64()).map(|n| n.to_string()))
            })
            .ok_or_else(|| AppError::Transient {
                code: "send_no_id".into(),
                message: "email-cli send response missing data.remote_id and data.id".into(),
                suggestion: "Check email-cli version compatibility".into(),
            })?;
        Ok(id)
    }

    /// Shell out to `email-cli email list --limit N [--after cursor]`.
    /// Returns the parsed response as a `serde_json::Value`.
    #[allow(dead_code)]
    pub fn email_list(&self, limit: usize, after: Option<&str>) -> Result<Value, AppError> {
        self.throttle();
        let mut args: Vec<String> = vec![
            "--json".into(),
            "email".into(),
            "list".into(),
            "--limit".into(),
            limit.to_string(),
        ];
        if let Some(cursor) = after {
            args.push("--after".into());
            args.push(cursor.into());
        }
        let output = Command::new(&self.path)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Config {
                code: "email_cli_invoke_failed".into(),
                message: format!("could not run email-cli email list: {e}"),
                suggestion: "Check that email-cli is on PATH".into(),
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Transient {
                code: "email_list_failed".into(),
                message: format!(
                    "email-cli email list failed: {}",
                    stderr.lines().next().unwrap_or("(no stderr)")
                ),
                suggestion: "Run `email-cli profile test` to verify connectivity".into(),
            });
        }
        serde_json::from_slice(&output.stdout).map_err(|e| AppError::Transient {
            code: "email_list_parse".into(),
            message: format!("invalid JSON from email-cli email list: {e}"),
            suggestion: "Check email-cli version compatibility".into(),
        })
    }

    /// Run `email-cli --json profile test <profile>`.
    #[allow(dead_code)]
    pub fn profile_test(&self) -> Result<Value, AppError> {
        self.throttle();
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
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn missing_email_cli_returns_config_error() {
        let cli = EmailCli::new("/nonexistent/path/to/email-cli", "default");
        let err = cli.agent_info().unwrap_err();
        assert_eq!(err.code(), "email_cli_not_found");
        assert!(err.suggestion().contains("Install"));
    }

    fn retry_stub_path() -> String {
        let manifest = env!("CARGO_MANIFEST_DIR");
        format!("{manifest}/tests/support/stub_email_cli.sh")
    }

    fn fresh_counter_file(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "stub-counter-{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_file(&path);
        path
    }

    fn write_minimal_batch_file(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "batch-{}-{}-{}.json",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(
            &path,
            r#"[{"from":"a@b.com","to":["c@d.com"],"subject":"s","html":"<p>hi</p>","text":"hi"}]"#,
        )
        .unwrap();
        path
    }

    #[test]
    fn batch_send_retries_on_429_then_succeeds() {
        let counter = fresh_counter_file("429");
        // SAFETY: tests run with --test-threads=1 per CI, and retry tests use
        // unique counter files to avoid interfering with each other.
        unsafe {
            std::env::set_var("STUB_EMAIL_CLI_FAIL_COUNT", "2");
            std::env::set_var("STUB_EMAIL_CLI_COUNTER_FILE", &counter);
            std::env::remove_var("STUB_EMAIL_CLI_PERMANENT_4XX");
        }

        let cli = EmailCli::new(retry_stub_path(), "test");
        let batch = write_minimal_batch_file("429");
        let result = cli.batch_send(&batch, &["c@d.com".to_string()]);

        unsafe {
            std::env::remove_var("STUB_EMAIL_CLI_FAIL_COUNT");
            std::env::remove_var("STUB_EMAIL_CLI_COUNTER_FILE");
        }
        let _ = fs::remove_file(&counter);
        let _ = fs::remove_file(&batch);

        assert!(
            result.is_ok(),
            "batch_send should retry through 2 429s then succeed, got: {result:?}"
        );
        let pairs = result.unwrap();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "c@d.com");
        assert_eq!(pairs[0].1, "em_stub_1");
    }

    #[test]
    fn batch_send_does_not_retry_on_permanent_4xx_error() {
        // SAFETY: see note above on env-var usage.
        unsafe {
            std::env::set_var("STUB_EMAIL_CLI_PERMANENT_4XX", "1");
            std::env::remove_var("STUB_EMAIL_CLI_FAIL_COUNT");
            std::env::remove_var("STUB_EMAIL_CLI_COUNTER_FILE");
        }

        let cli = EmailCli::new(retry_stub_path(), "test");
        let batch = write_minimal_batch_file("perm");
        let start = std::time::Instant::now();
        let result = cli.batch_send(&batch, &["c@d.com".to_string()]);
        let elapsed = start.elapsed();

        unsafe {
            std::env::remove_var("STUB_EMAIL_CLI_PERMANENT_4XX");
        }
        let _ = fs::remove_file(&batch);

        assert!(
            elapsed < std::time::Duration::from_millis(1_000),
            "permanent error should not retry; took {elapsed:?}"
        );
        assert!(result.is_err(), "permanent 4xx should return Err");
        match result.unwrap_err() {
            AppError::Transient { code, .. } => {
                assert_eq!(code, "batch_send_failed");
            }
            other => panic!("expected Transient, got {other:?}"),
        }
    }

    #[test]
    fn batch_send_rate_limited_variant_when_retries_exhausted() {
        // FAIL_COUNT larger than MAX_RETRIES + 1 (so every attempt fails 429)
        let counter = fresh_counter_file("exhausted");
        unsafe {
            std::env::set_var("STUB_EMAIL_CLI_FAIL_COUNT", "99");
            std::env::set_var("STUB_EMAIL_CLI_COUNTER_FILE", &counter);
            std::env::remove_var("STUB_EMAIL_CLI_PERMANENT_4XX");
        }

        let cli = EmailCli::new(retry_stub_path(), "test");
        let batch = write_minimal_batch_file("exhausted");
        let result = cli.batch_send(&batch, &["c@d.com".to_string()]);

        unsafe {
            std::env::remove_var("STUB_EMAIL_CLI_FAIL_COUNT");
            std::env::remove_var("STUB_EMAIL_CLI_COUNTER_FILE");
        }
        let _ = fs::remove_file(&counter);
        let _ = fs::remove_file(&batch);

        assert!(result.is_err(), "exhausted retries should return Err");
        match result.unwrap_err() {
            AppError::RateLimited { code, .. } => {
                assert_eq!(code, "batch_send_retries_exhausted");
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[test]
    fn is_retryable_classifier_recognises_429_and_5xx() {
        // Exit code 4 is unambiguously retryable.
        assert!(is_retryable_batch_error(Some(4), ""));
        // stderr signals
        assert!(is_retryable_batch_error(
            Some(1),
            "HTTP 429 Too Many Requests"
        ));
        assert!(is_retryable_batch_error(Some(1), "rate_limit exceeded"));
        assert!(is_retryable_batch_error(Some(1), "Rate limit exceeded"));
        assert!(is_retryable_batch_error(
            Some(1),
            "connection reset by peer"
        ));
        assert!(is_retryable_batch_error(Some(1), "request timed out"));
        assert!(is_retryable_batch_error(Some(1), "connection timeout"));
        assert!(is_retryable_batch_error(
            Some(1),
            "HTTP 500 Internal Server Error"
        ));
        assert!(is_retryable_batch_error(Some(1), "HTTP 502 Bad Gateway"));
        assert!(is_retryable_batch_error(
            Some(1),
            "HTTP 503 Service Unavailable"
        ));
        assert!(is_retryable_batch_error(
            Some(1),
            "HTTP 504 Gateway Timeout"
        ));
        // Permanent 4xx classes are NOT retryable.
        assert!(!is_retryable_batch_error(
            Some(3),
            "HTTP 422 Unprocessable Entity: invalid from address"
        ));
        assert!(!is_retryable_batch_error(Some(3), "HTTP 401 Unauthorized"));
        assert!(!is_retryable_batch_error(Some(3), "HTTP 400 Bad Request"));
    }
}
