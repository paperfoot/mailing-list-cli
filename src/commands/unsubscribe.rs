use crate::broadcast::verify_token;
use crate::cli::{UnsubscribeAction, UnsubscribeSyncArgs};
use crate::config::Config;
use crate::db::Db;
use crate::error::AppError;
use crate::output::{self, Format};
use serde::Deserialize;
use serde_json::json;

const CURSOR_KEY: &str = "unsubscribe_sync_cursor";

pub fn run(format: Format, action: UnsubscribeAction) -> Result<(), AppError> {
    match action {
        UnsubscribeAction::Sync(args) => sync(format, args),
    }
}

#[derive(Debug, Deserialize)]
struct SyncEnvelope {
    status: String,
    data: Option<SyncData>,
    error: Option<SyncError>,
}

#[derive(Debug, Deserialize)]
struct SyncError {
    code: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SyncData {
    events: Vec<HostedUnsubscribeEvent>,
    next_cursor: i64,
    has_more: bool,
}

#[derive(Debug, Deserialize)]
struct HostedUnsubscribeEvent {
    id: i64,
    token: String,
    contact_id: i64,
    broadcast_id: i64,
    issued_at: i64,
}

#[derive(Default)]
struct SyncStats {
    fetched: usize,
    verified: usize,
    applied: usize,
    duplicates: usize,
    missing_contacts: usize,
    pages: usize,
    start_cursor: i64,
    next_cursor: i64,
    has_more: bool,
}

fn sync(format: Format, args: UnsubscribeSyncArgs) -> Result<(), AppError> {
    validate_args(&args)?;
    let config = Config::load()?;
    let db = Db::open()?;
    let endpoint = match args.endpoint {
        Some(endpoint) => endpoint,
        None => derive_sync_endpoint(&config.unsubscribe.public_url)?,
    };
    let api_key = resolve_api_key(&args.api_key_env)?;
    let secret = std::env::var(&config.unsubscribe.secret_env).map_err(|_| AppError::Config {
        code: "missing_unsubscribe_secret".into(),
        message: format!(
            "environment variable `{}` is not set; cannot verify hosted unsubscribe tokens",
            config.unsubscribe.secret_env
        ),
        suggestion: format!(
            "Set `{}` to the same value used by the SharpClap/Vercel app",
            config.unsubscribe.secret_env
        ),
    })?;

    let start_cursor = match args.after {
        Some(cursor) => cursor,
        None => saved_cursor(&db)?,
    };
    let mut stats = SyncStats {
        start_cursor,
        next_cursor: start_cursor,
        ..SyncStats::default()
    };

    for _ in 0..args.max_pages {
        let data = fetch_page(&endpoint, &api_key, stats.next_cursor, args.limit)?;
        stats.pages += 1;
        stats.has_more = data.has_more;

        if data.events.is_empty() {
            stats.next_cursor = data.next_cursor;
            break;
        }

        for event in &data.events {
            apply_event(&db, &secret, event, args.dry_run, &mut stats)?;
        }

        stats.next_cursor = data.next_cursor;
        if !args.dry_run {
            db.kv_set(CURSOR_KEY, &stats.next_cursor.to_string())?;
        }

        if !data.has_more {
            break;
        }
    }

    output::success(
        format,
        &format!(
            "unsubscribe sync: {} applied, {} duplicates, {} missing contacts",
            stats.applied, stats.duplicates, stats.missing_contacts
        ),
        json!({
            "endpoint": endpoint,
            "dry_run": args.dry_run,
            "pages": stats.pages,
            "fetched": stats.fetched,
            "verified": stats.verified,
            "applied": stats.applied,
            "duplicates": stats.duplicates,
            "missing_contacts": stats.missing_contacts,
            "start_cursor": stats.start_cursor,
            "next_cursor": stats.next_cursor,
            "has_more": stats.has_more,
        }),
    );
    Ok(())
}

fn validate_args(args: &UnsubscribeSyncArgs) -> Result<(), AppError> {
    if args.limit == 0 || args.limit > 500 {
        return Err(AppError::BadInput {
            code: "bad_unsubscribe_sync_limit".into(),
            message: format!("--limit must be between 1 and 500, got {}", args.limit),
            suggestion: "Use the default --limit 100 unless you have a reason to tune it".into(),
        });
    }
    if args.max_pages == 0 {
        return Err(AppError::BadInput {
            code: "bad_unsubscribe_sync_max_pages".into(),
            message: "--max-pages must be at least 1".into(),
            suggestion: "Use the default --max-pages 20".into(),
        });
    }
    if let Some(after) = args.after {
        if after < 0 {
            return Err(AppError::BadInput {
                code: "bad_unsubscribe_sync_cursor".into(),
                message: "--after cannot be negative".into(),
                suggestion: "Pass --after 0 to start from the beginning".into(),
            });
        }
    }
    Ok(())
}

fn saved_cursor(db: &Db) -> Result<i64, AppError> {
    match db.kv_get(CURSOR_KEY)? {
        Some(value) => value.parse::<i64>().map_err(|_| AppError::BadInput {
            code: "bad_unsubscribe_sync_saved_cursor".into(),
            message: format!("saved unsubscribe sync cursor `{value}` is not an integer"),
            suggestion: format!(
                "Reset it with `sqlite3 STATE.DB \"DELETE FROM kv WHERE key='{CURSOR_KEY}'\"`"
            ),
        }),
        None => Ok(0),
    }
}

fn derive_sync_endpoint(public_url: &str) -> Result<String, AppError> {
    if public_url.contains("yourdomain.com") {
        return Err(AppError::Config {
            code: "unsubscribe_public_url_not_configured".into(),
            message: format!("[unsubscribe].public_url is still `{public_url}`"),
            suggestion:
                "Set [unsubscribe].public_url = \"https://sharpclap.com/u\" or pass --endpoint"
                    .into(),
        });
    }

    let trimmed = public_url.trim_end_matches('/');
    if let Some(prefix) = trimmed.strip_suffix("/u") {
        return Ok(format!("{prefix}/api/unsubscribes"));
    }
    Ok(format!("{trimmed}/api/unsubscribes"))
}

fn resolve_api_key(api_key_env: &str) -> Result<String, AppError> {
    std::env::var(api_key_env)
        .or_else(|_| {
            if api_key_env == "SYNC_API_KEY" {
                Err(std::env::VarError::NotPresent)
            } else {
                std::env::var("SYNC_API_KEY")
            }
        })
        .map_err(|_| AppError::Config {
            code: "missing_unsubscribe_sync_key".into(),
            message: format!("neither `{api_key_env}` nor `SYNC_API_KEY` is set"),
            suggestion: "Set MLC_UNSUBSCRIBE_SYNC_KEY to the SharpClap sync API key".into(),
        })
}

fn fetch_page(
    endpoint: &str,
    api_key: &str,
    after: i64,
    limit: usize,
) -> Result<SyncData, AppError> {
    let separator = if endpoint.contains('?') { '&' } else { '?' };
    let url = format!("{endpoint}{separator}after={after}&limit={limit}");
    let response = ureq::get(&url)
        .set("Authorization", &format!("Bearer {api_key}"))
        .call();

    let envelope = match response {
        Ok(resp) => resp
            .into_json::<SyncEnvelope>()
            .map_err(|e| AppError::Transient {
                code: "unsubscribe_sync_bad_json".into(),
                message: format!("invalid JSON from unsubscribe sync endpoint: {e}"),
                suggestion: "Check that the endpoint is the SharpClap /api/unsubscribes route"
                    .into(),
            })?,
        Err(ureq::Error::Status(status, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            return Err(AppError::Transient {
                code: "unsubscribe_sync_http_error".into(),
                message: format!("unsubscribe sync endpoint returned HTTP {status}: {body}"),
                suggestion: "Check SYNC_API_KEY and the SharpClap deployment health".into(),
            });
        }
        Err(e) => {
            return Err(AppError::Transient {
                code: "unsubscribe_sync_network_error".into(),
                message: format!("could not call unsubscribe sync endpoint: {e}"),
                suggestion: "Check network access and the endpoint URL".into(),
            });
        }
    };

    if envelope.status != "success" {
        let (code, message) = envelope
            .error
            .map(|err| {
                (
                    err.code.unwrap_or_else(|| "unsubscribe_sync_error".into()),
                    err.message
                        .unwrap_or_else(|| "sync endpoint returned an error".into()),
                )
            })
            .unwrap_or_else(|| {
                (
                    "unsubscribe_sync_error".into(),
                    "sync endpoint returned an error".into(),
                )
            });
        return Err(AppError::Transient {
            code,
            message,
            suggestion: "Check SharpClap /api/health and the sync API key".into(),
        });
    }

    envelope.data.ok_or_else(|| AppError::Transient {
        code: "unsubscribe_sync_missing_data".into(),
        message: "sync endpoint returned success without data".into(),
        suggestion: "Check that the endpoint is compatible with this mailing-list-cli version"
            .into(),
    })
}

fn apply_event(
    db: &Db,
    secret: &str,
    event: &HostedUnsubscribeEvent,
    dry_run: bool,
    stats: &mut SyncStats,
) -> Result<(), AppError> {
    stats.fetched += 1;
    let (contact_id, broadcast_id, issued_at) = verify_token(secret.as_bytes(), &event.token)
        .map_err(|e| AppError::BadInput {
            code: "unsubscribe_sync_bad_token".into(),
            message: format!(
                "hosted unsubscribe event {} has an invalid token: {e}",
                event.id
            ),
            suggestion:
                "Confirm SharpClap and mailing-list-cli use the same MLC_UNSUBSCRIBE_SECRET".into(),
        })?;

    if contact_id != event.contact_id
        || broadcast_id != event.broadcast_id
        || issued_at != event.issued_at
    {
        return Err(AppError::BadInput {
            code: "unsubscribe_sync_event_mismatch".into(),
            message: format!(
                "hosted unsubscribe event {} payload does not match its token",
                event.id
            ),
            suggestion: "Treat the hosted event feed as compromised until inspected".into(),
        });
    }

    stats.verified += 1;
    if dry_run {
        return Ok(());
    }

    let is_new =
        db.unsubscribe_sync_event_insert(event.id, &event.token, contact_id, broadcast_id)?;
    if !is_new {
        stats.duplicates += 1;
        return Ok(());
    }

    match db.contact_get_by_id(contact_id)? {
        Some(contact) => {
            db.suppression_insert(&contact.email, "unsubscribed", Some(broadcast_id))?;
            db.contact_set_status(&contact.email, "unsubscribed")?;
            db.broadcast_increment_stat(broadcast_id, "unsubscribed_count")?;
            stats.applied += 1;
        }
        None => {
            stats.missing_contacts += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_sync_endpoint_from_unsubscribe_url() {
        assert_eq!(
            derive_sync_endpoint("https://sharpclap.com/u").unwrap(),
            "https://sharpclap.com/api/unsubscribes"
        );
        assert_eq!(
            derive_sync_endpoint("https://sharpclap.com/u/").unwrap(),
            "https://sharpclap.com/api/unsubscribes"
        );
    }
}
