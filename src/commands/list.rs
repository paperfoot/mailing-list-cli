use crate::cli::{ListAction, ListCreateArgs, ListShowArgs};
use crate::config::Config;
use crate::db::Db;
use crate::email_cli::EmailCli;
use crate::error::AppError;
use crate::output::{self, Format};
use serde_json::json;

pub fn run(format: Format, action: ListAction) -> Result<(), AppError> {
    let config = Config::load()?;
    let db = Db::open()?;
    let cli = EmailCli::new(&config.email_cli.path, &config.email_cli.profile);

    match action {
        ListAction::Create(args) => create(format, &db, &cli, args),
        ListAction::List => list_all(format, &db),
        ListAction::Show(args) => show(format, &db, args),
    }
}

fn create(format: Format, db: &Db, cli: &EmailCli, args: ListCreateArgs) -> Result<(), AppError> {
    // 1. Local pre-check: refuse early if the name is already taken so we don't
    //    needlessly create an upstream Resend audience that we then can't use.
    if db.list_get_by_name(&args.name)?.is_some() {
        return Err(AppError::BadInput {
            code: "list_already_exists".into(),
            message: format!("a list named '{}' already exists", args.name),
            suggestion:
                "Use `mailing-list-cli list ls` to see existing lists, or pick a different name"
                    .into(),
        });
    }

    // 2. Create the audience on Resend (via email-cli)
    let audience_id = cli.audience_create(&args.name)?;

    // 3. Insert the local row
    let id = db.list_create(&args.name, args.description.as_deref(), &audience_id)?;
    let list = db
        .list_get_by_id(id)?
        .expect("list just created must exist");

    output::success(format, &format!("list created: {}", list.name), list);
    Ok(())
}

fn list_all(format: Format, db: &Db) -> Result<(), AppError> {
    let lists = db.list_all()?;
    let count = lists.len();
    output::success(
        format,
        &format!("{count} list(s)"),
        json!({ "lists": lists, "count": count }),
    );
    Ok(())
}

fn show(format: Format, db: &Db, args: ListShowArgs) -> Result<(), AppError> {
    match db.list_get_by_id(args.id)? {
        Some(list) => {
            output::success(format, &format!("list: {}", list.name), list);
            Ok(())
        }
        None => Err(AppError::BadInput {
            code: "list_not_found".into(),
            message: format!("no list with id {}", args.id),
            suggestion: "Run `mailing-list-cli list ls` to see all lists".into(),
        }),
    }
}
