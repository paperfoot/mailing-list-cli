use crate::cli::{ContactAction, ContactAddArgs, ContactListArgs};
use crate::config::Config;
use crate::db::Db;
use crate::email_cli::EmailCli;
use crate::error::AppError;
use crate::output::{self, Format};
use serde_json::json;

pub fn run(format: Format, action: ContactAction) -> Result<(), AppError> {
    let config = Config::load()?;
    let mut db = Db::open()?;
    let cli = EmailCli::new(&config.email_cli.path, &config.email_cli.profile);

    match action {
        ContactAction::Add(args) => add(format, &db, &cli, args),
        ContactAction::List(args) => list_contacts(format, &db, args),
        ContactAction::Tag(args) => tag_contact(format, &db, args),
        ContactAction::Untag(args) => untag_contact(format, &db, args),
        ContactAction::Set(args) => set_field(format, &db, args),
        ContactAction::Show(args) => show_contact(format, &db, args),
        ContactAction::Import(args) => import(format, &mut db, &cli, args),
    }
}

fn import(
    format: Format,
    db: &mut Db,
    cli: &EmailCli,
    args: crate::cli::ContactImportArgs,
) -> Result<(), AppError> {
    // Defer real DOI to Phase 7.
    if args.double_opt_in {
        return Err(AppError::BadInput {
            code: "double_opt_in_not_available".into(),
            message:
                "--double-opt-in requires `optin start`/`verify` which ship in v0.1.3 (Phase 7)"
                    .into(),
            suggestion:
                "Rerun without --double-opt-in; for now imported contacts default to status=active"
                    .into(),
        });
    }

    // Clone the list details up-front so `db` is free to be borrowed
    // mutably inside the per-row apply loop.
    let list = db
        .list_get_by_id(args.list)?
        .ok_or_else(|| AppError::BadInput {
            code: "list_not_found".into(),
            message: format!("no list with id {}", args.list),
            suggestion: "Run `mailing-list-cli list ls`".into(),
        })?;
    let list_id = list.id;
    let list_name = list.name.clone();
    let list_resend_segment_id = list.resend_segment_id.clone();
    drop(list);

    // Read + validate all rows before touching the DB so a malformed file
    // never leaves a half-imported state.
    let file = std::fs::File::open(&args.file).map_err(|e| AppError::BadInput {
        code: "csv_open_failed".into(),
        message: format!("could not open {}: {e}", args.file.display()),
        suggestion: "Check the file path and permissions".into(),
    })?;
    let rows = crate::csv_import::read_rows(file, args.unsafe_no_consent)?;

    let total = rows.len();
    let mut summary = crate::csv_import::ImportSummary {
        total_rows: total,
        ..Default::default()
    };
    let _ = list_name;

    for (idx, row) in rows.iter().enumerate() {
        // Local write first
        match crate::csv_import::apply_row_local(db, list_id, row, args.unsafe_no_consent) {
            Ok(()) => {
                summary.inserted += 1;
                if args.unsafe_no_consent {
                    summary.tagged_without_consent += 1;
                }
            }
            Err(e) if e.code() == "contact_suppressed" => {
                summary.skipped_suppressed += 1;
                continue;
            }
            Err(e) => {
                summary.skipped_invalid += 1;
                summary
                    .errors
                    .push(format!("row {}: {}", idx + 2, e.message()));
                continue;
            }
        }

        // Mirror to Resend (rate-limited at the subprocess layer).
        // Failures don't abort the import — they roll up into the summary.
        if let Err(e) = cli.contact_create(
            &row.email,
            row.first_name.as_deref(),
            row.last_name.as_deref(),
            &[list_resend_segment_id.as_str()],
            None,
        ) {
            summary
                .errors
                .push(format!("row {}: {} (resend mirror)", idx + 2, e.message()));
        }

        // Progress to stderr every 100 rows
        if (idx + 1) % 100 == 0 {
            eprintln!("imported {}/{}", idx + 1, total);
        }
    }

    output::success(
        format,
        &format!(
            "imported {} of {} rows ({} suppressed, {} errors)",
            summary.inserted,
            summary.total_rows,
            summary.skipped_suppressed,
            summary.skipped_invalid
        ),
        json!({
            "total_rows": summary.total_rows,
            "inserted": summary.inserted,
            "skipped_suppressed": summary.skipped_suppressed,
            "skipped_invalid": summary.skipped_invalid,
            "tagged_without_consent": summary.tagged_without_consent,
            "errors": summary.errors
        }),
    );
    Ok(())
}

