use crate::cli::{
    TemplateAction, TemplateCreateArgs, TemplateLintArgs, TemplatePreviewArgs, TemplateRenderArgs,
    TemplateRmArgs, TemplateShowArgs,
};
use crate::db::Db;
use crate::error::AppError;
use crate::output::{self, Format};
use crate::paths;
use crate::template::{self, Rendered};
use serde_json::{Value, json};

/// Built-in scaffold: a minimal, inline-styled HTML template that demonstrates
/// every convention the v0.2 template system requires. Agents copy the
/// structure on first use; after that they iterate via `template preview`.
///
/// The scaffold IS the documentation: there's no separate `template
/// guidelines` command anymore. A strong scaffold + a fast preview loop
/// replaces a 153-line embedded guide.
const SCAFFOLD: &str = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>{{ NAME }}</title>
</head>
<body style="margin:0;padding:0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;background:#f4f4f4">
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0">
    <tr>
      <td align="center" style="padding:20px">
        <table role="presentation" width="600" cellpadding="0" cellspacing="0" border="0" style="background:#ffffff;max-width:600px">
          <tr>
            <td style="padding:30px">
              <h1 style="margin:0 0 16px;font-size:24px;line-height:1.3;color:#111">
                Hi {{ first_name }}
              </h1>
              <p style="margin:0 0 16px;font-size:16px;line-height:1.5;color:#333">
                Replace this body with your content. Write normal HTML with
                inline styles for best client compatibility. Use
                <code>{{ merge_tag }}</code> for contact fields and
                <code>{{#if name}}...{{/if}}</code> for conditionals.
              </p>
              <p style="margin:0 0 16px">
                <a href="https://example.com/cta"
                   style="display:inline-block;padding:12px 24px;background:#000;color:#fff;text-decoration:none;border-radius:4px;font-weight:600">
                  Call to action
                </a>
              </p>
              <p style="margin:30px 0 0;padding-top:20px;border-top:1px solid #eaeaea;font-size:12px;color:#666;text-align:center">
                {{{ unsubscribe_link }}}
                <br>
                {{{ physical_address_footer }}}
              </p>
            </td>
          </tr>
        </table>
      </td>
    </tr>
  </table>
</body>
</html>
"##;

pub fn run(format: Format, action: TemplateAction) -> Result<(), AppError> {
    match action {
        TemplateAction::Create(args) => create(format, args),
        TemplateAction::List => list(format),
        TemplateAction::Show(args) => show(format, args),
        TemplateAction::Render(args) => render(format, args),
        TemplateAction::Preview(args) => preview(format, args),
        TemplateAction::Lint(args) => lint_cmd(format, args),
        TemplateAction::Rm(args) => remove(format, args),
    }
}

fn create(format: Format, args: TemplateCreateArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let source = match &args.from_file {
        Some(path) => std::fs::read_to_string(path).map_err(|e| AppError::BadInput {
            code: "template_file_read_failed".into(),
            message: format!("could not read {}: {e}", path.display()),
            suggestion: "Check the file path and permissions".into(),
        })?,
        None => SCAFFOLD.replace("{{ NAME }}", &args.name),
    };
    let subject = args.subject.as_deref().unwrap_or("(no subject set)");
    let id = db.template_upsert(&args.name, subject, &source)?;
    output::success(
        format,
        &format!("template created: {}", args.name),
        json!({
            "id": id,
            "name": args.name,
            "subject": subject,
            "size_bytes": source.len(),
            "scaffolded": args.from_file.is_none()
        }),
    );
    Ok(())
}

fn list(format: Format) -> Result<(), AppError> {
    let db = Db::open()?;
    let templates = db.template_all()?;
    let summary: Vec<_> = templates
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.name,
                "subject": t.subject,
                "size_bytes": t.html_source.len(),
                "updated_at": t.updated_at
            })
        })
        .collect();
    let count = summary.len();
    output::success(
        format,
        &format!("{count} template(s)"),
        json!({ "templates": summary, "count": count }),
    );
    Ok(())
}

fn show(format: Format, args: TemplateShowArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let t = db
        .template_get_by_name(&args.name)?
        .ok_or_else(|| AppError::BadInput {
            code: "template_not_found".into(),
            message: format!("no template named '{}'", args.name),
            suggestion: "Run `mailing-list-cli template ls` to see all templates".into(),
        })?;
    output::success(
        format,
        &format!("template: {}", t.name),
        json!({
            "id": t.id,
            "name": t.name,
            "subject": t.subject,
            "html_source": t.html_source,
            "size_bytes": t.html_source.len(),
            "updated_at": t.updated_at
        }),
    );
    Ok(())
}

