//! Poll-based event ingestion via `email-cli email list`.
//!
//! The `email list` command returns each email with a `last_event` field
//! (e.g., "delivered", "bounced"). We translate that into a ResendEvent and
//! feed it to the shared handler.

use crate::db::Db;
use crate::email_cli::EmailCli;
use crate::error::AppError;
use crate::webhook::dispatch::{HandleOutcome, handle_event};
use crate::webhook::types::*;
use serde_json::Value;

const CURSOR_KEY: &str = "webhook.poll.last_email_id";
const DEFAULT_PAGE_SIZE: usize = 100;

pub struct PollResult {
    pub processed: usize,
    pub duplicates: usize,
    pub latest_cursor: Option<String>,
}

pub fn poll_events(db: &Db, cli: &EmailCli, reset_cursor: bool) -> Result<PollResult, AppError> {
    if reset_cursor {
        db.kv_set(CURSOR_KEY, "")?;
    }
    let cursor = db.kv_get(CURSOR_KEY)?.filter(|s| !s.is_empty());
    let response = cli.email_list(DEFAULT_PAGE_SIZE, cursor.as_deref())?;

    let data = response
        .get("data")
        .and_then(|d| d.get("data"))
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();

    let mut processed = 0;
    let mut duplicates = 0;
    let mut latest_cursor = cursor;

    for entry in &data {
        let Some(email_id) = entry.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(last_event) = entry.get("last_event").and_then(|v| v.as_str()) else {
            continue;
        };
        let event_type = map_last_event_to_type(last_event);
        if matches!(event_type, ResendEventType::Unknown) {
            continue;
        }
        let to_array: Vec<String> = entry
            .get("to")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let ev = ResendEvent {
            event_type,
            created_at: entry
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            data: ResendEventData {
                email_id: email_id.to_string(),
                to: to_array,
                subject: entry
                    .get("subject")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                bounce: None, // poll path doesn't expose bounce subtype
                click: None,
                complaint_type: None,
                tags: entry.get("tags").cloned().unwrap_or(Value::Null),
            },
        };
        match handle_event(db, &ev)? {
            HandleOutcome::Processed => processed += 1,
            HandleOutcome::Duplicate => duplicates += 1,
        }
        latest_cursor = Some(email_id.to_string());
    }

    if let Some(c) = &latest_cursor {
        db.kv_set(CURSOR_KEY, c)?;
    }

    Ok(PollResult {
        processed,
        duplicates,
        latest_cursor,
    })
}

fn map_last_event_to_type(s: &str) -> ResendEventType {
    match s {
        "sent" => ResendEventType::Sent,
        "delivered" => ResendEventType::Delivered,
        "delivery_delayed" => ResendEventType::DeliveryDelayed,
        "bounced" => ResendEventType::Bounced,
        "complained" => ResendEventType::Complained,
        "opened" => ResendEventType::Opened,
        "clicked" => ResendEventType::Clicked,
        "failed" => ResendEventType::Failed,
        _ => ResendEventType::Unknown,
    }
}