fn add(format: Format, db: &Db, cli: &EmailCli, args: ContactAddArgs) -> Result<(), AppError> {
    if !is_valid_email(&args.email) {
        return Err(AppError::BadInput {
            code: "invalid_email".into(),
            message: format!("'{}' is not a valid email address", args.email),
            suggestion: "Provide an email in the form local@domain".into(),
        });
    }

    // Parse --field key=val pairs into a validated (field, typed_value) list
    // BEFORE doing any DB writes. Fail fast on any coercion error.
    let mut typed_fields: Vec<(crate::models::Field, crate::db::TypedFieldValue)> = Vec::new();
    for pair in &args.fields {
        let (k, v) = pair.split_once('=').ok_or_else(|| AppError::BadInput {
            code: "invalid_field_arg".into(),
            message: format!("--field '{pair}' is not in key=val form"),
            suggestion: "Use `--field company=Acme`".into(),
        })?;
        let field = db.field_get(k)?.ok_or_else(|| AppError::BadInput {
            code: "field_not_found".into(),
            message: format!("no field named '{k}'"),
            suggestion: format!(
                "Create the field first with `mailing-list-cli field create {k} --type text`"
            ),
        })?;
        let typed = db.coerce_field_value(&field, v)?;
        typed_fields.push((field, typed));
    }

    let list = db
        .list_get_by_id(args.list)?
        .ok_or_else(|| AppError::BadInput {
            code: "list_not_found".into(),
            message: format!("no list with id {}", args.list),
            suggestion: "Run `mailing-list-cli list ls` to see all lists".into(),
        })?;

    // 1. Insert/upsert into local contact table
    let contact_id = db.contact_upsert(
        &args.email,
        args.first_name.as_deref(),
        args.last_name.as_deref(),
    )?;

    // 2. Write custom field values
    for (field, typed) in &typed_fields {
        db.contact_field_upsert(contact_id, field.id, typed)?;
    }

    // 3. Mirror to the Resend contact store (flat /contacts) and add to the
    //    list's backing segment in one call via --segments.
    cli.contact_create(
        &args.email,
        args.first_name.as_deref(),
        args.last_name.as_deref(),
        &[list.resend_segment_id.as_str()],
        None,
    )?;

    // 4. Wire up the local list_membership row
    db.contact_add_to_list(contact_id, list.id)?;

    output::success(
        format,
        &format!("added {} to list '{}'", args.email, list.name),
        json!({
            "contact_id": contact_id,
            "email": args.email,
            "list_id": list.id,
            "list_name": list.name,
            "fields_set": typed_fields.len()
        }),
    );
    Ok(())
}

fn set_field(format: Format, db: &Db, args: crate::cli::ContactSetArgs) -> Result<(), AppError> {
    let contact_id = contact_id_or_fail(db, &args.email)?;
    let field =
        db.field_get(&args.field)?
            .ok_or_else(|| AppError::BadInput {
                code: "field_not_found".into(),
                message: format!("no field named '{}'", args.field),
                suggestion:
                    "Run `mailing-list-cli field ls`; create the field with `field create` first"
                        .into(),
            })?;
    let typed = db.coerce_field_value(&field, &args.value)?;
    db.contact_field_upsert(contact_id, field.id, &typed)?;
    output::success(
        format,
        &format!("{}.{} = {}", args.email, args.field, args.value),
        json!({
            "email": args.email,
            "field": args.field,
            "value": args.value
        }),
    );
    Ok(())
}

fn show_contact(
    format: Format,
    db: &Db,
    args: crate::cli::ContactShowArgs,
) -> Result<(), AppError> {
    let contact = db
        .contact_get_by_email(&args.email)?
        .ok_or_else(|| AppError::BadInput {
            code: "contact_not_found".into(),
            message: format!("no contact with email '{}'", args.email),
            suggestion: "Run `mailing-list-cli contact ls --list <id>` to browse contacts".into(),
        })?;
    let tags = db.contact_tags_for(contact.id)?;
    let fields = db.contact_fields_for(contact.id)?;
    let lists = db.contact_lists_for(contact.id)?;
    let consent = db.contact_consent_for_email(&contact.email)?;
    let fields_json: serde_json::Map<String, serde_json::Value> = fields
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    let lists_json: Vec<serde_json::Value> = lists
        .into_iter()
        .map(|(id, name)| json!({ "id": id, "name": name }))
        .collect();
    let consent_json = consent
        .map(|c| json!({ "source": c.source, "at": c.at }))
        .unwrap_or_else(|| json!(null));
    output::success(
        format,
        &format!("contact: {}", contact.email),
        json!({
            "contact": contact,
            "tags": tags,
            "fields": fields_json,
            "lists": lists_json,
            "consent": consent_json
        }),
    );
    Ok(())
}