fn load_merge_data(path: Option<&std::path::PathBuf>) -> Result<Value, AppError> {
    match path {
        None => Ok(json!({})),
        Some(p) => {
            let text = std::fs::read_to_string(p).map_err(|e| AppError::BadInput {
                code: "data_file_read_failed".into(),
                message: format!("could not read {}: {e}", p.display()),
                suggestion: "Check the file path and permissions".into(),
            })?;
            serde_json::from_str(&text).map_err(|e| AppError::BadInput {
                code: "data_file_invalid_json".into(),
                message: format!("{} is not valid JSON: {e}", p.display()),
                suggestion: "Provide a file containing a single JSON object".into(),
            })
        }
    }
}

fn render(format: Format, args: TemplateRenderArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let t = db
        .template_get_by_name(&args.name)?
        .ok_or_else(|| AppError::BadInput {
            code: "template_not_found".into(),
            message: format!("no template named '{}'", args.name),
            suggestion: "Run `mailing-list-cli template ls`".into(),
        })?;
    let mut data = load_merge_data(args.with_data.as_ref())?;
    if args.with_placeholders {
        // Inject stub values for the two reserved triple-brace names so the
        // preview shows exactly what agents will see in a real send.
        if let Value::Object(map) = &mut data {
            map.entry(String::from("unsubscribe_link")).or_insert(json!(
                "<a href=\"https://hooks.example.invalid/u/PLACEHOLDER_UNSUBSCRIBE_TOKEN\" target=\"_blank\">Unsubscribe</a>"
            ));
            map.entry(String::from("physical_address_footer")).or_insert(json!(
                "<div style=\"color:#666;font-size:11px;text-align:center;margin-top:20px\">Your Company Name · 123 Example Street · City, ST 00000</div>"
            ));
        }
    }
    let rendered = template::render_preview(&t.html_source, &t.subject, &data);
    output::success(
        format,
        &format!("rendered template '{}'", t.name),
        json!({
            "name": t.name,
            "subject": rendered.subject,
            "html": rendered.html,
            "text": rendered.text,
            "size_bytes": rendered.size_bytes,
            "lint_errors": rendered.error_count(),
            "lint_warnings": rendered.warning_count(),
            "unresolved": rendered.unresolved,
            "findings": rendered.findings
        }),
    );
    Ok(())
}

/// `template preview <name>` — the v0.2 iteration primitive. Renders the
/// template to a deterministic file path and optionally opens it in the
/// default browser. This is the command that replaces the need for every
/// "catch a mistake upfront" lint rule the v0.1 system had.
fn preview(format: Format, args: TemplatePreviewArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let t = db
        .template_get_by_name(&args.name)?
        .ok_or_else(|| AppError::BadInput {
            code: "template_not_found".into(),
            message: format!("no template named '{}'", args.name),
            suggestion: "Run `mailing-list-cli template ls`".into(),
        })?;
    let mut data = load_merge_data(args.with_data.as_ref())?;
    // Inject realistic stubs for the two reserved triple-brace names so the
    // preview renders a full, visually accurate output.
    if let Value::Object(map) = &mut data {
        map.entry(String::from("unsubscribe_link")).or_insert(json!(
            "<a href=\"https://hooks.example.invalid/u/PLACEHOLDER_UNSUBSCRIBE_TOKEN\" target=\"_blank\">Unsubscribe</a>"
        ));
        map.entry(String::from("physical_address_footer")).or_insert(json!(
            "<div style=\"color:#666;font-size:11px;text-align:center;margin-top:20px\">Your Company Name · 123 Example Street · City, ST 00000</div>"
        ));
        map.entry(String::from("first_name"))
            .or_insert(json!("Preview"));
        map.entry(String::from("last_name"))
            .or_insert(json!("User"));
        map.entry(String::from("email"))
            .or_insert(json!("preview@example.invalid"));
        map.entry(String::from("current_year"))
            .or_insert(json!(2026));
        map.entry(String::from("broadcast_id")).or_insert(json!(0));
    }

    let rendered = template::render_preview(&t.html_source, &t.subject, &data);

    let out_dir = match args.out_dir {
        Some(p) => p,
        None => paths::cache_dir().join("preview").join(&t.name),
    };
    std::fs::create_dir_all(&out_dir).map_err(|e| AppError::Transient {
        code: "preview_dir_create_failed".into(),
        message: format!("could not create {}: {e}", out_dir.display()),
        suggestion: "Check directory permissions or pass --out-dir".into(),
    })?;

    let html_path = out_dir.join("index.html");
    let text_path = out_dir.join("plain.txt");
    let subject_path = out_dir.join("subject.txt");

    std::fs::write(&html_path, &rendered.html).map_err(|e| AppError::Transient {
        code: "preview_write_failed".into(),
        message: format!("could not write {}: {e}", html_path.display()),
        suggestion: "Check disk space and permissions".into(),
    })?;
    std::fs::write(&text_path, &rendered.text).map_err(|e| AppError::Transient {
        code: "preview_write_failed".into(),
        message: format!("could not write {}: {e}", text_path.display()),
        suggestion: "Check disk space and permissions".into(),
    })?;
    std::fs::write(&subject_path, &rendered.subject).map_err(|e| AppError::Transient {
        code: "preview_write_failed".into(),
        message: format!("could not write {}: {e}", subject_path.display()),
        suggestion: "Check disk space and permissions".into(),
    })?;

    if args.open {
        let _ = open_in_browser(&html_path);
    }

    output::success(
        format,
        &format!(
            "preview written: {} ({} errors, {} warnings)",
            html_path.display(),
            rendered.error_count(),
            rendered.warning_count()
        ),
        json!({
            "name": t.name,
            "html_path": html_path,
            "text_path": text_path,
            "subject_path": subject_path,
            "subject": rendered.subject,
            "size_bytes": rendered.size_bytes,
            "lint_errors": rendered.error_count(),
            "lint_warnings": rendered.warning_count(),
            "unresolved": rendered.unresolved,
            "findings": rendered.findings
        }),
    );
    Ok(())
}

