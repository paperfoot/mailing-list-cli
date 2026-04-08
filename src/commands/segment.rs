use crate::cli::{
    SegmentAction, SegmentCreateArgs, SegmentMembersArgs, SegmentRmArgs, SegmentShowArgs,
};
use crate::db::Db;
use crate::error::AppError;
use crate::output::{self, Format};
use crate::segment::{compiler, parser};
use serde_json::json;

pub fn run(format: Format, action: SegmentAction) -> Result<(), AppError> {
    let db = Db::open()?;
    match action {
        SegmentAction::Create(args) => create(format, &db, args),
        SegmentAction::List => list(format, &db),
        SegmentAction::Show(args) => show(format, &db, args),
        SegmentAction::Members(args) => members(format, &db, args),
        SegmentAction::Rm(args) => remove(format, &db, args),
    }
}

fn parse_and_store(expr: &str) -> Result<(crate::segment::SegmentExpr, String), AppError> {
    let parsed = parser::parse(expr).map_err(|e| AppError::BadInput {
        code: "invalid_filter_expression".into(),
        message: e.message.clone(),
        suggestion: e.suggestion.clone(),
    })?;
    let filter_json = serde_json::to_string(&parsed).map_err(|e| AppError::Transient {
        code: "segment_serialize_failed".into(),
        message: format!("could not serialize SegmentExpr: {e}"),
        suggestion: "Report as a bug".into(),
    })?;
    Ok((parsed, filter_json))
}

fn create(format: Format, db: &Db, args: SegmentCreateArgs) -> Result<(), AppError> {
    let (_expr, filter_json) = parse_and_store(&args.filter)?;
    let id = db.segment_create(&args.name, &filter_json)?;
    output::success(
        format,
        &format!("segment created: {}", args.name),
        json!({
            "id": id,
            "name": args.name,
            "filter": args.filter
        }),
    );
    Ok(())
}

fn list(format: Format, db: &Db) -> Result<(), AppError> {
    let segments = db.segment_all()?;
    // Compute member counts per segment via the compiler
    let mut enriched = Vec::with_capacity(segments.len());
    for s in segments {
        let expr: crate::segment::SegmentExpr =
            serde_json::from_str(&s.filter_json).map_err(|e| AppError::Transient {
                code: "segment_deserialize_failed".into(),
                message: format!("corrupted segment '{}': {e}", s.name),
                suggestion: "Recreate the segment with `segment rm` + `segment create`".into(),
            })?;
        let (frag, params) = compiler::to_sql_where(&expr);
        let count = db.segment_count_members(&frag, &params)?;
        enriched.push(json!({
            "id": s.id,
            "name": s.name,
            "created_at": s.created_at,
            "member_count": count
        }));
    }
    let count = enriched.len();
    output::success(
        format,
        &format!("{count} segment(s)"),
        json!({ "segments": enriched, "count": count }),
    );
    Ok(())
}

fn show(format: Format, db: &Db, args: SegmentShowArgs) -> Result<(), AppError> {
    let segment = db
        .segment_get_by_name(&args.name)?
        .ok_or_else(|| AppError::BadInput {
            code: "segment_not_found".into(),
            message: format!("no segment named '{}'", args.name),
            suggestion: "Run `mailing-list-cli segment ls`".into(),
        })?;
    let expr: crate::segment::SegmentExpr =
        serde_json::from_str(&segment.filter_json).map_err(|e| AppError::Transient {
            code: "segment_deserialize_failed".into(),
            message: format!("corrupted segment: {e}"),
            suggestion: "Recreate the segment".into(),
        })?;
    let (frag, params) = compiler::to_sql_where(&expr);
    let member_count = db.segment_count_members(&frag, &params)?;
    let sample = db.segment_members(&frag, &params, 10, None)?;
    output::success(
        format,
        &format!("segment: {}", segment.name),
        json!({
            "id": segment.id,
            "name": segment.name,
            "filter_json": segment.filter_json,
            "filter_ast": expr,
            "created_at": segment.created_at,
            "member_count": member_count,
            "sample": sample
        }),
    );
    Ok(())
}

fn members(format: Format, db: &Db, args: SegmentMembersArgs) -> Result<(), AppError> {
    let segment = db
        .segment_get_by_name(&args.name)?
        .ok_or_else(|| AppError::BadInput {
            code: "segment_not_found".into(),
            message: format!("no segment named '{}'", args.name),
            suggestion: "Run `mailing-list-cli segment ls`".into(),
        })?;
    let expr: crate::segment::SegmentExpr =
        serde_json::from_str(&segment.filter_json).map_err(|e| AppError::Transient {
            code: "segment_deserialize_failed".into(),
            message: format!("corrupted segment: {e}"),
            suggestion: "Recreate the segment".into(),
        })?;
    let (frag, params) = compiler::to_sql_where(&expr);
    let contacts = db.segment_members(&frag, &params, args.limit, args.cursor)?;
    let next_cursor = contacts.last().map(|c| c.id);
    let count = contacts.len();
    output::success(
        format,
        &format!("{count} contact(s) in segment '{}'", segment.name),
        json!({
            "segment": segment.name,
            "contacts": contacts,
            "count": count,
            "next_cursor": next_cursor
        }),
    );
    Ok(())
}

fn remove(format: Format, db: &Db, args: SegmentRmArgs) -> Result<(), AppError> {
    if !args.confirm {
        return Err(AppError::BadInput {
            code: "confirmation_required".into(),
            message: format!("deleting segment '{}' requires --confirm", args.name),
            suggestion: format!(
                "rerun with `mailing-list-cli segment rm {} --confirm`",
                args.name
            ),
        });
    }
    if !db.segment_delete(&args.name)? {
        return Err(AppError::BadInput {
            code: "segment_not_found".into(),
            message: format!("no segment named '{}'", args.name),
            suggestion: "Run `mailing-list-cli segment ls`".into(),
        });
    }
    output::success(
        format,
        &format!("segment '{}' removed", args.name),
        json!({ "name": args.name, "removed": true }),
    );
    Ok(())
}
