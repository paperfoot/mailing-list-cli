use crate::broadcast::pipeline;
use crate::cli::{
    BroadcastAction, BroadcastCancelArgs, BroadcastCreateArgs, BroadcastListArgs,
    BroadcastPreviewArgs, BroadcastScheduleArgs, BroadcastSendArgs, BroadcastShowArgs,
};
use crate::db::Db;
use crate::error::AppError;
use crate::output::{self, Format};
use serde_json::json;

pub fn run(format: Format, action: BroadcastAction) -> Result<(), AppError> {
    match action {
        BroadcastAction::Create(args) => create(format, args),
        BroadcastAction::Preview(args) => preview(format, args),
        BroadcastAction::Schedule(args) => schedule(format, args),
        // Send and Resume are the same handler — both call the pipeline,
        // which skips already-sent recipients. Resume is just a clearer
        // name for recovering from a mid-send crash.
        BroadcastAction::Send(args) => send(format, args),
        BroadcastAction::Resume(args) => send(format, args),
        BroadcastAction::Cancel(args) => cancel(format, args),
        BroadcastAction::List(args) => list(format, args),
        BroadcastAction::Show(args) => show(format, args),
    }
}

fn create(format: Format, args: BroadcastCreateArgs) -> Result<(), AppError> {
    let db = Db::open()?;

    // Resolve template
    let template = db
        .template_get_by_name(&args.template)?
        .ok_or_else(|| AppError::BadInput {
            code: "template_not_found".into(),
            message: format!("no template named '{}'", args.template),
            suggestion: "Run `mailing-list-cli template ls`".into(),
        })?;

    // Parse `list:<name>` or `segment:<name>`
    let (kind, name) = args.to.split_once(':').ok_or_else(|| AppError::BadInput {
        code: "bad_target_syntax".into(),
        message: format!(
            "--to '{}' must be `list:<name>` or `segment:<name>`",
            args.to
        ),
        suggestion: "Example: --to list:newsletter or --to segment:vips".into(),
    })?;

    let (target_kind, target_id) = match kind {
        "list" => {
            let list = db
                .list_get_by_name(name)?
                .ok_or_else(|| AppError::BadInput {
                    code: "list_not_found".into(),
                    message: format!("no list named '{name}'"),
                    suggestion: "Run `mailing-list-cli list ls`".into(),
                })?;
            ("list", list.id)
        }
        "segment" => {
            let segment = db
                .segment_get_by_name(name)?
                .ok_or_else(|| AppError::BadInput {
                    code: "segment_not_found".into(),
                    message: format!("no segment named '{name}'"),
                    suggestion: "Run `mailing-list-cli segment ls`".into(),
                })?;
            ("segment", segment.id)
        }
        other => {
            return Err(AppError::BadInput {
                code: "bad_target_kind".into(),
                message: format!("target kind '{other}' not recognized"),
                suggestion: "Use `list:<name>` or `segment:<name>`".into(),
            });
        }
    };

    let id = db.broadcast_create(&args.name, template.id, target_kind, target_id)?;
    let broadcast = db
        .broadcast_get(id)?
        .expect("just-created broadcast must exist");
    output::success(
        format,
        &format!("broadcast '{}' created as draft", args.name),
        json!({ "broadcast": broadcast }),
    );
    Ok(())
}

fn preview(format: Format, args: BroadcastPreviewArgs) -> Result<(), AppError> {
    let result = pipeline::preview_broadcast(args.id, &args.to)?;
    output::success(
        format,
        &format!("preview sent to {}", args.to),
        json!({ "broadcast_id": args.id, "to": args.to, "result": result }),
    );
    Ok(())
}

fn schedule(format: Format, args: BroadcastScheduleArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let _ = chrono::DateTime::parse_from_rfc3339(&args.at).map_err(|e| AppError::BadInput {
        code: "bad_scheduled_at".into(),
        message: format!("'{}' is not a valid RFC 3339 timestamp: {e}", args.at),
        suggestion: "Use e.g. 2026-04-09T12:00:00Z".into(),
    })?;
    let b = db
        .broadcast_get(args.id)?
        .ok_or_else(|| AppError::BadInput {
            code: "broadcast_not_found".into(),
            message: format!("no broadcast with id {}", args.id),
            suggestion: "Run `mailing-list-cli broadcast ls`".into(),
        })?;
    if b.status != "draft" {
        return Err(AppError::BadInput {
            code: "broadcast_bad_status".into(),
            message: format!(
                "broadcast is in status '{}' — only draft can be scheduled",
                b.status
            ),
            suggestion: "Re-create or use `broadcast cancel` first".into(),
        });
    }
    db.broadcast_set_scheduled(args.id, &args.at)?;
    output::success(
        format,
        &format!("broadcast {} scheduled for {}", args.id, args.at),
        json!({ "id": args.id, "scheduled_at": args.at }),
    );
    Ok(())
}

fn send(format: Format, args: BroadcastSendArgs) -> Result<(), AppError> {
    let result = pipeline::send_broadcast(args.id)?;
    output::success(
        format,
        &format!("broadcast {} sent", args.id),
        json!({
            "broadcast_id": result.broadcast_id,
            "sent": result.sent_count,
            "suppressed": result.suppressed_count,
            "failed": result.failed_count
        }),
    );
    Ok(())
}

fn cancel(format: Format, args: BroadcastCancelArgs) -> Result<(), AppError> {
    if !args.confirm {
        return Err(AppError::BadInput {
            code: "confirmation_required".into(),
            message: format!("cancelling broadcast {} requires --confirm", args.id),
            suggestion: format!(
                "rerun with `mailing-list-cli broadcast cancel {} --confirm`",
                args.id
            ),
        });
    }
    let db = Db::open()?;
    let b = db
        .broadcast_get(args.id)?
        .ok_or_else(|| AppError::BadInput {
            code: "broadcast_not_found".into(),
            message: format!("no broadcast with id {}", args.id),
            suggestion: "Run `mailing-list-cli broadcast ls`".into(),
        })?;
    if !matches!(b.status.as_str(), "draft" | "scheduled") {
        return Err(AppError::BadInput {
            code: "broadcast_bad_status".into(),
            message: format!(
                "broadcast is in status '{}' — only draft/scheduled can be cancelled",
                b.status
            ),
            suggestion: "Already-sent broadcasts cannot be cancelled".into(),
        });
    }
    db.broadcast_set_status(args.id, "cancelled", None)?;
    output::success(
        format,
        &format!("broadcast {} cancelled", args.id),
        json!({ "id": args.id, "status": "cancelled" }),
    );
    Ok(())
}

fn list(format: Format, args: BroadcastListArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let broadcasts = db.broadcast_all(args.status.as_deref(), args.limit)?;
    let count = broadcasts.len();
    output::success(
        format,
        &format!("{count} broadcast(s)"),
        json!({ "broadcasts": broadcasts, "count": count }),
    );
    Ok(())
}

fn show(format: Format, args: BroadcastShowArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let broadcast = db
        .broadcast_get(args.id)?
        .ok_or_else(|| AppError::BadInput {
            code: "broadcast_not_found".into(),
            message: format!("no broadcast with id {}", args.id),
            suggestion: "Run `mailing-list-cli broadcast ls`".into(),
        })?;
    output::success(
        format,
        &format!("broadcast: {}", broadcast.name),
        json!({ "broadcast": broadcast }),
    );
    Ok(())
}
