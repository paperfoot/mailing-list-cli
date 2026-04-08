//! Shared event handler. Both `event poll` and `webhook listen` call this.

use crate::db::Db;
use crate::error::AppError;
use crate::webhook::types::{ResendEvent, ResendEventType};

const SOFT_BOUNCE_STREAK_LIMIT: i64 = 5;

pub fn handle_event(db: &Db, ev: &ResendEvent) -> Result<HandleOutcome, AppError> {
    let payload_json = serde_json::to_string(ev).unwrap_or_default();
    let email_id = ev.data.email_id.clone();
    let event_type = ev.event_type.as_str();

    // Look up the broadcast_recipient first so we can attribute the event
    let recipient = db.recipient_by_resend_email_id(&email_id)?;
    let (broadcast_id, contact_id) = match recipient {
        Some(t) => (Some(t.0), Some(t.1)),
        None => (None, None),
    };

    // Idempotent insert into event table
    let inserted = db.event_insert(
        event_type,
        &email_id,
        broadcast_id,
        contact_id,
        &payload_json,
    )?;
    if !inserted {
        return Ok(HandleOutcome::Duplicate);
    }

    // Dispatch by event type
    match ev.event_type {
        ResendEventType::Delivered => {
            if let (Some(bid), Some(cid)) = (broadcast_id, contact_id) {
                db.broadcast_recipient_update_status(bid, cid, "delivered")?;
                db.broadcast_increment_stat(bid, "delivered_count")?;
                db.soft_bounce_reset(cid)?;
            }
        }
        ResendEventType::Bounced => {
            let is_permanent = ev
                .data
                .bounce
                .as_ref()
                .map(|b| b.bounce_type.eq_ignore_ascii_case("Permanent"))
                .unwrap_or(false);
            if is_permanent {
                if let (Some(bid), Some(cid)) = (broadcast_id, contact_id) {
                    db.broadcast_recipient_update_status(bid, cid, "bounced")?;
                    db.broadcast_increment_stat(bid, "bounced_count")?;
                }
                // Auto-suppress
                for to in &ev.data.to {
                    db.suppression_insert(to, "hard_bounced", broadcast_id)?;
                    db.contact_set_status(to, "bounced")?;
                }
            }
        }
        ResendEventType::DeliveryDelayed => {
            if let Some(cid) = contact_id {
                let streak = db.soft_bounce_increment(cid)?;
                if streak >= SOFT_BOUNCE_STREAK_LIMIT {
                    // Promote to permanent suppression
                    for to in &ev.data.to {
                        db.suppression_insert(to, "soft_bounced_repeat", broadcast_id)?;
                        db.contact_set_status(to, "bounced")?;
                    }
                }
            }
        }
        ResendEventType::Complained => {
            if let (Some(bid), Some(cid)) = (broadcast_id, contact_id) {
                db.broadcast_recipient_update_status(bid, cid, "complained")?;
                db.broadcast_increment_stat(bid, "complained_count")?;
                let _ = cid;
            }
            for to in &ev.data.to {
                db.suppression_insert(to, "complained", broadcast_id)?;
                db.contact_set_status(to, "complained")?;
            }
        }
        ResendEventType::Opened => {
            if let Some(bid) = broadcast_id {
                db.broadcast_increment_stat(bid, "opened_count")?;
            }
        }
        ResendEventType::Clicked => {
            if let Some(bid) = broadcast_id {
                db.broadcast_increment_stat(bid, "clicked_count")?;
                if let Some(click) = &ev.data.click {
                    db.click_insert(
                        bid,
                        contact_id,
                        &click.link,
                        click.ip_address.as_deref(),
                        click.user_agent.as_deref(),
                    )?;
                }
            }
        }
        ResendEventType::Suppressed => {
            // Already suppressed — no action beyond the event row
        }
        ResendEventType::Failed => {
            if let (Some(bid), Some(cid)) = (broadcast_id, contact_id) {
                db.broadcast_recipient_update_status(bid, cid, "failed")?;
            }
        }
        ResendEventType::Sent | ResendEventType::Scheduled | ResendEventType::Unknown => {
            // Informational / unknown events get logged but produce no state mutation
        }
    }

    Ok(HandleOutcome::Processed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleOutcome {
    Processed,
    Duplicate,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webhook::types::*;

    fn fresh_db() -> Db {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        Db::open_at(tmp.path()).unwrap()
    }

    fn seed_broadcast_with_recipient(db: &Db) -> (i64, i64, String) {
        let tid = db.template_upsert("t", "Hi", "<p>Hi</p>").unwrap();
        let list_id = db.list_create("news", None, "seg_x").unwrap();
        let bid = db.broadcast_create("Q1", tid, "list", list_id).unwrap();
        let cid = db.contact_upsert("alice@example.com", None, None).unwrap();
        db.broadcast_recipient_insert(bid, cid, "sent").unwrap();
        db.conn
            .execute(
                "UPDATE broadcast_recipient SET resend_email_id = 'em_x' WHERE broadcast_id = ?1 AND contact_id = ?2",
                [bid, cid],
            )
            .unwrap();
        (bid, cid, "em_x".into())
    }

    #[test]
    fn delivered_updates_broadcast_and_recipient() {
        let db = fresh_db();
        let (bid, cid, email_id) = seed_broadcast_with_recipient(&db);

        let ev = ResendEvent {
            event_type: ResendEventType::Delivered,
            created_at: "2026-04-08T12:00:00Z".into(),
            data: ResendEventData {
                email_id,
                to: vec!["alice@example.com".into()],
                subject: None,
                bounce: None,
                click: None,
                complaint_type: None,
                tags: serde_json::Value::Null,
            },
        };
        let outcome = handle_event(&db, &ev).unwrap();
        assert_eq!(outcome, HandleOutcome::Processed);

        let b = db.broadcast_get(bid).unwrap().unwrap();
        assert_eq!(b.delivered_count, 1);
        assert_eq!(
            db.broadcast_recipient_count_by_status(bid, "delivered")
                .unwrap(),
            1
        );
        let _ = cid;
    }

    #[test]
    fn bounced_permanent_auto_suppresses() {
        let db = fresh_db();
        let (bid, _cid, email_id) = seed_broadcast_with_recipient(&db);
        let ev = ResendEvent {
            event_type: ResendEventType::Bounced,
            created_at: "2026-04-08T12:00:00Z".into(),
            data: ResendEventData {
                email_id,
                to: vec!["alice@example.com".into()],
                subject: None,
                bounce: Some(BounceInfo {
                    bounce_type: "Permanent".into(),
                    message: None,
                    subtype: None,
                }),
                click: None,
                complaint_type: None,
                tags: serde_json::Value::Null,
            },
        };
        handle_event(&db, &ev).unwrap();
        let b = db.broadcast_get(bid).unwrap().unwrap();
        assert_eq!(b.bounced_count, 1);
        assert!(db.is_email_suppressed("alice@example.com").unwrap());
    }

    #[test]
    fn clicked_inserts_click_row_and_increments() {
        let db = fresh_db();
        let (bid, _cid, email_id) = seed_broadcast_with_recipient(&db);
        let ev = ResendEvent {
            event_type: ResendEventType::Clicked,
            created_at: "2026-04-08T12:00:00Z".into(),
            data: ResendEventData {
                email_id,
                to: vec!["alice@example.com".into()],
                subject: None,
                bounce: None,
                click: Some(ClickInfo {
                    link: "https://example.com/cta".into(),
                    ip_address: Some("1.2.3.4".into()),
                    user_agent: Some("test".into()),
                    timestamp: None,
                }),
                complaint_type: None,
                tags: serde_json::Value::Null,
            },
        };
        handle_event(&db, &ev).unwrap();
        let b = db.broadcast_get(bid).unwrap().unwrap();
        assert_eq!(b.clicked_count, 1);
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM click WHERE broadcast_id = ?1",
                [bid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn duplicate_event_returns_duplicate_outcome() {
        let db = fresh_db();
        let (_bid, _cid, email_id) = seed_broadcast_with_recipient(&db);
        let ev = ResendEvent {
            event_type: ResendEventType::Delivered,
            created_at: "2026-04-08T12:00:00Z".into(),
            data: ResendEventData {
                email_id,
                to: vec!["alice@example.com".into()],
                subject: None,
                bounce: None,
                click: None,
                complaint_type: None,
                tags: serde_json::Value::Null,
            },
        };
        assert_eq!(handle_event(&db, &ev).unwrap(), HandleOutcome::Processed);
        assert_eq!(handle_event(&db, &ev).unwrap(), HandleOutcome::Duplicate);
    }
}