fn list_contacts(format: Format, db: &Db, args: ContactListArgs) -> Result<(), AppError> {
    let limit = args.limit.min(10_000);

    // Build the base SQL fragment from filter + optional list restriction.
    // If both are present, AND them; if neither, match all contacts.
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(expr_text) = &args.filter {
        let parsed = crate::segment::parser::parse(expr_text).map_err(|e| AppError::BadInput {
            code: "invalid_filter_expression".into(),
            message: e.message.clone(),
            suggestion: e.suggestion.clone(),
        })?;
        let field_types = resolve_field_types(db, &parsed)?;
        let (frag, filter_params) =
            crate::segment::compiler::to_sql_where_with_field_types(&parsed, &field_types);
        clauses.push(format!("({frag})"));
        params.extend(filter_params);
    }

    if let Some(list_id) = args.list {
        // Verify the list exists first so agents get a clear error instead of "0 results".
        let list = db
            .list_get_by_id(list_id)?
            .ok_or_else(|| AppError::BadInput {
                code: "list_not_found".into(),
                message: format!("no list with id {list_id}"),
                suggestion: "Run `mailing-list-cli list ls` to see all lists".into(),
            })?;
        clauses.push(
            "c.id IN (SELECT lm.contact_id FROM list_membership lm WHERE lm.list_id = ?)"
                .to_string(),
        );
        params.push(rusqlite::types::Value::Integer(list.id));
    }

    let fragment = if clauses.is_empty() {
        "1 = 1".to_string()
    } else {
        clauses.join(" AND ")
    };

    let contacts = db.segment_members(&fragment, &params, limit, args.cursor)?;
    let count = contacts.len();
    let next_cursor = contacts.last().map(|c| c.id);
    output::success(
        format,
        &format!("{count} contact(s)"),
        json!({
            "contacts": contacts,
            "count": count,
            "next_cursor": next_cursor,
            "limit": limit
        }),
    );
    Ok(())
}

/// Pre-resolve the declared type of every custom field referenced in a
/// parsed filter expression. The resulting map lets the segment compiler
/// pick the right `contact_field_value` column (text/number/date/bool)
/// instead of sniffing the string literal.
fn resolve_field_types(
    db: &Db,
    expr: &crate::segment::SegmentExpr,
) -> Result<std::collections::HashMap<String, String>, AppError> {
    let mut map = std::collections::HashMap::new();
    for key in crate::segment::collect_field_keys(expr) {
        if let Some(ty) = db.field_get_type(&key)? {
            map.insert(key, ty);
        }
    }
    Ok(map)
}

/// Minimal email validity check: contains exactly one '@', non-empty parts, no whitespace.
fn is_valid_email(s: &str) -> bool {
    let parts: Vec<&str> = s.split('@').collect();
    parts.len() == 2
        && !parts[0].is_empty()
        && !parts[1].is_empty()
        && !s.contains(' ')
        && parts[1].contains('.')
}

fn tag_contact(format: Format, db: &Db, args: crate::cli::ContactTagArgs) -> Result<(), AppError> {
    let contact_id = contact_id_or_fail(db, &args.email)?;
    let tag_id = db.tag_get_or_create(&args.tag)?;
    db.contact_tag_add(contact_id, tag_id)?;
    output::success(
        format,
        &format!("tagged {} with '{}'", args.email, args.tag),
        json!({
            "email": args.email,
            "tag": args.tag,
            "contact_id": contact_id
        }),
    );
    Ok(())
}

fn untag_contact(
    format: Format,
    db: &Db,
    args: crate::cli::ContactTagArgs,
) -> Result<(), AppError> {
    let contact_id = contact_id_or_fail(db, &args.email)?;
    let tag_id = match db.tag_find(&args.tag)? {
        Some(id) => id,
        None => {
            return Err(AppError::BadInput {
                code: "tag_not_found".into(),
                message: format!("no tag named '{}'", args.tag),
                suggestion: "Run `mailing-list-cli tag ls` to see all tags".into(),
            });
        }
    };
    let removed = db.contact_tag_remove(contact_id, tag_id)?;
    output::success(
        format,
        &format!(
            "{} tag '{}' from {}",
            if removed { "removed" } else { "no-op;" },
            args.tag,
            args.email
        ),
        json!({
            "email": args.email,
            "tag": args.tag,
            "removed": removed
        }),
    );
    Ok(())
}

/// Look up a contact by email, return exit 3 if missing.
fn contact_id_or_fail(db: &Db, email: &str) -> Result<i64, AppError> {
    db.contact_find_id(email)?
        .ok_or_else(|| AppError::BadInput {
            code: "contact_not_found".into(),
            message: format!("no contact with email '{email}'"),
            suggestion: "Run `mailing-list-cli contact ls --list <id>` to find existing contacts"
                .into(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_validation_basics() {
        assert!(is_valid_email("alice@example.com"));
        assert!(is_valid_email("a+b@sub.example.co"));
        assert!(!is_valid_email("alice@"));
        assert!(!is_valid_email("@example.com"));
        assert!(!is_valid_email("alice"));
        assert!(!is_valid_email("alice@example"));
        assert!(!is_valid_email("alice @example.com"));
        assert!(!is_valid_email(""));
    }
}
