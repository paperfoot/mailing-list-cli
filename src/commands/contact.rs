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

    // 2. Mirror to the Resend audience via email-cli
    cli.contact_create(
        &list.resend_audience_id,
        &args.email,
        args.first_name.as_deref(),
        args.last_name.as_deref(),
    )?;

    // 3. Wire up the local list_membership row
    db.contact_add_to_list(contact_id, list.id)?;

    output::success(
        format,
        &format!("added {} to list '{}'", args.email, list.name),
        json!({
            "contact_id": contact_id,
            "email": args.email,
            "list_id": list.id,
            "list_name": list.name
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