/// Open `path` in the default OS handler. Best-effort; returns an error but
/// the preview command treats it as non-fatal.
fn open_in_browser(path: &std::path::Path) -> Result<(), AppError> {
    let (cmd, arg): (&str, &std::path::Path) = if cfg!(target_os = "macos") {
        ("open", path)
    } else if cfg!(target_os = "windows") {
        ("cmd", path) // needs `/C start`; simpler cross-platform is `start`
    } else {
        ("xdg-open", path)
    };
    let status = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .status()
    } else {
        std::process::Command::new(cmd).arg(arg).status()
    };
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => Err(AppError::Transient {
            code: "open_failed".into(),
            message: format!("could not open {} with the default handler", path.display()),
            suggestion: "Open the file manually from the printed path".into(),
        }),
    }
}

fn lint_cmd(format: Format, args: TemplateLintArgs) -> Result<(), AppError> {
    let db = Db::open()?;
    let t = db
        .template_get_by_name(&args.name)?
        .ok_or_else(|| AppError::BadInput {
            code: "template_not_found".into(),
            message: format!("no template named '{}'", args.name),
            suggestion: "Run `mailing-list-cli template ls`".into(),
        })?;
    let outcome: Rendered = template::lint(&t.html_source, &t.subject);
    if outcome.has_errors() {
        return Err(AppError::BadInput {
            code: "template_lint_errors".into(),
            message: format!(
                "template '{}' has {} lint error(s)",
                t.name,
                outcome.error_count()
            ),
            suggestion: serde_json::to_string(&outcome.findings).unwrap(),
        });
    }
    output::success(
        format,
        &format!("lint passed with {} warning(s)", outcome.warning_count()),
        json!({
            "name": t.name,
            "errors": outcome.error_count(),
            "warnings": outcome.warning_count(),
            "findings": outcome.findings
        }),
    );
    Ok(())
}

fn remove(format: Format, args: TemplateRmArgs) -> Result<(), AppError> {
    if !args.confirm {
        return Err(AppError::BadInput {
            code: "confirmation_required".into(),
            message: format!("deleting template '{}' requires --confirm", args.name),
            suggestion: format!(
                "rerun with `mailing-list-cli template rm {} --confirm`",
                args.name
            ),
        });
    }
    let db = Db::open()?;
    if !db.template_delete(&args.name)? {
        return Err(AppError::BadInput {
            code: "template_not_found".into(),
            message: format!("no template named '{}'", args.name),
            suggestion: "Run `mailing-list-cli template ls`".into(),
        });
    }
    output::success(
        format,
        &format!("template '{}' removed", args.name),
        json!({ "name": args.name, "removed": true }),
    );
    Ok(())
}
