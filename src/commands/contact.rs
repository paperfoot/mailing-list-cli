use crate::cli::{ContactAction, ContactAddArgs, ContactListArgs};
use crate::config::Config;
use crate::db::Db;
use crate::email_cli::EmailCli;
use crate::error::AppError;
use crate::output::{self, Format};
use serde_json::json;

pub fn run(format: Format, action: ContactAction) -> Result<(), AppError> {
    let config = Config::load()?;
    let db = Db::open()?;
    let cli = EmailCli::new(&config.email_cli.path, &config.email_cli.profile);

    match action {
        ContactAction::Add(args) => add(format, &db, &cli, args),
        ContactAction::List(args) => list_contacts(format, &db, args),
        ContactAction::Tag(args) => tag_contact(format, &db, args),
        ContactAction::Untag(args) => untag_contact(format, &db, args),
        ContactAction::Set(args) => set_field(format, &db, args),
        ContactAction::Show(args) => show_contact(format, &db, args),
    }
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
    let fields_json: serde_json::Map<String, serde_json::Value> = fields
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    let lists_json: Vec<serde_json::Value> = lists
        .into_iter()
        .map(|(id, name)| json!({ "id": id, "name": name }))
        .collect();
    output::success(
        format,
        &format!("contact: {}", contact.email),
        json!({
            "contact": contact,
            "tags": tags,
            "fields": fields_json,
            "lists": lists_json
        }),
    );
    Ok(())
}

fn list_contacts(format: Format, db: &Db, args: ContactListArgs) -> Result<(), AppError> {
    let list = db
        .list_get_by_id(args.list)?
        .ok_or_else(|| AppError::BadInput {
            code: "list_not_found".into(),
            message: format!("no list with id {}", args.list),
            suggestion: "Run `mailing-list-cli list ls` to see all lists".into(),
        })?;

    let contacts = db.contact_list_in_list(list.id, args.limit)?;
    let count = contacts.len();
    output::success(
        format,
        &format!("{} contact(s) in list '{}'", count, list.name),
        json!({
            "list_id": list.id,
            "list_name": list.name,
            "contacts": contacts,
            "count": count
        }),
    );
    Ok(())
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
