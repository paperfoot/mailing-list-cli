use crate::cli::{FieldAction, FieldCreateArgs, FieldRmArgs};
use crate::db::Db;
use crate::error::AppError;
use crate::output::{self, Format};
use serde_json::json;

pub fn run(format: Format, action: FieldAction) -> Result<(), AppError> {
    let db = Db::open()?;
    match action {
        FieldAction::Create(args) => create(format, &db, args),
        FieldAction::List => list(format, &db),
        FieldAction::Rm(args) => remove(format, &db, args),
    }
}

fn create(format: Format, db: &Db, args: FieldCreateArgs) -> Result<(), AppError> {
    let options_vec: Option<Vec<String>> = args.options.as_ref().map(|s| {
        s.split(',')
            .map(|o| o.trim().to_string())
            .filter(|o| !o.is_empty())
            .collect()
    });
    let id = db.field_create(&args.key, &args.r#type, options_vec.as_deref())?;
    let field = db
        .field_get(&args.key)?
        .expect("field just created must exist");
    output::success(
        format,
        &format!("field created: {} ({})", args.key, args.r#type),
        json!({
            "id": id,
            "field": field
        }),
    );
    Ok(())
}

fn list(format: Format, db: &Db) -> Result<(), AppError> {
    let fields = db.field_all()?;
    let count = fields.len();
    output::success(
        format,
        &format!("{count} field(s)"),
        json!({ "fields": fields, "count": count }),
    );
    Ok(())
}

fn remove(format: Format, db: &Db, args: FieldRmArgs) -> Result<(), AppError> {
    if !args.confirm {
        return Err(AppError::BadInput {
            code: "confirmation_required".into(),
            message: format!("deleting field '{}' requires --confirm", args.key),
            suggestion: format!(
                "rerun with `mailing-list-cli field rm {} --confirm`",
                args.key
            ),
        });
    }
    let removed = db.field_delete(&args.key)?;
    if !removed {
        return Err(AppError::BadInput {
            code: "field_not_found".into(),
            message: format!("no field named '{}'", args.key),
            suggestion: "Run `mailing-list-cli field ls` to see existing fields".into(),
        });
    }
    output::success(
        format,
        &format!("field '{}' removed", args.key),
        json!({ "key": args.key, "removed": true }),
    );
    Ok(())
}
