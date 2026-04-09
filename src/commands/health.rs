use crate::config::Config;
use crate::db::Db;
use crate::email_cli::EmailCli;
use crate::error::AppError;
use crate::output::{self, Format};
use serde_json::json;

pub fn run(format: Format) -> Result<(), AppError> {
    let mut checks: Vec<(&str, &str, String)> = vec![];

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
                    "status": "fail",
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

    // 5. (v0.3) sender domain is verified in Resend. We cannot currently
    // surface open/click tracking config because email-cli v0.6.3's
    // `domain list` output doesn't include those fields — that surfacing is
    // deferred to v0.3.1 pending an upstream fix. What we CAN do right now
    // is confirm the sender domain is registered and verified, which
    // catches the most common "why didn't my broadcast send" failure mode.
    if let Some(from) = config.sender.from.as_deref() {
        if let Some(at) = from.rfind('@') {
            let sender_domain = &from[at + 1..];
            match cli.domain_list() {
                Ok(domains) => {
                    let matching = domains.iter().find(|d| {
                        d.get("name")
                            .and_then(|v| v.as_str())
                            .map(|n| n.eq_ignore_ascii_case(sender_domain))
                            .unwrap_or(false)
                    });
                    match matching {
                        Some(d) => {
                            let status = d
                                .get("status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            if status == "verified" {
                                checks.push((
                                    "sender_domain_verified",
                                    "ok",
                                    format!("{sender_domain} verified in Resend"),
                                ));
                            } else {
                                checks.push((
                                    "sender_domain_verified",
                                    "warn",
                                    format!(
                                        "{sender_domain} is registered in Resend but status is '{status}' — runs will bounce at the Resend boundary until the domain is verified. Enable DNS records and run `email-cli domain verify`."
                                    ),
                                ));
                            }
                        }
                        None => {
                            checks.push((
                                "sender_domain_verified",
                                "warn",
                                format!(
                                    "{sender_domain} is not registered in Resend. Run `email-cli domain create {sender_domain}` followed by `email-cli domain verify` before sending broadcasts."
                                ),
                            ));
                        }
                    }
                }
                Err(e) => {
                    // email-cli itself already has a check above; if that
                    // passed but domain_list failed, still flag it.
                    checks.push((
                        "sender_domain_verified",
                        "warn",
                        format!("could not query domain list: {}", e.message()),
                    ));
                }
            }
        }
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
